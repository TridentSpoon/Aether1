"""
Automated Test Suite for AETHER1 Core Services.
Tests:
1. System Telemetry & Static Info
2. SQLite Memory DB CRUD Operations
3. LLM Engine with Instant Commands & Persona Prompts
4. Neural TTS Generation Pipeline
5. FastAPI REST API endpoints
"""

import os
import sys
import asyncio
from fastapi.testclient import TestClient

# Ensure project root is in sys.path
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))

from backend.system_monitor import system_monitor
from backend.memory_db import memory_db
from backend.llm_engine import llm_engine
from backend.tts_engine import tts_engine
from backend.main import app

client = TestClient(app)

def test_telemetry():
    print("🧪 Testing System Telemetry...")
    static_info = system_monitor.get_static_info()
    assert "distro" in static_info
    assert static_info["cpu_cores_logical"] >= 1
    print(f"   ✔ OS Distro: {static_info['distro']}, Cores: {static_info['cpu_cores_logical']}")

    telem = system_monitor.get_telemetry()
    assert "cpu" in telem
    assert "ram" in telem
    assert "status" in telem
    print(f"   ✔ CPU: {telem['cpu']['total_percent']}%, RAM: {telem['ram']['percent']}%")

    diag = system_monitor.get_diagnostic_report()
    assert "SYSTEM DIAGNOSTIC REPORT" in diag
    print("   ✔ Diagnostic report generated successfully.")

def test_memory_db():
    print("🧪 Testing SQLite Memory Store...")
    session_id = "test_session"
    memory_db.clear_history(session_id)
    memory_db.add_message(session_id, "user", "Test command")
    memory_db.add_message(session_id, "agent", "Test response")

    msgs = memory_db.get_messages(session_id)
    assert len(msgs) == 2
    assert msgs[0]["text"] == "Test command"
    assert msgs[1]["text"] == "Test response"
    print("   ✔ Conversation history CRUD verified.")

    # Long-term memory
    memory_db.set_memory("fedora_system", "ThinkPad X1 Carbon with Fedora 41")
    val = memory_db.get_memory("fedora_system")
    assert val == "ThinkPad X1 Carbon with Fedora 41"
    print("   ✔ Long-term fact recall verified.")

def test_memory_db_connection_closes():
    print("🧪 Testing SQLite connections are actually closed (not leaked)...")
    import sqlite3
    with memory_db._get_connection() as conn:
        pass
    try:
        conn.execute("SELECT 1")
        assert False, "connection should have been closed by _get_connection's context manager"
    except sqlite3.ProgrammingError:
        pass  # expected: "Cannot operate on a closed database."
    print("   ✔ Connection is closed after the `with` block exits.")

def test_settings_round_trip_json_lookalike_strings():
    print("🧪 Testing settings round-trip for JSON-syntax-like strings...")
    # Regression test: a string value that happens to be valid JSON syntax (a bare
    # number, "true"/"false"/"null") must come back as that same string, not get
    # silently reinterpreted as the JSON type it looks like.
    memory_db.set_setting("test_numeric_looking_setting", "9000")
    val = memory_db.get_setting("test_numeric_looking_setting")
    assert val == "9000" and isinstance(val, str), f"expected the string '9000', got {val!r} ({type(val)})"
    print("   ✔ A numeric-looking string setting round-trips as a string.")

def test_llm_engine():
    print("🧪 Testing LLM Engine Commands...")
    
    # Test instant command
    resp = asyncio.run(llm_engine.generate_response("status"))
    assert "SYSTEM DIAGNOSTIC REPORT" in resp
    print("   ✔ Instant status command intercepted.")

    # Test offline reply
    resp2 = asyncio.run(llm_engine.generate_response("hello"))
    assert "Offline Standby Mode" in resp2 or "Greetings" in resp2
    print(f"   ✔ Contextual response generated: {resp2[:60]}...")

def test_llm_engine_set_name_is_case_insensitive():
    print("🧪 Testing \"set name\" command is case-insensitive...")
    # Regression test: the startswith() gate lowercases the prompt, but the old
    # extraction (prompt.replace("set name ", "")) was case-sensitive against the
    # original-case prompt, so "Set Name TestBot" left the leftover command text stuck
    # in the new name instead of extracting just "TestBot".
    resp = llm_engine._check_instant_commands("Set Name TestBot")
    assert resp is not None and "TestBot" in resp, f"expected TestBot in reply, got: {resp!r}"
    assert "Set Name" not in resp, f"leftover command text leaked into the name: {resp!r}"
    assert llm_engine.agent_name == "TestBot"
    print("   ✔ Mixed-case \"Set Name\" extracts the name correctly.")

def test_llm_engine_remember_fact_keys_dont_collide():
    print("🧪 Testing auto-generated memory keys don't collide after a deletion...")
    # Regression test: a key derived from the live memory count (fact_{count+1})
    # collided with an existing key once anything had ever been deleted, silently
    # overwriting it (set_memory is an upsert).
    before_keys = {m["key"] for m in memory_db.get_all_memories()}
    llm_engine._check_instant_commands("remember that this is the first fact")
    llm_engine._check_instant_commands("remember that this is a second, different fact")
    after_keys = {m["key"] for m in memory_db.get_all_memories()}
    new_keys = after_keys - before_keys
    assert len(new_keys) == 2, f"expected 2 distinct new keys, got {new_keys!r}"
    print(f"   ✔ Two colon-less facts got distinct keys: {new_keys!r}")

def test_tts_engine():
    print("🧪 Testing Edge-TTS Neural Speech Synthesis...")
    audio_path = asyncio.run(tts_engine.generate_speech("AETHER system online. All subsystems nominal."))
    if audio_path:
        assert os.path.exists(audio_path)
        assert os.path.getsize(audio_path) > 0
        print(f"   ✔ Neural audio file generated ({os.path.getsize(audio_path)} bytes): {audio_path}")
    else:
        print("   ⚠ Edge-TTS generation skipped (network dependent).")

def test_api_routes():
    print("🧪 Testing FastAPI Endpoints...")
    # Health
    r = client.get("/api/health")
    assert r.status_code == 200
    assert r.json()["status"] == "ONLINE"
    print("   ✔ GET /api/health returned 200 OK")

    # Static Info
    r = client.get("/api/static-info")
    assert r.status_code == 200
    print("   ✔ GET /api/static-info returned 200 OK")

    # Chat
    r = client.post("/api/chat", json={"message": "diagnostics", "session_id": "test_api"})
    assert r.status_code == 200
    assert "SYSTEM DIAGNOSTIC REPORT" in r.json()["reply"]
    print("   ✔ POST /api/chat returned 200 OK")

    # Settings
    r = client.get("/api/settings")
    assert r.status_code == 200
    print("   ✔ GET /api/settings returned 200 OK")

if __name__ == "__main__":
    print("==================================================")
    print("🚀 RUNNING AUTOMATED TEST SUITE")
    print("==================================================")
    test_telemetry()
    test_memory_db()
    test_memory_db_connection_closes()
    test_settings_round_trip_json_lookalike_strings()
    test_llm_engine()
    test_llm_engine_set_name_is_case_insensitive()
    test_llm_engine_remember_fact_keys_dont_collide()
    test_tts_engine()
    test_api_routes()
    print("==================================================")
    print("✨ ALL TESTS PASSED SUCCESSFULLY!")
    print("==================================================")
