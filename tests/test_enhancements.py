"""
Automated Test Suite for AETHER1 Enhancements:
- Agent Naming & Identity Genesis
- Model Scanner & Cloud API Key Auto-Discovery
- Real-Time Token Telemetry & Usage Tracker
- API Endpoints
"""

import os
import sys
import asyncio
from fastapi.testclient import TestClient

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))

from backend.token_tracker import token_tracker
from backend.model_scanner import model_scanner
from backend.llm_engine import llm_engine
from backend.main import app

client = TestClient(app)

def test_token_tracker():
    print("🧪 Testing Token Tracker & Availability Telemetry...")
    token_tracker.reset_session()
    
    prompt = "Explain quantum computing in detail"
    completion = "Quantum computing uses superposition and entanglement to execute parallel state computations."
    token_tracker.record_usage(prompt, completion, duration_sec=0.5, model="test-model")

    telem = token_tracker.get_telemetry()
    assert telem["session_prompt_tokens"] > 0
    assert telem["session_completion_tokens"] > 0
    assert telem["total_session_tokens"] == telem["session_prompt_tokens"] + telem["session_completion_tokens"]
    assert telem["total_requests"] == 1
    assert telem["last_tps"] > 0
    assert len(telem["sparkline"]) >= 1
    print(f"   ✔ Tokens: Prompt={telem['session_prompt_tokens']}, Comp={telem['session_completion_tokens']}, TPS={telem['last_tps']}")

def test_model_scanner():
    print("🧪 Testing Model & Cloud Key Scanner...")
    # Environment key detect
    cloud_keys_info = model_scanner.detect_cloud_api_keys()
    assert "detected_env_keys" in cloud_keys_info
    print(f"   ✔ Keys Found: {cloud_keys_info['detected_env_keys']}")

    # Scan All
    results = asyncio.run(model_scanner.scan_all())
    assert "cloud_keys" in results
    assert "ollama" in results
    assert "lmstudio" in results
    print(f"   ✔ Scanner complete. Ollama available: {results['ollama']['available']}, LM Studio: {results['lmstudio']['available']}")

def test_identity_genesis():
    print("🧪 Testing Agent Identity Genesis Engine...")
    # Test coding purpose
    res1 = asyncio.run(llm_engine.generate_identity_from_purpose("I want you to help me code fullstack apps in Python and React"))
    assert res1["name"] == "SYNAPSE"
    assert "callsign" in res1
    print(f"   ✔ Coding Purpose -> Name: {res1['name']} ({res1['callsign']})")

    # Test security / linux admin purpose
    res2 = asyncio.run(llm_engine.generate_identity_from_purpose("Monitor my CachyOS Linux kernel and network security"))
    assert res2["name"] == "NEXUS-09"
    print(f"   ✔ Linux/Security Purpose -> Name: {res2['name']}")

    # Test custom name set
    resp = asyncio.run(llm_engine.generate_response("set name VALKYRIE"))
    assert "VALKYRIE" in resp
    assert llm_engine.agent_name == "VALKYRIE"
    print(f"   ✔ Custom agent name set: {llm_engine.agent_name}")

def test_api_endpoints():
    print("🧪 Testing Enhanced API Endpoints...")
    # Token telemetry endpoint
    r = client.get("/api/telemetry/tokens")
    assert r.status_code == 200
    data = r.json()
    assert "total_session_tokens" in data
    print("   ✔ GET /api/telemetry/tokens -> 200 OK")

    # Scanner endpoint
    r = client.get("/api/scanner/status")
    assert r.status_code == 200
    print("   ✔ GET /api/scanner/status -> 200 OK")

    # Genesis endpoint
    r = client.post("/api/agent/genesis", json={"purpose": "Manage my daily schedule and tactical tasks"})
    assert r.status_code == 200
    data = r.json()
    assert data["name"] == "VALKYRIE"
    print("   ✔ POST /api/agent/genesis -> 200 OK")

if __name__ == "__main__":
    print("==================================================")
    print("🚀 RUNNING ENHANCEMENT TEST SUITE")
    print("==================================================")
    test_token_tracker()
    test_model_scanner()
    test_identity_genesis()
    test_api_endpoints()
    print("==================================================")
    print("✨ ALL ENHANCEMENT TESTS PASSED!")
    print("==================================================")
