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

/// Where an action stands. Every tool call the companion makes gets a row in action_log,
/// so this is also the record of what it did on the operator's behalf -- the thing the
/// memory browser and the undo path in later steps both read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionStatus {
    /// A mutating call waiting for the operator to approve it.
    Proposed,
    /// Ran, and the result is recorded.
    Executed,
    /// Ran and failed; `result` carries the error.
    Failed,
    /// The operator declined it.
    Rejected,
    /// Executed and then reversed.
    Undone,
}

impl ActionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActionStatus::Proposed => "proposed",
            ActionStatus::Executed => "executed",
            ActionStatus::Failed => "failed",
            ActionStatus::Rejected => "rejected",
            ActionStatus::Undone => "undone",
        }
    }

    fn from_str(raw: &str) -> ActionStatus {
        match raw {
            "executed" => ActionStatus::Executed,
            "failed" => ActionStatus::Failed,
            "rejected" => ActionStatus::Rejected,
            "undone" => ActionStatus::Undone,
            _ => ActionStatus::Proposed,
        }
    }
}

/// One row of the action log.
#[derive(Debug, Clone, Serialize)]
pub struct ActionRecord {
    pub id: i64,
    pub timestamp: String,
    pub tool: String,
    /// The arguments the tool was called with, as JSON.
    pub args: JsonValue,
    pub mutating: bool,
    pub status: ActionStatus,
    /// Output on success, the error on failure, None while still proposed.
    pub result: Option<String>,
    /// Whatever the tool needs to reverse itself, as JSON -- the previous contents of a
    /// file it overwrote, the setting it replaced. None when the action can't be undone.
    pub undo: Option<JsonValue>,
    /// One line describing what this action does, in the tool's own words -- what an
    /// approval card shows. Filled in when the record is read back, since it comes from
    /// the tool rather than from the row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// Whether "stop asking about this tool" is on offer for this one. Filled in with the
    /// preview, from the tool rather than the row, so an approval card doesn't show a
    /// checkbox the consent layer would refuse. None when the tool is no longer known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_allowable: Option<bool>,
    /// Who let this run: "operator" for an explicit approval, "always-allow" for one the
    /// operator had pre-approved for this tool, "automatic" for a read-only call that
    /// needed no approval at all. None while a proposal is still waiting.
    pub approved_by: Option<String>,
    /// Why this needed asking about, when the answer is not simply "it changes something".
    /// An out-of-domain read carries the persona and the field it falls outside, so the
    /// approval card can say what is unusual about the request and the log can still answer
    /// "why did it read that?" a week later. None for a mutating proposal, whose reason is
    /// the tool itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
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
            );
            CREATE TABLE IF NOT EXISTS action_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts DATETIME DEFAULT CURRENT_TIMESTAMP,
                tool TEXT NOT NULL,
                args_json TEXT NOT NULL,
                mutating INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                result TEXT,
                undo_json TEXT
            );",
        )?;
        db.migrate()?;
        db.restrict_permissions();
        Ok(db)
    }

    /// Additive column migrations, run on every open. Guarded by a PRAGMA lookup rather
    /// than a version counter: the check is cheap, it's idempotent, and a database created
    /// by an older build opens without ceremony.
    fn migrate(&self) -> rusqlite::Result<()> {
        let conn = self.connect()?;
        let mut existing = conn.prepare("PRAGMA table_info(action_log)")?;
        let columns: Vec<String> = existing
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<_>>()?;

        if !columns.iter().any(|c| c == "approved_by") {
            conn.execute("ALTER TABLE action_log ADD COLUMN approved_by TEXT", [])?;
        }
        if !columns.iter().any(|c| c == "reason") {
            conn.execute("ALTER TABLE action_log ADD COLUMN reason TEXT", [])?;
        }
        Ok(())
    }

    /// Locks the database file down to owner-only read/write (0600). This file holds chat
    /// history, saved memories, and settings -- including the LLM provider API key -- so it
    /// should never be group/world-readable. No-op on Windows, which has no POSIX permission
    /// bits and relies on the user profile directory being private by default instead.
    /// Best-effort: a failure here (e.g. a filesystem that doesn't support Unix permissions)
    /// shouldn't stop the app from working, just leave it at the OS default.
    #[cfg(unix)]
    fn restrict_permissions(&self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.db_path, std::fs::Permissions::from_mode(0o600));
    }

    #[cfg(not(unix))]
    fn restrict_permissions(&self) {}

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

    pub fn clear_history(&self, session_id: &str) -> rusqlite::Result<()> {
        self.connect()?.execute(
            "DELETE FROM messages WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
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
        // Always store as JSON, including strings (JsonValue::String -> a properly quoted
        // JSON string) -- storing strings raw/unquoted made a string that happened to look
        // like JSON (a bare number, "true", "false", "null") indistinguishable on read from
        // that literal value, e.g. set_setting("agent_name", "9000") followed by
        // get_setting("agent_name") would come back as the *integer* 9000, not the string
        // "9000". get_setting's fallback-to-raw-string still covers old rows written by the
        // previous raw-string scheme, so this doesn't need a data migration.
        let val_str = value.to_string();
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
        // Falls back to treating the stored value as a plain string if it doesn't parse as
        // JSON -- covers rows written before this method always JSON-encoded (see
        // set_setting), so existing settings.db files don't need migrating.
        Ok(raw.map(|s| serde_json::from_str(&s).unwrap_or(JsonValue::String(s))))
    }

    pub fn get_setting_string(&self, key: &str, default: &str) -> String {
        match self.get_setting(key) {
            Ok(Some(JsonValue::String(s))) => s,
            Ok(Some(other)) => other.to_string(),
            _ => default.to_string(),
        }
    }

    /// Removes a setting, so it falls back to its default. Used by undo, where "there was
    /// no value before" has to be restorable as faithfully as an old value would be.
    pub fn delete_setting(&self, key: &str) -> rusqlite::Result<()> {
        self.connect()?
            .execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(())
    }

    /// A stored boolean, tolerating the string forms ("true"/"false") that a settings
    /// payload from the frontend can carry for a checkbox.
    pub fn get_setting_bool(&self, key: &str, default: bool) -> bool {
        match self.get_setting(key) {
            Ok(Some(JsonValue::Bool(b))) => b,
            Ok(Some(JsonValue::String(s))) => match s.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => true,
                "false" | "0" | "no" => false,
                _ => default,
            },
            _ => default,
        }
    }

    /// Every stored setting as a single object, matching memory_db.py's
    /// get_all_settings -- used for the Settings modal's bulk load/save.
    pub fn get_all_settings(&self) -> rusqlite::Result<JsonValue> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |row| {
            let key: String = row.get(0)?;
            let raw: String = row.get(1)?;
            Ok((key, raw))
        })?;

        let mut map = serde_json::Map::new();
        for row in rows {
            let (key, raw) = row?;
            let value = serde_json::from_str(&raw).unwrap_or(JsonValue::String(raw));
            map.insert(key, value);
        }
        Ok(JsonValue::Object(map))
    }

    /// Records a tool call and returns its id. Every call is logged, mutating or not, and
    /// logged *before* it runs -- an action that panics or never returns still leaves a
    /// trace of having been attempted, which is the whole point of having the log.
    pub fn log_action(
        &self,
        tool: &str,
        args: &JsonValue,
        mutating: bool,
        status: ActionStatus,
        reason: Option<&str>,
    ) -> rusqlite::Result<i64> {
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO action_log (tool, args_json, mutating, status, reason) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                tool,
                args.to_string(),
                mutating as i64,
                status.as_str(),
                reason
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Fills in how an action turned out. `undo` is stored only when the tool provides it.
    pub fn set_action_outcome(
        &self,
        id: i64,
        status: ActionStatus,
        result: Option<&str>,
        undo: Option<&JsonValue>,
        approved_by: Option<&str>,
    ) -> rusqlite::Result<()> {
        self.connect()?.execute(
            "UPDATE action_log SET status = ?2, result = ?3, undo_json = ?4, \
             approved_by = COALESCE(?5, approved_by) WHERE id = ?1",
            params![
                id,
                status.as_str(),
                result,
                undo.map(|u| u.to_string()),
                approved_by,
            ],
        )?;
        Ok(())
    }

    /// Actions waiting for the operator, oldest first -- the order they should be answered
    /// in. Read from the table rather than from memory, so a proposal outlives a restart
    /// and can't be silently lost when the app is closed with a card still on screen.
    pub fn pending_actions(&self) -> rusqlite::Result<Vec<ActionRecord>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT id, ts, tool, args_json, mutating, status, result, undo_json, approved_by, reason \
             FROM action_log WHERE status = 'proposed' ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], action_from_row)?;
        rows.collect()
    }

    /// Rejects proposals older than `minutes`, and returns how many. An approval is
    /// consent to do a thing *now*: a card answered an hour later is answering a question
    /// about a machine that has moved on.
    pub fn expire_stale_proposals(&self, minutes: u32) -> rusqlite::Result<usize> {
        let changed = self.connect()?.execute(
            "UPDATE action_log SET status = 'rejected', result = 'expired before approval' \
             WHERE status = 'proposed' AND ts < datetime('now', ?1)",
            params![format!("-{minutes} minutes")],
        )?;
        Ok(changed)
    }

    #[allow(dead_code)] // read by the undo path, once actions can be undone
    pub fn get_action(&self, id: i64) -> rusqlite::Result<Option<ActionRecord>> {
        self.connect()?
            .query_row(
                "SELECT id, ts, tool, args_json, mutating, status, result, undo_json, approved_by, reason \
                 FROM action_log WHERE id = ?1",
                params![id],
                action_from_row,
            )
            .optional()
    }

    /// Most recent first -- the order an operator reading back what happened wants.
    pub fn recent_actions(&self, limit: u32) -> rusqlite::Result<Vec<ActionRecord>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT id, ts, tool, args_json, mutating, status, result, undo_json, approved_by, reason \
             FROM action_log ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], action_from_row)?;
        rows.collect()
    }
}

/// Stored JSON that no longer parses (a hand-edited row, a schema change) becomes Null
/// rather than failing the whole read -- a corrupt row shouldn't hide the rest of the log.
fn action_from_row(row: &rusqlite::Row) -> rusqlite::Result<ActionRecord> {
    let args_raw: String = row.get(3)?;
    let undo_raw: Option<String> = row.get(7)?;
    let status_raw: String = row.get(5)?;
    Ok(ActionRecord {
        id: row.get(0)?,
        timestamp: row.get(1)?,
        tool: row.get(2)?,
        args: serde_json::from_str(&args_raw).unwrap_or(JsonValue::Null),
        mutating: row.get::<_, i64>(4)? != 0,
        status: ActionStatus::from_str(&status_raw),
        result: row.get(6)?,
        undo: undo_raw.and_then(|u| serde_json::from_str(&u).ok()),
        approved_by: row.get(8)?,
        reason: row.get(9)?,
        preview: None,
        always_allowable: None,
    })
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
    #[cfg(unix)]
    fn open_restricts_db_file_to_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let db = temp_db("permissions");
        let mode = std::fs::metadata(&db.db_path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "db file should be owner-read/write only, got {:o}",
            mode & 0o777
        );
    }

    #[test]
    fn actions_are_logged_before_they_run_and_updated_after() {
        let db = temp_db("actions");
        let args = serde_json::json!({"path": "/etc/hostname"});

        // Logged as proposed, with no outcome yet -- this is the state a mutating call
        // sits in while it waits for the operator.
        let id = db
            .log_action("write_file", &args, true, ActionStatus::Proposed, None)
            .unwrap();
        let logged = db.get_action(id).unwrap().expect("action should be stored");
        assert_eq!(logged.tool, "write_file");
        assert_eq!(logged.args, args);
        assert!(logged.mutating);
        assert_eq!(logged.status, ActionStatus::Proposed);
        assert!(logged.result.is_none());
        assert!(logged.undo.is_none());

        let undo = serde_json::json!({"restore": "old contents"});
        db.set_action_outcome(
            id,
            ActionStatus::Executed,
            Some("wrote 12 bytes"),
            Some(&undo),
            Some("operator"),
        )
        .unwrap();
        let done = db.get_action(id).unwrap().unwrap();
        assert_eq!(done.status, ActionStatus::Executed);
        assert_eq!(done.result.as_deref(), Some("wrote 12 bytes"));
        assert_eq!(done.undo, Some(undo));
        assert_eq!(done.approved_by.as_deref(), Some("operator"));
    }

    #[test]
    fn recent_actions_are_newest_first_and_bounded() {
        let db = temp_db("recent_actions");
        for i in 0..5 {
            db.log_action(
                "read_file",
                &serde_json::json!({"n": i}),
                false,
                ActionStatus::Executed,
                None,
            )
            .unwrap();
        }
        let recent = db.recent_actions(3).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].args["n"], serde_json::json!(4));
        assert_eq!(recent[2].args["n"], serde_json::json!(2));
    }

    #[test]
    fn an_unknown_action_id_is_none_not_an_error() {
        let db = temp_db("missing_action");
        assert!(db.get_action(4242).unwrap().is_none());
    }

    #[test]
    fn setting_bools_tolerate_the_string_forms_a_checkbox_can_send() {
        let db = temp_db("setting_bool");
        assert!(!db.get_setting_bool("tools_enabled", false));
        db.set_setting("tools_enabled", &serde_json::Value::Bool(true))
            .unwrap();
        assert!(db.get_setting_bool("tools_enabled", false));
        db.set_setting(
            "tools_enabled",
            &serde_json::Value::String("false".to_string()),
        )
        .unwrap();
        assert!(!db.get_setting_bool("tools_enabled", true));
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
    fn string_settings_that_look_like_json_round_trip_as_strings() {
        // Regression test: a string value that happens to be valid JSON syntax (a bare
        // number, "true"/"false"/"null", or another quoted string) must still come back as
        // a String, not get silently reinterpreted as that JSON type.
        let db = temp_db("json_lookalike_settings");

        db.set_setting("agent_name", &JsonValue::String("9000".to_string()))
            .unwrap();
        assert_eq!(
            db.get_setting("agent_name").unwrap(),
            Some(JsonValue::String("9000".to_string())),
            "a numeric-looking name must stay a string, not become the integer 9000"
        );
        assert_eq!(db.get_setting_string("agent_name", "HALCY"), "9000");

        for tricky in ["true", "false", "null", "\"already quoted\""] {
            db.set_setting("agent_name", &JsonValue::String(tricky.to_string()))
                .unwrap();
            assert_eq!(
                db.get_setting("agent_name").unwrap(),
                Some(JsonValue::String(tricky.to_string())),
                "JSON-syntax-like string {tricky:?} must round-trip as itself"
            );
        }
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
