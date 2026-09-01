// Plain functions behind the Tauri commands in main.rs, extracted so the axum server in
// server.rs can call the exact same logic instead of duplicating it. Each function here is
// what a #[tauri::command] wrapper's body used to be, verbatim -- the wrappers in main.rs are
// now one-line pass-throughs.

use std::path::PathBuf;

use serde_json::Value;

use crate::llm::{self, LlmEngine};
use crate::model_scanner;
use crate::project_root;

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
    let reply = engine.generate_response_streamed(&prompt, &session_id, sink);
    let agent_name = engine.agent_name();
    engine.add_message(&session_id, &agent_name.to_lowercase(), &reply);

    Ok(serde_json::json!({ "reply": reply, "agent_name": agent_name }))
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

pub fn pull_model(model_name: String) -> model_scanner::PullResult {
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
        "persona_type": "halcy",
        "custom_directive": "",
        "voice_name": llm::DEFAULT_VOICE,
        "enable_sfx": true,
        "auto_speak": true,
        "hotkey_toggle": crate::hotkey::DEFAULT_TOGGLE,
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

/// Shared by the Tauri `generate_speech_rust` command and the axum server's TTS-bundling
/// logic in /api/chat and /api/agent/genesis.
pub fn synthesize_speech(text: &str, voice: Option<&str>) -> Result<PathBuf, String> {
    let cache_dir = project_root().join("backend").join("audio_cache");
    llm::generate_speech(&cache_dir, text, voice)
}
