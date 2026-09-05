// Headless command-line entry points: `aether1 prompt`, `aether1 status`, `aether1 say`.
//
// The point of these is reachability -- the companion should be usable from a terminal, a
// script, or a keybinding without opening the HUD first. They're deliberately thin: each
// one builds the same engine main() would (build_llm_engine) and calls the same
// commands::* function the Tauri command and the axum route both call, so a CLI invocation
// and a chat message go down one code path and share one conversation history.
//
// Parsing is hand-rolled rather than pulling in a CLI crate: there are three subcommands
// and a handful of flags, and main() already had to sniff argv for --serve before
// tauri::Builder is constructed.

use std::io::{IsTerminal, Read};
use std::path::Path;

use crate::commands;
use crate::llm::{Telemetry, DEFAULT_VOICE};

/// mp3 players tried in order, with the flags that make each one exit after playing
/// without opening a window. `paplay`/`aplay` are deliberately absent: generate_speech
/// writes mp3 (see llm/tts.rs) and neither decodes it.
const PLAYERS: &[(&str, &[&str])] = &[
    ("mpv", &["--no-video", "--really-quiet"]),
    ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
    ("mpg123", &["-q"]),
    ("cvlc", &["--intf", "dummy", "--play-and-exit"]),
    ("afplay", &[]),
];

const USAGE: &str = "\
AETHER1 -- cybernetic AI companion

USAGE:
    aether1                        Launch the desktop app (tray + HUD)
    aether1 prompt [TEXT]          Ask the companion; prints the reply to stdout
    aether1 status                 Print a system diagnostic report
    aether1 say [TEXT]             Speak text in the companion's voice
    aether1 show | toggle          Summon (or dismiss) the HUD of a running instance
    aether1 --serve                Run headless as an HTTP/WebSocket server, reachable
                                   from this machine only

OPTIONS:
    prompt --session <ID>          Conversation to continue (default: \"default\",
                                   the same history the HUD shows)
    status --json                  Emit the raw telemetry JSON instead of a report
    say --voice <NAME>             Override the configured voice
    say --no-play                  Synthesize only; print the audio file path
    --serve --lan                  Also accept connections from your network. There is no
                                   password: anyone who can reach this machine can read
                                   your conversation and approve pending actions.
    -h, --help                     Show this help
    -V, --version                  Show the version

TEXT may be omitted for `prompt` and `say` when it is piped in on stdin:
    echo \"what is eating my RAM\" | aether1 prompt

The HUD toggle is also bound to a global hotkey (Super+Shift+A by default, changeable in
Settings). On Wayland, where an application can't grab keys system-wide, bind your
compositor's keybinding to `aether1 toggle` instead.
";

#[derive(Debug, PartialEq, Eq)]
pub enum Invocation {
    /// No recognized subcommand: launch the desktop app, as before.
    App,
    /// `show` / `toggle`: reaches an already-running instance through the single-instance
    /// plugin, which is why this goes down the same path as App rather than being handled
    /// here -- with no instance running, it simply starts one.
    Window {
        toggle: bool,
    },
    /// `lan` is the opt-in from `--serve --lan`: bind every interface instead of
    /// the loopback address, so other machines can reach the HUD.
    Serve {
        lan: bool,
    },
    Prompt {
        text: Option<String>,
        session: Option<String>,
    },
    Status {
        json: bool,
    },
    Say {
        text: Option<String>,
        voice: Option<String>,
        play: bool,
    },
    Help,
    Version,
    /// A recognized-shape invocation that can't be run: message printed to stderr,
    /// followed by the usage text, exit code 2.
    Invalid(String),
}

/// Pulls `--name <value>` out of `args`, returning the value and everything else. A
/// trailing `--name` with no value yields Err.
fn take_option(args: &[String], name: &str) -> Result<(Option<String>, Vec<String>), String> {
    let mut value = None;
    let mut rest = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == name {
            match iter.next() {
                Some(v) => value = Some(v.clone()),
                None => return Err(format!("{name} needs a value")),
            }
        } else {
            rest.push(arg.clone());
        }
    }
    Ok((value, rest))
}

fn take_flag(args: &[String], name: &str) -> (bool, Vec<String>) {
    let present = args.iter().any(|a| a == name);
    let rest = args.iter().filter(|a| *a != name).cloned().collect();
    (present, rest)
}

/// Joins the leftover words into the free-text argument, rejecting anything that still
/// looks like a flag -- inside a subcommand an unrecognized `--foo` is a mistake worth
/// reporting, not something to silently fold into the prompt text.
fn free_text(rest: Vec<String>) -> Result<Option<String>, String> {
    if let Some(unknown) = rest.iter().find(|a| a.starts_with("--")) {
        return Err(format!("unknown option {unknown:?}"));
    }
    let joined = rest.join(" ").trim().to_string();
    Ok(if joined.is_empty() {
        None
    } else {
        Some(joined)
    })
}

pub fn parse(argv: &[String]) -> Invocation {
    let args: Vec<String> = argv.iter().skip(1).cloned().collect();

    // Checked before anything else so the existing `--serve` behavior is unchanged: it
    // wins wherever it appears in argv.
    if args.iter().any(|a| a == "--serve") {
        return Invocation::Serve {
            lan: args.iter().any(|a| a == "--lan"),
        };
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Invocation::Help;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        return Invocation::Version;
    }

    // No bare word anywhere means no subcommand was given. Unrecognized *flags* are
    // tolerated here rather than rejected: a desktop launcher, a webview, or a display
    // server can append its own, and refusing to start the GUI over one would be worse
    // than ignoring it.
    let Some(pos) = args.iter().position(|a| !a.starts_with('-')) else {
        return Invocation::App;
    };
    let (command, rest) = (args[pos].clone(), args[pos + 1..].to_vec());

    let parsed = match command.as_str() {
        "prompt" => take_option(&rest, "--session").and_then(|(session, rest)| {
            free_text(rest).map(|text| Invocation::Prompt { text, session })
        }),
        "status" => {
            let (json, rest) = take_flag(&rest, "--json");
            free_text(rest).and_then(|extra| match extra {
                Some(extra) => Err(format!("status takes no arguments (got {extra:?})")),
                None => Ok(Invocation::Status { json }),
            })
        }
        "say" => take_option(&rest, "--voice").and_then(|(voice, rest)| {
            let (no_play, rest) = take_flag(&rest, "--no-play");
            free_text(rest).map(|text| Invocation::Say {
                text,
                voice,
                play: !no_play,
            })
        }),
        "show" | "toggle" => free_text(rest.clone()).and_then(|extra| match extra {
            Some(extra) => Err(format!("{command} takes no arguments (got {extra:?})")),
            None => Ok(Invocation::Window {
                toggle: command == "toggle",
            }),
        }),
        other => Err(format!("unknown command {other:?}")),
    };

    parsed.unwrap_or_else(Invocation::Invalid)
}

/// Free text from the command line, or stdin when it's a pipe and no text was given.
/// A terminal stdin is never read -- `aether1 prompt` with nothing to say should print
/// usage, not hang waiting for input.
fn text_or_stdin(text: Option<String>, what: &str) -> Result<String, String> {
    if let Some(text) = text {
        return Ok(text);
    }
    if std::io::stdin().is_terminal() {
        return Err(format!(
            "nothing to {what}: pass text or pipe it in on stdin"
        ));
    }
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|e| format!("could not read stdin: {e}"))?;
    let buffer = buffer.trim().to_string();
    if buffer.is_empty() {
        Err(format!("nothing to {what}: stdin was empty"))
    } else {
        Ok(buffer)
    }
}

fn play_audio(path: &Path) -> Result<(), String> {
    for (player, flags) in PLAYERS {
        let Ok(binary) = which::which(player) else {
            continue;
        };
        let status = std::process::Command::new(binary)
            .args(*flags)
            .arg(path)
            .status()
            .map_err(|e| format!("could not run {player}: {e}"))?;
        return if status.success() {
            Ok(())
        } else {
            Err(format!("{player} exited with {status}"))
        };
    }
    Err(format!(
        "no audio player found (tried {}); the file is at {}",
        PLAYERS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", "),
        path.display()
    ))
}

fn run_prompt(text: Option<String>, session: Option<String>) -> Result<String, String> {
    let text = text_or_stdin(text, "ask")?;
    let engine = crate::build_llm_engine();
    let response = commands::generate_response(&engine, text, session)?;
    Ok(response
        .get("reply")
        .and_then(|r| r.as_str())
        .unwrap_or_default()
        .to_string())
}

fn run_status(json: bool) -> String {
    let telemetry = Telemetry::snapshot();
    if json {
        telemetry.to_wire_json().to_string()
    } else {
        telemetry.diagnostic_report()
    }
}

fn run_say(text: Option<String>, voice: Option<String>, play: bool) -> Result<String, String> {
    let text = text_or_stdin(text, "say")?;
    // Engine and voice come from settings, so the CLI sounds like whatever the HUD sounds
    // like -- including using the local engine when one is installed.
    let engine = crate::build_llm_engine();
    let voice = voice.or_else(|| Some(engine.db().get_setting_string("voice_name", DEFAULT_VOICE)));
    let path = commands::synthesize_speech(&engine, &text, voice.as_deref())?;
    if play {
        play_audio(&path)?;
        Ok(String::new())
    } else {
        Ok(path.display().to_string())
    }
}

/// Runs a headless invocation and returns the process exit code. `App` and `Serve` are
/// handled by main() and are a no-op here.
pub fn run(invocation: Invocation) -> i32 {
    let result = match invocation {
        Invocation::App | Invocation::Serve { .. } | Invocation::Window { .. } => Ok(String::new()),
        Invocation::Help => Ok(USAGE.trim_end().to_string()),
        Invocation::Version => Ok(format!(
            "{} ({})",
            crate::APP_VERSION,
            crate::short_hash(crate::BUILT_COMMIT)
        )),
        Invocation::Prompt { text, session } => run_prompt(text, session),
        Invocation::Status { json } => Ok(run_status(json)),
        Invocation::Say { text, voice, play } => run_say(text, voice, play),
        Invocation::Invalid(message) => {
            eprintln!("aether1: {message}\n\n{}", USAGE.trim_end());
            return 2;
        }
    };

    match result {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            0
        }
        Err(message) => {
            eprintln!("aether1: {message}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Invocation {
        let argv: Vec<String> = std::iter::once("aether1")
            .chain(args.iter().copied())
            .map(String::from)
            .collect();
        parse(&argv)
    }

    #[test]
    fn no_arguments_launches_the_app() {
        assert_eq!(parse_args(&[]), Invocation::App);
    }

    #[test]
    fn unknown_flags_alone_still_launch_the_app() {
        // A launcher or display server can append flags of its own; refusing to start the
        // GUI over one would be worse than ignoring it.
        assert_eq!(parse_args(&["--some-webview-flag"]), Invocation::App);
    }

    #[test]
    fn serve_wins_wherever_it_appears() {
        assert_eq!(parse_args(&["--serve"]), Invocation::Serve { lan: false });
        assert_eq!(
            parse_args(&["--serve", "--lan"]),
            Invocation::Serve { lan: true }
        );
        assert_eq!(
            parse_args(&["--lan", "--serve"]),
            Invocation::Serve { lan: true }
        );
        // --lan is meaningless on its own, and must not be what turns a plain launch
        // into a server: opening a port is never a side effect of an unrelated flag.
        assert_eq!(parse_args(&["--lan"]), Invocation::App);
        assert_eq!(
            parse_args(&["prompt", "hi", "--serve"]),
            Invocation::Serve { lan: false }
        );
    }

    #[test]
    fn prompt_joins_its_words() {
        assert_eq!(
            parse_args(&["prompt", "what", "is", "my", "cpu", "doing"]),
            Invocation::Prompt {
                text: Some("what is my cpu doing".to_string()),
                session: None,
            }
        );
    }

    #[test]
    fn prompt_takes_a_session() {
        assert_eq!(
            parse_args(&["prompt", "--session", "scratch", "hello"]),
            Invocation::Prompt {
                text: Some("hello".to_string()),
                session: Some("scratch".to_string()),
            }
        );
    }

    #[test]
    fn prompt_without_text_is_left_to_stdin() {
        assert_eq!(
            parse_args(&["prompt"]),
            Invocation::Prompt {
                text: None,
                session: None,
            }
        );
    }

    #[test]
    fn say_defaults_to_playing() {
        assert_eq!(
            parse_args(&["say", "systems", "nominal"]),
            Invocation::Say {
                text: Some("systems nominal".to_string()),
                voice: None,
                play: true,
            }
        );
    }

    #[test]
    fn say_no_play_and_voice_are_stripped_from_the_text() {
        assert_eq!(
            parse_args(&["say", "--no-play", "--voice", "en-GB-SoniaNeural", "hello"]),
            Invocation::Say {
                text: Some("hello".to_string()),
                voice: Some("en-GB-SoniaNeural".to_string()),
                play: false,
            }
        );
    }

    #[test]
    fn status_rejects_stray_arguments() {
        assert_eq!(
            parse_args(&["status", "--json"]),
            Invocation::Status { json: true }
        );
        assert!(matches!(
            parse_args(&["status", "please"]),
            Invocation::Invalid(_)
        ));
    }

    #[test]
    fn show_and_toggle_are_recognized() {
        assert_eq!(parse_args(&["show"]), Invocation::Window { toggle: false });
        assert_eq!(parse_args(&["toggle"]), Invocation::Window { toggle: true });
        assert!(matches!(
            parse_args(&["toggle", "now"]),
            Invocation::Invalid(_)
        ));
    }

    #[test]
    fn a_mistyped_command_is_an_error_not_a_silent_gui_launch() {
        assert!(matches!(
            parse_args(&["promt", "hi"]),
            Invocation::Invalid(_)
        ));
    }

    #[test]
    fn an_option_missing_its_value_is_an_error() {
        assert!(matches!(
            parse_args(&["prompt", "hi", "--session"]),
            Invocation::Invalid(_)
        ));
    }

    #[test]
    fn an_unknown_option_inside_a_subcommand_is_an_error() {
        assert!(matches!(
            parse_args(&["prompt", "--wat", "hi"]),
            Invocation::Invalid(_)
        ));
    }

    #[test]
    fn help_and_version_win_over_a_subcommand() {
        assert_eq!(parse_args(&["prompt", "hi", "--help"]), Invocation::Help);
        assert_eq!(parse_args(&["--version"]), Invocation::Version);
    }
}
