// AETHER1 native shell.
//
// The whole app -- chat, TTS, telemetry, memory/settings, model scanning, self-updating --
// runs natively in this process; nothing is spawned or shelled out to at runtime except the
// update mechanism's own `git pull` (see perform_update_core) and TTS/update-check's network
// calls. The webview loads the same frontend/ files used by the old browser flow, but talks
// to this process over Tauri's IPC (see frontend/js/app.js's IS_TAURI branches) instead of
// HTTP -- there's no server listening on any port.
//
// Running this same binary with `--serve` instead launches a headless axum HTTP server
// (server.rs) exposing the REST/WebSocket surface frontend/js/app.js's non-Tauri fallback
// path expects, reusing the exact same commands:: functions the Tauri commands below call.
// That's the browser-based dev flow's backend now (./start.sh / ./start_daemon.sh) -- the
// Python backend/ tree is gone.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod llm;
mod model_scanner;
mod server;

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{Emitter, Manager};

use llm::{LlmEngine, MemoryDb};

const UPDATE_REPO: &str = "TridentSpoon/Aether1";
const TRAY_ID: &str = "main-tray";
const MAIN_LABEL: &str = "main";
const SPRITE_LABEL: &str = "sprite";

/// Set by build.rs from `git rev-parse HEAD` at compile time; "unknown" if this wasn't
/// built from a git checkout (e.g. a source tarball without a .git directory).
const BUILT_COMMIT: &str = env!("AETHER1_GIT_COMMIT");

/// Set by build.rs: the PR number of the most recently merged pull request reachable from
/// the built commit (see build.rs for how this is derived). "0" if none was found (e.g. a
/// checkout before any PR had ever been merged).
const BUILT_PR_REV: &str = env!("AETHER1_PR_REV");

/// Aether1 0.3.Rev{N} -- 0.x because still in dev; the 3 marks the project's third era
/// (1: Antigravity project, 2: ported to Claude, 3: native Rust/Tauri rewrite); Rev{N} is
/// the PR number this build was built from, so the version always tracks the last merge
/// without needing a hand-maintained counter.
const APP_VERSION: &str = concat!("Aether1 0.3.Rev", env!("AETHER1_PR_REV"));

/// CARGO_MANIFEST_DIR is src-tauri/ at build time; frontend/ and the backend/ data
/// directory (aether1_memory.db, audio_cache/) all live one level up, at the repo root.
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri should have a parent directory")
        .to_path_buf()
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
///
/// This -- and perform_update's `git pull`, which relies the same way on whatever
/// credentials this machine's `git` is already configured with (an SSH key with push
/// access to this repo, in practice) -- is a private-repo-only interim measure. If this
/// project ever goes public, both need replacing with a real public update mechanism (e.g.
/// Tauri's signed-updater plugin against public release artifacts) that doesn't assume the
/// end user has any credentials for this repo at all.
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

/// Result of comparing this build against the latest commit on `main` -- the shared shape
/// returned to the frontend (see check_for_update_rust) and used internally by the tray's
/// run_update_check to decide what to show. `checked` is false when the comparison itself
/// couldn't complete (see `error`); `up_to_date` is only meaningful when `checked` is true.
#[derive(serde::Serialize)]
struct UpdateStatus {
    checked: bool,
    up_to_date: bool,
    version: String,
    built_commit: String,
    built_commit_short: String,
    latest_commit: Option<String>,
    error: Option<String>,
}

/// Does the actual comparison: this build's baked-in commit (BUILT_COMMIT / build.rs)
/// against the latest commit on `main` via the GitHub API. Safe to call from any thread;
/// never panics or blocks the caller beyond the network timeout in fetch_latest_main_sha.
fn compute_update_status() -> UpdateStatus {
    let base = UpdateStatus {
        checked: false,
        up_to_date: false,
        version: APP_VERSION.to_string(),
        built_commit: BUILT_COMMIT.to_string(),
        built_commit_short: short_hash(BUILT_COMMIT).to_string(),
        latest_commit: None,
        error: None,
    };

    if BUILT_COMMIT == "unknown" {
        return UpdateStatus {
            error: Some("not built from a git checkout -- nothing to compare against".to_string()),
            ..base
        };
    }

    match fetch_latest_main_sha() {
        Ok(latest) => UpdateStatus {
            checked: true,
            up_to_date: latest == BUILT_COMMIT,
            latest_commit: Some(latest),
            ..base
        },
        Err(e) => UpdateStatus {
            error: Some(e),
            ..base
        },
    }
}

/// Rust-native equivalent for the frontend of what the tray's "Check for Updates" click
/// does -- lets the HUD show the same real version check instead of only being visible via
/// the taskbar icon. Does not itself update `update_available`/the tray UI; those stay
/// tray-only state (see run_update_check).
#[tauri::command]
fn check_for_update_rust() -> UpdateStatus {
    compute_update_status()
}

/// Rust-native equivalent for the frontend of get_version_info -- current version string
/// plus the exact commit this build came from, for display in the HUD.
#[tauri::command]
fn get_version_info() -> serde_json::Value {
    serde_json::json!({
        "version": APP_VERSION,
        "pr_rev": BUILT_PR_REV,
        "commit": BUILT_COMMIT,
        "commit_short": short_hash(BUILT_COMMIT),
    })
}

/// Compares the running build's baked-in commit against the latest commit on `main`, and
/// reflects the result in the tray tooltip and the "Check for Updates" menu item's label --
/// and in `update_available`, which the menu click handler reads to decide whether the next
/// click should check again or actually install the update (see perform_update). Thin
/// wrapper around compute_update_status that adds the tray-specific UI updates.
fn run_update_check<R: tauri::Runtime>(
    tray: &TrayIcon<R>,
    update_item: &MenuItem<R>,
    update_available: &AtomicBool,
) {
    let status = compute_update_status();

    if !status.checked {
        if let Some(e) = &status.error {
            eprintln!("[AETHER1] Update check failed: {e}");
        }
        // Leave update_available and the menu label as-is if we already know an update
        // was available -- a transient network error re-checking shouldn't erase that
        // state or make the label lie about what the next click will do.
        if !update_available.load(Ordering::Relaxed) {
            let _ = update_item.set_text("🔄 Check for Updates");
        }
        return;
    }

    update_available.store(!status.up_to_date, Ordering::Relaxed);

    if status.up_to_date {
        println!(
            "[AETHER1] Update check: up to date ({}, build {})",
            APP_VERSION,
            short_hash(BUILT_COMMIT)
        );
        let _ = tray.set_tooltip(Some(format!(
            "{APP_VERSION} -- running (build {}, up to date)",
            short_hash(BUILT_COMMIT)
        )));
        let _ = update_item.set_text("✅ Up to Date");
    } else {
        let latest = status.latest_commit.as_deref().unwrap_or("unknown");
        println!(
            "[AETHER1] Update check: new commit available (running {}, latest {})",
            short_hash(BUILT_COMMIT),
            short_hash(latest)
        );
        let _ = tray.set_tooltip(Some(format!(
            "{APP_VERSION} -- update available (running {}, latest {}) -- click \"Update Available\" in the tray menu to install",
            short_hash(BUILT_COMMIT),
            short_hash(latest)
        )));
        let _ = update_item.set_text("⬆ Update Available (click to install)");
    }
}

/// Which step of the update sequence failed, so callers (tray UI, the frontend) can show a
/// specific message instead of a generic "it broke somewhere".
enum UpdateStage {
    Pull,
    Build,
    Relaunch,
}

/// Actually applies an update: `git pull --ff-only` (see the auth note on github_token),
/// rebuild, then relaunch the freshly built binary and exit this process so the new build
/// takes over. Blocking (a rebuild can take over a minute); always call this off the main
/// thread. On success this process exits and never returns to the caller; on failure it
/// returns which stage failed.
///
/// Windows has no XDG-style stable install location -- start.bat just builds straight into
/// src-tauri/target/release/aether1.exe and runs it from there -- so on Windows this rebuilds
/// in place via `cargo build --release` and relaunches that same .exe (see the Windows branch
/// below for why the running exe has to be renamed out of the way first). Elsewhere it
/// delegates to scripts/install_desktop_app.sh, which also copies the binary to
/// $HOME/.local/bin/aether1 and refreshes the desktop launcher entry.
fn perform_update_core<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), (UpdateStage, String)> {
    let root = project_root();

    println!("[AETHER1] Update: running `git pull --ff-only`...");
    let pull_ok = Command::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(&root)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !pull_ok {
        return Err((UpdateStage::Pull, "git pull did not succeed".to_string()));
    }

    let new_binary = if cfg!(target_os = "windows") {
        let release_dir = root.join("src-tauri").join("target").join("release");
        let target_exe = release_dir.join("aether1.exe");

        // Unlike a plain overwrite-by-rename on Linux (see the "Text file busy" note in
        // install_desktop_app.sh), Windows won't let the linker write straight over the
        // .exe this very process is running from -- it stays open for execution and the
        // write fails with a sharing violation (LNK1104). Windows *does* allow renaming a
        // running executable's file out from under it (the already-loaded image keeps
        // running on its open handle), so move it aside first to free up the path for
        // `cargo build --release` to write a fresh one to.
        let old_exe = release_dir.join("aether1.exe.old");
        let _ = std::fs::remove_file(&old_exe); // leftover .old from a previous update, if any
        if target_exe.exists() {
            if let Err(e) = std::fs::rename(&target_exe, &old_exe) {
                return Err((
                    UpdateStage::Build,
                    format!("could not move the running binary aside before rebuilding: {e}"),
                ));
            }
        }

        println!("[AETHER1] Update: rebuilding via `cargo build --release`...");
        let build_ok = Command::new("cargo")
            .args(["build", "--release"])
            .current_dir(root.join("src-tauri"))
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !build_ok {
            return Err((UpdateStage::Build, "rebuild did not succeed".to_string()));
        }

        target_exe
    } else {
        println!("[AETHER1] Update: rebuilding via scripts/install_desktop_app.sh...");
        let build_ok = Command::new("bash")
            .arg(root.join("scripts").join("install_desktop_app.sh"))
            .current_dir(&root)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !build_ok {
            return Err((UpdateStage::Build, "rebuild did not succeed".to_string()));
        }

        // Relaunch the installed copy at ~/.local/bin/aether1 (see
        // scripts/install_desktop_app.sh), not the raw build artifact under
        // src-tauri/target/ -- the whole point of that install step is that the
        // launcher (and now the self-update relaunch too) runs from a stable
        // location, not from wherever this checkout happens to live.
        std::env::var("HOME")
            .map(|home| {
                PathBuf::from(home)
                    .join(".local")
                    .join("bin")
                    .join("aether1")
            })
            .unwrap_or_else(|_| {
                root.join("src-tauri")
                    .join("target")
                    .join("release")
                    .join("aether1")
            })
    };

    println!("[AETHER1] Update: relaunching {}...", new_binary.display());
    match Command::new(&new_binary).current_dir(&root).spawn() {
        Ok(_child) => {
            app.exit(0);
            Ok(())
        }
        Err(e) => Err((UpdateStage::Relaunch, format!("relaunch failed: {e}"))),
    }
}

/// Tray-specific wrapper around perform_update_core that keeps the tray tooltip and menu
/// label updated through each stage, so a failure is visible in the tray instead of silent.
fn perform_update<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    tray: &TrayIcon<R>,
    update_item: &MenuItem<R>,
) {
    let _ = tray.set_tooltip(Some("AETHER1 -- updating (git pull)...".to_string()));
    let _ = update_item.set_text("⏳ Updating...");

    if let Err((stage, msg)) = perform_update_core(app) {
        eprintln!("[AETHER1] Update failed: {msg}");
        let stage_label = match stage {
            UpdateStage::Pull => "git pull",
            UpdateStage::Build => "build",
            UpdateStage::Relaunch => "relaunch",
        };
        let _ = tray.set_tooltip(Some(format!(
            "AETHER1 -- update failed ({stage_label}) -- see terminal output for details"
        )));
        let _ = update_item.set_text("⚠ Update Failed (see logs)");
    }
    // No else branch: on success perform_update_core calls app.exit(0) and this process is
    // gone, so there's nothing left to update the tray with.
}

/// Rust-native equivalent for the frontend of the tray's install-update action. Blocking
/// (a rebuild can take over a minute) -- the frontend should show a "working" state while
/// this call is in flight. On success this process exits before ever returning a response,
/// so the frontend only ever observes this call either hang (app about to exit) or reject.
#[tauri::command]
fn apply_update_rust(app: tauri::AppHandle) -> Result<(), String> {
    perform_update_core(&app).map_err(|(_, msg)| msg)
}

/// Rust-native equivalent of POST /api/chat (backend/main.py), minus voice generation --
/// the frontend calls generate_speech_rust separately for that, matching the two-command
/// split the rest of this file already uses instead of one do-everything endpoint. Body
/// lives in commands::generate_response, shared with the axum server's /api/chat handler.
#[tauri::command]
fn generate_response_rust(
    engine: tauri::State<LlmEngine>,
    prompt: String,
    session_id: Option<String>,
) -> Result<serde_json::Value, String> {
    commands::generate_response(&engine, prompt, session_id)
}

/// Rust-native equivalent of POST /api/agent/genesis (backend/main.py), minus TTS audio
/// generation (see generate_speech_rust). Body lives in commands::agent_genesis, shared
/// with the axum server's /api/agent/genesis handler.
#[tauri::command]
fn agent_genesis_rust(
    engine: tauri::State<LlmEngine>,
    purpose: String,
) -> Result<serde_json::Value, String> {
    commands::agent_genesis(&engine, purpose)
}

/// Rust-native equivalent of GET /api/scanner/status (backend/main.py) -- cloud API key
/// detection plus Ollama/LM Studio probes. Blocking (matches this file's existing
/// synchronous command style); each probe has its own short timeout so this can't hang.
#[tauri::command]
fn scan_models_rust() -> model_scanner::ScanResult {
    model_scanner::scan_all()
}

/// Rust-native equivalent of POST /api/scanner/pull-model (backend/main.py).
#[tauri::command]
fn pull_model_rust(model_name: String) -> model_scanner::PullResult {
    commands::pull_model(model_name)
}

/// Rust-native equivalent of GET /api/static-info (backend/main.py) -- just the OS/arch
/// badge in the header, so a full Telemetry::snapshot() (which briefly sleeps to sample CPU
/// usage) would be needlessly slow for something this static; read it directly instead.
#[tauri::command]
fn get_static_info_rust() -> serde_json::Value {
    commands::static_info()
}

/// Rust-native equivalent of GET /api/messages (backend/main.py).
#[tauri::command]
fn get_messages_rust(engine: tauri::State<LlmEngine>, limit: Option<u32>) -> Vec<llm::Message> {
    commands::get_messages(&engine, limit)
}

/// Rust-native equivalent of DELETE /api/messages (backend/main.py).
#[tauri::command]
fn clear_messages_rust(engine: tauri::State<LlmEngine>) -> Result<(), String> {
    commands::clear_messages(&engine)
}

/// Rust-native equivalent of GET /api/settings (backend/main.py) -- same default-filling
/// behavior, so a fresh install (no settings rows yet) still gets sensible values.
#[tauri::command]
fn get_settings_rust(engine: tauri::State<LlmEngine>) -> serde_json::Value {
    commands::get_settings(&engine)
}

/// Rust-native equivalent of POST /api/settings (backend/main.py).
#[tauri::command]
fn save_settings_rust(
    engine: tauri::State<LlmEngine>,
    settings: serde_json::Value,
) -> Result<(), String> {
    commands::save_settings(&engine, settings)
}

/// Rust-native equivalent of POST /api/tts + GET /api/audio/{filename} (backend/main.py)
/// combined into one call: synthesizes speech and returns a local file path directly,
/// since the Tauri IPC path has no HTTP server to stream it from. The frontend turns this
/// into a playable URL via Tauri's convertFileSrc (see frontend/js/app.js). The axum
/// server's /api/chat and /api/agent/genesis handlers call commands::synthesize_speech
/// directly instead, since they need a URL string rather than a raw path.
#[tauri::command]
fn generate_speech_rust(text: String, voice: Option<String>) -> Result<String, String> {
    let path = commands::synthesize_speech(&text, voice.as_deref())?;
    Ok(path.to_string_lossy().to_string())
}

/// Builds (but does not show-if-already-open -- callers check first) the floating "desktop
/// sprite" window: a small, transparent, undecorated, always-on-top webview showing just the
/// hologram avatar (frontend/sprite.html reuses the same Three.js avatar code as the main
/// HUD), PNGTuber-style. Positioned in the bottom-right corner of the primary monitor when
/// that's available; falls back to Tauri's default placement otherwise.
fn build_sprite_window(app: &tauri::AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    const WIDTH: f64 = 300.0;
    const HEIGHT: f64 = 380.0;
    const MARGIN: f64 = 24.0;

    let mut builder = tauri::WebviewWindowBuilder::new(
        app,
        SPRITE_LABEL,
        tauri::WebviewUrl::App("sprite.html".into()),
    )
    .title("AETHER1")
    .inner_size(WIDTH, HEIGHT)
    .min_inner_size(160.0, 200.0)
    .transparent(true)
    .decorations(false)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .resizable(true);

    if let Ok(Some(monitor)) = app.primary_monitor() {
        let size = monitor.size().to_logical::<f64>(monitor.scale_factor());
        builder = builder.position(size.width - WIDTH - MARGIN, size.height - HEIGHT - MARGIN);
    }

    builder.build()
}

/// Rust-native command backing the Settings modal's "Desktop Sprite Mode" toggle: opens (or
/// re-shows an already-built) sprite window, or closes it. The frontend persists the
/// preference itself via save_settings_rust -- this command only ever manages window
/// lifecycle, it never touches storage.
#[tauri::command]
fn toggle_sprite_window_rust(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        match app.get_webview_window(SPRITE_LABEL) {
            Some(window) => window.show().map_err(|e| e.to_string()),
            None => build_sprite_window(&app).map(|_| ()).map_err(|e| e.to_string()),
        }
    } else if let Some(window) = app.get_webview_window(SPRITE_LABEL) {
        window.close().map_err(|e| e.to_string())
    } else {
        Ok(())
    }
}

/// Re-shows and focuses the main HUD window -- used by the sprite's "open main HUD" button,
/// since closing to tray (see the CloseRequested handler in main()) hides rather than
/// destroys it.
#[tauri::command]
fn show_main_window_rust(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(MAIN_LABEL) {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Starts an OS-native window drag for whichever window invoked this command. The sprite
/// window has no decorations (so no native title bar to drag by); the frontend calls this on
/// mousedown over the avatar instead (see frontend/js/sprite.js), matching Tauri's usual
/// pattern for custom-titlebar dragging. `window` is auto-populated by Tauri with the
/// invoking window, not necessarily the sprite -- harmless either way.
#[tauri::command]
fn start_window_drag_rust(window: tauri::WebviewWindow) -> Result<(), String> {
    window.start_dragging().map_err(|e| e.to_string())
}

/// Shared by both the native Tauri path and `--serve`: opens the real sqlite file at
/// backend/aether1_memory.db, falling back to a temp-dir sqlite file if that fails.
fn build_llm_engine() -> LlmEngine {
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
                "[AETHER1] Could not open {} ({e}); the Rust LLM engine will use {} for this session instead.",
                db_path.display(),
                fallback_path.display()
            );
            LlmEngine::new(
                MemoryDb::open(&fallback_path).expect("fallback sqlite path should always open"),
            )
        }
    }
}

fn main() {
    // Both `ring` and `aws-lc-rs` end up in the dependency tree now (ureq pulls in one,
    // msedge-tts's cert verifier the other), and rustls refuses to guess between two linked
    // providers -- pin one explicitly before anything on any thread makes its first TLS
    // connection (the update-checker and TTS both do). Must run before the telemetry/update
    // threads spawned below (or --serve's own telemetry thread) could possibly race it.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("installing the rustls crypto provider should only fail if called twice");

    // Headless HTTP mode: short-circuit before tauri::Builder is ever constructed, so the
    // native app's setup (tray, asset-protocol scope, window) never runs in this process.
    // See server.rs for the actual axum app.
    if std::env::args().any(|arg| arg == "--serve") {
        let engine = build_llm_engine();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime for --serve");
        runtime.block_on(server::run(engine));
        return;
    }

    let llm_engine = build_llm_engine();

    tauri::Builder::default()
        .manage(llm_engine)
        .invoke_handler(tauri::generate_handler![
            generate_response_rust,
            agent_genesis_rust,
            scan_models_rust,
            pull_model_rust,
            get_static_info_rust,
            get_messages_rust,
            clear_messages_rust,
            get_settings_rust,
            save_settings_rust,
            generate_speech_rust,
            get_version_info,
            check_for_update_rust,
            apply_update_rust,
            toggle_sprite_window_rust,
            show_main_window_rust,
            start_window_drag_rust
        ])
        .on_window_event(|window, event| {
            // Closing the main HUD window would otherwise exit the whole app (Tauri's
            // default with no other running windows/tray keeping it alive) -- but if the
            // desktop sprite is up, the app should keep running headless-with-sprite instead,
            // matching the tray's existing "Show AETHER1" affordance. Only intercepts the
            // close when the sprite is actually open, so anyone not using that feature sees
            // the same close-quits-the-app behavior as before.
            if window.label() == MAIN_LABEL {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if window.app_handle().get_webview_window(SPRITE_LABEL).is_some() {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        })
        .setup(|app| {
            // Let the webview load synthesized speech files directly off disk via
            // convertFileSrc (see synthesizeSpeechUrl in frontend/js/app.js) -- the asset
            // protocol is opt-in per-directory, and this path is only known at runtime (it's
            // relative to wherever this checkout happens to live), so it's granted here
            // rather than as a fixed glob in tauri.conf.json.
            let audio_cache_dir = project_root().join("backend").join("audio_cache");
            std::fs::create_dir_all(&audio_cache_dir)?;
            app.asset_protocol_scope()
                .allow_directory(&audio_cache_dir, false)?;

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

            // Whether the last check found a newer commit on `main` -- read by the
            // "check_update" click handler to decide whether the next click should check
            // again or actually install the update (see perform_update).
            let update_available = Arc::new(AtomicBool::new(false));
            // Debounce: true while a check or an update is already running, so a double
            // click (or a click landing during the launch-time check below) can't start a
            // second one -- two concurrent `perform_update` calls would run two concurrent
            // `git pull`/`cargo build` and could both relaunch the app.
            let update_in_progress = Arc::new(AtomicBool::new(false));

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
                    let update_available = update_available.clone();
                    let update_in_progress = update_in_progress.clone();
                    move |app, event| match event.id.as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "check_update" => {
                            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                                if update_in_progress
                                    .compare_exchange(
                                        false,
                                        true,
                                        Ordering::Relaxed,
                                        Ordering::Relaxed,
                                    )
                                    .is_ok()
                                {
                                    let update_item = update_item.clone();
                                    let update_available = update_available.clone();
                                    let update_in_progress = update_in_progress.clone();
                                    let app = app.clone();
                                    std::thread::spawn(move || {
                                        if update_available.load(Ordering::Relaxed) {
                                            perform_update(&app, &tray, &update_item);
                                        } else {
                                            run_update_check(
                                                &tray,
                                                &update_item,
                                                &update_available,
                                            );
                                        }
                                        // Unreached if perform_update succeeded (it calls
                                        // app.exit(0)), which is fine -- the process is
                                        // gone, so there's no stuck flag to worry about.
                                        update_in_progress.store(false, Ordering::Relaxed);
                                    });
                                }
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
                update_in_progress.store(true, Ordering::Relaxed);
                std::thread::spawn(move || {
                    run_update_check(&tray, &update_item, &update_available);
                    update_in_progress.store(false, Ordering::Relaxed);
                });
            }

            // Re-open the desktop sprite on launch if it was left enabled last session
            // (persisted the same way as every other setting -- see
            // toggle_sprite_window_rust/save_settings_rust). Defaults to off: this is an
            // opt-in feature, not something a fresh install should surprise anyone with.
            let sprite_was_enabled = app
                .state::<LlmEngine>()
                .db()
                .get_setting("desktop_sprite_enabled")
                .ok()
                .flatten()
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if sprite_was_enabled {
                if let Err(e) = build_sprite_window(app.handle()) {
                    eprintln!("[AETHER1] Could not reopen the desktop sprite window: {e}");
                }
            }

            // Live telemetry push, replacing the Python backend's /ws/telemetry loop: an
            // event instead of a websocket message, but the same ~1s cadence and the same
            // payload shape (see Telemetry::to_wire_json / UsageSnapshot), so the frontend's
            // existing updateHardwareTelemetry/updateTokenTelemetry handle both paths as-is.
            {
                let app_handle = app.handle().clone();
                std::thread::spawn(move || loop {
                    let telemetry = llm::Telemetry::snapshot();
                    let engine = app_handle.state::<LlmEngine>();
                    let payload = serde_json::json!({
                        "telemetry": telemetry.to_wire_json(),
                        "tokens": engine.usage_snapshot(),
                        "agent_name": engine.agent_name(),
                    });
                    let _ = app_handle.emit("telemetry-update", payload);
                    std::thread::sleep(Duration::from_millis(1000));
                });
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, _event| {});
}
