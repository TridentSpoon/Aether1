// Rust port of backend/llm_engine.py. Persona/provider dispatch use `match` on enums
// (see persona.rs) instead of the dict lookups and if/elif chains in the Python; chat
// history and memories are Vec<Struct> (see db.rs) instead of List[Dict]. Shares the same
// SQLite file and schema as the Python backend, so either engine can serve a request
// against the same conversation/settings state.

mod db;
pub mod flow;
mod genesis;
mod persona;
mod pricing;
pub(crate) mod providers;
pub mod routing;
mod stt;
pub mod telemetry;
pub mod tts;

use std::sync::Mutex;
use std::time::Instant;

pub use db::{
    ActionRecord, ActionStatus, DayCount, MemoryDb, Message, ModelBenchmark, SessionSummary,
    UsageTotals,
};
pub use genesis::Identity;
// The voices behind a persona, re-exported because speech resolves them per persona at the
// moment it speaks -- see commands::synthesize_speech.
pub use genesis::{local_voice_for as persona_local_voice, voice_for as persona_voice};
// Re-exported because commands.rs serves the persona catalogue to the HUD: the Settings list
// is built from the enum rather than written out again in the markup.
pub use persona::{Domain, Persona, Provider, Root};
use providers::ChatContext;
pub use providers::Sink;
pub use stt::{local_status as stt_local_status, managed_env_command, stage_audio, transcribe};
pub use telemetry::{Pulse, Sampler, Telemetry};
pub use tts::{
    generate_speech_reporting, generate_speech_with, local_status as tts_local_status,
    os_engine_name as tts_os_engine_name, os_status as tts_os_status, Engine as TtsEngine,
    DEFAULT_VOICE, LOCAL_NAME as TTS_LOCAL_NAME,
};

/// How many times the model may call tools before it has to answer. High enough for a
/// real chain (look at a directory, read the interesting file, check a process), low
/// enough that a model stuck in a loop stops costing time and tokens.
const MAX_TOOL_ROUNDS: usize = 6;

/* There was a SESSION_TOKEN_BUDGET here: a flat 100,000 invented in this file, which the
HUD then divided into to show "Used: 1.3%" and "Budget left: 98.7k" beside a gauge bar.
Nothing enforced it and no provider reported it, so those three readings were a number this
constant made up being displayed back as though it were a measurement -- and on a local
model, where tokens cost nothing at all, there was no budget to have any of left. The panel
now shows what a reply cost in money when the model has a published price, and nothing at
all when it does not. See pricing.rs.

The sparkline went with it. Fifteen per-request token totals drawn as a filled area chart
is a shape, not a finding: nobody can act on "that reply was longer than this one", and with
one reply recorded it drew a diagonal line across the panel that looked like a trend. */

#[derive(Default)]
struct UsageStats {
    session_prompt_tokens: u64,
    session_completion_tokens: u64,
    total_requests: u64,
    /// How many of those requests came back with real counts. The difference between this
    /// and total_requests is how much of the number on screen is a guess.
    measured_requests: u64,
    last_tps: f64,
    /// Whether the most recent reply's numbers were reported by the provider.
    last_measured: bool,
}

/// Serializable snapshot of UsageStats for the HUD's telemetry panel.
#[derive(serde::Serialize)]
pub struct UsageSnapshot {
    pub last_tps: f64,
    pub total_session_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub requests: u64,
    /// True when the last reply's counts came from the provider rather than from the
    /// character-count heuristic. The panel says which, because "1,204 tokens" and
    /// "about 1,200 tokens" are different claims and only one of them is checkable.
    pub measured: bool,
    pub measured_requests: u64,
    /// "local", "cloud" or "offline" -- which half of the panel is the relevant one.
    pub mode: &'static str,
    pub provider: String,
    pub model: String,
    /// What the local model is, when there is one and it says. None for cloud.
    pub capability: Option<providers::LocalCapability>,
    /// What this session's cloud tokens have cost, when the model has a known price.
    ///
    /// None covers two different situations that the panel words differently: a local
    /// model, whose tokens are free, and a cloud model nobody has priced yet, which needs
    /// a line adding to the price file. `mode` is what tells them apart.
    pub cost: Option<pricing::Cost>,
    /// Whether every request behind `cost` reported its own token counts. A cost built on
    /// even one estimated reply is shown as approximate, because it is.
    pub cost_fully_measured: bool,
    /// When the built-in price list was last checked, shown beside any cost derived from
    /// it so a stale table is visible rather than silently trusted.
    pub prices_as_of: &'static str,
    /// Every model measured on this machine, fastest first. Empty until a provider that
    /// reports its own generation time has answered at least once.
    pub benchmarks: Vec<ModelBenchmark>,
    /// Whether the configured provider reports the time it spent generating, and so whether
    /// this model can ever appear on the scoreboard.
    ///
    /// An empty board has two entirely different causes -- a provider that cannot be timed,
    /// and a board that has simply not been filled yet (a fresh install, or a reset) -- and
    /// the panel has to word them differently. Only the engine knows which it is.
    pub provider_times_itself: bool,
}

/// About how many tokens a piece of text is, when nothing better is available.
///
/// Four characters per token is roughly right for English prose and wrong for code, for
/// non-Latin scripts, and for anything with a lot of punctuation. It is a fallback, and
/// anything derived from it is flagged as estimated rather than shown as fact.
fn estimate_tokens(text: &str) -> u64 {
    if text.is_empty() {
        0
    } else {
        (text.chars().count() as u64).div_ceil(4).max(1)
    }
}

/// Everything `UsageStats::snapshot` needs that the counters themselves do not hold.
///
/// A struct rather than six more parameters: the counters know how many tokens went by,
/// and nothing else -- which provider they went to, what it charges, and what this machine
/// has clocked other models at all live in the database, and passing them positionally
/// through a seven-argument call was how `provider` and `model`, both `String`, would
/// eventually end up the wrong way round.
struct SnapshotContext<'a> {
    mode: &'static str,
    provider: String,
    model: String,
    capability: Option<providers::LocalCapability>,
    prices: &'a std::collections::HashMap<String, pricing::ModelPrice>,
    benchmarks: Vec<ModelBenchmark>,
    provider_times_itself: bool,
}

impl UsageStats {
    /// `reported` is what the provider said about itself, when it said anything. The text
    /// is still passed in because it is what the estimate falls back to.
    fn record_usage(
        &mut self,
        prompt: &str,
        completion: &str,
        duration_secs: f64,
        reported: Option<providers::TokenUsage>,
    ) {
        let (prompt_tok, completion_tok) = match reported {
            Some(usage) => (usage.prompt_tokens, usage.completion_tokens),
            None => (estimate_tokens(prompt), estimate_tokens(completion)),
        };

        self.session_prompt_tokens += prompt_tok;
        self.session_completion_tokens += completion_tok;
        self.total_requests += 1;
        self.last_measured = reported.is_some();
        if self.last_measured {
            self.measured_requests += 1;
        }

        /* Generation time, not wall-clock time, wherever the provider measures it. Wall
        clock includes loading a model off disk and waiting behind another request, so a
        7B model that answers in two seconds after a four-second load looks half as fast
        as it is -- and looks slower on the first reply than on every one after, which
        reads as a fault rather than as a cold start. */
        let seconds = reported
            .and_then(|u| u.eval_nanos)
            .filter(|nanos| *nanos > 0)
            .map(|nanos| nanos as f64 / 1_000_000_000.0)
            .unwrap_or(duration_secs);
        self.last_tps = (completion_tok as f64 / seconds.max(0.05) * 10.0).round() / 10.0;
    }

    fn snapshot(&self, about: SnapshotContext) -> UsageSnapshot {
        let total_session = self.session_prompt_tokens + self.session_completion_tokens;

        /* Cloud only. A local model's tokens are free, and running them through a price
        table would produce $0.00 -- indistinguishable on screen from a cloud model whose
        price nobody has filled in yet. */
        let cost = (about.mode == "cloud")
            .then(|| {
                pricing::cost_of(
                    &about.model,
                    self.session_prompt_tokens,
                    self.session_completion_tokens,
                    about.prices,
                )
            })
            .flatten();

        UsageSnapshot {
            last_tps: self.last_tps,
            total_session_tokens: total_session,
            prompt_tokens: self.session_prompt_tokens,
            completion_tokens: self.session_completion_tokens,
            requests: self.total_requests,
            measured: self.last_measured,
            measured_requests: self.measured_requests,
            mode: about.mode,
            provider: about.provider,
            model: about.model,
            capability: about.capability,
            cost,
            cost_fully_measured: self.total_requests > 0
                && self.measured_requests == self.total_requests,
            prices_as_of: pricing::PRICES_AS_OF,
            benchmarks: about.benchmarks,
            provider_times_itself: about.provider_times_itself,
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
    /// How to name the box this is running on: its nickname and kind from the Profile pane,
    /// always with the hostname in it. Resolved once per turn so every mention in a reply
    /// names the same machine, and so a rename mid-generation cannot split a single answer
    /// between two names.
    machine: String,
    /// What to call the operator, from the Profile pane. Empty when they have not said,
    /// which is every install before that field existed -- and then the prompt says nothing
    /// about who is on the other side rather than inventing a name for them.
    operator_name: String,
    provider: Provider,
    model_name: String,
    api_key: String,
    endpoint: String,
    persona: Persona,
    persona_key: String,
    custom_directive: String,
    /// Whether this turn may leave the machine. Resolved once per turn in load_config so
    /// every decision below reads the same answer, even if the operator flips the setting
    /// while a reply is being generated.
    local_only: bool,
}

impl Config {
    /// Whether answering this turn would send the prompt off the local host or the local
    /// network. The provider alone does not settle it: Ollama and LM Studio are addressed
    /// by endpoint, so "ollama" pointed at a rented box on the internet is a cloud call
    /// wearing a local provider's name.
    fn reaches_the_internet(&self) -> bool {
        match self.provider {
            // Never leaves the process -- the reply is generated from the persona.
            Provider::Offline => false,
            Provider::Ollama | Provider::LmStudio => {
                !crate::local_only::is_local_endpoint(&self.endpoint)
            }
            Provider::OpenAi | Provider::Groq | Provider::Gemini | Provider::Anthropic => true,
        }
    }

    /// How the refusal names what it declined to talk to. The endpoint is included only
    /// for the providers that are addressed by one -- naming `llm_endpoint` while
    /// refusing OpenAI would send the operator to fix the wrong setting.
    fn what_was_not_contacted(&self) -> String {
        match self.provider {
            Provider::Ollama | Provider::LmStudio => {
                format!("{} at {} was not contacted", self.provider, self.endpoint)
            }
            _ => format!("{} was not contacted", self.provider),
        }
    }
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
    /// What the current local model says it is, keyed by the endpoint and model it was
    /// asked about. Filled in after a turn rather than when the panel asks, because the
    /// panel asks about once a second and this costs an HTTP round trip -- a getter that
    /// quietly does I/O on a timer is a getter that will one day be the reason the HUD
    /// stutters.
    capability: Mutex<Option<(String, providers::LocalCapability)>>,
    /// What the configured endpoint last said it can run, with the endpoint it was asked
    /// about and when. Step 19's routing needs this on every turn, and asking the server
    /// every turn would put an HTTP round trip in front of each reply for a list that
    /// changes when someone installs a model -- which is to say, rarely.
    available: Mutex<Option<(String, Instant, Vec<String>)>>,
    /// Models already reported missing this session, so "your model is gone" is said once
    /// rather than before every reply. Per process rather than stored: the notice is about
    /// this run of the app noticing, and a fact worth telling someone twice a week is not
    /// worth telling them forty times an evening.
    reported_missing: Mutex<std::collections::HashSet<String>>,
    /// The line the routing wants the operator to see, waiting to be collected. Read and
    /// cleared by `take_routing_notice`.
    routing_notice: Mutex<Option<String>>,
}

/// How long the endpoint's model list is trusted before asking again. Short enough that
/// installing a model and switching to it feels immediate, long enough that a conversation
/// is not a series of round trips to `/api/tags`.
const AVAILABLE_TTL: std::time::Duration = std::time::Duration::from_secs(60);

impl LlmEngine {
    pub fn new(db: MemoryDb) -> LlmEngine {
        LlmEngine {
            db,
            usage: Mutex::new(UsageStats::default()),
            capability: Mutex::new(None),
            available: Mutex::new(None),
            reported_missing: Mutex::new(std::collections::HashSet::new()),
            routing_notice: Mutex::new(None),
        }
    }

    /// What the configured endpoint can run, cached for `AVAILABLE_TTL`.
    ///
    /// None throughout means "could not ask", which `routing::resolve` treats as a reason
    /// to leave every stored choice alone rather than as an empty machine.
    fn available_models(&self, endpoint: &str) -> Vec<String> {
        {
            let cached = self.available.lock().unwrap();
            if let Some((known, at, models)) = cached.as_ref() {
                if known == endpoint && at.elapsed() < AVAILABLE_TTL {
                    return models.clone();
                }
            }
        }
        let models = crate::model_scanner::models_at(endpoint).unwrap_or_default();
        *self.available.lock().unwrap() =
            Some((endpoint.to_string(), Instant::now(), models.clone()));
        models
    }

    /// The model this persona should run on, and the line to show if that is a surprise.
    ///
    /// The notice is parked rather than returned, because the caller here is building a
    /// turn and has nowhere to put a sentence -- `take_routing_notice` is how the HUD and
    /// the CLI collect it.
    fn route(&self, persona: &Persona, endpoint: &str) -> Option<String> {
        let available = self.available_models(endpoint);
        let choice = routing::resolve(
            &self.db,
            persona,
            &available,
            &routing::measured_speeds(&self.db),
        );
        if let routing::Reason::Missing(gone) = &choice.reason {
            // Once per model per session: the same missing model on every turn is the
            // notice teaching the operator to ignore notices.
            let first_time = self
                .reported_missing
                .lock()
                .unwrap()
                .insert(format!("{}|{gone}", persona.key()));
            if first_time {
                *self.routing_notice.lock().unwrap() = choice.notice(persona);
            }
        }
        choice.model
    }

    /// Collects the routing's pending line, if there is one, and clears it.
    pub fn take_routing_notice(&self) -> Option<String> {
        self.routing_notice.lock().unwrap().take()
    }

    /// Which half of the telemetry panel this configuration makes sense for.
    ///
    /// Takes the provider and endpoint rather than a Config so the one-second telemetry
    /// tick, which reads settings directly, gets the same answer as the turn that reads a
    /// whole Config. Two copies of this rule would eventually be two different rules.
    ///
    /// The provider alone does not settle it, for the same reason `reaches_the_internet`
    /// exists: Ollama pointed at a rented box is a cloud call wearing a local provider's
    /// name, and its tokens are somebody's bill.
    fn telemetry_mode(provider: Provider, endpoint: &str) -> &'static str {
        match provider {
            Provider::Offline => "offline",
            Provider::Ollama | Provider::LmStudio
                if crate::local_only::is_local_endpoint(endpoint) =>
            {
                "local"
            }
            _ => "cloud",
        }
    }

    /// After a turn on a local model, ask the server what that model is -- once per
    /// endpoint-and-model pair, and never again until one of them changes.
    fn refresh_capability(&self, config: &Config) {
        if Self::telemetry_mode(config.provider, &config.endpoint) != "local"
            || config.provider != Provider::Ollama
        {
            return;
        }
        let key = format!("{}|{}", config.endpoint, config.model_name);
        if matches!(self.capability.lock().unwrap().as_ref(), Some((known, _)) if *known == key) {
            return;
        }
        if let Some(found) = providers::ollama_capability(&config.endpoint, &config.model_name) {
            *self.capability.lock().unwrap() = Some((key, found));
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
        let persona_key = self.db.get_setting_string("persona_type", "default");
        let custom_directive = self.db.get_setting_string("custom_directive", "");
        let operator_name = self.db.get_setting_string("operator_name", "");
        let machine = crate::profile::machine_description(&self.db);

        if agent_name == "HALCY" {
            agent_name = match persona_key.as_str() {
                "arx-limes" => "A.R.X.LIMES".to_string(),
                "arx-logos" => "A.R.X.LOGOS".to_string(),
                "nexus" => "THE NEXUS".to_string(),
                "red9000" | "red" => "R.E.D. 9000".to_string(),
                "alt" | "cunningham" | "a1ter_nul" => "A1ter_nul".to_string(),
                _ => agent_name,
            };
        }

        let local_only = crate::local_only::enabled(&self.db);

        // A cloud key lying around in the environment is a convenience: it saves an
        // operator who already has one from typing it in again. It is also the one place
        // Aether1 promotes itself from offline to cloud without being asked, which is
        // precisely what local-only mode exists to stop -- so with the mode on, the scan
        // does not happen and the operator's actual choice stands.
        if api_key.is_empty() && !local_only {
            if let Some((key, detected_provider)) = detect_cloud_api_key() {
                api_key = key;
                if provider_key == "offline" {
                    provider_key = detected_provider.to_string();
                }
            }
        }

        let provider = Provider::from_key(&provider_key);
        let persona = Persona::from_key(&persona_key);

        // Step 19. Only for a local provider: routing between models the operator has
        // installed is the whole point, and quietly swapping the model on a cloud call
        // would change what somebody is billed for.
        let model_name = match provider {
            Provider::Ollama | Provider::LmStudio => {
                self.route(&persona, &endpoint).unwrap_or(model_name)
            }
            _ => model_name,
        };

        Config {
            agent_name,
            machine,
            operator_name,
            provider,
            model_name,
            api_key,
            endpoint,
            persona,
            persona_key,
            custom_directive,
            local_only,
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

        // The vault replaces what used to be here: the first ten key-value rows, pasted
        // in whether or not they had anything to do with the question. Priming from an
        // index is both smaller and better -- the model is told what exists and reads what
        // it needs, so memory can outgrow any context window.
        let memory_context = crate::vault::prime(&self.db);

        // The model's-own persona sends no directive at all, so the prompt must not open with
        // two blank lines where a personality would have been.
        let base_persona = if base_persona.trim().is_empty() {
            String::new()
        } else {
            format!("{base_persona}\n\n")
        };

        format!(
            "{base_persona}\
             [LIVE HOST TELEMETRY]\n\
             - Identity: {agent_name}\n\
             - Machine: {machine}\n\
             {operator_line}\
             - OS: {os_name} ({architecture})\n\
             - CPU Load: {cpu_percent:.1}% | RAM: {ram_used:.2}GB / {ram_total:.2}GB ({ram_percent:.1}%)\n\
             - Uptime: {uptime}\n\
             - System Health: {status}\n\
             {memory_context}\n\n\
             Instructions:\n\
             1. Refer to live telemetry if asked about the system or device health. Name this \
             machine as it is named on the Machine line rather than calling it \"this machine\": \
             the operator may be running AETHER1 on several.\n\
             2. Persona is light flavor, not a requirement -- always prioritize a clear, accurate, directly \
             useful answer over staying in character.\n\
             3. Refer to yourself as {agent_name}.",
            agent_name = config.agent_name,
            machine = config.machine,
            operator_line = if config.operator_name.is_empty() {
                String::new()
            } else {
                format!("- Operator: {}\n", config.operator_name)
            },
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

    /// The one-line trace naming which vault notes are behind an answer -- the always-primed
    /// ones (see vault::prime, which put them in system_prompt in the first place). An ad
    /// hoc read_file mid-turn already gets its own trace line (see tool_loop); this is the
    /// one the operator otherwise never sees, since priming happens by concatenation, not by
    /// a tool call. Empty when there's no vault yet, same as prime().
    fn vault_trace(&self) -> String {
        let notes = crate::vault::primed_notes(&self.db);
        if notes.is_empty() {
            String::new()
        } else {
            crate::tools::protocol::trace_of(&format!("vault notes loaded: {}", notes.join(", ")))
        }
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
            "status" | "system status" | "telemetry" | "specs" => {
                return Some(Telemetry::snapshot().diagnostic_report());
            }
            "diagnostics" | "health check" => {
                return Some(crate::cli::run_diagnostics());
            }
            "who are you" | "who are you?" | "identify" | "identify yourself" => {
                return Some(config.persona.who_are_you(&config.agent_name));
            }
            "list memory" | "show memories" | "recall memories" | "list notes" => {
                return Some(crate::vault::describe(&self.db));
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
            // Straight into the vault, without an approval card. This is the operator
            // typing "remember that ..." themselves -- asking them to approve their own
            // instruction would be ceremony, not consent. Everything the *model* decides
            // to record still goes through append_note and waits.
            let fact = rest.trim();
            if !fact.is_empty() {
                if let Ok(note) = crate::vault::remember(&self.db, fact) {
                    return Some(format!("Written to `{note}`."));
                }
            }
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
    ) -> Result<providers::Completion, StreamFailure> {
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
                Provider::Offline => Ok(providers::Completion::default()),
            }
        };

        match streamed {
            Ok(completion) => Ok(completion),
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
                    Provider::Offline => Ok(providers::Completion::default()),
                };
                match blocking {
                    Ok(completion) => {
                        sink(&completion.text);
                        Ok(completion)
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

    /// Tries the given provider/model/endpoint/key with one trivial call, bypassing
    /// history, persona and token accounting entirely -- this is a connectivity probe run
    /// from Settings before Save, not a chat turn. Reuses explain_failure so a bad key or a
    /// wrong model name reads exactly the same here as it would mid-conversation.
    pub fn test_connection(
        &self,
        provider_key: &str,
        model: &str,
        endpoint: &str,
        api_key: &str,
    ) -> Result<String, String> {
        let provider = Provider::from_key(provider_key);

        if provider == Provider::Offline {
            return Ok(
                "Offline mode needs no connection -- it never leaves this machine.".to_string(),
            );
        }

        // Same rule Config::reaches_the_internet applies to a real turn: Ollama/LM Studio
        // are judged by where their endpoint actually points, every other provider always
        // leaves the machine. A test click is not an exemption from local-only mode.
        let reaches_the_internet = !matches!(provider, Provider::Ollama | Provider::LmStudio)
            || !crate::local_only::is_local_endpoint(endpoint);
        if reaches_the_internet && crate::local_only::enabled(&self.db) {
            return Err(crate::local_only::refusal(
                "the test connection was not attempted",
            ));
        }

        let ctx = ChatContext {
            system_prompt: "",
            history: &[],
            prompt: "Reply with just the word OK.",
            images: &[],
            agent_name: "",
            // No tools offered and no prior rounds to replay -- this is a bare connectivity
            // probe, not a real turn, so there is nothing native tool-calling needs to see.
            tools: &[],
            exchanges: &[],
        };

        let started = Instant::now();
        let result = match provider {
            Provider::Ollama => providers::call_ollama(endpoint, model, &ctx),
            Provider::LmStudio | Provider::OpenAi | Provider::Groq => {
                providers::call_openai_compatible(provider, endpoint, api_key, model, &ctx)
            }
            Provider::Gemini => providers::call_gemini(api_key, model, &ctx),
            Provider::Anthropic => providers::call_anthropic(api_key, model, &ctx),
            Provider::Offline => unreachable!("handled above"),
        };

        match result {
            Ok(completion) => Ok(format!(
                "{provider} replied in {:.1}s: \"{}\"",
                started.elapsed().as_secs_f64(),
                Self::truncate_reply(&completion.text)
            )),
            Err(e) => Err(providers::explain_failure(provider, endpoint, &e)),
        }
    }

    /// Keeps a chatty test model's reply from blowing up the status box -- this only needs
    /// to prove the round trip worked, not display the whole thing.
    fn truncate_reply(text: &str) -> String {
        const LIMIT: usize = 160;
        if text.len() <= LIMIT {
            return text.to_string();
        }
        let mut end = LIMIT;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &text[..end])
    }

    /// Runs rounds of model call -> tool calls -> results until the model answers without
    /// asking for a tool, and returns what the operator saw.
    ///
    /// What the operator sees and what the model writes are deliberately different: the
    /// fenced tool blocks are filtered out of the stream and replaced by a one-line trace
    /// of what actually ran. The model's raw text is still what gets parsed, and the
    /// visible text is what gets stored as the reply -- the conversation history should
    /// read the way the conversation looked.
    fn tool_loop(
        &self,
        config: &Config,
        system_prompt: &str,
        base_history: Vec<Message>,
        user_prompt: &str,
        images: &[providers::MediaAttachment],
        preamble: &str,
        sink: providers::Sink,
    ) -> providers::Completion {
        let registry = crate::tools::registry();
        let tool_ctx = crate::tools::ToolContext::new(&self.db);

        let mut history = base_history;
        let mut current_prompt = user_prompt.to_string();
        let mut visible = String::new();
        if !preamble.is_empty() {
            visible.push_str(preamble);
            sink(preamble);
        }
        /* One answer can take several round trips, and every one of them costs tokens. A
        loop that reported only its final round would show a fraction of what a tool-using
        turn actually spent, which is precisely the sort of comfortable under-count this
        whole change exists to remove. */
        let mut total = providers::TokenUsage::default();
        let mut any_reported = false;
        let mut carry = |usage: Option<providers::TokenUsage>| {
            if let Some(usage) = usage {
                any_reported = true;
                total.prompt_tokens += usage.prompt_tokens;
                total.completion_tokens += usage.completion_tokens;
                if let Some(nanos) = usage.eval_nanos {
                    total.eval_nanos = Some(total.eval_nanos.unwrap_or(0) + nanos);
                }
            }
        };
        let done = |text: String, any_reported: bool, total: providers::TokenUsage| {
            providers::Completion {
                text,
                usage: any_reported.then_some(total),
                calls: Vec::new(),
            }
        };

        /* Providers that take a tool list in their own request format get one, and answer
        with structured calls. The rest stay on the fenced text protocol. The two paths
        differ in three places and nowhere else: what goes into the request, where the
        calls are read from, and how the round is carried into the next one -- a native
        round becomes an Exchange, a text round becomes two more history messages. */
        let native = config.provider.supports_native_tools();
        let schemas = registry.schemas();
        let none: Vec<crate::tools::ToolSchema> = Vec::new();
        let mut exchanges: Vec<providers::Exchange> = Vec::new();

        for _round in 0..MAX_TOOL_ROUNDS {
            let ctx = ChatContext {
                system_prompt,
                history: &history,
                prompt: &current_prompt,
                images,
                agent_name: &config.agent_name,
                tools: if native { &schemas } else { &none },
                exchanges: &exchanges,
            };

            // Nothing to filter on the native path: the calls never appear in the text, so
            // withholding a fence there could only swallow prose the operator should see.
            let mut filter = crate::tools::protocol::FenceFilter::new();
            let outcome = {
                let mut round_sink = |delta: &str| {
                    let shown = if native {
                        delta.to_string()
                    } else {
                        filter.push(delta)
                    };
                    if !shown.is_empty() {
                        visible.push_str(&shown);
                        sink(&shown);
                    }
                };
                self.call_provider(config, &ctx, &mut round_sink)
            };
            let tail = filter.finish();
            if !native && !tail.is_empty() {
                visible.push_str(&tail);
                sink(&tail);
            }

            let (raw, native_calls) = match outcome {
                Ok(completion) => {
                    carry(completion.usage);
                    (completion.text, completion.calls)
                }
                Err(failure) => {
                    eprintln!(
                        "[AETHER1] LLM Engine Error: provider {} error: {}",
                        config.provider, failure.message
                    );
                    let notice = format!(
                        "\n\n[HUD Alert: {}]",
                        providers::explain_failure(
                            config.provider,
                            &config.endpoint,
                            &failure.message
                        )
                    );
                    sink(&notice);
                    return done(format!("{visible}{notice}"), any_reported, total);
                }
            };

            /* Both paths converge on the same list. A text-protocol call has no id of its
            own, so it is given its position in the round -- nothing reads it back on that
            path, and it keeps one shape for the executor to work from. */
            let calls: Vec<providers::NativeCall> = if native {
                native_calls
            } else {
                crate::tools::protocol::parse_calls(&raw)
                    .into_iter()
                    .enumerate()
                    .map(|(index, call)| providers::NativeCall {
                        id: index.to_string(),
                        tool: call.tool,
                        arguments: call.arguments,
                    })
                    .collect()
            };
            if calls.is_empty() {
                return done(visible.trim().to_string(), any_reported, total);
            }

            let mut results = Vec::new();
            for call in &calls {
                // A mutating tool describes itself for an approval card, and that
                // description is a better trace line than its raw arguments -- "Create
                // ~/notes.md (31 bytes)" beats the file's entire contents inlined.
                let trace = match registry.get(&call.tool) {
                    Some(tool) if tool.mutating() => {
                        crate::tools::protocol::trace_of(&tool.preview(&call.arguments))
                    }
                    _ => crate::tools::protocol::trace_line(&crate::tools::protocol::ToolCall {
                        tool: call.tool.clone(),
                        arguments: call.arguments.clone(),
                    }),
                };
                visible.push_str(&trace);
                sink(&trace);
                results.push((
                    call.tool.clone(),
                    crate::tools::run(registry, &tool_ctx, &call.tool, &call.arguments),
                ));
            }

            if native {
                /* The round goes back as the provider's own structures: the assistant turn
                that asked, then one turn carrying every answer. The stored conversation
                stays plain text -- these live only for the length of this turn, and
                nothing reads them back afterwards. */
                let returned = calls
                    .iter()
                    .zip(&results)
                    .map(|(call, (tool, outcome))| providers::CallResult {
                        id: call.id.clone(),
                        tool: tool.clone(),
                        output: match outcome {
                            Ok(text) => text.clone(),
                            Err(error) => format!("FAILED: {error}"),
                        },
                    })
                    .collect();
                exchanges.push(providers::Exchange::Called { text: raw, calls });
                exchanges.push(providers::Exchange::Returned(returned));
            } else {
                // Carry the round into the history so the next one can see what it asked
                // for and what came back.
                history.push(Message {
                    sender: "user".to_string(),
                    text: current_prompt,
                    timestamp: String::new(),
                });
                history.push(Message {
                    sender: "assistant".to_string(),
                    text: raw,
                    timestamp: String::new(),
                });
                current_prompt = crate::tools::protocol::format_results(&results);
            }
        }

        let notice = format!(
            "\n\n[HUD Alert: stopped after {MAX_TOOL_ROUNDS} rounds of tool calls without \
             reaching an answer.]"
        );
        sink(&notice);
        done(format!("{visible}{notice}"), any_reported, total)
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
        self.generate_response_streamed_with_media(prompt, session_id, &[], sink)
    }

    pub fn generate_response_streamed_with_media(
        &self,
        prompt: &str,
        session_id: &str,
        images: &[providers::MediaAttachment],
        sink: providers::Sink,
    ) -> String {
        let config = self.load_config();
        let start = Instant::now();

        // Instant commands and offline mode produce their whole reply locally, with no
        // stream to follow -- they arrive as one delta.
        if let Some(reply) = self.check_instant_commands(prompt, &config) {
            sink(&reply);
            self.usage.lock().unwrap().record_usage(
                prompt,
                &reply,
                start.elapsed().as_secs_f64(),
                None,
            );
            return reply;
        }

        let history = self.db.get_messages(session_id, 8).unwrap_or_default();
        let telem = Telemetry::snapshot();
        let mut system_prompt = self.system_prompt(&config, &telem);

        // Inject Graft code graph context if a project is selected
        let graft_context = crate::graft::prime(&self.db, prompt);
        system_prompt.push_str(&graft_context);

        // Tools are off by default and the catalog can be empty, in which case the prompt
        // says nothing about tools and the turn is exactly what it was before they existed.
        let registry = crate::tools::registry();
        let tools_on = crate::tools::tools_enabled(&self.db) && !registry.is_empty();
        if tools_on {
            let field = config.persona.domain().field();
            system_prompt.push_str(&if config.provider.supports_native_tools() {
                crate::tools::protocol::native_instructions(&field)
            } else {
                crate::tools::protocol::instructions(&registry.prompt_catalog(), &field)
            });
        }

        // Local-only mode refuses the cloud rather than quietly routing around it. The
        // operator still gets an answer -- going mute would be its own kind of failure --
        // but they are told which provider was not contacted, so "it still worked" can
        // never hide "it went out to the internet".
        // What the provider said about its own token use, when it said anything. Only the
        // branches that actually reach a model can fill this in; the offline and refusal
        // paths generate their text here, so there is nothing to report and the estimate
        // is the honest answer for them.
        let mut reported: Option<providers::TokenUsage> = None;
        let reply = if config.local_only && config.reaches_the_internet() {
            let fallback = config.persona.offline_reply(
                prompt,
                &telem.os_name,
                telem.cpu_percent,
                &config.agent_name,
            );
            let reply = format!(
                "[HUD Alert: {} Answering locally instead.]\n\n{fallback}",
                crate::local_only::refusal(&config.what_was_not_contacted()),
            );
            sink(&reply);
            reply
        } else if config.provider == Provider::Offline {
            // Nothing here can answer -- but another AETHER1 on this network may be able to,
            // and the operator has named one. Asked only on this branch, so a machine with a
            // model of its own never sends a word anywhere: this is what a question would
            // otherwise have got instead, which is a canned line about being offline.
            //
            // A machine that cannot be reached, or that says nothing useful, falls through
            // to that same canned line rather than surfacing a network error at someone who
            // asked a question. The reason is printed for whoever is reading the terminal.
            let setting = if crate::peers::is_relaying() {
                // This answer is already being produced for another machine. Passing the
                // question on again is how a pair of machines that name each other end up
                // talking to nobody.
                String::new()
            } else {
                self.db.get_setting_string(crate::peers::HELPER_SETTING, "")
            };
            let from_peer = crate::peers::helper(&setting).and_then(|(address, port)| {
                match crate::peers::chat_on(&address, port, prompt, session_id) {
                    Ok(reply) => Some(reply),
                    Err(e) => {
                        eprintln!("[AETHER1] {address} could not answer that: {e}");
                        None
                    }
                }
            });
            let reply = from_peer.unwrap_or_else(|| {
                config.persona.offline_reply(
                    prompt,
                    &telem.os_name,
                    telem.cpu_percent,
                    &config.agent_name,
                )
            });
            sink(&reply);
            reply
        } else if tools_on {
            let completion = self.tool_loop(
                &config,
                &system_prompt,
                history,
                prompt,
                images,
                &self.vault_trace(),
                sink,
            );
            reported = completion.usage;
            completion.text
        } else {
            // system_prompt (and whatever the vault primed into it) is only actually sent
            // to a model in this branch and the tool_loop one above -- the offline/
            // local-only-refusal replies above never look at it, so tracing it there would
            // claim something that did not happen.
            let vault_trace = self.vault_trace();
            if !vault_trace.is_empty() {
                sink(&vault_trace);
            }
            let ctx = ChatContext {
                system_prompt: &system_prompt,
                history: &history,
                prompt,
                images,
                agent_name: &config.agent_name,
                tools: &[],
                exchanges: &[],
            };
            let text = match self.call_provider(&config, &ctx, sink) {
                Ok(completion) => {
                    reported = completion.usage;
                    completion.text
                }
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
                            "[HUD Alert: {} Answering locally instead.]\n\n{fallback}",
                            providers::explain_failure(
                                config.provider,
                                &config.endpoint,
                                &failure.message
                            )
                        );
                        sink(&reply);
                        reply
                    } else {
                        // Text already reached the operator; append the notice to what
                        // they are reading instead of replacing it.
                        let notice = format!(
                            "\n\n[HUD Alert: {}]",
                            providers::explain_failure(
                                config.provider,
                                &config.endpoint,
                                &failure.message
                            )
                        );
                        sink(&notice);
                        format!("{}{notice}", failure.partial)
                    }
                }
            };
            format!("{vault_trace}{text}")
        };

        self.usage.lock().unwrap().record_usage(
            prompt,
            &reply,
            start.elapsed().as_secs_f64(),
            reported,
        );
        self.record_benchmark_sample(&config, reported);
        self.refresh_capability(&config);
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

    /// True when the model that would answer a turn right now is a server on this machine.
    ///
    /// Reads the settings directly rather than going through load_config, for the same
    /// reason usage_snapshot does: the callers are lifecycle decisions about the local
    /// server, not turns, and scanning the environment for cloud keys tells them nothing.
    pub fn uses_local_server(&self) -> bool {
        let provider_key = self.db.get_setting_string("llm_provider", "offline");
        let provider = persona::Provider::from_key(&provider_key);
        let endpoint = self
            .db
            .get_setting_string("llm_endpoint", "http://localhost:11434");
        Self::telemetry_mode(provider, &endpoint) == "local"
    }

    /// The panel's whole payload. Reads settings directly rather than going through
    /// load_config, which scans the environment for cloud keys -- fine once a turn, wasteful
    /// on the one-second telemetry tick this feeds.
    pub fn usage_snapshot(&self) -> UsageSnapshot {
        let provider_key = self.db.get_setting_string("llm_provider", "offline");
        let provider = persona::Provider::from_key(&provider_key);
        let endpoint = self
            .db
            .get_setting_string("llm_endpoint", "http://localhost:11434");
        let model = self.db.get_setting_string("llm_model", "");

        let mode = Self::telemetry_mode(provider, &endpoint);

        let key = format!("{endpoint}|{model}");
        let capability = self
            .capability
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(known, _)| *known == key)
            .map(|(_, found)| found.clone());

        /* Read from disk on every tick rather than cached at startup, so an operator who
        corrects a price in model_prices.json sees the corrected figure within the second,
        without restarting the app. It is a file of a dozen lines and the tick is one a
        second; the read does not register against anything. */
        let prices = self
            .db
            .dir()
            .map(pricing::load_overrides)
            .unwrap_or_default();

        self.usage.lock().unwrap().snapshot(SnapshotContext {
            mode,
            provider: provider.label().to_string(),
            model: model.clone(),
            capability: if mode == "local" { capability } else { None },
            prices: &prices,
            /* A failed read leaves the scoreboard empty for this tick rather than taking
            the panel down with it. */
            benchmarks: self.db.benchmarks().unwrap_or_default(),
            provider_times_itself: providers::reports_generation_time(provider),
        })
    }

    /// Adds one reply to the model-speed scoreboard, when the reply is worth measuring.
    ///
    /// Both guards matter. Without reported token counts the numerator is a
    /// characters-divided-by-four guess, and without the provider's own generation time the
    /// denominator is a wall clock that was also running while the model was read off disk
    /// -- which is why the first reply after launch looks half as fast as every one after
    /// it. Either one alone turns the scoreboard into a ranking of measurement error.
    ///
    /// So a sample is only kept when the provider reported both. Today that means Ollama,
    /// which returns `eval_count` and `eval_duration` on the last object of its stream. A
    /// server that reports neither simply never appears on the scoreboard, and the panel
    /// says so -- an empty row is a true statement about what can be measured, and an
    /// invented one is not.
    fn record_benchmark_sample(&self, config: &Config, reported: Option<providers::TokenUsage>) {
        let Some(usage) = reported else { return };
        let Some(nanos) = usage.eval_nanos.filter(|nanos| *nanos > 0) else {
            return;
        };
        if config.model_name.is_empty() {
            return;
        }
        let _ = self.db.record_benchmark(
            config.provider.label(),
            &config.model_name,
            usage.completion_tokens,
            nanos as f64 / 1_000_000_000.0,
        );
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
        let db = MemoryDb::open(&path).expect("temp db should open");
        // Point the vault at a scratch directory: an engine test must not create or read
        // notes in the developer's own home.
        let vault = path.with_extension("vault");
        let _ = std::fs::remove_dir_all(&vault);
        let _ = db.set_setting(
            "vault_path",
            &serde_json::Value::String(vault.to_string_lossy().to_string()),
        );
        LlmEngine::new(db)
    }

    /// The panel is fed entirely by this one snapshot, so what it does and does not carry
    /// is the whole contract. A local model's tokens are free: the cost line has to be
    /// absent rather than zero, because "$0.00" is what an unpriced cloud model would also
    /// render as, and those are opposite situations.
    #[test]
    fn a_local_snapshot_costs_nothing_and_says_so_by_omission() {
        let engine = temp_engine("snapshot_local");
        let _ = engine
            .db
            .set_setting("llm_provider", &serde_json::Value::String("ollama".into()));
        let _ = engine.db.set_setting(
            "llm_endpoint",
            &serde_json::Value::String("http://localhost:11434".into()),
        );
        let _ = engine.db.set_setting(
            "llm_model",
            &serde_json::Value::String("llama3.1:8b".into()),
        );

        engine.usage.lock().unwrap().record_usage(
            "a question",
            "an answer",
            2.0,
            Some(providers::TokenUsage {
                prompt_tokens: 40,
                completion_tokens: 200,
                eval_nanos: Some(2_000_000_000),
            }),
        );

        let snap = engine.usage_snapshot();
        assert_eq!(snap.mode, "local");
        assert_eq!(snap.prompt_tokens, 40);
        assert_eq!(snap.completion_tokens, 200);
        assert_eq!(snap.total_session_tokens, 240);
        assert!(snap.measured, "the provider reported its own counts");
        assert_eq!(snap.measured_requests, 1);
        assert!(
            snap.cost.is_none(),
            "a model running on this machine bills nobody: {:?}",
            snap.cost
        );
        assert!(
            snap.provider_times_itself,
            "Ollama reports eval_duration, so an empty board here means 'not yet', not 'never'"
        );
    }

    /// An empty scoreboard has two causes that need opposite sentences under it, and the
    /// panel cannot tell them apart on its own -- so the snapshot has to. Getting this
    /// backwards puts "this provider cannot be timed" under a provider that can, moments
    /// after someone pressed RESET READINGS.
    #[test]
    fn the_snapshot_says_whether_the_provider_can_be_timed_at_all() {
        let engine = temp_engine("snapshot_timeable");
        let _ = engine.db.set_setting(
            "llm_endpoint",
            &serde_json::Value::String("http://localhost:1234".into()),
        );

        for (provider, timeable) in [("ollama", true), ("lmstudio", false)] {
            let _ = engine
                .db
                .set_setting("llm_provider", &serde_json::Value::String(provider.into()));
            let snap = engine.usage_snapshot();
            assert_eq!(
                snap.mode, "local",
                "{provider} on loopback is the local half"
            );
            assert_eq!(
                snap.provider_times_itself, timeable,
                "{provider} reports its own generation time: {timeable}"
            );
        }
    }

    /// The cloud half. A model with a published price turns real counts into a real figure,
    /// and the snapshot says whether every count behind that figure was reported or some of
    /// it was guessed -- a cost built on an estimate is still an estimate.
    #[test]
    fn a_cloud_snapshot_prices_reported_tokens_and_flags_a_guessed_one() {
        let engine = temp_engine("snapshot_cloud");
        let _ = engine
            .db
            .set_setting("llm_provider", &serde_json::Value::String("openai".into()));
        let _ = engine.db.set_setting(
            "llm_model",
            // Dated build suffix on purpose: the price file lists the base name, and the
            // panel is useless if a routine version bump silently unprices the model.
            &serde_json::Value::String("gpt-4o-mini-2024-07-18".into()),
        );

        engine.usage.lock().unwrap().record_usage(
            "q",
            "a",
            1.0,
            Some(providers::TokenUsage {
                prompt_tokens: 1_000_000,
                completion_tokens: 1_000_000,
                eval_nanos: None,
            }),
        );

        let snap = engine.usage_snapshot();
        assert_eq!(snap.mode, "cloud");
        let cost = snap.cost.expect("gpt-4o-mini has a built-in price");
        assert_eq!(cost.input_usd, 0.15);
        assert_eq!(cost.output_usd, 0.60);
        assert_eq!(cost.total_usd, 0.75);
        assert!(snap.cost_fully_measured, "the one request was reported");
        assert!(
            !snap.prices_as_of.is_empty(),
            "a price shown without a date is a price nobody can tell is stale"
        );

        // One reply the provider said nothing about, so the counts fall back to the
        // character heuristic -- and the whole session's cost stops being exact.
        engine
            .usage
            .lock()
            .unwrap()
            .record_usage("q", "a", 1.0, None);
        let snap = engine.usage_snapshot();
        assert!(!snap.cost_fully_measured);
        assert!(!snap.measured);
    }

    /// The scoreboard reaches the panel through the snapshot, and only through it. It is
    /// empty until something measurable has actually answered -- not seeded, not
    /// pre-populated with the configured model at a speed nobody observed.
    #[test]
    fn the_scoreboard_starts_empty_and_fills_from_measured_replies() {
        let engine = temp_engine("snapshot_board");
        assert!(
            engine.usage_snapshot().benchmarks.is_empty(),
            "nothing has answered yet"
        );

        engine
            .db
            .record_benchmark("Ollama", "llama3.1:8b", 300, 10.0)
            .unwrap();
        engine
            .db
            .record_benchmark("Ollama", "mistral:latest", 300, 20.0)
            .unwrap();

        let board = engine.usage_snapshot().benchmarks;
        assert_eq!(board.len(), 2);
        assert_eq!(board[0].model, "llama3.1:8b", "fastest first");
        assert_eq!(board[0].average_tps, 30.0);
        assert_eq!(board[1].average_tps, 15.0);
    }

    /// The trace is how the operator finds out the vault primed anything at all -- priming
    /// happens by concatenating into the system prompt, which leaves no other trace of it.
    #[test]
    fn vault_trace_names_the_notes_actually_primed() {
        let engine = temp_engine("vault_trace");
        assert_eq!(engine.vault_trace(), "", "no vault means nothing to report");

        crate::vault::ensure(&engine.db).unwrap();
        let trace = engine.vault_trace();
        assert!(
            trace.contains("⚙ vault notes loaded:"),
            "should read like the trace a tool call gets: {trace}"
        );
        for note in ["INDEX.md", "profile.md", "machine.md"] {
            assert!(trace.contains(note), "{trace}");
        }
    }

    /// The end the operator actually experiences: with the mode on, a configured cloud
    /// provider produces a local answer and a line saying who was not contacted -- not a
    /// silent cloud call, and not silence.
    #[test]
    fn local_only_answers_locally_instead_of_calling_a_cloud_provider() {
        let engine = temp_engine("local_only_cloud");
        let _ = engine
            .db
            .set_setting("llm_provider", &serde_json::Value::String("openai".into()));
        let _ = engine
            .db
            .set_setting("llm_api_key", &serde_json::Value::String("sk-test".into()));
        let _ = engine
            .db
            .set_setting(crate::local_only::SETTING, &serde_json::Value::Bool(true));

        let mut streamed = String::new();
        let reply =
            engine.generate_response_streamed("what is the weather", "default", &mut |delta| {
                streamed.push_str(delta)
            });

        assert!(reply.contains("local-only mode is on"), "{reply}");
        assert!(reply.contains("openai was not contacted"), "{reply}");
        // Still an answer, and the sink saw exactly what was returned.
        assert!(reply.len() > 80, "{reply}");
        assert_eq!(streamed, reply);
    }

    /// The hole a provider-name check alone would leave: "ollama" is allowed, so the
    /// address it is pointed at has to be checked too.
    #[test]
    fn local_only_refuses_a_local_provider_pointed_at_the_internet() {
        let engine = temp_engine("local_only_endpoint");
        let _ = engine
            .db
            .set_setting("llm_provider", &serde_json::Value::String("ollama".into()));
        let _ = engine.db.set_setting(
            "llm_endpoint",
            &serde_json::Value::String("https://ollama.example.com".into()),
        );
        let _ = engine
            .db
            .set_setting(crate::local_only::SETTING, &serde_json::Value::Bool(true));

        let reply = engine.generate_response_streamed("hello", "default", &mut |_| {});
        assert!(reply.contains("local-only mode is on"), "{reply}");
        assert!(reply.contains("ollama.example.com"), "{reply}");
    }

    /// The same install with the mode off must still be free to use an Ollama on the LAN.
    /// Local-only mode is about the internet, not about this one machine.
    #[test]
    fn a_model_server_on_the_lan_is_allowed_with_the_mode_on() {
        let engine = temp_engine("local_only_lan");
        let _ = engine
            .db
            .set_setting("llm_provider", &serde_json::Value::String("ollama".into()));
        let _ = engine.db.set_setting(
            "llm_endpoint",
            &serde_json::Value::String("http://192.168.1.50:11434".into()),
        );
        let _ = engine
            .db
            .set_setting(crate::local_only::SETTING, &serde_json::Value::Bool(true));

        // Nothing is listening at that address in a test run, so the turn ends in the
        // normal provider-failure path -- what matters is that it was attempted at all
        // rather than refused by the mode.
        let reply = engine.generate_response_streamed("hello", "default", &mut |_| {});
        assert!(!reply.contains("local-only mode is on"), "{reply}");
    }

    /// With nothing reported, the character-count guess is still what fills the panel --
    /// but it is recorded as a guess.
    #[test]
    fn usage_stats_fall_back_to_the_estimate_and_admit_it() {
        let mut stats = UsageStats::default();
        stats.record_usage("hello world", "hi there friend", 1.0, None);
        assert!(stats.session_prompt_tokens > 0);
        assert!(stats.session_completion_tokens > 0);
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.measured_requests, 0);
        assert!(!stats.last_measured);
        assert!(stats.last_tps > 0.0);
    }

    /// The point of the whole change: when the provider says what it used, that is what is
    /// counted -- not a number derived from the length of the strings.
    #[test]
    fn a_reported_count_beats_the_estimate() {
        let mut stats = UsageStats::default();
        // Text whose estimate would be nothing like the reported truth.
        stats.record_usage(
            "hi",
            "ok",
            1.0,
            Some(providers::TokenUsage {
                prompt_tokens: 900,
                completion_tokens: 100,
                eval_nanos: None,
            }),
        );
        assert_eq!(stats.session_prompt_tokens, 900);
        assert_eq!(stats.session_completion_tokens, 100);
        assert_eq!(stats.measured_requests, 1);
        assert!(stats.last_measured);
    }

    /// Tokens per second is computed from the model's own generation time when it reports
    /// one. Wall clock includes loading the model off disk, which makes a fast model look
    /// slow on its first reply and faster on every one after -- a cold start that reads as
    /// a fault.
    #[test]
    fn throughput_prefers_the_models_own_clock_to_the_wall_clock() {
        let mut stats = UsageStats::default();
        stats.record_usage(
            "p",
            "c",
            10.0, // ten seconds of wall clock, most of it loading
            Some(providers::TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 100,
                eval_nanos: Some(2_000_000_000), // two seconds actually generating
            }),
        );
        assert_eq!(stats.last_tps, 50.0, "100 tokens in 2s, not in 10s");

        // With no reported generation time, wall clock is all there is.
        let mut wall = UsageStats::default();
        wall.record_usage(
            "p",
            "c",
            10.0,
            Some(providers::TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 100,
                eval_nanos: None,
            }),
        );
        assert_eq!(wall.last_tps, 10.0);
    }

    /// A turn that used several rounds of tools must report all of them. Reporting only the
    /// last round would under-count exactly the turns that cost the most.
    #[test]
    fn a_session_accumulates_across_requests() {
        let mut stats = UsageStats::default();
        let reported = |p, c| {
            Some(providers::TokenUsage {
                prompt_tokens: p,
                completion_tokens: c,
                eval_nanos: None,
            })
        };
        stats.record_usage("a", "b", 1.0, reported(100, 20));
        stats.record_usage("a", "b", 1.0, reported(150, 30));
        stats.record_usage("a", "b", 1.0, None);

        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.measured_requests, 2);
        assert!(stats.session_prompt_tokens >= 250);
        assert!(
            !stats.last_measured,
            "the most recent reply was the estimated one"
        );
        assert_eq!(stats.session_completion_tokens, 50 + estimate_tokens("b"));
    }

    /// An endpoint on the loopback or the LAN is local; the same provider pointed at a
    /// rented box is a cloud call wearing a local provider's name, and its tokens are
    /// somebody's bill.
    #[test]
    fn the_panel_mode_follows_the_endpoint_not_just_the_provider() {
        let local = Config {
            agent_name: "A1".into(),
            machine: "vm".into(),
            operator_name: String::new(),
            provider: Provider::Ollama,
            model_name: "llama3".into(),
            api_key: String::new(),
            endpoint: "http://localhost:11434".into(),
            persona: Persona::Default,
            persona_key: "default".into(),
            custom_directive: String::new(),
            local_only: false,
        };
        assert_eq!(
            LlmEngine::telemetry_mode(local.provider, &local.endpoint),
            "local"
        );

        let rented = Config {
            endpoint: "https://ollama.example.com".into(),
            ..local
        };
        assert_eq!(
            LlmEngine::telemetry_mode(rented.provider, &rented.endpoint),
            "cloud"
        );
        // And nothing configured is neither, rather than quietly counting as cloud.
        assert_eq!(LlmEngine::telemetry_mode(Provider::Offline, ""), "offline");
    }

    // load_config's cloud-key fallback reads the same env vars model_scanner::tests
    // mutates; every test below that touches config/generate_response needs this guard
    // so the two test modules don't race each other under parallel test execution.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::model_scanner::CLOUD_ENV_TEST_GUARD.lock().unwrap()
    }

    /// A fresh install runs the diagnostic persona, not the conversational one -- the same
    /// answer `get_settings` and the tool layer give, so the persona generating the reply is
    /// the persona whose field decides what runs without asking.
    #[test]
    fn default_config_is_offline_with_the_diagnostic_persona() {
        let _guard = env_guard();
        let engine = temp_engine("default_config");
        let config = engine.load_config();
        assert_eq!(config.provider, Provider::Offline);
        assert_eq!(config.persona, Persona::Default);
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

    /// Offline, the reply has one job: say there is no reasoning engine behind it and point
    /// at where to connect one. Each persona says that in its own words -- the diagnostic
    /// default reports on the machine first -- so this asserts the job rather than one
    /// persona's phrasing.
    #[test]
    fn generate_response_offline_says_there_is_no_engine_and_where_to_connect_one() {
        let _guard = env_guard();
        let engine = temp_engine("offline_default");
        crate::vault::ensure(engine.db()).unwrap();
        let reply = generate(&engine, "hello there", "test-session");
        assert!(reply.contains("Ollama"), "reply was: {reply}");
        assert!(reply.contains("Settings"), "reply was: {reply}");
        // The canned offline reply never looks at system_prompt (see Persona::offline_reply's
        // signature), so claiming the vault was loaded here would be exactly the kind of
        // dressed-up-as-a-measurement claim step 14 already refuses to make elsewhere.
        assert!(
            !reply.contains("vault notes loaded"),
            "a vault that was never consulted must not be claimed: {reply}"
        );
    }

    #[test]
    fn who_are_you_instant_command_short_circuits_network() {
        let _guard = env_guard();
        let engine = temp_engine("who_are_you");
        let reply = generate(&engine, "who are you?", "test-session");
        assert!(reply.contains("HALCY"), "reply was: {reply}");
    }

    #[test]
    fn remember_that_writes_a_note_the_operator_can_open() {
        let _guard = env_guard();
        let engine = temp_engine("remember");

        let ack = generate(
            &engine,
            "remember that the laptop is called tycho",
            "test-session",
        );
        assert!(ack.contains("memories.md"), "ack was: {ack}");

        // The point of the vault: what was remembered is a file, readable without us.
        let note = crate::vault::vault_path(engine.db()).join("memories.md");
        let contents = std::fs::read_to_string(&note).expect("the note should exist");
        assert!(
            contents.contains("the laptop is called tycho"),
            "{contents}"
        );
    }

    #[test]
    fn the_vault_is_what_primes_the_prompt() {
        let _guard = env_guard();
        let engine = temp_engine("priming");
        crate::vault::ensure(engine.db()).unwrap();
        let profile = crate::vault::vault_path(engine.db()).join("profile.md");
        std::fs::write(&profile, "# Profile\n\nThe operator prefers helix.\n").unwrap();

        let config = engine.load_config();
        let prompt = engine.system_prompt(&config, &Telemetry::snapshot());
        assert!(
            prompt.contains("The operator prefers helix."),
            "a fact written into the vault by hand must reach the prompt"
        );
    }

    /// The Profile pane's one field with teeth: a name saved there is what the model is
    /// told it is talking to, and an empty one says nothing at all rather than inventing
    /// somebody.
    #[test]
    fn the_operators_name_reaches_the_prompt_only_once_they_have_given_one() {
        let _guard = env_guard();
        let engine = temp_engine("operator_name");

        let config = engine.load_config();
        let before = engine.system_prompt(&config, &Telemetry::snapshot());
        assert!(
            !before.contains("- Operator:"),
            "an install with no name set must not claim one: {before}"
        );

        engine
            .db()
            .set_setting("operator_name", &serde_json::Value::String("Ada".into()))
            .unwrap();
        let config = engine.load_config();
        let after = engine.system_prompt(&config, &Telemetry::snapshot());
        assert!(after.contains("- Operator: Ada"), "{after}");
    }

    /// With several boxes running AETHER1, a reply that says "this machine" is a reply the
    /// operator has to go and disambiguate. The prompt always carries a name for the box,
    /// and carries the operator's own once they have given one.
    #[test]
    fn the_prompt_always_names_the_machine_it_is_running_on() {
        let _guard = env_guard();
        let engine = temp_engine("machine_name");

        let config = engine.load_config();
        let bare = engine.system_prompt(&config, &Telemetry::snapshot());
        let host = sysinfo::System::host_name().unwrap_or_else(|| "this machine".to_string());
        assert!(bare.contains(&format!("- Machine: {host}")), "{bare}");

        engine
            .db()
            .set_setting(
                "machine_nickname",
                &serde_json::Value::String("Aegis".into()),
            )
            .unwrap();
        engine
            .db()
            .set_setting("machine_kind", &serde_json::Value::String("homelab".into()))
            .unwrap();
        let config = engine.load_config();
        let named = engine.system_prompt(&config, &Telemetry::snapshot());
        assert!(
            named.contains(&format!("- Machine: Aegis (hostname {host}), a homelab")),
            "{named}"
        );
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
