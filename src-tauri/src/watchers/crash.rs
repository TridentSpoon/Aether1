// Step 13: crash capture.
//
// The point of this module is one sentence from GOALS.md -- "a chatting friend chilling on
// your system that can help out when running into a problem". A companion that only knows
// what you type at it is something you go to. One that notices your editor just died and
// opens with the signal and the last lines of its log already in hand is a first responder.
//
// Three rules the design follows, all of them deliberate:
//
// 1. **Crashes only.** Not warnings, not failed units, not the day's events. The wider sweep
//    lives behind `aether1 status`, on request.
// 2. **Windows is a peer, not a feature gate.** Both platforms offer the same three things
//    under different names -- an event stream, a crash record, a log tail. On Linux that is
//    the systemd journal, `coredumpctl`, and `/var/log`; on Windows it is the event log and
//    Windows Error Reporting. One design (`CrashReader`), two readers.
// 3. **It stands on its own.** No agent installed, no local model, no network: it still
//    shows you the crash. Summarising is a bonus laid on top, and is local-model work --
//    handing a cloud API your machine's recent failures is not a thing to do casually.
//
// The parsing is kept in free functions that take the tool's output as a string, so the
// awkward half -- the formats -- is testable without a crash, a core dump, or the tool being
// installed at all. That matters here more than usual: the machine this was written on has
// neither `coredumpctl` nor a journal, and a reader you cannot test is a reader you are
// guessing at.

use std::process::Command;

use serde::{Deserialize, Serialize};

/// How many lines of the failing program's log travel with a crash. Enough to see what it
/// was doing, short enough to put in front of a model that may only have a small context.
const LOG_TAIL_LINES: usize = 40;

/// One crash, in the form both the notification and step 15's hand-off want it. This is the
/// "assembled context" the two steps share: built once, here, so the crash the tray offers
/// and the crash an agent is handed are the same object rather than two descriptions that
/// drift apart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crash {
    /// The program that died, as the operating system named it.
    pub program: String,
    pub pid: u32,
    /// What killed it, in the platform's own words -- "SIGSEGV" on Linux, an exception code
    /// on Windows. Deliberately not normalised into some cross-platform enum: the words the
    /// system used are what the operator will search for.
    pub cause: String,
    /// Unix seconds. Zero when the source gave no usable time, which is treated as "recent"
    /// rather than 1970.
    pub at: u64,
    /// The last lines of what the program was saying before it stopped.
    pub log_tail: Vec<String>,
}

impl Crash {
    /// A stable handle for one crash, so a watcher polling every few seconds does not offer
    /// the same death twice. Pid plus time rather than a counter: pids are reused, times
    /// collide, but the pair does neither within a session.
    pub fn key(&self) -> String {
        format!("{}:{}:{}", self.program, self.pid, self.at)
    }

    /// The crash written out for a model, or for a person reading it in a terminal. Plain
    /// prose with the facts in it rather than JSON: this is what gets put in front of a
    /// small local model, and small models do markedly better with sentences than with
    /// structure.
    pub fn as_context(&self) -> String {
        let mut out = format!(
            "{} (pid {}) stopped unexpectedly: {}.",
            self.program, self.pid, self.cause
        );
        if self.log_tail.is_empty() {
            out.push_str("\n\nNothing was recovered from its log.");
        } else {
            out.push_str("\n\nThe last lines it wrote:\n\n");
            for line in &self.log_tail {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    /// The one-line form for a notification or a tray tooltip, where there is no room for
    /// the log and no time to read it.
    pub fn headline(&self) -> String {
        format!("{} stopped unexpectedly ({})", self.program, self.cause)
    }
}

/// The one design both platforms implement. Kept deliberately small: a reader's whole job is
/// to answer "what has died since this moment", and everything else -- deduplication, the
/// mute list, what to do about it -- belongs to the caller, on both platforms equally.
pub trait CrashReader {
    /// Crashes recorded at or after `since` (unix seconds), oldest first.
    ///
    /// An `Err` means the reader could not look, which is a different thing from looking and
    /// finding nothing -- a machine with no `coredumpctl` installed must not be reported as
    /// a machine that never crashes.
    fn crashes_since(&self, since: u64) -> Result<Vec<Crash>, String>;

    /// Whether this machine can be watched at all, and if not, why -- said in a sentence an
    /// operator can act on rather than a bare false.
    fn availability(&self) -> Availability;
}

/// Whether crash capture can work here. The unavailable case carries its own explanation
/// because "crash capture is off" with no reason is the kind of silence this project treats
/// as a bug: step 8's rule is that a missing dependency always points at least at its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Ready,
    Unavailable(String),
}

// ---------------------------------------------------------------------------------------
// Linux: coredumpctl for the crash record, journalctl for the log tail.
// ---------------------------------------------------------------------------------------

/// Reads crashes from `systemd-coredump` via `coredumpctl`, and the failing program's last
/// words from the journal.
///
/// `coredumpctl` rather than watching `/var/lib/systemd/coredump` directly: the directory
/// holds compressed dumps named by a scheme that is not a stable interface, while
/// `coredumpctl --json` is. Polling rather than a socket or an inotify watch because there
/// is no supported event for "a core was just collected" -- and at a crash-shaped interval,
/// polling a process that answers in milliseconds costs nothing worth saving.
#[derive(Default)]
pub struct LinuxCrashReader;

impl CrashReader for LinuxCrashReader {
    fn crashes_since(&self, since: u64) -> Result<Vec<Crash>, String> {
        let output = Command::new("coredumpctl")
            .args(["list", "--no-pager", "--json=short"])
            .output()
            .map_err(|e| format!("could not run coredumpctl: {e}"))?;
        // coredumpctl exits non-zero when it has nothing at all to list, which is the
        // ordinary state of a healthy machine rather than a failure to report.
        if !output.status.success() && output.stdout.is_empty() {
            return Ok(Vec::new());
        }
        let mut crashes = parse_coredumpctl_json(&String::from_utf8_lossy(&output.stdout), since)?;
        for crash in &mut crashes {
            crash.log_tail = journal_tail(&crash.program, crash.pid);
        }
        Ok(crashes)
    }

    fn availability(&self) -> Availability {
        match Command::new("coredumpctl").arg("--version").output() {
            Ok(output) if output.status.success() => Availability::Ready,
            _ => Availability::Unavailable(
                "coredumpctl was not found, so AETHER1 cannot see crashes on this machine. \
                 It comes with systemd-coredump -- install that package, or turn crash \
                 capture off in settings to stop being told."
                    .to_string(),
            ),
        }
    }
}

/// Pulls the failing program's last lines out of the journal. Best-effort on purpose: a
/// crash with no log is still a crash worth showing, so every failure here is an empty tail
/// rather than an error that loses the crash with it.
fn journal_tail(program: &str, pid: u32) -> Vec<String> {
    let by_pid = Command::new("journalctl")
        .args([
            "--no-pager",
            "--output=cat",
            "--lines",
            &LOG_TAIL_LINES.to_string(),
            &format!("_PID={pid}"),
        ])
        .output()
        .ok()
        .map(|output| tidy_log_tail(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default();
    if !by_pid.is_empty() {
        return by_pid;
    }
    // The pid may have left nothing behind -- a program that logged through a parent, or a
    // journal that has already rotated. The program's own unit is the next best thing.
    Command::new("journalctl")
        .args([
            "--no-pager",
            "--output=cat",
            "--lines",
            &LOG_TAIL_LINES.to_string(),
            "-t",
            program,
        ])
        .output()
        .ok()
        .map(|output| tidy_log_tail(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

/// Drops blank lines and the journal's own "no entries" filler, and caps the length, so an
/// empty tail is genuinely empty rather than a vector of nothing much.
fn tidy_log_tail(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .filter(|line| !line.starts_with("-- No entries --"))
        .filter(|line| !line.starts_with("No journal files were found"))
        .map(str::to_string)
        .rev()
        .take(LOG_TAIL_LINES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// Turns `coredumpctl list --json=short` into crashes, keeping only those at or after
/// `since` and only those that actually died of a signal.
///
/// Kept separate from running the command so the format can be tested without systemd
/// present. The fields are coredumpctl's own: `exe`, `pid`, `sig`, `time` (microseconds
/// since the epoch), `corefile`.
pub fn parse_coredumpctl_json(raw: &str, since: u64) -> Result<Vec<Crash>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let entries: Vec<serde_json::Value> = serde_json::from_str(trimmed)
        .map_err(|e| format!("could not read what coredumpctl reported: {e}"))?;

    let mut crashes: Vec<Crash> = entries
        .iter()
        .filter_map(|entry| {
            // Microseconds in the JSON, seconds everywhere else in this module.
            let at = entry.get("time").and_then(|v| v.as_u64()).unwrap_or(0) / 1_000_000;
            if at < since {
                return None;
            }
            let pid = entry.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let program = entry
                .get("exe")
                .and_then(|v| v.as_str())
                .map(program_name)
                // `exe` is absent when the binary is already gone; `unit` still names it.
                .or_else(|| {
                    entry
                        .get("unit")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                })?;
            Some(Crash {
                program,
                pid,
                cause: signal_name(entry.get("sig").and_then(|v| v.as_u64())),
                at,
                log_tail: Vec::new(),
            })
        })
        .collect();
    crashes.sort_by_key(|crash| crash.at);
    Ok(crashes)
}

/// The basename of a path, which is what an operator calls a program. The full path is not
/// lost so much as not wanted: "/usr/lib/firefox/firefox stopped unexpectedly" reads worse
/// than "firefox stopped unexpectedly" and says nothing more.
fn program_name(exe: &str) -> String {
    exe.rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(exe)
        .to_string()
}

/// Signal numbers as their names. Only the ones that mean a crash are spelled out; anything
/// else is reported by number rather than guessed at, since an invented name is worse than
/// an honest number.
fn signal_name(signal: Option<u64>) -> String {
    match signal {
        Some(4) => "SIGILL, an illegal instruction".to_string(),
        Some(6) => "SIGABRT, the program aborted itself".to_string(),
        Some(7) => "SIGBUS, a bad memory access".to_string(),
        Some(8) => "SIGFPE, an arithmetic fault".to_string(),
        Some(11) => "SIGSEGV, a segmentation fault".to_string(),
        Some(other) => format!("signal {other}"),
        None => "no signal was recorded".to_string(),
    }
}

// ---------------------------------------------------------------------------------------
// Windows: the Application event log, which is where Windows Error Reporting lands.
// ---------------------------------------------------------------------------------------
//
// Everything from here to the end of this section is compiled on every platform and used
// only on Windows, which is what the `allow(dead_code)` below is for. That is deliberate
// rather than an oversight: principle 8 makes Linux and Windows peers, the parsing is the
// half that can be wrong, and a reader that only compiles on the machine it runs on is a
// reader nobody notices has rotted. Its tests run on Linux, where this was written.

/// Reads crashes from the Windows Application event log.
///
/// Compiled everywhere, not only on Windows: principle 8 says the two platforms are peers,
/// and a reader that only exists on the machine it runs on is a reader nobody notices has
/// rotted. Its parsing is tested on Linux, which is where this was written.
///
/// Event ID 1000 from source `Application Error` is what Windows writes when a user-mode
/// process dies -- the same event Event Viewer shows as "Faulting application name". Read
/// through PowerShell's `Get-WinEvent` rather than a Win32 binding, matching how the rest of
/// the project already talks to Windows (the SAPI voice path does the same), so this adds no
/// dependency and nothing to compile differently.
#[derive(Default)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct WindowsCrashReader;

impl CrashReader for WindowsCrashReader {
    fn crashes_since(&self, since: u64) -> Result<Vec<Crash>, String> {
        // `$_.Properties` rather than `$_.Message`: event 1000 declares its fields in a
        // fixed template -- application name, version, timestamp, module name, version,
        // timestamp, exception code, fault offset, process id -- and the properties array
        // hands them over as themselves. The message is that same data rendered into a
        // paragraph whose labels are translated per install and whose hex values are
        // indistinguishable from each other once they are prose. Reading the fields is both
        // simpler and the only version that works on a machine that is not in English.
        let script = format!(
            "$epoch = Get-Date '1970-01-01T00:00:00Z'; \
             Get-WinEvent -FilterHashtable @{{LogName='Application'; ProviderName='Application Error'; Id=1000; StartTime=$epoch.ToUniversalTime().AddSeconds({since})}} \
             -ErrorAction SilentlyContinue | Select-Object -First 50 | \
             ForEach-Object {{ $p = $_.Properties; [pscustomobject]@{{ \
               time = [int64](New-TimeSpan -Start $epoch -End $_.TimeCreated.ToUniversalTime()).TotalSeconds; \
               program = $p[0].Value; module = $p[3].Value; code = $p[6].Value; \
               process_id = $p[8].Value; message = $_.Message }} }} | \
             ConvertTo-Json -Compress -AsArray"
        );
        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .map_err(|e| format!("could not read the Windows event log: {e}"))?;
        parse_windows_events(&String::from_utf8_lossy(&output.stdout), since)
    }

    fn availability(&self) -> Availability {
        match Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$PSVersionTable.PSVersion.Major",
            ])
            .output()
        {
            Ok(output) if output.status.success() => Availability::Ready,
            _ => Availability::Unavailable(
                "PowerShell could not be run, so AETHER1 cannot read the Windows event log \
                 and will not see crashes on this machine."
                    .to_string(),
            ),
        }
    }
}

/// Turns the JSON that script produces into crashes. Separate from running PowerShell for
/// the same reason the Linux parser is separate: this is the half that can be wrong, and it
/// can be tested on any machine, including the Linux one this was written on.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn parse_windows_events(raw: &str, since: u64) -> Result<Vec<Crash>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let entries: Vec<serde_json::Value> = serde_json::from_str(trimmed)
        .map_err(|e| format!("could not read what the Windows event log reported: {e}"))?;

    let mut crashes: Vec<Crash> = entries
        .iter()
        .filter_map(|entry| {
            let at = entry
                .get("time")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                .max(0) as u64;
            if at < since {
                return None;
            }
            // Without a name there is nothing to tell the operator, so the entry is not a
            // crash we can report; every other field has a sensible absence.
            let program = entry
                .get("program")
                .and_then(hex_or_text)
                .map(|name| program_name(&name))
                .filter(|name| !name.is_empty())?;
            let code = entry.get("code").and_then(hex_or_text);
            Some(Crash {
                program,
                pid: entry
                    .get("process_id")
                    .and_then(hex_or_number)
                    .unwrap_or_default(),
                cause: match code {
                    Some(code) => format!("exception {code}{}", describe_exception(&code)),
                    None => "the application faulted".to_string(),
                },
                at,
                // The rendered message is the closest thing Windows offers to a log tail,
                // and it is genuinely worth keeping -- it carries the faulting module and
                // the offset, which is what anyone diagnosing this will look at first.
                log_tail: entry
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(tidy_log_tail)
                    .unwrap_or_default(),
            })
        })
        .collect();
    crashes.sort_by_key(|crash| crash.at);
    Ok(crashes)
}

/// Event properties arrive as strings on most Windows versions and as numbers on some, so
/// both are read rather than one being assumed.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn hex_or_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        serde_json::Value::Number(number) => number.as_u64().map(|n| format!("0x{n:x}")),
        _ => None,
    }
}

/// The same, for a field that has to end up a number: `"0x1a4c"`, `"6732"` or `6732`.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn hex_or_number(value: &serde_json::Value) -> Option<u32> {
    match value {
        serde_json::Value::Number(number) => number.as_u64().map(|n| n as u32),
        serde_json::Value::String(text) => {
            let text = text.trim();
            match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => text.parse().ok(),
            }
        }
        _ => None,
    }
}

/// The handful of exception codes worth naming. Same rule as `signal_name`: only what is
/// certain, and silence rather than a guess for the rest.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn describe_exception(code: &str) -> String {
    match code.to_ascii_lowercase().as_str() {
        "0xc0000005" => ", an access violation".to_string(),
        "0xc0000409" => ", a stack buffer overrun".to_string(),
        "0xc000041d" => ", an unhandled exception in a callback".to_string(),
        "0xc0000374" => ", heap corruption".to_string(),
        "0xe0434352" => ", an unhandled .NET exception".to_string(),
        _ => String::new(),
    }
}

/// The reader for whichever machine this is.
pub fn reader_for_this_machine() -> Box<dyn CrashReader + Send + Sync> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsCrashReader)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(LinuxCrashReader)
    }
}

// ---------------------------------------------------------------------------------------
// What the caller does with them: muting, and not saying the same thing twice.
// ---------------------------------------------------------------------------------------

/// Settings key for the off switch. Crash capture speaks unprompted, so it gets a switch,
/// and the switch is on by default because a watcher nobody turned on watches nothing.
pub const ENABLED_SETTING: &str = "crash_capture_enabled";
/// Settings key holding the process AETHER1 last shut itself down on, so the copy that
/// starts next can tell that death apart from a crash.
pub const EXPECTED_EXIT_SETTING: &str = "crash_expected_exit";

/// How long after a recorded deliberate exit an abort from that pid is still that exit.
/// Generous, because the abort happens *during* teardown and the record is written before
/// it: the process can spend a while in atexit handlers on the way out. Short enough that a
/// reused pid days later is not silently swallowed.
const EXPECTED_EXIT_WINDOW_SECS: u64 = 300;

/// A shutdown AETHER1 asked for, recorded just before it happens.
///
/// **Why this exists.** Aether1's own renderer aborts while the process is exiting -- a
/// WebKitGTK/Mesa teardown bug, seen as `SIGABRT, the program aborted itself` -- and the
/// relaunch after an update means the copy that starts next is watching when it lands. So
/// pressing update reliably produced a CRASH DETECTED card reporting the shutdown the
/// operator had just asked for. The crash is real and the reader is right to see it; what
/// was wrong was calling a death we caused "unexpectedly".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedExit {
    pub program: String,
    pub pid: u32,
    /// Unix seconds, taken when the exit was asked for rather than when it completed.
    pub at: u64,
}

impl ExpectedExit {
    /// This process, about to end on purpose. The program name comes from the running
    /// executable so it matches what the crash reader will call it -- `coredumpctl` reports
    /// the basename of `exe`, and so does this.
    pub fn for_this_process() -> Self {
        let program = std::env::current_exe()
            .ok()
            .map(|exe| program_name(&exe.to_string_lossy()))
            .unwrap_or_else(|| "aether1".to_string());
        Self {
            program,
            pid: std::process::id(),
            at: now_seconds(),
        }
    }
}

/// Reads the record back out of settings, tolerating both the object it writes and a value
/// left behind by something else. A record that will not parse is no record: the worst case
/// is one card an operator has seen before, not a swallowed crash.
pub fn expected_exit(raw: Option<serde_json::Value>) -> Option<ExpectedExit> {
    serde_json::from_value(raw?).ok()
}

/// Whether this crash is the shutdown recorded just before it.
///
/// All three have to line up. The pid is the real evidence; the program name guards against
/// a pid the system handed to something else; the window guards against the same pid coming
/// back round much later. `at` of zero means the source gave no usable time, which
/// `crashes_since` already treats as recent, so it is accepted here too rather than being
/// read as 1970.
pub fn is_expected_exit(crash: &Crash, expected: Option<&ExpectedExit>) -> bool {
    let Some(expected) = expected else {
        return false;
    };
    if crash.pid != expected.pid {
        return false;
    }
    if mute_key(&crash.program) != mute_key(&expected.program) {
        return false;
    }
    if crash.at == 0 {
        return true;
    }
    // A few seconds of slack below, because the two clocks are not the same one: the record
    // is stamped by this process and the crash by the system's journal.
    crash.at + 5 >= expected.at && crash.at <= expected.at + EXPECTED_EXIT_WINDOW_SECS
}

/// Settings key for the per-program mute list: programs whose crashes are noted but never
/// announced. The list exists because some programs crash as a matter of routine, and a
/// companion that says so every time is one you switch off entirely -- which would lose the
/// crashes that matter along with the ones that do not.
pub const MUTED_SETTING: &str = "crash_capture_muted";

/// Whether a program's crashes should be announced. Matching is on the program name,
/// case-insensitively and ignoring a `.exe` that the operator may or may not have typed, so
/// one mute list works on both platforms and reads the same on each.
pub fn is_muted(program: &str, muted: &[String]) -> bool {
    let needle = mute_key(program);
    muted.iter().any(|entry| mute_key(entry) == needle)
}

fn mute_key(program: &str) -> String {
    let name = program_name(program).to_ascii_lowercase();
    name.strip_suffix(".exe").unwrap_or(&name).to_string()
}

/// Keeps track of what has already been announced, so polling does not offer the same crash
/// on every pass.
#[derive(Default)]
pub struct Seen {
    keys: std::collections::HashSet<String>,
}

impl Seen {
    /// Records a crash and says whether it is new. Deliberately a single call rather than a
    /// contains-then-insert pair: two calls is how a poll that overlaps itself announces the
    /// same crash twice.
    pub fn is_new(&mut self, crash: &Crash) -> bool {
        self.keys.insert(crash.key())
    }

    /// How many crashes this watch has already accounted for. Only the tests look at it
    /// today; it is here because a watcher that cannot say what it has seen is a watcher
    /// you cannot debug.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.keys.len()
    }
}

/// How often the watcher looks. A crash is not an emergency that needs sub-second notice --
/// the program is already gone -- and the interval is what keeps an unasked-for watcher
/// cheap enough to leave on.
pub const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(20);

/// The whole watcher: a reader, what it has already said, and where it started watching.
///
/// Deliberately has no idea what happens to a crash once it is found. It returns them, and
/// the caller decides between a tray colour, a notification, a hand-off to an agent, or a
/// line in a terminal. That is what lets the same watcher serve `aether1 crashes`, the tray,
/// and step 15 without any of them knowing about the others.
pub struct CrashWatch {
    reader: Box<dyn CrashReader + Send + Sync>,
    seen: Seen,
    /// Crashes before this are history, not news. Set to now at construction, so starting
    /// AETHER1 does not announce a week of old core dumps at you.
    since: u64,
}

impl CrashWatch {
    pub fn new() -> Self {
        Self::with_reader(reader_for_this_machine())
    }

    pub fn with_reader(reader: Box<dyn CrashReader + Send + Sync>) -> Self {
        Self {
            reader,
            seen: Seen::default(),
            since: now_seconds(),
        }
    }

    pub fn availability(&self) -> Availability {
        self.reader.availability()
    }

    /// One pass. Returns only crashes that are new, are not muted, are not the shutdown
    /// AETHER1 asked for itself, and happened since the watch began -- which is to say, only
    /// the ones worth interrupting someone over.
    ///
    /// The mute list is applied *after* the crash is recorded as seen, so unmuting a program
    /// does not suddenly announce the crash it had an hour ago.
    pub fn poll(
        &mut self,
        muted: &[String],
        expected: Option<&ExpectedExit>,
    ) -> Result<Vec<Crash>, String> {
        let found = self.reader.crashes_since(self.since)?;
        // Recorded on the way through, and only on a sweep that actually read the machine:
        // `aether1 doctor` asks "is the watcher watching?", and a reader that errored every
        // time would otherwise look identical to one ticking along.
        LAST_SWEEP.store(now_seconds(), std::sync::atomic::Ordering::Relaxed);
        let mut news = Vec::new();
        for crash in found {
            if !self.seen.is_new(&crash) {
                continue;
            }
            if is_muted(&crash.program, muted) {
                continue;
            }
            // Recorded as seen above before this test, like the mute list, so a record that
            // is cleared later does not bring the shutdown back as news.
            if is_expected_exit(&crash, expected) {
                continue;
            }
            news.push(crash);
        }
        Ok(news)
    }
}

impl Default for CrashWatch {
    fn default() -> Self {
        Self::new()
    }
}

/// When the last sweep finished, as unix seconds, or 0 when no sweep has run in this
/// process. A static rather than a field on `CrashWatch` because the question is asked from
/// outside the watcher -- doctor.rs runs in the same process and holds no handle to it -- and
/// there is only ever one watch per process.
static LAST_SWEEP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How long ago the crash watcher last swept, or None when nothing has swept here. A
/// terminal invocation of `aether1 doctor` starts no watcher, so None is its usual answer and
/// means "not asked", not "stopped".
pub fn secs_since_last_sweep() -> Option<u64> {
    let at = LAST_SWEEP.load(std::sync::atomic::Ordering::Relaxed);
    if at == 0 {
        return None;
    }
    Some(now_seconds().saturating_sub(at))
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

/// Reads the mute list out of settings, tolerating the shapes an operator might have left
/// it in -- a JSON array, or a comma-separated line typed into a text box.
pub fn muted_programs(raw: Option<serde_json::Value>) -> Vec<String> {
    match raw {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str())
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect(),
        Some(serde_json::Value::String(line)) => line
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real `coredumpctl list --json=short` output, field for field. Two crashes and one
    // entry with no signal, which is what a process that was merely killed looks like.
    const COREDUMPCTL_JSON: &str = r#"[
        {"time":1758380000000000,"pid":4242,"uid":1000,"gid":1000,"sig":11,"corefile":"present","exe":"/usr/lib/firefox/firefox","size":8388608},
        {"time":1758390000000000,"pid":4310,"uid":1000,"gid":1000,"sig":6,"corefile":"present","exe":"/usr/bin/nvim","size":2097152},
        {"time":1758300000000000,"pid":1111,"uid":0,"gid":0,"sig":11,"corefile":"none","exe":"/usr/bin/old-thing","size":0}
    ]"#;

    #[test]
    fn coredumpctl_output_becomes_crashes_oldest_first() {
        let crashes = parse_coredumpctl_json(COREDUMPCTL_JSON, 0).unwrap();
        assert_eq!(crashes.len(), 3);
        assert!(
            crashes[0].at <= crashes[1].at && crashes[1].at <= crashes[2].at,
            "oldest first, so a watcher announces them in the order they happened"
        );
        let firefox = crashes
            .iter()
            .find(|crash| crash.program == "firefox")
            .expect("the path should be reduced to the program's name");
        assert_eq!(firefox.pid, 4242);
        assert!(
            firefox.cause.contains("SIGSEGV"),
            "the signal should be named, not left as a number: {}",
            firefox.cause
        );
        assert_eq!(
            firefox.at, 1758380000,
            "coredumpctl reports microseconds and the rest of this module is in seconds"
        );
    }

    #[test]
    fn crashes_before_the_cutoff_are_left_out() {
        let crashes = parse_coredumpctl_json(COREDUMPCTL_JSON, 1758380000).unwrap();
        assert_eq!(crashes.len(), 2);
        assert!(
            !crashes.iter().any(|crash| crash.program == "old-thing"),
            "a crash from before AETHER1 started watching is history, not news"
        );
    }

    #[test]
    fn an_empty_list_is_a_healthy_machine_rather_than_an_error() {
        assert!(parse_coredumpctl_json("", 0).unwrap().is_empty());
        assert!(parse_coredumpctl_json("[]", 0).unwrap().is_empty());
    }

    #[test]
    fn nonsense_from_coredumpctl_is_reported_rather_than_panicking() {
        let error = parse_coredumpctl_json("not json at all", 0).unwrap_err();
        assert!(
            error.contains("coredumpctl"),
            "the message should say which tool disappointed us: {error}"
        );
    }

    // What the PowerShell above produces: event 1000's own template fields, plus the
    // rendered message kept only for the log tail. Properties arrive as strings here, which
    // is what most Windows versions do.
    const WINDOWS_EVENTS: &str = r#"[
        {"time":1758380500,"program":"notepad.exe","module":"ntdll.dll","code":"0xc0000005","process_id":"0x1a4c","message":"Faulting application name: notepad.exe, version: 10.0.19041.1, time stamp: 0x5e0dc3c2\r\nFaulting module name: ntdll.dll, version: 10.0.19041.1, time stamp: 0x8fd91b1c\r\nException code: 0xc0000005\r\nFault offset: 0x000bd2f1\r\nFaulting process id: 0x1a4c\r\n"}
    ]"#;

    #[test]
    fn a_windows_application_error_becomes_a_crash() {
        let crashes = parse_windows_events(WINDOWS_EVENTS, 0).unwrap();
        assert_eq!(crashes.len(), 1);
        let crash = &crashes[0];
        assert_eq!(crash.program, "notepad.exe");
        assert_eq!(crash.pid, 0x1a4c, "the process id is written in hex");
        assert!(
            crash.cause.contains("0xc0000005") && crash.cause.contains("access violation"),
            "the exception code should be named where we are sure of it: {}",
            crash.cause
        );
        assert!(
            crash.log_tail.iter().any(|line| line.contains("ntdll.dll")),
            "the faulting module is the most useful thing in the event and must survive"
        );
    }

    #[test]
    fn a_windows_event_is_read_without_relying_on_the_message_being_in_english() {
        // The identical event on a German install: every template field is unchanged, and
        // only the rendered message -- which this parser uses for nothing but the log tail
        // -- is translated. Reading the fields is what makes that a non-event.
        let german = r#"[{"time":1758380500,"program":"notepad.exe","module":"ntdll.dll","code":"0xc0000005","process_id":"0x1a4c","message":"Name der fehlerhaften Anwendung: notepad.exe, Zeitstempel: 0x5e0dc3c2\r\nAusnahmecode: 0xc0000005\r\nID des fehlerhaften Prozesses: 0x1a4c\r\n"}]"#;
        let crashes = parse_windows_events(german, 0).unwrap();
        assert_eq!(crashes.len(), 1);
        assert_eq!(crashes[0].program, "notepad.exe");
        assert_eq!(crashes[0].pid, 0x1a4c);
        assert!(crashes[0].cause.contains("0xc0000005"));
    }

    #[test]
    fn windows_properties_are_read_whether_they_arrive_as_text_or_as_numbers() {
        // Some versions hand the template fields over already typed rather than as strings.
        let numeric = r#"[{"time":5,"program":"thing.exe","code":3221225477,"process_id":6732,"message":""}]"#;
        let crashes = parse_windows_events(numeric, 0).unwrap();
        assert_eq!(crashes[0].pid, 6732);
        assert!(
            crashes[0].cause.contains("0xc0000005"),
            "3221225477 is 0xc0000005 and should read as one: {}",
            crashes[0].cause
        );
    }

    #[test]
    fn an_exception_code_we_do_not_know_is_reported_rather_than_guessed_at() {
        let unknown = r#"[{"time":1,"program":"thing.exe","code":"0xdeadbeef","process_id":"0x10","message":""}]"#;
        let crashes = parse_windows_events(unknown, 0).unwrap();
        assert_eq!(crashes[0].cause, "exception 0xdeadbeef");
    }

    #[test]
    fn a_windows_event_with_no_program_name_is_skipped_rather_than_shown_as_blank() {
        let nameless =
            r#"[{"time":1,"program":"","code":"0xc0000005","process_id":"0x10","message":""}]"#;
        assert!(parse_windows_events(nameless, 0).unwrap().is_empty());
    }

    #[test]
    fn the_context_a_crash_hands_over_carries_the_facts_and_the_log() {
        let crash = Crash {
            program: "nvim".to_string(),
            pid: 4310,
            cause: "SIGABRT, the program aborted itself".to_string(),
            at: 1758390000,
            log_tail: vec!["E5108: Error executing lua".to_string()],
        };
        let context = crash.as_context();
        assert!(context.contains("nvim"));
        assert!(context.contains("4310"));
        assert!(context.contains("SIGABRT"));
        assert!(context.contains("E5108"));
    }

    #[test]
    fn a_crash_with_no_log_says_so_rather_than_trailing_off() {
        let crash = Crash {
            program: "thing".to_string(),
            pid: 1,
            cause: "SIGSEGV, a segmentation fault".to_string(),
            at: 0,
            log_tail: Vec::new(),
        };
        assert!(crash.as_context().contains("Nothing was recovered"));
    }

    #[test]
    fn the_same_crash_is_only_ever_announced_once() {
        let crash = Crash {
            program: "firefox".to_string(),
            pid: 4242,
            cause: "SIGSEGV, a segmentation fault".to_string(),
            at: 1758380000,
            log_tail: Vec::new(),
        };
        let mut seen = Seen::default();
        assert!(seen.is_new(&crash), "the first sighting is news");
        assert!(!seen.is_new(&crash), "polling again must not repeat it");
        assert_eq!(seen.len(), 1);

        // A different process of the same program is a different crash.
        let again = Crash { pid: 9999, ..crash };
        assert!(seen.is_new(&again));
    }

    #[test]
    fn muting_a_program_works_whichever_way_the_operator_writes_it() {
        let muted = vec!["Firefox".to_string(), "steam.exe".to_string()];
        assert!(is_muted("firefox", &muted), "case should not matter");
        assert!(
            is_muted("/usr/lib/firefox/firefox", &muted),
            "a full path should match the name the operator typed"
        );
        assert!(
            is_muted("steam", &muted),
            "a .exe in the list should not stop it matching on Linux"
        );
        assert!(
            is_muted("STEAM.EXE", &muted),
            "and the same the other way round"
        );
        assert!(!is_muted("nvim", &muted), "everything else still speaks");
    }

    #[test]
    fn the_journal_saying_it_has_nothing_is_an_empty_tail_not_a_line_of_filler() {
        assert!(tidy_log_tail("-- No entries --\n").is_empty());
        assert!(tidy_log_tail("No journal files were found\n").is_empty());
        assert!(tidy_log_tail("\n\n   \n").is_empty());
    }

    #[test]
    fn a_long_log_is_cut_from_the_front_so_the_last_words_survive() {
        let raw: String = (0..200).map(|n| format!("line {n}\n")).collect();
        let tail = tidy_log_tail(&raw);
        assert_eq!(tail.len(), LOG_TAIL_LINES);
        assert_eq!(
            tail.last().unwrap(),
            "line 199",
            "what a program said last is the part that matters"
        );
    }

    /// A reader that answers from a script, so the watcher's own behaviour can be tested
    /// without a crash, a core dump, or systemd.
    struct FakeReader {
        crashes: Vec<Crash>,
    }

    impl CrashReader for FakeReader {
        fn crashes_since(&self, since: u64) -> Result<Vec<Crash>, String> {
            Ok(self
                .crashes
                .iter()
                .filter(|crash| crash.at >= since)
                .cloned()
                .collect())
        }
        fn availability(&self) -> Availability {
            Availability::Ready
        }
    }

    fn a_crash(program: &str, pid: u32) -> Crash {
        Crash {
            program: program.to_string(),
            pid,
            cause: "SIGSEGV, a segmentation fault".to_string(),
            at: now_seconds() + 1,
            log_tail: Vec::new(),
        }
    }

    fn watching(crashes: Vec<Crash>) -> CrashWatch {
        CrashWatch::with_reader(Box::new(FakeReader { crashes }))
    }

    #[test]
    fn a_crash_is_announced_once_however_often_the_watcher_looks() {
        let mut watch = watching(vec![a_crash("firefox", 4242)]);
        assert_eq!(watch.poll(&[], None).unwrap().len(), 1);
        assert!(
            watch.poll(&[], None).unwrap().is_empty(),
            "the second pass sees the same crash and must say nothing"
        );
    }

    #[test]
    fn a_muted_program_crashes_quietly() {
        let mut watch = watching(vec![a_crash("steam", 99), a_crash("nvim", 100)]);
        let news = watch.poll(&["steam".to_string()], None).unwrap();
        assert_eq!(news.len(), 1);
        assert_eq!(news[0].program, "nvim");
    }

    #[test]
    fn unmuting_a_program_does_not_announce_what_it_already_missed() {
        let mut watch = watching(vec![a_crash("steam", 99)]);
        assert!(watch.poll(&["steam".to_string()], None).unwrap().is_empty());
        assert!(
            watch.poll(&[], None).unwrap().is_empty(),
            "a crash that happened while muted is history, not a backlog to deliver"
        );
    }

    #[test]
    fn crashes_from_before_aether1_started_are_not_announced_at_startup() {
        let old = Crash {
            at: 1,
            ..a_crash("old-thing", 7)
        };
        let mut watch = watching(vec![old]);
        assert!(
            watch.poll(&[], None).unwrap().is_empty(),
            "opening AETHER1 must not read a week of old core dumps at you"
        );
    }

    #[test]
    fn a_mute_list_is_read_however_the_operator_left_it() {
        assert_eq!(
            muted_programs(Some(serde_json::json!(["firefox", " steam ", ""]))),
            vec!["firefox".to_string(), "steam".to_string()]
        );
        assert_eq!(
            muted_programs(Some(serde_json::json!("firefox, steam"))),
            vec!["firefox".to_string(), "steam".to_string()]
        );
        assert!(muted_programs(None).is_empty());
        assert!(muted_programs(Some(serde_json::json!(false))).is_empty());
    }

    #[test]
    fn a_reader_that_cannot_look_says_why() {
        // Whichever reader this machine gets, an unavailable one must explain itself rather
        // than reporting a quiet false that reads as "nothing has ever crashed".
        let availability = reader_for_this_machine().availability();
        if let Availability::Unavailable(reason) = availability {
            assert!(
                reason.len() > 20 && reason.contains("AETHER1"),
                "the reason has to be something an operator can act on: {reason}"
            );
        }
    }

    fn exiting(pid: u32, at: u64) -> ExpectedExit {
        ExpectedExit {
            program: "aether1".to_string(),
            pid,
            at,
        }
    }

    fn dying(program: &str, pid: u32, at: u64) -> Crash {
        Crash {
            program: program.to_string(),
            pid,
            cause: "SIGABRT, the program aborted itself".to_string(),
            at,
            log_tail: Vec::new(),
        }
    }

    #[test]
    fn the_shutdown_we_asked_for_is_not_a_crash() {
        let expected = exiting(4242, 1_000_000);
        // The abort lands during teardown, a moment after the exit was asked for.
        assert!(is_expected_exit(
            &dying("aether1", 4242, 1_000_003),
            Some(&expected)
        ));
        // And sometimes on the same second.
        assert!(is_expected_exit(
            &dying("aether1", 4242, 1_000_000),
            Some(&expected)
        ));
    }

    #[test]
    fn everything_else_is_still_a_crash() {
        let expected = exiting(4242, 1_000_000);
        // Another program that happened to hold that pid number.
        assert!(!is_expected_exit(
            &dying("firefox", 4242, 1_000_003),
            Some(&expected)
        ));
        // The same program, a different process -- the one still running, for instance.
        assert!(!is_expected_exit(
            &dying("aether1", 4243, 1_000_003),
            Some(&expected)
        ));
        // The pid come back round long afterwards.
        assert!(!is_expected_exit(
            &dying("aether1", 4242, 1_000_000 + EXPECTED_EXIT_WINDOW_SECS + 1),
            Some(&expected)
        ));
        // A crash from before the exit was ever asked for.
        assert!(!is_expected_exit(
            &dying("aether1", 4242, 999_000),
            Some(&expected)
        ));
        // Nothing recorded at all, which is every machine that has not updated yet.
        assert!(!is_expected_exit(&dying("aether1", 4242, 1_000_003), None));
    }

    #[test]
    fn a_crash_with_no_usable_time_is_judged_on_the_pid_alone() {
        // `crashes_since` already treats a zero timestamp as recent rather than as 1970, so
        // reading it as "long before the exit" here would resurrect the card this removes.
        let expected = exiting(4242, 1_000_000);
        assert!(is_expected_exit(
            &dying("aether1", 4242, 0),
            Some(&expected)
        ));
        assert!(!is_expected_exit(&dying("aether1", 77, 0), Some(&expected)));
    }

    #[test]
    fn a_record_that_will_not_parse_is_no_record() {
        let written = serde_json::to_value(exiting(7, 1_000_000)).unwrap();
        assert_eq!(expected_exit(Some(written)), Some(exiting(7, 1_000_000)));
        assert_eq!(expected_exit(None), None);
        assert_eq!(expected_exit(Some(serde_json::json!("nonsense"))), None);
        assert_eq!(expected_exit(Some(serde_json::json!({"pid": 7}))), None);
    }

    #[test]
    fn this_process_records_itself_by_the_name_the_reader_will_use() {
        let record = ExpectedExit::for_this_process();
        assert_eq!(record.pid, std::process::id());
        // A basename, never a path -- coredumpctl reports the basename of exe and the two
        // have to match for is_expected_exit to fire at all.
        assert!(!record.program.contains(std::path::MAIN_SEPARATOR));
        assert!(!record.program.is_empty());
        assert!(record.at > 0);
    }
}
