"""
Main FastAPI Server for Project AETHER1.
Hosts REST APIs, WebSockets for live telemetry and chat streaming,
Agent Identity Genesis, Local Model Auto-Scanner, and Token Telemetry.
"""

import os
import asyncio
import json
from typing import Optional, Dict, Any
from fastapi import FastAPI, WebSocket, WebSocketDisconnect, HTTPException, Query
from fastapi.responses import FileResponse, JSONResponse
from fastapi.staticfiles import StaticFiles
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel

from backend.system_monitor import system_monitor
from backend.memory_db import memory_db
from backend.tts_engine import tts_engine, CACHE_DIR, VOICES
from backend.llm_engine import llm_engine, PERSONAS
from backend.token_tracker import token_tracker
from backend.model_scanner import model_scanner

app = FastAPI(
    title="AETHER1 AI Core",
    description="Cybernetic Operational Reconnaissance & Telemetry Autonomous Network Assistant",
    version="2.1.0"
)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

FRONTEND_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "frontend"))

# Pydantic Schemas
class ChatRequest(BaseModel):
    message: str
    session_id: Optional[str] = "default"
    generate_voice: Optional[bool] = False
    voice_name: Optional[str] = None

class MemoryRequest(BaseModel):
    key: str
    value: str
    category: Optional[str] = "general"

class SettingsRequest(BaseModel):
    settings: Dict[str, Any]

class GenesisRequest(BaseModel):
    purpose: str

# API Endpoints
@app.get("/api/health")
async def health_check():
    telem = system_monitor.get_telemetry()
    static = system_monitor.get_static_info()
    return {
        "status": "ONLINE",
        "agent_name": llm_engine.agent_name,
        "system": "AETHER1_CORE_V2",
        "distro": static["distro"],
        "telemetry_status": telem["status"],
        "uptime": telem["uptime"]
    }

@app.get("/api/telemetry")
async def get_telemetry():
    return system_monitor.get_telemetry()

@app.get("/api/telemetry/tokens")
async def get_token_telemetry():
    return token_tracker.get_telemetry()

@app.get("/api/static-info")
async def get_static_info():
    return system_monitor.get_static_info()

@app.get("/api/diagnostics")
async def get_diagnostics():
    report = system_monitor.get_diagnostic_report()
    return {"report": report}

@app.post("/api/chat")
async def chat_endpoint(req: ChatRequest):
    if not req.message.strip():
        raise HTTPException(status_code=400, detail="Empty message")

    session_id = req.session_id or "default"
    memory_db.add_message(session_id, "user", req.message)

    ai_reply = await llm_engine.generate_response(req.message, session_id=session_id)
    memory_db.add_message(session_id, llm_engine.agent_name.lower(), ai_reply)

    audio_url = None
    if req.generate_voice:
        audio_path = await tts_engine.generate_speech(ai_reply, voice=req.voice_name)
        if audio_path:
            filename = os.path.basename(audio_path)
            audio_url = f"/api/audio/{filename}"

    return {
        "reply": ai_reply,
        "audio_url": audio_url,
        "session_id": session_id,
        "agent_name": llm_engine.agent_name
    }

# Agent Identity Genesis
@app.post("/api/agent/genesis")
async def agent_genesis(req: GenesisRequest):
    if not req.purpose.strip():
        raise HTTPException(status_code=400, detail="Please provide a purpose description")
    
    result = await llm_engine.generate_identity_from_purpose(req.purpose)
    
    # Generate speech for greeting
    audio_path = await tts_engine.generate_speech(result["greeting"], voice=result["voice"])
    audio_url = f"/api/audio/{os.path.basename(audio_path)}" if audio_path else None
    
    # Store greeting in chat
    memory_db.add_message("default", result["name"].lower(), result["greeting"])

    return {
        **result,
        "audio_url": audio_url
    }

# Auto-Discovery & Model Scanner Endpoints
@app.get("/api/scanner/status")
async def get_scanner_status():
    return await model_scanner.scan_all()

@app.post("/api/scanner/pull-model")
async def pull_model(model_name: str = Query("llama3.2:1b")):
    return await model_scanner.pull_model(model_name)

@app.post("/api/tts")
async def generate_tts(text: str = Query(...), voice: Optional[str] = None):
    audio_path = await tts_engine.generate_speech(text, voice=voice)
    if not audio_path or not os.path.exists(audio_path):
        raise HTTPException(status_code=500, detail="Failed to synthesize speech")
    filename = os.path.basename(audio_path)
    return {"audio_url": f"/api/audio/{filename}"}

@app.get("/api/audio/{filename}")
async def stream_audio(filename: str):
    safe_filename = os.path.basename(filename)
    file_path = os.path.join(CACHE_DIR, safe_filename)
    if not os.path.exists(file_path):
        raise HTTPException(status_code=404, detail="Audio file not found")
    return FileResponse(file_path, media_type="audio/mpeg")

@app.get("/api/messages")
async def get_messages(session_id: str = "default", limit: int = 50):
    return memory_db.get_messages(session_id, limit)

@app.delete("/api/messages")
async def clear_messages(session_id: str = "default"):
    memory_db.clear_history(session_id)
    return {"status": "cleared", "session_id": session_id}

@app.get("/api/memories")
async def get_memories():
    return memory_db.get_all_memories()

@app.post("/api/memories")
async def add_memory(req: MemoryRequest):
    memory_db.set_memory(req.key, req.value, req.category)
    return {"status": "saved", "key": req.key}

@app.delete("/api/memories/{key}")
async def delete_memory(key: str):
    memory_db.delete_memory(key)
    return {"status": "deleted", "key": key}

@app.get("/api/settings")
async def get_settings():
    current_settings = memory_db.get_all_settings()
    defaults = {
        "agent_name": "HALCY",
        "llm_provider": "offline",
        "llm_model": "halcy-core",
        "llm_endpoint": "http://localhost:11434",
        "llm_api_key": "",
        "persona_type": "halcy",
        "custom_directive": "",
        "voice_name": "en-US-AriaNeural",
        "enable_sfx": True,
        "auto_speak": True
    }
    for k, v in defaults.items():
        if k not in current_settings:
            current_settings[k] = v
    return {
        "settings": current_settings,
        "available_personas": list(PERSONAS.keys()),
        "available_voices": VOICES
    }

@app.post("/api/settings")
async def save_settings(req: SettingsRequest):
    for k, v in req.settings.items():
        memory_db.set_setting(k, v)
    llm_engine.reload_config()
    return {"status": "saved", "settings": memory_db.get_all_settings()}

# WebSocket Telemetry push (Hardware + Tokens)
@app.websocket("/ws/telemetry")
async def websocket_telemetry(websocket: WebSocket):
    await websocket.accept()
    try:
        while True:
            telem = system_monitor.get_telemetry()
            tokens = token_tracker.get_telemetry()
            payload = {
                "telemetry": telem,
                "tokens": tokens,
                "agent_name": llm_engine.agent_name
            }
            await websocket.send_json(payload)
            await asyncio.sleep(1.0)
    except WebSocketDisconnect:
        pass
    except Exception as e:
        print(f"[WebSocket Telemetry Error] {e}")

# Serve Static Frontend Files
if os.path.exists(FRONTEND_DIR):
    app.mount("/static", StaticFiles(directory=FRONTEND_DIR), name="static")

@app.get("/")
async def root():
    index_file = os.path.join(FRONTEND_DIR, "index.html")
    if os.path.exists(index_file):
        return FileResponse(index_file)
    return {"message": "AETHER1 Core Running. Frontend directory not initialized."}

if __name__ == "__main__":
    import uvicorn
    uvicorn.run("backend.main:app", host="0.0.0.0", port=8378, reload=True)
