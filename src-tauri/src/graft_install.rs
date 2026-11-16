//! Installing Graft, so that a human does not have to.
//!
//! Graft is not shipped inside Aether1 and is not going to be: it is a Node program with its
//! own release cadence, and vendoring it would mean shipping a Node runtime and then owning
//! its updates forever. So it is fetched on demand, the same answer the project already gives
//! for Ollama and LM Studio -- detect it, set it up, do not hand the operator a command and
//! walk away.
//!
//! **This installs a program, which `voice_download` deliberately does not.** That module
//! draws a line and gives the reason: a `.onnx` is weights, never executed, never on a PATH,
//! so a corrupt one makes speech sound wrong while a corrupt *program* is code running as the
//! operator. The line is still right and this does not erase it -- it goes around it, by not
//! fetching a program at all. What runs here is `npm install -g`, so the download, the
//! integrity check against the registry's own hashes, and the placement on PATH are all npm's,
//! exactly as they would be if the operator typed it. Aether1 contributes no URL, no byte
//! handling and no chmod. That is the whole reason this half is safe to automate when fetching
//! a binary over HTTP and marking it executable would not be, and if a future change starts
//! downloading Graft directly, this comment is the thing it has to argue with first.
//!
//! The second thing it does is turn Graft's telemetry off. Graft reports anonymous usage by
//! default; Aether1 is a local-first application whose main promise is that nothing leaves the
//! machine unless you send it. Installing a phone-home tool on the operator's behalf and
//! saying nothing would make that promise false in a way they could not see.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::downloads::Phase;
use crate::llm::MemoryDb;

/// The only package name that is the tool.
///
/// Written down once, because the near misses are a documented trap: `cargo install graft`
/// does not exist, bare `graft` on npm is an unrelated package, and `pip install graft` is a
/// squatter that shadows the real binary on PATH.
pub const PACKAGE: &str = "@nanonets/graft";

/// How long the install may run before it is killed.
///
/// Generous, because this is one npm install over whatever connection the operator has, and a
/// install killed at four minutes on a slow line is a worse outcome than one that took six.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(600);

/// How long `graft telemetry disable` and `graft --version` may each take. Local, no network, so
/// anything beyond this is a hang rather than slowness.
const QUICK_TIMEOUT: Duration = Duration::from_secs(30);

/// The steps the operator is shown, in order. The percentage is derived from position in this
/// list rather than from npm's output, which is deliberate: npm reports a spinner and a tree,
/// not a byte count, and a progress bar that claims a precision it does not have is worse than
/// one that admits to counting steps.
const STEPS: [&str; 4] = [
    "checking for npm",
    "installing from npm",
    "switching telemetry off",
    "checking the install",
];

/// What the install has reported so far.
///
/// The same `phase`/`detail`/`percent` shape as `downloads::Download` and
/// `voice_download::VoiceFetch`, so the HUD's existing progress row draws it with no new
/// rendering code.
#[derive(Debug, Clone, Serialize)]
pub struct Install {
    pub phase: Phase,
    /// What is happening now, in npm's own words where there are any. Passed through rather
    /// than prettified: when an install misbehaves, the line the operator can search for is
    /// worth more than the line we would have written.
    pub detail: String,
    pub step: usize,
    pub steps: usize,
    pub percent: Option<u8>,
    /// The version string Graft reported once it was in, so the panel can say what landed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Install {
    fn new() -> Install {
        Install {
            phase: Phase::Starting,
            detail: "starting".to_string(),
            step: 0,
            steps: STEPS.len(),
            percent: Some(0),
            version: None,
            error: None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.phase, Phase::Done | Phase::Failed)
    }
}

/// One install at a time, because there is one Graft. A second press while the first is
/// running is answered with the first rather than starting a race for the same path on disk.
static STATE: LazyLock<Mutex<Option<Install>>> = LazyLock::new(|| Mutex::new(None));

/// Same reasoning as `downloads::registry`: a status board is not worth taking the app down
/// over, so a poisoned lock is recovered from rather than panicked on.
fn state() -> std::sync::MutexGuard<'static, Option<Install>> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn update(f: impl FnOnce(&mut Install)) {
    if let Some(entry) = state().as_mut() {
        f(entry);
    }
}

fn enter_step(index: usize) {
    update(|entry| {
        entry.step = index + 1;
        entry.phase = Phase::Downloading;
        entry.detail = STEPS[index].to_string();
        entry.percent = Some(((index * 100) / STEPS.len()) as u8);
    });
}

fn fail(message: String) {
    update(|entry| {
        entry.phase = Phase::Failed;
        entry.detail = "failed".to_string();
        entry.percent = None;
        entry.error = Some(message);
    });
}

/// What the install has reported, or `None` when none has ever been started this session.
pub fn snapshot() -> Option<Install> {
    state().clone()
}

/// Clears a finished install so the panel can go back to showing plain status.
///
/// Refuses to clear one that is still running: the worker writes through `update`, which finds
/// nothing once the slot is empty, so clearing mid-flight would silently orphan it.
pub fn forget() -> bool {
    let mut slot = state();
    match slot.as_ref() {
        Some(entry) if entry.is_terminal() => {
            *slot = None;
            true
        }
        _ => false,
    }
}

/// Starts installing Graft.
///
/// Refuses in local-only mode, for the same reason a voice download is refused there: that
/// switch is a promise that nothing reaches off this machine, and npm fetching a package is
/// traffic like any other.
///
/// Checked twice, which is the pattern the rest of the app already uses: `local_only::enabled`
/// refuses first with wording that says what did not happen, and `net::require_forwarding` sits
/// under it as the backstop that also catches a machine nailed shut by the environment in an
/// entry point that never installed a policy. Both matter more here than usual, because
/// `scripts/check_egress_gate.sh` reads Rust source for ureq calls and **cannot see a
/// subprocess** -- npm's traffic is invisible to the guard, so these two calls are the whole
/// of what stands between the mode and the network.
pub fn start(db: &MemoryDb) -> Result<Install, String> {
    if crate::local_only::enabled(db) {
        return Err(crate::local_only::refusal("Graft was not installed"));
    }
    crate::net::require_forwarding("Graft was not installed")?;

    {
        let mut slot = state();
        if let Some(existing) = slot.as_ref() {
            if !existing.is_terminal() {
                return Ok(existing.clone());
            }
        }
        *slot = Some(Install::new());
    }

    std::thread::spawn(run);
    Ok(state().clone().unwrap_or_else(Install::new))
}

/// Drives the install to the end, writing progress into the slot as it goes.
fn run() {
    enter_step(0);
    let npm = match crate::paths::find_installed_binary(&["npm"]) {
        Some(found) => found,
        None => {
            // Named, with what to do about it: npm is not something Aether1 can install on
            // somebody's behalf, and "not found" without the reason sends them looking for a
            // bug in Aether1 rather than for Node.
            fail(
                "npm was not found, and Graft is an npm package. Install Node.js (which \
                 includes npm) from nodejs.org or your package manager, then try again."
                    .to_string(),
            );
            return;
        }
    };

    enter_step(1);
    /* Where it goes. `npm install -g` writes into npm's own prefix, and on a machine whose
    npm came from the package manager that is /usr, which only root may write: the install
    ends in EACCES and the answer the internet gives is sudo. So when the prefix cannot be
    written, Graft goes into a folder of Aether1's own instead -- same package, same
    command, no elevation -- and `graft::program` looks there. */
    let own_prefix = if npm_prefix_writable(&npm) {
        None
    } else {
        crate::graft::own_prefix()
    };
    let mut args: Vec<String> = vec!["install".into(), "-g".into()];
    if let Some(prefix) = &own_prefix {
        if let Err(e) = std::fs::create_dir_all(prefix) {
            fail(format!("{} could not be made: {e}", prefix.display()));
            return;
        }
        args.push("--prefix".into());
        args.push(prefix.to_string_lossy().to_string());
    }
    args.push(PACKAGE.into());
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    match run_streaming(&npm, &borrowed, INSTALL_TIMEOUT) {
        Ok(()) => {}
        Err(why) => {
            fail(format!("npm {} failed: {why}", args.join(" ")));
            return;
        }
    }

    // From here the install has landed and the rest is tidying. A failure in either step is
    // reported in the detail line rather than failing the whole install, because a Graft that
    // is installed and talkative is still a Graft that works, and telling the operator the
    // install failed when it did not would send them round the loop again.
    enter_step(2);
    // `disable`, not `off`. Graft 0.28.1 answers `off` with
    // `unknown action "off": expected status, enable, disable, or debug`, which would have
    // put "switching Graft's telemetry off failed" on the panel of every successful install.
    let telemetry = quiet_graft(&["telemetry", "disable"]);

    enter_step(3);
    let version = quiet_graft(&["--version"])
        .ok()
        .map(|out| out.lines().next().unwrap_or_default().trim().to_string());

    update(|entry| {
        entry.phase = Phase::Done;
        entry.step = STEPS.len();
        entry.percent = Some(100);
        entry.version = version.filter(|v| !v.is_empty());
        entry.detail = match &telemetry {
            Ok(_) => "installed, telemetry off".to_string(),
            // Said out loud rather than swallowed: the operator was not asked whether Graft
            // may phone home, so they are owed the fact that switching it off did not take.
            Err(why) => format!(
                "installed, but switching Graft's telemetry off failed ({why}) -- run \
                 `graft telemetry disable` yourself"
            ),
        };
    });
}

/// Whether this user can write where `npm install -g` would put a package.
///
/// Asked of npm rather than guessed: the prefix is configurable, and nvm, Homebrew and a
/// distribution's package each put it somewhere different. An answer that cannot be had
/// counts as writable, which leaves npm to try its own default and report in its own words.
fn npm_prefix_writable(npm: &Path) -> bool {
    let Ok(output) = crate::paths::quiet_command(npm)
        .args(["config", "get", "prefix"])
        .stdin(Stdio::null())
        .output()
    else {
        return true;
    };
    let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || prefix.is_empty() {
        return true;
    }
    let prefix = PathBuf::from(prefix);
    let target = if cfg!(target_os = "windows") {
        prefix
    } else {
        prefix.join("lib").join("node_modules")
    };
    dir_writable(&target)
}

/// Whether a file could be created in `dir`, or in the nearest folder above it that exists
/// (npm makes the ones that do not). Found by doing it, because permission bits do not
/// account for a read-only filesystem or an immutable distribution.
fn dir_writable(dir: &Path) -> bool {
    let Some(existing) = dir.ancestors().find(|candidate| candidate.is_dir()) else {
        return false;
    };
    let probe = existing.join(format!(".aether1-write-test-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Runs a command, streaming each output line into the detail field, and kills it at `timeout`.
///
/// Both pipes are read, on their own threads, for the reason the updater's rebuild does it:
/// an undrained pipe fills its buffer and stalls the child, so a progress feature that only
/// read stdout would hang the very install it was reporting on.
fn run_streaming(
    program: &std::path::Path,
    args: &[&str],
    timeout: Duration,
) -> Result<(), String> {
    let mut command = crate::paths::quiet_command(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|e| format!("could not be started: {e}"))?;

    let mut readers = Vec::new();
    for pipe in [
        child.stdout.take().map(PipeKind::Out),
        child.stderr.take().map(PipeKind::Err),
    ]
    .into_iter()
    .flatten()
    {
        readers.push(std::thread::spawn(move || {
            let tail = Mutex::new(Vec::new());
            let reader: Box<dyn std::io::Read + Send> = match pipe {
                PipeKind::Out(out) => Box::new(out),
                PipeKind::Err(err) => Box::new(err),
            };
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                update(|entry| entry.detail = truncate(&line, 160));
                let mut tail = tail.lock().unwrap_or_else(|e| e.into_inner());
                tail.push(line);
                // Only the end is kept: npm prints a whole dependency tree on a bad day and
                // the last few lines are what says why it stopped.
                if tail.len() > 12 {
                    tail.remove(0);
                }
            }
            tail.into_inner().unwrap_or_default()
        }));
    }

    let started = Instant::now();
    let timed_out = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(120));
            }
            Err(e) => return Err(format!("could not be waited on: {e}")),
        }
    };

    let tail: Vec<String> = readers
        .into_iter()
        .filter_map(|handle| handle.join().ok())
        .flatten()
        .collect();

    match timed_out {
        None => Err(format!(
            "it was still running after {} seconds and was stopped",
            timeout.as_secs()
        )),
        Some(status) if status.success() => Ok(()),
        Some(status) => {
            let code = status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "a signal".to_string());
            let why = tail.join("; ");
            Err(if why.is_empty() {
                format!("it exited with {code}")
            } else {
                format!("it exited with {code}: {}", truncate(&why, 400))
            })
        }
    }
}

enum PipeKind {
    Out(std::process::ChildStdout),
    Err(std::process::ChildStderr),
}

/// Runs a short Graft command and returns its stdout.
///
/// Resolved through `graft::program()` rather than a bare name, because the install has just
/// put the binary where npm puts globals, which is routinely not on the PATH a desktop
/// launcher started the app with -- the whole reason that resolver exists.
fn quiet_graft(args: &[&str]) -> Result<String, String> {
    let mut command = crate::paths::quiet_command(crate::graft::program());
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|e| format!("could not be started: {e}"))?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if started.elapsed() >= QUICK_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("it did not finish".to_string());
                }
                std::thread::sleep(Duration::from_millis(60));
            }
            Err(e) => return Err(format!("could not be waited on: {e}")),
        }
    }

    let out = child
        .wait_with_output()
        .map_err(|e| format!("its output could not be read: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(truncate(String::from_utf8_lossy(&out.stderr).trim(), 200))
    }
}

/// Cuts to at most `limit` *characters*, because npm prints package names in every script a
/// registry allows and a snippet ending mid-character is a bug looking for a name to be
/// reported under.
fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    format!("{}…", text.chars().take(limit).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(name: &str) -> MemoryDb {
        let path = std::env::temp_dir().join(format!(
            "aether1_graft_install_{name}_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(path).unwrap()
    }

    fn clear() {
        *state() = None;
    }

    /// There is one status board for the whole process, so the tests that write to it take
    /// this before they touch it. Without it they interleave -- one test's `clear` lands
    /// between another's `fail` and the snapshot it reads back, and the suite fails on a
    /// schedule rather than on a fault.
    fn alone() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The package name is the whole trap. Three plausible spellings install something that is
    /// not Graft, and one of them shadows the real binary, so this is pinned rather than left
    /// to be retyped from memory by whoever next touches this file.
    #[test]
    fn a_folder_that_cannot_be_written_is_found_out_by_trying() {
        let dir = std::env::temp_dir().join(format!("aether1_graft_prefix_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // The folder npm would make does not exist yet; the one above it does.
        assert!(dir_writable(&dir.join("lib").join("node_modules")));
        assert!(
            std::fs::read_dir(&dir).unwrap().next().is_none(),
            "the probe is removed"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
            // Root writes through permission bits, so this half only means something as a user.
            let is_root = std::fs::File::create(dir.join("probe")).is_ok();
            if !is_root {
                assert!(!dir_writable(&dir.join("lib").join("node_modules")));
            }
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn the_package_is_the_scoped_one_and_not_a_near_miss() {
        assert_eq!(PACKAGE, "@nanonets/graft");
    }

    /* Local-only mode is a promise that nothing reaches off the machine, and npm fetching a
    package is traffic like any other. This exercises the database half only --
    `local_only::enabled` reads the db handed in, so it is deterministic. The gate underneath
    it (`net::require_forwarding`) is **not** tested from here on purpose: `net::POLICY` is a
    process-wide `OnceLock` and the environment override is process-wide too, so a test that
    installed a policy would decide the answer for every other test in this binary running
    beside it. net.rs says this in its own words and keeps one ordered test for the gate's
    states; this is that rule being obeyed, not an omission. */
    #[test]
    fn local_only_mode_refuses_to_install() {
        let _alone = alone();
        clear();
        let db = temp_db("local_only");
        db.set_setting(crate::local_only::SETTING, &serde_json::Value::Bool(true))
            .unwrap();

        let refused = start(&db).unwrap_err();
        assert!(
            refused.contains("Graft was not installed"),
            "the refusal has to name what did not happen: {refused}"
        );
        assert!(
            snapshot().is_none(),
            "a refused install must not leave a row on the status board"
        );
    }

    /// Progress is counted in steps and says so. A bar that claimed bytes would be claiming a
    /// precision npm's output does not carry.
    #[test]
    fn the_steps_are_counted_and_the_last_one_is_a_check_that_it_worked() {
        let _alone = alone();
        clear();
        *state() = Some(Install::new());

        let fresh = snapshot().expect("just set");
        assert_eq!(fresh.steps, STEPS.len());
        assert_eq!(fresh.step, 0);
        assert_eq!(fresh.percent, Some(0));

        enter_step(0);
        let first = snapshot().expect("still there");
        assert_eq!(first.step, 1);
        assert_eq!(first.detail, "checking for npm");
        assert_eq!(first.percent, Some(0));

        enter_step(STEPS.len() - 1);
        let last = snapshot().expect("still there");
        assert_eq!(last.step, STEPS.len());
        assert!(last.percent.unwrap() < 100, "only Done reaches 100");
        assert_eq!(last.detail, "checking the install");
        clear();
    }

    /// A finished install can be cleared; a running one cannot, because the worker writes
    /// through a slot that clearing would empty, and it would carry on with nobody watching.
    #[test]
    fn only_a_finished_install_can_be_forgotten() {
        let _alone = alone();
        clear();
        *state() = Some(Install::new());
        assert!(!forget(), "a running install is not clearable");
        assert!(snapshot().is_some());

        fail("something went wrong".to_string());
        assert!(forget(), "a failed one is");
        assert!(snapshot().is_none());
    }

    /// A failure leaves the reason where the panel can show it, and drops the percentage
    /// rather than freezing a bar partway, which reads as "still going".
    #[test]
    fn a_failure_keeps_its_reason_and_stops_claiming_progress() {
        let _alone = alone();
        clear();
        *state() = Some(Install::new());
        enter_step(1);
        fail("npm exited with 1".to_string());

        let entry = snapshot().expect("still there");
        assert_eq!(entry.phase, Phase::Failed);
        assert_eq!(entry.percent, None);
        assert_eq!(entry.error.as_deref(), Some("npm exited with 1"));
        clear();
    }

    #[test]
    fn a_long_line_is_cut_on_a_character_boundary() {
        let wide = "é".repeat(300);
        let cut = truncate(&wide, 160);
        assert_eq!(cut.chars().count(), 161, "160 characters plus the ellipsis");
        assert!(cut.ends_with('…'));
        assert_eq!(truncate("short", 160), "short");
    }
}
