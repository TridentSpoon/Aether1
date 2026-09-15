// Lifecycle for background processes AETHER1 starts on its own behalf (currently just
// Ollama), plus Game Mode, which stops them again without quitting AETHER1 itself.
//
// commands::start_local_server (the manual "start" button) spawns Ollama and drops the
// Child immediately -- fine for a one-off click, but nothing can be stopped later without
// keeping the handle. This module keeps it, and keeps it *only* for a process AETHER1
// itself spawned: an Ollama the operator already had running, or started independently of
// AETHER1, is never touched here, matching the "something already answers -> don't start a
// second one" rule commands::start_local_server_tracked already follows.

use std::process::Child;
use std::sync::Mutex;

use crate::llm::LlmEngine;

/// Tauri-managed state: the Ollama process AETHER1 itself started, if any.
#[derive(Default)]
pub struct ManagedOllama(Mutex<Option<Child>>);

impl ManagedOllama {
    pub fn is_managed(&self) -> bool {
        self.0.lock().unwrap().is_some()
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
        *managed.0.lock().unwrap() = Some(child);
    }
    result
}

/// Kills the tracked child, if AETHER1 started one; no-ops otherwise. Called by Game Mode
/// (and would be called on quit, if AETHER1 quitting mid-session were a thing this app does).
pub fn stop_managed_ollama(managed: &ManagedOllama) {
    if let Some(mut child) = managed.0.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
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
}
