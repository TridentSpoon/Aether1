"""
Persistent Memory and Lore Store using SQLite.
Stores conversations, persistent knowledge, and user settings.
"""

import sqlite3
import os
import json
from datetime import datetime
from typing import List, Dict, Any, Optional

DB_PATH = os.path.join(os.path.dirname(__file__), "cortana_memory.db")

class MemoryDB:
    def __init__(self, db_path: str = DB_PATH):
        self.db_path = db_path
        self._init_db()

    def _get_connection(self):
        conn = sqlite3.connect(self.db_path)
        conn.row_factory = sqlite3.Row
        return conn

    def _init_db(self):
        with self._get_connection() as conn:
            cursor = conn.cursor()
            
            # Conversations table
            cursor.execute("""
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                sender TEXT NOT NULL,
                text TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                metadata TEXT
            )
            """)

            # Persistent key-value memory table (facts, lore, user preferences)
            cursor.execute("""
            CREATE TABLE IF NOT EXISTS long_term_memory (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                category TEXT DEFAULT 'general',
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            """)

            # Settings table
            cursor.execute("""
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )
            """)
            conn.commit()

    def add_message(self, session_id: str, sender: str, text: str, metadata: Optional[Dict[str, Any]] = None):
        with self._get_connection() as conn:
            cursor = conn.cursor()
            meta_json = json.dumps(metadata) if metadata else None
            cursor.execute(
                "INSERT INTO messages (session_id, sender, text, metadata) VALUES (?, ?, ?, ?)",
                (session_id, sender, text, meta_json)
            )
            conn.commit()

    def get_messages(self, session_id: str = "default", limit: int = 50) -> List[Dict[str, Any]]:
        with self._get_connection() as conn:
            cursor = conn.cursor()
            cursor.execute(
                "SELECT id, session_id, sender, text, timestamp, metadata FROM messages WHERE session_id = ? ORDER BY id DESC LIMIT ?",
                (session_id, limit)
            )
            rows = cursor.fetchall()
            messages = []
            for r in reversed(rows):
                messages.append({
                    "id": r["id"],
                    "session_id": r["session_id"],
                    "sender": r["sender"],
                    "text": r["text"],
                    "timestamp": r["timestamp"],
                    "metadata": json.loads(r["metadata"]) if r["metadata"] else {}
                })
            return messages

    def clear_history(self, session_id: str = "default"):
        with self._get_connection() as conn:
            cursor = conn.cursor()
            cursor.execute("DELETE FROM messages WHERE session_id = ?", (session_id,))
            conn.commit()

    def set_memory(self, key: str, value: str, category: str = "general"):
        with self._get_connection() as conn:
            cursor = conn.cursor()
            cursor.execute(
                "INSERT INTO long_term_memory (key, value, category, updated_at) VALUES (?, ?, ?, CURRENT_TIMESTAMP) "
                "ON CONFLICT(key) DO UPDATE SET value=excluded.value, category=excluded.category, updated_at=CURRENT_TIMESTAMP",
                (key, value, category)
            )
            conn.commit()

    def get_memory(self, key: str) -> Optional[str]:
        with self._get_connection() as conn:
            cursor = conn.cursor()
            cursor.execute("SELECT value FROM long_term_memory WHERE key = ?", (key,))
            row = cursor.fetchone()
            return row["value"] if row else None

    def get_all_memories(self) -> List[Dict[str, Any]]:
        with self._get_connection() as conn:
            cursor = conn.cursor()
            cursor.execute("SELECT key, value, category, updated_at FROM long_term_memory ORDER BY category, key")
            return [dict(r) for r in cursor.fetchall()]

    def delete_memory(self, key: str):
        with self._get_connection() as conn:
            cursor = conn.cursor()
            cursor.execute("DELETE FROM long_term_memory WHERE key = ?", (key,))
            conn.commit()

    def set_setting(self, key: str, value: Any):
        with self._get_connection() as conn:
            cursor = conn.cursor()
            val_str = json.dumps(value) if not isinstance(value, str) else value
            cursor.execute(
                "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                (key, val_str)
            )
            conn.commit()

    def get_setting(self, key: str, default: Any = None) -> Any:
        with self._get_connection() as conn:
            cursor = conn.cursor()
            cursor.execute("SELECT value FROM settings WHERE key = ?", (key,))
            row = cursor.fetchone()
            if not row:
                return default
            try:
                return json.loads(row["value"])
            except Exception:
                return row["value"]

    def get_all_settings(self) -> Dict[str, Any]:
        with self._get_connection() as conn:
            cursor = conn.cursor()
            cursor.execute("SELECT key, value FROM settings")
            res = {}
            for r in cursor.fetchall():
                try:
                    res[r["key"]] = json.loads(r["value"])
                except Exception:
                    res[r["key"]] = r["value"]
            return res

# Global instance
memory_db = MemoryDB()
