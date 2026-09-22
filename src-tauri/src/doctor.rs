// Whether AETHER1 itself is working, said as a list -- and the repairs it is allowed to make
// to the ground it stands on, each one asked for first.
//
// `aether1 status` describes the computer: CPU, RAM, disk, the graphics card, what is eating
// the network. Useful, and not the question an operator asks when the companion feels wrong,
// which is *is AETHER1 working?* Until now the only thing that answered any part of that was
// `setup::advise`, and it looks at one subsystem -- the model -- in the five stages a fresh
// install moves through. Everything else reported its own health in its own way to whoever
// happened to call it: the voice failed inside tts.rs, the hotkey never fired on Wayland and
// said nothing, a missing coredumpctl was mentioned only if you ran `aether1 crashes`.
//
// **A check here is a pure function of an observation.** A probe touches the world and
// returns a plain data structure; a verdict reads that structure and decides. This is the
// split `setup::advise` already has, and everything below depends on it:
//
//   * An observation can be written down. The bytes that made a check fail on one machine
//     can be carried to another machine and replayed -- `aether1 doctor --replay`.
//   * A check can therefore be tested against a machine it never saw, in CI, for ever after.
//     The tests at the bottom of this file are exactly that, and they need no Ollama, no
//     Windows and no desktop session.
//   * A repair can be *verified*, by running the same verdict against a fresh probe. That is
//     the difference between fixing something and hoping.
//
// **What it may repair, and what it may never do.** The foundation AETHER1 stands on is fair
// game: a service that is not running, a half-finished download, a stale queue. AETHER1's own
// source is not, on any machine, including this one with the checkout sitting right there.
// And nothing is repaired without being asked for at the moment of acting -- detection is
// automatic, changing anything is not, which is rule 1 of installs.rs applied to a wider
// surface. A repair that needs root is never run by AETHER1 at all: it is handed over as the
// exact command, because AETHER1 asking for a password is a bigger change to this program
// than any package it could install.
//
// **Three moments, and no fourth.** The list runs at startup, again when a subsystem actually
// fails, and whenever the operator asks. Nothing sweeps on a timer: a companion quietly
// probing ports and spawning processes every minute costs more than it is worth, and these
// checks are cheap only for as long as they are not all running all the time.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::llm::{ActionStatus, LlmEngine, MemoryDb};
use crate::setup::{Os, Step};

/// Every check, in the order a report prints them: what AETHER1 owns first, since a fault
/// there is AETHER1's own bug and no amount of installing things is the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckId {
    Database,
    Hud,
    Tray,
    BackgroundServices,
    Watchers,
    Server,
    ConsentQueue,
    ModelEndpoint,
    ConfiguredModel,
    VoiceOut,
    VoiceIn,
    Hotkey,
    DiskRoom,
    UpdatePath,
}

/// Whose fault a failure is, which decides what can be done about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Owner {
    /// AETHER1's own. A fault here is a bug, and the honest response is a bug report.
    Aether1,
    /// The machine underneath. This is the class where "fix it" can mean something.
    Foundation,
}

impl CheckId {
    pub const ALL: [CheckId; 14] = [
        CheckId::Database,
        CheckId::Hud,
        CheckId::Tray,
        CheckId::BackgroundServices,
        CheckId::Watchers,
        CheckId::Server,
        CheckId::ConsentQueue,
        CheckId::ModelEndpoint,
        CheckId::ConfiguredModel,
        CheckId::VoiceOut,
        CheckId::VoiceIn,
        CheckId::Hotkey,
        CheckId::DiskRoom,
        CheckId::UpdatePath,
    ];

    /// The stable key: what a repair record names, and what `--replay` matches on. Spelled
    /// out rather than derived from the variant name so renaming a variant cannot quietly
    /// invalidate every record ever written.
    pub fn key(self) -> &'static str {
        match self {
            CheckId::Database => "database",
            CheckId::Hud => "hud",
            CheckId::Tray => "tray",
            CheckId::BackgroundServices => "background-services",
            CheckId::Watchers => "watchers",
            CheckId::Server => "server",
            CheckId::ConsentQueue => "consent-queue",
            CheckId::ModelEndpoint => "model-endpoint",
            CheckId::ConfiguredModel => "configured-model",
            CheckId::VoiceOut => "voice-out",
            CheckId::VoiceIn => "voice-in",
            CheckId::Hotkey => "hotkey",
            CheckId::DiskRoom => "disk-room",
            CheckId::UpdatePath => "update-path",
        }
    }

    /// The check a key names, or None when it names none -- a report from a build with a check
    /// this one does not have, or a typo in a repair request.
    pub fn from_key(key: &str) -> Option<CheckId> {
        CheckId::ALL.iter().copied().find(|id| id.key() == key)
    }

    /// The row's name in a report, in the words an operator would use.
    pub fn title(self) -> &'static str {
        match self {
            CheckId::Database => "Memory database",
            CheckId::Hud => "The window",
            CheckId::Tray => "Tray icon",
            CheckId::BackgroundServices => "Background services",
            CheckId::Watchers => "Crash watcher",
            CheckId::Server => "Server",
            CheckId::ConsentQueue => "Things waiting to be approved",
            CheckId::ModelEndpoint => "A model answers",
            CheckId::ConfiguredModel => "The chosen model is installed",
            CheckId::VoiceOut => "Speaking",
            CheckId::VoiceIn => "Listening",
            CheckId::Hotkey => "The keys that summon it",
            CheckId::DiskRoom => "Room to work",
            CheckId::UpdatePath => "Updates",
        }
    }

    pub fn owner(self) -> Owner {
        match self {
            CheckId::Database
            | CheckId::Hud
            | CheckId::Tray
            | CheckId::BackgroundServices
            | CheckId::Watchers
            | CheckId::Server
            | CheckId::ConsentQueue => Owner::Aether1,
            CheckId::ModelEndpoint
            | CheckId::ConfiguredModel
            | CheckId::VoiceOut
            | CheckId::VoiceIn
            | CheckId::Hotkey
            | CheckId::DiskRoom
            | CheckId::UpdatePath => Owner::Foundation,
        }
    }
}

/// How a check came out.
///
/// `Unknown` is the fourth verdict and it is here deliberately. Several of these facts are
/// only knowable inside the running desktop process -- whether the window answers, whether
/// the hotkey registered -- and `aether1 doctor` in a terminal is a different process. The
/// alternative to saying so is reporting a thing nobody asked as healthy, and a
/// known-unknowable reported as fine is worse than reporting it broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Ok,
    Unknown,
    Degraded,
    Failed,
}

impl Verdict {
    pub fn mark(self) -> &'static str {
        match self {
            Verdict::Ok => "OK",
            Verdict::Unknown => "??",
            Verdict::Degraded => "--",
            Verdict::Failed => "XX",
        }
    }
}

// ---------------------------------------------------------------------------------------
// The observation: everything the checks read, written down so it can be replayed.
// ---------------------------------------------------------------------------------------

/// One recorded look at the machine. Serializable in both directions on purpose: this is the
/// fixture format, and a fault observed on a desktop nobody on this project owns is a file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Observation {
    /// The OS the observation was *taken* on, not the one replaying it.
    pub os: Os,
    /// Version of the build that took it, for a record that outlives this release.
    #[serde(default)]
    pub version: String,
    pub database: DatabaseProbe,
    pub app: AppProbe,
    pub tray: TrayProbe,
    pub watcher: WatcherProbe,
    pub server: ServerProbe,
    pub consent: ConsentProbe,
    pub model: ModelProbe,
    pub voice: VoiceProbe,
    pub hotkey: HotkeyProbe,
    pub disk: DiskProbe,
    pub update: UpdateProbe,
}

/// The memory database: it opens, a write round-trips, and there is room under it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatabaseProbe {
    pub opened: bool,
    /// Whether a setting written by the probe read back with the same value. The only way to
    /// tell a read-only filesystem from a healthy one without waiting for a save to fail.
    pub write_round_trip: bool,
    pub error: Option<String>,
    pub path: Option<String>,
}

/// Facts only the desktop process can answer, and `None` everywhere else. `asked_in_app`
/// exists so a verdict can tell "nobody asked" from "asked, and the answer was no".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppProbe {
    pub asked_in_app: bool,
    pub hud_windows: Option<u32>,
    pub hud_responding: Option<bool>,
    /// Whether AETHER1 is holding a live Ollama child of its own.
    pub managed_ollama: Option<bool>,
    pub autostart_ollama: bool,
    pub hotkey_registered: Option<bool>,
}

/// Whether anything on this desktop can host a tray icon. On a bare window manager there is
/// no StatusNotifierItem host, the icon simply never appears, and that looks exactly like
/// AETHER1 failing to start.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrayProbe {
    /// None when this machine could not be asked -- no `gdbus`, or no session bus.
    pub host_present: Option<bool>,
    pub desktop: Option<String>,
}

/// The crash watcher: its reader exists, and it has swept recently.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WatcherProbe {
    pub enabled: bool,
    /// The reason the reader cannot work, or None when it can.
    pub unavailable: Option<String>,
    /// Seconds since the last sweep finished, or None when no watcher is running in this
    /// process -- which is every invocation from a terminal.
    pub last_sweep_secs_ago: Option<u64>,
}

/// The HTTP server, which is only meant to be up when somebody started it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerProbe {
    /// Whether anything answers on --serve's port. Probed by connecting, from any process.
    pub port_answers: bool,
    pub port: u16,
    /// Whether *this* process is the one serving, and how. None outside a --serve process.
    pub serving: Option<bool>,
    pub lan: Option<bool>,
    pub tls_fingerprint: Option<String>,
    pub mdns_announcing: Option<bool>,
}

/// The consent queue. Proposals expire after fifteen minutes by design, so a pile of expired
/// ones nobody ever answered means the card is not reaching the operator at all.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConsentProbe {
    pub pending: u32,
    pub expired: u32,
}

/// The model side, serving two checks: something answers, and the chosen model is there.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelProbe {
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    /// None when nothing answered at all, `Some(vec![])` when a server answered with an
    /// empty list. Step 19 insists on the difference and so does the verdict below.
    pub models: Option<Vec<String>>,
    pub ollama_cli_installed: bool,
    pub local_only: bool,
    /// A cloud provider needs no local server; the endpoint checks do not apply to it.
    pub cloud: bool,
}

/// Both halves of the voice, and the thing underneath both of them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoiceProbe {
    pub auto_speak: bool,
    pub chosen_engine: String,
    pub local_voice: String,
    /// The Piper failure, or None when Piper and its voice files are both there.
    pub piper_error: Option<String>,
    /// The OS voice failure (espeak-ng, or SAPI on Windows), or None when it works.
    pub os_voice_error: Option<String>,
    /// The webview cannot play what the engines produce -- the failure that looks like
    /// nothing at all. Carries the detail of `voice_setup::media_playback_step`.
    pub playback_problem: Option<String>,
    /// The command that fixes the playback problem, when there is one. Needs root, so it is
    /// handed over rather than run.
    pub playback_command: Option<String>,
    pub stt_model: String,
    pub stt_error: Option<String>,
}

/// The hotkey. Registration is an in-process fact; Wayland is a fact about the machine and
/// is readable from anywhere.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HotkeyProbe {
    pub chord: String,
    pub wayland: bool,
}

/// Room for what is configured but not yet downloaded.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiskProbe {
    pub free_gb: f64,
    /// What is still owed, and roughly how big: a model, a voice, an STT model.
    pub wanted: Vec<(String, f64)>,
}

/// The update path (step 46). `implemented` is false while that step is unbuilt, which is a
/// fact about this build rather than a fault on this machine.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateProbe {
    pub implemented: bool,
    pub signed_in: Option<bool>,
    pub declined: bool,
}

// ---------------------------------------------------------------------------------------
// The probe: the only part of this file that touches the world.
// ---------------------------------------------------------------------------------------

/// What the caller knows and this module cannot find out for itself. The desktop process
/// fills it in; a terminal invocation passes `Default`, and the verdicts say so rather than
/// guessing.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub in_app: bool,
    pub hud_windows: Option<u32>,
    pub hud_responding: Option<bool>,
    pub managed_ollama: Option<bool>,
    pub hotkey_registered: Option<bool>,
    pub serving: Option<bool>,
    pub lan: Option<bool>,
    pub mdns_announcing: Option<bool>,
    pub tls_fingerprint: Option<String>,
}

/// --serve's port, as `server::bind_address` spells it.
const SERVE_PORT: u16 = 8378;

/// Looks at everything, once. Costs a handful of process spawns and two short-timeout
/// connections, which is why nothing calls it on a timer.
pub fn observe(engine: &LlmEngine, facts: &Facts) -> Observation {
    let db = engine.db();
    let settings = |key: &str, default: &str| db.get_setting_string(key, default);

    let provider = settings("llm_provider", "offline");
    let endpoint = settings("llm_endpoint", "http://localhost:11434");
    let model = settings("llm_model", "");
    let cloud = !matches!(provider.as_str(), "offline" | "ollama" | "lmstudio");

    // One scan, read by both model checks. `models_at` is the narrower question and the one
    // that distinguishes "nothing answered" from "answered with nothing".
    let models = if cloud {
        None
    } else {
        crate::model_scanner::models_at(&endpoint)
    };

    let local_voice = settings("tts_local_voice", "");
    let piper_error = crate::llm::tts_local_status(Some(&local_voice)).err();
    let os_voice_error = crate::llm::tts_os_status().err();
    let playback = crate::voice_setup::media_playback_step();
    let stt_model = settings("stt_model_path", "");
    let stt_error = crate::llm::stt_local_status(Some(&stt_model)).err();

    let telemetry = crate::llm::Telemetry::snapshot();
    let free_gb = (telemetry.disk_total_gb - telemetry.disk_used_gb).max(0.0);

    let watcher_reader = crate::watchers::crash::reader_for_this_machine();
    let unavailable = match watcher_reader.availability() {
        crate::watchers::crash::Availability::Ready => None,
        crate::watchers::crash::Availability::Unavailable(reason) => Some(reason),
    };

    // Before the struct below, because `models` moves into it and this reads it: the one scan
    // answers both the model checks and "is there room for what is still to come?".
    let wanted = wanted_downloads(db, telemetry.ram_total_gb, models.as_deref());

    Observation {
        os: Os::current(),
        version: crate::APP_VERSION.to_string(),
        database: probe_database(db),
        app: AppProbe {
            asked_in_app: facts.in_app,
            hud_windows: facts.hud_windows,
            hud_responding: facts.hud_responding,
            managed_ollama: facts.managed_ollama,
            autostart_ollama: db.get_setting_bool("autostart_ollama", false),
            hotkey_registered: facts.hotkey_registered,
        },
        tray: probe_tray(),
        watcher: WatcherProbe {
            enabled: db.get_setting_bool(crate::watchers::crash::ENABLED_SETTING, true),
            unavailable,
            last_sweep_secs_ago: crate::watchers::crash::secs_since_last_sweep(),
        },
        server: ServerProbe {
            port: SERVE_PORT,
            port_answers: port_answers(SERVE_PORT),
            serving: facts.serving,
            lan: facts.lan,
            tls_fingerprint: facts.tls_fingerprint.clone(),
            mdns_announcing: facts.mdns_announcing,
        },
        consent: probe_consent(db),
        model: ModelProbe {
            provider,
            endpoint,
            model,
            models,
            ollama_cli_installed: which::which("ollama").is_ok(),
            local_only: crate::local_only::enabled(db),
            cloud,
        },
        voice: VoiceProbe {
            auto_speak: db.get_setting_bool("auto_speak", true),
            chosen_engine: settings("tts_engine", "auto"),
            local_voice,
            piper_error,
            os_voice_error,
            playback_problem: playback.as_ref().map(|step| step.detail.clone()),
            playback_command: playback.and_then(|step| step.command),
            stt_model,
            stt_error,
        },
        hotkey: HotkeyProbe {
            chord: settings("hotkey_toggle", crate::hotkey::DEFAULT_TOGGLE),
            wayland: crate::hotkey::is_wayland(),
        },
        disk: DiskProbe {
            free_gb: round1(free_gb),
            wanted,
        },
        update: UpdateProbe {
            implemented: false,
            signed_in: None,
            declined: false,
        },
    }
}

/// Opens the database and writes to it. The write is the point: a database that opens on a
/// read-only filesystem looks perfect until the first save.
fn probe_database(db: &MemoryDb) -> DatabaseProbe {
    const KEY: &str = "doctor_write_probe";
    let path = db.dir().map(|dir| dir.display().to_string());
    let stamp = json!(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default());
    match db.set_setting(KEY, &stamp) {
        Ok(()) => {
            let read_back = db.get_setting(KEY).ok().flatten();
            DatabaseProbe {
                opened: true,
                write_round_trip: read_back.as_ref() == Some(&stamp),
                error: None,
                path,
            }
        }
        Err(error) => DatabaseProbe {
            // The open itself is what set_setting does first, so a failure here cannot tell
            // the two apart; the message can, and it is the message an operator reads.
            opened: false,
            write_round_trip: false,
            error: Some(error.to_string()),
            path,
        },
    }
}

/// Whether this desktop has a tray host. Asked of `gdbus`, which ships with GLib and so is
/// present wherever a GTK webview is; a machine that cannot be asked returns None rather
/// than a guess, because sending someone to install a tray they already have is its own bug.
fn probe_tray() -> TrayProbe {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    if !cfg!(target_os = "linux") {
        // Windows and macOS always have somewhere to put it.
        return TrayProbe {
            host_present: Some(true),
            desktop,
        };
    }
    let mut cmd = std::process::Command::new("gdbus");
    cmd.args([
        "call",
        "--session",
        "--dest",
        "org.freedesktop.DBus",
        "--object-path",
        "/org/freedesktop/DBus",
        "--method",
        "org.freedesktop.DBus.NameHasOwner",
        "org.kde.StatusNotifierWatcher",
    ]);
    crate::paths::suppress_console_window(&mut cmd);
    let host_present = cmd.output().ok().and_then(|out| {
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        Some(text.contains("true"))
    });
    TrayProbe {
        host_present,
        desktop,
    }
}

fn probe_consent(db: &MemoryDb) -> ConsentProbe {
    // Expiring first is what the app does on every pass anyway; doing it here means the
    // count below is of proposals that are genuinely still answerable.
    let expired = db
        .expire_stale_proposals(crate::tools::consent::PROPOSAL_TTL_MINUTES)
        .unwrap_or(0) as u32;
    let pending = db.pending_actions().map(|p| p.len()).unwrap_or(0) as u32;
    ConsentProbe { pending, expired }
}

/// Whether anything is listening. A connect with a short timeout rather than a bind attempt:
/// binding to find out would fight the server it is asking about.
fn port_answers(port: u16) -> bool {
    use std::net::{Ipv4Addr, SocketAddr, TcpStream};
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&address, std::time::Duration::from_millis(300)).is_ok()
}

/// What is configured, absent, and therefore still to be downloaded -- with a rough size, so
/// "is there room?" can be answered before the download rather than half-way through it.
///
/// Rough on purpose. The real figure is the server's own Content-Length once bytes are moving;
/// what this needs is the difference between a gigabyte and five.
/// `installed` is the model list from the one scan `observe` already made -- passed in rather
/// than probed again, since asking the endpoint twice in one report is two of the process
/// spawns this whole module is careful about.
fn wanted_downloads(
    db: &MemoryDb,
    ram_total_gb: f64,
    installed: Option<&[String]>,
) -> Vec<(String, f64)> {
    let mut wanted = Vec::new();

    let provider = db.get_setting_string("llm_provider", "offline");
    let model = db.get_setting_string("llm_model", "");
    if matches!(provider.as_str(), "ollama" | "lmstudio") && !model.trim().is_empty() {
        let have = installed
            .unwrap_or_default()
            .iter()
            .any(|name| name.split(':').next() == model.split(':').next());
        if !have {
            // The wizard's own catalogue knows this one's size; a model from outside it gets
            // the middle of the range rather than a number invented per call.
            let size = crate::setup::models_for(ram_total_gb)
                .iter()
                .find(|choice| choice.name.split(':').next() == model.split(':').next())
                .and_then(|choice| parse_gb(&choice.download))
                .unwrap_or(5.0);
            wanted.push((format!("the model {model}"), size));
        }
    }

    let voice = db.get_setting_string("tts_local_voice", "");
    if !voice.trim().is_empty()
        && crate::llm::tts_local_status(Some(&voice)).is_err()
        && crate::voice_download::catalogue()
            .iter()
            .any(|entry| entry.name == voice.trim())
    {
        wanted.push((format!("the {voice} voice"), 0.06));
    }

    let stt_model = db.get_setting_string("stt_model_path", "");
    if crate::llm::stt_local_status(Some(&stt_model)).is_err() {
        wanted.push(("the speech recognizer's model".to_string(), 0.15));
    }

    wanted
}

/// The gigabytes out of a size hint like "about 4.7 GB" or "1.3 GB". None when the hint is
/// not in gigabytes, which is safer than reading "600 MB" as 600.
fn parse_gb(hint: &str) -> Option<f64> {
    let lowered = hint.to_ascii_lowercase();
    if !lowered.contains("gb") {
        return None;
    }
    lowered
        .split_whitespace()
        .find_map(|token| token.trim_end_matches("gb").parse::<f64>().ok())
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

// ---------------------------------------------------------------------------------------
// The verdicts: pure functions of the observation above. Nothing here touches the world.
// ---------------------------------------------------------------------------------------

/// One row of the report.
#[derive(Debug, Clone, Serialize)]
pub struct CheckReport {
    pub id: CheckId,
    pub key: &'static str,
    pub title: &'static str,
    pub owner: Owner,
    pub verdict: Verdict,
    /// What is true, in sentences. Always filled in, including when the verdict is Ok --
    /// "working" with no detail is the report people stop reading.
    pub detail: String,
    /// What the operator can do, when there is something. Rung 0 of the ladder.
    pub steps: Vec<Step>,
    /// The repair AETHER1 knows how to make, when it knows one. Rung 1. Naming it is not
    /// doing it: nothing here runs until `apply` is called with it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair: Option<Repair>,
}

/// The whole report.
#[derive(Debug, Clone, Serialize)]
pub struct Health {
    pub os: Os,
    pub headline: String,
    pub checks: Vec<CheckReport>,
    pub needs_attention: bool,
    /// Checks repaired so often that the repair is hiding a bug rather than fixing one.
    pub repeated: Vec<String>,
}

impl Health {
    pub fn worst(&self) -> Verdict {
        self.checks
            .iter()
            .map(|check| check.verdict)
            .max()
            .unwrap_or(Verdict::Ok)
    }

    pub fn check(&self, id: CheckId) -> Option<&CheckReport> {
        self.checks.iter().find(|check| check.id == id)
    }
}

/// The report, from an observation and nothing else. This is the function the tests drive and
/// the one `--replay` calls, and it must stay free of anything that reads the machine.
pub fn diagnose(obs: &Observation) -> Health {
    let checks: Vec<CheckReport> = CheckId::ALL
        .iter()
        .map(|id| {
            let (verdict, detail, steps) = judge(*id, obs);
            let repair = if matches!(verdict, Verdict::Degraded | Verdict::Failed) {
                repair_for(*id, obs)
            } else {
                None
            };
            CheckReport {
                id: *id,
                key: id.key(),
                title: id.title(),
                owner: id.owner(),
                verdict,
                detail,
                steps,
                repair,
            }
        })
        .collect();

    let failed = checks
        .iter()
        .filter(|c| c.verdict == Verdict::Failed)
        .count();
    let degraded = checks
        .iter()
        .filter(|c| c.verdict == Verdict::Degraded)
        .count();
    let headline = match (failed, degraded) {
        (0, 0) => "Everything AETHER1 needs is working.".to_string(),
        (0, n) => format!(
            "AETHER1 is working, with {n} thing{} not at its best.",
            plural(n)
        ),
        (1, _) => "One thing AETHER1 needs is broken.".to_string(),
        (n, _) => format!("{n} things AETHER1 needs are broken."),
    };

    Health {
        os: obs.os,
        headline,
        needs_attention: failed + degraded > 0,
        checks,
        repeated: Vec::new(),
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// The verdict table. One arm per check, each reading only its own part of the observation.
fn judge(id: CheckId, obs: &Observation) -> (Verdict, String, Vec<Step>) {
    match id {
        CheckId::Database => judge_database(&obs.database),
        CheckId::Hud => judge_hud(&obs.app),
        CheckId::Tray => judge_tray(&obs.tray, obs.os),
        CheckId::BackgroundServices => judge_services(&obs.app, &obs.model),
        CheckId::Watchers => judge_watcher(&obs.watcher),
        CheckId::Server => judge_server(&obs.server),
        CheckId::ConsentQueue => judge_consent(&obs.consent),
        CheckId::ModelEndpoint => judge_endpoint(&obs.model),
        CheckId::ConfiguredModel => judge_model(&obs.model),
        CheckId::VoiceOut => judge_voice_out(&obs.voice),
        CheckId::VoiceIn => judge_voice_in(&obs.voice),
        CheckId::Hotkey => judge_hotkey(&obs.hotkey, &obs.app),
        CheckId::DiskRoom => judge_disk(&obs.disk),
        CheckId::UpdatePath => judge_update(&obs.update),
    }
}

fn judge_database(probe: &DatabaseProbe) -> (Verdict, String, Vec<Step>) {
    if !probe.opened {
        let reason = probe.error.clone().unwrap_or_default();
        return (
            Verdict::Failed,
            format!(
                "The memory database would not open: {reason}. Conversations, settings and \
                 everything the vault indexes live in it, so almost nothing works until it does."
            ),
            vec![Step::say(
                "Check the folder it lives in",
                "The database sits under AETHER1's own data folder. A full disk, a folder that \
                 has become read-only, or a permission that changed will all produce this.",
            )],
        );
    }
    if !probe.write_round_trip {
        return (
            Verdict::Failed,
            "The memory database opens and will not accept a write. This is what a read-only \
             filesystem or a full disk looks like from in here, and it means nothing you \
             change is being kept."
                .to_string(),
            vec![Step::say(
                "Free some disk space, or fix the permissions",
                "Everything AETHER1 remembers is written to one file; while that write fails, \
                 settings appear to save and are gone at the next launch.",
            )],
        );
    }
    (
        Verdict::Ok,
        match &probe.path {
            Some(path) => format!("Opens, and a write read back correctly. It lives in {path}."),
            None => "Opens, and a write read back correctly.".to_string(),
        },
        Vec::new(),
    )
}

fn judge_hud(probe: &AppProbe) -> (Verdict, String, Vec<Step>) {
    if !probe.asked_in_app {
        return (
            Verdict::Unknown,
            "Only the running app can answer this, and this was asked from a terminal. Open \
             AETHER1 and press Diagnose in Settings to have the window asked about itself."
                .to_string(),
            Vec::new(),
        );
    }
    match (probe.hud_responding, probe.hud_windows) {
        (Some(true), _) => (
            Verdict::Ok,
            "The window exists and answered.".to_string(),
            Vec::new(),
        ),
        (_, Some(0)) => (
            Verdict::Failed,
            "AETHER1 is running with no window at all. The tray menu's Show is the way back; \
             if that does nothing, the webview failed to start."
                .to_string(),
            Vec::new(),
        ),
        _ => (
            Verdict::Failed,
            "The window is there and did not answer. Step 32's reload is the first thing to \
             try; a window that will not come back wants a restart."
                .to_string(),
            Vec::new(),
        ),
    }
}

fn judge_tray(probe: &TrayProbe, os: Os) -> (Verdict, String, Vec<Step>) {
    if os != Os::Linux {
        return (
            Verdict::Ok,
            "This desktop always has somewhere to put the icon.".to_string(),
            Vec::new(),
        );
    }
    match probe.host_present {
        Some(true) => (
            Verdict::Ok,
            "Something on this desktop is hosting tray icons.".to_string(),
            Vec::new(),
        ),
        Some(false) => (
            Verdict::Degraded,
            format!(
                "Nothing on this desktop hosts tray icons{}, so AETHER1's icon never appears. \
                 The app is running -- it just has nowhere to show itself, which looks \
                 identical to it having failed to start.",
                match &probe.desktop {
                    Some(desktop) if !desktop.is_empty() => format!(" ({desktop})"),
                    _ => String::new(),
                }
            ),
            vec![Step::say(
                "Summon it with the hotkey instead",
                "On a bare window manager, install a StatusNotifierItem host -- snixembed, \
                 stalonetray with a bridge, or your panel's tray module -- or use the hotkey \
                 and `aether1 toggle`, which do not need a tray at all.",
            )],
        ),
        None => (
            Verdict::Unknown,
            "This machine could not be asked: `gdbus` is missing, or there is no session bus \
             to ask. That is not a fault in itself -- it only means the tray cannot be \
             checked from here."
                .to_string(),
            Vec::new(),
        ),
    }
}

fn judge_services(app: &AppProbe, model: &ModelProbe) -> (Verdict, String, Vec<Step>) {
    if !app.autostart_ollama {
        return (
            Verdict::Ok,
            "Autostart is off, so AETHER1 is not meant to be running a model server of its \
             own. A server you started yourself is yours and is not accounted for here."
                .to_string(),
            Vec::new(),
        );
    }
    if !app.asked_in_app {
        return (
            Verdict::Unknown,
            "Autostart is on. Whether the child AETHER1 started is still alive is a fact about \
             the running app, and this was asked from a terminal."
                .to_string(),
            Vec::new(),
        );
    }
    match (app.managed_ollama, model.models.is_some()) {
        (Some(true), _) => (
            Verdict::Ok,
            "Autostart is on and the model server AETHER1 started is still running.".to_string(),
            Vec::new(),
        ),
        // Nothing of AETHER1's is running but something answers: the operator's own server
        // got there first, which is the documented "don't start a second one" case.
        (_, true) => (
            Verdict::Ok,
            "Autostart is on and a model server was already answering, so AETHER1 left it \
             alone rather than starting a second one."
                .to_string(),
            Vec::new(),
        ),
        _ => (
            Verdict::Failed,
            "Autostart is on, the server AETHER1 started is gone, and nothing answers in its \
             place. Every reply is coming from offline standby until something does."
                .to_string(),
            Vec::new(),
        ),
    }
}

fn judge_watcher(probe: &WatcherProbe) -> (Verdict, String, Vec<Step>) {
    if !probe.enabled {
        return (
            Verdict::Ok,
            "Crash capture is switched off, so there is nothing to be watching.".to_string(),
            Vec::new(),
        );
    }
    if let Some(reason) = &probe.unavailable {
        return (
            Verdict::Degraded,
            format!(
                "Crash capture is on and cannot work on this machine: {reason} Crashes will \
                 happen and go unmentioned."
            ),
            vec![Step::say(
                "Install what reads the crash records",
                "The reason above names it. Until then `aether1 crashes` has nothing to read \
                 and the tray will never go amber.",
            )],
        );
    }
    match probe.last_sweep_secs_ago {
        None => (
            Verdict::Unknown,
            "The reader works. Whether the poll is ticking is a fact about the running app, \
             and no watcher runs in a terminal invocation."
                .to_string(),
            Vec::new(),
        ),
        // Four poll intervals. One missed tick is scheduling; a minute and a half of them is
        // a thread that has stopped.
        Some(secs) if secs > 4 * crate::watchers::crash::POLL_INTERVAL.as_secs() => (
            Verdict::Degraded,
            format!(
                "The reader works and the last sweep was {secs} seconds ago, which is several \
                 intervals late. The watcher thread has most likely stopped."
            ),
            Vec::new(),
        ),
        Some(secs) => (
            Verdict::Ok,
            format!("Watching, and the last sweep was {secs} seconds ago."),
            Vec::new(),
        ),
    }
}

fn judge_server(probe: &ServerProbe) -> (Verdict, String, Vec<Step>) {
    match probe.serving {
        None => {
            if probe.port_answers {
                (
                    Verdict::Ok,
                    format!(
                        "Something is answering on port {}, so an AETHER1 server is up. It was \
                         started elsewhere, so this check cannot say anything about its \
                         certificate or its LAN announcement.",
                        probe.port
                    ),
                    Vec::new(),
                )
            } else {
                (
                    Verdict::Ok,
                    format!(
                        "Nothing is listening on port {}, which is right unless you meant to \
                         run `aether1 --serve`.",
                        probe.port
                    ),
                    Vec::new(),
                )
            }
        }
        Some(false) => (
            Verdict::Ok,
            "This copy was not started as a server.".to_string(),
            Vec::new(),
        ),
        Some(true) => {
            let mut problems = Vec::new();
            if !probe.port_answers {
                problems.push(format!("nothing answers on port {}", probe.port));
            }
            if probe.lan == Some(true) {
                if probe.tls_fingerprint.as_deref().unwrap_or("").is_empty() {
                    problems.push("the TLS certificate did not load".to_string());
                }
                if probe.mdns_announcing == Some(false) {
                    problems.push("mDNS is not announcing this machine".to_string());
                }
            }
            if problems.is_empty() {
                (
                    Verdict::Ok,
                    format!("Serving on port {}, and answering.", probe.port),
                    Vec::new(),
                )
            } else {
                (
                    Verdict::Failed,
                    format!("Serving was asked for and {}.", problems.join("; and ")),
                    Vec::new(),
                )
            }
        }
    }
}

fn judge_consent(probe: &ConsentProbe) -> (Verdict, String, Vec<Step>) {
    // One expired proposal is ordinary -- somebody walked away from a card. A pile of them
    // means the card is not reaching the operator at all, which is a fault in AETHER1.
    if probe.expired >= 3 {
        return (
            Verdict::Degraded,
            format!(
                "{} proposals expired without ever being answered. Either nobody is seeing the \
                 approval cards, or something is proposing work nobody asked for.",
                probe.expired
            ),
            vec![Step::say(
                "Look at the action log",
                "Settings has the recent actions. A run of expired proposals from one tool is \
                 the shape of a loop; a run from several is a card that is not being shown.",
            )],
        );
    }
    let detail = match (probe.pending, probe.expired) {
        (0, 0) => "Nothing is waiting to be approved.".to_string(),
        (pending, 0) => format!(
            "{pending} thing{} waiting for you to approve or decline.",
            plural(pending as usize)
        ),
        (pending, expired) => format!(
            "{pending} waiting, and {expired} expired unanswered just now, which is ordinary \
             if you were away from the machine."
        ),
    };
    (Verdict::Ok, detail, Vec::new())
}

fn judge_endpoint(probe: &ModelProbe) -> (Verdict, String, Vec<Step>) {
    if probe.provider == "offline" {
        return (
            Verdict::Degraded,
            "No model is configured, so every reply is canned offline standby text. Settings -> \
             The Brain walks through it in five steps."
                .to_string(),
            vec![Step::say(
                "Finish the setup wizard",
                "It reads this machine and says what the one next thing is, whether that is \
                 installing Ollama, starting it, or downloading a model.",
            )],
        );
    }
    if probe.cloud {
        return (
            Verdict::Ok,
            format!(
                "{} is a cloud provider, so there is no local server for this check to look \
                 for. Whether the key works is what a failed reply tells you.",
                probe.provider
            ),
            Vec::new(),
        );
    }
    match &probe.models {
        // Step 19's distinction, kept: a silent endpoint and an endpoint with an empty list
        // are different faults with different fixes, and merging them is how someone ends up
        // downloading a model onto a machine whose server is not running.
        None => (
            Verdict::Failed,
            format!(
                "Nothing answered at {}. The program that serves the model is not running.",
                probe.endpoint
            ),
            vec![Step::run(
                "Start the model server",
                "Nothing is listening, so there is nothing to ask. This starts it in the \
                 foreground; leave the terminal open.",
                "ollama serve",
            )],
        ),
        Some(models) if models.is_empty() => (
            Verdict::Degraded,
            format!(
                "A server answered at {} and has no models. It is running and has nothing to \
                 run.",
                probe.endpoint
            ),
            vec![Step::run(
                "Download a model",
                "The setup wizard lists the ones this machine has the memory for, with a \
                 recommendation.",
                "ollama pull llama3.2",
            )],
        ),
        Some(models) => (
            Verdict::Ok,
            format!(
                "{} answered with {} model{}.",
                probe.endpoint,
                models.len(),
                plural(models.len())
            ),
            Vec::new(),
        ),
    }
}

fn judge_model(probe: &ModelProbe) -> (Verdict, String, Vec<Step>) {
    if probe.provider == "offline" {
        return (
            Verdict::Ok,
            "Nothing is configured to be installed yet.".to_string(),
            Vec::new(),
        );
    }
    if probe.cloud {
        return (
            Verdict::Ok,
            format!(
                "{} runs on {}'s machines, not this one.",
                probe.model, probe.provider
            ),
            Vec::new(),
        );
    }
    let Some(models) = &probe.models else {
        return (
            Verdict::Unknown,
            "Nothing answered, so this machine could not be asked what it has installed. Fix \
             the endpoint above first."
                .to_string(),
            Vec::new(),
        );
    };
    if probe.model.trim().is_empty() {
        return (
            Verdict::Degraded,
            "A provider is set and no model is chosen. Settings -> The Brain has the list."
                .to_string(),
            Vec::new(),
        );
    }
    // The same forgiving match routing uses: `llama3.2` and `llama3.2:latest` are one model
    // wearing two names, and reporting them as different is how a working setup gets called
    // broken.
    let installed = models.iter().any(|name| {
        name == &probe.model
            || name.split(':').next() == Some(probe.model.as_str())
            || probe.model.split(':').next() == Some(name.as_str())
    });
    if installed {
        (
            Verdict::Ok,
            format!("{} is installed and is what answers.", probe.model),
            Vec::new(),
        )
    } else {
        (
            Verdict::Degraded,
            format!(
                "{} is chosen and is not installed. Routing falls back to one that is, so \
                 replies are coming from a model you did not pick.",
                probe.model
            ),
            vec![Step::run(
                "Download the model you chose",
                "Or pick one of the installed ones in Settings -> The Brain, which costs no \
                 download at all.",
                &format!("ollama pull {}", probe.model),
            )],
        )
    }
}

fn judge_voice_out(probe: &VoiceProbe) -> (Verdict, String, Vec<Step>) {
    if !probe.auto_speak {
        return (
            Verdict::Ok,
            "Replies are not spoken, so there is nothing for this to check.".to_string(),
            Vec::new(),
        );
    }
    // Asked before the engines, because it outranks them: on a machine that cannot play
    // audio at all, every engine below reports success while the operator hears nothing.
    if let Some(problem) = &probe.playback_problem {
        let mut steps = Vec::new();
        if let Some(command) = &probe.playback_command {
            steps.push(Step::run("Install the audio decoders", problem, command));
        }
        return (
            Verdict::Failed,
            format!("Speech is produced and cannot be heard. {problem}"),
            steps,
        );
    }
    match (&probe.piper_error, &probe.os_voice_error) {
        (None, _) => (
            Verdict::Ok,
            format!(
                "Piper is installed with its voice files, so speech is local and offline{}.",
                if probe.local_voice.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", probe.local_voice)
                }
            ),
            Vec::new(),
        ),
        (Some(piper), None) => (
            Verdict::Degraded,
            format!(
                "Piper is not usable ({piper}), so the operating system's own voice is doing \
                 the speaking. It works and it sounds like a robot."
            ),
            vec![Step::say(
                "Download a Piper voice",
                "Settings -> Voice & Sound has the catalogue; each voice is about 60 MB and \
                 stays on this machine.",
            )],
        ),
        (Some(piper), Some(os_voice)) => (
            Verdict::Failed,
            format!("Nothing can speak. Piper: {piper}. The system voice: {os_voice}"),
            vec![Step::say(
                "Install one speech engine",
                "Either is enough: a Piper voice from Settings -> Voice & Sound for the good \
                 one, or your distribution's espeak-ng package for the plain one.",
            )],
        ),
    }
}

fn judge_voice_in(probe: &VoiceProbe) -> (Verdict, String, Vec<Step>) {
    match &probe.stt_error {
        None => (
            Verdict::Ok,
            "The recognizer and its model are both here, so push-to-talk works offline."
                .to_string(),
            Vec::new(),
        ),
        Some(error) => (
            Verdict::Degraded,
            format!(
                "Push-to-talk cannot work: {error} Typing is unaffected; only the microphone \
                 is."
            ),
            vec![Step::say(
                "Set up the recognizer",
                "Settings -> Voice & Sound walks through it, including the Python environment \
                 AETHER1 owns rather than touching the system one.",
            )],
        ),
    }
}

fn judge_hotkey(probe: &HotkeyProbe, app: &AppProbe) -> (Verdict, String, Vec<Step>) {
    if probe.chord.trim().is_empty() {
        return (
            Verdict::Ok,
            "No hotkey is set, which is a choice rather than a fault.".to_string(),
            Vec::new(),
        );
    }
    // Wayland first, and reported as broken even when registration succeeded: the registration
    // does succeed, and the key then never fires. Reporting that as healthy is the worst of
    // the available answers.
    if probe.wayland {
        return (
            Verdict::Degraded,
            format!(
                "This is a Wayland session, where no application can grab keys system-wide. \
                 {} is registered and will never fire.",
                probe.chord
            ),
            vec![Step::run(
                "Bind the compositor instead",
                "Your compositor's own keybinding can run this, and that is the supported \
                 route on Wayland -- GNOME's Settings -> Keyboard, or a `bind` line in \
                 Hyprland or Sway.",
                "aether1 toggle",
            )],
        );
    }
    match app.hotkey_registered {
        Some(true) => (
            Verdict::Ok,
            format!("{} is registered.", probe.chord),
            Vec::new(),
        ),
        Some(false) => (
            Verdict::Degraded,
            format!(
                "{} could not be registered -- something else on this desktop has it. \
                 `aether1 toggle` does the same thing from a terminal or a launcher.",
                probe.chord
            ),
            Vec::new(),
        ),
        None => (
            Verdict::Unknown,
            format!(
                "{} is the chord that is set. Whether it registered is a fact about the \
                 running app, and this was asked from a terminal.",
                probe.chord
            ),
            Vec::new(),
        ),
    }
}

fn judge_disk(probe: &DiskProbe) -> (Verdict, String, Vec<Step>) {
    let owed: f64 = probe.wanted.iter().map(|(_, gb)| gb).sum();
    if owed <= 0.0 {
        return (
            Verdict::Ok,
            format!(
                "{:.1} GB free, and nothing configured is waiting to be downloaded.",
                probe.free_gb
            ),
            Vec::new(),
        );
    }
    let what = probe
        .wanted
        .iter()
        .map(|(name, gb)| format!("{name} (about {gb:.1} GB)"))
        .collect::<Vec<_>>()
        .join(", ");
    if probe.free_gb < owed {
        (
            Verdict::Failed,
            format!(
                "{:.1} GB free and about {owed:.1} GB still to download: {what}. The download \
                 will fail part-way, which is the state that looks like a corrupt file.",
                probe.free_gb
            ),
            vec![Step::say(
                "Free some space first",
                "A part-downloaded model or voice reads as broken rather than as missing, so \
                 this is worth doing before starting the download rather than after.",
            )],
        )
    } else {
        (
            Verdict::Ok,
            format!(
                "{:.1} GB free, enough for what is still to come: {what}.",
                probe.free_gb
            ),
            Vec::new(),
        )
    }
}

fn judge_update(probe: &UpdateProbe) -> (Verdict, String, Vec<Step>) {
    if !probe.implemented {
        return (
            Verdict::Ok,
            "This build has no update path yet (step 46), so there is nothing here that could \
             be broken and nothing that will nag you."
                .to_string(),
            Vec::new(),
        );
    }
    match (probe.signed_in, probe.declined) {
        (Some(true), _) => (
            Verdict::Ok,
            "Signed in, so updates can be checked for.".to_string(),
            Vec::new(),
        ),
        (_, true) => (
            Verdict::Ok,
            "You declined the update sign-in, and that answer is being kept.".to_string(),
            Vec::new(),
        ),
        _ => (
            Verdict::Degraded,
            "Updates cannot be checked for until you sign in, which is offered once in \
             Settings and never again unless you ask."
                .to_string(),
            Vec::new(),
        ),
    }
}

// ---------------------------------------------------------------------------------------
// Rung 1: the repairs written by hand, each one asked for at the moment of acting.
// ---------------------------------------------------------------------------------------

/// A repair AETHER1 knows how to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepairId {
    /// Start the local model server, for a machine that has the program and is not running it.
    StartModelServer,
    /// Download the model the settings name.
    PullConfiguredModel,
    /// Fetch the Piper voice that is configured and absent -- or the default one, when none
    /// is configured, since the catalogue is a fixed table either way.
    FetchVoice,
    /// Expire and clear the proposals nobody answered.
    ClearStaleProposals,
    /// Re-register the global hotkey. Needs the running app; a terminal cannot.
    ReregisterHotkey,
    /// Install the GStreamer decoders the webview needs. Needs root, so it is handed over.
    InstallAudioDecoders,
}

/// What kind of act a repair is, which decides who does it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RepairKind {
    /// AETHER1 can do this itself once approved: its own files, its own child processes, its
    /// own database.
    Run,
    /// Only the running desktop app can do it -- it needs a handle a terminal does not have.
    InApp,
    /// Needs root, so AETHER1 never runs it. **This is the line this module does not cross.**
    /// An app that can install packages is an app that holds your password, and no diagnostic
    /// is worth that. The command is exact, it is shown, and the operator runs it.
    HandOver { command: String },
}

/// A proposed repair. Constructing one changes nothing; `apply` is what acts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repair {
    pub id: RepairId,
    /// The imperative, short enough to be a button.
    pub title: String,
    /// Exactly what will happen if it is approved, in sentences. This is what the operator
    /// is agreeing to, so it says the command or the file, not "fix it".
    pub detail: String,
    pub kind: RepairKind,
}

/// The repair table: check plus observation in, at most one repair out. Pure, so a report can
/// say what it *would* try without trying anything, and so the table is testable.
///
/// Deterministic and hand-written on purpose. These need no model at all, which is the whole
/// point: the machine whose model endpoint is down is exactly the machine that cannot ask a
/// model what to do about it.
pub fn repair_for(id: CheckId, obs: &Observation) -> Option<Repair> {
    match id {
        CheckId::ModelEndpoint if obs.model.models.is_none() && obs.model.ollama_cli_installed => {
            Some(Repair {
                id: RepairId::StartModelServer,
                title: "Start the model server".to_string(),
                detail: format!(
                    "Runs `ollama serve` in the background, on this machine, and waits for it \
                     to answer at {}. Nothing is downloaded and nothing leaves the machine.",
                    obs.model.endpoint
                ),
                kind: RepairKind::Run,
            })
        }
        CheckId::ConfiguredModel
            if !obs.model.model.trim().is_empty()
                && obs.model.models.is_some()
                && !obs.model.cloud =>
        {
            Some(Repair {
                id: RepairId::PullConfiguredModel,
                title: format!("Download {}", obs.model.model),
                detail: format!(
                    "Runs `ollama pull {}`. This is a download of several gigabytes from \
                     Ollama's registry to this machine, and it will take a while.",
                    obs.model.model
                ),
                kind: RepairKind::Run,
            })
        }
        CheckId::VoiceOut => {
            // The playback failure comes first, because it is the one that outranks the
            // engines -- and it is the one AETHER1 must not fix itself.
            if let Some(command) = &obs.voice.playback_command {
                return Some(Repair {
                    id: RepairId::InstallAudioDecoders,
                    title: "Install the audio decoders".to_string(),
                    detail: format!(
                        "This one AETHER1 will not run for you: it needs root, and AETHER1 does \
                         not ask for your password. Run it yourself, then restart AETHER1:\n    \
                         {command}"
                    ),
                    kind: RepairKind::HandOver {
                        command: command.clone(),
                    },
                });
            }
            if obs.voice.piper_error.is_some() {
                let voice = default_voice_name(&obs.voice.local_voice);
                return Some(Repair {
                    id: RepairId::FetchVoice,
                    title: format!("Download the {voice} voice"),
                    detail: format!(
                        "Fetches the two files Piper needs for {voice} (about 60 MB) into \
                         AETHER1's own voices folder. The name comes from a fixed catalogue, \
                         so nothing typed anywhere becomes part of a URL."
                    ),
                    kind: RepairKind::Run,
                });
            }
            None
        }
        CheckId::ConsentQueue if obs.consent.expired >= 3 => Some(Repair {
            id: RepairId::ClearStaleProposals,
            title: "Clear the stale proposals".to_string(),
            detail: "Marks the proposals that expired unanswered as expired and leaves the \
                     action log's record of them. Nothing they proposed is run."
                .to_string(),
            kind: RepairKind::Run,
        }),
        CheckId::Hotkey if !obs.hotkey.wayland && obs.app.hotkey_registered == Some(false) => {
            Some(Repair {
                id: RepairId::ReregisterHotkey,
                title: "Try the hotkey again".to_string(),
                detail: format!(
                    "Unregisters and re-registers {}. Worth one try: whatever held the chord \
                     may have let go since AETHER1 started.",
                    obs.hotkey.chord
                ),
                kind: RepairKind::InApp,
            })
        }
        _ => None,
    }
}

/// The configured Piper voice, or the one the catalogue leads with when none is set. Named
/// here rather than inside the repair so the table above stays a pure function.
fn default_voice_name(configured: &str) -> String {
    if !configured.trim().is_empty() {
        return configured.trim().to_string();
    }
    crate::voice_download::catalogue()
        .first()
        .map(|voice| voice.name.to_string())
        .unwrap_or_else(|| "en_GB-alba-medium".to_string())
}

/// What a repair attempt did, and -- because this is the part that makes it a repair rather
/// than a hope -- what the check said afterwards.
#[derive(Debug, Clone, Serialize)]
pub struct RepairOutcome {
    pub check: &'static str,
    pub repair: RepairId,
    /// Whether the act itself succeeded.
    pub ran: bool,
    pub message: String,
    /// The verdict from a fresh probe, after the repair. `None` only when the repair never
    /// ran at all.
    pub rechecked: Option<Verdict>,
    /// The re-check's detail, so the caller can say what is still wrong.
    pub recheck_detail: Option<String>,
    /// The action-log row this attempt was recorded as.
    pub record: Option<i64>,
}

/// Rung 3, kept honestly: one repair of each check per session, then it stops. A loop that
/// keeps trying is worse than a broken check, because it burns the operator's attention and
/// they stop reading.
static ATTEMPTS: Mutex<Option<HashMap<(CheckId, RepairId), u32>>> = Mutex::new(None);

fn attempts_this_session(key: (CheckId, RepairId)) -> u32 {
    let mut guard = ATTEMPTS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    *map.get(&key).unwrap_or(&0)
}

fn record_attempt_in_session(key: (CheckId, RepairId)) {
    let mut guard = ATTEMPTS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    *map.entry(key).or_insert(0) += 1;
}

/// Forgets this session's attempts. Tests only -- a real session never wants this, since
/// forgetting is exactly how the loop rung 3 prevents gets back in.
#[cfg(test)]
fn forget_attempts() {
    let mut guard = ATTEMPTS.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

/// The tool name every repair is recorded under in the action log, so the whole history of
/// AETHER1 repairing itself is one query.
pub const REPAIR_TOOL: &str = "doctor.repair";

/// Makes a repair, having been told to.
///
/// **This function is the go-ahead.** It does not ask -- the caller asked, and calling this
/// is what "yes" means: the CLI only reaches it after a typed y on an interactive terminal,
/// and the HUD only after the button in the Diagnostics panel. Nothing calls it on a
/// schedule, and nothing calls it in a loop: `attempts` stops the second try.
///
/// Every attempt is recorded, succeeded or failed, because knowing that a plausible fix does
/// not work is worth as much as knowing one does.
/// A repair only the running desktop app can make, handed in as a closure by the one caller
/// that holds what it needs (an `AppHandle`, for the hotkey). Everything else about the ladder
/// stays here, including the record and the re-check, so an in-app repair is not a second path
/// with its own rules.
pub type InAppRepair<'a> = &'a (dyn Fn(RepairId) -> Result<String, String> + 'a);

/// `apply`, for callers with nothing in-app to offer. See `apply_with`.
pub fn apply(
    engine: &LlmEngine,
    check: CheckId,
    repair: RepairId,
) -> Result<RepairOutcome, String> {
    apply_with(engine, check, repair, None)
}

pub fn apply_with(
    engine: &LlmEngine,
    check: CheckId,
    repair: RepairId,
    in_app: Option<InAppRepair>,
) -> Result<RepairOutcome, String> {
    let key = (check, repair);
    if attempts_this_session(key) > 0 {
        return Err(format!(
            "{} has already been repaired once this session, and repeating it is not a repair. \
             Restart AETHER1 to try again, and if it comes back it is a bug worth reporting.",
            check.title()
        ));
    }

    // Recorded before it runs, not after: a repair that crashes the app half-way is exactly
    // the one worth having a record of.
    let record = engine
        .db()
        .log_action(
            REPAIR_TOOL,
            &json!({ "check": check.key(), "repair": repair, "os": Os::current() }),
            true,
            ActionStatus::Proposed,
            Some(&format!("Repairing {}", check.title())),
        )
        .ok();
    record_attempt_in_session(key);

    let ran = run_repair(engine, repair, in_app);

    // The re-check is mandatory. A repair whose verdict is unchanged is a failed repair,
    // whatever its own exit code said.
    let after = diagnose(&observe(engine, &Facts::default()));
    let rechecked = after.check(check).map(|c| c.verdict);
    let recheck_detail = after.check(check).map(|c| c.detail.clone());

    let (status, message) = match &ran {
        Ok(message) => (
            match rechecked {
                Some(Verdict::Ok) => ActionStatus::Executed,
                // Ran, and the thing it was meant to fix is still broken. Recording that as
                // an execution is how a repair table rots into a pile of workarounds.
                _ => ActionStatus::Failed,
            },
            message.clone(),
        ),
        Err(error) => (ActionStatus::Failed, error.clone()),
    };

    if let (Some(id), db) = (record, engine.db()) {
        let result = match rechecked {
            Some(verdict) => format!("{message} -- afterwards: {}", verdict.mark()),
            None => message.clone(),
        };
        let _ = db.set_action_outcome(id, status, Some(&result), None, Some("operator"));
    }

    Ok(RepairOutcome {
        check: check.key(),
        repair,
        ran: ran.is_ok(),
        message,
        rechecked,
        recheck_detail,
        record,
    })
}

/// The repairs themselves. Each one is an existing capability of this app called with no
/// choices left to make -- which is what keeps them deterministic and model-free.
fn run_repair(
    engine: &LlmEngine,
    repair: RepairId,
    in_app: Option<InAppRepair>,
) -> Result<String, String> {
    match repair {
        RepairId::StartModelServer => {
            let result = crate::commands::start_local_server(engine);
            let message = result
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("started")
                .to_string();
            if result.get("success").and_then(|s| s.as_bool()) == Some(false) {
                Err(message)
            } else {
                Ok(message)
            }
        }
        RepairId::PullConfiguredModel => {
            let model = engine.db().get_setting_string("llm_model", "");
            if model.trim().is_empty() {
                return Err("no model is configured to download".to_string());
            }
            let result = crate::commands::pull_model(engine, model);
            match result.status {
                crate::model_scanner::PullStatus::Error => Err(result.message),
                _ => Ok(result.message),
            }
        }
        RepairId::FetchVoice => {
            let configured = engine.db().get_setting_string("tts_local_voice", "");
            let voice = default_voice_name(&configured);
            crate::voice_download::start(engine.db(), &voice)
                .map(|fetch| format!("downloading {} -- {}", fetch.voice, fetch.detail))
        }
        RepairId::ClearStaleProposals => {
            let cleared = engine
                .db()
                .expire_stale_proposals(crate::tools::consent::PROPOSAL_TTL_MINUTES)
                .map_err(|e| e.to_string())?;
            Ok(format!("expired {cleared} stale proposal(s)"))
        }
        // Runnable only where there is an app to run it: the HUD hands in the closure, and a
        // terminal has nothing to hand. The CLI does not even offer it -- see how
        // RepairKind::InApp is rendered there.
        RepairId::ReregisterHotkey => match in_app {
            Some(act) => act(repair),
            None => Err(
                "the global hotkey can only be re-registered by the running app -- open AETHER1 \
                 and press this in Settings -> Diagnostics"
                    .to_string(),
            ),
        },
        RepairId::InstallAudioDecoders => Err(
            "AETHER1 does not run commands that need root. The command is in the report; run it \
             yourself and restart AETHER1."
                .to_string(),
        ),
    }
}

/// Checks that have been repaired so often that the repair is suppressing a defect rather
/// than fixing one. Read from the action log, so it spans sessions -- which is the only way
/// to see a check that is repaired cleanly on every single launch.
///
/// Three, because two is a coincidence and a thing that needs fixing every time you start
/// the program is not healed, it is hidden.
pub fn repeated_repairs(db: &MemoryDb) -> Vec<String> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for record in db.recent_actions(200).unwrap_or_default() {
        if record.tool != REPAIR_TOOL {
            continue;
        }
        if let Some(check) = record.args.get("check").and_then(|c| c.as_str()) {
            *counts.entry(check.to_string()).or_insert(0) += 1;
        }
    }
    let mut repeated: Vec<String> = counts
        .into_iter()
        .filter(|(_, count)| *count >= 3)
        .map(|(check, count)| {
            format!(
                "{check} has been repaired {count} times. A check that needs repairing every \
                 time is a defect being suppressed, not a defect being fixed -- worth \
                 reporting as its own bug."
            )
        })
        .collect();
    repeated.sort();
    repeated
}

/// The whole report, with the cross-session finding above folded in. This is what the CLI and
/// the HUD both call.
pub fn report(engine: &LlmEngine, facts: &Facts) -> (Observation, Health) {
    let observation = observe(engine, facts);
    let mut health = diagnose(&observation);
    health.repeated = repeated_repairs(engine.db());
    if !health.repeated.is_empty() {
        health.needs_attention = true;
    }
    (observation, health)
}

/// A report to hand to a person, with the observation that produced it.
///
/// **It goes nowhere.** Step 47 wants a fault that looks like a bug rather than a missing
/// dependency to be offered to the developers as an issue on the repository, and step 46's
/// GitHub identity does not exist yet -- and would need `issues: write` added to an App
/// scoped `contents: read` before it could. Until then this writes the same bundle to a file
/// the operator can read, edit and paste, which honours the part of that design that matters
/// most: nothing about this machine leaves it unseen.
pub fn bug_report(observation: &Observation, health: &Health) -> String {
    let mut out = String::new();
    out.push_str("AETHER1 self-diagnosis\n");
    out.push_str(&format!("Version: {}\n", observation.version));
    out.push_str(&format!("OS: {:?}\n\n", observation.os));
    out.push_str(&format!("{}\n\n", health.headline));
    for check in &health.checks {
        if check.verdict == Verdict::Ok {
            continue;
        }
        out.push_str(&format!(
            "[{}] {} ({})\n    {}\n",
            check.verdict.mark(),
            check.title,
            check.key,
            check.detail
        ));
    }
    for line in &health.repeated {
        out.push_str(&format!("\n[!!] {line}\n"));
    }
    out.push_str(
        "\nThe observation below is what the checks read. It replays anywhere with\n\
         `aether1 doctor --replay <this file>`, which is what turns a fault on one\n\
         machine into a test on a machine that never saw it. Read it before you send it\n\
         anywhere: it names paths, a model and the programs installed here.\n\n",
    );
    out.push_str(&serde_json::to_string_pretty(observation).unwrap_or_else(|e| format!("({e})")));
    out.push('\n');
    out
}

/// Reads an observation recorded elsewhere. The other half of `--replay`, and the reason the
/// probe structs derive `Deserialize`.
pub fn replay(json: &str) -> Result<Health, String> {
    let observation: Observation = serde_json::from_str(json)
        .map_err(|e| format!("that is not an observation this build can read: {e}"))?;
    Ok(diagnose(&observation))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine where everything works, as the baseline every case below deviates from by
    /// exactly one field. Written by hand rather than probed, which is the entire point.
    fn healthy() -> Observation {
        Observation {
            os: Os::Linux,
            version: "Ver 0.4.0".to_string(),
            database: DatabaseProbe {
                opened: true,
                write_round_trip: true,
                error: None,
                path: Some("/home/someone/.local/share/aether1".to_string()),
            },
            app: AppProbe {
                asked_in_app: true,
                hud_windows: Some(1),
                hud_responding: Some(true),
                managed_ollama: Some(true),
                autostart_ollama: true,
                hotkey_registered: Some(true),
            },
            tray: TrayProbe {
                host_present: Some(true),
                desktop: Some("KDE".to_string()),
            },
            watcher: WatcherProbe {
                enabled: true,
                unavailable: None,
                last_sweep_secs_ago: Some(5),
            },
            server: ServerProbe {
                port: 8378,
                port_answers: false,
                serving: Some(false),
                lan: None,
                tls_fingerprint: None,
                mdns_announcing: None,
            },
            consent: ConsentProbe {
                pending: 0,
                expired: 0,
            },
            model: ModelProbe {
                provider: "ollama".to_string(),
                endpoint: "http://localhost:11434".to_string(),
                model: "llama3.2".to_string(),
                models: Some(vec!["llama3.2:latest".to_string()]),
                ollama_cli_installed: true,
                local_only: false,
                cloud: false,
            },
            voice: VoiceProbe {
                auto_speak: true,
                chosen_engine: "auto".to_string(),
                local_voice: "en_GB-alba-medium".to_string(),
                piper_error: None,
                os_voice_error: None,
                playback_problem: None,
                playback_command: None,
                stt_model: "base.en".to_string(),
                stt_error: None,
            },
            hotkey: HotkeyProbe {
                chord: "Super+Shift+A".to_string(),
                wayland: false,
            },
            disk: DiskProbe {
                free_gb: 80.0,
                wanted: Vec::new(),
            },
            update: UpdateProbe {
                implemented: false,
                signed_in: None,
                declined: false,
            },
        }
    }

    fn verdict(obs: &Observation, id: CheckId) -> Verdict {
        diagnose(obs)
            .check(id)
            .expect("every check reports")
            .verdict
    }

    #[test]
    fn a_healthy_machine_reports_nothing_to_do() {
        let health = diagnose(&healthy());
        assert_eq!(health.worst(), Verdict::Ok, "{:#?}", health.checks);
        assert!(!health.needs_attention);
        assert_eq!(health.checks.len(), CheckId::ALL.len());
        // Every row says something, including the ones that passed: a report of bare ticks
        // is a report nobody reads twice.
        assert!(health.checks.iter().all(|check| !check.detail.is_empty()));
    }

    #[test]
    fn a_database_that_will_not_take_a_write_is_a_failure_not_a_warning() {
        let mut obs = healthy();
        obs.database.write_round_trip = false;
        assert_eq!(verdict(&obs, CheckId::Database), Verdict::Failed);
    }

    #[test]
    fn facts_only_the_app_knows_are_unknown_from_a_terminal_rather_than_ok() {
        let mut obs = healthy();
        obs.app.asked_in_app = false;
        obs.app.hud_responding = None;
        obs.app.hud_windows = None;
        obs.app.managed_ollama = None;
        obs.app.hotkey_registered = None;
        obs.watcher.last_sweep_secs_ago = None;
        for id in [
            CheckId::Hud,
            CheckId::BackgroundServices,
            CheckId::Watchers,
            CheckId::Hotkey,
        ] {
            assert_eq!(verdict(&obs, id), Verdict::Unknown, "{}", id.key());
        }
        // And an Unknown is never something to repair: there is nothing established to fix.
        assert!(diagnose(&obs)
            .checks
            .iter()
            .filter(|check| check.verdict == Verdict::Unknown)
            .all(|check| check.repair.is_none()));
    }

    #[test]
    fn a_silent_endpoint_and_an_empty_one_are_different_faults() {
        let mut silent = healthy();
        silent.model.models = None;
        assert_eq!(verdict(&silent, CheckId::ModelEndpoint), Verdict::Failed);
        // And the model check cannot answer while nothing answers, rather than claiming the
        // model is missing -- which is the sentence that gets a model downloaded twice.
        assert_eq!(verdict(&silent, CheckId::ConfiguredModel), Verdict::Unknown);

        let mut empty = healthy();
        empty.model.models = Some(Vec::new());
        assert_eq!(verdict(&empty, CheckId::ModelEndpoint), Verdict::Degraded);
    }

    #[test]
    fn a_model_wearing_its_latest_tag_is_the_same_model() {
        let mut obs = healthy();
        obs.model.model = "llama3.2:latest".to_string();
        obs.model.models = Some(vec!["llama3.2".to_string()]);
        assert_eq!(verdict(&obs, CheckId::ConfiguredModel), Verdict::Ok);
    }

    #[test]
    fn a_registered_hotkey_on_wayland_is_still_broken() {
        let mut obs = healthy();
        obs.hotkey.wayland = true;
        assert_eq!(obs.app.hotkey_registered, Some(true));
        assert_eq!(verdict(&obs, CheckId::Hotkey), Verdict::Degraded);
        let report = diagnose(&obs);
        let step = &report.check(CheckId::Hotkey).unwrap().steps[0];
        assert_eq!(step.command.as_deref(), Some("aether1 toggle"));
        // And there is nothing to repair: re-registering it would succeed and change nothing.
        assert!(report.check(CheckId::Hotkey).unwrap().repair.is_none());
    }

    #[test]
    fn silence_at_the_speakers_outranks_a_working_engine() {
        let mut obs = healthy();
        obs.voice.playback_problem = Some("wavparse is missing.".to_string());
        obs.voice.playback_command = Some("sudo pacman -S gst-plugins-good".to_string());
        assert_eq!(verdict(&obs, CheckId::VoiceOut), Verdict::Failed);
        // The repair for it is handed over, never run: this is the root line.
        let repair = diagnose(&obs)
            .check(CheckId::VoiceOut)
            .unwrap()
            .repair
            .clone()
            .expect("a command to hand over");
        assert_eq!(
            repair.kind,
            RepairKind::HandOver {
                command: "sudo pacman -S gst-plugins-good".to_string()
            }
        );
    }

    #[test]
    fn no_engine_at_all_fails_and_one_working_engine_only_degrades() {
        let mut both = healthy();
        both.voice.piper_error = Some("no piper".to_string());
        both.voice.os_voice_error = Some("no espeak-ng".to_string());
        assert_eq!(verdict(&both, CheckId::VoiceOut), Verdict::Failed);

        let mut piper_only = healthy();
        piper_only.voice.piper_error = Some("no voice files".to_string());
        assert_eq!(verdict(&piper_only, CheckId::VoiceOut), Verdict::Degraded);
    }

    #[test]
    fn speech_switched_off_is_not_a_fault() {
        let mut obs = healthy();
        obs.voice.auto_speak = false;
        obs.voice.piper_error = Some("no piper".to_string());
        obs.voice.os_voice_error = Some("no espeak-ng".to_string());
        assert_eq!(verdict(&obs, CheckId::VoiceOut), Verdict::Ok);
    }

    #[test]
    fn a_pile_of_expired_proposals_is_a_fault_and_one_is_not() {
        let mut one = healthy();
        one.consent.expired = 1;
        assert_eq!(verdict(&one, CheckId::ConsentQueue), Verdict::Ok);

        let mut pile = healthy();
        pile.consent.expired = 4;
        assert_eq!(verdict(&pile, CheckId::ConsentQueue), Verdict::Degraded);
        assert_eq!(
            diagnose(&pile)
                .check(CheckId::ConsentQueue)
                .unwrap()
                .repair
                .as_ref()
                .map(|r| r.id),
            Some(RepairId::ClearStaleProposals)
        );
    }

    #[test]
    fn a_tray_that_cannot_be_asked_about_is_unknown_and_a_missing_host_degrades() {
        let mut unasked = healthy();
        unasked.tray.host_present = None;
        assert_eq!(verdict(&unasked, CheckId::Tray), Verdict::Unknown);

        let mut bare = healthy();
        bare.tray.host_present = Some(false);
        assert_eq!(verdict(&bare, CheckId::Tray), Verdict::Degraded);

        // The same observation on Windows: there is always somewhere to put the icon, so the
        // Linux-only probe result must not be read as a fault. This is a fixture from one
        // machine judged as another, which is the whole mechanism.
        let mut windows = bare.clone();
        windows.os = Os::Windows;
        assert_eq!(verdict(&windows, CheckId::Tray), Verdict::Ok);
    }

    #[test]
    fn a_watcher_that_has_stopped_sweeping_is_reported() {
        let mut obs = healthy();
        obs.watcher.last_sweep_secs_ago = Some(600);
        assert_eq!(verdict(&obs, CheckId::Watchers), Verdict::Degraded);
    }

    #[test]
    fn a_server_nobody_asked_for_is_not_a_fault_and_one_that_was_is() {
        let mut idle = healthy();
        idle.server.serving = None;
        assert_eq!(verdict(&idle, CheckId::Server), Verdict::Ok);

        let mut serving = healthy();
        serving.server.serving = Some(true);
        serving.server.port_answers = false;
        assert_eq!(verdict(&serving, CheckId::Server), Verdict::Failed);

        let mut lan = healthy();
        lan.server.serving = Some(true);
        lan.server.port_answers = true;
        lan.server.lan = Some(true);
        lan.server.tls_fingerprint = None;
        assert_eq!(verdict(&lan, CheckId::Server), Verdict::Failed);
    }

    #[test]
    fn a_download_bigger_than_the_disk_is_reported_before_it_starts() {
        let mut obs = healthy();
        obs.disk.free_gb = 1.2;
        obs.disk.wanted = vec![("llama3.2".to_string(), 4.7)];
        assert_eq!(verdict(&obs, CheckId::DiskRoom), Verdict::Failed);

        obs.disk.free_gb = 40.0;
        assert_eq!(verdict(&obs, CheckId::DiskRoom), Verdict::Ok);
    }

    #[test]
    fn a_dead_managed_server_with_nothing_in_its_place_is_a_failure() {
        let mut obs = healthy();
        obs.app.managed_ollama = Some(false);
        obs.model.models = None;
        assert_eq!(verdict(&obs, CheckId::BackgroundServices), Verdict::Failed);

        // ...but an Ollama the operator started themselves is not AETHER1's to account for.
        obs.model.models = Some(vec!["llama3.2".to_string()]);
        assert_eq!(verdict(&obs, CheckId::BackgroundServices), Verdict::Ok);
    }

    #[test]
    fn a_repair_is_only_offered_for_something_actually_broken() {
        // The table is a pure function, so this is checkable without running anything: a
        // healthy machine must not have a single repair attached to it.
        let health = diagnose(&healthy());
        assert!(health.checks.iter().all(|check| check.repair.is_none()));
    }

    #[test]
    fn a_silent_endpoint_offers_to_start_the_server_only_when_the_program_is_there() {
        let mut obs = healthy();
        obs.model.models = None;
        let with_cli = diagnose(&obs);
        assert_eq!(
            with_cli
                .check(CheckId::ModelEndpoint)
                .unwrap()
                .repair
                .as_ref()
                .map(|r| r.id),
            Some(RepairId::StartModelServer)
        );

        obs.model.ollama_cli_installed = false;
        assert!(diagnose(&obs)
            .check(CheckId::ModelEndpoint)
            .unwrap()
            .repair
            .is_none());
    }

    #[test]
    fn an_observation_round_trips_through_json_and_judges_the_same() {
        let obs = {
            let mut obs = healthy();
            obs.voice.piper_error = Some("no piper".to_string());
            obs.model.models = None;
            obs
        };
        let json = serde_json::to_string(&obs).expect("an observation serializes");
        let replayed = replay(&json).expect("and reads back");
        let direct = diagnose(&obs);
        assert_eq!(replayed.headline, direct.headline);
        for (a, b) in replayed.checks.iter().zip(direct.checks.iter()) {
            assert_eq!(a.key, b.key);
            assert_eq!(a.verdict, b.verdict);
            assert_eq!(a.detail, b.detail);
        }
    }

    #[test]
    fn a_report_that_is_not_an_observation_is_refused_rather_than_guessed_at() {
        assert!(replay("{\"nonsense\": true").is_err());
        // A JSON object that is not one of ours: serde fills the defaults, which would read
        // as a machine where nothing is configured. That is a real risk of the format and the
        // reason a replayed report says which version recorded it.
        let health = replay("{}").expect("defaults are a valid observation");
        assert!(health.needs_attention);
    }

    #[test]
    fn rung_three_stops_at_one_attempt_per_check_per_session() {
        forget_attempts();
        let key = (CheckId::ModelEndpoint, RepairId::StartModelServer);
        assert_eq!(attempts_this_session(key), 0);
        record_attempt_in_session(key);
        assert_eq!(attempts_this_session(key), 1);
        // A different repair of the same check is its own budget; the same one is spent.
        assert_eq!(
            attempts_this_session((CheckId::ModelEndpoint, RepairId::PullConfiguredModel)),
            0
        );
        forget_attempts();
    }

    #[test]
    fn every_check_has_a_distinct_key_and_says_whose_fault_it_is() {
        let mut keys: Vec<&str> = CheckId::ALL.iter().map(|id| id.key()).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before, "two checks share a key");
        // Seven and seven, as step 47 splits them. A new check has to decide which side it is
        // on, because that is what decides whether "fix it" can mean anything.
        assert_eq!(
            CheckId::ALL
                .iter()
                .filter(|id| id.owner() == Owner::Aether1)
                .count(),
            7
        );
    }
}
