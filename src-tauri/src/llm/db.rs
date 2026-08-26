// Persistent memory/settings store, ported from backend/memory_db.py. Points at the same
// SQLite file (backend/aether1_memory.db) and the same schema, so the Rust and Python
// engines can run side by side against one shared source of truth -- no data migration,
// no divergent state. Structs (Message, MemoryEntry) replace Python's List[Dict] returns.
//
// Matches memory_db.py's own connection pattern of opening a fresh connection per call
// rather than holding one open across the app's lifetime -- SQLite handles that fine at
// this traffic level, and it sidesteps any Send/Sync ceremony around a shared handle.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub sender: String,
    pub text: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryEntry {
    pub key: String,
    pub value: String,
    pub category: String,
}

pub struct MemoryDb {
    db_path: PathBuf,
}

impl MemoryDb {
    /// `db_path` must be a real file path, not the special SQLite string ":memory:" --
    /// every method here opens its own fresh connection (see `connect`), and SQLite
    /// destroys a ":memory:" database the moment its one connection closes, so it would
    /// silently lose every write instead of erroring. Use a real (possibly temp-dir) file
    /// for anything that needs to survive more than one call.
    pub fn open(db_path: impl AsRef<Path>) -> rusqlite::Result<MemoryDb> {
        let db_path = db_path.as_ref().to_path_buf();
        let db = MemoryDb { db_path };
        db.connect()?.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                sender TEXT NOT NULL,
                text TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                metadata TEXT
            );
            CREATE TABLE IF NOT EXISTS long_term_memory (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                category TEXT DEFAULT 'general',
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;
        Ok(db)
    }

    fn connect(&self) -> rusqlite::Result<Connection> {
        Connection::open(&self.db_path)
    }

    pub fn add_message(&self, session_id: &str, sender: &str, text: &str) -> rusqlite::Result<()> {
        self.connect()?.execute(
            "INSERT INTO messages (session_id, sender, text) VALUES (?1, ?2, ?3)",
            params![session_id, sender, text],
        )?;
        Ok(())
    }

    pub fn get_messages(&self, session_id: &str, limit: u32) -> rusqlite::Result<Vec<Message>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT sender, text, timestamp FROM messages \
             WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let mut rows: Vec<Message> = stmt
            .query_map(params![session_id, limit], |row| {
                Ok(Message {
                    sender: row.get(0)?,
                    text: row.get(1)?,
                    timestamp: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        rows.reverse(); // oldest first, matching memory_db.py's get_messages order
        Ok(rows)
    }

    pub fn get_all_memories(&self) -> rusqlite::Result<Vec<MemoryEntry>> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare("SELECT key, value, category FROM long_term_memory ORDER BY category, key")?;
        let rows = stmt.query_map([], |row| {
            Ok(MemoryEntry {
                key: row.get(0)?,
                value: row.get(1)?,
                category: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    pub fn set_memory(&self, key: &str, value: &str, category: &str) -> rusqlite::Result<()> {
        self.connect()?.execute(
            "INSERT INTO long_term_memory (key, value, category, updated_at) \
             VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value, category=excluded.category, \
             updated_at=CURRENT_TIMESTAMP",
            params![key, value, category],
        )?;
        Ok(())
    }

    pub fn set_setting(&self, key: &str, value: &JsonValue) -> rusqlite::Result<()> {
        // Mirrors memory_db.py's set_setting: plain strings are stored raw (no quotes),
        // everything else as JSON, so get_setting's plain-string fallback still round-trips.
        let val_str = match value {
            JsonValue::String(s) => s.clone(),
            other => other.to_string(),
        };
        self.connect()?.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, val_str],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> rusqlite::Result<Option<JsonValue>> {
        let conn = self.connect()?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(raw.map(|s| serde_json::from_str(&s).unwrap_or(JsonValue::String(s))))
    }

    pub fn get_setting_string(&self, key: &str, default: &str) -> String {
        match self.get_setting(key) {
            Ok(Some(JsonValue::String(s))) => s,
            Ok(Some(other)) => other.to_string(),
            _ => default.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A real temp file per test, not ":memory:" -- see the warning on MemoryDb::open.
    fn temp_db(name: &str) -> MemoryDb {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("aether1_test_{name}_{}_{n}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(path).expect("temp db should open")
    }

    #[test]
    fn messages_round_trip_in_order() {
        let db = temp_db("messages");
        db.add_message("s1", "user", "hello").unwrap();
        db.add_message("s1", "halcy", "hi there").unwrap();
        db.add_message("s2", "user", "other session").unwrap();

        let msgs = db.get_messages("s1", 10).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].sender, "user");
        assert_eq!(msgs[0].text, "hello");
        assert_eq!(msgs[1].sender, "halcy");
        assert_eq!(msgs[1].text, "hi there");
    }

    #[test]
    fn settings_round_trip_string_and_json() {
        let db = temp_db("settings");
        assert_eq!(db.get_setting_string("agent_name", "HALCY"), "HALCY");

        db.set_setting("agent_name", &JsonValue::String("R.E.D. 9000".to_string()))
            .unwrap();
        assert_eq!(db.get_setting_string("agent_name", "HALCY"), "R.E.D. 9000");

        db.set_setting("some_flag", &JsonValue::Bool(true)).unwrap();
        assert_eq!(
            db.get_setting("some_flag").unwrap(),
            Some(JsonValue::Bool(true))
        );
    }

    #[test]
    fn memories_round_trip_sorted() {
        let db = temp_db("memories");
        db.set_memory("zeta", "last", "general").unwrap();
        db.set_memory("alpha", "first", "general").unwrap();

        let mems = db.get_all_memories().unwrap();
        assert_eq!(mems.len(), 2);
        assert_eq!(mems[0].key, "alpha");
        assert_eq!(mems[1].key, "zeta");

        // ON CONFLICT update path
        db.set_memory("alpha", "updated", "general").unwrap();
        let mems = db.get_all_memories().unwrap();
        assert_eq!(mems[0].value, "updated");
    }
}
