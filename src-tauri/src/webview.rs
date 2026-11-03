//! Keeping the Linux webview's renderer alive, and healing the machine where it will not.
//!
//! On Linux the window is WebKitGTK, and the page is drawn by a **separate process**,
//! `WebKitWebProcess`. That process can die on its own: the usual cause is the accelerated
//! path -- DMA-BUF buffer sharing between the renderer and the compositor -- hitting a
//! driver that cannot import what it is handed. The symptom an operator sees is a blank
//! window, or the renderer simply aborting (`SIGABRT`) while the rest of AETHER1 carries on
//! running, which is the worst shape a failure can take: the app is alive, the tray is
//! there, and there is nothing to look at.
//!
//! WebKitGTK has two environment escape hatches for this, and they are not equal:
//!
//! | level | variables | cost |
//! |---|---|---|
//! | 0 | none | the fast path, which is what almost every machine should be on |
//! | 1 | `WEBKIT_DISABLE_DMABUF_RENDERER=1` | buffers go through the slower path; animation gets more expensive |
//! | 2 | ...plus `WEBKIT_DISABLE_COMPOSITING_MODE=1` | no GPU compositing at all; the last resort |
//!
//! **The decision this module makes is to never set either of them pre-emptively.** Setting
//! them everywhere would tidy a crash that happens on a minority of driver stacks by taking
//! the accelerated path away from every machine, including the low-end laptops whose
//! complaint is already frame rate -- and this was weighed once before and rejected for
//! exactly that reason. So the ladder is climbed only by a machine that has demonstrated it
//! needs to: a launch where the renderer never rendered, or a run where the renderer died,
//! is remembered, and the *next* launch starts one rung up. A machine that then works keeps
//! the rung it is on, because the driver underneath did not change.
//!
//! That leaves one honest cost: the first failure on a new machine is still a failure. The
//! machine heals on the launch after it rather than during it, and `aether1 doctor` says
//! which rung it is on and why.
//!
//! An operator who sets either variable by hand owns the decision entirely -- nothing here
//! overrides, and nothing here records, so their export is not mistaken for a rung later.
//!
//! The parsing and the ladder are pure functions over a tiny record, so the whole policy is
//! testable without a GPU, a webview, or a crash. The side-effecting half is four short
//! functions at the bottom.

use std::path::{Path, PathBuf};

/// The highest rung. There is no third escape hatch, so a machine that still cannot render
/// here has a problem that is not WebKitGTK's to work around.
pub const MAX_LEVEL: u8 = 2;

/// The file the rung is remembered in, under the app's own data directory.
const RECORD_FILE: &str = "webview-renderer";

/// The two variables, in the order the rungs add them.
pub const DISABLE_DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
pub const DISABLE_COMPOSITING: &str = "WEBKIT_DISABLE_COMPOSITING_MODE";

/// What the last run of AETHER1 managed to do with its renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// A launch began and nothing has reported a rendered page yet. Read on the *next*
    /// launch, this means the renderer never came up at all.
    Starting,
    /// A page loaded. The rung this run is on works on this machine.
    Rendered,
    /// A page loaded and the renderer died afterwards, during the run.
    Crashed,
}

impl State {
    fn key(self) -> &'static str {
        match self {
            State::Starting => "starting",
            State::Rendered => "rendered",
            State::Crashed => "crashed",
        }
    }

    fn from_key(key: &str) -> Option<State> {
        match key {
            "starting" => Some(State::Starting),
            "rendered" => Some(State::Rendered),
            "crashed" => Some(State::Crashed),
            _ => None,
        }
    }
}

/// What the record file holds: the rung a run used, and how that run went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    pub level: u8,
    pub state: State,
}

/// Reads the record. Anything unreadable, truncated or from a future version reads as
/// `None` -- a corrupt file must not pin a machine to a rung it never needed, and the cost
/// of being wrong is one launch on the fast path.
pub fn parse(text: &str) -> Option<Record> {
    let mut level = None;
    let mut state = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "level" => level = value.trim().parse::<u8>().ok().filter(|l| *l <= MAX_LEVEL),
            "state" => state = State::from_key(value.trim()),
            _ => {}
        }
    }
    Some(Record {
        level: level?,
        state: state?,
    })
}

pub fn render(record: Record) -> String {
    format!(
        "# How AETHER1's webview renderer went last time. Delete this file to start again \
         on the accelerated path.\nlevel={}\nstate={}\n",
        record.level,
        record.state.key()
    )
}

/// The rung this launch should use, from how the last one went.
///
/// `Starting` means the last launch never rendered anything, and `Crashed` means it rendered
/// and then lost the renderer; both are the renderer failing on this rung, so both climb.
/// `Rendered` keeps the rung -- including rung 0, which is the overwhelmingly common case.
pub fn next_level(previous: Option<Record>) -> u8 {
    match previous {
        None => 0,
        Some(record) => match record.state {
            State::Rendered => record.level,
            State::Starting | State::Crashed => record.level.saturating_add(1).min(MAX_LEVEL),
        },
    }
}

/// The variables a rung sets. Rung 0 sets nothing, which is the point of it existing as a
/// rung rather than as the absence of one.
pub fn env_for(level: u8) -> Vec<(&'static str, &'static str)> {
    let mut vars = Vec::new();
    if level >= 1 {
        vars.push((DISABLE_DMABUF, "1"));
    }
    if level >= 2 {
        vars.push((DISABLE_COMPOSITING, "1"));
    }
    vars
}

/// What to tell an operator about the rung they are on, in a sentence they can act on.
pub fn explain(level: u8, operator_set: bool) -> String {
    if operator_set {
        return format!(
            "You have set {DISABLE_DMABUF} or {DISABLE_COMPOSITING} yourself, so AETHER1 \
             leaves the webview's rendering entirely to you and records nothing about it."
        );
    }
    match level {
        0 => "The webview is on the accelerated rendering path, which is where it should be."
            .to_string(),
        1 => format!(
            "The webview's renderer failed on the accelerated path on this machine, so \
             AETHER1 now starts it with {DISABLE_DMABUF}=1. Drawing is a little more \
             expensive and the window works. Delete the `{RECORD_FILE}` file in AETHER1's \
             data directory to try the fast path again -- after a driver update, say."
        ),
        _ => format!(
            "The webview's renderer failed with DMA-BUF off as well, so AETHER1 now starts \
             it with {DISABLE_COMPOSITING}=1 too. This is the last fallback there is: if the \
             window still does not draw, the fault is below WebKitGTK and worth a bug report."
        ),
    }
}

// ---------------------------------------------------------------------------------------
// The side-effecting half.
// ---------------------------------------------------------------------------------------

fn record_path() -> Option<PathBuf> {
    crate::paths::app_data_dir().map(|dir| dir.join(RECORD_FILE))
}

fn read_record(path: &Path) -> Option<Record> {
    parse(&std::fs::read_to_string(path).ok()?)
}

fn write_record(path: &Path, record: Record) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Quietly. A machine whose data directory cannot be written has larger problems than
    // which rendering path its webview is on, and failing the launch over it would turn a
    // slow window into no window.
    let _ = std::fs::write(path, render(record));
}

/// True when the operator has made this decision themselves, in which case this module
/// makes none.
///
/// **Answered once, before `arm` touches the environment, and then remembered.** Asking the
/// environment later would be asking it about AETHER1's own variables: a machine on rung 1
/// has `WEBKIT_DISABLE_DMABUF_RENDERER` set *by `arm`*, so a fresh read says the operator
/// decided and every later call declines to record anything. The ladder would then never
/// see a rung work, and would climb one rung per launch until it ran out -- which is what
/// happened the first time this was run against the real binary, and does not show up in
/// any test that calls these functions one at a time.
fn operator_set() -> bool {
    static ASKED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ASKED.get_or_init(|| {
        [DISABLE_DMABUF, DISABLE_COMPOSITING]
            .iter()
            .any(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
    })
}

/// Call once, before the webview exists: sets this launch's variables and records that a
/// launch has begun. Returns the rung, for the log line and for `doctor`.
///
/// Not called by `--serve` or by any CLI subcommand -- they have no webview, and a headless
/// run recording "starting" and never rendering would climb the ladder for a window that
/// was never asked for.
pub fn arm() -> u8 {
    if !cfg!(target_os = "linux") || operator_set() {
        return 0;
    }
    let Some(path) = record_path() else {
        return 0;
    };
    let level = next_level(read_record(&path));
    for (name, value) in env_for(level) {
        // Safe on edition 2021, and this runs on the main thread before any other thread
        // exists -- `set_var` is only a hazard once threads are reading the environment.
        std::env::set_var(name, value);
    }
    if level > 0 {
        eprintln!(
            "[AETHER1] The webview renderer failed here last time; starting it at fallback \
             level {level} ({}).",
            env_for(level)
                .iter()
                .map(|(n, v)| format!("{n}={v}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    write_record(
        &path,
        Record {
            level,
            state: State::Starting,
        },
    );
    level
}

/// Call when a page has actually loaded: the renderer is up, so this rung works here.
///
/// Idempotent and cheap enough to call on every page load; it is a short write to a file
/// that nothing reads until the next launch.
pub fn rendered() {
    if !cfg!(target_os = "linux") || operator_set() {
        return;
    }
    let Some(path) = record_path() else {
        return;
    };
    let previous = read_record(&path);
    if previous.map(|record| record.state) == Some(State::Rendered) {
        return;
    }
    let level = previous.map(|record| record.level).unwrap_or(0);
    write_record(
        &path,
        Record {
            level,
            state: State::Rendered,
        },
    );
}

/// Call when the crash watcher sees the renderer die while AETHER1 is running. The next
/// launch climbs a rung.
pub fn note_crash() {
    if !cfg!(target_os = "linux") || operator_set() {
        return;
    }
    let Some(path) = record_path() else {
        return;
    };
    let level = read_record(&path).map(|r| r.level).unwrap_or(0);
    write_record(
        &path,
        Record {
            level,
            state: State::Crashed,
        },
    );
}

/// The rung in force, for a report. Reads; changes nothing.
pub fn current_level() -> u8 {
    if !cfg!(target_os = "linux") {
        return 0;
    }
    record_path()
        .as_deref()
        .and_then(read_record)
        .map(|r| r.level)
        .unwrap_or(0)
}

pub fn operator_decides() -> bool {
    operator_set()
}

/// Whether a crashed program is the webview's renderer.
///
/// Matched on the name the operating system reports rather than on a path: `coredumpctl`
/// says `WebKitWebProcess`, and the older GTK3 build and the sandboxed launcher spell it
/// slightly differently, so the test is a prefix on a lowercased name.
pub fn is_renderer_process(program: &str) -> bool {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    name.starts_with("webkitwebprocess")
        || name.starts_with("webkitnetworkprocess")
        || name.starts_with("webkitgpuprocess")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_machine_with_no_record_starts_on_the_accelerated_path() {
        assert_eq!(next_level(None), 0);
        assert!(env_for(0).is_empty());
    }

    #[test]
    fn a_launch_that_never_rendered_climbs_one_rung() {
        let previous = Record {
            level: 0,
            state: State::Starting,
        };
        assert_eq!(next_level(Some(previous)), 1);
        assert_eq!(env_for(1), vec![(DISABLE_DMABUF, "1")]);
    }

    #[test]
    fn a_renderer_that_died_during_the_run_climbs_too() {
        let previous = Record {
            level: 0,
            state: State::Crashed,
        };
        assert_eq!(next_level(Some(previous)), 1);
    }

    #[test]
    fn a_rung_that_worked_is_kept_rather_than_given_back() {
        // The driver underneath did not change because the window opened, so a machine
        // that needed the fallback last time needs it this time.
        let previous = Record {
            level: 1,
            state: State::Rendered,
        };
        assert_eq!(next_level(Some(previous)), 1);
    }

    #[test]
    fn the_ladder_stops_at_the_last_fallback_there_is() {
        let previous = Record {
            level: MAX_LEVEL,
            state: State::Crashed,
        };
        assert_eq!(next_level(Some(previous)), MAX_LEVEL);
        assert_eq!(
            env_for(MAX_LEVEL),
            vec![(DISABLE_DMABUF, "1"), (DISABLE_COMPOSITING, "1")]
        );
    }

    #[test]
    fn a_record_survives_the_round_trip_and_a_corrupt_one_does_not_pin_a_rung() {
        let record = Record {
            level: 2,
            state: State::Rendered,
        };
        assert_eq!(parse(&render(record)), Some(record));
        assert_eq!(parse(""), None);
        assert_eq!(parse("level=9\nstate=rendered\n"), None);
        assert_eq!(parse("level=1\nstate=sideways\n"), None);
    }

    #[test]
    fn the_renderer_is_recognised_by_name_however_it_is_spelled() {
        assert!(is_renderer_process("WebKitWebProcess"));
        assert!(is_renderer_process(
            "/usr/lib/webkit2gtk-4.1/WebKitWebProcess"
        ));
        assert!(is_renderer_process("WebKitGPUProcess"));
        assert!(!is_renderer_process("aether1"));
        assert!(!is_renderer_process("firefox"));
    }
}
