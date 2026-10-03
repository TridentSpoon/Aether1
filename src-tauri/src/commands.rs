// Plain functions behind the Tauri commands in main.rs, extracted so the axum server in
// server.rs can call the exact same logic instead of duplicating it. Each function here is
// what a #[tauri::command] wrapper's body used to be, verbatim -- the wrappers in main.rs are
// now one-line pass-throughs.

use std::path::PathBuf;

use serde_json::Value;

use crate::llm::{self, ActionRecord, LlmEngine};
use crate::model_scanner;
use crate::project_root;
use crate::setup;
use crate::tools;

/// Told once, before a word of the reply exists, when flow mode is passing this question
/// to somebody else. Separate from the delta sink because it is a different speaker: the
/// node on its way out, not the one about to answer.
pub type HandoverSink<'a> = &'a mut dyn FnMut(&llm::flow::Handover);

pub fn generate_response(
    engine: &LlmEngine,
    prompt: String,
    session_id: Option<String>,
) -> Result<Value, String> {
    generate_response_streamed(engine, prompt, session_id, &mut |_| {}, &mut |_| {})
}

/// generate_response, with each piece of the reply handed to `sink` as it arrives. The
/// returned value is identical either way -- the sink is an extra, not an alternative, so
/// history and usage accounting can't differ between a streamed and an unstreamed turn.
pub fn generate_response_streamed(
    engine: &LlmEngine,
    prompt: String,
    session_id: Option<String>,
    sink: llm::Sink,
    on_handover: HandoverSink,
) -> Result<Value, String> {
    if prompt.trim().is_empty() {
        return Err("Empty message".to_string());
    }
    let session_id = valid_session_id(session_id)?;

    engine.add_message(&session_id, "user", &prompt);

    // Flow mode, before a word is generated: the question has to reach whoever it belongs
    // to, and the hand-off line belongs to the node on its way out, so it is said first and
    // recorded first. A failure to persist the switch is not a reason to refuse the answer
    // -- the current node simply keeps it.
    let handover = llm::flow::consider(engine.db(), &prompt).filter(|h| {
        match llm::flow::apply(engine.db(), h) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("[AETHER1] Could not hand the question over: {e}");
                false
            }
        }
    });
    if let Some(h) = &handover {
        on_handover(h);
        engine.add_message(&session_id, &h.speaker_name().to_lowercase(), &h.line);
    }

    // Opened and drained around the one call that reads the vault, so the notes reported
    // belong to this answer and no other. Both are on this thread, which is the whole
    // reason the record can be a thread-local -- see vault::consulted.
    crate::vault::consulted::begin();
    let reply = engine.generate_response_streamed(&prompt, &session_id, sink);
    let notes = crate::vault::consulted::taken();
    let agent_name = engine.agent_name();
    engine.add_message(&session_id, &agent_name.to_lowercase(), &reply);

    // The exchange goes into the vault as well as the database, because the two are for
    // different things: the database is the transcript, and the vault is the part the
    // operator can open, edit and keep. Failure is reported here and nowhere else -- a
    // reply that was generated has been generated, and a full disk is not a reason to
    // replace it with an error.
    if let Err(e) = crate::vault::journal_exchange(engine.db(), &agent_name, &prompt, &reply) {
        if crate::vault::journal_enabled(engine.db()) {
            eprintln!("[AETHER1] Could not write the conversation to the vault: {e}");
        }
    }

    Ok(serde_json::json!({
        "reply": reply,
        "agent_name": agent_name,
        "notes": crate::vault::consulted::to_json(&notes),
        "handover": handover.as_ref().map(handover_json),
    }))
}

/// Tries a provider/model/endpoint/key combination with one trivial call before it's ever
/// saved -- the values come straight from the Settings form, not from what's persisted, so
/// a mistake is caught before Save rather than discovered on the next real chat message.
pub fn test_llm_connection(
    engine: &LlmEngine,
    provider: String,
    model: String,
    endpoint: String,
    api_key: String,
) -> Result<String, String> {
    engine.test_connection(&provider, &model, &endpoint, &api_key)
}

pub fn agent_genesis(engine: &LlmEngine, purpose: String) -> Result<Value, String> {
    if purpose.trim().is_empty() {
        return Err("Please provide a purpose description".to_string());
    }
    let identity = engine.generate_identity_from_purpose(&purpose);
    engine.add_message("default", &identity.name.to_lowercase(), &identity.greeting);

    Ok(serde_json::json!({
        "name": identity.name,
        "callsign": identity.callsign,
        "persona": identity.persona_directive,
        "voice": identity.voice,
        "greeting": identity.greeting,
    }))
}

/// Where this machine is on the road to having a model, and the single next thing to do.
///
/// The three facts the advisor needs are gathered here rather than inside it: a fresh probe
/// of the machine, how much memory it has, and whether the settings already name a real
/// provider and model. Keeping `setup::advise` a pure function of those three is what lets
/// every case it can land in be tested without a model server anywhere near the test.
pub fn setup_advice(engine: &LlmEngine) -> Value {
    let scan = model_scanner::scan_all();
    let ram_total_gb = llm::Telemetry::snapshot().ram_total_gb;

    // "Configured" means both halves are filled in. A provider with no model is the state a
    // half-finished pass through the wizard leaves behind, and it answers nothing.
    let provider = engine.db().get_setting_string("llm_provider", "offline");
    let model = engine.db().get_setting_string("llm_model", "");
    let configured = provider != "offline" && !model.trim().is_empty();

    serde_json::to_value(crate::setup::advise(
        &scan,
        ram_total_gb,
        crate::gpu::cached(),
        configured,
    ))
    .unwrap_or_else(|_| serde_json::json!({}))
}

/// Starts a model download and returns immediately with its first state.
///
/// Unlike `pull_model`, which spawns the CLI and can only say "started", this drives
/// Ollama's own streaming pull on a background thread and records what it reports, so
/// `download_status` has real bytes to show. Several can run at once; `downloads::start`
/// decides how many is too many.
pub fn start_download(engine: &LlmEngine, model_name: String, endpoint: String) -> Value {
    if crate::local_only::enabled(engine.db()) {
        return serde_json::json!({
            "ok": false,
            "message": crate::local_only::refusal("no model was downloaded"),
        });
    }

    // An empty endpoint means the caller had no server to name, which is the state the
    // wizard is in before it has found one. The default port is the only guess worth
    // making, and a wrong guess surfaces as a connection error rather than a silent stall.
    let endpoint = if endpoint.trim().is_empty() {
        "http://localhost:11434".to_string()
    } else {
        endpoint
    };

    match crate::downloads::start(&endpoint, &model_name) {
        Ok(download) => serde_json::json!({ "ok": true, "download": download }),
        Err(message) => serde_json::json!({ "ok": false, "message": message }),
    }
}

/// Every download this session knows about, running and finished alike.
///
/// Read-only and cheap by design: the HUD asks for this about once a second while a bar is
/// on screen, and it must never be the reason a download slows down.
pub fn download_status() -> Value {
    serde_json::json!({
        "downloads": crate::downloads::snapshot(),
        "max_concurrent": crate::downloads::MAX_CONCURRENT,
    })
}

/// Clears one finished or failed row out of the list. Never touches a running one.
pub fn forget_download(model_name: String) -> Value {
    serde_json::json!({ "ok": crate::downloads::forget(&model_name) })
}

/// The voices Aether1 can fetch, and which of them are already here.
pub fn voice_catalogue() -> Value {
    serde_json::json!({ "voices": crate::voice_download::catalogue() })
}

/// Starts fetching one voice by name, returning immediately with its first state.
///
/// The name is the whole input, and `voice_download::start` refuses any that is not in its
/// own table -- so nothing arriving from the HUD becomes part of a URL or a path. The
/// local-only refusal lives there too rather than here, because this is not the only door:
/// keeping it beside the thing that opens the connection means it cannot be walked around.
pub fn start_voice_download(engine: &LlmEngine, voice: String) -> Value {
    match crate::voice_download::start(engine.db(), &voice) {
        Ok(fetch) => serde_json::json!({ "ok": true, "download": fetch }),
        Err(message) => serde_json::json!({ "ok": false, "message": message }),
    }
}

/// Every voice download this session knows about. Polled about once a second while a bar is
/// on screen, so it stays a cheap read for the same reason `download_status` does.
pub fn voice_download_status() -> Value {
    serde_json::json!({ "downloads": crate::voice_download::snapshot() })
}

/// Clears one finished or failed voice row. Never touches a running one.
pub fn forget_voice_download(voice: String) -> Value {
    serde_json::json!({ "ok": crate::voice_download::forget(&voice) })
}

/// The speakers and microphones this machine has, as the operating system names them.
///
/// Asked for every time the panel opens rather than cached: a headset plugged in while
/// AETHER1 was running is the whole reason somebody opens this list.
pub fn audio_devices() -> Value {
    serde_json::json!({ "devices": crate::audio_devices::scan() })
}

/// Starts the local model server when it is installed but not running.
///
/// This is the one case where the app can fix a missing dependency itself rather than
/// telling someone to go and fix it: `ollama serve` is what the service would have run, it
/// needs no administrator, and it is already on this machine or this does nothing.
///
/// Deliberately narrow. There is no shell, no argument comes from the caller, and the
/// binary is the one `which` finds under exactly the name `ollama` -- so the whole of what
/// this can be made to run is "the ollama already installed here, serving". It is reachable
/// from the HUD and from the setup wizard; it is not a tool, so nothing the companion says
/// in a conversation can reach it.
pub fn start_local_server(engine: &LlmEngine) -> Value {
    start_local_server_tracked(engine).0
}

/// Same as `start_local_server`, but also hands back the spawned `Child` (when one was
/// actually spawned) instead of dropping it. `start_local_server` itself just discards that
/// half for the manual "start" button, which has nothing to do with a process afterwards --
/// but `background_services` needs the handle so it can stop the very same process later
/// (for Game Mode), without ever touching an Ollama the operator started independently.
pub fn start_local_server_tracked(engine: &LlmEngine) -> (Value, Option<std::process::Child>) {
    // Starting a server that then talks to a registry is not itself a network trip, but
    // local-only mode is about what the operator has asked the app not to do on their
    // behalf, and starting daemons uninvited is squarely in that spirit.
    if crate::local_only::enabled(engine.db()) {
        return (
            serde_json::json!({
                "ok": false,
                "message": crate::local_only::refusal("no server was started"),
            }),
            None,
        );
    }

    let Ok(binary) = which::which("ollama") else {
        return (
            serde_json::json!({
                "ok": false,
                "message": "There is no `ollama` command on this machine to start. It needs \
                            installing first.",
            }),
            None,
        );
    };

    // If something already answers, starting a second one would fail on the port and look
    // like a broken button. Saying so is the more useful answer.
    if model_scanner::scan_all().has_local_provider {
        return (
            serde_json::json!({
                "ok": true,
                "message": "A model server is already running on this computer.",
            }),
            None,
        );
    }

    match std::process::Command::new(&binary)
        .arg("serve")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        // Started, not proven: it takes a moment to bind the port, and whoever called this
        // confirms by probing rather than by trusting this answer. That is the same rule
        // the rest of the setup path follows -- the machine is asked, never assumed.
        Ok(child) => (
            serde_json::json!({
                "ok": true,
                "message": "Starting the model server. Give it a few seconds.",
            }),
            Some(child),
        ),
        Err(e) => (
            serde_json::json!({
                "ok": false,
                "message": format!("Could not start the model server: {e}"),
            }),
            None,
        ),
    }
}

/// Downloads a model through the local Ollama, which fetches it from Ollama's registry --
/// the one deliberate internet round trip left in the app, and the reason local-only mode
/// has to have an opinion about it. Refusing is the honest answer: the mode says nothing
/// leaves this machine, and a model pull is a download.
pub fn pull_model(engine: &LlmEngine, model_name: String) -> model_scanner::PullResult {
    if crate::local_only::enabled(engine.db()) {
        return model_scanner::PullResult {
            status: model_scanner::PullStatus::Error,
            message: crate::local_only::refusal("no model was downloaded"),
        };
    }
    let model_name = if model_name.trim().is_empty() {
        "llama3.2:1b".to_string()
    } else {
        model_name
    };
    model_scanner::pull_model(&model_name)
}

pub fn static_info() -> Value {
    let os_name = sysinfo::System::long_os_version()
        .or_else(sysinfo::System::name)
        .unwrap_or_else(|| "Unknown OS".to_string());
    serde_json::json!({
        "distro": os_name,
        "architecture": std::env::consts::ARCH,
    })
}

/// The one gate every session id passes through before it reaches SQL.
///
/// It refuses rather than repairs. A sanitiser that strips the characters it doesn't like
/// turns a bad id into a *different valid one*, and the conversation the operator asked
/// for silently becomes somebody else's -- reading one transcript while writing another.
/// An error is the honest answer. `None` still means the original "default", so every
/// caller that predates sessions keeps working untouched.
///
/// The character set is deliberately narrower than SQLite needs: ids are generated here,
/// never typed, so there is nothing to lose by allowing only what a generated id contains.
pub fn valid_session_id(session_id: Option<String>) -> Result<String, String> {
    let Some(raw) = session_id else {
        return Ok(DEFAULT_SESSION.to_string());
    };
    let id = raw.trim();
    if id.is_empty() {
        return Ok(DEFAULT_SESSION.to_string());
    }
    if id.len() > 64 {
        return Err("Conversation id is too long".to_string());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Conversation id has characters that aren't allowed".to_string());
    }
    Ok(id.to_string())
}

/// The conversation everything landed in before conversations existed. Kept as the
/// fallback so the history already on disk stays reachable instead of being orphaned.
pub const DEFAULT_SESSION: &str = "default";

/// How many conversations the history list will show. A hard ceiling rather than paging:
/// the list is for finding the conversation you remember, and past a few hundred rows
/// nobody is scrolling anyway.
const MAX_SESSIONS: u32 = 200;

/// Every conversation with something in it, newest first.
///
/// Note what is *not* here: no tool, and nothing in the settings allowlist. The companion
/// cannot list, open, rename, delete or switch conversations. A session is a privacy
/// boundary as much as a convenience -- the model is given the history of the session it
/// is answering in and has no way to reach across to another one, or even to learn that
/// another one exists.
pub fn list_sessions(engine: &LlmEngine) -> Result<Vec<llm::SessionSummary>, String> {
    engine
        .db()
        .list_sessions(MAX_SESSIONS)
        .map_err(|e| e.to_string())
}

/// Mints an id for a new conversation. Nothing is written: a conversation starts existing
/// when something is said in it, which is why the history list is derived from messages.
///
/// The id is the clock in nanoseconds, in base 36. There is no `uuid` or `rand` crate in
/// this build and this does not need one -- ids are minted by a single operator on a
/// single machine, one at a time, and are never guessed at, never used as a secret, and
/// never shared between machines. Uniqueness against a monotonic-enough clock is the whole
/// requirement, and the id is not used as an authorisation token anywhere.
pub fn new_session() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut n = nanos;
    let mut out = Vec::new();
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    let tail = String::from_utf8(out).unwrap_or_else(|_| "0".to_string());
    format!("c{tail}")
}

pub fn rename_session(
    engine: &LlmEngine,
    session_id: Option<String>,
    title: String,
) -> Result<(), String> {
    let session_id = valid_session_id(session_id)?;
    if title.chars().count() > 200 {
        return Err("That name is too long".to_string());
    }
    engine
        .db()
        .set_session_title(&session_id, &title)
        .map_err(|e| e.to_string())
}

/// Deletes a conversation and everything said in it.
///
/// "default" is refused, because it is the fallback every session-unaware caller lands in:
/// deleting it doesn't remove a conversation so much as empty the one the app falls back
/// to. Clearing it is still available through `clear_messages`, which is the honest name
/// for what that would be.
pub fn delete_session(engine: &LlmEngine, session_id: String) -> Result<(), String> {
    let session_id = valid_session_id(Some(session_id))?;
    if session_id == DEFAULT_SESSION {
        return Err("The first conversation can be cleared, but not deleted".to_string());
    }
    engine
        .db()
        .delete_session(&session_id)
        .map_err(|e| e.to_string())
}

pub fn get_messages(
    engine: &LlmEngine,
    limit: Option<u32>,
    session_id: Option<String>,
) -> Result<Vec<llm::Message>, String> {
    let session_id = valid_session_id(session_id)?;
    Ok(engine
        .db()
        .get_messages(&session_id, limit.unwrap_or(50))
        .unwrap_or_default())
}

pub fn clear_messages(engine: &LlmEngine, session_id: Option<String>) -> Result<(), String> {
    let session_id = valid_session_id(session_id)?;
    engine
        .db()
        .clear_history(&session_id)
        .map_err(|e| e.to_string())
}

/// Throws away the model-speed scoreboard.
///
/// The readings describe a machine, and a machine changes -- a new graphics card, a
/// different quantisation of the same model, an Ollama release that got faster. Without
/// this the old numbers keep being averaged in with the new ones for as long as the
/// database survives, and the scoreboard slowly becomes a record of a computer that no
/// longer exists.
pub fn reset_benchmarks(engine: &LlmEngine) -> Result<(), String> {
    engine.db().clear_benchmarks().map_err(|e| e.to_string())
}

/// The persona catalogue Settings renders its list from. The list itself depends on nothing
/// but the enum, but it goes through the same two transports as everything else rather than
/// being written out a second time in the HTML, which is how the list and the behaviour
/// would come to disagree.
///
/// Each row carries both answers about voice: `voice`/`local_voice` are what this avatar
/// actually speaks in, and `default_voice`/`default_local_voice` are the identity table's
/// own, so the avatar's pane can show "default: X" beside a changed choice and know whether
/// there is anything to reset. They are equal on an avatar nobody has touched.
pub fn list_personas(engine: &LlmEngine) -> Value {
    let db = engine.db();
    let mut rows = llm::Persona::catalogue();
    if let Value::Array(rows) = &mut rows {
        for row in rows.iter_mut() {
            let Some(key) = row.get("key").and_then(Value::as_str).map(str::to_string) else {
                continue;
            };
            let chosen = crate::persona_voice::choice(db, &key);
            row["default_voice"] = row["voice"].clone();
            row["default_local_voice"] = row["local_voice"].clone();
            row["voice_customised"] = Value::Bool(!chosen.is_empty());
            if let Some(voice) = chosen.voice {
                row["voice"] = Value::String(voice);
            }
            if let Some(voice) = chosen.local_voice {
                row["local_voice"] = Value::String(voice);
            }
        }
    }
    rows
}

/// The voices an avatar can be given: the cloud names, and the Piper catalogue with whether
/// each one is on this machine.
pub fn voice_pickers() -> Value {
    crate::persona_voice::pickers()
}

/// Gives one avatar a voice of the operator's choosing. An empty string for either half
/// means "back to the identity table's" for that half alone.
pub fn set_persona_voice(
    engine: &LlmEngine,
    persona: String,
    voice: Option<String>,
    local_voice: Option<String>,
) -> Result<Value, String> {
    crate::persona_voice::set(
        engine.db(),
        &persona,
        voice.as_deref(),
        local_voice.as_deref(),
    )?;
    Ok(list_personas(engine))
}

/// How this machine says the words it keeps getting wrong.
pub fn pronunciations(engine: &LlmEngine) -> Value {
    serde_json::json!({
        "words": crate::speech_words::all(engine.db()),
        "max_entries": crate::speech_words::MAX_ENTRIES,
        "max_length": crate::speech_words::MAX_LEN,
    })
}

/// Stores the whole list at once rather than one row at a time.
///
/// The pane edits a table, and a table has no stable identity per row -- a save per keystroke
/// on a row whose word is half typed would store `Aeth` as a rule. The reply is the list as
/// stored, which is what the pane redraws from: it has been trimmed and de-duplicated on the
/// way in, so a pane that trusted its own copy would be showing something speech disagrees
/// with.
pub fn set_pronunciations(
    engine: &LlmEngine,
    words: Vec<crate::speech_words::Say>,
) -> Result<Value, String> {
    crate::speech_words::set(engine.db(), words)?;
    Ok(pronunciations(engine))
}

/// Puts one avatar back to the voices it was written with.
pub fn clear_persona_voice(engine: &LlmEngine, persona: String) -> Result<Value, String> {
    crate::persona_voice::clear(engine.db(), &persona)?;
    Ok(list_personas(engine))
}

pub fn get_settings(engine: &LlmEngine) -> Value {
    let mut settings = engine
        .db()
        .get_all_settings()
        .unwrap_or(serde_json::json!({}));
    let defaults = serde_json::json!({
        "agent_name": "HALCY",
        "llm_provider": "offline",
        "llm_model": "halcy-core",
        "llm_endpoint": "http://localhost:11434",
        "llm_api_key": "",
        "persona_type": "default",
        "custom_directive": "",
        "voice_name": llm::DEFAULT_VOICE,
        "enable_sfx": true,
        "auto_speak": true,
        "tools_enabled": false,
        "command_allowlist": [],
        "tts_engine": "auto",
        "tts_local_voice": "",
        // Which speaker and microphone to use, as audio_devices.rs names them. Empty is
        // the ordinary value and means "whatever the system picks" -- the behaviour
        // everything had before there was anywhere to choose. The label is stored beside
        // the id because the two lists this is matched against (the operating system's
        // and the browser's) do not share identifiers; see resolveAudioDevice in app.js.
        "audio_output_device": "",
        "audio_output_label": "",
        "audio_input_device": "",
        "audio_input_label": "",
        "stt_model_path": "",
        "stt_language": "en",
        "vault_path": "",
        // On unless it has been switched off: see vault::journal_enabled. Stated here as
        // well so the box in Settings starts ticked on a fresh install rather than
        // starting blank and describing the opposite of what the vault actually does.
        "vault_journal": true,
        // Whether `graft ask` may re-index the files that changed before answering. On
        // unless switched off: a stale graph points the model at line numbers that have
        // moved, which is worse than the second the refresh costs. See graft.rs.
        "graft_auto_refresh": true,
        // The HUD saves this alongside its own browser copy so the two agree; without a
        // default the key simply wouldn't come back on a fresh install, and the page would
        // have nothing to reconcile against.
        "color_theme": "",
        "hotkey_toggle": crate::hotkey::DEFAULT_TOGGLE,
        "desktop_sprite_enabled": false,
        "local_only": false,
        // AETHER CODE's three read permissions, granted unless switched off. See
        // code_perms.rs for why the default is on and why there is no write permission
        // listed beside them.
        "code_perm_system": true,
        "code_perm_github": true,
        "code_perm_internet": true,
        // Startup & Performance: launch AETHER1 at login, auto-start Ollama when AETHER1
        // starts, whether the voice self-test speaks its confirmation phrase or checks
        // silently, and whether Game Mode is currently switched on (persisted so it
        // survives a restart rather than silently reverting).
        "autostart_app": false,
        "autostart_ollama": false,
        // Minutes with no local reply before the model server AETHER1 started is stopped
        // again; 0 keeps it running. See background_services::IDLE_SETTING.
        "ollama_idle_minutes": crate::background_services::DEFAULT_IDLE_MINUTES,
        // Remote & LAN: whether AETHER1 puts this machine on the network as it starts.
        // Off unless asked for -- see lan.rs.
        "lan_autostart": false,
        "voice_startup_audible": true,
        "game_mode": false,
        // The two switches around the sandbox `run` spawns commands in. Both off: a test
        // suite does not need the network, and a machine that cannot confine a command
        // refuses to run one until the operator says otherwise -- see code_sandbox.rs.
        "code_run_network": false,
        "code_run_unconfined": false,
    });
    if let (Some(settings_obj), Some(defaults_obj)) =
        (settings.as_object_mut(), defaults.as_object())
    {
        for (key, value) in defaults_obj {
            settings_obj
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }
    // Alongside the settings, not inside them: the operating system is not a preference,
    // it is a fact about the machine, and putting it in `settings` would make it look
    // saveable. It rides along here because the Settings page needs it for the same
    // reason the setup wizard does -- to describe *this* machine's folders and programs
    // rather than a guess drawn from whatever browser is pointed at it.
    // `sandbox` rides along for the same reason as `os`, and is the more important of the
    // two: whether `run` is actually confined on this machine is a fact about the machine,
    // not a preference, and the Settings page has to be able to say so plainly rather than
    // describing a boundary that may not be there.
    let sandbox = crate::code_sandbox::detect();
    // And the autonomy level, which is a fact about the *project* rather than this machine
    // or these settings -- it lives in the project's own `.aether/policy.json`. It rides
    // here so the Settings page can show what the nominated project is trusted with without
    // a second round trip. No project folder set: `level` is null, and the page says so
    // rather than offering a choice that would have nowhere to be written.
    let autonomy = match crate::code_workspace::root(engine.db()) {
        Ok(root) => {
            let (level, matches) = crate::code_policy::effective(&root, engine.db());
            serde_json::json!({
                "root": root.to_string_lossy(),
                "level": level.key(),
                "matches": matches,
                "trusted": crate::code_policy::is_trusted(&root),
                "shares": crate::code_policy::mounts(&root)
                    .into_iter()
                    .map(|mount| serde_json::json!({
                        "path": mount.path.to_string_lossy(),
                        "write": mount.write,
                    }))
                    .collect::<Vec<_>>(),
            })
        }
        Err(_) => serde_json::json!({ "level": Value::Null }),
    };
    serde_json::json!({
        "settings": settings,
        "os": setup::Os::current(),
        "sandbox": {
            "confines": sandbox.confines(),
            "description": sandbox.description(),
        },
        "autonomy": autonomy,
        // The four levels themselves, so the page describes them in the same words the CLI
        // does rather than keeping its own copy of them.
        "levels": crate::code_policy::ALL
            .iter()
            .map(|level| serde_json::json!({
                "key": level.key(),
                "title": level.title(),
                "description": level.description(),
            }))
            .collect::<Vec<_>>(),
    })
}

/// Sets the nominated project's autonomy level -- the Settings page's twin of `aether1 code
/// level <name>`. Both go through `code_policy::set_level`, so the project's file and the
/// switches it means move together whichever surface asked.
pub fn code_set_level(engine: &LlmEngine, level: &str) -> Result<Value, String> {
    let level = crate::code_policy::Level::from_key(level).ok_or_else(|| {
        format!(
            "{level:?} is not a level. The four are: {}",
            crate::code_policy::ALL
                .iter()
                .map(|l| l.key())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let root = crate::code_workspace::root(engine.db())?;
    crate::code_policy::set_level(&root, engine.db(), level)?;
    Ok(serde_json::json!({
        "level": level.key(),
        "title": level.title(),
        "description": level.description(),
    }))
}

pub fn save_settings(engine: &LlmEngine, settings: Value) -> Result<(), String> {
    let Some(map) = settings.as_object() else {
        return Err("settings payload must be a JSON object".to_string());
    };
    for (key, value) in map {
        engine
            .db()
            .set_setting(key, value)
            .map_err(|e| format!("could not save setting {key:?}: {e}"))?;
    }
    Ok(())
}

/// What the companion can currently do to the machine, and whether that is switched on.
/// The list is empty until the first tools are registered; the shape is stable from now
/// on so the UI and the prompt renderer can both be built against it.
pub fn tool_catalog(engine: &LlmEngine) -> Value {
    tools::catalog(engine.db(), tools::registry())
}

/// Opens the vault folder in the file manager on the machine actually running Aether1 --
/// the HUD's "which notes were loaded" caption exists precisely so there's somewhere to
/// point a click at. Over `--lan`, that's the server's machine, same as everything else
/// under [Local host] in IMPLEMENTATION.md's vocabulary table -- there is no way to hand a
/// folder-manager window to a remote browser, only to the desktop this process is on.
pub fn open_vault_folder(engine: &LlmEngine) -> Result<(), String> {
    crate::vault::open_folder(engine.db())
}

/// Every note in the vault, newest first, for the reader panel's list.
///
/// Read-only, all three of these. The vault is changed through the tools -- with the
/// consent path and undo behind them -- and a reader that could also write would be a
/// second door into the same folder that nothing had approved.
pub fn vault_notes(engine: &LlmEngine) -> Vec<crate::vault::reader::NoteSummary> {
    crate::vault::reader::notes(engine.db())
}

/// One note: its text, the links in it, and the notes pointing back at it.
pub fn vault_note(
    engine: &LlmEngine,
    name: &str,
) -> Result<crate::vault::reader::NoteView, String> {
    crate::vault::reader::read(engine.db(), name)
}

/// The vault as a graph, for drawing.
pub fn vault_graph(engine: &LlmEngine) -> crate::vault::reader::Graph {
    crate::vault::reader::graph(engine.db())
}

/// The same search the companion itself uses on the vault, handed to the operator.
///
/// Built into JSON here rather than derived on `search::Hit`, because the hit carries a
/// modified time that exists only to break ties between two equally good matches; it is
/// ranking machinery, not something the reader has any use for.
pub fn vault_search(engine: &LlmEngine, query: &str) -> Value {
    let results = crate::vault::search::search(engine.db(), query);
    serde_json::json!({
        "hits": results.hits.iter().map(|h| serde_json::json!({
            "note": h.note,
            "score": h.score,
            "heading": h.heading,
            "snippet": h.snippet,
        })).collect::<Vec<_>>(),
        "scanned": results.scanned,
        "partial": results.partial,
    })
}

/// The record of what the companion has actually done, newest first.
pub fn recent_actions(engine: &LlmEngine, limit: Option<u32>) -> Vec<ActionRecord> {
    engine
        .db()
        .recent_actions(limit.unwrap_or(50))
        .unwrap_or_default()
}

/// Actions waiting for the operator's answer.
pub fn pending_actions(engine: &LlmEngine) -> Vec<ActionRecord> {
    tools::consent::pending(engine.db(), tools::registry())
}

/// Approves and runs one proposed action.
pub fn approve_action(engine: &LlmEngine, id: i64) -> Result<Value, String> {
    let ctx = tools::ToolContext::new(engine.db());
    let result = tools::consent::approve(tools::registry(), &ctx, id, "operator")?;
    Ok(serde_json::json!({ "id": id, "result": result }))
}

/// Reverses an action that ran.
pub fn undo_action(engine: &LlmEngine, id: i64) -> Result<Value, String> {
    let ctx = tools::ToolContext::new(engine.db());
    let result = tools::consent::undo(tools::registry(), &ctx, id)?;
    Ok(serde_json::json!({ "id": id, "result": result }))
}

/// Declines one proposed action.
pub fn reject_action(engine: &LlmEngine, id: i64) -> Result<(), String> {
    tools::consent::reject(engine.db(), id)
}

/// Adds or removes a tool from the list the operator has stopped being asked about.
pub fn set_always_allowed(engine: &LlmEngine, tool: String, allowed: bool) -> Result<(), String> {
    tools::consent::set_always_allowed(engine.db(), tools::registry(), &tool, allowed)
}

/// What the selected persona reads without asking, and the folders the operator has added
/// to that.
///
/// Reported for the *selected* persona rather than a named one, for the same reason
/// `ToolContext::new` reads it from settings: there is one answer to "which persona is
/// running", and a panel that let you edit a different one's field than the one in use
/// would be a setting that appears not to work.
pub fn persona_access(engine: &LlmEngine) -> Value {
    let db = engine.db();
    let key = db.get_setting_string("persona_type", "default");
    let persona = llm::Persona::from_key(&key);
    serde_json::json!({
        "persona": persona.key(),
        "speciality": persona.short_name(),
        "declared_field": persona.domain().field(),
        "extra_roots": tools::domain::extra_roots(db, &persona),
        "max_extra_roots": tools::domain::MAX_EXTRA_ROOTS,
    })
}

/// Replaces the folders in the selected persona's field.
///
/// Returns what was accepted and what was refused rather than failing on the first bad
/// entry: someone correcting four folders should not lose the three that were right because
/// of the one that was mistyped.
pub fn set_persona_access(engine: &LlmEngine, paths: Vec<String>) -> Result<Value, String> {
    let db = engine.db();
    let key = db.get_setting_string("persona_type", "default");
    let persona = llm::Persona::from_key(&key);
    let (accepted, refused) = tools::domain::set_extra_roots(db, &persona, &paths)?;
    Ok(serde_json::json!({
        "persona": persona.key(),
        "speciality": persona.short_name(),
        "accepted": accepted,
        "refused": refused,
    }))
}

/// The chin bar's STATIC/FLOW state: whether hand-offs are on, and the line the selected
/// avatar belongs to. A `group` of null is what hides the toggle -- an avatar that belongs
/// to no line has nothing to flow to.
///
/// Shared by both transports so the browser HUD shows the same switch the native window
/// does; it used to exist only as a Tauri command, which left the toggle permanently
/// hidden in a browser rather than merely inactive.
pub fn flow_mode(engine: &LlmEngine) -> Value {
    let db = engine.db();
    let persona = llm::Persona::from_key(&db.get_setting_string("persona_type", "default"));
    let picked = llm::flow::line(db);
    serde_json::json!({
        "enabled": llm::flow::enabled(db),
        // The line hand-offs may move within: the one picked whole if there is one, and
        // otherwise the one the selected avatar belongs to.
        "group": picked.or_else(|| persona.group()),
        // Which of those two it is. `line` is what the Avatars pane draws as picked, and
        // it is what survives a hand-off -- `group` follows the avatar once it moves.
        "line": picked,
        // The lines that can be picked whole, so the pane offers the ones that can
        // actually pass a question around rather than every cast in the catalogue. The
        // eXcelsior Class (no personas yet) and Trace Protocols (one) are not in here.
        "lines": llm::Persona::flow_lines(),
    })
}

pub fn set_flow_mode(engine: &LlmEngine, enabled: bool) -> Result<(), String> {
    llm::flow::set_enabled(engine.db(), enabled)
}

/// Pick a whole line, or release it with `None`, and report the state the chin bar and the
/// Avatars pane should now show. `persona` is set when picking moved the operator to the
/// line's anchor, so the HUD knows to wear that avatar; it is null when they were already
/// standing inside the line.
pub fn set_flow_line(engine: &LlmEngine, line: Option<String>) -> Result<Value, String> {
    let db = engine.db();
    let moved = llm::flow::set_line(db, line.as_deref())?;
    let mut state = flow_mode(engine);
    state["persona"] = match &moved {
        Some(p) => Value::String(p.key().to_string()),
        None => Value::Null,
    };
    state["agent_name"] = match &moved {
        Some(p) => Value::String(p.avatar().unwrap_or("AETHER").to_string()),
        None => Value::Null,
    };
    Ok(state)
}

/// Shared by the Tauri `generate_speech_rust` command and the axum server's TTS-bundling
/// logic in /api/chat and /api/agent/genesis. Engine and voices come from settings, so the
/// operator's choice of local-or-cloud applies wherever speech is produced.
pub fn synthesize_speech(
    engine: &LlmEngine,
    text: &str,
    voice: Option<&str>,
) -> Result<PathBuf, String> {
    let db = engine.db();
    let cache_dir = project_root().join("backend").join("audio_cache");
    let configured_voice = db.get_setting_string("voice_name", llm::DEFAULT_VOICE);
    let configured_local_voice = db.get_setting_string("tts_local_voice", "");

    // The voice belongs to the avatar. Resolving it here, from whichever persona is
    // selected at the moment of speech, is what makes that true for every way the avatar
    // can change -- picked in Settings, forged from a purpose, handed over mid-conversation
    // by flow mode -- without any of them having to remember to write a voice setting, and
    // without overwriting the operator's own choice, which is still what a persona with no
    // voice of its own speaks in. An explicit `voice` argument (the voice test) still wins.
    // The operator can disagree with the identity table's choice, one avatar at a time
    // (persona_voice::set, from the avatar's own pane in Settings); `cloud_voice` and
    // `local_voice` answer with that choice where there is one and the table's where there
    // is not, so nothing below has to know which of the two it got.
    let persona_key = db.get_setting_string("persona_type", "default");
    let persona_voice = voice
        .map(str::to_string)
        .or_else(|| crate::persona_voice::cloud_voice(db, &persona_key))
        .unwrap_or(configured_voice);
    // Piper speaks a model file rather than a voice name, so an identity's local voice is
    // only usable once that model is on disk. A persona whose voice has not been downloaded
    // speaks in the one the operator installed rather than failing to speak at all.
    let local_voice = crate::persona_voice::local_voice(db, &persona_key)
        .and_then(|name| llm::tts::installed_catalogue_voice(&name))
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or(configured_local_voice);

    // The cloud voice is the one path that used to leave the machine without anyone
    // choosing it: with Piper absent, `auto` quietly sent the text of everything the
    // companion said to Microsoft. Local-only mode collapses that choice to Piper, and
    // when Piper is not installed it says so instead of reaching out.
    let local_only = crate::local_only::enabled(db);
    let tts_engine =
        llm::TtsEngine::from_key(&db.get_setting_string("tts_engine", "auto")).resolve(local_only);

    // How the operator says the words, applied on the way to whichever engine speaks. It is
    // read here, per clip, rather than held anywhere: the list is edited in Settings while
    // the app is running, and the loop this feature lives or dies by is type it, press the
    // test, listen.
    let speller = crate::speech_words::speller(db);

    llm::generate_speech_with(
        &cache_dir,
        text,
        tts_engine,
        Some(&persona_voice),
        Some(&local_voice),
        &speller,
    )
    .map_err(|why| {
        if local_only {
            format!(
                "{} ({why})",
                crate::local_only::refusal("the cloud voice was not used")
            )
        } else {
            why
        }
    })
}

/// Where the voice stands: what can speak, what can listen, and what to do about either.
/// Settings are read here so both transports get the same answer from the same source.
/// Whether AETHER1 itself is working, for the HUD's Diagnostics panel and for the HTTP route
/// behind it. `facts` carries what only the caller knows -- the desktop process fills it in,
/// a browser-served HUD passes what it can and the report says which rows nobody could answer.
pub fn doctor_report(engine: &LlmEngine, facts: crate::doctor::Facts) -> Value {
    let (observation, health) = crate::doctor::report(engine.db(), &facts);
    serde_json::json!({ "health": health, "observation": observation })
}

/// Makes one repair, having been asked to by a person pressing the button for it.
///
/// The names arrive as the strings the report handed out, and an unknown one is refused rather
/// than guessed at: this function's whole job is acting on the machine, and there is no shape
/// of "near enough" worth having here.
pub fn doctor_repair(
    engine: &LlmEngine,
    check: &str,
    repair: &str,
    in_app: Option<crate::doctor::InAppRepair>,
) -> Result<Value, String> {
    let check_id = crate::doctor::CheckId::from_key(check)
        .ok_or_else(|| format!("there is no check called {check:?}"))?;
    let repair_id: crate::doctor::RepairId = serde_json::from_value(serde_json::json!(repair))
        .map_err(|_| format!("there is no repair called {repair:?}"))?;

    // The repair the report offered for this check, recomputed from a fresh probe: a button
    // pressed ten minutes after the panel was drawn must not act on what was true then.
    let (observation, _) = crate::doctor::report(engine.db(), &crate::doctor::Facts::default());
    let offered = crate::doctor::repair_for(check_id, &observation);
    match offered {
        Some(offered) if offered.id == repair_id => {}
        Some(_) | None => {
            return Err(format!(
                "{} is not what AETHER1 would repair about {} now -- press Diagnose again and \
                 read what it says.",
                repair,
                check_id.title()
            ))
        }
    }

    let outcome = crate::doctor::apply_with(engine, check_id, repair_id, in_app)?;
    serde_json::to_value(outcome).map_err(|e| e.to_string())
}

/// Fixes everything AETHER1 can fix about itself, because somebody pressed the one button
/// that says so.
///
/// The button rather than the switch: this is the attended path, and pressing it is the
/// go-ahead for the whole pass in the same way pressing one repair is the go-ahead for that
/// repair. The unattended path is `SELF_REPAIR_SETTING` and lives in main.rs's startup
/// thread; it reaches the same `doctor::attend`, so there is one behaviour and not two.
pub fn doctor_attend(
    engine: &LlmEngine,
    facts: crate::doctor::Facts,
    in_app: Option<crate::doctor::InAppRepair>,
) -> Result<Value, String> {
    let attended = crate::doctor::attend(engine, &facts, in_app);
    serde_json::to_value(attended).map_err(|e| e.to_string())
}

/// Runs diagnostics and returns a structured report with simplified output.
/// Includes:
/// - Simplified voice-over format
/// - Status (NOMINAL|WARNING|CRITICAL)
/// - Filtered metrics (only those >75% usage/load)
/// - Deduplicated error logs with counts
/// - Flag for repair agent availability
pub fn run_diagnostics_rust(engine: &LlmEngine) -> Value {
    use std::collections::HashMap;

    let db = engine.db();
    let telemetry = llm::Telemetry::snapshot();
    let machine_nickname = db.get_setting_string("machine_nickname", "");
    let machine = crate::profile::machine_description(db);

    // Simplified voice-over
    let voice_over = if !machine_nickname.is_empty() {
        format!("Diagnostics for {}", machine_nickname)
    } else {
        format!("Diagnostics for {}", machine)
    };

    // Determine status based on telemetry
    let status = if telemetry.cpu_percent > 85.0 || telemetry.ram_percent > 90.0 {
        if telemetry.cpu_percent > 95.0 || telemetry.ram_percent > 95.0 {
            "CRITICAL"
        } else {
            "WARNING"
        }
    } else {
        "NOMINAL"
    };

    // Filter metrics - only show those >75%
    let mut filtered_metrics = serde_json::json!({});

    if telemetry.cpu_percent > 75.0 {
        filtered_metrics["cpu_percent"] =
            serde_json::json!(format!("{:.1}%", telemetry.cpu_percent));
    }

    if telemetry.ram_percent > 75.0 {
        filtered_metrics["ram_percent"] =
            serde_json::json!(format!("{:.1}%", telemetry.ram_percent));
    }

    if telemetry.disk_percent > 75.0 {
        filtered_metrics["disk_percent"] =
            serde_json::json!(format!("{:.1}%", telemetry.disk_percent));
    }

    // Get and deduplicate error logs
    let error_logs = crate::watchers::events::sweep().unwrap_or_default();
    let mut deduplicated_errors: HashMap<String, usize> = HashMap::new();
    let mut first_error_per_message: HashMap<String, crate::watchers::events::Event> =
        HashMap::new();

    for event in error_logs {
        let count = deduplicated_errors.entry(event.text.clone()).or_insert(0);
        *count += 1;
        first_error_per_message
            .entry(event.text.clone())
            .or_insert(event);
    }

    let deduplicated: Vec<Value> = deduplicated_errors
        .iter()
        .map(|(text, count)| {
            if let Some(event) = first_error_per_message.get(text) {
                serde_json::json!({
                    "when": event.when,
                    "source": event.source,
                    "text": text,
                    "count": count,
                })
            } else {
                serde_json::json!({
                    "text": text,
                    "count": count,
                })
            }
        })
        .collect();

    let suggest_repair_agent = !deduplicated.is_empty();

    serde_json::json!({
        "voice_over": voice_over,
        "status": status,
        "machine_name": machine,
        "machine_nickname": machine_nickname,
        "metrics": filtered_metrics,
        "error_logs": deduplicated,
        "suggest_repair_agent": suggest_repair_agent,
        "uptime": telemetry.uptime,
    })
}

pub fn voice_advice(engine: &LlmEngine) -> crate::voice_setup::VoiceAdvice {
    let db = engine.db();
    let local_only = crate::local_only::enabled(db);
    crate::voice_setup::advise(
        db.get_setting_bool("auto_speak", true),
        &db.get_setting_string("tts_engine", "auto"),
        &db.get_setting_string("tts_local_voice", ""),
        &db.get_setting_string("stt_model_path", ""),
        local_only,
    )
}

/// What this machine still needs before it can write code offline, and the commands to do
/// it with.
///
/// The facts the advisor needs are gathered here rather than inside it -- a fresh probe of
/// the machine, how much memory it has, whether local-only mode is on, and which agent
/// commands are on the PATH -- for the reason `setup_advice` does the same: it keeps
/// `code_setup::advise` a pure function, and "no coding agent installed" is exactly the
/// state the machine running the tests is never in.
pub fn code_advice(engine: &LlmEngine) -> crate::code_setup::CodingAdvice {
    let scan = model_scanner::scan_all();
    let ram_total_gb = llm::Telemetry::snapshot().ram_total_gb;
    // What AETHER1 itself runs on, but only when that is a model on this machine: on a
    // cloud provider none of what the panel says about the tool protocol applies, and
    // naming a cloud model there would be advice about the wrong thing.
    let db = engine.db();
    let provider = db.get_setting_string("llm_provider", "offline");
    let own_model = db.get_setting_string("llm_model", "");
    let own_model = matches!(provider.as_str(), "ollama" | "lmstudio").then_some(own_model);

    crate::code_setup::advise(
        &scan,
        ram_total_gb,
        crate::gpu::cached(),
        crate::local_only::enabled(db),
        crate::code_setup::AgentsFound::probe(),
        own_model.as_deref(),
    )
}

/// Puts a question to the coding model and streams the answer back.
///
/// The server and the model are found the same way the panel finds them -- one scan, the
/// server with models on it, the best coding model already downloaded -- rather than from
/// the `llm_provider` and `llm_model` settings. Those name what the *companion* runs on,
/// which is a different model for a different job and is frequently a cloud one; asking a
/// paid API a question the operator opened this panel to keep local would be the wrong
/// answer in the most expensive possible way.
pub fn code_chat_ask(
    engine: &LlmEngine,
    prompt: &str,
    sink: llm::Sink,
) -> Result<crate::code_chat::CodeReply, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("Nothing to ask.".to_string());
    }

    let scan = model_scanner::scan_all();
    let server = scan
        .local_servers
        .iter()
        .find(|server| !server.models.is_empty())
        .ok_or_else(|| {
            "No AI is running on this computer, so there is nothing here to ask. Open The \
             Brain and set a server up first."
                .to_string()
        })?;

    let advice = code_advice(engine);
    if !advice.model_installed {
        return Err(format!(
            "No coding model is downloaded yet. {} is the one for this machine -- download \
             it from the coding panel under The Brain, and this will use it.",
            advice.model
        ));
    }

    let os = llm::Telemetry::snapshot().os_name;
    crate::code_chat::ask(
        engine.db(),
        &server.endpoint,
        server.api,
        &advice.model,
        &os,
        prompt,
        sink,
    )
}

/// What has been said in the coding conversation, for a panel that was just opened.
pub fn code_chat_history(engine: &LlmEngine) -> Vec<serde_json::Value> {
    crate::code_chat::transcript(engine.db())
        .into_iter()
        .map(|message| {
            // The commands are worked out again here rather than stored, so a reply
            // reloaded from the database offers exactly what it offered when it arrived.
            let commands = if message.sender == "user" {
                Vec::new()
            } else {
                crate::code_chat::commands_in(&message.text)
            };
            serde_json::json!({
                "sender": message.sender,
                "text": message.text,
                "timestamp": message.timestamp,
                "commands": commands,
            })
        })
        .collect()
}

pub fn code_chat_clear(engine: &LlmEngine) -> Result<(), String> {
    crate::code_chat::clear(engine.db())
}

/// The sentence the voice test speaks. Short enough to be quick, long enough that a voice
/// which is technically producing audio but producing rubbish is audibly rubbish.
pub const VOICE_TEST_SENTENCE: &str =
    "Welcome to the Aether1 Platform. Speech is working correctly.";

/// Actually says something out loud, and reports every engine it tried on the way.
///
/// The point is the report. A status field can say Piper is installed and the operator can
/// still hear nothing, because "installed" and "produces audio on this machine right now"
/// are different claims and only one of them is the one they care about. So this makes the
/// real attempt and hands back what each engine said -- including on success, because
/// "it spoke, but with the basic voice" is the answer to the most common complaint.
pub fn test_speech(engine: &LlmEngine) -> Result<(PathBuf, Value), Value> {
    let db = engine.db();
    let cache_dir = project_root().join("backend").join("audio_cache");
    let configured_voice = db.get_setting_string("voice_name", llm::DEFAULT_VOICE);
    let local_voice = db.get_setting_string("tts_local_voice", "");
    let local_only = crate::local_only::enabled(db);
    let tts_engine =
        llm::TtsEngine::from_key(&db.get_setting_string("tts_engine", "auto")).resolve(local_only);

    // The test sentence goes through the operator's list like anything else. A test that
    // took a different route through this function would be proving the wrong route works.
    let speller = crate::speech_words::speller(db);

    match llm::generate_speech_reporting(
        &cache_dir,
        VOICE_TEST_SENTENCE,
        tts_engine,
        Some(&configured_voice),
        Some(&local_voice),
        &speller,
    ) {
        Ok(speech) => {
            let report = serde_json::json!({
                "ok": true,
                "engine": speech.engine,
                "attempts": speech.attempts,
                "spoken": VOICE_TEST_SENTENCE,
            });
            Ok((speech.path, report))
        }
        Err(attempts) => Err(serde_json::json!({
            "ok": false,
            "attempts": attempts,
            // Nothing spoke, so the operator is owed the reason in one line rather than a
            // list they have to read backwards.
            "why": attempts
                .last()
                .map(|a| format!("{}: {}", a.engine, a.detail))
                .unwrap_or_else(|| "nothing tried to speak".to_string()),
        })),
    }
}

/// Transcribes a recording made in the page. The audio is written to the same cache
/// directory the synthesized speech lives in, transcribed, and deleted -- a recording of
/// the operator's voice is not something to leave lying around after it has been read.
pub fn transcribe_audio(engine: &LlmEngine, wav: &[u8]) -> Result<String, String> {
    let db = engine.db();
    let cache_dir = project_root().join("backend").join("audio_cache");
    let staged = llm::stage_audio(&cache_dir, wav)?;

    let model = db.get_setting_string("stt_model_path", "");
    let language = db.get_setting_string("stt_language", "en");
    let result = llm::transcribe(&staged, Some(&model), Some(&language));

    let _ = std::fs::remove_file(&staged);
    result
}

/// Whether speech in and out can happen without the network, and if not, what is missing.
/// The settings panel reports this rather than making the operator guess why the
/// microphone button does nothing.
pub fn voice_status(engine: &LlmEngine) -> Value {
    let db = engine.db();
    let tts_engine = db.get_setting_string("tts_engine", "auto");
    let local_voice = db.get_setting_string("tts_local_voice", "");
    let stt_model = db.get_setting_string("stt_model_path", "");

    let speech_out = llm::tts_local_status(Some(&local_voice));
    let speech_in = llm::stt_local_status(Some(&stt_model));

    serde_json::json!({
        "tts_engine": tts_engine,
        // Reported next to the two engines because it changes what they mean: with the
        // mode on, "auto" is Piper or nothing, and a missing Piper is now silence rather
        // than an unannounced trip to a third party.
        "local_only": crate::local_only::enabled(db),
        "local_only_forced": crate::local_only::env_forced(),
        "speech_out": match &speech_out {
            Ok((binary, voice)) => serde_json::json!({
                "local": true,
                "binary": binary.display().to_string(),
                "voice": voice.display().to_string(),
            }),
            // Piper isn't installed/configured, but Auto (and the OS-only choice) can
            // still speak through the OS's own voice -- SAPI on Windows always, espeak-ng
            // on Linux if setup.sh's package got installed. That is what "always works"
            // means for speech out, so it counts as local here too rather than reporting
            // the operator as one step further from offline than they actually are.
            Err(why) => match llm::tts_os_status() {
                Ok(()) => serde_json::json!({ "local": true, "binary": llm::tts_os_engine_name() }),
                Err(_) => serde_json::json!({ "local": false, "why": why }),
            },
        },
        "speech_in": match &speech_in {
            Ok((binary, model)) => serde_json::json!({
                "local": true,
                "binary": binary.display().to_string(),
                "model": model.display().to_string(),
            }),
            Err(why) => serde_json::json!({ "local": false, "why": why }),
        },
        "offline_capable": speech_out.is_ok() && speech_in.is_ok(),
    })
}

/// The shape both front ends read: who is leaving, what they said, and who is answering.
pub fn handover_json(handover: &llm::flow::Handover) -> Value {
    serde_json::json!({
        "from": handover.speaker_name(),
        "line": handover.line,
        "to": handover.to_name(),
        "to_key": handover.to_key(),
    })
}

/// Get the list of detected projects and their Graft status
pub fn graft_detect_projects() -> Result<Value, String> {
    let projects = crate::graft::detect_projects();
    let projects_json: Vec<Value> = projects
        .iter()
        .map(|p| {
            serde_json::json!({
                "path": p.path.to_string_lossy().to_string(),
                "name": p.name,
                "graft_status": p.graft_status.to_string(),
            })
        })
        .collect();
    Ok(serde_json::json!(projects_json))
}

/// Build the Graft graph for a project
pub fn graft_build_graph(_engine: &LlmEngine, project_path: String) -> Result<Value, String> {
    let path = PathBuf::from(&project_path);
    crate::graft::build_graph(&path)?;
    Ok(serde_json::json!({
        "success": true,
        "message": format!("Graft graph built for {}", path.display()),
    }))
}

/// Select a project for code analysis
pub fn graft_select_project(engine: &LlmEngine, project_path: String) -> Result<Value, String> {
    let path = PathBuf::from(&project_path);
    crate::graft::set_selected_project(engine.db(), &path)?;
    Ok(serde_json::json!({
        "success": true,
        "message": format!("Selected project: {}", path.display()),
    }))
}

/// Get the currently selected project
pub fn graft_get_selected_project(engine: &LlmEngine) -> Result<Value, String> {
    match crate::graft::get_selected_project(engine.db()) {
        Some(path) => Ok(serde_json::json!({
            "path": path.to_string_lossy().to_string(),
        })),
        None => Ok(serde_json::json!({
            "path": serde_json::Value::Null,
        })),
    }
}

/// Get Graft version information
pub fn graft_version() -> Result<Value, String> {
    let version = crate::graft::get_graft_version()?;
    Ok(serde_json::json!({
        "version": version,
        "installed": true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Settings panel rewrites its own help text from this field -- which folders the
    /// companion may read, which programs it is worth naming -- so that the page describes
    /// the machine Aether1 is running on rather than the one whose browser is pointed at
    /// it. See applyOsWording() in frontend/js/app.js. Losing the field here does not break
    /// the page, it makes it quietly vague, which is the kind of regression nothing else
    /// would catch.
    #[test]
    fn the_settings_response_says_which_machine_this_is() {
        let path = std::env::temp_dir().join(format!(
            "aether1_settings_os_test_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let db = llm::MemoryDb::open(&path).expect("temp db should open");
        let engine = LlmEngine::new(db);

        let response = get_settings(&engine);

        let os = response["os"].as_str().expect("os should be a string");
        assert!(
            ["windows", "mac", "linux"].contains(&os),
            "unexpected os {os:?}"
        );
        // Beside the settings, not inside them: a fact about the machine is not something
        // the operator can save.
        assert!(response["settings"]["os"].is_null());

        let _ = std::fs::remove_file(&path);
    }

    /// The Settings page shows a dropdown of levels and the one in force. Both come from
    /// here, so that the page and `aether1 code level` describe them in the same words --
    /// and so that a page told there is no project folder says so rather than offering a
    /// choice that has nowhere to be written.
    #[test]
    fn the_settings_response_carries_the_levels_and_this_projects_answer() {
        let dir = std::env::temp_dir().join(format!(
            "aether1_settings_level_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("project")).expect("temp project");
        let db = llm::MemoryDb::open(dir.join("memory.db")).expect("temp db should open");
        let engine = LlmEngine::new(db);

        // No project folder nominated yet: the four levels are still described, and the
        // answer is null rather than a default nobody chose.
        let response = get_settings(&engine);
        let levels = response["levels"]
            .as_array()
            .expect("levels should be a list");
        assert_eq!(levels.len(), crate::code_policy::ALL.len());
        assert!(levels
            .iter()
            .any(|level| level["key"] == "developer" && level["title"] == "Developer"));
        assert!(response["autonomy"]["level"].is_null());

        let root = dir.join("project").canonicalize().expect("project root");
        engine
            .db()
            .set_setting(
                crate::code_workspace::ROOT_SETTING,
                &serde_json::json!(root.to_string_lossy()),
            )
            .expect("workspace root should save");

        assert_eq!(get_settings(&engine)["autonomy"]["level"], "developer");
        code_set_level(&engine, "agent").expect("agent is a level");
        let response = get_settings(&engine);
        assert_eq!(response["autonomy"]["level"], "agent");
        // And the switches it stands for really moved, which is the only reason the level
        // is worth recording.
        assert!(response["settings"][crate::code_sandbox::NETWORK_SETTING] == true);

        // A level nobody defined is refused by name, not rounded to the nearest one.
        let refused = code_set_level(&engine, "yolo").expect_err("yolo is not a level");
        assert!(refused.contains("unrestricted"), "unhelpful: {refused}");
        assert_eq!(get_settings(&engine)["autonomy"]["level"], "agent");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_conversation_id_is_refused_rather_than_repaired() {
        // The whole point: none of these come back as a *different* valid id, because a
        // sanitiser would quietly hand the caller somebody else's transcript.
        assert!(valid_session_id(Some("../../etc/passwd".into())).is_err());
        assert!(valid_session_id(Some("c1'; DROP TABLE messages--".into())).is_err());
        assert!(valid_session_id(Some("c 1".into())).is_err());
        assert!(valid_session_id(Some("x".repeat(65))).is_err());

        assert_eq!(valid_session_id(None).unwrap(), "default");
        assert_eq!(
            valid_session_id(Some("   ".into())).unwrap(),
            "default",
            "an empty id means the caller didn't say, not that they said nothing"
        );
        assert_eq!(valid_session_id(Some("c1a-b_2".into())).unwrap(), "c1a-b_2");
    }

    #[test]
    fn minted_ids_are_distinct_and_pass_their_own_gate() {
        let a = new_session();
        let b = new_session();
        assert_ne!(a, b, "two conversations asked for are two conversations");
        assert_eq!(valid_session_id(Some(a.clone())).unwrap(), a);
        assert!(a.starts_with('c'), "never empty, even at the epoch");
    }
}
