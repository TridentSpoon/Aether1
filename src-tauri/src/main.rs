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

mod cli;
mod commands;
mod discovery;
mod downloads;
mod hotkey;
mod llm;
mod local_only;
mod model_scanner;
mod paths;
mod serve_auth;
mod server;
mod setup;
mod tools;
mod vault;
mod voice_download;
mod voice_setup;

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
const AVATAR_LAB_LABEL: &str = "avatar-lab";

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

/// Rust-native equivalent of POST /api/settings (backend/main.py).
#[tauri::command(async)]
fn save_settings_rust(
    app: tauri::AppHandle,
    engine: tauri::State<LlmEngine>,
    settings: serde_json::Value,
) -> Result<(), String> {
    let hotkey_changed = settings.get("hotkey_toggle").is_some();
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
    match invocation {
        // `show`/`toggle` continue into the app path: the single-instance plugin below
        // hands their argv to the already-running instance, and if there isn't one, this
        // launch becomes it.
        cli::Invocation::App | cli::Invocation::Window { .. } => {}
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
                _ => hotkey::show_window(app),
            },
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(llm_engine)
        .invoke_handler(tauri::generate_handler![
            generate_response_rust,
            generate_response_streaming_rust,
            agent_genesis_rust,
            test_llm_connection_rust,
            scan_models_rust,
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
            check_for_update_rust,
            apply_update_rust,
            install_gh_via_winget_rust,
            toggle_sprite_window_rust,
            open_avatar_lab_rust,
            open_panel_window_rust,
            set_window_always_on_top_rust,
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
                    if window
                        .app_handle()
                        .get_webview_window(SPRITE_LABEL)
                        .is_some()
                    {
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
