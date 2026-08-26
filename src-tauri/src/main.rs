// AETHER1 native shell (incremental Rust port).
//
// This first step only proves out the window + existing web frontend
// (Three.js avatars, HUD) rendering correctly inside Tauri's native
// webview. Backend logic (chat, TTS, telemetry, memory) still runs via
// the existing Python/FastAPI server during this incremental migration;
// Rust commands will replace pieces of it over time.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
