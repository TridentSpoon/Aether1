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

/// The chord that types the last answer into whatever text box has the focus (see
/// handoff.rs). Registered only when the operator has turned the hand-off on, which is why
/// a chord on the same modifiers as the toggle is safe to default to: on an install that
/// never asked for this, nothing is grabbed. V for the reason Ctrl+V is V.
pub const DEFAULT_HANDOFF: &str = "Super+Shift+V";

/// The setting holding the chord above, so it can be changed from the Settings pane.
pub const HANDOFF_SETTING: &str = "hotkey_handoff";

/// The event the hand-off chord fires.
///
/// The chord does not type anything itself: it asks the HUD to, because the HUD is what
/// holds the answer. Rust keeping its own copy of the last reply would be a second answer
/// to "what did it say", and the one place that already knows is the page that rendered it.
/// See initAnswerHandoff in frontend/js/app.js.
pub const HANDOFF_EVENT: &str = "answer-handoff-requested";

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

/// Registers the chords: the HUD toggle, and -- when the hand-off is switched on -- the one
/// that types the last answer into the focused text box.
///
/// Returns the toggle's own parse/registration error rather than panicking: a bad chord in
/// the settings table must not stop the app from starting. A bad *hand-off* chord is
/// reported the same way but does not take the toggle down with it, because the toggle is
/// how an operator who has mistyped a chord gets their window back.
pub fn register<R: Runtime>(
    app: &AppHandle<R>,
    chord: &str,
    handoff_chord: Option<&str>,
) -> Result<(), String> {
    let manager = app.global_shortcut();
    // unregister_all() rather than unregistering the previous chords specifically: these are
    // the only shortcuts the app registers, so there is nothing else to preserve, and it
    // means a failed re-register cannot leave two chords both live.
    let _ = manager.unregister_all();

    // The hand-off first, and its failure kept rather than returned: registering it after
    // the toggle would mean a bad hand-off chord silently skipping the toggle's own
    // registration on the way out.
    let mut handoff_error = None;
    if let Some(handoff_chord) = handoff_chord.map(str::trim).filter(|c| !c.is_empty()) {
        match handoff_chord.parse::<Shortcut>() {
            Ok(shortcut) => {
                if let Err(e) = manager.on_shortcut(shortcut, |app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        request_handoff(app);
                    }
                }) {
                    handoff_error = Some(format!("could not register {handoff_chord:?}: {e}"));
                }
            }
            Err(e) => {
                handoff_error = Some(format!("{handoff_chord:?} is not a valid shortcut: {e}"));
            }
        }
    }

    let result = chord
        .parse::<Shortcut>()
        .map_err(|e| format!("{chord:?} is not a valid shortcut: {e}"))
        .and_then(|shortcut| {
            manager
                .on_shortcut(shortcut, |app, _shortcut, event| {
                    // Press only. Without this the handler runs again on release and the
                    // window toggles straight back.
                    if event.state == ShortcutState::Pressed {
                        toggle_window(app);
                    }
                })
                .map_err(|e| format!("could not register {chord:?}: {e}"))
        });
    remember(&result);
    if let Some(e) = handoff_error {
        eprintln!("[AETHER1] The answer hand-off chord was not registered: {e}");
    }
    result
}

/// Asks the HUD to type the last answer into whatever has the focus.
///
/// Broadcast to every window rather than sent to the main one, because this is also how
/// `aether1 type-answer` reaches it (see the single-instance handler in main.rs) and the
/// answering page is the HUD whether or not it is the window on screen. Exactly one page
/// listens -- see the note on HANDOFF_EVENT.
pub fn request_handoff<R: Runtime>(app: &AppHandle<R>) {
    use tauri::Emitter;
    let _ = app.emit(HANDOFF_EVENT, ());
}

/// Re-reads hotkey_toggle from the settings table and registers it. Called at startup and
/// again whenever settings are saved from the HUD.
pub fn reregister_from_settings<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let db = app.state::<crate::llm::LlmEngine>();
    let chord = db.db().get_setting_string("hotkey_toggle", DEFAULT_TOGGLE);
    // Only when the operator has turned the hand-off on. An install that has not asked for
    // it grabs nothing, which is why the default chord can sit on the same modifiers as the
    // toggle without taking a key combination away from anybody.
    let handoff_chord = if db.db().get_setting_bool(crate::handoff::SETTING, false) {
        Some(db.db().get_setting_string(HANDOFF_SETTING, DEFAULT_HANDOFF))
    } else {
        None
    };
    if chord.trim().is_empty() {
        // An empty chord is how the operator turns the hotkey off. Nothing was attempted, so
        // the registration question goes back to having no answer rather than a false one.
        let _ = app.global_shortcut().unregister_all();
        REGISTERED.store(-1, std::sync::atomic::Ordering::Relaxed);
        // The hand-off chord is registered by the same call, so an empty toggle must not
        // take it down with it: register with the toggle left out instead of returning.
        if let Some(handoff_chord) = handoff_chord.as_deref() {
            let _ = register_handoff_only(app, handoff_chord);
        }
        return Ok(());
    }
    register(app, chord.trim(), handoff_chord.as_deref())
}

/// The hand-off chord on its own, for the install that has turned the HUD toggle off.
fn register_handoff_only<R: Runtime>(app: &AppHandle<R>, chord: &str) -> Result<(), String> {
    let shortcut: Shortcut = chord
        .parse()
        .map_err(|e| format!("{chord:?} is not a valid shortcut: {e}"))?;
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                request_handoff(app);
            }
        })
        .map_err(|e| format!("could not register {chord:?}: {e}"))
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
