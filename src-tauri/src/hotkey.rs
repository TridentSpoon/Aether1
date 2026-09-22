// Global hotkey: one chord, registered OS-wide, that summons or dismisses the HUD.
//
// The chord itself is a setting (hotkey_toggle in the settings table) rather than a
// compile-time constant, so it can be changed from the Settings panel and re-registered
// live -- see reregister_from_settings, called by save_settings_rust.
//
// Linux caveat worth knowing about: the underlying global-hotkey crate talks X11, and
// Wayland compositors deliberately don't let an application grab keys system-wide. Under a
// Wayland session the registration below usually "succeeds" via XWayland and then never
// fires, which is a confusing way to fail -- so warn_if_wayland says so plainly at startup
// and points at the alternative that does work there: bind the compositor's own keybinding
// to `aether1 toggle` (see cli.rs), which reaches the running instance through the
// single-instance plugin.

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Super is the least-contested modifier across desktops, and Super+Shift+A is not a
/// default binding in GNOME, KDE, Hyprland or Windows.
pub const DEFAULT_TOGGLE: &str = "Super+Shift+A";

/// Shows and focuses the HUD, or hides it when it's already the visible, focused window.
pub fn toggle_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    // Visible but unfocused (behind something else, on another workspace) should raise it,
    // not hide it -- hiding a window the operator can't currently see reads as the hotkey
    // doing nothing.
    let showing = window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false);
    if showing {
        let _ = window.hide();
    } else {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Shows and focuses the HUD unconditionally.
pub fn show_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Whether the last attempt to register a chord succeeded, for anyone asking from outside.
/// `None` until something has tried, which is what a process with no hotkey to register --
/// `aether1 doctor` in a terminal -- honestly reports. A static rather than a returned value
/// because the question is asked much later, by doctor.rs, from a different call stack.
static REGISTERED: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(-1);

/// The answer to "did the hotkey take?", or None when nothing in this process has tried.
pub fn registered() -> Option<bool> {
    match REGISTERED.load(std::sync::atomic::Ordering::Relaxed) {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn remember(result: &Result<(), String>) {
    REGISTERED.store(
        if result.is_ok() { 1 } else { 0 },
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// Registers `chord` as the toggle hotkey, replacing whatever was registered before.
/// Returns the parse/registration error rather than panicking: a bad chord in the settings
/// table must not stop the app from starting.
pub fn register<R: Runtime>(app: &AppHandle<R>, chord: &str) -> Result<(), String> {
    let shortcut: Shortcut = chord
        .parse()
        .map_err(|e| format!("{chord:?} is not a valid shortcut: {e}"))?;

    let manager = app.global_shortcut();
    // unregister_all() rather than unregistering the previous chord specifically: this is
    // the only shortcut the app registers, so there's nothing else to preserve, and it
    // means a failed re-register can't leave two chords both live.
    let _ = manager.unregister_all();

    let result = manager
        .on_shortcut(shortcut, |app, _shortcut, event| {
            // Press only. Without this the handler runs again on release and the window
            // toggles straight back.
            if event.state == ShortcutState::Pressed {
                toggle_window(app);
            }
        })
        .map_err(|e| format!("could not register {chord:?}: {e}"));
    remember(&result);
    result
}

/// Re-reads hotkey_toggle from the settings table and registers it. Called at startup and
/// again whenever settings are saved from the HUD.
pub fn reregister_from_settings<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let chord = app
        .state::<crate::llm::LlmEngine>()
        .db()
        .get_setting_string("hotkey_toggle", DEFAULT_TOGGLE);
    if chord.trim().is_empty() {
        // An empty chord is how the operator turns the hotkey off. Nothing was attempted, so
        // the registration question goes back to having no answer rather than a false one.
        let _ = app.global_shortcut().unregister_all();
        REGISTERED.store(-1, std::sync::atomic::Ordering::Relaxed);
        return Ok(());
    }
    register(app, chord.trim())
}

/// True when this looks like a Wayland session, where an app-registered global hotkey
/// won't actually fire.
pub fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|t| t.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
}

pub fn warn_if_wayland(chord: &str) {
    if !is_wayland() {
        return;
    }
    eprintln!(
        "[AETHER1] Wayland session detected: {chord:?} is registered but Wayland compositors \
         don't allow an application to grab keys system-wide, so it probably won't fire.\n\
         [AETHER1] Bind your compositor's own keybinding to `aether1 toggle` instead -- e.g. \
         for Hyprland, in hyprland.conf:\n\
         [AETHER1]     bind = SUPER SHIFT, A, exec, aether1 toggle"
    );
}
