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

/// One conversation, as the history list sees it. The list is derived from the messages
/// table rather than kept as its own bookkeeping, so a session that exists only because
/// something was said in it -- including the original hard-coded "default" -- shows up
/// without a migration dance, and a row can never go stale against its own transcript.
///
/// `title` is the operator's own name for the conversation when they set one, and the
/// opening line of it when they haven't. Nothing here is written by the model.
#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    /// True when `title` is the operator's, false when it's the fallback first line.
    pub named: bool,
    pub messages: u32,
    pub started: String,
    pub last: String,
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

/// What one model has been measured doing on this machine.
///
/// The scoreboard's row. Every number here is derived from replies the provider reported
/// counts and a generation time for; nothing in it is estimated, which is why a model the
/// operator has only talked to through a server that reports nothing never appears at all.
#[derive(Debug, Clone, Serialize)]
pub struct ModelBenchmark {
    pub provider: String,
    pub model: String,
    /// How many replies this average is built from. Shown, because three samples and three
    /// hundred are different levels of confidence in the same number.
    pub samples: u64,
    pub completion_tokens: u64,
    /// Total tokens divided by total generation time -- see `record_benchmark` for why it
    /// is weighted that way rather than being the mean of the per-reply rates.
    pub average_tps: f64,
    /// The best single reading, which is roughly what the model does once it is warm and
    /// the machine is otherwise idle.
    pub best_tps: f64,
    pub last_tps: f64,
    pub last_used: String,
}

/// The counted shape of everything ever said in this install, for the Profile pane.
///
/// Counted from the messages table each time it is asked for rather than kept as running
/// totals: there is no separate bookkeeping to drift, and deleting a conversation takes its
/// messages out of the numbers the same moment it takes them out of the history list.
#[derive(Debug, Clone, Serialize)]
pub struct UsageTotals {
    pub conversations: u32,
    pub messages: u32,
    /// Lines the operator typed (`sender` is "user"); the rest came back from the model.
    pub sent: u32,
    pub received: u32,
    /// The local date of the first message, or None on an install nothing has been said in.
    pub first_day: Option<String>,
    /// The longest single conversation, in messages.
    pub longest_chat: u32,
}

/// How many messages fell on one local day. Only days with something on them are returned.
#[derive(Debug, Clone, Serialize)]
pub struct DayCount {
    /// `YYYY-MM-DD`, in local time -- the same day boundary the operator lived through.
    pub day: String,
    pub messages: u32,
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

/// The name a conversation gets when the operator hasn't given it one: its opening line,
/// squeezed onto a single row. Truncation counts characters rather than bytes, because a
/// byte slice through a multi-byte character would panic, and the first thing anyone says
/// to their companion is exactly the sort of line that carries an emoji or an accent.
fn summarise(first_line: &str) -> String {
    const MAX_CHARS: usize = 48;
    let flat: String = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        return "New conversation".to_string();
    }
    if flat.chars().count() <= MAX_CHARS {
        return flat;
    }
    let mut out: String = flat.chars().take(MAX_CHARS).collect();
    out.push('\u{2026}');
    out
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
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT,
                created DATETIME DEFAULT CURRENT_TIMESTAMP
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
            );
            CREATE TABLE IF NOT EXISTS model_benchmarks (
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                samples INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                generation_seconds REAL NOT NULL DEFAULT 0,
                best_tps REAL NOT NULL DEFAULT 0,
                last_tps REAL NOT NULL DEFAULT 0,
                last_used DATETIME DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (provider, model)
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

    /// Every conversation that has anything in it, newest activity first.
    ///
    /// The join runs the other way round from what you might expect: messages is the
    /// source of truth and sessions only supplies a title, because a title is the one
    /// thing that can't be recovered from the transcript. A session row without messages
    /// is not a conversation yet and deliberately doesn't appear.
    pub fn list_sessions(&self, limit: u32) -> rusqlite::Result<Vec<SessionSummary>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT m.session_id, \
                    s.title, \
                    COUNT(m.id), \
                    MIN(m.timestamp), \
                    MAX(m.timestamp), \
                    ( SELECT text FROM messages f \
                      WHERE f.session_id = m.session_id AND f.sender = 'user' \
                      ORDER BY f.id LIMIT 1 ) \
             FROM messages m \
             LEFT JOIN sessions s ON s.id = m.session_id \
             GROUP BY m.session_id \
             ORDER BY MAX(m.id) DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            let id: String = row.get(0)?;
            let title: Option<String> = row.get(1)?;
            let first: Option<String> = row.get(5)?;
            let named = title.as_ref().is_some_and(|t| !t.trim().is_empty());
            let title = if named {
                title.unwrap_or_default().trim().to_string()
            } else {
                summarise(first.as_deref().unwrap_or(""))
            };
            Ok(SessionSummary {
                id,
                title,
                named,
                messages: row.get(2)?,
                started: row.get(3)?,
                last: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Names a conversation. An empty title removes the operator's name for it, which puts
    /// the list back on the fallback first line rather than leaving a blank row.
    pub fn set_session_title(&self, session_id: &str, title: &str) -> rusqlite::Result<()> {
        let conn = self.connect()?;
        let title = title.trim();
        if title.is_empty() {
            conn.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])?;
            return Ok(());
        }
        conn.execute(
            "INSERT INTO sessions (id, title) VALUES (?1, ?2) \
             ON CONFLICT(id) DO UPDATE SET title = excluded.title",
            params![session_id, title],
        )?;
        Ok(())
    }

    /// Deletes a conversation outright: the transcript and the title together. This is the
    /// operator's own delete, so it takes the messages with it -- a conversation you asked
    /// to be gone that leaves its words behind in the database is not gone.
    pub fn delete_session(&self, session_id: &str) -> rusqlite::Result<()> {
        let conn = self.connect()?;
        conn.execute(
            "DELETE FROM messages WHERE session_id = ?1",
            params![session_id],
        )?;
        conn.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])?;
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

    /// Today's date and the time right now, in the machine's own timezone, as
    /// `("2026-09-14", "14:32")`.
    ///
    /// SQLite rather than a date crate: the database is already open on every path that
    /// needs this, and the alternative is a dependency carried into the binary for two
    /// strings. `localtime` matters -- a journal note filed under yesterday's date because
    /// the machine is west of UTC is a note the operator cannot find by looking for the day
    /// it happened on.
    pub fn local_now(&self) -> (String, String) {
        self.connect()
            .and_then(|conn| {
                conn.query_row(
                    "SELECT date('now', 'localtime'), strftime('%H:%M', 'now', 'localtime')",
                    [],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
            })
            .unwrap_or_else(|_| ("unknown-date".to_string(), "??:??".to_string()))
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

    /// A stored whole number, tolerating the string form a number typed into a settings
    /// field arrives as. Anything that is not a non-negative whole number -- a word, a
    /// negative, a fraction -- falls back to the default rather than being rounded into
    /// something the operator did not ask for.
    pub fn get_setting_u64(&self, key: &str, default: u64) -> u64 {
        match self.get_setting(key) {
            Ok(Some(JsonValue::Number(n))) => n.as_u64().unwrap_or(default),
            Ok(Some(JsonValue::String(s))) => s.trim().parse().unwrap_or(default),
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

    /// The database file itself. `MemoryDb` is a path and opens a connection per call, so
    /// this is how another thread gets a handle of its own on the same database.
    pub fn path(&self) -> &Path {
        &self.db_path
    }

    /// The directory the database lives in, which is also where hand-editable companion
    /// files (the model price list) are looked for.
    pub fn dir(&self) -> Option<&Path> {
        self.db_path.parent()
    }

    /// Folds one reply into a model's running benchmark.
    ///
    /// Only replies whose token count and generation time both came from the provider
    /// should reach this -- see `LlmEngine::record_benchmark_sample`. A sample built on a
    /// character-count guess, or timed by a wall clock that includes loading the model off
    /// disk, would make the scoreboard slower and less true the more it was used.
    ///
    /// Totals rather than a running average, because the average has to be token-weighted
    /// to mean anything. Averaging the per-reply rates instead would let a four-token
    /// "Yes." -- whose rate is mostly measurement noise -- count for as much as a
    /// five-hundred-token essay.
    pub fn record_benchmark(
        &self,
        provider: &str,
        model: &str,
        completion_tokens: u64,
        generation_seconds: f64,
    ) -> rusqlite::Result<()> {
        // is_finite before the comparison, because a NaN duration compares false against
        // everything -- including `<= 0.0` -- and would otherwise sail through to divide
        // into a speed of NaN and sit at the top of a board ordered by speed.
        if completion_tokens == 0 || !generation_seconds.is_finite() || generation_seconds <= 0.0 {
            return Ok(());
        }
        let tps = completion_tokens as f64 / generation_seconds;
        self.connect()?.execute(
            "INSERT INTO model_benchmarks
                 (provider, model, samples, completion_tokens, generation_seconds, best_tps, last_tps, last_used)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?5, CURRENT_TIMESTAMP)
             ON CONFLICT(provider, model) DO UPDATE SET
                 samples = samples + 1,
                 completion_tokens = completion_tokens + ?3,
                 generation_seconds = generation_seconds + ?4,
                 best_tps = MAX(best_tps, ?5),
                 last_tps = ?5,
                 last_used = CURRENT_TIMESTAMP",
            params![provider, model, completion_tokens, generation_seconds, tps],
        )?;
        Ok(())
    }

    /// Everything the Profile pane counts, in one pass over the messages table.
    ///
    /// Timestamps are stored as UTC (`CURRENT_TIMESTAMP`), so every date here is converted
    /// with SQLite's `localtime` modifier: a conversation at eleven at night should count
    /// towards that evening, not the next morning in Greenwich.
    pub fn usage_totals(&self) -> rusqlite::Result<UsageTotals> {
        let conn = self.connect()?;
        let (conversations, messages, sent, first_day) = conn.query_row(
            "SELECT COUNT(DISTINCT session_id), COUNT(*), \
                    COALESCE(SUM(sender = 'user'), 0), \
                    MIN(DATE(timestamp, 'localtime')) \
             FROM messages",
            [],
            |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )?;
        let longest_chat = conn
            .query_row(
                "SELECT COUNT(*) AS n FROM messages GROUP BY session_id ORDER BY n DESC LIMIT 1",
                [],
                |row| row.get::<_, u32>(0),
            )
            .optional()?
            .unwrap_or(0);
        Ok(UsageTotals {
            conversations,
            messages,
            sent,
            received: messages.saturating_sub(sent),
            first_day,
            longest_chat,
        })
    }

    /// Messages per local day over the last `days` days, oldest first. Days with nothing on
    /// them are left out rather than returned as zeroes -- the caller draws the empty ones.
    pub fn daily_message_counts(&self, days: u32) -> rusqlite::Result<Vec<DayCount>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT DATE(timestamp, 'localtime') AS day, COUNT(*) \
             FROM messages \
             WHERE DATE(timestamp, 'localtime') >= DATE('now', 'localtime', ?1) \
             GROUP BY day ORDER BY day",
        )?;
        let offset = format!("-{days} days");
        let rows = stmt.query_map(params![offset], |row| {
            Ok(DayCount {
                day: row.get(0)?,
                messages: row.get(1)?,
            })
        })?;
        rows.collect()
    }

    /// Every model this machine has measured, fastest first.
    ///
    /// Ordered by the token-weighted average rather than by the best or most recent
    /// reading, because the question the scoreboard answers is "which model is fast on this
    /// machine", and a single lucky reply is not an answer to it.
    pub fn benchmarks(&self) -> rusqlite::Result<Vec<ModelBenchmark>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT provider, model, samples, completion_tokens, generation_seconds, best_tps, last_tps, last_used \
             FROM model_benchmarks WHERE generation_seconds > 0 \
             ORDER BY (completion_tokens / generation_seconds) DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let completion_tokens: u64 = row.get(3)?;
            let generation_seconds: f64 = row.get(4)?;
            let round = |v: f64| (v * 10.0).round() / 10.0;
            Ok(ModelBenchmark {
                provider: row.get(0)?,
                model: row.get(1)?,
                samples: row.get(2)?,
                completion_tokens,
                average_tps: round(completion_tokens as f64 / generation_seconds),
                best_tps: round(row.get(5)?),
                last_tps: round(row.get(6)?),
                last_used: row.get(7)?,
            })
        })?;
        rows.collect()
    }

    /// Throws the scoreboard away. The readings are about a machine, and a machine changes
    /// -- new graphics card, a different quantisation of the same model, Ollama updated --
    /// so there has to be a way to stop averaging the old one in with the new.
    pub fn clear_benchmarks(&self) -> rusqlite::Result<()> {
        self.connect()?
            .execute("DELETE FROM model_benchmarks", [])?;
        Ok(())
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
    fn the_totals_count_both_sides_of_every_conversation() {
        let db = temp_db("usage_totals");
        db.add_message("default", "user", "hello there").unwrap();
        db.add_message("default", "halcy", "hello yourself")
            .unwrap();
        db.add_message("default", "user", "still here").unwrap();
        db.add_message("c9", "user", "second room").unwrap();

        let totals = db.usage_totals().unwrap();
        assert_eq!(totals.conversations, 2);
        assert_eq!(totals.messages, 4);
        assert_eq!(totals.sent, 3);
        assert_eq!(totals.received, 1);
        assert_eq!(totals.longest_chat, 3);
        assert!(totals.first_day.is_some());
    }

    #[test]
    fn an_install_nothing_was_said_in_counts_zero_rather_than_failing() {
        let db = temp_db("usage_totals_empty");
        let totals = db.usage_totals().unwrap();
        assert_eq!((totals.conversations, totals.messages), (0, 0));
        assert_eq!(totals.longest_chat, 0);
        assert!(totals.first_day.is_none());
        assert!(db.daily_message_counts(308).unwrap().is_empty());
    }

    /// Today's messages land on today, and the count is per day rather than per message --
    /// the activity grid is drawn straight off these rows.
    #[test]
    fn the_daily_counts_put_todays_messages_on_todays_date() {
        let db = temp_db("daily_counts");
        db.add_message("default", "user", "one").unwrap();
        db.add_message("default", "halcy", "two").unwrap();

        let counts = db.daily_message_counts(308).unwrap();
        assert_eq!(counts.len(), 1);
        assert_eq!(counts[0].messages, 2);
        assert_eq!(counts[0].day, db.local_now().0);
    }

    #[test]
    fn a_conversation_appears_because_something_was_said_in_it() {
        let db = temp_db("sessions_derived");
        db.add_message("default", "user", "hello there").unwrap();
        db.add_message("default", "halcy", "hello yourself")
            .unwrap();
        db.add_message("c9", "user", "second room").unwrap();

        let rows = db.list_sessions(50).unwrap();
        assert_eq!(rows.len(), 2, "two sessions have messages in them");
        assert_eq!(rows[0].id, "c9", "newest activity first");
        assert_eq!(rows[0].title, "second room", "the opening line names it");
        assert!(!rows[0].named, "nobody named it, so this is the fallback");
        let older = &rows[1];
        assert_eq!(older.messages, 2, "both sides of the exchange are counted");
        assert_eq!(
            older.title, "hello there",
            "the title is the first thing the operator said, not the reply"
        );
    }

    #[test]
    fn a_name_survives_and_an_empty_name_gives_the_first_line_back() {
        let db = temp_db("sessions_title");
        db.add_message("c1", "user", "how do I mount a drive")
            .unwrap();

        db.set_session_title("c1", "  Disk notes  ").unwrap();
        let rows = db.list_sessions(50).unwrap();
        assert_eq!(rows[0].title, "Disk notes", "trimmed, and the operator's");
        assert!(rows[0].named);

        db.set_session_title("c1", "   ").unwrap();
        let rows = db.list_sessions(50).unwrap();
        assert_eq!(
            rows[0].title, "how do I mount a drive",
            "clearing the name falls back rather than leaving a blank row"
        );
        assert!(!rows[0].named);
    }

    #[test]
    fn deleting_a_conversation_takes_its_words_with_it() {
        let db = temp_db("sessions_delete");
        db.add_message("c1", "user", "keep me").unwrap();
        db.add_message("c2", "user", "delete me").unwrap();
        db.set_session_title("c2", "Doomed").unwrap();

        db.delete_session("c2").unwrap();

        let rows = db.list_sessions(50).unwrap();
        assert_eq!(rows.len(), 1, "only the survivor is listed");
        assert_eq!(rows[0].id, "c1");
        assert!(
            db.get_messages("c2", 50).unwrap().is_empty(),
            "a conversation asked to be gone leaves no transcript behind"
        );
    }

    #[test]
    fn one_conversation_cannot_see_another() {
        let db = temp_db("sessions_isolation");
        db.add_message("c1", "user", "my bank pin is 1234").unwrap();
        db.add_message("c2", "user", "what is the weather").unwrap();

        let seen = db.get_messages("c2", 50).unwrap();
        assert_eq!(seen.len(), 1, "only this conversation's own history");
        assert_eq!(seen[0].text, "what is the weather");
    }

    #[test]
    fn a_long_opening_line_is_cut_on_a_character_not_a_byte() {
        // Every one of these is multi-byte, so a byte-index truncation would panic.
        let long = "\u{e9}".repeat(200);
        let cut = summarise(&long);
        assert_eq!(
            cut.chars().count(),
            49,
            "48 characters plus the ellipsis that says there is more"
        );
        assert!(cut.ends_with('\u{2026}'));

        assert_eq!(
            summarise("  what   is\n  this  "),
            "what is this",
            "a title is one line, however the message was typed"
        );
        assert_eq!(summarise(""), "New conversation");
    }

    #[test]
    fn a_model_accumulates_across_replies() {
        let db = temp_db("bench_accumulate");
        // 100 tokens in 10s, then 300 in 10s: 400 tokens across 20 seconds.
        db.record_benchmark("Ollama", "mistral:latest", 100, 10.0)
            .unwrap();
        db.record_benchmark("Ollama", "mistral:latest", 300, 10.0)
            .unwrap();

        let board = db.benchmarks().unwrap();
        assert_eq!(board.len(), 1, "the same model is one row, not two");
        assert_eq!(board[0].samples, 2);
        assert_eq!(board[0].completion_tokens, 400);
        assert_eq!(board[0].average_tps, 20.0, "400 tokens over 20 seconds");
        assert_eq!(board[0].best_tps, 30.0, "the faster of the two readings");
        assert_eq!(board[0].last_tps, 30.0);
    }

    /// The average has to be token-weighted. A four-token "Yes." is mostly measurement
    /// noise, and averaging the per-reply *rates* would let it count for as much as the
    /// long reply that actually says what the model can do.
    #[test]
    fn the_average_is_weighted_by_tokens_not_by_reply() {
        let db = temp_db("bench_weighted");
        db.record_benchmark("Ollama", "m", 1000, 100.0).unwrap(); // 10 tok/s over a long reply
        db.record_benchmark("Ollama", "m", 4, 0.02).unwrap(); //  200 tok/s over four tokens

        let board = db.benchmarks().unwrap();
        // Weighted: 1004 tokens / 100.02s ~= 10.0. Unweighted it would be about 105.
        assert_eq!(board[0].average_tps, 10.0);
    }

    #[test]
    fn the_board_is_ordered_fastest_first() {
        let db = temp_db("bench_order");
        db.record_benchmark("Ollama", "slow", 100, 20.0).unwrap();
        db.record_benchmark("Ollama", "fast", 100, 2.0).unwrap();
        db.record_benchmark("Ollama", "middling", 100, 5.0).unwrap();

        let names: Vec<String> = db
            .benchmarks()
            .unwrap()
            .into_iter()
            .map(|b| b.model)
            .collect();
        assert_eq!(names, vec!["fast", "middling", "slow"]);
    }

    /// Two providers can serve a model of the same name (Ollama's llama3 and a hosted one),
    /// and they are not the same measurement.
    #[test]
    fn the_same_model_on_two_providers_is_two_rows() {
        let db = temp_db("bench_providers");
        db.record_benchmark("Ollama", "llama3", 100, 10.0).unwrap();
        db.record_benchmark("LM Studio", "llama3", 100, 5.0)
            .unwrap();
        assert_eq!(db.benchmarks().unwrap().len(), 2);
    }

    /// A reply with no tokens, or one that claims to have taken no time, would divide into
    /// zero or infinity and put either on the scoreboard as a speed.
    #[test]
    fn a_sample_with_nothing_in_it_is_ignored() {
        let db = temp_db("bench_empty");
        db.record_benchmark("Ollama", "m", 0, 10.0).unwrap();
        db.record_benchmark("Ollama", "m", 100, 0.0).unwrap();
        db.record_benchmark("Ollama", "m", 100, -1.0).unwrap();
        assert!(db.benchmarks().unwrap().is_empty());
    }

    #[test]
    fn resetting_empties_the_board() {
        let db = temp_db("bench_reset");
        db.record_benchmark("Ollama", "m", 100, 10.0).unwrap();
        assert_eq!(db.benchmarks().unwrap().len(), 1);
        db.clear_benchmarks().unwrap();
        assert!(db.benchmarks().unwrap().is_empty());
    }

    /// A database written by a build from before the scoreboard existed must open and gain
    /// the table, not fail on the first reading.
    #[test]
    fn an_older_database_gains_the_benchmark_table() {
        let path = std::env::temp_dir().join(format!(
            "aether1_test_bench_migrate_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        // Stand up a database with only the pre-scoreboard tables in it.
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();

        let db = MemoryDb::open(&path).expect("an older database should still open");
        db.record_benchmark("Ollama", "m", 100, 10.0).unwrap();
        assert_eq!(db.benchmarks().unwrap().len(), 1);
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
