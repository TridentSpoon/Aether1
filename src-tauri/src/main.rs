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

use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

const BACKEND_HOST: &str = "127.0.0.1";
const BACKEND_PORT: u16 = 8378;

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

fn main() {
    let backend: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(spawn_backend()));

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
        .setup(|app| {
            // Native tray icon so there's a visible indicator (and a quick way to
            // reopen/quit) while AETHER1 runs headlessly in the background.
            let show_item = MenuItem::with_id(app, "show", "Show AETHER1", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit AETHER1", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("AETHER1 -- running")
                .menu(&tray_menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

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
