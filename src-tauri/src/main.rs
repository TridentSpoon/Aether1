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

mod background_services;
mod cli;
mod commands;
mod discovery;
mod downloads;
mod hotkey;
mod installs;
mod llm;
mod local_only;
mod model_scanner;
mod paths;
mod serve_auth;
mod serve_tls;
mod server;
mod setup;
mod terminal;
mod tools;
mod vault;
mod voice_download;
mod voice_setup;
mod watchers;

// installs::Machine is the trait the scan reads the machine through; brought in as `_` because
// only its methods are called here, never its name.
use installs::Machine as _;

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::Deserialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{Emitter, Manager};

use llm::{LlmEngine, MemoryDb};

const UPDATE_REPO: &str = "TridentSpoon/Aether1";
const TRAY_ID: &str = "main-tray";
const MAIN_LABEL: &str = "main";
const SPRITE_LABEL: &str = "sprite";
const AVATAR_LAB_LABEL: &str = "avatar-lab";
const FACE_LABEL: &str = "face";

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
/// directory (aether1_memory.db, audio_cache/) all live one level up, at the repo root --
/// for a dev build compiled on this same machine, that is exactly right.
///
/// It is baked in at *compile* time, though, so a binary built by CI (the offline
/// installer's release.yml) carries the runner's own checkout path (e.g.
/// `D:\a\Aether1\Aether1` on the Windows runner) -- a path that cannot exist on whichever
/// machine later installs it. Every caller of this function reaches it via `.join("backend")
/// .join(...)`, and `create_dir_all` fails silently there (caught, and for TTS/STT
/// swallowed all the way up to a console.warn the operator never sees) rather than
/// panicking, so the symptom is "speech does nothing" with no error, not a crash.
///
/// Detecting the two cases at runtime -- does frontend/ actually exist next to the baked-in
/// path on *this* machine? -- and falling back to the same ~/.local/share convention
/// already used for Piper's and whisper.cpp's own data (see llm/tts.rs, llm/stt.rs) keeps
/// every existing caller unchanged; only where "backend" ends up living differs.
fn project_root() -> PathBuf {
    let baked_in = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri should have a parent directory")
        .to_path_buf();
    if baked_in.join("frontend").is_dir() {
        return baked_in;
    }
    paths::home_dir()
        .map(|home| home.join(".local").join("share").join("aether1"))
        .unwrap_or(baked_in)
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

/// Distinguishes "no `gh` credentials" from every other way the update check can fail, so
/// the frontend can offer a fix for this one specifically (an Install via winget button on
/// Windows) instead of only ever displaying prose. Kept as its own type rather than matching
/// on the rendered message string, which would silently break the moment either wording
/// changed.
enum UpdateCheckError {
    NoGithubAuth,
    Other(String),
}

impl UpdateCheckError {
    fn message(&self) -> String {
        match self {
            UpdateCheckError::NoGithubAuth => {
                "this repo is private and no GitHub credentials were found on this machine -- \
                 install the `gh` CLI and run `gh auth login`, then try again"
                    .to_string()
            }
            UpdateCheckError::Other(e) => e.clone(),
        }
    }
}

/// Blocking GET against GitHub's REST API for the latest commit on `main`. Always call
/// this off the main thread -- it can take up to the timeout below if the network is slow
/// or absent, and must never hold up the tray or the window.
fn fetch_latest_main_sha() -> Result<String, UpdateCheckError> {
    let url = format!("https://api.github.com/repos/{UPDATE_REPO}/commits/main");
    let mut request = ureq::get(&url)
        .config()
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .header("User-Agent", "AETHER1-desktop-app")
        .header("Accept", "application/vnd.github+json");

    let token = github_token();
    let have_token = token.is_some();
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }

    let response = request.call().map_err(|e| {
        // A private repo 404s on an unauthenticated request -- indistinguishable from a
        // genuinely missing repo, but far more likely given how this project is set up
        // (see github_token's docs), and "http status: 404" on its own reads as a bug
        // report waiting to happen rather than the expected result of not being logged
        // into `gh` on this machine.
        if !have_token && matches!(e, ureq::Error::StatusCode(404)) {
            UpdateCheckError::NoGithubAuth
        } else {
            UpdateCheckError::Other(e.to_string())
        }
    })?;

    response
        .into_body()
        .read_json::<GhCommit>()
        .map(|c| c.sha)
        .map_err(|e| UpdateCheckError::Other(e.to_string()))
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
    /// True only for the specific "no `gh` credentials" failure -- the frontend uses this,
    /// not the error text, to decide whether to offer the Windows winget install button.
    needs_gh_auth: bool,
}

/// Whether this install is in local-only mode. Read through the managed engine so the
/// answer is the operator's current setting rather than whatever it was at launch;
/// `try_state` because this is also called from paths that run before/outside the managed
/// state, where "not in local-only mode" is the honest answer unless the environment says
/// otherwise.
fn local_only_enabled<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    local_only::env_forced()
        || app
            .try_state::<LlmEngine>()
            .map(|engine| local_only::enabled(engine.db()))
            .unwrap_or(false)
}

/// Does the actual comparison: this build's baked-in commit (BUILT_COMMIT / build.rs)
/// against the latest commit on `main` via the GitHub API. Safe to call from any thread;
/// never panics or blocks the caller beyond the network timeout in fetch_latest_main_sha.
fn compute_update_status(local_only: bool) -> UpdateStatus {
    // The update check was the last thing in Aether1 that reached the internet on its own,
    // every single launch, whatever the operator had configured. It is a GitHub API call,
    // so with local-only mode on it does not happen -- and says so, rather than reporting
    // "up to date" from a comparison it never made.
    if local_only {
        return UpdateStatus {
            checked: false,
            up_to_date: false,
            version: APP_VERSION.to_string(),
            built_commit: BUILT_COMMIT.to_string(),
            built_commit_short: short_hash(BUILT_COMMIT).to_string(),
            latest_commit: None,
            error: Some(local_only::refusal("GitHub was not contacted")),
            needs_gh_auth: false,
        };
    }

    let base = UpdateStatus {
        checked: false,
        up_to_date: false,
        version: APP_VERSION.to_string(),
        built_commit: BUILT_COMMIT.to_string(),
        built_commit_short: short_hash(BUILT_COMMIT).to_string(),
        latest_commit: None,
        error: None,
        needs_gh_auth: false,
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
            needs_gh_auth: matches!(e, UpdateCheckError::NoGithubAuth),
            error: Some(e.message()),
            ..base
        },
    }
}

/// Rust-native equivalent for the frontend of what the tray's "Check for Updates" click
/// does -- lets the HUD show the same real version check instead of only being visible via
/// the taskbar icon. Does not itself update `update_available`/the tray UI; those stay
/// tray-only state (see run_update_check).
#[tauri::command(async)]
fn check_for_update_rust(app: tauri::AppHandle) -> UpdateStatus {
    compute_update_status(local_only_enabled(&app))
}

/// Runs `winget install --id GitHub.cli` for the operator who hit `needs_gh_auth` above and
/// clicked the resulting "Install via winget" button, so getting past that error doesn't
/// require leaving the app to find a terminal. winget itself still needs the operator to
/// have accepted its Store agreement at least once (Windows' own one-time step, not
/// something this can do for them) -- a failure here says so via winget's own stderr rather
/// than trying to paper over it.
#[tauri::command(async)]
fn install_gh_via_winget_rust() -> Result<String, String> {
    if !cfg!(target_os = "windows") {
        return Err("winget is only available on Windows".to_string());
    }
    let mut cmd = Command::new("winget");
    cmd.args([
        "install",
        "--id",
        "GitHub.cli",
        "-e",
        "--source",
        "winget",
        "--accept-package-agreements",
        "--accept-source-agreements",
    ]);
    paths::suppress_console_window(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| format!("could not run winget: {e}"))?;
    if output.status.success() {
        Ok(
            "gh installed via winget. Run `gh auth login` in a terminal, then Check for \
            Updates again."
                .to_string(),
        )
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(format!(
            "winget install failed: {}",
            if stderr.is_empty() { stdout } else { stderr }
        ))
    }
}

/// Rust-native equivalent for the frontend of get_version_info -- current version string
/// plus the exact commit this build came from, for display in the HUD.
#[tauri::command(async)]
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
    local_only: bool,
) {
    // Said plainly in the tray rather than left as a check that quietly never runs: a
    // switched-off update check should look switched off, not broken.
    if local_only {
        println!("[AETHER1] Local-only mode: update check skipped (GitHub not contacted).");
        let _ = update_item.set_text("\u{1f512} Updates off (local-only)");
        update_available.store(false, Ordering::Relaxed);
        return;
    }

    let status = compute_update_status(local_only);

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
    // `git pull` is a network fetch like any other, and it rewrites the running install.
    // Local-only mode stops it here rather than only hiding the button that starts it.
    if local_only_enabled(app) {
        return Err((
            UpdateStage::Pull,
            local_only::refusal("no update was pulled"),
        ));
    }

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
#[tauri::command(async)]
fn apply_update_rust(app: tauri::AppHandle) -> Result<(), String> {
    perform_update_core(&app).map_err(|(_, msg)| msg)
}

/// Rust-native equivalent of POST /api/chat (backend/main.py), minus voice generation --
/// the frontend calls generate_speech_rust separately for that, matching the two-command
/// split the rest of this file already uses instead of one do-everything endpoint. Body
/// lives in commands::generate_response, shared with the axum server's /api/chat handler.
#[tauri::command(async)]
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
/// Streaming counterpart of generate_response_rust: the reply is emitted delta by delta as
/// `chat-delta` events (each tagged with the caller's stream_id so two in-flight turns can't
/// interleave in the UI), and the whole reply still comes back as the return value. The
/// frontend renders the deltas and uses the return value as the authoritative final text.
#[tauri::command(async)]
fn generate_response_streaming_rust(
    app: tauri::AppHandle,
    engine: tauri::State<LlmEngine>,
    prompt: String,
    session_id: Option<String>,
    stream_id: String,
) -> Result<serde_json::Value, String> {
    commands::generate_response_streamed(&engine, prompt, session_id, &mut |delta| {
        let _ = app.emit(
            "chat-delta",
            serde_json::json!({ "stream_id": stream_id, "delta": delta }),
        );
    })
}

#[tauri::command(async)]
fn agent_genesis_rust(
    engine: tauri::State<LlmEngine>,
    purpose: String,
) -> Result<serde_json::Value, String> {
    commands::agent_genesis(&engine, purpose)
}

#[tauri::command(async)]
fn test_llm_connection_rust(
    engine: tauri::State<LlmEngine>,
    provider: String,
    model: String,
    endpoint: String,
    api_key: String,
) -> Result<String, String> {
    commands::test_llm_connection(&engine, provider, model, endpoint, api_key)
}

/// Rust-native equivalent of GET /api/scanner/status (backend/main.py) -- cloud API key
/// detection plus Ollama/LM Studio probes. Blocking (matches this file's existing
/// synchronous command style); each probe has its own short timeout so this can't hang.
#[tauri::command(async)]
fn scan_models_rust() -> model_scanner::ScanResult {
    model_scanner::scan_all()
}

/// Where this machine is on the road to having a model, and what to do about it. Drives
/// the setup wizard; safe to call as often as the wizard likes, since it is only probes.
#[tauri::command(async)]
fn setup_advice_rust(engine: tauri::State<LlmEngine>) -> serde_json::Value {
    commands::setup_advice(&engine)
}

#[tauri::command(async)]
fn start_download_rust(
    engine: tauri::State<LlmEngine>,
    model_name: String,
    endpoint: Option<String>,
) -> serde_json::Value {
    commands::start_download(&engine, model_name, endpoint.unwrap_or_default())
}

#[tauri::command(async)]
fn download_status_rust() -> serde_json::Value {
    commands::download_status()
}

#[tauri::command(async)]
fn forget_download_rust(model_name: String) -> serde_json::Value {
    commands::forget_download(model_name)
}

#[tauri::command(async)]
fn voice_catalogue_rust() -> serde_json::Value {
    commands::voice_catalogue()
}

#[tauri::command(async)]
fn start_voice_download_rust(engine: tauri::State<LlmEngine>, voice: String) -> serde_json::Value {
    commands::start_voice_download(&engine, voice)
}

#[tauri::command(async)]
fn voice_download_status_rust() -> serde_json::Value {
    commands::voice_download_status()
}

#[tauri::command(async)]
fn forget_voice_download_rust(voice: String) -> serde_json::Value {
    commands::forget_voice_download(voice)
}

#[tauri::command(async)]
fn start_local_server_rust(engine: tauri::State<LlmEngine>) -> serde_json::Value {
    commands::start_local_server(&engine)
}

/// Rust-native equivalent of POST /api/scanner/pull-model (backend/main.py).
#[tauri::command(async)]
fn pull_model_rust(
    engine: tauri::State<LlmEngine>,
    model_name: String,
) -> model_scanner::PullResult {
    commands::pull_model(&engine, model_name)
}

/// Rust-native equivalent of GET /api/static-info (backend/main.py) -- just the OS/arch
/// badge in the header, so a full Telemetry::snapshot() (which briefly sleeps to sample CPU
/// usage) would be needlessly slow for something this static; read it directly instead.
#[tauri::command(async)]
fn get_static_info_rust() -> serde_json::Value {
    commands::static_info()
}

/// Rust-native equivalent of GET /api/messages (backend/main.py).
#[tauri::command(async)]
fn get_tools_rust(engine: tauri::State<LlmEngine>) -> serde_json::Value {
    commands::tool_catalog(&engine)
}

#[tauri::command(async)]
fn get_actions_rust(engine: tauri::State<LlmEngine>, limit: Option<u32>) -> Vec<llm::ActionRecord> {
    commands::recent_actions(&engine, limit)
}

#[tauri::command(async)]
fn pending_actions_rust(engine: tauri::State<LlmEngine>) -> Vec<llm::ActionRecord> {
    commands::pending_actions(&engine)
}

#[tauri::command(async)]
fn approve_action_rust(
    engine: tauri::State<LlmEngine>,
    id: i64,
) -> Result<serde_json::Value, String> {
    commands::approve_action(&engine, id)
}

#[tauri::command(async)]
fn undo_action_rust(engine: tauri::State<LlmEngine>, id: i64) -> Result<serde_json::Value, String> {
    commands::undo_action(&engine, id)
}

#[tauri::command(async)]
fn reject_action_rust(engine: tauri::State<LlmEngine>, id: i64) -> Result<(), String> {
    commands::reject_action(&engine, id)
}

#[tauri::command(async)]
fn persona_access_rust(engine: tauri::State<LlmEngine>) -> serde_json::Value {
    commands::persona_access(&engine)
}

#[tauri::command(async)]
fn set_persona_access_rust(
    engine: tauri::State<LlmEngine>,
    paths: Vec<String>,
) -> Result<serde_json::Value, String> {
    commands::set_persona_access(&engine, paths)
}

#[tauri::command(async)]
fn set_always_allowed_rust(
    engine: tauri::State<LlmEngine>,
    tool: String,
    allowed: bool,
) -> Result<(), String> {
    commands::set_always_allowed(&engine, tool, allowed)
}

/* -------------------------------------------------------------------------- */
/* The operator's terminal.                                                    */
/*                                                                             */
/* These four commands are the *only* way into terminal.rs, and they are        */
/* declared here rather than in commands.rs on purpose. commands.rs is the      */
/* shared layer both transports call: anything put there is reachable from      */
/* server.rs, which means reachable over HTTP and, with --lan, over the         */
/* network. A #[tauri::command] registered in this file has no such second      */
/* door -- `--serve` returns from main() before tauri::Builder is ever          */
/* constructed, so in a headless run this code is not merely unrouted, it is    */
/* never reached at all.                                                       */
/*                                                                             */
/* There is deliberately no tool wrapping any of this. The companion has no     */
/* name it can emit that arrives here.                                         */
/* -------------------------------------------------------------------------- */

/// Opens a terminal and starts streaming it to the window that asked.
///
/// Output leaves here as a `terminal-output` event and goes nowhere else: not to the
/// database, not to the vault, not to the action log. That is what stops the transcript of
/// what the operator did in their own terminal from becoming a file the companion could
/// later be pointed at.
///
/// The bytes are base64 on the way across because a pty emits escape sequences and
/// half-characters, and Tauri's event payloads are JSON -- which would mangle both. The
/// frontend hands them to the terminal emulator still encoded.
#[tauri::command(async)]
fn terminal_open_rust(
    app: tauri::AppHandle,
    terminals: tauri::State<terminal::Handle>,
    rows: u16,
    cols: u16,
) -> Result<String, String> {
    let output_app = app.clone();
    let exit_app = app.clone();
    terminal::open(
        &terminals,
        rows,
        cols,
        move |id, bytes| {
            let _ = output_app.emit(
                "terminal-output",
                serde_json::json!({
                    "id": id,
                    "bytes": BASE64.encode(bytes),
                }),
            );
        },
        move |id| {
            let _ = exit_app.emit("terminal-exit", serde_json::json!({ "id": id }));
        },
    )
}

/// Keystrokes from the window, base64 for the same reason as the output: a terminal's input
/// includes control bytes and escape sequences, not just text.
#[tauri::command(async)]
fn terminal_write_rust(
    terminals: tauri::State<terminal::Handle>,
    id: String,
    bytes: String,
) -> Result<(), String> {
    let decoded = BASE64
        .decode(bytes.as_bytes())
        .map_err(|_| "that keystroke did not arrive intact".to_string())?;
    terminal::write(&terminals, &id, &decoded)
}

#[tauri::command(async)]
fn terminal_resize_rust(
    terminals: tauri::State<terminal::Handle>,
    id: String,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    terminal::resize(&terminals, &id, rows, cols)
}

#[tauri::command(async)]
fn terminal_close_rust(
    terminals: tauri::State<terminal::Handle>,
    id: String,
) -> Result<(), String> {
    terminal::close(&terminals, &id)
}

#[tauri::command(async)]
fn get_messages_rust(
    engine: tauri::State<LlmEngine>,
    limit: Option<u32>,
    session_id: Option<String>,
) -> Result<Vec<llm::Message>, String> {
    commands::get_messages(&engine, limit, session_id)
}

/// Rust-native equivalent of DELETE /api/messages (backend/main.py).
#[tauri::command(async)]
fn clear_messages_rust(
    engine: tauri::State<LlmEngine>,
    session_id: Option<String>,
) -> Result<(), String> {
    commands::clear_messages(&engine, session_id)
}

/// The history list. Counterpart of GET /api/sessions.
#[tauri::command(async)]
fn list_sessions_rust(engine: tauri::State<LlmEngine>) -> Result<Vec<llm::SessionSummary>, String> {
    commands::list_sessions(&engine)
}

/// Counterpart of POST /api/sessions/new. Mints an id and writes nothing.
#[tauri::command(async)]
fn new_session_rust() -> String {
    commands::new_session()
}

/// Counterpart of POST /api/sessions/rename.
#[tauri::command(async)]
fn rename_session_rust(
    engine: tauri::State<LlmEngine>,
    session_id: Option<String>,
    title: String,
) -> Result<(), String> {
    commands::rename_session(&engine, session_id, title)
}

/// Counterpart of POST /api/sessions/delete.
#[tauri::command(async)]
fn delete_session_rust(engine: tauri::State<LlmEngine>, session_id: String) -> Result<(), String> {
    commands::delete_session(&engine, session_id)
}

/// Rust-native equivalent of POST /api/benchmarks/reset.
#[tauri::command(async)]
fn reset_benchmarks_rust(engine: tauri::State<LlmEngine>) -> Result<(), String> {
    commands::reset_benchmarks(&engine)
}

/// Rust-native equivalent of POST /api/vault/open.
#[tauri::command(async)]
fn open_vault_folder_rust(engine: tauri::State<LlmEngine>) -> Result<(), String> {
    commands::open_vault_folder(&engine)
}

/// Rust-native equivalent of GET /api/vault/notes.
#[tauri::command(async)]
fn vault_notes_rust(engine: tauri::State<LlmEngine>) -> Vec<vault::reader::NoteSummary> {
    commands::vault_notes(&engine)
}

/// Rust-native equivalent of GET /api/vault/note?name=...
#[tauri::command(async)]
fn vault_note_rust(
    engine: tauri::State<LlmEngine>,
    name: String,
) -> Result<vault::reader::NoteView, String> {
    commands::vault_note(&engine, &name)
}

/// Rust-native equivalent of GET /api/vault/graph.
#[tauri::command(async)]
fn vault_graph_rust(engine: tauri::State<LlmEngine>) -> vault::reader::Graph {
    commands::vault_graph(&engine)
}

/// Rust-native equivalent of GET /api/vault/search?q=...
#[tauri::command(async)]
fn vault_search_rust(engine: tauri::State<LlmEngine>, query: String) -> serde_json::Value {
    commands::vault_search(&engine, &query)
}

/// Rust-native equivalent of GET /api/settings (backend/main.py) -- same default-filling
/// behavior, so a fresh install (no settings rows yet) still gets sensible values.
#[tauri::command(async)]
fn get_settings_rust(engine: tauri::State<LlmEngine>) -> serde_json::Value {
    commands::get_settings(&engine)
}

/// Enables or disables AETHER1's own login autostart (XDG autostart entry on Linux, a Run
/// registry key on Windows -- the plugin covers both uniformly), matching whatever the
/// operator just set `autostart_app` to. Best-effort: no permission is needed for either
/// mechanism, but a failure here (e.g. a read-only autostart directory) is reported rather
/// than silently ignored, same as the hotkey re-registration right below this.
fn sync_autostart(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager.enable()
    } else {
        manager.disable()
    }
    .map_err(|e| e.to_string())
}

/// Rust-native equivalent of POST /api/settings (backend/main.py).
#[tauri::command(async)]
fn save_settings_rust(
    app: tauri::AppHandle,
    engine: tauri::State<LlmEngine>,
    settings: serde_json::Value,
) -> Result<(), String> {
    let hotkey_changed = settings.get("hotkey_toggle").is_some();
    let autostart_app = settings
        .get("autostart_app")
        .and_then(serde_json::Value::as_bool);
    commands::save_settings(&engine, settings)?;
    // Rebind the global hotkey in place so a chord edited in Settings takes effect without
    // a restart. A chord that won't parse is reported to the operator but doesn't fail the
    // save -- the rest of the settings were still written, and refusing the whole save
    // would lose them. (The axum path has no window to summon, so it has no equivalent.)
    if hotkey_changed {
        if let Err(e) = hotkey::reregister_from_settings(&app) {
            return Err(format!("settings saved, but the hotkey was not: {e}"));
        }
    }
    if let Some(enabled) = autostart_app {
        if let Err(e) = sync_autostart(&app, enabled) {
            return Err(format!("settings saved, but launch-at-login was not: {e}"));
        }
    }
    Ok(())
}

/// Toggles Game Mode: stops whatever background server AETHER1 itself started (never one the
/// operator runs independently -- see background_services) and drops the HUD down to a
/// minimal footprint. Never quits AETHER1: the tray icon and hotkey are the only way to
/// switch Game Mode back off, so this only hides the window and tells the frontend to back
/// off its telemetry polling, the two things actually costing CPU/GPU while the HUD sits
/// idle behind a game. Reverses symmetrically: Ollama is restarted only if Game Mode is what
/// stopped it, so turning Game Mode off always returns the machine to the state it was in
/// right before Game Mode was switched on, not to whatever `autostart_ollama` says in
/// general.
#[tauri::command(async)]
fn set_game_mode_rust(
    app: tauri::AppHandle,
    engine: tauri::State<LlmEngine>,
    managed_ollama: tauri::State<background_services::ManagedOllama>,
    enabled: bool,
) -> Result<(), String> {
    engine
        .db()
        .set_setting("game_mode", &serde_json::json!(enabled))
        .map_err(|e| e.to_string())?;

    if enabled {
        let was_managed = managed_ollama.is_managed();
        engine
            .db()
            .set_setting("_game_mode_stopped_ollama", &serde_json::json!(was_managed))
            .map_err(|e| e.to_string())?;
        background_services::stop_managed_ollama(&managed_ollama);
        if let Some(window) = app.get_webview_window(MAIN_LABEL) {
            let _ = window.hide();
        }
    } else {
        if engine
            .db()
            .get_setting_bool("_game_mode_stopped_ollama", false)
        {
            background_services::start_ollama_if_needed(&engine, &managed_ollama);
        }
        if let Some(window) = app.get_webview_window(MAIN_LABEL) {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
    let _ = app.emit("game-mode-changed", enabled);
    Ok(())
}

/// Rust-native equivalent of POST /api/tts + GET /api/audio/{filename} (backend/main.py)
/// combined into one call: synthesizes speech and returns a local file path directly,
/// since the Tauri IPC path has no HTTP server to stream it from. The frontend turns this
/// into a playable URL via Tauri's convertFileSrc (see frontend/js/app.js). The axum
/// server's /api/chat and /api/agent/genesis handlers call commands::synthesize_speech
/// directly instead, since they need a URL string rather than a raw path.
#[tauri::command(async)]
fn transcribe_rust(engine: tauri::State<LlmEngine>, wav: Vec<u8>) -> Result<String, String> {
    commands::transcribe_audio(&engine, &wav)
}

#[tauri::command(async)]
fn voice_status_rust(engine: tauri::State<LlmEngine>) -> serde_json::Value {
    commands::voice_status(&engine)
}

/// Rust-native equivalent of GET /api/voice/advice -- what can speak, what can listen, and
/// what to do about either. See voice_setup for why this is a probe rather than a wizard
/// page number.
#[tauri::command(async)]
fn voice_advice_rust(engine: tauri::State<LlmEngine>) -> voice_setup::VoiceAdvice {
    commands::voice_advice(&engine)
}

/// Rust-native equivalent of POST /api/voice/test. Speaks a fixed sentence and returns the
/// report with the audio path folded in, so the HUD can both play it and say which engine
/// managed it.
#[tauri::command(async)]
fn test_speech_rust(engine: tauri::State<LlmEngine>) -> serde_json::Value {
    match commands::test_speech(&engine) {
        Ok((path, mut report)) => {
            report["path"] = serde_json::json!(path.to_string_lossy());
            report
        }
        Err(report) => report,
    }
}

#[tauri::command(async)]
fn list_personas_rust() -> serde_json::Value {
    commands::list_personas()
}

#[tauri::command(async)]
fn generate_speech_rust(
    engine: tauri::State<LlmEngine>,
    text: String,
    voice: Option<String>,
) -> Result<String, String> {
    let path = commands::synthesize_speech(&engine, &text, voice.as_deref())?;
    Ok(path.to_string_lossy().to_string())
}

/// Builds (but does not show-if-already-open -- callers check first) the floating "desktop
/// sprite" window: a small, transparent, undecorated, always-on-top webview showing just the
/// hologram avatar (frontend/sprite.html reuses the same Three.js avatar code as the main
/// HUD). It's a live mirror of the main HUD's own hologram -- the frontend pushes avatar,
/// theme, state and audio changes to it over Tauri events (see setAvatarState and
/// pushAudioToSprite in js/app.js) rather than this window running any conversation of its
/// own -- so the main HUD's hologram panel goes quiet while this is open instead of drawing
/// a second, separate avatar. Positioned in the bottom-right corner of the primary monitor
/// when that's available; falls back to Tauri's default placement otherwise.
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
            None => build_sprite_window(&app)
                .map(|_| ())
                .map_err(|e| e.to_string()),
        }
    } else if let Some(window) = app.get_webview_window(SPRITE_LABEL) {
        window.close().map_err(|e| e.to_string())
    } else {
        Ok(())
    }
}

/// Builds the fullscreen face window: the avatar filling a whole screen with nothing around
/// it but the word for what it is doing (frontend/face.html). Meant for a spare screen or a
/// second monitor, so it goes to a monitor that is *not* the one the HUD is on when there is
/// one -- putting it over the window it mirrors would be a strange default -- and falls back
/// to the primary monitor otherwise.
///
/// Like the desktop sprite this is a mirror, not a second companion: it has no chat, no
/// microphone and no settings of its own, and everything it draws arrives from the main HUD
/// over the same Tauri events the sprite already listens to (see frontend/js/face.js). Unlike
/// the sprite it takes no input at all beyond Esc to close, which is why it does not need --
/// and deliberately does not get -- the sprite's click-to-listen bridge.
///
/// Undecorated as well as fullscreen: a title bar on a screen showing one face is the one
/// piece of furniture there is no excuse for. That makes Esc the only way out, so face.js
/// says so on screen rather than leaving the operator to guess.
fn build_face_window(app: &tauri::AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let mut builder = tauri::WebviewWindowBuilder::new(
        app,
        FACE_LABEL,
        tauri::WebviewUrl::App("face.html".into()),
    )
    .title("AETHER1")
    .fullscreen(true)
    .decorations(false)
    .resizable(false)
    .skip_taskbar(true);

    if let Some(monitor) = spare_monitor(app) {
        // Fullscreen is applied to whichever monitor the window is on, so the position has
        // to be set before it is shown -- one logical pixel inside the spare monitor's own
        // origin is enough to land it there.
        let origin = monitor.position().to_logical::<f64>(monitor.scale_factor());
        builder = builder.position(origin.x + 1.0, origin.y + 1.0);
    }

    builder.build()
}

/// A monitor other than the one the HUD is currently on, if this machine has one. `None`
/// on a single-monitor machine (and on any platform where the monitor list is unavailable),
/// which leaves the face on the only screen there is -- the right answer there, even though
/// it covers the HUD, because the alternative is refusing to open at all.
fn spare_monitor(app: &tauri::AppHandle) -> Option<tauri::window::Monitor> {
    let main = app.get_webview_window(MAIN_LABEL)?;
    let current = main.current_monitor().ok().flatten()?;
    let monitors = app.available_monitors().ok()?;
    monitors
        .into_iter()
        .find(|m| m.position() != current.position())
}

/// Opens or closes the fullscreen face. Backs both the tray's "Fullscreen Face" item and
/// `aether1 face`, and the Esc key inside the face window itself -- one command for all
/// three so there is a single answer to "is it open", rather than three places that each
/// think they know.
#[tauri::command]
fn toggle_face_window_rust(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        match app.get_webview_window(FACE_LABEL) {
            Some(window) => {
                window.show().map_err(|e| e.to_string())?;
                window.set_focus().map_err(|e| e.to_string())
            }
            None => build_face_window(&app)
                .map(|_| ())
                .map_err(|e| e.to_string()),
        }
    } else if let Some(window) = app.get_webview_window(FACE_LABEL) {
        window.close().map_err(|e| e.to_string())
    } else {
        Ok(())
    }
}

/// Opens the avatar workbench (frontend/avatar-lab.html) in its own window: the avatar
/// engine with nothing else running, plus the builder that writes the custom avatar's
/// recipe. A separate window rather than a route inside the HUD, because the workbench
/// stands up its own engine and rebuilds it on every change -- doing that inside the HUD
/// would mean tearing down the avatar the operator is talking to.
///
/// Re-shows an existing one rather than building a second: two workbenches writing the
/// same recipe would each overwrite the other's work.
#[tauri::command]
fn open_avatar_lab_rust(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(AVATAR_LAB_LABEL) {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        AVATAR_LAB_LABEL,
        tauri::WebviewUrl::App("avatar-lab.html".into()),
    )
    .title("AETHER1 -- Avatar Workbench")
    .inner_size(1180.0, 820.0)
    .min_inner_size(720.0, 520.0)
    .resizable(true)
    .build()
    .map(|_| ())
    .map_err(|e| e.to_string())
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

/// Toggles "always on top" for whichever window invoked this command -- the "(optionally)"
/// in an undocked panel's always-on-top pane. Generic rather than sprite-specific: the sprite
/// window's own always-on-top is fixed true at build time (see build_sprite_window) because
/// that is the whole point of a desktop pet, but a decorated panel window opened via
/// open_panel_window_rust has a corner checkbox (see the solo-pin-control markup in
/// index.html and initSoloPanel in app.js) that calls this to opt in.
/// Step 19's dropdown, as data: every speciality, what it will run on, and why.
///
/// One command rather than one per persona, because the dropdown is a table and asking
/// sixteen times to draw it would put sixteen settings reads and a cached list lookup
/// behind one panel opening.
#[tauri::command]
async fn speciality_models_rust(
    engine: tauri::State<'_, LlmEngine>,
) -> Result<serde_json::Value, String> {
    let db = engine.db();
    let endpoint = db.get_setting_string("llm_endpoint", "http://localhost:11434");
    // Asked directly rather than through the engine's cache: a panel the operator just
    // opened to change a model is exactly when a minute-old list is the wrong answer.
    let answered = model_scanner::models_at(&endpoint);
    let available = answered.clone().unwrap_or_default();
    let speeds = llm::routing::measured_speeds(db);

    let specialities: Vec<serde_json::Value> = llm::Persona::all()
        .iter()
        .map(|persona| {
            let choice = llm::routing::resolve(db, persona, &available, &speeds);
            let suggested = llm::routing::suggestion(persona, &available, &speeds);
            serde_json::json!({
                "key": persona.key(),
                "speciality": persona.speciality(),
                "chosen": llm::routing::choices(db).get(persona.key()),
                "suggested": suggested,
                "running": choice.model,
                "reason": match choice.reason {
                    llm::routing::Reason::Chosen => "chosen",
                    llm::routing::Reason::Suggested => "suggested",
                    llm::routing::Reason::Missing(_) => "missing",
                    llm::routing::Reason::General => "general",
                },
                "notice": choice.notice(persona),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "endpoint": endpoint,
        // The three-way distinction the routing rests on, preserved for the panel: null is
        // "nothing answered", [] is "answered with nothing loaded".
        "available": answered.map(|models| {
            models
                .into_iter()
                .map(|model| {
                    serde_json::json!({
                        "good_at": llm::routing::good_at(&model),
                        "model": model,
                    })
                })
                .collect::<Vec<_>>()
        }),
        "specialities": specialities,
    }))
}

/// Points one speciality at a model, or clears it back to the suggestion.
#[tauri::command]
async fn set_speciality_model_rust(
    engine: tauri::State<'_, LlmEngine>,
    persona: String,
    model: Option<String>,
) -> Result<(), String> {
    let resolved = llm::Persona::from_key(&persona);
    // from_key falls back rather than failing, so an unknown key would silently set the
    // default persona's model -- a quiet wrong answer in a panel nobody would think to
    // double-check.
    if resolved.key() != persona.to_ascii_lowercase() {
        return Err(format!("no speciality called {persona:?}"));
    }
    llm::routing::set_choice(engine.db(), &resolved, model.as_deref())
}

#[tauri::command]
fn set_window_always_on_top_rust(
    window: tauri::WebviewWindow,
    enabled: bool,
) -> Result<(), String> {
    window.set_always_on_top(enabled).map_err(|e| e.to_string())
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

/// Every HUD panel that can be undocked into its own window other than the hologram --
/// that one already has a floating window of its own (see build_sprite_window) with its own
/// PNGTuber-style presentation, so its "Undock" button goes there instead. Panel id, window
/// title suffix, and a starting size sane for that panel's content.
const PANEL_WINDOWS: &[(&str, &str, f64, f64)] = &[
    ("tokens", "Model Performance", 380.0, 560.0),
    ("hardware", "Hardware Telemetry", 380.0, 560.0),
    ("commands", "Quick Commands", 340.0, 260.0),
    ("chat", "Neural Dialogue Stream", 480.0, 680.0),
    // Wider and shorter than the rest: 80 columns is what a terminal is for, and a shell
    // squeezed into a 380px column wraps every second command.
    ("terminal", "Terminal", 760.0, 520.0),
];

/// Rust-native "undock" for any HUD panel that isn't the hologram: opens a normal, decorated,
/// resizable window showing just that one panel full-bleed, so a screen too small for the
/// three-column HUD (see the narrow-window stacking in frontend/css/layout.css) has somewhere
/// to send a panel instead of squeezing everything into one scrolling column. The window loads
/// the same index.html as the main HUD with `?panel=<id>` on the URL; frontend/index.html's
/// inline bootstrap script and the `[data-solo-panel]` rules in layout.css do the actual
/// hiding, so this window runs the exact same app.js the main HUD does -- nothing about a
/// panel's live behaviour (chat, telemetry polling, quick commands) is reimplemented here.
///
/// Re-shows and focuses an already-open one rather than building a second, matching
/// toggle_sprite_window_rust and open_avatar_lab_rust.
#[tauri::command]
fn open_panel_window_rust(app: tauri::AppHandle, panel: String) -> Result<(), String> {
    let Some(&(id, title, width, height)) = PANEL_WINDOWS.iter().find(|(id, ..)| *id == panel)
    else {
        return Err(format!("{panel:?} is not a panel that can be undocked"));
    };

    let label = format!("panel-{id}");
    if let Some(window) = app.get_webview_window(&label) {
        window.show().map_err(|e| e.to_string())?;
        return window.set_focus().map_err(|e| e.to_string());
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        &label,
        tauri::WebviewUrl::App(format!("index.html?panel={id}").into()),
    )
    .title(format!("AETHER1 -- {title}"))
    .inner_size(width, height)
    .min_inner_size(280.0, 200.0)
    .resizable(true)
    .build()
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Shared by both the native Tauri path and `--serve`: opens the real sqlite file at
/// backend/aether1_memory.db, falling back to a temp-dir sqlite file if that fails.
/// Copies a borrowed icon into one that can outlive the setup closure, which is what a
/// background thread needs. Tauri hands out the window icon as a borrow of the app.
fn owned_icon(base: Option<&tauri::image::Image<'_>>) -> Option<tauri::image::Image<'static>> {
    let base = base?;
    Some(tauri::image::Image::new_owned(
        base.rgba().to_vec(),
        base.width(),
        base.height(),
    ))
}

/// Tints the tray icon amber, so a crash is visible at a glance without shipping a second
/// icon file. Pushing the existing pixels towards amber rather than drawing a new image
/// means this keeps working if the icon is ever redrawn, and it keeps the silhouette --
/// what changes is the colour, which is the whole signal.
fn amber_icon(base: Option<&tauri::image::Image<'_>>) -> Option<tauri::image::Image<'static>> {
    let base = base?;
    let (width, height) = (base.width(), base.height());
    let mut rgba = base.rgba().to_vec();
    // `as_chunks_mut::<4>` rather than `chunks_exact_mut(4)`: the pixel width is a constant,
    // and saying so lets the compiler drop the remainder branch. Newer clippy asks for this
    // by name.
    for pixel in rgba.as_chunks_mut::<4>().0 {
        // Alpha is left alone: tinting the transparent parts would turn the icon into a
        // square. Everything visible is mixed halfway towards amber, which keeps the shape
        // readable where a flat fill would lose it.
        if pixel[3] == 0 {
            continue;
        }
        pixel[0] = ((pixel[0] as u16 + 0xFF) / 2) as u8;
        pixel[1] = ((pixel[1] as u16 + 0xA5) / 2) as u8;
        pixel[2] = ((pixel[2] as u16) / 2) as u8;
    }
    Some(tauri::image::Image::new_owned(rgba, width, height))
}

/// Offers the companion as first responder for one crash: the tray goes amber and says what
/// died, a notification carries the same sentence, and the HUD is handed the whole assembled
/// context so the conversation it opens already knows about the crash rather than asking.
///
/// Every step here is allowed to fail quietly except the last. A tray that will not repaint
/// or a desktop that has no notifications is a worse experience, not a reason to lose the
/// crash -- so the event to the frontend goes out regardless, and the HUD is the one surface
/// that is always told.
fn announce_crash<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    crash: &watchers::crash::Crash,
    amber: Option<tauri::image::Image<'static>>,
    plain: Option<tauri::image::Image<'static>>,
) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Some(amber) = amber {
            let _ = tray.set_icon(Some(amber));
        }
        let _ = tray.set_tooltip(Some(format!(
            "AETHER1 -- {}. Open AETHER1 to look into it.",
            crash.headline()
        )));
        // The amber is a notice, not a state to live in: it clears itself so the tray does
        // not stay orange for the rest of the session over a crash you already read.
        let app_for_reset = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(120));
            if let Some(tray) = app_for_reset.tray_by_id(TRAY_ID) {
                if let Some(plain) = plain {
                    let _ = tray.set_icon(Some(plain));
                }
                let _ = tray.set_tooltip(Some(format!(
                    "AETHER1 -- running (build {})",
                    short_hash(BUILT_COMMIT)
                )));
            }
        });
    }

    use tauri_plugin_notification::NotificationExt;
    let _ = app
        .notification()
        .builder()
        .title(crash.headline())
        .body("AETHER1 has the details. Open it to look into this together.")
        .show();

    let _ = app.emit(
        "crash-detected",
        serde_json::json!({
            "headline": crash.headline(),
            "context": crash.as_context(),
            "crash": crash,
        }),
    );
}

/// Every copy of AETHER1 on this machine, as the HUD reads it. The shape is flattened here
/// rather than derived, so the wire format stays something a person can read in devtools
/// and does not change whenever the Rust enums do.
fn installs_payload(
    found: &[installs::Install],
    running: Option<&installs::Version>,
) -> Vec<serde_json::Value> {
    found
        .iter()
        .map(|install| {
            let standing = installs::standing(install, running);
            serde_json::json!({
                "id": install.id,
                "path": install.path.display().to_string(),
                "kind": install.kind.describe(),
                "version": install.version.as_ref().map(|version| version.label()),
                "running": install.running,
                "offered": standing.offered(),
                "description": install.describe(),
                "command": match &install.removal {
                    installs::Removal::HandOver { command, .. } => Some(command.clone()),
                    _ => None,
                },
            })
        })
        .collect()
}

/// Step 48: what else is installed. Read on demand by the HUD, and at startup by the scan
/// below -- both go through installs::detect so there is one answer, not two.
#[tauri::command(async)]
fn other_installs_rust() -> Vec<serde_json::Value> {
    let machine = installs::ThisMachine;
    let found = installs::detect(&machine);
    let running = machine.running_version();
    installs_payload(&found, running.as_ref())
}

/// Removes one copy, named by the id the scan gave it. Re-detects rather than trusting an
/// id the HUD has been holding since startup: between the prompt appearing and the operator
/// answering it, the copy may already be gone.
#[tauri::command(async)]
fn remove_install_rust(id: String) -> Result<String, String> {
    let machine = installs::ThisMachine;
    let found = installs::detect(&machine);
    let install = found
        .iter()
        .find(|install| install.id == id)
        .ok_or_else(|| format!("that copy is no longer here (id {id})"))?;
    match installs::remove(install, &machine)? {
        installs::Outcome::Removed { paths } => Ok(format!(
            "Removed {} ({} file{} deleted).",
            install.path.display(),
            paths.len(),
            if paths.len() == 1 { "" } else { "s" }
        )),
        installs::Outcome::Ran { .. } => Ok(format!("Uninstalled {}.", install.path.display())),
        installs::Outcome::HandedOver { command } => Ok(format!(
            "{} belongs to a package manager. Run this to remove it:\n{command}",
            install.path.display()
        )),
    }
}

/// "Keep them, and stop asking." Answering the prompt with no is a decision, and a decision
/// that is forgotten by the next launch is a prompt that never goes away.
#[tauri::command(async)]
fn keep_other_installs_rust(engine: tauri::State<'_, LlmEngine>) -> Result<(), String> {
    engine
        .db()
        .set_setting(installs::NOTICE_SETTING, &serde_json::json!(false))
        .map_err(|e| format!("could not save that: {e}"))
}

fn build_llm_engine() -> LlmEngine {
    let db_path = project_root().join("backend").join("aether1_memory.db");
    // The native path creates backend/ as a side effect of setting up the audio cache in
    // setup(), but a headless run (--serve, or a CLI subcommand) reaches this first. Without
    // this, sqlite can't create the file in a directory that doesn't exist yet, and every
    // headless invocation on a fresh checkout would silently fall back to a temp database --
    // i.e. the CLI would keep its own separate memory until the GUI had been opened once.
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match MemoryDb::open(&db_path) {
        Ok(db) => {
            // Create the vault on first run, so the very first conversation already has
            // somewhere to remember things. Best-effort: a companion that refuses to start
            // because it could not create a folder would be worse than one with no memory.
            if let Err(e) = vault::ensure(&db) {
                eprintln!("[AETHER1] Memory vault unavailable: {e}");
            }
            // Likewise once, on the first run only: the programs the companion may ask to
            // run. See STARTER_ALLOWLIST for why the list is short and what keeps it safe.
            tools::mutating::seed_starter_allowlist(&db);
            LlmEngine::new(db)
        }
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

    // Everything except a bare launch short-circuits before tauri::Builder is ever
    // constructed, so the native app's setup (tray, asset-protocol scope, window) never
    // runs in a headless process. See cli.rs for the argument parsing and server.rs for
    // the axum app.
    let invocation = cli::parse(&std::env::args().collect::<Vec<_>>());
    // `aether1 face` with nothing already running: this launch becomes the instance, and
    // the face has to be opened from setup() below rather than by the single-instance
    // handler, which only ever runs for the *second* launch.
    let open_face_at_launch = matches!(invocation, cli::Invocation::Face);
    match invocation {
        // `show`/`toggle`/`face` continue into the app path: the single-instance plugin
        // below hands their argv to the already-running instance, and if there isn't one,
        // this launch becomes it.
        cli::Invocation::App | cli::Invocation::Window { .. } | cli::Invocation::Face => {}
        // Headless HTTP mode.
        cli::Invocation::Serve { lan } => {
            let engine = build_llm_engine();
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime for --serve");
            runtime.block_on(server::run(engine, lan));
            return;
        }
        // One-shot CLI: prompt/status/say/discover/announce/help/version.
        other => std::process::exit(cli::run(other)),
    }

    let llm_engine = build_llm_engine();

    tauri::Builder::default()
        // Must be the first plugin registered (see the plugin's own docs). A second
        // `aether1` launch -- including `aether1 show` and `aether1 toggle` -- exits
        // immediately after handing its argv to the instance already running, which is
        // what makes those subcommands reach this window, and what stops the tray from
        // sprouting a second icon when the app is launched twice.
        .plugin(tauri_plugin_single_instance::init(
            |app, argv, _cwd| match cli::parse(&argv) {
                cli::Invocation::Window { toggle: true } => hotkey::toggle_window(app),
                // `aether1 face` from a second launch opens the face and leaves the HUD
                // exactly as it was -- summoning the main window too would undo the point
                // of a command whose whole job is to put something on a *different* screen.
                cli::Invocation::Face => {
                    if let Err(e) = toggle_face_window_rust(app.clone(), true) {
                        eprintln!("[AETHER1] Could not open the fullscreen face: {e}");
                    }
                }
                _ => hotkey::show_window(app),
            },
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(llm_engine)
        .manage(background_services::ManagedOllama::default())
        // Only the native app ever has this. `--serve` returns from main() long before
        // here, so in a headless run the map of live terminals does not exist to be
        // reached -- the isolation is a fact about the process, not a check that has to be
        // remembered.
        .manage(terminal::Handle::default())
        .invoke_handler(tauri::generate_handler![
            generate_response_rust,
            generate_response_streaming_rust,
            agent_genesis_rust,
            test_llm_connection_rust,
            scan_models_rust,
            speciality_models_rust,
            set_speciality_model_rust,
            setup_advice_rust,
            pull_model_rust,
            start_download_rust,
            download_status_rust,
            forget_download_rust,
            voice_catalogue_rust,
            start_voice_download_rust,
            voice_download_status_rust,
            forget_voice_download_rust,
            start_local_server_rust,
            get_static_info_rust,
            get_tools_rust,
            get_actions_rust,
            pending_actions_rust,
            approve_action_rust,
            undo_action_rust,
            reject_action_rust,
            set_always_allowed_rust,
            persona_access_rust,
            set_persona_access_rust,
            get_messages_rust,
            clear_messages_rust,
            list_sessions_rust,
            new_session_rust,
            rename_session_rust,
            delete_session_rust,
            terminal_open_rust,
            terminal_write_rust,
            terminal_resize_rust,
            terminal_close_rust,
            reset_benchmarks_rust,
            open_vault_folder_rust,
            vault_notes_rust,
            vault_note_rust,
            vault_graph_rust,
            vault_search_rust,
            get_settings_rust,
            save_settings_rust,
            generate_speech_rust,
            transcribe_rust,
            voice_status_rust,
            voice_advice_rust,
            test_speech_rust,
            list_personas_rust,
            get_version_info,
            other_installs_rust,
            remove_install_rust,
            keep_other_installs_rust,
            check_for_update_rust,
            apply_update_rust,
            install_gh_via_winget_rust,
            toggle_sprite_window_rust,
            toggle_face_window_rust,
            open_avatar_lab_rust,
            open_panel_window_rust,
            set_window_always_on_top_rust,
            show_main_window_rust,
            start_window_drag_rust,
            set_game_mode_rust
        ])
        .on_window_event(|window, event| {
            // Closing the main HUD window would otherwise exit the whole app (Tauri's
            // default with no other running windows/tray keeping it alive) -- but if the
            // desktop sprite or the fullscreen face is up, the app should keep running
            // headless-with-avatar instead, matching the tray's existing "Show AETHER1"
            // affordance. Both count, and for the same reason: each is a window showing the
            // avatar somewhere other than the HUD, and quitting out from under one of them
            // reads as the close button having killed a window on a different screen. Only
            // intercepts the close when one of them is actually open, so anyone not using
            // either feature sees the same close-quits-the-app behavior as before.
            if window.label() == MAIN_LABEL {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    let app = window.app_handle();
                    let avatar_is_elsewhere = app.get_webview_window(SPRITE_LABEL).is_some()
                        || app.get_webview_window(FACE_LABEL).is_some();
                    if avatar_is_elsewhere {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        })
        .setup(move |app| {
            // Let the webview load synthesized speech files directly off disk via
            // convertFileSrc (see synthesizeSpeechUrl in frontend/js/app.js) -- the asset
            // protocol is opt-in per-directory, and this path is only known at runtime (it's
            // relative to wherever this checkout happens to live), so it's granted here
            // rather than as a fixed glob in tauri.conf.json.
            let audio_cache_dir = project_root().join("backend").join("audio_cache");
            std::fs::create_dir_all(&audio_cache_dir)?;
            app.asset_protocol_scope()
                .allow_directory(&audio_cache_dir, false)?;

            // Global hotkey to summon/dismiss the HUD. A chord that won't parse or is
            // already taken by another application is a warning, never a startup failure --
            // the tray and `aether1 toggle` both still work without it.
            match hotkey::reregister_from_settings(app.handle()) {
                Ok(()) => hotkey::warn_if_wayland(
                    &app.state::<LlmEngine>()
                        .db()
                        .get_setting_string("hotkey_toggle", hotkey::DEFAULT_TOGGLE),
                ),
                Err(e) => eprintln!("[AETHER1] Global hotkey not registered: {e}"),
            }

            // Startup & Performance: reconcile the login-autostart entry with the saved
            // setting (it can drift -- e.g. someone deletes the .desktop file by hand), then
            // auto-start Ollama if that's turned on and Game Mode isn't currently active
            // (Game Mode's whole point is to keep it stopped across a restart too).
            {
                let engine = app.state::<LlmEngine>();
                let autostart_app = engine.db().get_setting_bool("autostart_app", false);
                if let Err(e) = sync_autostart(app.handle(), autostart_app) {
                    eprintln!("[AETHER1] Could not sync launch-at-login: {e}");
                }

                let autostart_ollama = engine.db().get_setting_bool("autostart_ollama", false);
                let game_mode = engine.db().get_setting_bool("game_mode", false);
                if autostart_ollama && !game_mode {
                    let managed = app.state::<background_services::ManagedOllama>();
                    let result = background_services::start_ollama_if_needed(&engine, &managed);
                    if result["ok"] != serde_json::json!(true) {
                        eprintln!(
                            "[AETHER1] Auto-start of the model server: {}",
                            result["message"].as_str().unwrap_or("unknown error")
                        );
                    }
                }
            }

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
            // In the tray rather than only in the HUD: the face is for a screen you are not
            // sitting in front of, and reaching it should not require first summoning the
            // window it is meant to replace.
            let face_item =
                MenuItem::with_id(app, "face", "🙂 Fullscreen Face", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit AETHER1", true, None::<&str>)?;
            let tray_menu =
                Menu::with_items(app, &[&show_item, &face_item, &update_item, &quit_item])?;

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
                                                local_only_enabled(&app),
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
                        // A toggle, like the menu item's own wording implies: clicking it
                        // again is how you get a fullscreen, undecorated window back off a
                        // screen if Esc did not reach it (a face on a monitor without focus
                        // never sees a keystroke).
                        "face" => {
                            let open = app.get_webview_window(FACE_LABEL).is_some();
                            if let Err(e) = toggle_face_window_rust(app.clone(), !open) {
                                eprintln!("[AETHER1] Could not toggle the fullscreen face: {e}");
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
                let launch_local_only = local_only_enabled(&app.handle().clone());
                update_in_progress.store(true, Ordering::Relaxed);
                std::thread::spawn(move || {
                    run_update_check(&tray, &update_item, &update_available, launch_local_only);
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

            // `aether1 face` started this process. Not persisted the way the sprite is:
            // asking for the face once is asking for it now, not forever -- a fullscreen
            // window that reappears on every launch until you find the setting that stops
            // it is a trap, and the tray item is right there when you want it again.
            if open_face_at_launch {
                if let Err(e) = build_face_window(app.handle()) {
                    eprintln!("[AETHER1] Could not open the fullscreen face: {e}");
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
                        // Step 19: usually null. It carries one line when a model the
                        // operator picked has been uninstalled, said once per model per
                        // session -- taken here rather than read, so collecting it is what
                        // clears it and two windows cannot both claim to have shown it.
                        "routing_notice": engine.take_routing_notice(),
                    });
                    let _ = app_handle.emit("telemetry-update", payload);
                    // Game Mode's "low usage" half: the HUD window is hidden (so its own
                    // render loop is already throttled by the webview), but this thread
                    // keeps sampling sysinfo regardless of window visibility -- so it's the
                    // one piece Game Mode has to slow down itself rather than getting for
                    // free. Read fresh every tick rather than cached, so switching Game
                    // Mode off is felt on the very next tick instead of waiting out a long
                    // sleep that was already in progress.
                    let game_mode = engine.db().get_setting_bool("game_mode", false);
                    let interval = if game_mode { 5000 } else { 1000 };
                    std::thread::sleep(Duration::from_millis(interval));
                });
            }

            // Step 13: crash capture. A companion that only knows what you type at it is
            // something you go to; one that notices your editor just died is a first
            // responder. The watcher is the only thing in AETHER1 that speaks without
            // being asked, which is why it is crashes only -- see watchers/crash.rs.
            {
                let app_handle = app.handle().clone();
                let amber = amber_icon(app.default_window_icon());
                let plain = owned_icon(app.default_window_icon());
                std::thread::spawn(move || {
                    let mut watch = watchers::crash::CrashWatch::new();
                    // Said once, at startup, rather than swallowed: a machine that cannot
                    // be watched should not look like a machine that never crashes.
                    if let watchers::crash::Availability::Unavailable(reason) = watch.availability()
                    {
                        println!("[AETHER1] crash capture is not available here. {reason}");
                        return;
                    }
                    loop {
                        std::thread::sleep(watchers::crash::POLL_INTERVAL);
                        let engine = app_handle.state::<LlmEngine>();
                        if !engine
                            .db()
                            .get_setting_bool(watchers::crash::ENABLED_SETTING, true)
                        {
                            continue;
                        }
                        let muted = watchers::crash::muted_programs(
                            engine
                                .db()
                                .get_setting(watchers::crash::MUTED_SETTING)
                                .ok()
                                .flatten(),
                        );
                        let Ok(news) = watch.poll(&muted) else {
                            continue;
                        };
                        for crash in news {
                            announce_crash(&app_handle, &crash, amber.clone(), plain.clone());
                        }
                    }
                });
            }

            // Step 48: the copies of AETHER1 that are not this one. Installing a second way
            // never removes the first, so an old binary keeps sitting on PATH with a fixed
            // bug still in it. Said once, at startup, and only when there is something to
            // say -- on the machine of someone who installed it once this thread finds
            // nothing and stays quiet forever.
            //
            // Detection only. Nothing is removed here: the prompt this raises is the ask,
            // and an uninstall that happened before anyone was asked cannot be taken back.
            {
                let app_handle = app.handle().clone();
                std::thread::spawn(move || {
                    // After the window and the tray, so a scan that runs long does not delay
                    // either, and after the operator has seen the app start.
                    std::thread::sleep(Duration::from_secs(5));
                    let engine = app_handle.state::<LlmEngine>();
                    if !engine.db().get_setting_bool(installs::NOTICE_SETTING, true) {
                        return;
                    }
                    let machine = installs::ThisMachine;
                    let found = installs::detect(&machine);
                    let running = machine.running_version();
                    let others = installs::others(&found, running.as_ref());
                    if others.is_empty() {
                        return;
                    }
                    let headline = match others.len() {
                        1 => "AETHER1 found another copy of itself installed".to_string(),
                        n => format!("AETHER1 found {n} other copies of itself installed"),
                    };
                    use tauri_plugin_notification::NotificationExt;
                    let _ = app_handle
                        .notification()
                        .builder()
                        .title(headline.clone())
                        .body("Open AETHER1 to remove them, or run `aether1 installs`.")
                        .show();
                    let _ = app_handle.emit(
                        "old-installs-detected",
                        serde_json::json!({
                            "headline": headline,
                            "installs": installs_payload(&others, running.as_ref()),
                        }),
                    );
                });
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, _event| {});
}

#[cfg(test)]
mod ipc_thread_tests {
    /// The freeze, pinned as a test.
    ///
    /// Tauri runs a synchronous `#[tauri::command]` **on the main thread** -- the same
    /// thread that draws the window and answers the operating system. Every command here
    /// was synchronous, and most of them block: ureq waits on a socket, rusqlite waits on
    /// a file, the speech engines wait on a subprocess or a WebSocket to Microsoft. So
    /// pressing a button that asks the model anything froze the HUD until the answer came
    /// back, and Windows put "(Not Responding)" in the title bar -- not a slow model, a
    /// blocked event loop.
    ///
    /// `#[tauri::command(async)]` on a synchronous function runs it on a thread pool
    /// instead (see tauri-macros: `ExecutionContext::Async` with no `asyncness` resolves
    /// to `sync_threadpool`), which is what every command that touches the network, the
    /// disk, the database or a subprocess needs.
    ///
    /// Reading our own source is a blunt instrument, but the alternative is a rule that
    /// lives only in somebody's memory -- and the cost of forgetting it is the whole app
    /// locking up, which is exactly the kind of thing that comes back.
    const SOURCE: &str = include_str!("main.rs");

    /// The commands that must stay on the main thread: they create, show or drag windows.
    /// None of them blocks on anything, so none of them can freeze the HUD.
    const MAIN_THREAD_ONLY: &[&str] = &[
        "toggle_sprite_window_rust",
        "toggle_face_window_rust",
        "open_avatar_lab_rust",
        "open_panel_window_rust",
        "set_window_always_on_top_rust",
        "show_main_window_rust",
        "start_window_drag_rust",
    ];

    #[test]
    fn no_command_blocks_the_window() {
        let mut offenders = Vec::new();
        for (i, line) in SOURCE.lines().enumerate() {
            if line.trim() != "#[tauri::command]" {
                continue;
            }
            // The declaration is the next line that starts a function.
            let name = SOURCE
                .lines()
                .skip(i + 1)
                .find_map(|l| l.trim().strip_prefix("fn ").map(|rest| rest.trim_end()))
                .map(|rest| {
                    rest.split(['(', '<'])
                        .next()
                        .unwrap_or(rest)
                        .trim()
                        .to_string()
                })
                .unwrap_or_else(|| format!("the command on line {}", i + 1));
            if !MAIN_THREAD_ONLY.contains(&name.as_str()) {
                offenders.push(name);
            }
        }
        assert!(
            offenders.is_empty(),
            "these commands run on the main thread and will freeze the window if they \
             ever block -- use #[tauri::command(async)], or add them to MAIN_THREAD_ONLY \
             if they genuinely must run there: {offenders:?}",
        );
    }

    /// The allow-list only means anything while the names in it are real.
    #[test]
    fn the_main_thread_list_is_not_stale() {
        for name in MAIN_THREAD_ONLY {
            assert!(
                SOURCE.contains(&format!("fn {name}(")),
                "{name} is allowed on the main thread but no longer exists",
            );
        }
    }
}
