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
    aether1 face                   Put the avatar fullscreen on a spare screen -- just the
                                   face and what it is doing (Esc closes it)
    aether1 --serve                Run headless as an HTTP/WebSocket server, reachable
                                   from this machine only
    aether1 --serve --lan          Also announce on the LAN (mDNS) and accept connections
                                   from your network, gated by a one-time pairing phrase
    aether1 pair                   Generate a new --lan pairing phrase, unpairing every
                                   device that paired with the old one
    aether1 devices                List the devices paired with this machine
    aether1 revoke <ID>            Take one device's access away, leaving the rest alone
    aether1 revoke all             Take every device's access away, keeping the phrase
    aether1 crashes                List the crashes AETHER1 has seen on this machine
    aether1 discover               List other AETHER1 instances announcing themselves on
                                   the LAN (default: listens 3 seconds, then stops)
    aether1 announce               Announce this machine on the LAN for testing `discover`
                                   without starting the full server (Ctrl+C to stop)

OPTIONS:
    prompt --session <ID>          Conversation to continue (default: \"default\",
                                   the same history the HUD shows)
    status --json                  Emit the raw telemetry JSON instead of a report
    say --voice <NAME>             Override the configured voice
    say --no-play                  Synthesize only; print the audio file path
    --serve --lan                  The pairing phrase is shown once, the first time you run
                                   this; run `aether1 pair` anytime for a new one. Without
                                   the matching token, a request over the network gets
                                   nothing -- no conversation, no tools, nothing to approve.
    discover --timeout <SECS>      How long to listen for (default: 3)
    announce --name <NAME>         Instance name other machines will see (default: this
                                   machine's hostname)
    announce --port <PORT>         Port to announce (default: 8378, --serve's own port)
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
    /// `devices`: what has paired with this machine over --lan.
    Devices,
    /// `revoke <id>`: one device's access, taken away without disturbing the others.
    /// `revoke all` takes every device's, leaving the phrase itself alone.
    Revoke {
        id: String,
    },
    /// `crashes`: what has died on this machine recently, asked for rather than announced.
    Crashes,
    /// `face`: the fullscreen avatar on a spare screen. Like `show`/`toggle` this reaches an
    /// already-running instance through the single-instance plugin rather than being handled
    /// in cli::run -- the face is a mirror of the HUD's own avatar (see frontend/js/face.js),
    /// so there has to be a HUD for it to mirror. With no instance running, this launch
    /// becomes one and opens the face alongside it.
    Face,
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
        /// `--events`: also sweep the system log for recent errors. Off by default, and
        /// deliberately so -- see watchers/events.rs for why noise is opt-in.
        events: bool,
    },
    Say {
        text: Option<String>,
        voice: Option<String>,
        play: bool,
    },
    Discover {
        timeout_secs: u64,
    },
    Announce {
        name: Option<String>,
        port: Option<u16>,
    },
    Pair,
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

fn parse_discover(rest: Vec<String>) -> Result<Invocation, String> {
    let (timeout, rest) = take_option(&rest, "--timeout")?;
    let timeout_secs = match timeout {
        Some(t) => t
            .parse::<u64>()
            .map_err(|_| format!("--timeout expects a whole number of seconds (got {t:?})"))?,
        None => 3,
    };
    match free_text(rest)? {
        Some(extra) => Err(format!("discover takes no arguments (got {extra:?})")),
        None => Ok(Invocation::Discover { timeout_secs }),
    }
}

fn parse_announce(rest: Vec<String>) -> Result<Invocation, String> {
    let (name, rest) = take_option(&rest, "--name")?;
    let (port, rest) = take_option(&rest, "--port")?;
    let port = match port {
        Some(p) => Some(
            p.parse::<u16>()
                .map_err(|_| format!("--port expects a number from 0-65535 (got {p:?})"))?,
        ),
        None => None,
    };
    match free_text(rest)? {
        Some(extra) => Err(format!("announce takes no arguments (got {extra:?})")),
        None => Ok(Invocation::Announce { name, port }),
    }
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
            let (events, rest) = take_flag(&rest, "--events");
            free_text(rest).and_then(|extra| match extra {
                Some(extra) => Err(format!("status takes no arguments (got {extra:?})")),
                None => Ok(Invocation::Status { json, events }),
            })
        }
        "crashes" => free_text(rest).and_then(|extra| match extra {
            Some(extra) => Err(format!("crashes takes no arguments (got {extra:?})")),
            None => Ok(Invocation::Crashes),
        }),
        "say" => take_option(&rest, "--voice").and_then(|(voice, rest)| {
            let (no_play, rest) = take_flag(&rest, "--no-play");
            free_text(rest).map(|text| Invocation::Say {
                text,
                voice,
                play: !no_play,
            })
        }),
        "discover" => parse_discover(rest),
        "announce" => parse_announce(rest),
        "pair" => free_text(rest).and_then(|extra| match extra {
            Some(extra) => Err(format!("pair takes no arguments (got {extra:?})")),
            None => Ok(Invocation::Pair),
        }),
        "devices" => free_text(rest).and_then(|extra| match extra {
            Some(extra) => Err(format!("devices takes no arguments (got {extra:?})")),
            None => Ok(Invocation::Devices),
        }),
        "revoke" => free_text(rest).and_then(|id| match id {
            Some(id) => Ok(Invocation::Revoke { id }),
            None => Err(
                "revoke needs the id of a device, or `all` -- run `aether1 devices` to see them"
                    .to_string(),
            ),
        }),
        "face" => free_text(rest).and_then(|extra| match extra {
            Some(extra) => Err(format!("face takes no arguments (got {extra:?})")),
            None => Ok(Invocation::Face),
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

fn run_status(json: bool, events: bool) -> String {
    let telemetry = Telemetry::snapshot();
    if json {
        return telemetry.to_wire_json().to_string();
    }
    let report = telemetry.diagnostic_report();
    if !events {
        return report;
    }
    // Step 13's rule, kept honest at the command line: the wider sweep of the event log is
    // something you ask for. It is appended to the report rather than replacing it, since
    // "what is this machine doing" and "what has gone wrong on it" are read together.
    format!("{report}\n\n{}", crate::watchers::events::report())
}

/// `aether1 crashes`. Deliberately shows muted programs too, marked -- the mute list stops
/// AETHER1 interrupting you, and this is you doing the asking.
fn run_crashes() -> Result<String, String> {
    use crate::watchers::crash::{self, Availability};

    let reader = crash::reader_for_this_machine();
    if let Availability::Unavailable(reason) = reader.availability() {
        return Err(reason);
    }

    // A week: long enough that a crash from Friday is still findable on Monday, short
    // enough that the list is a list rather than a history.
    let week = 7 * 24 * 60 * 60;
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
        .saturating_sub(week);

    let crashes = reader.crashes_since(since)?;
    if crashes.is_empty() {
        return Ok("Nothing has crashed on this machine in the past week.".to_string());
    }

    let engine = crate::build_llm_engine();
    let muted = crash::muted_programs(engine.db().get_setting(crash::MUTED_SETTING).ok().flatten());

    let mut out = format!(
        "{} crash{} in the past week, oldest first:\n",
        crashes.len(),
        if crashes.len() == 1 { "" } else { "es" }
    );
    for crash in &crashes {
        let ago = how_long_ago(since_then(crash.at));
        let muted_note = if crash::is_muted(&crash.program, &muted) {
            "  (muted)"
        } else {
            ""
        };
        out.push_str(&format!("\n    {} -- {ago}{muted_note}", crash.headline()));
    }
    out.push_str("\n\nRun `aether1 status --events` for the wider sweep of the system log.");
    Ok(out)
}

/// Seconds between a unix timestamp and now, floored at zero so a clock that has moved
/// backwards reports "just now" rather than a wild number.
fn since_then(at: u64) -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
        .saturating_sub(at)
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

fn run_discover(timeout_secs: u64) -> Result<String, String> {
    let peers = crate::discovery::discover(std::time::Duration::from_secs(timeout_secs))?;
    if peers.is_empty() {
        return Ok(format!(
            "No other AETHER1 instances answered within {timeout_secs}s. \
             (Nothing is on the LAN to find until another machine runs `aether1 announce`.)"
        ));
    }
    let mut lines = vec![format!("Found {} instance(s):", peers.len())];
    for peer in peers {
        let addresses = peer
            .addresses
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!(
            "  {} -- {}:{} ({addresses})",
            peer.instance_name, peer.host, peer.port
        ));
        for (key, value) in &peer.properties {
            lines.push(format!("      {key} = {value}"));
        }
    }
    Ok(lines.join("\n"))
}

fn run_announce(name: Option<String>, port: Option<u16>) -> Result<String, String> {
    let name = name
        .or_else(sysinfo::System::host_name)
        .unwrap_or_else(|| "aether1".to_string());
    let port = port.unwrap_or(8378);
    println!(
        "Announcing this machine as {name:?} on port {port} ({}) -- Ctrl+C to stop.",
        crate::discovery::SERVICE_TYPE
    );
    let version = crate::APP_VERSION;
    let _announcement = crate::discovery::announce(&name, port, &[("version", version)])?;
    // Nothing else to do -- the mDNS daemon answers queries on its own background thread
    // for as long as `_announcement` is alive, which is until this process exits.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

fn run_pair() -> Result<String, String> {
    let (phrase, unpaired) = crate::serve_auth::rotate()?;
    let devices = match unpaired {
        0 => String::new(),
        1 => "One device was unpaired and will have to pair again. ".to_string(),
        many => format!("{many} devices were unpaired and will have to pair again. "),
    };
    Ok(format!(
        "New --lan pairing phrase (shown once -- write it down now):\n\n    {phrase}\n\n\
         {devices}Any phrase paired before this no longer works. Type this one into another \
         AETHER1 instance's pairing prompt, or POST it as {{\"phrase\": ...}} to /api/pair, to \
         let it reach this machine.\n\n\
         To cut off one machine rather than all of them, use `aether1 devices` and \
         `aether1 revoke <id>` instead."
    ))
}

/// How long ago, in the roughest units that still mean something. A device list is read to
/// answer "which one is the laptop I lent out", and an exact timestamp helps with that less
/// than "yesterday" does.
fn how_long_ago(seconds: u64) -> String {
    match seconds {
        0..=90 => "just now".to_string(),
        91..=5399 => format!("{} minutes ago", seconds / 60),
        5400..=86_399 => format!("{} hours ago", seconds / 3600),
        _ => format!("{} days ago", seconds / 86_400),
    }
}

fn run_devices() -> Result<String, String> {
    let auth = crate::serve_auth::open_devices()?;
    let devices = auth.list_devices();
    if devices.is_empty() {
        return Ok("No devices are paired with this machine.".to_string());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default();
    let mut out = format!(
        "{} device{} paired with this machine:\n",
        devices.len(),
        if devices.len() == 1 { "" } else { "s" }
    );
    for device in devices {
        let ago = how_long_ago(now.saturating_sub(device.paired_at));
        out.push_str(&format!(
            "\n    {}  {}  (paired {ago})",
            device.id, device.label
        ));
    }
    out.push_str(
        "\n\nRun `aether1 revoke <id>` to take one device's access away, or \
         `aether1 revoke all` to take every one.",
    );
    Ok(out)
}

fn run_revoke(id: &str) -> Result<String, String> {
    let auth = crate::serve_auth::open_devices()?;
    // A device id is eight hex characters, so "all" can never be one of them and is safe to
    // read as the word. It is the lost-the-laptop door: everything re-pairs, but the phrase
    // you have written down stays the phrase. Rotating is the harsher `aether1 pair`.
    if id == "all" {
        let count = auth.revoke_all_devices()?;
        return Ok(match count {
            0 => "Nothing was paired, so nothing was revoked.".to_string(),
            1 => "Revoked the one paired device. The phrase still works, so it can pair again."
                .to_string(),
            _ => format!(
                "Revoked all {count} paired devices. The phrase is unchanged, so each one can \
                 pair again with it."
            ),
        });
    }
    match auth.revoke_device(id)? {
        Some(label) => Ok(format!(
            "Revoked {id} ({label}). Every other paired device still works; that one has to \
             pair again with the phrase."
        )),
        None => Err(format!(
            "no device with the id {id} -- run `aether1 devices` to see what is paired"
        )),
    }
}

/// Runs a headless invocation and returns the process exit code. `App`, `Serve`, `Window`
/// and `Face` are handled by main() and are a no-op here.
pub fn run(invocation: Invocation) -> i32 {
    let result = match invocation {
        Invocation::App
        | Invocation::Serve { .. }
        | Invocation::Window { .. }
        | Invocation::Face => Ok(String::new()),
        Invocation::Help => Ok(USAGE.trim_end().to_string()),
        Invocation::Version => Ok(format!(
            "{} ({})",
            crate::APP_VERSION,
            crate::short_hash(crate::BUILT_COMMIT)
        )),
        Invocation::Prompt { text, session } => run_prompt(text, session),
        Invocation::Status { json, events } => Ok(run_status(json, events)),
        Invocation::Crashes => run_crashes(),
        Invocation::Say { text, voice, play } => run_say(text, voice, play),
        Invocation::Discover { timeout_secs } => run_discover(timeout_secs),
        Invocation::Announce { name, port } => run_announce(name, port),
        Invocation::Pair => run_pair(),
        Invocation::Devices => run_devices(),
        Invocation::Revoke { id } => run_revoke(&id),
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
            Invocation::Status {
                json: true,
                events: false
            }
        );
        assert_eq!(
            parse_args(&["status", "--events"]),
            Invocation::Status {
                json: false,
                events: true
            },
            "the wider sweep of the event log is asked for, never volunteered"
        );
        assert_eq!(parse_args(&["crashes"]), Invocation::Crashes);
        assert!(matches!(
            parse_args(&["crashes", "please"]),
            Invocation::Invalid(_)
        ));
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
    fn face_is_recognized_and_takes_nothing() {
        assert_eq!(parse_args(&["face"]), Invocation::Face);
        // Not folded into free text like `prompt`/`say`: there is nothing for the face to
        // do with a word, so a stray one is a typo worth reporting.
        assert!(matches!(
            parse_args(&["face", "please"]),
            Invocation::Invalid(_)
        ));
    }

    #[test]
    fn face_is_a_window_request_not_a_headless_one() {
        // run() is the headless path, and `face` is not headless -- main() sends it to the
        // desktop path alongside App and Window. This pins the half of that arrangement
        // that is testable: run() treats it as a no-op rather than erroring, so if the
        // routing in main() is ever changed the failure is a window that does not open,
        // not a spurious "aether1: ..." on stderr from a command that was never meant to
        // arrive here.
        assert_eq!(run(Invocation::Face), 0);
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

    #[test]
    fn revoke_takes_an_id_or_the_word_all() {
        assert_eq!(
            parse_args(&["revoke", "a1b2c3d4"]),
            Invocation::Revoke {
                id: "a1b2c3d4".to_string()
            }
        );
        assert_eq!(
            parse_args(&["revoke", "all"]),
            Invocation::Revoke {
                id: "all".to_string()
            }
        );
        // Naming nothing is the dangerous reading -- it must not quietly mean "all".
        assert!(matches!(parse_args(&["revoke"]), Invocation::Invalid(_)));
    }
}
