// AETHER1 native shell (incremental Rust port).
//
// This launches the existing Python/FastAPI backend as a managed child process so the
// whole app is a single thing to run -- no separate "start the server, then open a
// browser tab" step. The webview loads the same frontend/ files used by the browser
// flow, but talks to the backend over an absolute http://localhost:8378 URL (see
// frontend/js/app.js's API_BASE) since a Tauri-loaded page isn't served from that origin.
//
// Backend logic itself (chat, TTS, telemetry, memory) is still Python for now; Rust
// commands will replace pieces of it over time.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod llm;

use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::Manager;

use llm::{LlmEngine, MemoryDb};

const BACKEND_HOST: &str = "127.0.0.1";
const BACKEND_PORT: u16 = 8378;

const UPDATE_REPO: &str = "TridentSpoon/Aether1";
const TRAY_ID: &str = "main-tray";

/// Set by build.rs from `git rev-parse HEAD` at compile time; "unknown" if this wasn't
/// built from a git checkout (e.g. a source tarball without a .git directory).
const BUILT_COMMIT: &str = env!("AETHER1_GIT_COMMIT");

/// CARGO_MANIFEST_DIR is src-tauri/ at build time; the Python backend, its venv, and
/// frontend/ all live one level up, at the repo root.
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri should have a parent directory")
        .to_path_buf()
}

fn backend_already_running() -> bool {
    let addr: SocketAddr = format!("{BACKEND_HOST}:{BACKEND_PORT}")
        .parse()
        .expect("static host:port should always parse");
    TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
}

fn spawn_backend() -> Option<Child> {
    if backend_already_running() {
        println!(
            "[AETHER1] Backend already running on port {BACKEND_PORT}; using it instead of spawning another one."
        );
        return None;
    }

    let root = project_root();
    let uvicorn = root.join("venv").join("bin").join("uvicorn");

    if !uvicorn.exists() {
        eprintln!(
            "[AETHER1] Could not find {}. Run ./setup.sh from the project root first to create the Python venv.",
            uvicorn.display()
        );
        return None;
    }

    let mut cmd = Command::new(&uvicorn);
    cmd.args([
        "backend.main:app",
        "--host",
        BACKEND_HOST,
        "--port",
        &BACKEND_PORT.to_string(),
    ])
    .current_dir(&root);

    // Ask the kernel to auto-kill this child if we (the parent) die for any reason --
    // window closed, killed externally, crashed, logged out, etc. This is the actually
    // reliable mechanism; catching signals in the parent process (see main()) races
    // against Tauri/tao's own internal signal handling and isn't dependable on its own --
    // confirmed by testing: `kill <tauri-pid>` left the child running without this.
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    match cmd.spawn() {
        Ok(child) => {
            println!("[AETHER1] Spawned backend (pid {})", child.id());
            Some(child)
        }
        Err(e) => {
            eprintln!("[AETHER1] Failed to spawn backend via {}: {e}", uvicorn.display());
            None
        }
    }
}

fn kill_backend(slot: &Mutex<Option<Child>>) {
    if let Ok(mut guard) = slot.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Deserialize)]
struct GhCommit {
    sha: String,
}

fn short_hash(hash: &str) -> &str {
    &hash[..hash.len().min(7)]
}

/// AETHER1's GitHub repo is private, so an unauthenticated request 404s. Shell out to the
/// `gh` CLI for a token if it's installed and already logged in (as it is on a dev machine
/// that can push to this repo at all) -- there's no bundled/embedded credential. Returns
/// None (not an error) if `gh` is missing or not authenticated; the caller just skips auth
/// and lets the request fail normally.
fn github_token() -> Option<String> {
    let output = Command::new("gh").args(["auth", "token"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// Blocking GET against GitHub's REST API for the latest commit on `main`. Always call
/// this off the main thread -- it can take up to the timeout below if the network is slow
/// or absent, and must never hold up the tray or the window.
fn fetch_latest_main_sha() -> Result<String, String> {
    let url = format!("https://api.github.com/repos/{UPDATE_REPO}/commits/main");
    let mut request = ureq::get(&url)
        .config()
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .header("User-Agent", "AETHER1-desktop-app")
        .header("Accept", "application/vnd.github+json");

    if let Some(token) = github_token() {
        request = request.header("Authorization", format!("Bearer {token}"));
    }

    let response = request.call().map_err(|e| e.to_string())?;

    response
        .into_body()
        .read_json::<GhCommit>()
        .map(|c| c.sha)
        .map_err(|e| e.to_string())
}

/// Compares the running build's baked-in commit (see BUILT_COMMIT / build.rs) against the
/// latest commit on `main`, and reflects the result in the tray tooltip and the "Check for
/// Updates" menu item's label. Safe to call from any thread; never panics or blocks the
/// caller beyond the network timeout in fetch_latest_main_sha.
fn run_update_check<R: tauri::Runtime>(tray: &TrayIcon<R>, update_item: &MenuItem<R>) {
    if BUILT_COMMIT == "unknown" {
        // Not built from a git checkout -- nothing to compare against.
        return;
    }

    match fetch_latest_main_sha() {
        Ok(latest) if latest == BUILT_COMMIT => {
            println!(
                "[AETHER1] Update check: up to date (build {})",
                short_hash(BUILT_COMMIT)
            );
            let _ = tray.set_tooltip(Some(format!(
                "AETHER1 -- running (build {}, up to date)",
                short_hash(BUILT_COMMIT)
            )));
            let _ = update_item.set_text("✅ Up to Date");
        }
        Ok(latest) => {
            println!(
                "[AETHER1] Update check: new commit available (running {}, latest {})",
                short_hash(BUILT_COMMIT),
                short_hash(&latest)
            );
            let _ = tray.set_tooltip(Some(format!(
                "AETHER1 -- update available (running {}, latest {}) -- run: git pull && ./setup.sh",
                short_hash(BUILT_COMMIT),
                short_hash(&latest)
            )));
            let _ = update_item.set_text("⬆ Update Available (git pull)");
        }
        Err(e) => {
            eprintln!("[AETHER1] Update check failed: {e}");
            let _ = update_item.set_text("🔄 Check for Updates");
        }
    }
}

/// Rust-native equivalent of POST /api/chat's LLM call (backend/main.py), calling straight
/// into the ported llm::LlmEngine instead of the Python backend. Runs alongside the
/// existing Python /api/chat route rather than replacing it -- the frontend can opt into
/// this per-call, and both read/write the same SQLite file, so switching between them
/// mid-conversation doesn't lose history.
#[tauri::command]
fn generate_response_rust(
    engine: tauri::State<LlmEngine>,
    prompt: String,
    session_id: Option<String>,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("Empty message".to_string());
    }
    let session_id = session_id.unwrap_or_else(|| "default".to_string());

    engine.add_message(&session_id, "user", &prompt);
    let reply = engine.generate_response(&prompt, &session_id);
    let agent_name = engine.agent_name();
    engine.add_message(&session_id, &agent_name.to_lowercase(), &reply);

    Ok(reply)
}

/// Rust-native equivalent of POST /api/agent/genesis (backend/main.py), minus TTS audio
/// generation -- that stays Python-only for now (see backend/tts_engine.py).
#[tauri::command]
fn agent_genesis_rust(
    engine: tauri::State<LlmEngine>,
    purpose: String,
) -> Result<serde_json::Value, String> {
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

fn main() {
    let backend: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(spawn_backend()));

    let llm_engine = {
        let db_path = project_root().join("backend").join("aether1_memory.db");
        match MemoryDb::open(&db_path) {
            Ok(db) => LlmEngine::new(db),
            Err(e) => {
                // NOT ":memory:" -- MemoryDb opens a fresh connection per call (matching
                // memory_db.py's own pattern, which is fine for a real file), so a literal
                // SQLite ":memory:" database would be destroyed and recreated empty on
                // every single call. A real temp file actually persists for the session.
                let fallback_path = std::env::temp_dir().join("aether1_fallback_memory.db");
                eprintln!(
                    "[AETHER1] Could not open {} ({e}); the Rust LLM engine will use {} for this session \
                     instead (won't be shared with the Python backend).",
                    db_path.display(),
                    fallback_path.display()
                );
                LlmEngine::new(
                    MemoryDb::open(&fallback_path)
                        .expect("fallback sqlite path should always open"),
                )
            }
        }
    };

    // Defense in depth: also clean up the backend on SIGINT/SIGTERM (e.g. the app being
    // killed from a terminal, or a system shutdown/logout), not just when Tauri's own
    // window-close event fires. Confirmed by testing that `kill <pid>` on the Tauri
    // process alone left the spawned uvicorn process running otherwise.
    {
        let backend_for_signal = backend.clone();
        let _ = ctrlc::set_handler(move || {
            kill_backend(&backend_for_signal);
            std::process::exit(0);
        });
    }

    let backend_for_exit = backend.clone();
    tauri::Builder::default()
        .manage(llm_engine)
        .invoke_handler(tauri::generate_handler![
            generate_response_rust,
            agent_genesis_rust
        ])
        .setup(|app| {
            // Native tray icon so there's a visible indicator (and a quick way to
            // reopen/quit) while AETHER1 runs headlessly in the background.
            let show_item = MenuItem::with_id(app, "show", "Show AETHER1", true, None::<&str>)?;
            let update_item = MenuItem::with_id(
                app,
                "check_update",
                "🔄 Check for Updates",
                true,
                None::<&str>,
            )?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit AETHER1", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &update_item, &quit_item])?;

            let tray = TrayIconBuilder::with_id(TRAY_ID)
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip(format!(
                    "AETHER1 -- running (build {})",
                    short_hash(BUILT_COMMIT)
                ))
                .menu(&tray_menu)
                .show_menu_on_left_click(true)
                .on_menu_event({
                    let update_item = update_item.clone();
                    move |app, event| match event.id.as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "check_update" => {
                            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                                let update_item = update_item.clone();
                                std::thread::spawn(move || run_update_check(&tray, &update_item));
                            }
                        }
                        "quit" => app.exit(0),
                        _ => {}
                    }
                })
                .build(app)?;

            // Check once on launch, off the main thread so a slow/absent network never
            // delays showing the window.
            {
                let tray = tray.clone();
                std::thread::spawn(move || run_update_check(&tray, &update_item));
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |_app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                kill_backend(&backend_for_exit);
            }
        });
}
