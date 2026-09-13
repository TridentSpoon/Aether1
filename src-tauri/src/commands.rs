// Plain functions behind the Tauri commands in main.rs, extracted so the axum server in
// server.rs can call the exact same logic instead of duplicating it. Each function here is
// what a #[tauri::command] wrapper's body used to be, verbatim -- the wrappers in main.rs are
// now one-line pass-throughs.

use std::path::PathBuf;

use serde_json::Value;

use crate::llm::{self, ActionRecord, LlmEngine};
use crate::model_scanner;
use crate::project_root;
use crate::tools;

pub fn generate_response(
    engine: &LlmEngine,
    prompt: String,
    session_id: Option<String>,
) -> Result<Value, String> {
    generate_response_streamed(engine, prompt, session_id, &mut |_| {})
}

/// generate_response, with each piece of the reply handed to `sink` as it arrives. The
/// returned value is identical either way -- the sink is an extra, not an alternative, so
/// history and usage accounting can't differ between a streamed and an unstreamed turn.
pub fn generate_response_streamed(
    engine: &LlmEngine,
    prompt: String,
    session_id: Option<String>,
    sink: llm::Sink,
) -> Result<Value, String> {
    if prompt.trim().is_empty() {
        return Err("Empty message".to_string());
    }
    let session_id = session_id.unwrap_or_else(|| "default".to_string());

    engine.add_message(&session_id, "user", &prompt);
    // Opened and drained around the one call that reads the vault, so the notes reported
    // belong to this answer and no other. Both are on this thread, which is the whole
    // reason the record can be a thread-local -- see vault::consulted.
    crate::vault::consulted::begin();
    let reply = engine.generate_response_streamed(&prompt, &session_id, sink);
    let notes = crate::vault::consulted::taken();
    let agent_name = engine.agent_name();
    engine.add_message(&session_id, &agent_name.to_lowercase(), &reply);

    Ok(serde_json::json!({
        "reply": reply,
        "agent_name": agent_name,
        "notes": crate::vault::consulted::to_json(&notes),
    }))
}

/// Opens the vault folder in the operator's own file manager.
///
/// The point of keeping memory as markdown is that it can be opened without us, and a
/// folder you have to go and find is a promise half kept. Creates the vault first if it
/// isn't there yet: a button that opens nothing would be a worse answer than a button that
/// shows you the empty room it made.
///
/// Deliberately not exposed over HTTP and not a tool. It is reachable only from the desktop
/// app's own Settings pane, by the person sitting at the machine -- a browser on the LAN
/// asking a computer in another room to pop open a file manager is not a feature. The path
/// comes from the vault setting rather than from any argument, and the launcher is spawned
/// with no shell, so there is nothing here for a prompt to steer.
pub fn open_vault_folder(engine: &LlmEngine) -> Result<String, String> {
    let root = crate::vault::ensure(engine.db())?;

    let launcher = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };

    std::process::Command::new(launcher)
        .arg(&root)
        .spawn()
        .map_err(|e| format!("could not open {} with {launcher}: {e}", root.display()))?;

    Ok(root.to_string_lossy().to_string())
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

    serde_json::to_value(crate::setup::advise(&scan, ram_total_gb, configured))
        .unwrap_or_else(|_| serde_json::json!({}))
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

pub fn get_messages(engine: &LlmEngine, limit: Option<u32>) -> Vec<llm::Message> {
    engine
        .db()
        .get_messages("default", limit.unwrap_or(50))
        .unwrap_or_default()
}

pub fn clear_messages(engine: &LlmEngine) -> Result<(), String> {
    engine
        .db()
        .clear_history("default")
        .map_err(|e| e.to_string())
}

/// The persona catalogue Settings renders its list from. Static -- it depends on nothing but
/// the enum -- but it goes through the same two transports as everything else rather than
/// being written out a second time in the HTML, which is how the list and the behaviour
/// would come to disagree.
pub fn list_personas() -> Value {
    llm::Persona::catalogue()
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
        "stt_model_path": "",
        "stt_language": "en",
        "vault_path": "",
        // The HUD saves this alongside its own browser copy so the two agree; without a
        // default the key simply wouldn't come back on a fresh install, and the page would
        // have nothing to reconcile against.
        "color_theme": "",
        "hotkey_toggle": crate::hotkey::DEFAULT_TOGGLE,
        "desktop_sprite_enabled": false,
        "local_only": false,
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
    serde_json::json!({ "settings": settings })
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
    let local_voice = db.get_setting_string("tts_local_voice", "");

    // The cloud voice is the one path that used to leave the machine without anyone
    // choosing it: with Piper absent, `auto` quietly sent the text of everything the
    // companion said to Microsoft. Local-only mode collapses that choice to Piper, and
    // when Piper is not installed it says so instead of reaching out.
    let local_only = crate::local_only::enabled(db);
    let tts_engine =
        llm::TtsEngine::from_key(&db.get_setting_string("tts_engine", "auto")).resolve(local_only);

    llm::generate_speech_with(
        &cache_dir,
        text,
        tts_engine,
        Some(voice.unwrap_or(&configured_voice)),
        Some(&local_voice),
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
