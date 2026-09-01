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
mod tts;

use std::sync::Mutex;
use std::time::Instant;

pub use db::{ActionRecord, MemoryDb, Message};
pub use genesis::Identity;
use persona::{Persona, Provider};
use providers::ChatContext;
pub use providers::Sink;
pub use telemetry::Telemetry;
pub use tts::{generate_speech, DEFAULT_VOICE};

const SESSION_TOKEN_BUDGET: u64 = 100_000; // matches token_tracker.py's daily_budget_tokens
const SPARKLINE_LEN: usize = 15; // matches token_tracker.py's history[-15:]

#[derive(Default)]
struct UsageStats {
    session_prompt_tokens: u64,
    session_completion_tokens: u64,
    total_requests: u64,
    last_tps: f64,
    /// Per-request total token counts, most recent last, capped at SPARKLINE_LEN --
    /// mirrors token_tracker.py's history-derived sparkline.
    sparkline: Vec<u64>,
}

/// Serializable snapshot of UsageStats for the frontend's token telemetry panel. Field
/// names match token_tracker.py's get_telemetry() so the same JS (updateTokenTelemetry)
/// handles both the Python websocket and this Tauri event without a separate code path.
#[derive(serde::Serialize)]
pub struct UsageSnapshot {
    pub last_tps: f64,
    pub total_session_tokens: u64,
    pub used_percent: f64,
    pub available_tokens: u64,
    pub sparkline: Vec<u64>,
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

        self.sparkline.push(prompt_tok + completion_tok);
        if self.sparkline.len() > SPARKLINE_LEN {
            self.sparkline.remove(0);
        }
    }

    fn snapshot(&self) -> UsageSnapshot {
        let total_session = self.session_prompt_tokens + self.session_completion_tokens;
        let used_percent =
            ((total_session as f64 / SESSION_TOKEN_BUDGET.max(1) as f64) * 100.0).min(100.0);
        let used_percent = (used_percent * 10.0).round() / 10.0;
        let available_tokens = SESSION_TOKEN_BUDGET.saturating_sub(total_session);

        UsageSnapshot {
            last_tps: self.last_tps,
            total_session_tokens: total_session,
            used_percent,
            available_tokens,
            sparkline: if self.sparkline.is_empty() {
                vec![0]
            } else {
                self.sparkline.clone()
            },
        }
    }
}

/// A provider call that could not be completed. `partial` is whatever text had already
/// been streamed to the operator before it failed -- empty when nothing had.
struct StreamFailure {
    message: String,
    partial: String,
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

/// Scans the environment for a usable cloud API key via crate::model_scanner (the same
/// scan the Settings UI's "Scan System" button will eventually trigger), so this logic
/// lives in exactly one place instead of being duplicated between the two modules.
fn detect_cloud_api_key() -> Option<(String, &'static str)> {
    let detected = crate::model_scanner::detect_cloud_keys();
    match detected.detected_provider.as_str() {
        "gemini" => Some((detected.detected_key, "gemini")),
        "groq" => Some((detected.detected_key, "groq")),
        "openai" => Some((detected.detected_key, "openai")),
        _ => None,
    }
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
                // A key derived from the live memory count (fact_{count+1}) collides with
                // an existing key once anything's ever been deleted (set_memory is an
                // upsert, so that silently overwrites the wrong entry instead of adding a
                // new one). A millisecond timestamp can't repeat across two separate chat
                // commands, so it can't collide with an earlier fact_* key.
                let millis = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                let _ = self
                    .db
                    .set_memory(&format!("fact_{millis}"), fact, "general");
                return Some(format!("Archived to neural memory: \"{fact}\""));
            }
        }

        None
    }

    /// Runs the configured provider, feeding reply text to `sink` as it arrives.
    ///
    /// Streaming is tried first for every provider. If the stream fails before any text
    /// has reached the operator, the blocking call is retried once -- that's what keeps a
    /// provider or model that can't stream working, and it costs nothing in the normal
    /// case. Once text *has* been streamed, a failure can't be retried: the operator is
    /// already reading a half-finished answer, and starting a second one over the top of
    /// it would be worse than saying the link dropped.
    fn call_provider(
        &self,
        config: &Config,
        ctx: &ChatContext,
        sink: providers::Sink,
    ) -> Result<String, StreamFailure> {
        let mut partial = String::new();

        let streamed = {
            let mut collect = |delta: &str| {
                partial.push_str(delta);
                sink(delta);
            };
            match config.provider {
                Provider::Ollama => providers::stream_ollama(
                    &config.endpoint,
                    &config.model_name,
                    ctx,
                    &mut collect,
                ),
                Provider::LmStudio | Provider::OpenAi | Provider::Groq => {
                    providers::stream_openai_compatible(
                        config.provider,
                        &config.endpoint,
                        &config.api_key,
                        &config.model_name,
                        ctx,
                        &mut collect,
                    )
                }
                Provider::Gemini => {
                    providers::stream_gemini(&config.api_key, &config.model_name, ctx, &mut collect)
                }
                Provider::Anthropic => providers::stream_anthropic(
                    &config.api_key,
                    &config.model_name,
                    ctx,
                    &mut collect,
                ),
                // Handled before this is ever called.
                Provider::Offline => Ok(String::new()),
            }
        };

        match streamed {
            Ok(reply) => Ok(reply),
            Err(stream_error) if partial.is_empty() => {
                let blocking = match config.provider {
                    Provider::Ollama => {
                        providers::call_ollama(&config.endpoint, &config.model_name, ctx)
                    }
                    Provider::LmStudio | Provider::OpenAi | Provider::Groq => {
                        providers::call_openai_compatible(
                            config.provider,
                            &config.endpoint,
                            &config.api_key,
                            &config.model_name,
                            ctx,
                        )
                    }
                    Provider::Gemini => {
                        providers::call_gemini(&config.api_key, &config.model_name, ctx)
                    }
                    Provider::Anthropic => {
                        providers::call_anthropic(&config.api_key, &config.model_name, ctx)
                    }
                    Provider::Offline => Ok(String::new()),
                };
                match blocking {
                    Ok(reply) => {
                        sink(&reply);
                        Ok(reply)
                    }
                    Err(call_error) => Err(StreamFailure {
                        message: format!("{stream_error} (retry without streaming: {call_error})"),
                        partial: String::new(),
                    }),
                }
            }
            Err(stream_error) => Err(StreamFailure {
                message: stream_error,
                partial,
            }),
        }
    }

    /// Generates a reply, feeding it to `sink` in the order it arrives: one call per
    /// delta while streaming, or a single call with the whole text for instant commands,
    /// offline mode, and the non-streaming fallback. The returned String is always the
    /// concatenation of everything the sink was given.
    pub fn generate_response_streamed(
        &self,
        prompt: &str,
        session_id: &str,
        sink: providers::Sink,
    ) -> String {
        let config = self.load_config();
        let start = Instant::now();

        // Instant commands and offline mode produce their whole reply locally, with no
        // stream to follow -- they arrive as one delta.
        if let Some(reply) = self.check_instant_commands(prompt, &config) {
            sink(&reply);
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

        let reply = if config.provider == Provider::Offline {
            let reply = config.persona.offline_reply(
                prompt,
                &telem.os_name,
                telem.cpu_percent,
                &config.agent_name,
            );
            sink(&reply);
            reply
        } else {
            match self.call_provider(&config, &ctx, sink) {
                Ok(reply) => reply,
                Err(failure) => {
                    eprintln!(
                        "[AETHER1] LLM Engine Error: provider {} error: {}",
                        config.provider, failure.message
                    );
                    if failure.partial.is_empty() {
                        let fallback = config.persona.offline_reply(
                            prompt,
                            &telem.os_name,
                            telem.cpu_percent,
                            &config.agent_name,
                        );
                        let reply = format!(
                            "[HUD Alert: Neural link to {} timed out. Engaging localized cognitive fallback]\n\n{fallback}",
                            config.provider
                        );
                        sink(&reply);
                        reply
                    } else {
                        // Text already reached the operator; append the notice to what
                        // they are reading instead of replacing it.
                        let notice = format!(
                            "\n\n[HUD Alert: Neural link to {} dropped mid-transmission.]",
                            config.provider
                        );
                        sink(&notice);
                        format!("{}{notice}", failure.partial)
                    }
                }
            }
        };

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

    pub fn usage_snapshot(&self) -> UsageSnapshot {
        self.usage.lock().unwrap().snapshot()
    }

    pub fn db(&self) -> &MemoryDb {
        &self.db
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

    // load_config's cloud-key fallback reads the same env vars model_scanner::tests
    // mutates; every test below that touches config/generate_response needs this guard
    // so the two test modules don't race each other under parallel test execution.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::model_scanner::CLOUD_ENV_TEST_GUARD.lock().unwrap()
    }

    #[test]
    fn default_config_is_offline_halcy() {
        let _guard = env_guard();
        let engine = temp_engine("default_config");
        let config = engine.load_config();
        assert_eq!(config.provider, Provider::Offline);
        assert_eq!(config.persona, Persona::Halcy);
        assert_eq!(config.agent_name, "HALCY");
    }

    /// Every reply in these tests goes through the streaming path and asserts the
    /// invariant the whole design rests on: the deltas handed to the sink, concatenated,
    /// are exactly the reply that was returned.
    fn generate(engine: &LlmEngine, prompt: &str, session_id: &str) -> String {
        let mut streamed = String::new();
        let reply = engine
            .generate_response_streamed(prompt, session_id, &mut |delta| streamed.push_str(delta));
        assert_eq!(
            streamed, reply,
            "the sink's deltas must reconstruct the returned reply"
        );
        reply
    }

    #[test]
    fn generate_response_offline_default_mentions_standby() {
        let _guard = env_guard();
        let engine = temp_engine("offline_default");
        let reply = generate(&engine, "hello there", "test-session");
        assert!(reply.contains("Offline Standby Mode"), "reply was: {reply}");
    }

    #[test]
    fn who_are_you_instant_command_short_circuits_network() {
        let _guard = env_guard();
        let engine = temp_engine("who_are_you");
        let reply = generate(&engine, "who are you?", "test-session");
        assert!(reply.contains("HALCY"), "reply was: {reply}");
    }

    #[test]
    fn remember_that_persists_and_list_memory_reads_it_back() {
        let _guard = env_guard();
        let engine = temp_engine("remember");
        let ack = generate(
            &engine,
            "remember that favorite_color: blue",
            "test-session",
        );
        assert!(ack.contains("favorite_color"), "ack was: {ack}");

        let listing = generate(&engine, "list memory", "test-session");
        assert!(listing.contains("favorite_color"), "listing was: {listing}");
        assert!(listing.contains("blue"), "listing was: {listing}");
    }

    #[test]
    fn set_name_updates_agent_name_case_preserving() {
        let _guard = env_guard();
        let engine = temp_engine("set_name");
        let reply = generate(&engine, "Set Name Aria", "test-session");
        assert!(reply.contains("Aria"), "reply was: {reply}");
        assert_eq!(engine.agent_name(), "Aria");
    }

    #[test]
    fn generate_identity_from_purpose_persists_settings() {
        let _guard = env_guard();
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
        let _guard = env_guard();
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

        let reply = generate(
            &engine,
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
