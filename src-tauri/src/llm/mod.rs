// Rust port of backend/llm_engine.py. Persona/provider dispatch use `match` on enums
// (see persona.rs) instead of the dict lookups and if/elif chains in the Python; chat
// history and memories are Vec<Struct> (see db.rs) instead of List[Dict]. Shares the same
// SQLite file and schema as the Python backend, so either engine can serve a request
// against the same conversation/settings state.

mod db;
mod genesis;
mod persona;
mod providers;
mod telemetry;

use std::sync::Mutex;
use std::time::Instant;

pub use db::MemoryDb;
pub use genesis::Identity;
use persona::{Persona, Provider};
use providers::ChatContext;
use telemetry::Telemetry;

#[derive(Default)]
struct UsageStats {
    session_prompt_tokens: u64,
    session_completion_tokens: u64,
    total_requests: u64,
    last_tps: f64,
}

fn estimate_tokens(text: &str) -> u64 {
    if text.is_empty() {
        0
    } else {
        // Same ~4-chars-per-token heuristic as token_tracker.py's estimate_tokens.
        (text.chars().count() as u64).div_ceil(4).max(1)
    }
}

impl UsageStats {
    fn record_usage(&mut self, prompt: &str, completion: &str, duration_secs: f64) {
        let prompt_tok = estimate_tokens(prompt);
        let completion_tok = estimate_tokens(completion);
        self.session_prompt_tokens += prompt_tok;
        self.session_completion_tokens += completion_tok;
        self.total_requests += 1;
        self.last_tps = (completion_tok as f64 / duration_secs.max(0.05) * 10.0).round() / 10.0;
    }
}

struct Config {
    agent_name: String,
    provider: Provider,
    model_name: String,
    api_key: String,
    endpoint: String,
    persona: Persona,
    persona_key: String,
    custom_directive: String,
}

/// Scans the environment for a usable cloud API key, same set/priority as
/// model_scanner.py's detect_cloud_api_keys. Anthropic is deliberately not part of the
/// auto-select priority here, matching the Python (which scans ANTHROPIC_API_KEY for
/// display purposes but never routes to it automatically).
fn detect_cloud_api_key() -> Option<(String, &'static str)> {
    if let Ok(key) = std::env::var("GEMINI_API_KEY").or_else(|_| std::env::var("GOOGLE_API_KEY")) {
        return Some((key, "gemini"));
    }
    if let Ok(key) = std::env::var("GROQ_API_KEY") {
        return Some((key, "groq"));
    }
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        return Some((key, "openai"));
    }
    None
}

pub struct LlmEngine {
    db: MemoryDb,
    usage: Mutex<UsageStats>,
}

impl LlmEngine {
    pub fn new(db: MemoryDb) -> LlmEngine {
        LlmEngine {
            db,
            usage: Mutex::new(UsageStats::default()),
        }
    }

    fn load_config(&self) -> Config {
        let mut agent_name = self.db.get_setting_string("agent_name", "HALCY");
        let mut provider_key = self.db.get_setting_string("llm_provider", "offline");
        let model_name = self.db.get_setting_string("llm_model", "halcy-core");
        let mut api_key = self.db.get_setting_string("llm_api_key", "");
        let endpoint = self
            .db
            .get_setting_string("llm_endpoint", "http://localhost:11434");
        let persona_key = self.db.get_setting_string("persona_type", "halcy");
        let custom_directive = self.db.get_setting_string("custom_directive", "");

        if agent_name == "HALCY" {
            agent_name = match persona_key.as_str() {
                "arx-limes" => "A.R.X.LIMES".to_string(),
                "arx-logos" => "A.R.X.LOGOS".to_string(),
                "nexus" => "THE NEXUS".to_string(),
                "red9000" | "red" => "R.E.D. 9000".to_string(),
                _ => agent_name,
            };
        }

        if api_key.is_empty() {
            if let Some((key, detected_provider)) = detect_cloud_api_key() {
                api_key = key;
                if provider_key == "offline" {
                    provider_key = detected_provider.to_string();
                }
            }
        }

        Config {
            agent_name,
            provider: Provider::from_key(&provider_key),
            model_name,
            api_key,
            endpoint,
            persona: Persona::from_key(&persona_key),
            persona_key,
            custom_directive,
        }
    }

    fn system_prompt(&self, config: &Config, telem: &Telemetry) -> String {
        let base_persona = if config.persona_key == "custom" {
            if config.custom_directive.is_empty() {
                Persona::Halcy.template(&config.agent_name)
            } else {
                config
                    .custom_directive
                    .replace("{AGENT_NAME}", &config.agent_name)
            }
        } else {
            config.persona.template(&config.agent_name)
        };

        let memories = self.db.get_all_memories().unwrap_or_default();
        let memory_context = if memories.is_empty() {
            String::new()
        } else {
            let lines: Vec<String> = memories
                .iter()
                .take(10)
                .map(|m| format!("- {}: {}", m.key, m.value))
                .collect();
            format!("\n[RECALLED KNOWLEDGE STORE]:\n{}", lines.join("\n"))
        };

        format!(
            "{base_persona}\n\n\
             [LIVE HOST TELEMETRY]\n\
             - Identity: {agent_name}\n\
             - OS: {os_name} ({architecture})\n\
             - CPU Load: {cpu_percent:.1}% | RAM: {ram_used:.2}GB / {ram_total:.2}GB ({ram_percent:.1}%)\n\
             - Uptime: {uptime}\n\
             - System Health: {status}\n\
             {memory_context}\n\n\
             Instructions:\n\
             1. Refer to live telemetry if asked about the system or device health.\n\
             2. Persona is light flavor, not a requirement -- always prioritize a clear, accurate, directly \
             useful answer over staying in character.\n\
             3. Refer to yourself as {agent_name}.",
            agent_name = config.agent_name,
            os_name = telem.os_name,
            architecture = telem.architecture,
            cpu_percent = telem.cpu_percent,
            ram_used = telem.ram_used_gb,
            ram_total = telem.ram_total_gb,
            ram_percent = telem.ram_percent,
            uptime = telem.uptime,
            status = telem.status,
        )
    }

    /// Prefix match that's case-insensitive on the *prefix* but preserves whatever case
    /// the operator typed in the rest of the command -- fixes a bug in the Python original,
    /// where `prompt.replace("set name ", "")` silently no-ops unless the user typed that
    /// exact lowercase prefix (the startswith check that gates it is case-insensitive, the
    /// extraction wasn't). Both trimmed to ASCII-safe byte offsets since every prefix here
    /// is a plain ASCII literal.
    fn strip_ci_prefix<'a>(trimmed: &'a str, lowered: &str, prefix: &str) -> Option<&'a str> {
        if lowered.starts_with(prefix) {
            Some(&trimmed[prefix.len()..])
        } else {
            None
        }
    }

    fn check_instant_commands(&self, prompt: &str, config: &Config) -> Option<String> {
        let trimmed = prompt.trim();
        let lowered = trimmed.to_lowercase();

        match lowered.as_str() {
            "status" | "system status" | "telemetry" | "diagnostics" | "health check" | "specs" => {
                return Some(Telemetry::snapshot().diagnostic_report());
            }
            "who are you" | "who are you?" | "identify" | "identify yourself" => {
                return Some(config.persona.who_are_you(&config.agent_name));
            }
            "list memory" | "show memories" | "recall memories" => {
                let mems = self.db.get_all_memories().unwrap_or_default();
                return Some(if mems.is_empty() {
                    "Neural memory banks are currently clear.".to_string()
                } else {
                    let lines: Vec<String> = mems
                        .iter()
                        .map(|m| format!("- **{}**: {}", m.key, m.value))
                        .collect();
                    format!(
                        "### \u{1f9e0} Active Knowledge Store:\n{}",
                        lines.join("\n")
                    )
                });
            }
            _ => {}
        }

        if let Some(rest) = Self::strip_ci_prefix(trimmed, &lowered, "set name ")
            .or_else(|| Self::strip_ci_prefix(trimmed, &lowered, "change name to "))
        {
            let new_name = rest.trim();
            if !new_name.is_empty() {
                let _ = self.db.set_setting(
                    "agent_name",
                    &serde_json::Value::String(new_name.to_string()),
                );
                return Some(format!(
                    "Identifier recalibrated. I am now **{new_name}**. Standing by."
                ));
            }
        } else if let Some(rest) = Self::strip_ci_prefix(trimmed, &lowered, "remember that ")
            .or_else(|| Self::strip_ci_prefix(trimmed, &lowered, "save memory "))
        {
            let fact = rest.trim();
            if let Some((k, v)) = fact.split_once(':') {
                let (k, v) = (k.trim(), v.trim());
                let _ = self.db.set_memory(k, v, "general");
                return Some(format!("Data synthesized into memory: **{k}** = `{v}`"));
            } else if !fact.is_empty() {
                let count = self.db.get_all_memories().map(|m| m.len()).unwrap_or(0);
                let _ = self
                    .db
                    .set_memory(&format!("fact_{}", count + 1), fact, "general");
                return Some(format!("Archived to neural memory: \"{fact}\""));
            }
        }

        None
    }

    pub fn generate_response(&self, prompt: &str, session_id: &str) -> String {
        let config = self.load_config();
        let start = Instant::now();

        if let Some(reply) = self.check_instant_commands(prompt, &config) {
            self.usage
                .lock()
                .unwrap()
                .record_usage(prompt, &reply, start.elapsed().as_secs_f64());
            return reply;
        }

        let history = self.db.get_messages(session_id, 8).unwrap_or_default();
        let telem = Telemetry::snapshot();
        let system_prompt = self.system_prompt(&config, &telem);
        let ctx = ChatContext {
            system_prompt: &system_prompt,
            history: &history,
            prompt,
            agent_name: &config.agent_name,
        };

        let result = match config.provider {
            Provider::Ollama => providers::call_ollama(&config.endpoint, &config.model_name, &ctx),
            Provider::LmStudio | Provider::OpenAi | Provider::Groq => {
                providers::call_openai_compatible(
                    config.provider,
                    &config.endpoint,
                    &config.api_key,
                    &config.model_name,
                    &ctx,
                )
            }
            Provider::Gemini => providers::call_gemini(&config.api_key, &config.model_name, &ctx),
            Provider::Anthropic => {
                providers::call_anthropic(&config.api_key, &config.model_name, &ctx)
            }
            Provider::Offline => Ok(config.persona.offline_reply(
                prompt,
                &telem.os_name,
                telem.cpu_percent,
                &config.agent_name,
            )),
        };

        let reply = result.unwrap_or_else(|e| {
            eprintln!("[AETHER1] LLM Engine Error: provider {} error: {e}", config.provider);
            let fallback = config.persona.offline_reply(prompt, &telem.os_name, telem.cpu_percent, &config.agent_name);
            format!(
                "[HUD Alert: Neural link to {} timed out. Engaging localized cognitive fallback]\n\n{fallback}",
                config.provider
            )
        });

        self.usage
            .lock()
            .unwrap()
            .record_usage(prompt, &reply, start.elapsed().as_secs_f64());
        reply
    }

    pub fn generate_identity_from_purpose(&self, purpose: &str) -> Identity {
        let identity = genesis::generate_identity(purpose);

        let _ = self.db.set_setting(
            "agent_name",
            &serde_json::Value::String(identity.name.clone()),
        );
        let _ = self.db.set_setting(
            "persona_type",
            &serde_json::Value::String(identity.persona_type.clone()),
        );
        let _ = self.db.set_setting(
            "custom_directive",
            &serde_json::Value::String(identity.persona_directive.clone()),
        );
        let _ = self.db.set_setting(
            "voice_name",
            &serde_json::Value::String(identity.voice.clone()),
        );

        identity
    }

    pub fn add_message(&self, session_id: &str, sender: &str, text: &str) {
        let _ = self.db.add_message(session_id, sender, text);
    }

    pub fn agent_name(&self) -> String {
        self.load_config().agent_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_engine(name: &str) -> LlmEngine {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "aether1_engine_test_{name}_{}_{n}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        LlmEngine::new(MemoryDb::open(path).expect("temp db should open"))
    }

    #[test]
    fn usage_stats_estimate_and_accumulate() {
        let mut stats = UsageStats::default();
        stats.record_usage("hello world", "hi there friend", 1.0);
        assert!(stats.session_prompt_tokens > 0);
        assert!(stats.session_completion_tokens > 0);
        assert_eq!(stats.total_requests, 1);
        assert!(stats.last_tps > 0.0);
    }

    #[test]
    fn default_config_is_offline_halcy() {
        let engine = temp_engine("default_config");
        let config = engine.load_config();
        assert_eq!(config.provider, Provider::Offline);
        assert_eq!(config.persona, Persona::Halcy);
        assert_eq!(config.agent_name, "HALCY");
    }

    #[test]
    fn generate_response_offline_default_mentions_standby() {
        let engine = temp_engine("offline_default");
        let reply = engine.generate_response("hello there", "test-session");
        assert!(reply.contains("Offline Standby Mode"), "reply was: {reply}");
    }

    #[test]
    fn who_are_you_instant_command_short_circuits_network() {
        let engine = temp_engine("who_are_you");
        let reply = engine.generate_response("who are you?", "test-session");
        assert!(reply.contains("HALCY"), "reply was: {reply}");
    }

    #[test]
    fn remember_that_persists_and_list_memory_reads_it_back() {
        let engine = temp_engine("remember");
        let ack = engine.generate_response("remember that favorite_color: blue", "test-session");
        assert!(ack.contains("favorite_color"), "ack was: {ack}");

        let listing = engine.generate_response("list memory", "test-session");
        assert!(listing.contains("favorite_color"), "listing was: {listing}");
        assert!(listing.contains("blue"), "listing was: {listing}");
    }

    #[test]
    fn set_name_updates_agent_name_case_preserving() {
        let engine = temp_engine("set_name");
        let reply = engine.generate_response("Set Name Aria", "test-session");
        assert!(reply.contains("Aria"), "reply was: {reply}");
        assert_eq!(engine.agent_name(), "Aria");
    }

    #[test]
    fn generate_identity_from_purpose_persists_settings() {
        let engine = temp_engine("genesis");
        let identity = engine.generate_identity_from_purpose("I want a reactive engine daemon");
        assert_eq!(identity.name, "R.E.D. 9000");

        // Persisted to settings, so a fresh config load reflects the new identity.
        let config = engine.load_config();
        assert_eq!(config.agent_name, "R.E.D. 9000");
        assert_eq!(config.persona, Persona::Red9000);
    }

    /// Real end-to-end test against a locally running Ollama server -- not a mock. Skips
    /// itself (rather than failing the suite) if Ollama isn't reachable, since CI/other
    /// dev machines won't have it running; run manually with `cargo test -- --ignored` is
    /// NOT needed since this checks liveness itself instead of using #[ignore].
    #[test]
    fn generate_response_against_live_ollama_if_available() {
        let ollama_up = ureq::get("http://localhost:11434/api/tags")
            .config()
            .timeout_global(Some(std::time::Duration::from_millis(500)))
            .build()
            .call()
            .is_ok();
        if !ollama_up {
            eprintln!(
                "skipping generate_response_against_live_ollama_if_available: no Ollama on :11434"
            );
            return;
        }

        let engine = temp_engine("live_ollama");
        let _ = engine.db.set_setting(
            "llm_provider",
            &serde_json::Value::String("ollama".to_string()),
        );
        let _ = engine.db.set_setting(
            "llm_model",
            &serde_json::Value::String("llama3.2:1b".to_string()),
        );
        let _ = engine.db.set_setting(
            "llm_endpoint",
            &serde_json::Value::String("http://localhost:11434".to_string()),
        );

        let reply = engine.generate_response(
            "Reply with only the single word PONG, nothing else, no punctuation.",
            "live-test-session",
        );

        assert!(!reply.is_empty());
        assert!(
            !reply.starts_with("[HUD Alert"),
            "generate_response fell back to the offline/error path instead of calling Ollama: {reply}"
        );
        println!("live Ollama reply: {reply:?}");
    }
}
