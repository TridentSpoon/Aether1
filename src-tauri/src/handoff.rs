// Putting the companion's answer into somebody else's text box.
//
// Everything else in this program writes into windows it owns. This writes into whatever the
// operator is pointing at -- a browser form, an editor, a chat client, a terminal -- by
// synthesising keystrokes at the operating system, the way a keyboard does. There is no
// other mechanism: a third-party text box has no API, and the clipboard still needs somebody
// to press paste.
//
// Trident, 2026-11-07: *"I also want it to be able to input it's answer into a 3rd party text
// box of my choosing."* The shape that follows from "of my choosing" is that the operator
// chooses by focusing the box, and the companion never takes focus -- see the chord in
// hotkey.rs. A window that had to be clicked would steal the focus it is trying to type into.
//
// Three properties are deliberate and worth not undoing:
//
//   1. **Nothing is typed unless the operator asked for it in that moment.** There is no
//      "always send answers" setting, and the model cannot ask for this -- it is not a tool,
//      it is not in the registry, and no prompt can reach it. Synthetic keystrokes go
//      wherever the focus is, and a companion that could type unprompted into a terminal is
//      a companion that can run commands. See the `handoff_enabled` setting, off by default.
//   2. **Return is never sent at the end.** `prepare` strips trailing newlines, so the last
//      line of an answer lands in the box without being submitted. A shell runs a line when
//      it sees Return; so does a chat client, and so does half the web. The operator presses
//      it, or does not.
//   3. **The modifiers are released first.** The chord that asks for this is held down at the
//      moment it fires -- Super and Shift are physically pressed. Typing underneath them
//      produces compositor shortcuts rather than text, which on a tiling window manager
//      means a few dozen random commands. `release_modifiers` lets go of them on the
//      operating system's behalf before the first character.
//
// Linux is two platforms here. X11 takes synthetic input through XTEST and works. Wayland
// deliberately does not let one client type into another, and only compositors implementing
// the wlroots virtual-keyboard protocol (Sway, Hyprland, river) make an exception; GNOME and
// KDE do not, and there the honest answer is that this cannot work, which `availability`
// gives in those words rather than leaving a chord that silently does nothing. That is the
// same wall hotkey.rs already documents for the global shortcut.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// The operator's standing answer to "may AURORA1 type into other applications". Off until
/// it is switched on, and checked in Rust rather than only in the page -- see
/// answer_handoff_rust in main.rs.
pub const SETTING: &str = "handoff_enabled";

/// Whether line breaks are flattened to spaces before typing. Remembered because the answer
/// depends on where the operator usually sends text, not on the answer being sent.
///
/// **It defaults to on, and that was learned the hard way.** The first live run of this
/// typed a three-paragraph answer into a terminal, and a terminal acts on Return: every
/// line but the last ran as a command. Stripping the *trailing* newline -- property 2 at the
/// top of this file -- only ever protected the last line, which is the one line that was
/// never the problem. Flattened, the whole answer arrives as one line sitting at the prompt
/// with nothing submitted, which is the behaviour somebody expects from "put the answer in
/// this box". Turning it off is a deliberate choice for a box that wants paragraphs, and the
/// switch says what it costs.
pub const FLATTEN_SETTING: &str = "handoff_flatten";

/// The longest answer that will be typed in one go.
///
/// Not a safety limit -- a bound on how long the operator's keyboard is not their own. Text
/// is entered a character at a time on some platforms, so a very long answer is seconds of
/// keystrokes landing in a box that the operator can edit, scroll or close halfway through.
/// Past this the answer is better copied and pasted, and `type_text` says so.
pub const MAX_CHARS: usize = 8000;

/// How long to wait after letting go of the modifiers before typing.
///
/// The release above is sent to the operating system, which has to deliver it to the focused
/// client before the first character arrives, or that character is still read as part of the
/// chord. A tenth of a second is imperceptible to the operator and ample for every
/// compositor tested.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(120);

/// Whether synthetic typing can work on this machine, and what to tell the operator when it
/// cannot.
#[derive(serde::Serialize, Debug, PartialEq, Eq)]
pub struct Availability {
    /// Whether to offer the feature at all.
    pub ok: bool,
    /// The display server, as one word for the UI: `x11`, `wayland`, `windows`, `macos` or
    /// `unknown`.
    pub display: String,
    /// One sentence, written to be shown as-is.
    pub why: String,
    /// Why the chord will not fire, and what to bind instead -- or empty where it will.
    ///
    /// Separate from `why` because the two come apart, and only on Wayland: on Hyprland or
    /// Sway typing into another window works perfectly (`ok` is true) while
    /// `Super+Shift+V` registered by this application never arrives, because Wayland has no
    /// way for a client to claim a key combination. An operator told only that typing works
    /// would press the chord and conclude the feature is broken.
    pub chord: String,
}

/// What to say about the chord on a Wayland session, where the application cannot register
/// one at all. Empty everywhere else, because there the chord is registered and works.
///
/// The Hyprland line is spelled out rather than described: it is the compositor Trident
/// runs, and a line that can be pasted is worth more than a pointer to a manual.
#[cfg(all(unix, not(target_os = "macos")))]
fn chord_note(on_wayland: bool) -> String {
    if !on_wayland {
        return String::new();
    }
    "Wayland does not let an application claim a key combination, so the Super+Shift+V chord \
     will not fire. Bind your compositor's own keybinding to `aether1 type-answer` instead -- \
     in hyprland.conf, that is: bind = SUPER SHIFT, V, exec, aether1 type-answer"
        .into()
}

/// `availability`, from an already-read environment, so the decision is testable without a
/// display server.
///
/// `session` is `XDG_SESSION_TYPE` and `wayland` is `WAYLAND_DISPLAY`. Both are consulted
/// because neither is reliable alone: a session started outside a display manager often has
/// no `XDG_SESSION_TYPE` at all, and an X11 application running under XWayland sees
/// `WAYLAND_DISPLAY` set by the compositor that launched it.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn availability_from(
    session: Option<&str>,
    wayland: Option<&str>,
    desktop: Option<&str>,
) -> Availability {
    let session = session.unwrap_or("").to_ascii_lowercase();
    let on_wayland =
        session == "wayland" || (session.is_empty() && wayland.is_some_and(|v| !v.is_empty()));

    if !on_wayland {
        return Availability {
            ok: true,
            display: if session == "x11" {
                "x11".into()
            } else {
                "unknown".into()
            },
            why: "Typing into another window goes through X11, which allows it.".into(),
            chord: chord_note(false),
        };
    }

    // The compositors that implement zwp_virtual_keyboard_manager_v1. Read from
    // XDG_CURRENT_DESKTOP, which every one of them sets, and matched loosely because the
    // value is a colon-separated list and the spelling varies by distribution.
    let desktop = desktop.unwrap_or("").to_ascii_lowercase();
    let virtual_keyboard = ["sway", "hyprland", "river", "wlroots", "niri", "labwc"]
        .iter()
        .any(|name| desktop.contains(name));

    if virtual_keyboard {
        Availability {
            ok: true,
            display: "wayland".into(),
            why: "Your compositor supports the virtual-keyboard protocol, so typing into another window works.".into(),
            chord: chord_note(true),
        }
    } else {
        Availability {
            ok: false,
            display: "wayland".into(),
            why: "Wayland does not let one application type into another, and this desktop has no virtual-keyboard protocol. Copy the answer and paste it yourself.".into(),
            // No chord note here, and not because the chord works: nothing about the
            // hand-off works on this desktop, and a second sentence about a key
            // combination would only bury the one that matters.
            chord: String::new(),
        }
    }
}

/// Whether synthetic typing can work on this machine.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn availability() -> Availability {
    availability_from(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
    )
}

/// Whether synthetic typing can work on this machine.
///
/// Windows takes synthetic input through `SendInput` with no permission to ask for, so the
/// only honest answer here is yes.
#[cfg(target_os = "windows")]
pub fn availability() -> Availability {
    Availability {
        ok: true,
        display: "windows".into(),
        why: "Typing into another window goes through SendInput, which Windows allows.".into(),
        chord: String::new(),
    }
}

/// Whether synthetic typing can work on this machine.
///
/// macOS allows it only for an application the operator has granted Accessibility to, and
/// that grant cannot be read without prompting for it -- so this says yes and the first
/// attempt is what produces the system's own dialogue, which is the one place the operator
/// can actually grant it.
#[cfg(target_os = "macos")]
pub fn availability() -> Availability {
    Availability {
        ok: true,
        display: "macos".into(),
        why:
            "macOS will ask for Accessibility permission the first time, under Privacy & Security."
                .into(),
        chord: String::new(),
    }
}

/// What actually gets typed, given what the model wrote.
///
/// * CRLF becomes LF, because a synthesised Return is one keystroke and the carriage return
///   would be a second one -- a blank line in every box on Windows.
/// * Trailing whitespace goes, newlines included. This is property 2 at the top of the file:
///   the last line is left sitting in the box rather than submitted.
/// * `flatten` replaces the remaining line breaks with single spaces, for the boxes that
///   treat Return as send -- a chat client, a search field, most of a web form. It is a
///   choice in the Answer window rather than a guess here, because nothing in this process
///   can see what kind of box has the focus.
/// * The result is truncated at `MAX_CHARS` on a character boundary.
pub fn prepare(text: &str, flatten: bool) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.trim_end();
    let text = if flatten {
        text.split('\n')
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        text.to_string()
    };
    text.chars().take(MAX_CHARS).collect()
}

/// Lets go of every modifier the operator might be holding.
///
/// Releasing a key that was never pressed is a no-op everywhere this runs, so this is sent
/// unconditionally rather than tracked -- and an error is ignored for the same reason: a
/// platform that will not take the release of an unpressed key has nothing to report.
fn release_modifiers(enigo: &mut Enigo) {
    for key in [Key::Shift, Key::Control, Key::Alt, Key::Meta] {
        let _ = enigo.key(key, Direction::Release);
    }
}

/// Types `text` into whatever window currently has the focus.
///
/// Returns how many characters were typed, which is less than what came in when the answer
/// was over `MAX_CHARS` -- the caller says so to the operator rather than pretending the
/// whole thing arrived.
pub fn type_text(text: &str, flatten: bool) -> Result<usize, String> {
    let body = prepare(text, flatten);
    if body.is_empty() {
        return Err("There is nothing in that answer to type.".into());
    }

    let reach = availability();
    if !reach.ok {
        return Err(reach.why);
    }

    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| {
        format!("This machine would not give AURORA1 a way to type into other windows: {e}")
    })?;

    release_modifiers(&mut enigo);
    std::thread::sleep(SETTLE);

    enigo
        .text(&body)
        .map_err(|e| format!("The text could not be typed: {e}"))?;

    Ok(body.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_line_is_never_submitted() {
        // The whole of property 2: a shell, a chat box and most web forms act on Return, so
        // an answer that ends in one would send itself.
        assert_eq!(prepare("cargo test\n", false), "cargo test");
        assert_eq!(prepare("two\nlines\n\n\n", false), "two\nlines");
        assert_eq!(prepare("trailing spaces   ", false), "trailing spaces");
    }

    #[test]
    fn line_breaks_survive_unless_flattening_was_asked_for() {
        assert_eq!(prepare("one\ntwo", false), "one\ntwo");
        assert_eq!(prepare("one\ntwo", true), "one two");
        // Windows line endings are one Return, not two.
        assert_eq!(prepare("one\r\ntwo", false), "one\ntwo");
    }

    #[test]
    fn flattened_text_carries_no_newline_at_all() {
        /* The property the default rests on: with flattening on there is no Return
        anywhere in what gets typed, so nothing in the middle of an answer can submit
        itself. Driving this for real typed a three-paragraph answer into a terminal and
        ran every line but the last; this is the test that was owed to that. */
        let answer = "Here is the plan.\n\n1. Pull the branch.\n2. Run the tests.\n\nThat is all.";
        let typed = prepare(answer, true);
        assert!(
            !typed.contains('\n'),
            "{typed:?} still has a line break in it"
        );
        assert!(!typed.contains('\r'));
        assert_eq!(
            typed,
            "Here is the plan. 1. Pull the branch. 2. Run the tests. That is all."
        );
    }

    #[test]
    fn flattening_drops_the_blank_lines_rather_than_doubling_the_spaces() {
        // Markdown paragraphs are separated by a blank line, which flattened naively gives
        // "a  b" -- two spaces mid-sentence, in every answer.
        assert_eq!(prepare("a\n\nb", true), "a b");
        assert_eq!(prepare("  a  \n  b  ", true), "a b");
    }

    #[test]
    fn a_very_long_answer_is_cut_rather_than_refused() {
        let long = "x".repeat(MAX_CHARS + 500);
        assert_eq!(prepare(&long, false).chars().count(), MAX_CHARS);
        // On a character boundary: a multi-byte character must not be halved.
        let wide = "\u{00e9}".repeat(MAX_CHARS + 10);
        let cut = prepare(&wide, false);
        assert_eq!(cut.chars().count(), MAX_CHARS);
        assert!(cut.ends_with('\u{00e9}'));
    }

    #[test]
    fn nothing_to_type_is_an_error_not_an_empty_keystroke() {
        assert_eq!(prepare("   \n\n", false), "");
        assert!(type_text("   \n\n", false).is_err());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn x11_is_allowed_and_a_plain_wayland_desktop_is_not() {
        assert!(availability_from(Some("x11"), None, Some("KDE")).ok);
        // XWayland: an X11 session can have WAYLAND_DISPLAY set by the compositor that
        // started it, and it is still X11 that will take the keystrokes.
        assert!(availability_from(Some("x11"), Some("wayland-0"), Some("Hyprland")).ok);

        let refused = availability_from(Some("wayland"), Some("wayland-0"), Some("GNOME"));
        assert!(!refused.ok);
        assert_eq!(refused.display, "wayland");
        // The sentence is shown to the operator as it is, so it has to name the way out.
        assert!(refused.why.contains("paste"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn the_wlroots_compositors_are_the_wayland_exception() {
        for desktop in ["sway", "Hyprland", "river", "niri", "wlroots:wlr"] {
            let reach = availability_from(Some("wayland"), Some("wayland-0"), Some(desktop));
            assert!(
                reach.ok,
                "{desktop} implements the virtual-keyboard protocol"
            );
        }
        // A session with no XDG_SESSION_TYPE is read from WAYLAND_DISPLAY instead, which is
        // how a compositor started from a TTY presents itself.
        assert!(!availability_from(None, Some("wayland-1"), Some("GNOME")).ok);
        assert!(availability_from(None, None, None).ok);
    }

    /// The fault this field exists for: on Hyprland typing works, so the feature is offered,
    /// and the chord the UI shows beside it can never arrive. Both halves are asserted
    /// together because it is the combination that is the trap.
    #[test]
    fn wayland_says_the_chord_will_not_fire_even_where_typing_works() {
        let reach = availability_from(Some("wayland"), Some("wayland-0"), Some("Hyprland"));
        assert!(reach.ok, "typing works on Hyprland");
        assert!(
            reach.chord.contains("aether1 type-answer"),
            "the way out is to bind the compositor: {}",
            reach.chord
        );
        assert!(!reach.chord.contains('\n'), "one line for the UI");

        // X11 registers the chord itself, so there is nothing to say.
        assert!(availability_from(Some("x11"), None, Some("KDE"))
            .chord
            .is_empty());
    }
}
