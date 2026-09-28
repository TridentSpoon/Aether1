// Lifecycle for background processes AETHER1 starts on its own behalf (currently just
// Ollama), plus Game Mode, which stops them again without quitting AETHER1 itself.
//
// commands::start_local_server (the manual "start" button) spawns Ollama and drops the
// Child immediately -- fine for a one-off click, but nothing can be stopped later without
// keeping the handle. This module keeps it, and keeps it *only* for a process AETHER1
// itself spawned: an Ollama the operator already had running, or started independently of
// AETHER1, is never touched here, matching the "something already answers -> don't start a
// second one" rule commands::start_local_server_tracked already follows.
//
// Three moments make up the whole life of that process, and until now only the first was
// written down:
//
//   * it wakes when AETHER1 starts (if autostart is on) or on the first turn that needs it;
//   * it stops on its own after IDLE_SETTING minutes with no turn, because a model server
//     holding a model in memory with nobody talking to it is the most expensive idle thing
//     on the machine -- and the next turn brings it straight back (see `note_use`);
//   * it dies with AETHER1. "Quit AETHER1" used to leave it running: app.exit(0) does not
//     reap a child, so the model server outlived the program that started it, still holding
//     its model, until the machine was rebooted or somebody found it in a task manager.

use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::llm::LlmEngine;

/// Minutes of no local turns before the model server AETHER1 started is stopped again.
/// Zero means never -- for an operator who would rather pay the memory than the first-token
/// wait.
pub const IDLE_SETTING: &str = "ollama_idle_minutes";
pub const DEFAULT_IDLE_MINUTES: u64 = 15;

/// How often the idle watch looks. A minute's granularity on a timeout measured in minutes;
/// the thread is asleep the rest of the time.
pub const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// Tauri-managed state: the Ollama process AETHER1 itself started, if any.
pub struct ManagedOllama {
    child: Mutex<Option<Child>>,
    /// When a turn last went to a local model. Starts at "now" so a server started at
    /// launch gets its full idle window before anything stops it, rather than being timed
    /// out by a clock that began in 1970.
    last_use: Mutex<Instant>,
    /// Set only when the idle watch is what stopped the server, so the next turn knows it
    /// is the one that has to bring it back. An operator's own stop, and Game Mode's, leave
    /// this false: they mean stay stopped.
    stopped_when_idle: AtomicBool,
}

impl Default for ManagedOllama {
    fn default() -> Self {
        ManagedOllama {
            child: Mutex::new(None),
            last_use: Mutex::new(Instant::now()),
            stopped_when_idle: AtomicBool::new(false),
        }
    }
}

impl ManagedOllama {
    pub fn is_managed(&self) -> bool {
        self.child.lock().unwrap().is_some()
    }

    /// True while the idle watch is holding the server stopped -- i.e. the next local turn
    /// will restart it. What the HUD needs to say "asleep" rather than "off".
    pub fn is_sleeping(&self) -> bool {
        self.stopped_when_idle.load(Ordering::Relaxed)
    }

    pub fn idle_for(&self) -> Duration {
        self.last_use.lock().unwrap().elapsed()
    }

    fn touch(&self) {
        *self.last_use.lock().unwrap() = Instant::now();
    }
}

/// Starts Ollama if the autostart setting is on and nothing is already listening, and keeps
/// the spawned `Child` so `stop_managed_ollama` can stop this exact process later. Safe to
/// call speculatively -- it no-ops (without recording anything as managed) whenever
/// local-only mode is on, the binary is missing, or a server is already running, same as the
/// manual "start" button.
pub fn start_ollama_if_needed(engine: &LlmEngine, managed: &ManagedOllama) -> serde_json::Value {
    let (result, child) = crate::commands::start_local_server_tracked(engine);
    if let Some(child) = child {
        *managed.child.lock().unwrap() = Some(child);
    }
    // Whatever happens next, this is not a server the idle watch is holding down any more,
    // and the clock starts here rather than at whenever the last turn was.
    managed.stopped_when_idle.store(false, Ordering::Relaxed);
    managed.touch();
    result
}

/// Kills the tracked child, if AETHER1 started one; no-ops otherwise. Called by Game Mode,
/// and on the way out (see main.rs's RunEvent::Exit handler).
pub fn stop_managed_ollama(managed: &ManagedOllama) {
    if let Some(mut child) = managed.child.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    // An explicit stop is not a nap: nothing should quietly restart it.
    managed.stopped_when_idle.store(false, Ordering::Relaxed);
}

/// A turn is about to go out. Records it against the idle clock and, when the idle watch is
/// what stopped the server, starts it again first.
///
/// Returns true when this call restarted the server, so a caller can say so.
///
/// Only the local case does anything: a turn answered by a cloud model is not a reason to
/// spin a model server up on this machine, and not a reason to keep one alive either.
pub fn note_use(engine: &LlmEngine, managed: &ManagedOllama) -> bool {
    if !engine.uses_local_server() {
        return false;
    }
    if managed.stopped_when_idle.load(Ordering::Relaxed) {
        // start_ollama_if_needed touches the clock and clears the flag itself.
        start_ollama_if_needed(engine, managed);
        return true;
    }
    managed.touch();
    false
}

/// One look by the idle watch. Stops the model server when AETHER1 started it, it has had no
/// local turn for the configured number of minutes, and it is not already stopped.
///
/// Returns true when this call stopped it.
pub fn stop_if_idle(engine: &LlmEngine, managed: &ManagedOllama) -> bool {
    let minutes = engine
        .db()
        .get_setting_u64(IDLE_SETTING, DEFAULT_IDLE_MINUTES);
    if minutes == 0 || !managed.is_managed() {
        return false;
    }
    if managed.idle_for() < Duration::from_secs(minutes * 60) {
        return false;
    }
    if let Some(mut child) = managed.child.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    managed.stopped_when_idle.store(true, Ordering::Relaxed);
    true
}

/// Test-only: a managed server that exists as far as this module can tell, with its idle
/// clock already wound back, so the idle watch can be exercised without a real model server.
#[cfg(test)]
impl ManagedOllama {
    fn pretend(child: Child, idle: Duration) -> ManagedOllama {
        let managed = ManagedOllama::default();
        *managed.child.lock().unwrap() = Some(child);
        *managed.last_use.lock().unwrap() = Instant::now()
            .checked_sub(idle)
            .expect("the test clock is not near the start of time");
        managed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MemoryDb;

    // A real temp file, not ":memory:" -- see the warning on MemoryDb::open.
    fn temp_db(name: &str) -> MemoryDb {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "aether1_test_bgsvc_{name}_{}_{n}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(path).expect("temp db should open")
    }

    /// Mirrors start_local_server's own "already running -> don't start a second one"
    /// coverage: with local-only mode on, this must refuse without ever touching the
    /// managed-child slot, exactly like the manual button refuses without spawning anything.
    #[test]
    fn does_not_manage_a_child_when_local_only_refuses() {
        let db = temp_db("local_only_refuses");
        db.set_setting("local_only", &serde_json::json!(true))
            .unwrap();
        let engine = LlmEngine::new(db);
        let managed = ManagedOllama::default();

        let result = start_ollama_if_needed(&engine, &managed);

        assert_eq!(result["ok"], serde_json::json!(false));
        assert!(!managed.is_managed());
    }

    #[test]
    fn stop_is_a_no_op_when_nothing_is_managed() {
        let managed = ManagedOllama::default();
        stop_managed_ollama(&managed); // must not panic
        assert!(!managed.is_managed());
    }

    /// A stand-in for the model server: something with a pid that stays alive long enough
    /// to be killed. Unix only -- the idle rule itself is platform-independent, and the
    /// alternative is a second spawn incantation for a rule that has no platform in it.
    #[cfg(unix)]
    fn placeholder_child() -> Child {
        std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("sleep should be available")
    }

    #[test]
    fn the_idle_watch_leaves_alone_a_server_it_did_not_start() {
        let db = temp_db("idle_unmanaged");
        let engine = LlmEngine::new(db);
        let managed = ManagedOllama::default();

        assert!(!stop_if_idle(&engine, &managed));
        assert!(!managed.is_sleeping());
    }

    #[cfg(unix)]
    #[test]
    fn a_zero_timeout_means_never() {
        let db = temp_db("idle_zero");
        db.set_setting(IDLE_SETTING, &serde_json::json!(0)).unwrap();
        let engine = LlmEngine::new(db);
        let managed =
            ManagedOllama::pretend(placeholder_child(), Duration::from_secs(60 * 60 * 24));

        assert!(!stop_if_idle(&engine, &managed));
        assert!(managed.is_managed());

        stop_managed_ollama(&managed); // don't leave the placeholder behind
    }

    #[cfg(unix)]
    #[test]
    fn a_server_with_no_turns_for_the_timeout_is_stopped_and_marked_asleep() {
        let db = temp_db("idle_stops");
        db.set_setting(IDLE_SETTING, &serde_json::json!(5)).unwrap();
        let engine = LlmEngine::new(db);
        let managed = ManagedOllama::pretend(placeholder_child(), Duration::from_secs(6 * 60));

        assert!(stop_if_idle(&engine, &managed));
        assert!(!managed.is_managed());
        // "asleep", not "off": the next local turn is expected to bring it back.
        assert!(managed.is_sleeping());
    }

    #[cfg(unix)]
    #[test]
    fn a_recent_turn_holds_the_server_open() {
        let db = temp_db("idle_recent");
        db.set_setting(IDLE_SETTING, &serde_json::json!(5)).unwrap();
        // Ollama on localhost: a local turn, so note_use counts it.
        db.set_setting("llm_provider", &serde_json::json!("ollama"))
            .unwrap();
        db.set_setting("llm_endpoint", &serde_json::json!("http://localhost:11434"))
            .unwrap();
        let engine = LlmEngine::new(db);
        let managed = ManagedOllama::pretend(placeholder_child(), Duration::from_secs(6 * 60));

        assert!(!note_use(&engine, &managed)); // nothing to restart, just a turn noted
        assert!(!stop_if_idle(&engine, &managed));
        assert!(managed.is_managed());

        stop_managed_ollama(&managed);
    }

    /// The reverse: a turn answered in the cloud is not a reason to keep a model server on
    /// this machine awake, so it must not reset the idle clock.
    #[cfg(unix)]
    #[test]
    fn a_cloud_turn_does_not_hold_the_server_open() {
        let db = temp_db("idle_cloud");
        db.set_setting(IDLE_SETTING, &serde_json::json!(5)).unwrap();
        db.set_setting("llm_provider", &serde_json::json!("anthropic"))
            .unwrap();
        let engine = LlmEngine::new(db);
        let managed = ManagedOllama::pretend(placeholder_child(), Duration::from_secs(6 * 60));

        assert!(!note_use(&engine, &managed));
        assert!(stop_if_idle(&engine, &managed));
        assert!(!managed.is_managed());
    }

    /// An explicit stop means stay stopped -- nothing may quietly restart it on the next
    /// turn, which is what tells Game Mode's stop apart from the idle watch's.
    #[cfg(unix)]
    #[test]
    fn an_explicit_stop_is_not_a_nap() {
        let managed = ManagedOllama::pretend(placeholder_child(), Duration::from_secs(0));

        stop_managed_ollama(&managed);

        assert!(!managed.is_managed());
        assert!(!managed.is_sleeping());
    }

    /// The key the settings payload and the defaults both use. Two spellings of one setting
    /// is a setting that silently stops working, and the defaults in commands.rs have to
    /// write it as a literal (json! takes no expression for a key).
    #[test]
    fn the_setting_key_matches_the_one_the_defaults_write() {
        assert_eq!(IDLE_SETTING, "ollama_idle_minutes");
        assert!(include_str!("commands.rs").contains("\"ollama_idle_minutes\":"));
    }
}
