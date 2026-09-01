// Headless HTTP mode (`aether1 --serve`) -- a plain axum server standing in for the old
// Python FastAPI backend so the browser-based dev flow (start.sh et al.) no longer needs
// Python at all. Serves frontend/ statically and the same REST/WebSocket surface
// frontend/js/app.js's non-Tauri fallback path (IS_TAURI == false) already expects, calling
// into the exact same commands:: functions the native Tauri app's commands use.
//
// The native app is untouched by any of this -- see the `--serve` flag check in main() that
// routes here before tauri::Builder is ever constructed.

use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{header, StatusCode},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast;
use tower_http::services::ServeDir;

use crate::commands;
use crate::llm::{self, LlmEngine};
use crate::model_scanner;
use crate::project_root;

#[derive(Clone)]
struct AppState {
    engine: Arc<LlmEngine>,
    telemetry_tx: broadcast::Sender<Value>,
}

pub async fn run(engine: LlmEngine) {
    let engine = Arc::new(engine);
    let (telemetry_tx, _rx) = broadcast::channel::<Value>(8);

    // Mirrors the Tauri path's telemetry thread (main.rs's setup() closure): same
    // Telemetry::snapshot()/usage_snapshot()/agent_name() payload every ~1s, broadcast to
    // however many /ws/telemetry clients are connected instead of a Tauri event.
    {
        let engine = engine.clone();
        let tx = telemetry_tx.clone();
        std::thread::spawn(move || loop {
            let telemetry = llm::Telemetry::snapshot();
            let payload = serde_json::json!({
                "telemetry": telemetry.to_wire_json(),
                "tokens": engine.usage_snapshot(),
                "agent_name": engine.agent_name(),
            });
            let _ = tx.send(payload); // Err just means no WS clients are connected right now
            std::thread::sleep(Duration::from_millis(1000));
        });
    }

    let state = AppState {
        engine,
        telemetry_tx,
    };
    let static_service =
        ServeDir::new(project_root().join("frontend")).append_index_html_on_directories(true);

    let app = Router::new()
        .route("/api/static-info", get(static_info))
        .route("/api/chat", post(chat))
        .route("/api/agent/genesis", post(genesis))
        .route("/api/scanner/status", get(scanner_status))
        .route("/api/scanner/pull-model", post(pull_model))
        .route("/api/tools", get(get_tools))
        .route("/api/actions", get(get_actions))
        .route("/api/messages", get(get_messages).delete(clear_messages))
        .route("/api/settings", get(get_settings).post(save_settings))
        .route("/api/tts", post(tts))
        .route("/api/audio/{filename}", get(get_audio))
        .route("/ws/chat", get(ws_chat))
        .route("/ws/telemetry", get(ws_telemetry))
        .with_state(state)
        .fallback_service(static_service);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8378")
        .await
        .expect("failed to bind :8378 -- is another AETHER1 instance already running?");
    println!("[AETHER1] serving http://localhost:8378 (Ctrl+C to stop)");
    axum::serve(listener, app).await.expect("axum server error");
}

fn internal_error<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

async fn static_info() -> Json<Value> {
    Json(commands::static_info())
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    session_id: Option<String>,
    generate_voice: Option<bool>,
}

/// Unlike generate_response_rust (the Tauri command, which leaves TTS to a separate
/// generate_speech_rust invoke), the HTTP /api/chat response bundles audio_url directly --
/// matching what the old Python /api/chat did and what app.js's non-Tauri branch reads.
async fn chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let engine = state.engine.clone();
    let message = req.message;
    let session_id = req.session_id.unwrap_or_else(|| "default".to_string());
    let session_id_for_result = session_id.clone();

    let mut result = {
        let engine = engine.clone();
        let session_id = session_id.clone();
        tokio::task::spawn_blocking(move || {
            commands::generate_response(&engine, message, Some(session_id))
        })
        .await
        .map_err(internal_error)?
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?
    };

    let mut audio_url: Option<String> = None;
    if req.generate_voice.unwrap_or(false) {
        if let Some(reply) = result
            .get("reply")
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            audio_url =
                tokio::task::spawn_blocking(move || commands::synthesize_speech(&reply, None))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .and_then(|p| {
                        p.file_name()
                            .map(|f| format!("/api/audio/{}", f.to_string_lossy()))
                    });
        }
    }

    result["audio_url"] = serde_json::json!(audio_url);
    result["session_id"] = serde_json::json!(session_id_for_result);
    Ok(Json(result))
}

async fn get_tools(State(state): State<AppState>) -> Json<Value> {
    Json(commands::tool_catalog(&state.engine))
}

#[derive(Deserialize)]
struct ActionsQuery {
    limit: Option<u32>,
}

async fn get_actions(
    State(state): State<AppState>,
    Query(q): Query<ActionsQuery>,
) -> Json<Vec<llm::ActionRecord>> {
    Json(commands::recent_actions(&state.engine, q.limit))
}

#[derive(Deserialize)]
struct GenesisRequest {
    purpose: String,
}

/// Same bundling as chat(): the Tauri path generates the greeting's audio via a separate
/// invoke, but Python's /api/agent/genesis always synthesized the greeting, and app.js's
/// non-Tauri branch expects audio_url straight back in the response -- so do that here too.
async fn genesis(
    State(state): State<AppState>,
    Json(req): Json<GenesisRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let engine = state.engine.clone();
    let mut result =
        tokio::task::spawn_blocking(move || commands::agent_genesis(&engine, req.purpose))
            .await
            .map_err(internal_error)?
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let greeting = result
        .get("greeting")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let voice = result
        .get("voice")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let audio_url = tokio::task::spawn_blocking(move || {
        commands::synthesize_speech(&greeting, voice.as_deref())
    })
    .await
    .ok()
    .and_then(Result::ok)
    .and_then(|p| {
        p.file_name()
            .map(|f| format!("/api/audio/{}", f.to_string_lossy()))
    });

    result["audio_url"] = serde_json::json!(audio_url);
    Ok(Json(result))
}

async fn scanner_status() -> Json<model_scanner::ScanResult> {
    Json(
        tokio::task::spawn_blocking(model_scanner::scan_all)
            .await
            .expect("scan_all panicked"),
    )
}

#[derive(Deserialize)]
struct PullQuery {
    model_name: Option<String>,
}

async fn pull_model(Query(q): Query<PullQuery>) -> Json<model_scanner::PullResult> {
    let name = q.model_name.unwrap_or_default();
    Json(
        tokio::task::spawn_blocking(move || commands::pull_model(name))
            .await
            .expect("pull_model panicked"),
    )
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<u32>,
}

async fn get_messages(
    State(state): State<AppState>,
    Query(q): Query<LimitQuery>,
) -> Json<Vec<llm::Message>> {
    let engine = state.engine.clone();
    Json(
        tokio::task::spawn_blocking(move || commands::get_messages(&engine, q.limit))
            .await
            .unwrap_or_default(),
    )
}

async fn clear_messages(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let engine = state.engine.clone();
    tokio::task::spawn_blocking(move || commands::clear_messages(&engine))
        .await
        .map_err(internal_error)?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "status": "cleared" })))
}

async fn get_settings(State(state): State<AppState>) -> Json<Value> {
    let engine = state.engine.clone();
    Json(
        tokio::task::spawn_blocking(move || commands::get_settings(&engine))
            .await
            .unwrap_or(serde_json::json!({})),
    )
}

async fn save_settings(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let settings = body.get("settings").cloned().unwrap_or(body);
    let engine = state.engine.clone();
    tokio::task::spawn_blocking(move || commands::save_settings(&engine, settings))
        .await
        .map_err(internal_error)?
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "status": "saved" })))
}

async fn get_audio(Path(filename): Path<String>) -> Result<Response, StatusCode> {
    // Basename-only, matching Python's os.path.basename(filename) sanitization -- refuses
    // to let a crafted filename (e.g. "../../etc/passwd") escape audio_cache/.
    let safe_name = std::path::Path::new(&filename)
        .file_name()
        .ok_or(StatusCode::BAD_REQUEST)?;
    let path = project_root()
        .join("backend")
        .join("audio_cache")
        .join(safe_name);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Response::builder()
        .header(header::CONTENT_TYPE, "audio/mpeg")
        .body(Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Streaming chat, the browser fallback's counterpart to the Tauri path's `chat-delta`
/// events. A WebSocket rather than SSE because the socket plumbing is already here for
/// telemetry, and because the client sends the prompt on the same connection it reads the
/// reply from.
///
/// Protocol: the client sends one JSON message shaped like POST /api/chat, and receives a
/// stream of {"type":"delta","delta":...} messages followed by exactly one
/// {"type":"done", reply, agent_name, audio_url, session_id} -- the same object /api/chat
/// returns -- or {"type":"error","error":...}. The socket then closes.
#[derive(Deserialize)]
struct TtsRequest {
    text: String,
    voice: Option<String>,
}

/// Synthesizes one piece of text and returns its audio URL. The Tauri path has had this
/// all along as generate_speech_rust; the browser path only ever got whole-reply audio
/// bundled into /api/chat, which is no use once replies stream in sentence by sentence.
async fn tts(Json(req): Json<TtsRequest>) -> Result<Json<Value>, (StatusCode, String)> {
    let path = tokio::task::spawn_blocking(move || {
        commands::synthesize_speech(&req.text, req.voice.as_deref())
    })
    .await
    .map_err(internal_error)?
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "no audio file".to_string()))?;

    Ok(Json(
        serde_json::json!({ "audio_url": format!("/api/audio/{file_name}") }),
    ))
}

async fn ws_chat(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| chat_socket(socket, state))
}

async fn chat_socket(mut socket: WebSocket, state: AppState) {
    let Some(Ok(Message::Text(raw))) = socket.recv().await else {
        return;
    };
    let Ok(req) = serde_json::from_str::<ChatRequest>(&raw) else {
        let _ = socket
            .send(Message::Text(
                serde_json::json!({"type": "error", "error": "malformed chat request"})
                    .to_string()
                    .into(),
            ))
            .await;
        return;
    };

    let session_id = req.session_id.unwrap_or_else(|| "default".to_string());
    let engine = state.engine.clone();
    let message = req.message;

    // The engine is blocking and knows nothing about async, so it runs on a blocking task
    // and pushes deltas through a channel that this task forwards to the socket. Unbounded
    // so a slow client can never block generation itself -- the deltas are small and the
    // reply is bounded by the model's own output.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let generation = {
        let session_id = session_id.clone();
        tokio::task::spawn_blocking(move || {
            commands::generate_response_streamed(&engine, message, Some(session_id), &mut |delta| {
                let _ = tx.send(delta.to_string());
            })
        })
    };

    while let Some(delta) = rx.recv().await {
        if socket
            .send(Message::Text(
                serde_json::json!({"type": "delta", "delta": delta})
                    .to_string()
                    .into(),
            ))
            .await
            .is_err()
        {
            break; // client went away; the generation task still finishes and is stored
        }
    }

    let mut result = match generation.await {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => {
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({"type": "error", "error": e})
                        .to_string()
                        .into(),
                ))
                .await;
            return;
        }
        Err(e) => {
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({"type": "error", "error": e.to_string()})
                        .to_string()
                        .into(),
                ))
                .await;
            return;
        }
    };

    let mut audio_url: Option<String> = None;
    if req.generate_voice.unwrap_or(false) {
        if let Some(reply) = result
            .get("reply")
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            audio_url =
                tokio::task::spawn_blocking(move || commands::synthesize_speech(&reply, None))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .and_then(|p| {
                        p.file_name()
                            .map(|f| format!("/api/audio/{}", f.to_string_lossy()))
                    });
        }
    }
    result["type"] = serde_json::json!("done");
    result["audio_url"] = serde_json::json!(audio_url);
    result["session_id"] = serde_json::json!(session_id);

    let _ = socket.send(Message::Text(result.to_string().into())).await;
}

async fn ws_telemetry(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    let rx = state.telemetry_tx.subscribe();
    ws.on_upgrade(move |socket| telemetry_socket(socket, rx))
}

async fn telemetry_socket(mut socket: WebSocket, mut rx: broadcast::Receiver<Value>) {
    loop {
        match rx.recv().await {
            Ok(payload) => {
                if socket
                    .send(Message::Text(payload.to_string().into()))
                    .await
                    .is_err()
                {
                    break; // client disconnected
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}
