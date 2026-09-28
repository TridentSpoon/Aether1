// Headless HTTP mode (`aether1 --serve`) -- a plain axum server standing in for the old
// Python FastAPI backend so the browser-based dev flow (start.sh et al.) no longer needs
// Python at all. Serves frontend/ statically and the same REST/WebSocket surface
// frontend/js/app.js's non-Tauri fallback path (IS_TAURI == false) already expects, calling
// into the exact same commands:: functions the native Tauri app's commands use.
//
// The native app is untouched by any of this -- see the `--serve` flag check in main() that
// routes here before tauri::Builder is ever constructed.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Path, Query, Request, State,
    },
    http::{header, StatusCode},
    middleware,
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast;
use tower_http::services::ServeDir;

use crate::commands;
use crate::discovery;
use crate::llm::{self, LlmEngine};
use crate::model_scanner;
use crate::project_root;
use crate::serve_auth::{self, AttemptLimiter, ServeAuth, Setup};
use crate::serve_tls;
use crate::vault;

#[derive(Clone)]
struct AppState {
    engine: Arc<LlmEngine>,
    telemetry_tx: broadcast::Sender<Value>,
    /// Woken when a /ws/telemetry client subscribes, so the parked sampling loop starts
    /// feeding it on the next instant rather than after NO_CLIENT_RECHECK.
    telemetry_pulse: Arc<llm::Pulse>,
    /// Whether this server was started with --lan. Carried on the state because
    /// `/api/doctor` reports it: the TLS certificate and the mDNS announcement only exist in
    /// the --lan case, and a check that claimed otherwise would be reporting a fault on a
    /// loopback server for missing things it was never asked to have.
    lan: bool,
}

/// What the two `--lan` gates share: the token to check against, and the record of who has
/// been getting it wrong. Bundled because `/api/pair` and the middleware both need both --
/// a failed phrase and a failed token are the same attack seen at two doors, and counting
/// them separately would double whatever budget an attacker actually gets.
#[derive(Clone)]
struct LanState {
    auth: Arc<ServeAuth>,
    limiter: Arc<AttemptLimiter>,
}

/// The port both modes listen on. Named here because it is the one thing start.sh,
/// start.bat, the README and `aether1 doctor` all assume; the Remote & LAN pane probes it
/// to say whether anything is serving, and reads it from here rather than repeating it.
pub const SERVE_PORT: u16 = 8378;

/// How long the headless telemetry loop waits between looks while no browser is attached.
/// A subscribing client wakes it immediately; this is only the fallback, and a look is one
/// integer read.
const NO_CLIENT_RECHECK: Duration = Duration::from_secs(30);

/// The address the HTTP server listens on.
///
/// Loopback is the default because nothing here needs a password to be safe on it: every
/// route -- the conversation, the action log, approving a pending action -- is open to
/// whoever can reach the port, but reaching 127.0.0.1 already means being a process on this
/// machine. On 0.0.0.0 that same openness would mean everyone on the network, which for a
/// laptop means every cafe, hotel and office it is ever carried into -- so `--lan` gates
/// every route behind a pairing token instead (see serve_auth and require_lan_token below).
/// Binding 127.0.0.1 in the default case makes the operating system itself refuse remote
/// connections, so that guarantee doesn't depend on this code being careful; --lan's
/// guarantee depends on the token check below being correct, which is a strictly weaker
/// promise, made only when the operator has opted into needing it.
fn bind_address(lan: bool) -> &'static str {
    if lan {
        "0.0.0.0:8378"
    } else {
        "127.0.0.1:8378"
    }
}

pub async fn run(engine: LlmEngine, lan: bool) {
    let engine = Arc::new(engine);
    // The receiver is dropped at once rather than held: `receiver_count()` is how the loop
    // below knows whether anyone is listening, and a receiver kept alive here for the whole
    // life of the server would answer "yes" forever, on a machine with no browser open.
    let (telemetry_tx, _) = broadcast::channel::<Value>(8);

    // Mirrors the Tauri path's telemetry thread (main.rs's setup() closure): same
    // payload every ~1s, broadcast to however many /ws/telemetry clients are connected
    // instead of a Tauri event.
    //
    // And the same rule about who is listening. Here it is not a question of windows: a
    // headless server with no browser attached has nobody to send to, and `send` on a
    // broadcast channel with no receivers is not a cheap no-op if the payload cost a full
    // pass over the machine to build. So the loop parks until a /ws/telemetry client
    // subscribes (see llm::Pulse, woken by ws_telemetry below) -- `aether1 --serve` left
    // running in a terminal with no tab open now costs nothing at all.
    let pulse = Arc::new(llm::Pulse::default());
    {
        let engine = engine.clone();
        let tx = telemetry_tx.clone();
        let pulse = pulse.clone();
        std::thread::spawn(move || {
            let mut sampler = llm::Sampler::new();
            loop {
                if tx.receiver_count() == 0 {
                    pulse.park(NO_CLIENT_RECHECK);
                    continue;
                }
                let telemetry = sampler.sample();
                let payload = serde_json::json!({
                    "telemetry": telemetry.to_wire_json(),
                    "tokens": engine.usage_snapshot(),
                    "agent_name": engine.agent_name(),
                });
                let _ = tx.send(payload); // Err just means the last client left mid-tick
                pulse.park(Duration::from_millis(1000));
            }
        });
    }

    let state = AppState {
        engine,
        telemetry_tx,
        telemetry_pulse: pulse,
        lan,
    };
    let static_service =
        ServeDir::new(project_root().join("frontend")).append_index_html_on_directories(true);

    let mut app = Router::new()
        .route("/api/static-info", get(static_info))
        .route("/api/chat", post(chat))
        .route("/api/agent/genesis", post(genesis))
        .route("/api/llm/test-connection", post(test_llm_connection))
        .route("/api/scanner/status", get(scanner_status))
        .route("/api/setup/advice", get(setup_advice))
        .route("/api/voice/advice", get(voice_advice))
        .route("/api/code/advice", get(code_advice))
        .route("/api/code/conventions", get(code_conventions))
        .route("/api/code/chat", post(code_chat))
        .route("/api/code/chat/history", get(code_chat_history))
        .route("/api/code/chat/clear", post(code_chat_clear))
        .route("/api/code/level", post(code_level))
        .route("/api/voice/test", post(test_speech))
        .route("/api/scanner/pull-model", post(pull_model))
        .route("/api/setup/download", post(start_download))
        .route("/api/setup/downloads", get(download_status))
        .route("/api/setup/download/forget", post(forget_download))
        .route("/api/voice/catalogue", get(voice_catalogue))
        .route("/api/voice/download", post(start_voice_download))
        .route("/api/voice/downloads", get(voice_download_status))
        .route("/api/voice/download/forget", post(forget_voice_download))
        .route("/api/audio/devices", get(audio_devices))
        .route("/api/setup/start-server", post(start_local_server))
        .route("/api/tools", get(get_tools))
        .route("/api/actions", get(get_actions))
        .route("/api/actions/pending", get(get_pending_actions))
        .route("/api/actions/{id}/approve", post(approve_action))
        .route("/api/actions/{id}/undo", post(undo_action))
        .route("/api/actions/{id}/reject", post(reject_action))
        .route("/api/tools/always-allow", post(set_always_allowed))
        .route(
            "/api/persona/access",
            get(persona_access).post(set_persona_access),
        )
        .route("/api/messages", get(get_messages).delete(clear_messages))
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/new", post(new_session))
        .route("/api/sessions/rename", post(rename_session))
        .route("/api/sessions/delete", post(delete_session))
        .route("/api/benchmarks/reset", post(reset_benchmarks))
        .route("/api/vault/open", post(open_vault_folder))
        .route("/api/vault/notes", get(vault_notes))
        .route("/api/vault/note", get(vault_note))
        .route("/api/vault/graph", get(vault_graph))
        .route("/api/vault/search", get(vault_search))
        .route("/api/settings", get(get_settings).post(save_settings))
        .route("/api/tts", post(tts))
        .route("/api/stt", post(stt))
        .route("/api/voice/status", get(voice_status))
        .route("/api/personas", get(list_personas))
        .route("/api/personas/voice", post(set_persona_voice))
        .route("/api/personas/voice/reset", post(clear_persona_voice))
        .route("/api/voice/pickers", get(voice_pickers))
        .route(
            "/api/speech/pronunciations",
            get(pronunciations).post(set_pronunciations),
        )
        .route("/api/flow", get(flow_mode).post(set_flow_mode))
        .route("/api/flow/line", post(set_flow_line))
        .route("/api/profile", get(profile_report))
        .route("/api/profile/revoke", post(revoke_device))
        .route("/api/doctor", get(doctor_report))
        .route("/api/doctor/repair", post(doctor_repair))
        .route("/api/doctor/attend", post(doctor_attend))
        .route("/api/audio/{filename}", get(get_audio))
        .route("/ws/chat", get(ws_chat))
        .route("/ws/telemetry", get(ws_telemetry))
        .with_state(state);

    // Loopback mode needs none of this: the operating system already refuses every
    // connection that isn't from this machine, so a second "type a phrase in" gate on top
    // would be friction with nothing behind it. --lan is where an actual network boundary
    // exists, so it's the only mode that gets one.
    //
    // `_announcement` is never read again -- its only job is to stay alive (keeping the
    // mDNS daemon answering queries) for as long as `run()`'s stack frame does, which is
    // the life of the server.
    let mut _announcement = None;
    let mut tls = None;
    if lan {
        let (auth, setup) =
            serve_auth::load_or_create().expect("could not set up the --lan pairing token");
        if let Setup::New { phrase } = setup {
            println!(
                "\n[AETHER1] --lan pairing phrase (shown once -- write it down now):\n\n    {phrase}\n\n\
                 Type this into another AETHER1 instance's pairing prompt to let it reach this \
                 one. Run `aether1 pair` later to generate a new phrase and revoke this one.\n"
            );
        }
        let lan_state = LanState {
            auth: Arc::new(auth),
            limiter: Arc::new(AttemptLimiter::new()),
        };
        let pair_router = Router::new()
            .route("/api/pair", post(pair))
            .with_state(lan_state.clone());
        app = app
            .route_layer(middleware::from_fn_with_state(lan_state, require_lan_token))
            .merge(pair_router);

        let certificate =
            serve_tls::load_or_create().expect("could not set up the --lan certificate");
        if matches!(certificate.origin, serve_tls::Origin::New) {
            println!("[AETHER1] --lan: made this machine a certificate to identify itself by.");
        }
        println!(
            "\n[AETHER1] --lan certificate fingerprint (SHA-256):\n\n    {}\n\n\
             Your browser will warn that nobody vouches for this certificate, which is true: \
             this machine signed it itself, because no public authority will vouch for \
             an address on your own network. Check the fingerprint it shows you against the one \
             above, once, and you have done by hand what the padlock does for a public site.\n",
            certificate.fingerprint
        );
        tls = Some(certificate);

        let instance_name = sysinfo::System::host_name().unwrap_or_else(|| "aether1".to_string());
        match discovery::announce(&instance_name, 8378, &[("version", crate::APP_VERSION)]) {
            Ok(guard) => _announcement = Some(guard),
            Err(e) => eprintln!("[AETHER1] could not announce on the LAN: {e} (still serving)"),
        }
    }

    let app = app.fallback_service(static_service);

    // Bound before serving either way, so "that port is taken" is still reported the moment
    // it happens rather than somewhere inside the TLS handshake machinery.
    let listener = std::net::TcpListener::bind(bind_address(lan))
        .expect("failed to bind :8378 -- is another AETHER1 instance already running?");
    listener
        .set_nonblocking(true)
        .expect("could not put the listening socket into non-blocking mode");

    let scheme = if tls.is_some() { "https" } else { "http" };
    println!("[AETHER1] serving {scheme}://localhost:8378 (Ctrl+C to stop)");
    if lan {
        // Said plainly and every time. Someone who typed --lan once in a script should
        // still be told what it means on the day they run it somewhere unfamiliar.
        println!(
            "[AETHER1] --lan: reachable from your network over TLS, but every request needs \
             a token of its own -- a device gets one by POSTing the pairing phrase to \
             /api/pair, and until then AETHER1 refuses to show your conversation or run \
             anything. `aether1 devices` lists what is paired; `aether1 revoke <id>` cuts \
             one off straight away."
        );
    } else {
        println!("[AETHER1] reachable from this machine only. Use --lan to open it up.");
    }

    // `into_make_service_with_connect_info` is what puts the peer address within reach of
    // the handlers, which is what the attempt limiter counts against. Loopback pays for it
    // too, where it is unused but harmless, rather than having two ways to start the server.
    let service = app.into_make_service_with_connect_info::<SocketAddr>();

    match tls {
        Some(certificate) => {
            // rustls is built here without a default provider chosen for it, so one is named
            // explicitly. `ring` is the same provider the outbound HTTP client already uses,
            // which keeps one implementation in the binary rather than two.
            let _ = rustls::crypto::ring::default_provider().install_default();
            let config = RustlsConfig::from_pem(
                certificate.cert_pem.into_bytes(),
                certificate.key_pem.into_bytes(),
            )
            .await
            .expect("could not load the --lan certificate");
            axum_server::from_tcp_rustls(listener, config)
                .expect("could not start the TLS listener")
                .serve(service)
                .await
                .expect("axum server error");
        }
        None => {
            let listener = tokio::net::TcpListener::from_std(listener)
                .expect("could not hand the listening socket to tokio");
            axum::serve(listener, service)
                .await
                .expect("axum server error");
        }
    }
}

fn bearer_token(request: &Request) -> Option<String> {
    request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string)
}

/// The subprotocol a `/ws/*` client always offers, and the one this server selects. It
/// carries no credential; it exists so that a handshake which also offered a token has
/// something the server can echo back, since a browser closes a socket whose requested
/// subprotocol was not selected.
pub const WS_PROTOCOL: &str = "aether1";

/// The prefix of the subprotocol that carries a device token.
const WS_TOKEN_PREFIX: &str = "aether1.token.";

/// A WebSocket handshake can't carry a custom `Authorization` header from a browser, so the
/// `/ws/*` routes read the token from `Sec-WebSocket-Protocol` instead -- the one handshake
/// header a browser does let a page set, through the WebSocket constructor's second
/// argument.
///
/// It used to be `?token=...`, which is the better-known workaround and the wrong one: a URL
/// is the part of a request that gets written down. Access logs record the request line,
/// reverse proxies and their error pages record it too, and a device token here is not a
/// short-lived ticket but the credential for every other request that device makes. A
/// header is not immune to being logged, but it is not logged *by default* by the things in
/// front of this server, and it is the same place the credential already travels for every
/// route that is not a socket.
///
/// The server never echoes the token back. A client offers two subprotocols --
/// `aether1.token.<token>` and `aether1` -- and the handlers select `aether1`, so the
/// credential appears in one direction only.
fn token_from_subprotocol(request: &Request) -> Option<String> {
    let offered = request
        .headers()
        .get(header::SEC_WEBSOCKET_PROTOCOL)?
        .to_str()
        .ok()?;
    offered
        .split(',')
        .map(str::trim)
        .find_map(|protocol| protocol.strip_prefix(WS_TOKEN_PREFIX))
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({
            "error": "pairing required",
            "hint": "POST your pairing phrase to /api/pair to get a token, then send it back \
                      as `Authorization: Bearer <token>` (or, for a WebSocket, as the \
                      `aether1.token.<token>` subprotocol)"
        })),
    )
        .into_response()
}

/// The answer an address gets once it has spent its attempts. `Retry-After` is the header
/// HTTP already has for this, so a well-behaved client waits the right amount without being
/// told how in prose, and says the same thing in the body for a person reading it.
fn too_many_attempts(wait: Duration) -> Response {
    let seconds = wait.as_secs().max(1);
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, seconds.to_string())],
        Json(serde_json::json!({
            "error": "too many failed pairing attempts",
            "hint": format!("wait {seconds} seconds and try again"),
        })),
    )
        .into_response()
}

async fn require_lan_token(
    State(lan): State<LanState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    let ip = peer.ip();
    if let Some(wait) = lan.limiter.retry_after(ip) {
        return too_many_attempts(wait);
    }
    match bearer_token(&request).or_else(|| token_from_subprotocol(&request)) {
        Some(token) if lan.auth.accepts(&token) => {
            lan.limiter.record_success(ip);
            next.run(request).await
        }
        // A token that is wrong is a guess, and guesses are what the budget is for.
        Some(_) => {
            lan.limiter.record_failure(ip);
            unauthorized()
        }
        // A request carrying no credential at all is *not* counted, which is a change from
        // how this started out. The original reasoning was that a bare request looks like
        // the first step of someone probing -- true, but it made pairing from a browser
        // impossible in practice: an unpaired page load fires a dozen API calls before the
        // operator can type anything, so the device locked itself out of `/api/pair` on
        // arrival, every time, and the phrase never got a chance to be wrong. Nothing is
        // given away by not counting it either: a request with no credential carries no
        // guess, so there is nothing to learn from repeating it, and every actual guess --
        // a wrong token here, a wrong phrase at `/api/pair` -- is still counted against the
        // same budget.
        None => unauthorized(),
    }
}

#[derive(Deserialize)]
struct PairRequest {
    /// The twelve-word phrase, or the eight-character code the pairing sequence put on
    /// screen. One field for both because a device is answering one question -- "what did
    /// the other machine tell you" -- and which kind of secret it is, is for this end to
    /// work out, not for the person typing.
    phrase: String,
    /// What to call this device in the list. Optional: a browser will not send one, so the
    /// User-Agent stands in, and an operator can always tell one entry from another by when
    /// it paired even if both are called the same thing.
    #[serde(default)]
    device: Option<String>,
}

/// The name a device gets when it did not choose one. A User-Agent is long and full of
/// things nobody wants to read, so this keeps the part that identifies the browser and the
/// system and throws the version soup away.
fn label_from_user_agent(request_headers: &header::HeaderMap, peer: SocketAddr) -> String {
    let agent = request_headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let browser = ["Firefox", "Edg", "Chrome", "Safari", "curl", "AETHER1"]
        .into_iter()
        .find(|name| agent.contains(name));
    let system = ["Windows", "Macintosh", "Linux", "Android", "iPhone"]
        .into_iter()
        .find(|name| agent.contains(name));
    match (browser, system) {
        (Some(browser), Some(system)) => format!("{browser} on {system}"),
        (Some(browser), None) => browser.to_string(),
        (None, Some(system)) => system.to_string(),
        // Nothing recognisable: the address it paired from is at least something the
        // operator can match against a machine they own.
        (None, None) => format!("a device at {}", peer.ip()),
    }
}

/// The one route under `--lan` that needs no token -- it's what produces one. `phrase` is
/// checked and discarded here; only the derived token it resolves to ever goes back over the
/// wire, matching serve_auth's rule that the phrase itself never leaves the two ends that
/// already know it (the person who read it off this machine, and whoever they typed it into).
async fn pair(
    State(lan): State<LanState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: header::HeaderMap,
    Json(req): Json<PairRequest>,
) -> Response {
    let ip = peer.ip();
    if let Some(wait) = lan.limiter.retry_after(ip) {
        return too_many_attempts(wait);
    }
    // The one-time code is tried first, and a code that works ends the matter. It is the
    // shorter of the two and cannot be mistaken for a mnemonic, so there is no case where
    // one secret has to be told apart from the other.
    let by_code = lan.auth.code_matches(&req.phrase);
    if !by_code {
        // A phrase that isn't valid BIP-39 counts as a failure too: it is still a guess, and
        // letting malformed ones through free would make the budget trivial to avoid.
        let Ok(token) = serve_auth::derive_token_from_phrase(&req.phrase) else {
            lan.limiter.record_failure(ip);
            return (
                StatusCode::BAD_REQUEST,
                "that is not the pairing code or the pairing phrase".to_string(),
            )
                .into_response();
        };
        if !lan.auth.phrase_matches(&token) {
            lan.limiter.record_failure(ip);
            return (StatusCode::UNAUTHORIZED, "wrong pairing phrase".to_string()).into_response();
        }
    }

    lan.limiter.record_success(ip);
    // Spent on the way in, so the code lets exactly one device through. The phrase is not
    // spent: it is the standing credential, and each device that types it gets its own token.
    if by_code {
        lan.auth.spend_code();
    }
    // The phrase is spent the moment it is checked: what goes back is a token minted for
    // this device alone, so revoking it later takes access from this machine and no other.
    let label = req
        .device
        .filter(|chosen| !chosen.trim().is_empty())
        .unwrap_or_else(|| label_from_user_agent(&headers, peer));
    match lan.auth.add_device(&label) {
        Ok(device_token) => Json(serde_json::json!({ "token": device_token })).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
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
    headers: header::HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    // A question relayed from another AETHER1 is answered here or not at all. Without this,
    // two machines each naming the other as their helper would pass one question back and
    // forth until both timed out -- which is exactly what the first live test did.
    let relayed = headers.contains_key(crate::peers::RELAY_HEADER);
    let engine = state.engine.clone();
    let message = req.message;
    let session_id =
        commands::valid_session_id(req.session_id).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let session_id_for_result = session_id.clone();

    let mut result = {
        let engine = engine.clone();
        let session_id = session_id.clone();
        tokio::task::spawn_blocking(move || {
            // Held for the whole answer: the flag is thread-local, and this is the thread
            // the answer is produced on.
            let _relaying = relayed.then(crate::peers::Relaying::begin);
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
            let engine = state.engine.clone();
            audio_url = tokio::task::spawn_blocking(move || {
                commands::synthesize_speech(&engine, &reply, None)
            })
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

/// Takes a WAV recording made in the page and returns what was said. The body is raw
/// audio rather than JSON: base64-ing a recording to wrap it in an object would inflate it
/// by a third for no benefit.
async fn stt(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Result<Json<Value>, (StatusCode, String)> {
    let engine = state.engine.clone();
    let text = tokio::task::spawn_blocking(move || commands::transcribe_audio(&engine, &body))
        .await
        .map_err(internal_error)?
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "text": text })))
}

async fn voice_status(State(state): State<AppState>) -> Json<Value> {
    Json(commands::voice_status(&state.engine))
}

/// The browser HUD's counterpart of `doctor_report_rust`. The facts a desktop process would
/// fill in are left unset on purpose: this process is a server, not the window, so it cannot
/// say whether a HUD is answering or a hotkey registered, and the report says so per row
/// rather than reporting a thing nobody asked as healthy.
///
/// It does say what it *can* see about serving, which the desktop app cannot: this is the
/// process that bound the port.
async fn doctor_report(State(state): State<AppState>) -> Json<Value> {
    let facts = crate::doctor::Facts {
        in_app: false,
        serving: Some(true),
        lan: Some(state.lan),
        ..Default::default()
    };
    Json(commands::doctor_report(&state.engine, facts))
}

#[derive(Deserialize)]
struct DoctorRepairRequest {
    check: String,
    repair: String,
}

/// One repair, asked for from the browser HUD's Diagnostics panel. Reachable only with a
/// valid token under --lan, like every other mutating route here; the in-app repairs are not
/// offered, because this process holds no window to make them with.
async fn doctor_repair(
    State(state): State<AppState>,
    Json(req): Json<DoctorRepairRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    commands::doctor_repair(&state.engine, &req.check, &req.repair, None)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

/// Every repair this process can make, asked for from the browser HUD. Same rule as the one
/// above: no in-app repairs, because this process holds no window to make them with, so the
/// hotkey comes back in `remaining` rather than being silently skipped.
async fn doctor_attend(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, String)> {
    commands::doctor_attend(&state.engine, crate::doctor::Facts::default(), None)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

/// The browser HUD's counterpart of `flow_mode_rust`, so the chin bar's STATIC/FLOW
/// toggle is the same switch on both transports.
async fn flow_mode(State(state): State<AppState>) -> Json<Value> {
    Json(commands::flow_mode(&state.engine))
}

#[derive(Deserialize)]
struct FlowModeRequest {
    enabled: bool,
}

async fn set_flow_mode(
    State(state): State<AppState>,
    Json(req): Json<FlowModeRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    commands::set_flow_mode(&state.engine, req.enabled)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

#[derive(Deserialize)]
struct FlowLineRequest {
    /// The line to pick whole, or null to release the one that is picked.
    line: Option<String>,
}

/// The browser HUD's counterpart of `set_flow_line_rust`: picking a whole cast from the
/// Avatars pane rather than one of its members.
async fn set_flow_line(
    State(state): State<AppState>,
    Json(req): Json<FlowLineRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    commands::set_flow_line(&state.engine, req.line)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

/// The Profile pane, for a browser HUD. Behind the same token as everything else under
/// `--lan`: it lists the paired devices, which is a list a paired device may see and an
/// unpaired one may not.
async fn profile_report(State(state): State<AppState>) -> Json<Value> {
    Json(crate::profile::report(&state.engine))
}

#[derive(Deserialize)]
struct RevokeRequest {
    id: String,
}

/// Revoking from a browser can cut off the browser doing the revoking -- that is the point
/// of `revoke all`, and the frontend says so before it asks.
async fn revoke_device(
    Json(req): Json<RevokeRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    crate::profile::revoke(&req.id)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn list_personas(State(state): State<AppState>) -> Json<Value> {
    Json(commands::list_personas(&state.engine))
}

/// The voices an avatar can be given, for the browser HUD's avatar pane.
async fn voice_pickers() -> Json<Value> {
    Json(commands::voice_pickers())
}

#[derive(Deserialize)]
struct PersonaVoiceRequest {
    persona: String,
    #[serde(default)]
    voice: Option<String>,
    #[serde(default)]
    local_voice: Option<String>,
}

/// Both halves of one avatar's voice choice. The reply is the whole persona list again, the
/// same shape `/api/personas` returns, so the pane redraws from one answer rather than
/// patching its own copy and hoping it matches what was stored.
async fn set_persona_voice(
    State(state): State<AppState>,
    Json(req): Json<PersonaVoiceRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    commands::set_persona_voice(&state.engine, req.persona, req.voice, req.local_voice)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

/// The pronunciation list, and the ceilings the pane shows rather than guesses at.
async fn pronunciations(State(state): State<AppState>) -> Json<Value> {
    Json(commands::pronunciations(&state.engine))
}

#[derive(Deserialize)]
struct PronunciationsRequest {
    words: Vec<crate::speech_words::Say>,
}

/// The whole list at once; the reply is the list as stored, for the pane to redraw from.
async fn set_pronunciations(
    State(state): State<AppState>,
    Json(req): Json<PronunciationsRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    commands::set_pronunciations(&state.engine, req.words)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

#[derive(Deserialize)]
struct PersonaRequest {
    persona: String,
}

async fn clear_persona_voice(
    State(state): State<AppState>,
    Json(req): Json<PersonaRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    commands::clear_persona_voice(&state.engine, req.persona)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn get_pending_actions(State(state): State<AppState>) -> Json<Vec<llm::ActionRecord>> {
    Json(commands::pending_actions(&state.engine))
}

async fn approve_action(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let engine = state.engine.clone();
    // Blocking: approving runs the tool, and a tool can take as long as the work takes.
    tokio::task::spawn_blocking(move || commands::approve_action(&engine, id))
        .await
        .map_err(internal_error)?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn undo_action(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let engine = state.engine.clone();
    tokio::task::spawn_blocking(move || commands::undo_action(&engine, id))
        .await
        .map_err(internal_error)?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn reject_action(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    commands::reject_action(&state.engine, id)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn persona_access(State(state): State<AppState>) -> Json<Value> {
    Json(commands::persona_access(&state.engine))
}

#[derive(Deserialize)]
struct PersonaAccessRequest {
    paths: Vec<String>,
}

async fn set_persona_access(
    State(state): State<AppState>,
    Json(req): Json<PersonaAccessRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    commands::set_persona_access(&state.engine, req.paths)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

#[derive(Deserialize)]
struct AlwaysAllowRequest {
    tool: String,
    allowed: bool,
}

async fn set_always_allowed(
    State(state): State<AppState>,
    Json(req): Json<AlwaysAllowRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    commands::set_always_allowed(&state.engine, req.tool, req.allowed)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

#[derive(Deserialize)]
struct TestConnectionRequest {
    provider: String,
    model: String,
    endpoint: String,
    api_key: String,
}

/// The browser-flow counterpart of test_llm_connection_rust -- same unsaved-form-values
/// probe, reached over HTTP instead of Tauri IPC.
async fn test_llm_connection(
    State(state): State<AppState>,
    Json(req): Json<TestConnectionRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let engine = state.engine.clone();
    tokio::task::spawn_blocking(move || {
        commands::test_llm_connection(&engine, req.provider, req.model, req.endpoint, req.api_key)
    })
    .await
    .map_err(internal_error)?
    .map(|message| Json(serde_json::json!({ "message": message })))
    .map_err(|e| (StatusCode::BAD_REQUEST, e))
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
    let engine_for_tts = state.engine.clone();
    let audio_url = tokio::task::spawn_blocking(move || {
        commands::synthesize_speech(&engine_for_tts, &greeting, voice.as_deref())
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

/// Where this machine is on the road to having a model. Read-only probes plus two settings
/// lookups -- nothing here changes anything, so it is a GET and needs no approval.
async fn setup_advice(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(
        tokio::task::spawn_blocking(move || commands::setup_advice(&state.engine))
            .await
            .expect("setup_advice panicked"),
    )
}

/// Browser-transport twin of voice_advice_rust.
async fn code_advice(State(state): State<AppState>) -> Json<crate::code_setup::CodingAdvice> {
    Json(
        tokio::task::spawn_blocking(move || commands::code_advice(&state.engine))
            .await
            .expect("code_advice panicked"),
    )
}

/// The house rules as plain text, so a browser can copy them and a terminal can redirect
/// them straight into an AGENTS.md.
async fn code_conventions() -> String {
    crate::code_setup::conventions()
}

#[derive(Deserialize)]
struct CodeChatRequest {
    message: String,
}

/// The browser twin of code_chat_ask_rust.
///
/// Answered whole rather than streamed, because the browser fallback has no event channel
/// and a second streaming transport is a lot of machinery for a panel whose buttons do not
/// work here anyway -- the terminal exists only in the native window, so a browser gets the
/// answer and the commands to copy, and nothing to press.
async fn code_chat(
    State(state): State<AppState>,
    Json(req): Json<CodeChatRequest>,
) -> Result<Json<crate::code_chat::CodeReply>, (StatusCode, String)> {
    let engine = state.engine.clone();
    let reply = tokio::task::spawn_blocking(move || {
        commands::code_chat_ask(&engine, &req.message, &mut |_| {})
    })
    .await
    .map_err(internal_error)?
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(reply))
}

async fn code_chat_history(State(state): State<AppState>) -> Json<Vec<Value>> {
    Json(
        tokio::task::spawn_blocking(move || commands::code_chat_history(&state.engine))
            .await
            .expect("code_chat_history panicked"),
    )
}

async fn code_chat_clear(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || commands::code_chat_clear(&state.engine))
        .await
        .map_err(internal_error)?
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Browser-transport twin of code_set_level_rust.
async fn code_level(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let level = body
        .get("level")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let report =
        tokio::task::spawn_blocking(move || commands::code_set_level(&state.engine, &level))
            .await
            .map_err(internal_error)?
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(report))
}

async fn voice_advice(State(state): State<AppState>) -> Json<crate::voice_setup::VoiceAdvice> {
    Json(
        tokio::task::spawn_blocking(move || commands::voice_advice(&state.engine))
            .await
            .expect("voice_advice panicked"),
    )
}

/// Browser-transport twin of test_speech_rust. The page has no file paths, so the audio
/// comes back as a URL under /api/audio like every other piece of speech here.
async fn test_speech(State(state): State<AppState>) -> Json<serde_json::Value> {
    let outcome = tokio::task::spawn_blocking(move || commands::test_speech(&state.engine))
        .await
        .expect("test_speech panicked");
    Json(match outcome {
        Ok((path, mut report)) => {
            if let Some(file_name) = path.file_name() {
                report["audio_url"] =
                    serde_json::json!(format!("/api/audio/{}", file_name.to_string_lossy()));
            }
            report
        }
        Err(report) => report,
    })
}

#[derive(Deserialize)]
struct PullQuery {
    model_name: Option<String>,
}

async fn pull_model(
    State(state): State<AppState>,
    Query(q): Query<PullQuery>,
) -> Json<model_scanner::PullResult> {
    let name = q.model_name.unwrap_or_default();
    let engine = state.engine.clone();
    Json(
        tokio::task::spawn_blocking(move || commands::pull_model(&engine, name))
            .await
            .expect("pull_model panicked"),
    )
}

#[derive(Deserialize)]
struct DownloadQuery {
    model_name: Option<String>,
    endpoint: Option<String>,
}

async fn start_download(
    State(state): State<AppState>,
    Query(q): Query<DownloadQuery>,
) -> Json<serde_json::Value> {
    let engine = state.engine.clone();
    let name = q.model_name.unwrap_or_default();
    let endpoint = q.endpoint.unwrap_or_default();
    Json(
        tokio::task::spawn_blocking(move || commands::start_download(&engine, name, endpoint))
            .await
            .expect("start_download panicked"),
    )
}

// Read-only, and the one route here polled on a timer -- it reads a map in memory and
// touches neither the database nor the network, so a bar on screen costs nothing.
async fn download_status() -> Json<serde_json::Value> {
    Json(commands::download_status())
}

async fn forget_download(Query(q): Query<DownloadQuery>) -> Json<serde_json::Value> {
    Json(commands::forget_download(q.model_name.unwrap_or_default()))
}

#[derive(Deserialize)]
struct VoiceQuery {
    voice: Option<String>,
}

async fn voice_catalogue() -> Json<serde_json::Value> {
    Json(commands::voice_catalogue())
}

async fn start_voice_download(
    State(state): State<AppState>,
    Query(q): Query<VoiceQuery>,
) -> Json<serde_json::Value> {
    let engine = state.engine.clone();
    let voice = q.voice.unwrap_or_default();
    Json(
        tokio::task::spawn_blocking(move || commands::start_voice_download(&engine, voice))
            .await
            .expect("start_voice_download panicked"),
    )
}

async fn voice_download_status() -> Json<serde_json::Value> {
    Json(commands::voice_download_status())
}

async fn forget_voice_download(Query(q): Query<VoiceQuery>) -> Json<serde_json::Value> {
    Json(commands::forget_voice_download(q.voice.unwrap_or_default()))
}

async fn audio_devices() -> Json<serde_json::Value> {
    Json(
        tokio::task::spawn_blocking(commands::audio_devices)
            .await
            .expect("audio_devices panicked"),
    )
}

// Spawns the `ollama` already installed on this machine, with a fixed argument and no
// shell. Exposed here as well as over IPC because the browser fallback hits the same dead
// end as the native app does -- a server that is installed, not running, and nothing able
// to start it. Same class of action as pull-model, which has spawned that binary all along.
async fn start_local_server(State(state): State<AppState>) -> Json<serde_json::Value> {
    let engine = state.engine.clone();
    Json(
        tokio::task::spawn_blocking(move || commands::start_local_server(&engine))
            .await
            .expect("start_local_server panicked"),
    )
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<u32>,
    session_id: Option<String>,
}

async fn get_messages(
    State(state): State<AppState>,
    Query(q): Query<LimitQuery>,
) -> Result<Json<Vec<llm::Message>>, (StatusCode, String)> {
    let engine = state.engine.clone();
    let rows =
        tokio::task::spawn_blocking(move || commands::get_messages(&engine, q.limit, q.session_id))
            .await
            .map_err(internal_error)?
            // A refused conversation id is the caller's mistake, not the server's.
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(rows))
}

async fn clear_messages(
    State(state): State<AppState>,
    Query(q): Query<LimitQuery>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let engine = state.engine.clone();
    tokio::task::spawn_blocking(move || commands::clear_messages(&engine, q.session_id))
        .await
        .map_err(internal_error)?
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "status": "cleared" })))
}

#[derive(Deserialize)]
struct SessionBody {
    session_id: Option<String>,
    title: Option<String>,
}

async fn list_sessions(
    State(state): State<AppState>,
) -> Result<Json<Vec<llm::SessionSummary>>, (StatusCode, String)> {
    let engine = state.engine.clone();
    let rows = tokio::task::spawn_blocking(move || commands::list_sessions(&engine))
        .await
        .map_err(internal_error)?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(rows))
}

/// Mints an id and writes nothing. Deliberately a POST even though it has no side effect
/// on the database, because it is not idempotent -- asking twice is asking for two
/// conversations -- and a GET that returns something different every time is a trap for
/// every cache between here and the page.
async fn new_session() -> Json<Value> {
    Json(serde_json::json!({ "session_id": commands::new_session() }))
}

async fn rename_session(
    State(state): State<AppState>,
    Json(req): Json<SessionBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let engine = state.engine.clone();
    let title = req.title.unwrap_or_default();
    tokio::task::spawn_blocking(move || commands::rename_session(&engine, req.session_id, title))
        .await
        .map_err(internal_error)?
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "status": "renamed" })))
}

async fn delete_session(
    State(state): State<AppState>,
    Json(req): Json<SessionBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let engine = state.engine.clone();
    let id = req.session_id.unwrap_or_default();
    tokio::task::spawn_blocking(move || commands::delete_session(&engine, id))
        .await
        .map_err(internal_error)?
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

async fn reset_benchmarks(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let engine = state.engine.clone();
    tokio::task::spawn_blocking(move || commands::reset_benchmarks(&engine))
        .await
        .map_err(internal_error)?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "status": "cleared" })))
}

/// Opens the vault folder in the file manager on this machine -- see commands::
/// open_vault_folder for why that's the server's desktop rather than the requester's,
/// under `--lan`.
async fn open_vault_folder(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let engine = state.engine.clone();
    tokio::task::spawn_blocking(move || commands::open_vault_folder(&engine))
        .await
        .map_err(internal_error)?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "status": "opened" })))
}

/// The reader's four routes. All read-only, and all bound by the same rule as the rest of
/// the server: over `--lan` they are behind the pairing token, and they can no more reach
/// outside the vault folder than the companion's own tools can -- vault::reader checks
/// every name it is handed, both for its shape and, after canonicalising, for where it
/// actually lands.
async fn vault_notes(State(state): State<AppState>) -> Json<Vec<vault::reader::NoteSummary>> {
    let engine = state.engine.clone();
    Json(
        tokio::task::spawn_blocking(move || commands::vault_notes(&engine))
            .await
            .unwrap_or_default(),
    )
}

#[derive(Deserialize)]
struct NoteQuery {
    name: String,
}

async fn vault_note(
    State(state): State<AppState>,
    Query(q): Query<NoteQuery>,
) -> Result<Json<vault::reader::NoteView>, (StatusCode, String)> {
    let engine = state.engine.clone();
    let view = tokio::task::spawn_blocking(move || commands::vault_note(&engine, &q.name))
        .await
        .map_err(internal_error)?
        // A refused name is the caller's mistake, not the server's: 404 rather than 500,
        // and the same answer whether the note is missing or the path was never allowed,
        // so a probe cannot use the difference to map the disk.
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(view))
}

async fn vault_graph(State(state): State<AppState>) -> Json<Value> {
    let engine = state.engine.clone();
    let graph = tokio::task::spawn_blocking(move || commands::vault_graph(&engine)).await;
    Json(match graph {
        Ok(g) => serde_json::to_value(g).unwrap_or_else(|_| serde_json::json!({})),
        Err(_) => serde_json::json!({}),
    })
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn vault_search(State(state): State<AppState>, Query(sq): Query<SearchQuery>) -> Json<Value> {
    let engine = state.engine.clone();
    Json(
        tokio::task::spawn_blocking(move || commands::vault_search(&engine, &sq.q))
            .await
            .unwrap_or_else(|_| serde_json::json!({})),
    )
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
    // The local engine writes wav, the cloud one mp3; serving both as audio/mpeg makes
    // some browsers refuse to play the wav.
    let content_type = match path.extension().and_then(|e| e.to_str()) {
        Some("wav") => "audio/wav",
        _ => "audio/mpeg",
    };
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
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
async fn tts(
    State(state): State<AppState>,
    Json(req): Json<TtsRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let engine = state.engine.clone();
    let path = tokio::task::spawn_blocking(move || {
        commands::synthesize_speech(&engine, &req.text, req.voice.as_deref())
    })
    .await
    .map_err(internal_error)?
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            "no audio file".to_string(),
        ))?;

    Ok(Json(
        serde_json::json!({ "audio_url": format!("/api/audio/{file_name}") }),
    ))
}

async fn ws_chat(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.protocols([WS_PROTOCOL])
        .on_upgrade(move |socket| chat_socket(socket, state))
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

    let Ok(session_id) = commands::valid_session_id(req.session_id) else {
        let _ = socket
            .send(Message::Text(
                serde_json::json!({"type": "error", "error": "bad conversation id"})
                    .to_string()
                    .into(),
            ))
            .await;
        return;
    };
    let engine = state.engine.clone();
    let message = req.message;

    // The engine is blocking and knows nothing about async, so it runs on a blocking task
    // and pushes deltas through a channel that this task forwards to the socket. Unbounded
    // so a slow client can never block generation itself -- the deltas are small and the
    // reply is bounded by the model's own output.
    // Two kinds of thing come back over one channel, in order: at most one hand-off, then
    // the reply's deltas. One channel rather than two because the order between them is the
    // whole point -- the node leaving speaks before the node arriving starts answering.
    enum Chunk {
        Handover(serde_json::Value),
        Delta(String),
    }

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Chunk>();
    let generation = {
        let session_id = session_id.clone();
        let handover_tx = tx.clone();
        tokio::task::spawn_blocking(move || {
            commands::generate_response_streamed(
                &engine,
                message,
                Some(session_id),
                &mut |delta| {
                    let _ = tx.send(Chunk::Delta(delta.to_string()));
                },
                &mut |handover| {
                    let _ = handover_tx.send(Chunk::Handover(commands::handover_json(handover)));
                },
            )
        })
    };

    while let Some(chunk) = rx.recv().await {
        let frame = match chunk {
            Chunk::Handover(payload) => {
                let mut frame = payload;
                if let Some(object) = frame.as_object_mut() {
                    object.insert(
                        "type".to_string(),
                        serde_json::Value::String("handover".to_string()),
                    );
                }
                frame
            }
            Chunk::Delta(delta) => serde_json::json!({"type": "delta", "delta": delta}),
        };
        if socket
            .send(Message::Text(frame.to_string().into()))
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
            let engine = state.engine.clone();
            audio_url = tokio::task::spawn_blocking(move || {
                commands::synthesize_speech(&engine, &reply, None)
            })
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
    // Subscribe first, then wake: the loop checks receiver_count(), so waking before there
    // is a receiver to count would park it again for a whole NO_CLIENT_RECHECK.
    state.telemetry_pulse.wake();
    // `protocols` selects WS_PROTOCOL when the client offered it, and sets no header when it
    // offered nothing -- which is what a loopback tab that has no token to carry does. The
    // token subprotocol is deliberately not listed, so it is never echoed back.
    ws.protocols([WS_PROTOCOL])
        .on_upgrade(move |socket| telemetry_socket(socket, rx))
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

#[cfg(test)]
mod bind_tests {
    use super::bind_address;

    /// The default is the whole point of the flag, so it is asserted rather than assumed.
    /// Someone changing this line should have to change a test that says why.
    use super::SERVE_PORT;

    #[test]
    fn serving_without_lan_listens_only_on_the_loopback_address() {
        assert_eq!(bind_address(false), "127.0.0.1:8378");
    }

    #[test]
    fn lan_opts_in_to_every_interface() {
        assert_eq!(bind_address(true), "0.0.0.0:8378");
    }

    /// Whatever else changes, the port is the one thing start.sh, start.bat and the
    /// README all hard-code.
    #[test]
    fn both_modes_use_the_documented_port() {
        for address in [bind_address(false), bind_address(true)] {
            assert!(
                address.ends_with(&format!(":{SERVE_PORT}")),
                "unexpected port in {address}"
            );
        }
    }
}

#[cfg(test)]
mod ws_credential_tests {
    use super::*;

    fn handshake(header_value: Option<&str>) -> Request {
        let mut builder = Request::builder().uri("/ws/telemetry");
        if let Some(value) = header_value {
            builder = builder.header(header::SEC_WEBSOCKET_PROTOCOL, value);
        }
        builder.body(Body::empty()).expect("a request to build")
    }

    /// The point of the change: the credential is read from the handshake header and from
    /// nowhere else. A query string is the part of a request that gets written into logs, so
    /// a token offered there is not a token at all -- and this test is the only thing that
    /// stops it quietly coming back as a convenience.
    #[test]
    fn a_socket_token_is_read_from_the_handshake_and_not_from_the_url() {
        let secret = "a".repeat(64);

        assert_eq!(
            token_from_subprotocol(&handshake(Some(&format!(
                "{WS_TOKEN_PREFIX}{secret}, {WS_PROTOCOL}"
            )))),
            Some(secret.clone())
        );
        // Order is the client's to choose, and whitespace after a comma is normal.
        assert_eq!(
            token_from_subprotocol(&handshake(Some(&format!(
                "{WS_PROTOCOL},{WS_TOKEN_PREFIX}{secret}"
            )))),
            Some(secret.clone())
        );

        // A loopback tab offers the plain subprotocol and has no token to give.
        assert_eq!(token_from_subprotocol(&handshake(Some(WS_PROTOCOL))), None);
        assert_eq!(token_from_subprotocol(&handshake(None)), None);
        // The prefix with nothing after it is not an empty token, it is no token: an empty
        // string reaching `accepts` is a comparison nobody meant to make.
        assert_eq!(
            token_from_subprotocol(&handshake(Some(WS_TOKEN_PREFIX))),
            None
        );

        // And the URL is not consulted, however plausible the parameter looks.
        let in_the_url = Request::builder()
            .uri(format!("/ws/telemetry?token={secret}"))
            .body(Body::empty())
            .expect("a request to build");
        assert_eq!(token_from_subprotocol(&in_the_url), None);
        assert_eq!(bearer_token(&in_the_url), None);
    }

    /// The server selects the plain subprotocol and never the one carrying the token, so the
    /// credential travels in one direction. A browser closes a socket whose requested
    /// subprotocol was not selected, which is why the plain one has to exist at all.
    #[test]
    fn the_server_never_echoes_the_token_back() {
        assert!(!WS_PROTOCOL.starts_with(WS_TOKEN_PREFIX));
        assert!(WS_TOKEN_PREFIX.starts_with(WS_PROTOCOL));
        // The frontend's copy of these two names has to match, since a mismatch would fail
        // every socket from a paired browser with no error anyone could read.
        let lan_auth = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../frontend/js/lan-auth.js"),
        )
        .expect("lan-auth.js should be readable");
        assert!(
            lan_auth.contains(&format!("const WS_PROTOCOL = '{WS_PROTOCOL}';")),
            "lan-auth.js does not agree with WS_PROTOCOL"
        );
        assert!(
            lan_auth.contains(&format!("const WS_TOKEN_PREFIX = '{WS_TOKEN_PREFIX}';")),
            "lan-auth.js does not agree with WS_TOKEN_PREFIX"
        );
    }
}
