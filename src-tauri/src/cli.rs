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
    aether1 doctor                 Check AETHER1 itself -- every part of it, and what is
                                   wrong with the ones that are not working
    aether1 doctor --fix           Also offer the repairs it knows how to make, asking
                                   before each one
    aether1 models                 Show which local model each speciality runs on
    aether1 models <NAME> <MODEL>  Point one speciality at a model (`clear` to unset)
    aether1 flow                   Show whether the avatar follows the question (STATIC or
                                   FLOW), and which line it can move within
    aether1 flow on|off            Turn that on or off
    aether1 flow line <NAME>       Pick a whole line instead of one avatar, so the question
                                   goes to whichever of its nodes owns it (`clear` to release)
    aether1 words                  Show how this machine says the words the voice gets wrong
    aether1 words <WORD> <SAID>    Say one word differently (the rest of the line is how it
                                   sounds, so quotes are optional)
    aether1 words drop <WORD>      Take one off the list
    aether1 signin                 Sign in to GitHub, so this copy can see new releases.
                                   Shows a code to type in at github.com; nothing else
    aether1 signout                Forget that sign-in
    aether1 update                 What version is out, and download it when one is
    aether1 verify <FILE>          Check a bundle's signature against the key built into
                                   this copy -- for one that arrived by hand
    aether1 installs               List every copy of AETHER1 this machine has on it
    aether1 installs remove <ID>   Remove one of them, by the id the list prints
    aether1 installs remove old    Remove every copy older than the one you are running
    aether1 code                   What this machine still needs before it can write code
                                   offline, and the commands to set it up
    aether1 code ask <question>    Ask the coding model something, in the same
                                   conversation the HUD's Aether Code tab keeps
    aether1 code conventions       Print the house rules for a coding model to follow;
                                   redirect it into an AGENTS.md at the top of your project
    aether1 doctor --heal          Make every repair AETHER1 can make to itself, without
                                   asking about each one. Never anything needing root.
    aether1 code perms             What AETHER CODE is allowed to do: read the system,
                                   the GitHub CLI, the internet, and -- off until you say
                                   otherwise -- edit and run inside one project folder
    aether1 code perms <n> on|off  Turn one of those on or off
    aether1 code workspace [path]  The project folder it may change; prints the current one
    aether1 code run-network on|off   Let commands reach the network from inside the sandbox
    aether1 code run-unconfined on|off  Run commands on a machine with no sandbox anyway
    aether1 code run-allow <prog>  Let it run that program in the project folder
    aether1 code run-deny <prog>   Take that program back off the list
    aether1 discover               List other AETHER1 instances announcing themselves on
                                   the LAN (default: listens 3 seconds, then stops)
    aether1 announce               Announce this machine on the LAN for testing `discover`
                                   without starting the full server (Ctrl+C to stop)

OPTIONS:
    prompt --session <ID>          Conversation to continue (default: \"default\",
                                   the same history the HUD shows)
    status --json                  Emit the raw telemetry JSON instead of a report
    doctor --fix                   Offer each repair in turn. Nothing is changed without a
                                   typed y, and a repair that needs root is never run by
                                   AETHER1 at all -- it is handed to you as the command
    doctor --json                  The recorded observation and the verdicts, as JSON
    doctor --report [FILE]         Write the observation and the verdicts where you can read
                                   them and paste them into a bug report. Sends nothing
    doctor --replay <FILE>         Judge an observation recorded on another machine, which is
                                   how a fault seen once becomes a test
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
    /// `doctor`: whether AETHER1 itself is working, and the repairs it is allowed to make to
    /// the machine underneath it. A verb of its own rather than another `status` flag,
    /// because `status` is about the computer and this is about the app -- conflating them is
    /// what left the app with no way to say it was broken. See doctor.rs.
    Doctor {
        /// Offer the repairs. Each one is still asked about individually at the moment of
        /// acting; this flag only decides whether they are offered at all.
        fix: bool,
        /// Make every repair AETHER1 can make, without asking about each one. Typing this
        /// is the go-ahead for the pass -- see `doctor::attend`.
        heal: bool,
        json: bool,
        /// Where to write the pasteable report. `Some(None)` is --report with no path, which
        /// picks one under the temp directory and prints it.
        report: Option<Option<String>>,
        /// An observation recorded elsewhere, judged instead of this machine.
        replay: Option<String>,
    },
    /// `code`: the coding wizard, printed. `code conventions` prints the house rules
    /// instead, so they can be redirected into a project's AGENTS.md in one line rather
    /// than copied out of a panel.
    Code {
        conventions: bool,
        /// A question for the coding model, when there is one. The same conversation the
        /// HUD's Aether Code tab keeps, so a question asked here is remembered there.
        ask: Option<String>,
    },
    /// `code perms`: what AETHER CODE is allowed to look at, and with two arguments, the
    /// setting of one. See code_perms.rs for why there is no write permission to set.
    CodePerms {
        /// Which one, when the operator is changing it rather than reading the list.
        grant: Option<String>,
        on: Option<bool>,
    },
    /// `code workspace`: the project folder AETHER CODE may change, printed or set.
    CodeWorkspace {
        path: Option<String>,
    },
    /// `code run-network` / `code run-unconfined`: the two switches around the sandbox
    /// commands are spawned in. Printed with no argument, set with `on`/`off`.
    CodeSandbox {
        /// Which switch: the network inside the box, or running with no box at all.
        unconfined: bool,
        on: Option<bool>,
    },
    /// `code run-allow`: the programs it may run inside that folder, printed or added to.
    CodeRunAllow {
        program: Option<String>,
        /// True for `run-allow <p>`, false for `run-deny <p>`.
        add: bool,
    },
    /// `models`: which local model each speciality runs on, and with two arguments, the
    /// setting of one. See llm/routing.rs for why the key is the speciality.
    Models {
        persona: Option<String>,
        model: Option<String>,
    },
    /// `flow`: STATIC keeps the avatar the operator picked on every question; FLOW lets the
    /// specialist inside that avatar's own line take the ones that are its own. See
    /// llm/flow.rs for the rule that decides, and why a missed hand-off is the cheap
    /// mistake and a wrong one is not.
    Flow {
        state: Option<String>,
        /// `flow line <NAME>`: pick a whole cast rather than one of its members, so the
        /// question goes to whichever node of that line owns it. `clear` releases it.
        line: Option<String>,
    },
    /// `words`: the pronunciation list. With no arguments it prints it; with a word and a
    /// respelling it adds or replaces one; `drop <WORD>` removes one.
    Words {
        word: Option<String>,
        said_as: Option<String>,
        drop: bool,
    },
    /// `signin`: the GitHub device flow, in a terminal. Prints the code, waits for it to be
    /// approved in a browser, and keeps the token in the OS keychain. See github_auth.rs for
    /// why the app signs the operator in rather than carrying a credential of its own.
    SignIn,
    /// `signout`: forget the token, both places it could be.
    SignOut,
    /// `update`: what is published against what is running, and -- with `download` -- fetching
    /// the signed bundle. Never installs it: see releases.rs.
    Update {
        download: bool,
    },
    /// `verify <file>`: a bundle that arrived some other way -- a USB stick, a mirror, a
    /// download that was resumed by hand -- checked against the compiled-in public key. The
    /// second question releases.rs exists to ask: authentication proves who may download, a
    /// signature proves what was downloaded.
    Verify {
        path: String,
    },
    /// `installs`: every copy of AETHER1 on this machine, and taking the stale ones away.
    /// `remove` is None for a plain listing, or the id of one copy -- or the word `old`,
    /// meaning every copy that is behind the one running.
    Installs {
        remove: Option<String>,
    },
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
    /// `diagnostics` / `health check`: comprehensive system diagnostics including telemetry
    /// and error-level event logs. Always includes both sections, unlike status which makes
    /// the event log opt-in.
    Diagnostics,
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

/// `doctor`, and the four ways of asking it. Hand-rolled like the rest: `--replay` takes a
/// path, `--report` takes an optional one, and the two flags that take nothing are order-free.
fn parse_doctor(rest: Vec<String>) -> Result<Invocation, String> {
    let mut fix = false;
    let mut heal = false;
    let mut json = false;
    let mut report: Option<Option<String>> = None;
    let mut replay: Option<String> = None;
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--fix" => fix = true,
            "--heal" => heal = true,
            "--json" => json = true,
            "--replay" => match iter.next() {
                Some(path) => replay = Some(path.clone()),
                None => return Err("--replay needs the path of a recorded observation".into()),
            },
            // The path is optional, and the next argument is only the path when it is not
            // another flag -- `doctor --report --fix` must not write to a file called --fix.
            "--report" => {
                let mut peeked = iter.clone();
                match peeked.next() {
                    Some(path) if !path.starts_with("--") => {
                        report = Some(Some(path.clone()));
                        iter.next();
                    }
                    _ => report = Some(None),
                }
            }
            other => {
                return Err(format!(
                    "doctor does not take {other:?} -- it takes --fix, --json, --report [FILE] \
                     --heal, --json, --report [FILE] or --replay <FILE>"
                ))
            }
        }
    }
    if heal && fix {
        // Two different things: --fix asks about each repair, --heal makes them all. Asked
        // for together, which one the operator meant is genuinely unclear, and guessing
        // wrong means either a prompt they did not want or a repair they did not approve.
        return Err(
            "--fix asks about each repair and --heal makes them all, so they cannot both be \
             meant -- pick one"
                .into(),
        );
    }
    if replay.is_some() && (fix || heal) {
        // The observation came from another machine. Repairing this one from it would be
        // acting on a fault nobody here has.
        return Err(
            "--replay judges an observation from somewhere else, so there is nothing here to \
             fix -- run it on the machine that recorded it"
                .into(),
        );
    }
    Ok(Invocation::Doctor {
        fix,
        heal,
        json,
        report,
        replay,
    })
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
        "diagnostics" | "health" | "health check" => {
            free_text(rest).and_then(|extra| match extra {
                Some(extra) => Err(format!("diagnostics takes no arguments (got {extra:?})")),
                None => Ok(Invocation::Diagnostics),
            })
        }
        "doctor" => parse_doctor(rest),
        "crashes" => free_text(rest).and_then(|extra| match extra {
            Some(extra) => Err(format!("crashes takes no arguments (got {extra:?})")),
            None => Ok(Invocation::Crashes),
        }),
        "code" => match rest.first().map(String::as_str) {
            // `ask` takes the whole rest of the line as one question, quoted or not, the
            // way `prompt` does -- a question typed at a shell is full of words the shell
            // would otherwise have opinions about.
            Some("ask") => {
                let question = rest[1..].join(" ").trim().to_string();
                if question.is_empty() {
                    Err("code ask needs a question after it".to_string())
                } else {
                    Ok(Invocation::Code {
                        conventions: false,
                        ask: Some(question),
                    })
                }
            }
            // `perms` on its own lists them; with two words it sets one. Nothing in
            // between: `code perms github` reads as a question and would be answered by
            // silence, so it is an error that says which two words were expected.
            Some("perms") => match &rest[1..] {
                [] => Ok(Invocation::CodePerms {
                    grant: None,
                    on: None,
                }),
                [grant, state] => match state.to_lowercase().as_str() {
                    "on" => Ok(Invocation::CodePerms {
                        grant: Some(grant.to_string()),
                        on: Some(true),
                    }),
                    "off" => Ok(Invocation::CodePerms {
                        grant: Some(grant.to_string()),
                        on: Some(false),
                    }),
                    other => Err(format!(
                        "a permission is `on` or `off`, not {other:?} -- for example \
                         `code perms github off`"
                    )),
                },
                _ => Err(
                    "code perms takes nothing, or a permission and `on`/`off` -- for example \
                     `code perms internet off`"
                        .to_string(),
                ),
            },
            // The project folder. With no path it prints the one in force, which is the
            // question an operator asks before they trust any of this.
            Some("workspace") => match &rest[1..] {
                [] => Ok(Invocation::CodeWorkspace { path: None }),
                [path] => Ok(Invocation::CodeWorkspace {
                    path: Some(path.to_string()),
                }),
                _ => Err(
                    "code workspace takes one path, or nothing to print the current one"
                        .to_string(),
                ),
            },
            // The sandbox switches. Both print their state with no argument, because "is
            // this confined" is the question an operator asks before they trust `run` at
            // all, and it should be answerable without changing anything.
            Some(verb @ ("run-network" | "run-unconfined")) => {
                let unconfined = verb == "run-unconfined";
                match &rest[1..] {
                    [] => Ok(Invocation::CodeSandbox {
                        unconfined,
                        on: None,
                    }),
                    [state] => match state.to_lowercase().as_str() {
                        "on" => Ok(Invocation::CodeSandbox {
                            unconfined,
                            on: Some(true),
                        }),
                        "off" => Ok(Invocation::CodeSandbox {
                            unconfined,
                            on: Some(false),
                        }),
                        other => Err(format!("code {verb} is `on` or `off`, not {other:?}")),
                    },
                    _ => Err(format!("code {verb} takes `on`, `off`, or nothing")),
                }
            }
            Some(verb @ ("run-allow" | "run-deny")) => {
                let add = verb == "run-allow";
                match &rest[1..] {
                    [] if add => Ok(Invocation::CodeRunAllow { program: None, add }),
                    [program] => Ok(Invocation::CodeRunAllow {
                        program: Some(program.to_string()),
                        add,
                    }),
                    _ => Err(format!(
                        "code {verb} takes one program name -- for example `code {verb} cargo`"
                    )),
                }
            }
            _ => free_text(rest).and_then(|extra| match extra.as_deref() {
                None => Ok(Invocation::Code {
                    conventions: false,
                    ask: None,
                }),
                Some("conventions") => Ok(Invocation::Code {
                    conventions: true,
                    ask: None,
                }),
                Some(other) => Err(format!(
                    "code takes nothing, the word `conventions`, `perms`, or \
                     `ask <question>` (got {other:?})"
                )),
            }),
        },
        "models" => match rest.len() {
            0 => Ok(Invocation::Models {
                persona: None,
                model: None,
            }),
            2 => Ok(Invocation::Models {
                persona: Some(rest[0].to_string()),
                model: Some(rest[1].to_string()),
            }),
            _ => Err(
                "models takes either nothing, or a speciality and a model -- for example \
                 `aether1 models nexus qwen2.5-coder:7b`, or `aether1 models nexus clear`"
                    .to_string(),
            ),
        },
        "words" => match rest
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            [] => Ok(Invocation::Words {
                word: None,
                said_as: None,
                drop: false,
            }),
            ["drop", word @ ..] if !word.is_empty() => Ok(Invocation::Words {
                word: Some(word.join(" ")),
                said_as: None,
                drop: true,
            }),
            // The respelling is several words far more often than not ("see plus plus"), so
            // it is the rest of the line rather than one argument -- which also means the
            // shell's quoting is one less thing to get right.
            [word, said @ ..] if !said.is_empty() => Ok(Invocation::Words {
                word: Some((*word).to_string()),
                said_as: Some(said.join(" ")),
                drop: false,
            }),
            _ => Err(
                "words takes nothing, a word and how it sounds, or `drop <WORD>` -- \
                      for example `aether1 words nginx engine ex`"
                    .to_string(),
            ),
        },
        "flow" => match rest
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            [] => Ok(Invocation::Flow {
                state: None,
                line: None,
            }),
            // The line's name is several words ("The Umbrals"), so it is the rest of the
            // command rather than one argument -- and any part of the name will do.
            ["line", name @ ..] if !name.is_empty() => Ok(Invocation::Flow {
                state: None,
                line: Some(name.join(" ")),
            }),
            ["line"] => Err("flow line needs the name of a line, or `clear` -- run \
                             `aether1 flow` to see them"
                .to_string()),
            [word] => Ok(Invocation::Flow {
                state: Some((*word).to_string()),
                line: None,
            }),
            _ => Err(
                "flow takes nothing, `on` or `off`, or `line <NAME>` -- for example \
                 `aether1 flow line umbrals`"
                    .to_string(),
            ),
        },
        "signin" => free_text(rest).and_then(|extra| match extra {
            None => Ok(Invocation::SignIn),
            Some(extra) => Err(format!("signin takes no arguments (got {extra:?})")),
        }),
        "signout" => free_text(rest).and_then(|extra| match extra {
            None => Ok(Invocation::SignOut),
            Some(extra) => Err(format!("signout takes no arguments (got {extra:?})")),
        }),
        "update" => free_text(rest).and_then(|extra| match extra {
            None => Ok(Invocation::Update { download: false }),
            Some(extra) => match extra.trim() {
                // Downloading half a gigabyte is a verb of its own rather than what a bare
                // `update` does: the plain form answers the question, and nothing is fetched
                // by asking it.
                "download" => Ok(Invocation::Update { download: true }),
                _ => Err(format!(
                    "update takes nothing, or the word `download` (got {extra:?})"
                )),
            },
        }),
        "verify" => free_text(rest).and_then(|extra| match extra {
            Some(path) => Ok(Invocation::Verify { path }),
            None => Err("verify needs the file to check".to_string()),
        }),
        "installs" => free_text(rest).and_then(|extra| match extra {
            None => Ok(Invocation::Installs { remove: None }),
            Some(extra) => match extra.split_whitespace().collect::<Vec<_>>().as_slice() {
                ["remove", target] => Ok(Invocation::Installs {
                    remove: Some((*target).to_string()),
                }),
                ["remove"] => Err("installs remove needs an id, or the word `old` -- run \
                                   `aether1 installs` to see them"
                    .to_string()),
                _ => Err(format!(
                    "installs takes no arguments, or `remove <id>` (got {extra:?})"
                )),
            },
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

/// Plays a clip, on the chosen speaker where that can be arranged.
///
/// `device` is whatever Settings holds for the output device, and only PulseAudio-based
/// players can be steered by it -- `audio_devices::player_env` is what decides whether
/// the stored value is a real node name or only a label. An empty or unusable one is not
/// an error: the clip plays on the system default, which is what happened before this
/// setting existed.
fn play_audio(path: &Path, device: &str) -> Result<(), String> {
    let env = crate::audio_devices::player_env(device);
    for (player, flags) in PLAYERS {
        let Ok(binary) = which::which(player) else {
            continue;
        };
        let mut command = std::process::Command::new(binary);
        if let Some((key, value)) = &env {
            command.env(key, value);
        }
        let status = command
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
    let reply = response
        .get("reply")
        .and_then(|r| r.as_str())
        .unwrap_or_default();
    // A hand-off is something the companion said out loud, so it prints here too rather
    // than only in the HUD -- otherwise the reply appears to come from the wrong node.
    match response
        .get("handover")
        .and_then(|h| h.as_object())
        .and_then(|h| Some((h.get("from")?.as_str()?, h.get("line")?.as_str()?)))
    {
        Some((from, line)) => Ok(format!("{from}: {line}\n\n{reply}")),
        None => Ok(reply.to_string()),
    }
}

/// `aether1 flow`, the two words that set it, and `flow line` for picking a whole cast.
/// Reports the line hand-offs may move within as well as the mode, because FLOW with no
/// line to move within does nothing and the operator should be able to see that rather
/// than wonder why it is quiet.
/// `aether1 words` -- the same list the Words tab edits, from a terminal.
///
/// Worth having beyond symmetry: this is the one place the substitution can be checked
/// without a window, by adding a word here and running `aether1 say` on a sentence with it
/// in. The window's own "hear the list" button cannot help on a machine with no window.
fn run_words(word: Option<&str>, said_as: Option<&str>, drop: bool) -> Result<String, String> {
    use crate::speech_words::{self, Say};
    let engine = crate::build_llm_engine();
    let db = engine.db();

    if let Some(word) = word {
        let mut words = speech_words::all(db);
        let at = words
            .iter()
            .position(|w| w.from.eq_ignore_ascii_case(word.trim()));
        if drop {
            match at {
                Some(at) => {
                    let gone = words.remove(at);
                    speech_words::set(db, words)?;
                    return Ok(format!("{} is said the ordinary way again.", gone.from));
                }
                // Said rather than errored: the end state the operator asked for is the end
                // state they have, and a list they cannot see is not a list to be quizzed on.
                None => return Ok(format!("{word} was not on the list.")),
            }
        }
        let said_as = said_as.unwrap_or_default();
        let entry = Say {
            from: word.trim().to_string(),
            to: said_as.trim().to_string(),
        };
        match at {
            Some(at) => words[at] = entry,
            None => words.push(entry),
        }
        speech_words::set(db, words)?;
        return Ok(format!(
            "{} is now said \"{}\".",
            word.trim(),
            said_as.trim()
        ));
    }

    let words = speech_words::all(db);
    if words.is_empty() {
        return Ok(
            "Nothing is said differently. `aether1 words <WORD> <HOW IT SOUNDS>` \
                   adds one."
                .to_string(),
        );
    }
    let widest = words
        .iter()
        .map(|w| w.from.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::from("HOW THIS MACHINE SAYS THINGS\n");
    for w in &words {
        out.push_str(&format!(
            "  {:<widest$}  is said  {}\n",
            w.from,
            w.to,
            widest = widest
        ));
    }
    out.push_str("\nOnly the voice hears these; what is written stays as it is.");
    Ok(out)
}

fn run_flow(state: Option<&str>, line: Option<&str>) -> Result<String, String> {
    let engine = crate::build_llm_engine();
    let db = engine.db();

    if let Some(word) = state {
        let on = match word.to_ascii_lowercase().as_str() {
            "on" | "flow" => true,
            "off" | "static" => false,
            other => return Err(format!("flow takes `on` or `off` (got {other:?})")),
        };
        crate::llm::flow::set_enabled(db, on)?;
    }

    if let Some(name) = line {
        match name.trim().to_ascii_lowercase().as_str() {
            "clear" | "none" | "off" => {
                crate::llm::flow::set_line(db, None)?;
            }
            wanted => {
                let group = resolve_line(wanted)?;
                crate::llm::flow::set_line(db, Some(group))?;
            }
        }
    }

    let on = crate::llm::flow::enabled(db);
    let picked = crate::llm::flow::line(db);
    let persona = crate::llm::Persona::from_key(&db.get_setting_string("persona_type", "default"));
    let here = persona.avatar().unwrap_or("AETHER");
    let mut out = String::new();
    out.push_str(if on { "FLOW\n" } else { "STATIC\n" });
    match (on, picked, persona.group()) {
        // A picked line is a different arrangement from wearing one of its members, and
        // saying so is the difference between "this avatar can hand over" and "this cast
        // is answering, currently through this one".
        (true, Some(group), _) => out.push_str(&format!(
            "\n    {group} is picked whole. {here} is holding the line, and the question \
             goes to whichever node of the cast owns it.\n"
        )),
        (true, None, Some(group)) => out.push_str(&format!(
            "\n    {here} is answering, and the question can move to any other node of {group}.\n"
        )),
        (true, None, None) => out.push_str(&format!(
            "\n    {here} belongs to no line, so nothing moves. Pick an avatar from a group, \
             or a whole line with `aether1 flow line <NAME>`, to let it.\n"
        )),
        (false, Some(group), _) => out.push_str(&format!(
            "\n    {group} is picked whole, but STATIC holds the question with {here}.\n"
        )),
        (false, None, _) => out.push_str(&format!(
            "\n    {here} answers everything until you pick somebody else.\n"
        )),
    }
    out.push_str("\n    Lines that can be picked whole: ");
    out.push_str(&crate::llm::Persona::flow_lines().join(", "));
    out.push('\n');
    Ok(out)
}

/// Any part of a line's name, in any case, rather than the exact string: the names are
/// several words and typing "The Umbrals" exactly at a prompt is a worse answer than
/// typing "umbrals".
fn resolve_line(wanted: &str) -> Result<&'static str, String> {
    let groups = crate::llm::Persona::flow_lines();
    let hits: Vec<&'static str> = groups
        .iter()
        .copied()
        .filter(|g| g.to_ascii_lowercase().contains(wanted))
        .collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(format!(
            "no line matches {wanted:?} -- the lines are {}",
            groups.join(", ")
        )),
        several => Err(format!(
            "{wanted:?} matches more than one line: {}",
            several.join(", ")
        )),
    }
}

fn run_status(json: bool, events: bool) -> String {
    let telemetry = Telemetry::snapshot();
    if json {
        return telemetry.to_wire_json().to_string();
    }
    // Which box this is, above the readings. On one machine it is the hostname and barely
    // worth a line; with three of them, a report pasted into a message is otherwise anybody's
    // guess. The JSON form is left alone -- it is a wire shape with consumers.
    let machine = crate::profile::machine_description(crate::build_llm_engine().db());
    let report = format!("Machine: {machine}\n{}", telemetry.diagnostic_report());
    if !events {
        return report;
    }
    // Step 13's rule, kept honest at the command line: the wider sweep of the event log is
    // something you ask for. It is appended to the report rather than replacing it, since
    // "what is this machine doing" and "what has gone wrong on it" are read together.
    format!("{report}\n\n{}", crate::watchers::events::report())
}

/// Comprehensive system diagnostics combining telemetry snapshot and error-level event logs.
/// Unlike status, diagnostics always includes the event log sweep to help identify system issues.
pub(crate) fn run_diagnostics() -> String {
    let telemetry = Telemetry::snapshot();
    let machine = crate::profile::machine_description(crate::build_llm_engine().db());

    // System telemetry section
    let mut output = format!(
        "=== SYSTEM DIAGNOSTICS ===\nMachine: {machine}\n{}",
        telemetry.diagnostic_report()
    );

    // Recent issues section - always included in diagnostics
    output.push_str("\n\n=== RECENT ISSUES ===\n");
    output.push_str(&crate::watchers::events::report());

    output
}

/// `aether1 crashes`. Deliberately shows muted programs too, marked -- the mute list stops
/// AETHER1 interrupting you, and this is you doing the asking.
/// The coding wizard as text: where this machine is, and the commands to move it on.
///
/// The same advisor the HUD panel draws, printed. Somebody who is setting this up because
/// their subscription lapsed is quite likely doing it from a terminal in the first place,
/// and a wizard only reachable from a settings panel is one more window to find.
/// `aether1 code ask` -- the same conversation the HUD's tab keeps, from a terminal.
///
/// Streamed to stdout as it arrives, because the whole point of a local model is that it is
/// yours and the cost of that is that it is slow: a minute of nothing, then a paragraph, is
/// indistinguishable from a hang.
///
/// The commands are listed under the answer rather than offered as anything to press. In a
/// terminal the operator is already at a prompt -- what they need is the line to copy, not
/// a button, and AETHER1 typing into the shell that launched it would be a different and
/// much worse idea than the HUD's button, which types into a shell it started itself.
fn run_code_ask(question: &str) -> String {
    use std::io::Write;

    let engine = crate::build_llm_engine();
    let mut streamed = false;
    let result = commands::code_chat_ask(&engine, question, &mut |delta| {
        streamed = true;
        print!("{delta}");
        let _ = std::io::stdout().flush();
    });

    match result {
        Err(why) => format!("\n{why}\n"),
        Ok(reply) => {
            // Nothing streamed means the provider answered in one piece, so the text has
            // not been printed yet and printing it now is the only way it is seen.
            let mut out = if streamed {
                "\n".to_string()
            } else {
                format!("{}\n", reply.text)
            };
            if !reply.commands.is_empty() {
                out.push_str("\nCOMMANDS IN THAT ANSWER:\n");
                for command in &reply.commands {
                    out.push_str(&format!("    {command}\n"));
                }
                out.push_str(
                    "\n     Nothing above has been run. In the HUD each of these is a button \
                     that types it\n     into the terminal, still waiting on your Return key.\n",
                );
            }
            out
        }
    }
}

/// `aether1 code perms` -- what the coding panel may look at, and the switch for each one.
///
/// The list is deliberately not only a list of switches: it ends with the rule that has no
/// switch, because "what can this thing do to my machine" is the question somebody typing
/// this is really asking, and three yeses would be a misleading answer on their own.
fn run_code_perms(grant: Option<String>, on: Option<bool>) -> Result<String, String> {
    use crate::code_perms::{self, Grant};

    let engine = crate::build_llm_engine();
    let db = engine.db();

    if let (Some(name), Some(on)) = (grant.as_deref(), on) {
        let Some(grant) = Grant::from_key(&name.to_lowercase()) else {
            let known: Vec<&str> = code_perms::ALL.iter().map(|g| g.key()).collect();
            return Err(format!(
                "there is no permission called {name:?}. There are {}: {}",
                code_perms::ALL.len(),
                known.join(", ")
            ));
        };
        code_perms::set(db, grant, on)?;
        return Ok(format!(
            "AETHER CODE may {} {}.\n",
            if on { "now" } else { "no longer" },
            grant.description()[0..1].to_lowercase() + &grant.description()[1..]
        ));
    }

    let mut out = "AETHER CODE -- what it is allowed to look at\n\n".to_string();
    for grant in code_perms::ALL {
        out.push_str(&format!(
            "  {:<9} {}   {}\n",
            grant.key(),
            if code_perms::granted(db, *grant) {
                "ON "
            } else {
                "OFF"
            },
            grant.description()
        ));
    }
    out.push_str("\n  Change one with `aether1 code perms <name> on` or `... off`.\n\n");
    if code_perms::granted(db, Grant::Edit) || code_perms::granted(db, Grant::Run) {
        let where_it_is = match crate::code_workspace::root(db) {
            Ok(root) => root.display().to_string(),
            Err(_) => "not set yet -- `aether1 code workspace <path>`".to_string(),
        };
        out.push_str(&format!(
            "  It can change things, inside one folder and nowhere else: {where_it_is}\n\
             Programs it may run there: {}\n\
             Commands are {}\n\
             Everything outside that folder is still written into your terminal for you to\n\
             run, and nothing enters it but your own Return key.\n",
            match crate::code_workspace::allowlist(db) {
                list if list.is_empty() => "none".to_string(),
                list => list.join(", "),
            },
            crate::code_sandbox::detect().description(),
        ));
    } else {
        out.push_str(
            "  Everything switched on only reads. Nothing AETHER CODE can call changes a\n\
             file, a repository or a setting -- a command that would is written into your\n\
             terminal for you to run, and nothing enters it but your own Return key.\n",
        );
    }
    Ok(out)
}

/// `aether1 code run-network` and `aether1 code run-unconfined` -- the two answers to
/// "what is `run` allowed to reach", printed or set.
///
/// `run-unconfined` is the only switch in this program that removes a boundary rather than
/// adding one, so printing it always says what it costs, whether it is on or off.
fn run_code_sandbox(unconfined: bool, on: Option<bool>) -> Result<String, String> {
    let engine = crate::build_llm_engine();
    let db = engine.db();
    let key = if unconfined {
        crate::code_sandbox::UNCONFINED_SETTING
    } else {
        crate::code_sandbox::NETWORK_SETTING
    };
    if let Some(on) = on {
        db.set_setting(key, &serde_json::json!(on))
            .map_err(|e: rusqlite::Error| e.to_string())?;
    }
    let sandbox = crate::code_sandbox::detect();
    let state = |b: bool| if b { "on" } else { "off" };
    Ok(if unconfined {
        format!(
            "  Commands are {}
  run-unconfined is {}

             With it on, a command runs as you: python3, node, cargo and make can execute              any code they are given, so they can read your keys, write anywhere you can              write, and use the network. With it off, a machine that cannot confine a              command refuses to run one.
",
            sandbox.description(),
            state(crate::code_sandbox::unconfined_allowed(db)),
        )
    } else {
        format!(
            "  Commands are {}
  run-network is {}

             Off is the default: a test suite does not need the network, and a build that              wants to fetch dependencies is worth seeing rather than granting quietly.
",
            sandbox.description(),
            state(crate::code_sandbox::network_allowed(db)),
        )
    })
}

/// `aether1 code workspace [path]` -- the one folder AETHER CODE may change.
///
/// Setting it validates it immediately rather than at the first edit: a path that is a
/// typo, or is the home directory, is worth hearing about while the operator is still
/// looking at the terminal they typed it into.
fn run_code_workspace(path: Option<String>) -> Result<String, String> {
    let engine = crate::build_llm_engine();
    let db = engine.db();

    if let Some(path) = path {
        db.set_setting(
            crate::code_workspace::ROOT_SETTING,
            &serde_json::json!(path.trim()),
        )
        .map_err(|e| format!("cannot save the project folder: {e}"))?;
        let root = crate::code_workspace::root(db)?;
        return Ok(format!(
            "AETHER CODE's project folder is {}.\n\nIt cannot change anything until you \
             also turn the permissions on:\n  aether1 code perms edit on\n  aether1 code \
             perms run on\n",
            root.display()
        ));
    }

    match crate::code_workspace::root(db) {
        Ok(root) => Ok(format!("{}\n", root.display())),
        Err(why) => Ok(format!(
            "No project folder is set.\n\n{why}\n\nSet one with `aether1 code workspace \
             /path/to/project`.\n"
        )),
    }
}

/// `aether1 code run-allow|run-deny <program>` -- the programs it may spawn in that folder.
fn run_code_run_allow(program: Option<String>, add: bool) -> Result<String, String> {
    let engine = crate::build_llm_engine();
    let db = engine.db();
    let mut list = crate::code_workspace::allowlist(db);

    let Some(program) = program else {
        return Ok(if list.is_empty() {
            "AETHER CODE may run nothing in the project folder.\n".to_string()
        } else {
            format!(
                "AETHER CODE may run these in the project folder:\n\n  {}\n",
                list.join("\n  ")
            )
        });
    };
    let program = program.trim().to_string();
    if program.contains('/') || program.contains('\\') {
        return Err("name the program, not a path to it -- the list matches names".to_string());
    }

    let held = list.iter().any(|entry| entry == &program);
    if add && held {
        return Ok(format!("{program} was already on the list.\n"));
    }
    if !add && !held {
        return Ok(format!("{program} was not on the list.\n"));
    }
    if add {
        list.push(program.clone());
        list.sort();
    } else {
        list.retain(|entry| entry != &program);
    }
    db.set_setting(
        crate::code_workspace::ALLOWLIST_SETTING,
        &serde_json::json!(list),
    )
    .map_err(|e| format!("cannot save the list: {e}"))?;
    Ok(format!(
        "AETHER CODE may {} run {program} in the project folder.\n",
        if add { "now" } else { "no longer" }
    ))
}

fn run_code(conventions: bool, ask: Option<String>) -> String {
    if conventions {
        return crate::code_setup::conventions();
    }

    if let Some(question) = ask {
        return run_code_ask(&question);
    }

    let engine = crate::build_llm_engine();
    let advice = commands::code_advice(&engine);

    let mut out = format!(
        "AETHER1 -- writing code on this machine\n\n{}\n",
        advice.headline
    );

    for (index, step) in advice.steps.iter().enumerate() {
        out.push_str(&format!(
            "\n{}. {}\n   {}\n",
            index + 1,
            step.title,
            step.detail
        ));
        if let Some(command) = &step.command {
            out.push_str(&indented(command));
        }
        if let Some(url) = &step.url {
            out.push_str(&format!("\n       {url}\n"));
        }
    }

    // Only worth listing while there is a choice to make. Once a coding model is here, the
    // model line above the commands already names the one they will be run with.
    if !advice.model_installed {
        out.push_str("\nMODELS FOR THIS MACHINE (the marked one is the recommendation):\n");
        for model in advice.models.iter().filter(|m| m.fits) {
            let mark = if model.recommended { "->" } else { "  " };
            // Which side of the card's line this one falls on. Without it the row above the
            // recommendation looks like an equally good option that the wizard just missed.
            let speed = if model.fits_on_gpu {
                "  [fits on the card]"
            } else {
                ""
            };
            // The second job these can do, said on the row rather than in a footnote: it is
            // the difference between one download and two.
            let both = if model.runs_aether1 {
                "  [also runs AETHER1 itself]"
            } else {
                ""
            };
            out.push_str(&format!(
                "  {mark} {:<22} {:<14} {}{speed}{both}\n",
                model.name, model.download, model.label
            ));
        }
        out.push_str(&format!("\n     {}\n", advice.sized_against));
        out.push_str("\n     Download one with:  ollama pull <name>\n");
    }

    out.push_str(&format!(
        "\nUSING: {}{}\n",
        advice.model,
        if advice.model_installed {
            " (downloaded)"
        } else {
            " (not downloaded yet)"
        }
    ));

    if let Some(current) = &advice.aether1_model {
        out.push_str(&format!(
            "AETHER1 ITSELF IS ON: {current}{}\n",
            if advice.aether1_model_too_small {
                " -- too small to troubleshoot or to drive an agent"
            } else {
                ""
            }
        ));
    }

    for agent in &advice.agents {
        let state = if agent.installed {
            "installed"
        } else {
            "not installed"
        };
        out.push_str(&format!(
            "\n{} -- {state}\n   {}\n",
            agent.label, agent.blurb
        ));
        let steps = if agent.installed {
            &agent.connect
        } else {
            &agent.install
        };
        for step in steps {
            out.push_str(&format!("\n   {}\n", step.title));
            if let Some(command) = &step.command {
                out.push_str(&indented(command));
            }
        }
    }

    out.push_str(
        "\nBefore the first session, put the house rules at the top of your project:\n\
         \n       aether1 code conventions > AGENTS.md\n\
         \nBoth agents read that file every turn, and it does more to keep a local model in\n\
         your style than a bigger model would.\n",
    );
    out
}

/// A command block, indented so it reads as something to paste rather than as prose. A
/// multi-line command keeps its shape, which matters for the ones that write a config file.
fn indented(command: &str) -> String {
    command
        .lines()
        .map(|line| format!("       {line}\n"))
        .collect()
}

/// `aether1 models`, both the showing and the setting.
///
/// The showing is the important half: step 19's whole claim is that "which model for which
/// job" reads back as a sentence the operator can check, and a list they can see is what
/// makes that true rather than a thing the design asserts about itself.
fn run_models(persona: Option<String>, model: Option<String>) -> Result<String, String> {
    use crate::llm::routing;
    use crate::llm::Persona;

    let engine = crate::build_llm_engine();
    let db = engine.db();

    if let (Some(name), Some(model)) = (persona.as_deref(), model.as_deref()) {
        let persona = Persona::from_key(name);
        // from_key falls back rather than failing, so a typo would silently set the
        // default persona's model. Checked here, where there is someone to tell.
        if persona.key() != name.to_ascii_lowercase() {
            return Err(format!(
                "no speciality called {name:?} -- run `aether1 models` to see the names"
            ));
        }
        let clearing = matches!(model.to_ascii_lowercase().as_str(), "clear" | "none" | "-");
        routing::set_choice(db, &persona, (!clearing).then_some(model))?;
        return Ok(if clearing {
            format!(
                "\"{}\" no longer has a model of its own; it will use the suggestion, or the \
                 general model where nothing suits it.",
                persona.speciality()
            )
        } else {
            format!("\"{}\" will run on {model}.", persona.speciality())
        });
    }

    let endpoint = db.get_setting_string("llm_endpoint", "http://localhost:11434");
    let available = crate::model_scanner::models_at(&endpoint);
    let speeds = routing::measured_speeds(db);

    let mut out = match &available {
        Some(models) if models.is_empty() => format!(
            "{endpoint} answered but has no models loaded, so every speciality falls back to \
             the general model.\n"
        ),
        Some(models) => format!(
            "{} model{} available at {endpoint}:\n",
            models.len(),
            if models.len() == 1 { "" } else { "s" }
        ),
        None => format!(
            "Nothing answered at {endpoint}, so this is what is stored rather than what is \
             running.\n"
        ),
    };

    let available = available.unwrap_or_default();
    for persona in Persona::all() {
        let choice = routing::resolve(db, &persona, &available, &speeds);
        let line = match (&choice.model, &choice.reason) {
            (Some(model), routing::Reason::Chosen) => format!("{model}  (your choice)"),
            (Some(model), routing::Reason::Suggested) => format!("{model}  (suggested)"),
            (Some(model), routing::Reason::Missing(gone)) => {
                format!("{model}  (suggested -- your {gone} is not installed)")
            }
            (None, routing::Reason::Missing(gone)) => {
                format!("the general model  (your {gone} is not installed)")
            }
            // Reason::General carries no model by construction, and a Some here would mean
            // resolve() had changed underneath this; say what is true rather than assume.
            (Some(model), routing::Reason::General) => model.clone(),
            (None, _) => "the general model".to_string(),
        };
        out.push_str(&format!(
            "\n    {:<16} {}\n    {:<16} {line}\n",
            persona.key(),
            persona.speciality(),
            ""
        ));
    }
    out.push_str(
        "\nRun `aether1 models <speciality> <model>` to point one at a model, or \
         `aether1 models <speciality> clear` to go back to the suggestion.",
    );
    Ok(out)
}

/// `aether1 doctor`: the report, and -- with --fix -- the repairs, one asked-for question at a
/// time.
///
/// The asking happens here rather than in doctor.rs on purpose. `doctor::apply` is the act,
/// and the operator's y is what calls it; keeping the prompt in the interface means there is
/// no code path anywhere that can repair something without a person having answered. On a
/// pipe there is nobody to answer, so nothing is changed and the report says what it would
/// have offered.
fn run_doctor(
    fix: bool,
    heal: bool,
    json: bool,
    report: Option<Option<String>>,
    replay: Option<String>,
) -> Result<String, String> {
    use crate::doctor;

    // A recorded observation from another machine: judged, never repaired. This is the whole
    // reason the observation is a data structure -- a fault seen once on somebody's desktop
    // can be read here, and in a test, on a machine that never had it.
    if let Some(path) = replay {
        let raw =
            std::fs::read_to_string(&path).map_err(|e| format!("could not read {path}: {e}"))?;
        // The file may be a whole --report bundle, which is prose with the observation at the
        // end; take the JSON out of it rather than making the operator edit the file.
        let observation = match raw.find('{') {
            Some(at) => &raw[at..],
            None => raw.as_str(),
        };
        let health = doctor::replay(observation)?;
        return Ok(format!(
            "Judging an observation recorded elsewhere -- nothing here was looked at.\n\n{}",
            render_health(&health)
        ));
    }

    let engine = crate::build_llm_engine();
    // Facts::default(): the window, the tray registration and the watcher's poll are facts
    // about the running desktop process, and this is not it. The report says so per check
    // rather than guessing.
    let (observation, health) = doctor::report(engine.db(), &doctor::Facts::default());

    if json {
        let bundle = serde_json::json!({ "observation": observation, "health": health });
        return serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string());
    }

    if let Some(target) = report {
        let text = doctor::bug_report(&observation, &health);
        let path = match target {
            Some(path) => std::path::PathBuf::from(path),
            None => std::env::temp_dir().join("aether1-doctor-report.txt"),
        };
        std::fs::write(&path, &text)
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        return Ok(format!(
            "Written to {}.\n\nIt goes nowhere on its own. Read it before you send it \
             anywhere -- it names paths, the model you use and the programs installed here -- \
             and `aether1 doctor --replay` on that file judges it again anywhere.",
            path.display()
        ));
    }

    if heal {
        // Typing `--heal` is the go-ahead, which is why nothing is asked here. A terminal is
        // also the one place this can be asked for on a machine whose window will not open,
        // which is a fair part of why it exists.
        let attended = doctor::attend(&engine, &doctor::Facts::default(), None);
        return Ok(format!(
            "AETHER1 -- fixing what it can\n\n{}",
            attended.as_report()
        ));
    }

    let mut out = render_health(&health);
    if fix {
        out.push_str(&run_doctor_fixes(&engine, &health));
    } else if health.checks.iter().any(|check| check.repair.is_some()) {
        out.push_str(
            "\nSome of this AETHER1 can repair itself. Run `aether1 doctor --fix` to be asked \
             about each one, or `aether1 doctor --heal` to have it make them all.\n",
        );
    }
    Ok(out)
}

/// The report as an operator reads it: the broken things first, each with what is true and
/// what to do, then the ones that are fine as one line each.
fn render_health(health: &crate::doctor::Health) -> String {
    use crate::doctor::Verdict;

    let mut out = format!("AETHER1 -- is it working?\n\n{}\n", health.headline);

    for check in health.checks.iter().filter(|c| c.verdict != Verdict::Ok) {
        out.push_str(&format!(
            "\n[{}] {}\n     {}\n",
            check.verdict.mark(),
            check.title,
            check.detail
        ));
        for step in &check.steps {
            out.push_str(&format!(
                "\n     -> {}\n        {}\n",
                step.title, step.detail
            ));
            if let Some(command) = &step.command {
                out.push_str(&indented(command));
            }
            if let Some(url) = &step.url {
                out.push_str(&format!("\n       {url}\n"));
            }
        }
    }

    let working: Vec<&str> = health
        .checks
        .iter()
        .filter(|c| c.verdict == Verdict::Ok)
        .map(|c| c.title)
        .collect();
    if !working.is_empty() {
        out.push_str(&format!("\nWorking: {}\n", working.join(", ")));
    }

    for line in &health.repeated {
        out.push_str(&format!("\n[!!] {line}\n"));
    }

    out
}

/// The repairs, offered one at a time. Returns what happened, to be appended to the report.
fn run_doctor_fixes(engine: &crate::llm::LlmEngine, health: &crate::doctor::Health) -> String {
    use crate::doctor::RepairKind;

    let repairable: Vec<_> = health
        .checks
        .iter()
        .filter_map(|check| check.repair.clone().map(|repair| (check.id, repair)))
        .collect();
    if repairable.is_empty() {
        return "\nThere is nothing here AETHER1 knows how to repair by itself.\n".to_string();
    }

    let mut out = String::from("\nREPAIRS\n");
    for (check, repair) in repairable {
        out.push_str(&format!("\n  {}\n     {}\n", repair.title, repair.detail));
        match &repair.kind {
            // Never run from here, whatever the operator answers: the command needs root, and
            // AETHER1 holding a password is a bigger change to this program than any package.
            RepairKind::HandOver { command } => {
                out.push_str(&format!("\n     Run it yourself:\n{}", indented(command)));
                continue;
            }
            RepairKind::InApp => {
                out.push_str(
                    "\n     This one only the running app can do -- open AETHER1 and press it \
                     in Settings -> Diagnostics.\n",
                );
                continue;
            }
            RepairKind::Run => {}
        }

        // Nothing is changed on a pipe. A script that ran `aether1 doctor --fix` in a cron job
        // would otherwise be exactly the silent auto-repair this is not.
        if !std::io::stdin().is_terminal() {
            out.push_str(
                "\n     Not changed: there is no terminal here to ask. Run this from a shell \
                 and answer y.\n",
            );
            continue;
        }

        match ask_yes("     Do this now? [y/N] ") {
            Ok(false) => {
                out.push_str("\n     Left alone.\n");
                continue;
            }
            Err(error) => {
                out.push_str(&format!("\n     Not changed: {error}\n"));
                continue;
            }
            Ok(true) => {}
        }

        match crate::doctor::apply(engine, check, repair.id) {
            Ok(outcome) => {
                out.push_str(&format!("\n     {}\n", outcome.message));
                // The re-check is what makes this a repair rather than a hope, so it is
                // reported whether it is good news or not.
                if let (Some(verdict), Some(detail)) = (outcome.rechecked, outcome.recheck_detail) {
                    out.push_str(&format!("     Afterwards: [{}] {detail}\n", verdict.mark()));
                }
            }
            Err(error) => out.push_str(&format!("\n     {error}\n")),
        }
    }
    out
}

/// One yes-or-no question on the terminal. Anything but a y is a no, because the expensive
/// mistake here is acting on a keypress somebody did not mean.
fn ask_yes(question: &str) -> Result<bool, String> {
    use std::io::Write;
    print!("{question}");
    std::io::stdout()
        .flush()
        .map_err(|e| format!("could not ask: {e}"))?;
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|e| format!("could not read your answer: {e}"))?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

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
        play_audio(
            &path,
            &engine.db().get_setting_string("audio_output_device", ""),
        )?;
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

/// One row of `aether1 installs`: the id to name it by, what it is, and where it stands
/// against the copy that is running.
fn render_install(
    install: &crate::installs::Install,
    running: Option<&crate::installs::Version>,
) -> String {
    use crate::installs::Standing;
    let note = match crate::installs::standing(install, running) {
        Standing::Running => "the copy you are running".to_string(),
        Standing::Stale => "older than the one you are running".to_string(),
        Standing::Duplicate => {
            "the same build as the one you are running, in a second place".to_string()
        }
        Standing::Newer => "newer than the one you are running -- left alone".to_string(),
        Standing::Unknown => "cannot tell how old this one is".to_string(),
        Standing::Keep(why) => format!("kept: {why}"),
    };
    format!(
        "\n    {}  {}\n              {note}",
        install.id,
        install.describe()
    )
}
/// `aether1 signin`: the device flow with a terminal in front of it. Prints the code, opens
/// nothing, and waits.
///
/// The browser is the operator's to open. A CLI that launches one is a CLI that cannot be run
/// over ssh, which is exactly the machine most likely to be updated from a terminal.
fn run_sign_in() -> Result<String, String> {
    if !crate::github_auth::configured() {
        return Err(crate::github_auth::SignInError::NotConfigured.message());
    }
    if crate::github_auth::signed_in() {
        return Ok(
            "Already signed in. `aether1 signout` first if you want to sign in as somebody else."
                .to_string(),
        );
    }
    let code = crate::github_auth::start(crate::USER_AGENT).map_err(|e| e.message())?;
    // Printed and flushed before the wait begins, not after: the whole point of this output is
    // that it is read while the polling happens.
    println!(
        "Open {} and enter this code:\n\n    {}\n\nWaiting for it to be approved (Ctrl-C to stop)...",
        code.verification_uri, code.user_code
    );
    use std::io::Write as _;
    let _ = std::io::stdout().flush();

    let token =
        crate::github_auth::wait_for_token(&code, crate::USER_AGENT).map_err(|e| e.message())?;
    let storage = crate::github_auth::store_token(&token)?;
    // Where the token went is said out loud rather than assumed. On a box with no Secret
    // Service running -- a headless one, which is a real case here -- it lands in a 0600 file
    // instead, and a file is not a keychain however much it would be convenient to call it one.
    Ok(match storage {
        crate::github_auth::Storage::Keychain => {
            "Signed in. The token is in this machine's keychain.".to_string()
        }
        crate::github_auth::Storage::File => format!(
            "Signed in. This machine has no keychain service running, so the token is in a \
             file only you can read: {}",
            crate::project_root()
                .join("backend")
                .join("github_token")
                .display()
        ),
        crate::github_auth::Storage::None => "Signed in.".to_string(),
    })
}

fn run_sign_out() -> String {
    if crate::github_auth::clear_token() {
        "Signed out. New versions will not be checked for until you sign in again.".to_string()
    } else {
        "Nothing to sign out of.".to_string()
    }
}

/// `aether1 verify <file>`: the signature check on its own, for a bundle that did not come
/// through the download. The `.minisig` is expected beside it, which is how the release
/// attaches it and how anyone copying one would carry it.
fn run_verify(path: &str) -> Result<String, String> {
    let file = crate::paths::expand_home(path);
    if !file.exists() {
        return Err(format!("there is no file at {}", file.display()));
    }
    let signature = std::path::PathBuf::from(format!("{}.minisig", file.display()));
    if !signature.exists() {
        return Err(format!(
            "no signature beside it -- {} has to be there too, and it is attached to the \
             release next to the bundle",
            signature.display()
        ));
    }
    crate::releases::verify_file(&file, &signature)
        .map(|()| {
            format!(
                "{} is signed by the key built into this copy of AETHER1.",
                file.display()
            )
        })
        .map_err(|e| e.message())
}

/// `aether1 update`: the comparison, and with `download`, the fetch. Never the install -- see
/// releases.rs for why half a gigabyte replacing the running app is the operator's own last
/// step.
fn run_update(download: bool) -> Result<String, String> {
    let mode = crate::releases::mode();
    if mode == crate::releases::Mode::Checkout {
        return Ok(
            "This is a git checkout, so its update is `git pull` and a rebuild -- which is a \
             better update than downloading a bundle of your own work. The tray's Check for \
             Updates does both."
                .to_string(),
        );
    }
    let release =
        crate::releases::latest(crate::UPDATE_REPO, crate::USER_AGENT).map_err(|e| e.message())?;
    let running = crate::APP_VERSION;
    let newer = crate::releases::is_newer(running, &release.tag);

    if newer == Some(false) {
        return Ok(format!(
            "Up to date: running {running}, and {} is the latest release.",
            release.tag
        ));
    }
    if newer.is_none() {
        return Ok(format!(
            "The latest release is tagged {}, which is not a version this can compare {running} \
             against. {}",
            release.tag, release.html_url
        ));
    }

    let asset = release.asset.as_ref();
    if !download {
        let size = asset
            .map(|a| format!(" ({:.1} GB)", a.size as f64 / 1_000_000_000.0))
            .unwrap_or_default();
        return Ok(format!(
            "{} is out; you are running {running}.\n{}\n\nRun `aether1 update download` to fetch \
             it{size}. Nothing installs itself: you get a file whose signature has been \
             checked.",
            release.tag, release.html_url
        ));
    }

    let asset = asset.ok_or_else(|| {
        format!(
            "release {} has no bundle for this platform attached to it",
            release.tag
        )
    })?;
    // A progress line rather than a bar: this is stdout, possibly a log file, and a redrawn
    // bar in a log file is a thousand lines of nothing.
    let mut last_decile = 0;
    let path = crate::releases::download_and_verify(
        crate::UPDATE_REPO,
        asset,
        crate::USER_AGENT,
        |done, total| {
            if total == 0 {
                return;
            }
            let decile = done * 10 / total;
            if decile > last_decile {
                last_decile = decile;
                println!("  {}%", decile * 10);
            }
        },
    )
    .map_err(|e| e.message())?;

    Ok(format!(
        "Downloaded and its signature checked:\n\n    {}\n\nRun it when you are ready. AETHER1 \
         does not install it for you.",
        path.display()
    ))
}

fn run_installs(target: Option<&str>) -> Result<String, String> {
    use crate::installs::Machine as _;
    let machine = crate::installs::ThisMachine;
    let found = crate::installs::detect(&machine);
    let running = machine.running_version();
    match target {
        None => {
            if found.is_empty() {
                return Ok("No copy of AETHER1 was found anywhere this knows to look.".to_string());
            }
            let mut out = format!(
                "{} cop{} of AETHER1 on this machine:\n",
                found.len(),
                if found.len() == 1 { "y" } else { "ies" }
            );
            for install in &found {
                out.push_str(&render_install(install, running.as_ref()));
            }
            let stale = crate::installs::others(&found, running.as_ref());
            out.push_str(
                match stale.len() {
                    0 => "\n\nNothing here is worth removing.".into(),
                    1 => "\n\nRun `aether1 installs remove <id>` to take the older one away, or \
                      `aether1 installs remove old` to do the same thing without typing the id."
                        .to_string(),
                    n => format!(
                        "\n\nRun `aether1 installs remove <id>` to take one away, or \
                     `aether1 installs remove old` to take all {n} of them."
                    ),
                }
                .as_str(),
            );
            Ok(out)
        }
        Some("old") => {
            let stale = crate::installs::others(&found, running.as_ref());
            if stale.is_empty() {
                return Ok("Nothing to remove: this is the only copy of AETHER1 here.".to_string());
            }
            let mut out = String::new();
            for install in &stale {
                match crate::installs::remove(install, &machine) {
                    Ok(outcome) => out.push_str(&render_outcome(install, &outcome)),
                    Err(message) => out.push_str(&format!("\n    {} -- {message}", install.id)),
                }
            }
            Ok(out.trim_start_matches('\n').to_string())
        }
        Some(id) => {
            let install = found
                .iter()
                .find(|install| install.id == id)
                .ok_or_else(|| {
                    format!("no copy with the id {id} -- run `aether1 installs` to see them")
                })?;
            let outcome = crate::installs::remove(install, &machine)?;
            Ok(render_outcome(install, &outcome)
                .trim_start_matches('\n')
                .to_string())
        }
    }
}

/// What actually happened, said plainly -- two of the three outcomes did not delete
/// anything, and the difference between "it is gone" and "it is gone once you run this"
/// is the whole message.
fn render_outcome(
    install: &crate::installs::Install,
    outcome: &crate::installs::Outcome,
) -> String {
    use crate::installs::Outcome;
    match outcome {
        Outcome::Removed { paths } => format!(
            "\nRemoved {} ({} file{} deleted). Your vault, conversations and settings were not \
             touched.",
            install.path.display(),
            paths.len(),
            if paths.len() == 1 { "" } else { "s" }
        ),
        Outcome::Ran { command } => {
            format!("\nRan {command} to uninstall {}.", install.path.display())
        }
        Outcome::HandedOver { command } => format!(
            "\n{} is not AETHER1's to delete. Run this to remove it:\n    {command}",
            install.path.display()
        ),
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
        Invocation::Diagnostics => Ok(run_diagnostics()),
        Invocation::Crashes => run_crashes(),
        Invocation::Doctor {
            fix,
            heal,
            json,
            report,
            replay,
        } => run_doctor(fix, heal, json, report, replay),
        Invocation::Code { conventions, ask } => Ok(run_code(conventions, ask)),
        Invocation::CodePerms { grant, on } => run_code_perms(grant, on),
        Invocation::CodeWorkspace { path } => run_code_workspace(path),
        Invocation::CodeSandbox { unconfined, on } => run_code_sandbox(unconfined, on),
        Invocation::CodeRunAllow { program, add } => run_code_run_allow(program, add),
        Invocation::Models { persona, model } => run_models(persona, model),
        Invocation::Flow { state, line } => run_flow(state.as_deref(), line.as_deref()),
        Invocation::Words {
            word,
            said_as,
            drop,
        } => run_words(word.as_deref(), said_as.as_deref(), drop),
        Invocation::SignIn => run_sign_in(),
        Invocation::SignOut => Ok(run_sign_out()),
        Invocation::Update { download } => run_update(download),
        Invocation::Verify { path } => run_verify(&path),
        Invocation::Installs { remove } => run_installs(remove.as_deref()),
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

    fn doctor(args: &[&str]) -> Invocation {
        let mut argv = vec!["doctor"];
        argv.extend_from_slice(args);
        parse_args(&argv)
    }

    #[test]
    fn doctor_takes_its_flags_in_any_order() {
        assert_eq!(
            doctor(&[]),
            Invocation::Doctor {
                fix: false,
                heal: false,
                json: false,
                report: None,
                replay: None,
            }
        );
        assert_eq!(
            doctor(&["--json", "--fix"]),
            Invocation::Doctor {
                fix: true,
                heal: false,
                json: true,
                report: None,
                replay: None,
            }
        );
    }

    #[test]
    fn doctors_report_path_is_optional_and_never_eats_a_flag() {
        assert_eq!(
            doctor(&["--report"]),
            Invocation::Doctor {
                fix: false,
                heal: false,
                json: false,
                report: Some(None),
                replay: None,
            }
        );
        assert_eq!(
            doctor(&["--report", "/tmp/out.txt"]),
            Invocation::Doctor {
                fix: false,
                heal: false,
                json: false,
                report: Some(Some("/tmp/out.txt".to_string())),
                replay: None,
            }
        );
        // The flag after --report is a flag, not a filename: writing the report to a file
        // called "--fix" and silently not fixing anything is the worst of both.
        assert_eq!(
            doctor(&["--report", "--fix"]),
            Invocation::Doctor {
                fix: true,
                heal: false,
                json: false,
                report: Some(None),
                replay: None,
            }
        );
    }

    #[test]
    fn replaying_someone_elses_observation_cannot_repair_this_machine() {
        assert!(matches!(
            doctor(&["--replay", "/tmp/obs.json", "--fix"]),
            Invocation::Invalid(_)
        ));
        assert_eq!(
            doctor(&["--replay", "/tmp/obs.json"]),
            Invocation::Doctor {
                fix: false,
                heal: false,
                json: false,
                report: None,
                replay: Some("/tmp/obs.json".to_string()),
            }
        );
    }

    #[test]
    fn doctor_says_what_it_takes_rather_than_ignoring_a_typo() {
        assert!(matches!(doctor(&["--fixx"]), Invocation::Invalid(_)));
        assert!(matches!(doctor(&["--replay"]), Invocation::Invalid(_)));
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
        assert_eq!(
            parse_args(&["installs"]),
            Invocation::Installs { remove: None }
        );
        assert_eq!(
            parse_args(&["installs", "remove", "a1b2c3d4"]),
            Invocation::Installs {
                remove: Some("a1b2c3d4".to_string())
            }
        );
        assert_eq!(
            parse_args(&["installs", "remove", "old"]),
            Invocation::Installs {
                remove: Some("old".to_string())
            }
        );
        // `remove` with nothing named would otherwise have to guess which copy, and the
        // guess is not recoverable.
        assert!(matches!(
            parse_args(&["installs", "remove"]),
            Invocation::Invalid(_)
        ));
        assert!(matches!(
            parse_args(&["installs", "everything"]),
            Invocation::Invalid(_)
        ));
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
    fn code_is_recognized_with_and_without_its_one_word() {
        assert_eq!(
            parse_args(&["code"]),
            Invocation::Code {
                conventions: false,
                ask: None
            }
        );
        assert_eq!(
            parse_args(&["code", "conventions"]),
            Invocation::Code {
                conventions: true,
                ask: None
            }
        );
    }

    #[test]
    fn code_perms_is_recognized_on_its_own_and_as_a_switch() {
        assert_eq!(
            parse_args(&["code", "perms"]),
            Invocation::CodePerms {
                grant: None,
                on: None
            }
        );
        assert_eq!(
            parse_args(&["code", "perms", "github", "off"]),
            Invocation::CodePerms {
                grant: Some("github".to_string()),
                on: Some(false)
            }
        );
        assert_eq!(
            parse_args(&["code", "perms", "internet", "ON"]),
            Invocation::CodePerms {
                grant: Some("internet".to_string()),
                on: Some(true)
            }
        );
    }

    /// Half a switch. `code perms github` reads as a question, and answering it by doing
    /// nothing is how somebody comes away believing they turned something off.
    #[test]
    fn code_perms_refuses_half_a_switch() {
        assert!(matches!(
            parse_args(&["code", "perms", "github"]),
            Invocation::Invalid(_)
        ));
        assert!(matches!(
            parse_args(&["code", "perms", "github", "maybe"]),
            Invocation::Invalid(_)
        ));
    }

    /// `code rules` is the obvious near-miss, and silently printing the wizard for it would
    /// leave somebody wondering where their conventions went.
    #[test]
    fn code_rejects_a_word_it_does_not_know() {
        assert!(matches!(
            parse_args(&["code", "rules"]),
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
