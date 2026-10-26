// Making the local model server AETHER1's problem rather than the operator's.
//
// Everything needed to talk to a model on this machine was already here -- the port sweep
// in `model_scanner`, the spawn in `commands::start_local_server_tracked`, the idle
// lifecycle in `background_services` -- and none of it was joined up. Out of the box
// `llm_provider` is "offline" and `autostart_ollama` is false, so an operator with Ollama
// installed and running still had to open Settings, pick a provider, paste an address and
// choose a model before the companion would say anything. Get any one of the four wrong
// and the failure is somewhere else entirely: the wrong provider costs tool calling and
// the context window (see `llm::adopt_ollama_behind_openai_label`), the wrong address is
// a connection error, the wrong model is a 404 from the server.
//
// So this module does the joining. On every start AETHER1 looks at what is actually on the
// machine and configures itself to use it, and the decision is a pure function over a scan
// result so that it can be tested without a model server -- which matters here, because
// there is no model server in CI.
//
// Two things it deliberately does not do:
//
//   * override a deliberate cloud choice. An operator who has set up Anthropic or Gemini
//     has said what they want, and a local Ollama appearing on the machine is not a reason
//     to switch away from it. Only "offline", and a local provider that is not answering,
//     are treated as "nothing is configured".
//   * touch anything in local-only mode beyond reading. Starting daemons is gated there
//     already; adoption is only a settings write, but the scan behind it is a loopback
//     probe, which is what that mode permits.

use crate::llm::MemoryDb;
use crate::model_scanner::{LocalApi, LocalServer};

/// Set once AETHER1 has defaulted `autostart_ollama` on, so an operator who turns it off
/// again keeps it off. Same shape, and same reason, as `tools::apply_default_on`: the
/// settings pane writes every field on every Save, so a stored `false` sits on every
/// install where Settings has ever been opened, and a stored value beats a default.
const AUTOSTART_DEFAULT_MARKER: &str = "local_server_autostart_defaulted";

/// The provider keys that mean "a server on this machine", and so may be re-pointed at
/// whatever is actually listening. Anything else is a cloud provider the operator chose.
const LOCAL_PROVIDERS: &[&str] = &["ollama", "lmstudio"];

/// What AETHER1 decided to change, and why -- the "why" because this writes settings the
/// operator did not write, and a silent change to a provider or an address is the kind of
/// thing that makes the next failure unreadable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adoption {
    pub provider: &'static str,
    pub endpoint: String,
    /// None when the chosen server has no model loaded. The provider and address are
    /// still worth writing: that is a server to talk to as soon as a model is pulled, and
    /// the existing model setting is left alone rather than being blanked.
    pub model: Option<String>,
    pub why: String,
}

/// Ranks a discovered server as a candidate to adopt. Higher is better.
///
/// Two things decide it. A server with a model loaded beats one without, because the
/// second cannot answer a question yet. And the native API beats the OpenAI-compatible
/// one, because a server offering it is Ollama, where AETHER1 can probe capabilities,
/// send `tools` natively and set `num_ctx` -- none of which survive the `/v1` path. The
/// port is the tie-break only so that the choice is stable rather than dependent on the
/// order the sweep happened to finish in.
fn rank(server: &LocalServer) -> (u8, u8, i32) {
    let has_model = u8::from(!server.models.is_empty());
    let native = u8::from(server.api == LocalApi::Native);
    (has_model, native, -(server.port as i32))
}

/// Picks the model to use on a server AETHER1 is adopting.
///
/// The configured model wins whenever the server actually lists it, even on a server
/// AETHER1 is otherwise re-pointing: an operator who chose `qwen3:8b` means it, and the
/// address being wrong is no reason to also change the model. Otherwise the server's first
/// model is the only honest answer -- there is no basis here for preferring one of
/// somebody's installed models over another.
fn model_for(server: &LocalServer, configured: &str) -> Option<String> {
    if configured.is_empty() {
        return server.models.first().cloned();
    }
    // A server that lists nothing has not contradicted the configured model -- it has
    // nothing loaded yet. Keeping the choice is right in both directions: it is the model
    // that will be used the moment it is pulled, and replacing it with None would leave
    // the operator's choice looking abandoned in a settings pane.
    if server.models.is_empty() || server.models.iter().any(|m| m == configured) {
        return Some(configured.to_string());
    }
    server.models.first().cloned()
}

/// Decides whether to re-point AETHER1 at a server on this machine, given what the sweep
/// found and what is currently configured.
///
/// Returns None when there is nothing to do, which is the common case and covers four
/// different reasons: nothing is listening, a cloud provider is deliberately configured,
/// the configured local server is the one already chosen and running, or the configured
/// address is a local server that answered and so is none of this function's business.
pub fn choose(
    servers: &[LocalServer],
    provider: &str,
    endpoint: &str,
    model: &str,
) -> Option<Adoption> {
    // A cloud provider is a choice, not a gap. Leave it.
    if !provider.is_empty() && provider != "offline" && !LOCAL_PROVIDERS.contains(&provider) {
        return None;
    }

    let best = servers.iter().max_by_key(|s| rank(s))?;

    let configured_locally = LOCAL_PROVIDERS.contains(&provider);
    let endpoint_matches = endpoint.trim_end_matches('/') == best.endpoint.trim_end_matches('/');
    let provider_matches = provider == best.provider_key;

    // Already pointed at the server we would have chosen. Usually nothing to do -- but not
    // when the configured model is one the server does not have.
    //
    // That case used to return None here, on the reasoning that a model the operator chose
    // is not ours to change behind their back. A chat log settled it the other way: five
    // questions in a row answered with "ollama has no model by that name -- check MODEL
    // NAME / ID in Settings", while the server sat there listing models AETHER1 could have
    // used. Declining to choose is not deference when the alternative is five hard
    // failures, and the operator's choice is not being overruled by a judgement -- it is
    // not installed. So a server that lists something gets switched to it, loudly.
    if configured_locally && endpoint_matches && provider_matches {
        let model_is_missing =
            !model.is_empty() && !best.models.is_empty() && !best.models.iter().any(|m| m == model);
        if !model_is_missing {
            return None;
        }
        return Some(Adoption {
            provider: best.provider_key,
            endpoint: best.endpoint.clone(),
            model: best.models.first().cloned(),
            why: format!(
                "The configured model `{model}` is not installed on {}, which is serving {}.",
                best.label,
                describe_models(&best.models)
            ),
        });
    }

    // A configured local address that is answering is left alone even if it is not the
    // best candidate: the operator pointed AETHER1 at that server on purpose, and a second
    // server appearing on another port does not revoke that. Only an address that nothing
    // answers on is treated as a gap to fill.
    if configured_locally
        && servers
            .iter()
            .any(|s| s.endpoint.trim_end_matches('/') == endpoint.trim_end_matches('/'))
    {
        return None;
    }

    let why = if provider.is_empty() || provider == "offline" {
        format!(
            "No model was configured, and {} is serving {}.",
            best.label,
            describe_models(&best.models)
        )
    } else {
        format!(
            "The configured server at {} did not answer, and {} is serving {}.",
            if endpoint.is_empty() {
                "(no address)"
            } else {
                endpoint
            },
            best.label,
            describe_models(&best.models)
        )
    };

    Some(Adoption {
        provider: best.provider_key,
        endpoint: best.endpoint.clone(),
        model: model_for(best, model),
        why,
    })
}

fn describe_models(models: &[String]) -> String {
    match models.len() {
        0 => "no model yet".to_string(),
        1 => models[0].clone(),
        n => format!("{} and {} others", models[0], n - 1),
    }
}

/// Runs `choose` against the machine and writes the result.
///
/// Best-effort throughout: a settings write that fails must not stop AETHER1 starting, and
/// the worst case is that the same decision is made again on the next open. Prints what it
/// changed, for the same reason `adopt_ollama_behind_openai_label` does -- a provider and
/// address the operator did not type should be visible somewhere when the next thing goes
/// wrong.
pub fn adopt_local_server(db: &MemoryDb) {
    let servers = crate::model_scanner::scan_local_servers();
    let provider = db.get_setting_string("llm_provider", "offline");
    let endpoint = db.get_setting_string("llm_endpoint", "");
    let model = db.get_setting_string("llm_model", "");

    let Some(adoption) = choose(&servers, &provider, &endpoint, &model) else {
        return;
    };

    let _ = db.set_setting(
        "llm_provider",
        &serde_json::Value::String(adoption.provider.to_string()),
    );
    let _ = db.set_setting(
        "llm_endpoint",
        &serde_json::Value::String(adoption.endpoint.clone()),
    );
    if let Some(chosen) = &adoption.model {
        let _ = db.set_setting("llm_model", &serde_json::Value::String(chosen.clone()));
    }

    eprintln!(
        "[AETHER1] {} Talking to it as `{}` at {}{}.",
        adoption.why,
        adoption.provider,
        adoption.endpoint,
        match &adoption.model {
            Some(m) => format!(" with {m}"),
            None => String::new(),
        }
    );
}

/// Defaults `autostart_ollama` on, once.
///
/// It shipped false, which means an operator with Ollama installed but not running gets
/// nothing at all: AETHER1 will not start it, and the first question fails on a connection
/// error naming an address rather than saying "the server is not running". The whole
/// lifecycle for a server AETHER1 starts already exists and is careful -- it never touches
/// a server the operator started, it stops on idle, it dies with the app -- so there is
/// very little left to be cautious about, and the cost of the old default is that none of
/// that machinery ever runs.
pub fn apply_autostart_default(db: &MemoryDb) {
    if db.get_setting_bool(AUTOSTART_DEFAULT_MARKER, false) {
        return;
    }
    if !db.get_setting_bool("autostart_ollama", true) {
        let _ = db.set_setting("autostart_ollama", &serde_json::Value::Bool(true));
        eprintln!(
            "[AETHER1] AETHER1 now starts the local model server itself when one is \
             installed but not running. Turn it off again in Settings if that was \
             deliberate."
        );
    }
    let _ = db.set_setting(AUTOSTART_DEFAULT_MARKER, &serde_json::Value::Bool(true));
}

/// The context window to hand a server AETHER1 starts, for a model it loads without being
/// told one.
///
/// This is deliberately a floor and not the largest window the machine could hold, and the
/// reason is worth writing down because the obvious answer is wrong.
///
/// `OLLAMA_CONTEXT_LENGTH` is **server-wide**: it applies to every model the server loads,
/// and it is set before AETHER1 knows which model that will be. Context is memory -- the KV
/// cache for a long window on a large model is gigabytes -- so one generous figure is not
/// generous, it is a cliff. A 27B at a 2-bit quant is about 8GB of weights; on a 16GB card
/// a 32K window on top of that does not fit, so the server quietly moves layers to the CPU
/// and a greeting takes nearly three minutes. That is exactly what happened: PR #249 sized
/// this from system RAM via `code_setup::context_tokens`, which returns 32768 on a 32GB
/// machine, and the operator's 27B went from answering to "thinking... 167s".
///
/// RAM was the wrong question twice over. It is not what runs out -- VRAM is -- and even
/// VRAM cannot settle it without knowing the model's size.
///
/// What makes a floor sufficient is that the server-wide figure is not where a large window
/// belongs. `providers::local_context_window` already sets `num_ctx` **per request** on the
/// native path, clamped to 16K for this very reason ("asking for it on a 16GB card to hold a
/// chat is how a model ends up swapping to CPU"), and that is the only place the model is
/// known. So this needs to be exactly big enough that a first question cannot overflow it,
/// and no bigger; anything that wants more asks per request, where it can be judged.
///
/// The one thing this does not serve is the coding engine, which talks to the server over
/// the OpenAI-compatible endpoint where there is no `num_ctx` field, and which genuinely
/// wants a long window. It cannot be served by this number without putting the cliff back
/// for everybody; it needs its own load, and until it has one the setup wizard's own
/// context step is the honest answer for an operator who mainly codes.
pub fn server_context_tokens() -> u32 {
    SERVER_CONTEXT_FLOOR
}

/// Big enough that AETHER1's system prompt, a tool round and a few turns cannot overflow a
/// first question -- the overflow was at 5242 tokens against Ollama's 4096 -- and small
/// enough to cost nothing on any machine. The same figure as
/// `providers::LOCAL_CTX_FLOOR`, which is the floor the per-request path uses for a model
/// that reports no window of its own.
const SERVER_CONTEXT_FLOOR: u32 = 8192;

/// A local model server AETHER1 knows how to start, and the command that starts it.
///
/// Ollama first because `ollama serve` is one process in the foreground that can be held
/// as a `Child` and stopped again, which is what the rest of the lifecycle assumes. LM
/// Studio's `lms server start` is a client that asks the already-installed app to listen,
/// so the handle it hands back is not the server -- hence `holds_the_server`, which is how
/// `background_services` knows whether what it is keeping can later be stopped.
pub struct Startable {
    pub binary: std::path::PathBuf,
    pub args: &'static [&'static str],
    pub holds_the_server: bool,
    /// The environment variable, if any, that tells this server how much room to give a
    /// model it loads. Ollama reads `OLLAMA_CONTEXT_LENGTH`; LM Studio takes its context
    /// from the loaded model's own settings and has no equivalent, which is worth saying
    /// out loud rather than leaving as a silent gap.
    pub context_env: Option<&'static str>,
}

/// Finds a local model server to start, preferring Ollama.
///
/// Returns None when neither is installed, which is a different answer from "it is
/// installed and would not start" and is reported differently by the caller.
pub fn startable() -> Option<Startable> {
    if let Ok(binary) = which::which("ollama") {
        return Some(Startable {
            binary,
            args: &["serve"],
            holds_the_server: true,
            context_env: Some("OLLAMA_CONTEXT_LENGTH"),
        });
    }
    // `lms` is LM Studio's CLI. It is not on PATH by default on any platform -- the app
    // installs it under the user's home -- so the usual locations are tried too, in the
    // same spirit as the port sweep: ask the machine, do not assume.
    if let Some(binary) = lms_binary() {
        return Some(Startable {
            binary,
            args: &["server", "start"],
            holds_the_server: false,
            context_env: None,
        });
    }
    None
}

/// Where LM Studio puts its CLI. On PATH if the operator ran `lms bootstrap`, and
/// otherwise under the home directory where the app unpacks it.
fn lms_binary() -> Option<std::path::PathBuf> {
    if let Ok(found) = which::which("lms") {
        return Some(found);
    }
    let home = crate::paths::home_dir()?;
    let candidates = [
        home.join(".lmstudio/bin/lms"),
        home.join(".lmstudio/bin/lms.exe"),
        home.join(".cache/lm-studio/bin/lms"),
    ];
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(name: &str) -> MemoryDb {
        let path = std::env::temp_dir().join(format!(
            "aether1_localserver_{name}_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(path).unwrap()
    }

    /// The default alone fixes nothing for anybody already running AETHER1: the settings
    /// pane writes every field on every Save, so a stored `false` sits on every install
    /// where Settings has been opened once, and a stored value beats a default.
    #[test]
    fn an_install_with_autostart_stored_off_is_switched_on_once() {
        let db = temp_db("autostart_stored_off");
        db.set_setting("autostart_ollama", &serde_json::Value::Bool(false))
            .unwrap();

        apply_autostart_default(&db);
        assert!(db.get_setting_bool("autostart_ollama", false));

        // And an operator who turns it off *after* the flip keeps it off, because the
        // marker is written whether or not the flip was needed.
        db.set_setting("autostart_ollama", &serde_json::Value::Bool(false))
            .unwrap();
        apply_autostart_default(&db);
        assert!(
            !db.get_setting_bool("autostart_ollama", true),
            "a deliberate choice made after the migration is not undone by it"
        );
    }

    fn server(port: u16, api: LocalApi, models: &[&str]) -> LocalServer {
        let models: Vec<String> = models.iter().map(|m| m.to_string()).collect();
        LocalServer {
            endpoint: match api {
                LocalApi::Native => format!("http://127.0.0.1:{port}"),
                LocalApi::OpenAi => format!("http://127.0.0.1:{port}/v1"),
            },
            port,
            api,
            provider_key: api.provider_key(),
            label: format!("Local server on port {port}"),
            models: models.clone(),
        }
    }

    #[test]
    fn nothing_listening_changes_nothing() {
        assert_eq!(choose(&[], "offline", "", ""), None);
    }

    /// The case Trident actually had: Ollama running, AETHER1 freshly installed and
    /// configured for nothing at all. This is the whole point of the module.
    #[test]
    fn an_unconfigured_aether1_adopts_the_server_on_the_machine() {
        let found = [server(11434, LocalApi::Native, &["qwen3:8b"])];
        let adoption = choose(&found, "offline", "", "").expect("a running server is adopted");
        assert_eq!(adoption.provider, "ollama");
        assert_eq!(adoption.endpoint, "http://127.0.0.1:11434");
        assert_eq!(adoption.model.as_deref(), Some("qwen3:8b"));
    }

    /// A deliberate cloud provider is a choice. An Ollama appearing on the machine does not
    /// revoke it, and switching would be both surprising and, for somebody mid-conversation
    /// with a cloud model, a downgrade they did not ask for.
    #[test]
    fn a_cloud_provider_is_never_overridden() {
        let found = [server(11434, LocalApi::Native, &["qwen3:8b"])];
        assert_eq!(
            choose(&found, "anthropic", "", "claude-opus-5"),
            None,
            "a configured cloud provider is left exactly as it is"
        );
    }

    /// Ollama answers `/v1/models` as well as `/api/tags`, so both shapes are genuinely
    /// available on the same port. The native one is adopted, because it is the only one
    /// that carries native tools and `num_ctx` -- the fault that cost four PRs to find.
    #[test]
    fn the_native_api_is_preferred_over_the_openai_shape() {
        let found = [
            server(1234, LocalApi::OpenAi, &["some-model"]),
            server(11434, LocalApi::Native, &["qwen3:8b"]),
        ];
        let adoption = choose(&found, "offline", "", "").expect("one of them is adopted");
        assert_eq!(adoption.provider, "ollama");
        assert_eq!(adoption.endpoint, "http://127.0.0.1:11434");
    }

    /// A server with nothing loaded cannot answer a question, so one that can wins -- even
    /// though it is the OpenAI-compatible kind the rule above otherwise demotes.
    #[test]
    fn a_server_with_a_model_beats_an_empty_one_whatever_its_api() {
        let found = [
            server(11434, LocalApi::Native, &[]),
            server(1234, LocalApi::OpenAi, &["mistral-7b"]),
        ];
        let adoption = choose(&found, "offline", "", "").expect("the loaded server is adopted");
        assert_eq!(adoption.endpoint, "http://127.0.0.1:1234/v1");
        assert_eq!(adoption.model.as_deref(), Some("mistral-7b"));
    }

    /// An empty server is still worth adopting when it is all there is: that is the address
    /// to use the moment a model is pulled, and leaving the provider on "offline" until then
    /// means the pull appears to change nothing.
    #[test]
    fn a_server_with_no_model_is_still_worth_pointing_at() {
        let found = [server(11434, LocalApi::Native, &[])];
        let adoption = choose(&found, "offline", "", "qwen3:8b").expect("adopted anyway");
        assert_eq!(adoption.provider, "ollama");
        assert_eq!(
            adoption.model.as_deref(),
            Some("qwen3:8b"),
            "a model the operator chose is not blanked by a server that lists none"
        );
    }

    /// The regression this file's `server_context_tokens` comment is about. The figure must
    /// not exceed what the per-request path itself considers safe, or the server-wide
    /// default is the looser of two numbers deciding the same thing -- which is how a 27B
    /// ended up offloading to CPU and taking 167 seconds on a greeting.
    #[test]
    fn the_server_wide_context_does_not_exceed_what_a_request_would_ask_for() {
        let server_wide = u64::from(server_context_tokens());
        assert!(
            server_wide <= crate::llm::providers::local_context_window(Some(1_000_000)),
            "the server-wide default ({server_wide}) must not be larger than the ceiling \
             the per-request path clamps to"
        );
        assert!(
            server_wide > 4096,
            "and must still be above Ollama's 4096 default, which is what overflowed"
        );
    }

    /// Five questions in a row in a real chat log were answered with "ollama has no model
    /// by that name" while the server sat there listing models. Not choosing is not
    /// deference when the alternative is five hard failures.
    #[test]
    fn a_configured_model_the_server_does_not_have_is_replaced() {
        let found = [server(11434, LocalApi::Native, &["qwen3:8b"])];
        let adoption = choose(
            &found,
            "ollama",
            "http://127.0.0.1:11434",
            "a-model-never-pulled",
        )
        .expect("a model that is not installed is a gap to fill");
        assert_eq!(adoption.model.as_deref(), Some("qwen3:8b"));
        assert_eq!(adoption.endpoint, "http://127.0.0.1:11434");
        assert!(
            adoption.why.contains("not installed"),
            "the reason says which model was missing: {}",
            adoption.why
        );
    }

    /// The counterpart: a server that lists nothing has not contradicted anything, so the
    /// address being right still means there is nothing to do. Switching here would blank a
    /// deliberate choice on the strength of a server that simply has no model loaded yet.
    #[test]
    fn an_empty_server_at_the_configured_address_is_still_left_alone() {
        let found = [server(11434, LocalApi::Native, &[])];
        assert_eq!(
            choose(&found, "ollama", "http://127.0.0.1:11434", "qwen3:8b"),
            None
        );
    }

    #[test]
    fn a_configured_and_running_server_is_left_alone() {
        let found = [server(11434, LocalApi::Native, &["qwen3:8b"])];
        assert_eq!(
            choose(&found, "ollama", "http://127.0.0.1:11434", "qwen3:8b"),
            None
        );
    }

    /// Trailing slashes are the sort of difference that would otherwise rewrite the same
    /// address back onto itself on every single start.
    #[test]
    fn a_trailing_slash_is_not_a_different_address() {
        let found = [server(11434, LocalApi::Native, &["qwen3:8b"])];
        assert_eq!(
            choose(&found, "ollama", "http://127.0.0.1:11434/", "qwen3:8b"),
            None
        );
    }

    /// The second reason to act: the address in settings is stale -- the operator moved the
    /// server, or copied settings from another machine -- and a working one is right there.
    #[test]
    fn a_configured_address_that_nothing_answers_is_replaced() {
        let found = [server(11434, LocalApi::Native, &["qwen3:8b"])];
        let adoption = choose(&found, "ollama", "http://127.0.0.1:9999", "qwen3:8b")
            .expect("a dead address is a gap to fill");
        assert_eq!(adoption.endpoint, "http://127.0.0.1:11434");
        assert!(
            adoption.why.contains("did not answer"),
            "the reason names the stale address: {}",
            adoption.why
        );
    }

    /// A deliberate local address that is answering outranks the ranking: a second server
    /// on another port is not a reason to move somebody off the one they chose.
    #[test]
    fn a_working_configured_server_outranks_a_better_one() {
        let found = [
            server(1234, LocalApi::OpenAi, &["mistral-7b"]),
            server(11434, LocalApi::Native, &["qwen3:8b"]),
        ];
        assert_eq!(
            choose(&found, "lmstudio", "http://127.0.0.1:1234/v1", "mistral-7b"),
            None,
            "the operator pointed at 1234 on purpose"
        );
    }

    /// The mislabel from PR #248 in its other form: provider and address disagree about
    /// which shape the server speaks. `adopt_ollama_behind_openai_label` fixes the case
    /// where the `/v1` address is an Ollama; this catches it from the other end, where the
    /// sweep has found the same port speaking the native API.
    #[test]
    fn a_provider_that_disagrees_with_the_running_server_is_corrected() {
        let found = [server(11434, LocalApi::Native, &["qwen3:8b"])];
        let adoption = choose(&found, "lmstudio", "http://127.0.0.1:11434/v1", "qwen3:8b")
            .expect("the /v1 address is not what is listening");
        assert_eq!(adoption.provider, "ollama");
        assert_eq!(adoption.endpoint, "http://127.0.0.1:11434");
    }

    #[test]
    fn the_model_is_replaced_only_when_the_server_does_not_have_it() {
        let found = [server(11434, LocalApi::Native, &["llama3:8b", "qwen3:8b"])];
        let kept = choose(&found, "offline", "", "qwen3:8b").expect("adopted");
        assert_eq!(
            kept.model.as_deref(),
            Some("qwen3:8b"),
            "a model the server lists is kept even though it is not first"
        );

        let replaced = choose(&found, "offline", "", "a-model-that-was-removed").expect("adopted");
        assert_eq!(replaced.model.as_deref(), Some("llama3:8b"));
    }
}
