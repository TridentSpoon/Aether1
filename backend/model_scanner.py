"""
Auto-Discovery & Model Scanner Module for Project AETHER / CORTANA.
Discovers Antigravity/Gemini environment keys, probes local Ollama and LM Studio
services, and provides 1-click small model downloads.
"""

import os
import shutil
import asyncio
import httpx
import subprocess
from typing import Dict, Any, List

class ModelScanner:
    def __init__(self):
        self.ollama_url = "http://localhost:11434"
        self.lmstudio_url = "http://localhost:1234"

    async def scan_all(self) -> Dict[str, Any]:
        """Scan environment, Antigravity configs, Ollama, and LM Studio."""
        antigravity_info = self.detect_antigravity_and_keys()
        ollama_info = await self.scan_ollama()
        lmstudio_info = await self.scan_lmstudio()

        return {
            "antigravity": antigravity_info,
            "ollama": ollama_info,
            "lmstudio": lmstudio_info,
            "has_local_provider": ollama_info["available"] or lmstudio_info["available"],
            "has_cloud_key": bool(antigravity_info.get("detected_key"))
        }

    def detect_antigravity_and_keys(self) -> Dict[str, Any]:
        """Detect Antigravity environment, Gemini API keys, and OpenAI/Groq keys."""
        detected = {}
        
        # 1. Check direct environment variables
        env_keys = {
            "GEMINI_API_KEY": os.environ.get("GEMINI_API_KEY"),
            "ANTIGRAVITY_API_KEY": os.environ.get("ANTIGRAVITY_API_KEY"),
            "GOOGLE_API_KEY": os.environ.get("GOOGLE_API_KEY"),
            "OPENAI_API_KEY": os.environ.get("OPENAI_API_KEY"),
            "GROQ_API_KEY": os.environ.get("GROQ_API_KEY"),
            "ANTHROPIC_API_KEY": os.environ.get("ANTHROPIC_API_KEY")
        }

        found_env = {k: v[:6] + "..." + v[-4:] if v and len(v) > 10 else v for k, v in env_keys.items() if v}
        active_key = (
            env_keys.get("GEMINI_API_KEY") or 
            env_keys.get("ANTIGRAVITY_API_KEY") or 
            env_keys.get("GOOGLE_API_KEY") or
            env_keys.get("GROQ_API_KEY") or
            env_keys.get("OPENAI_API_KEY")
        )

        # 2. Check Antigravity presence
        is_antigravity = "ANTIGRAVITY_AGENT" in os.environ or os.path.exists(os.path.expanduser("~/.config/Antigravity"))
        
        return {
            "is_antigravity_host": is_antigravity,
            "detected_env_keys": list(found_env.keys()),
            "detected_key": active_key or "",
            "detected_provider": "gemini" if ("GEMINI_API_KEY" in found_env or "GOOGLE_API_KEY" in found_env) else ("groq" if "GROQ_API_KEY" in found_env else "openai" if "OPENAI_API_KEY" in found_env else "")
        }

    async def scan_ollama(self) -> Dict[str, Any]:
        """Probe Ollama server on localhost:11434."""
        is_cli_installed = bool(shutil.which("ollama"))
        models = []
        is_running = False

        try:
            async with httpx.AsyncClient(timeout=1.5) as client:
                resp = await client.get(f"{self.ollama_url}/api/tags")
                if resp.status_code == 200:
                    is_running = True
                    data = resp.json()
                    models = [m.get("name") for m in data.get("models", [])]
        except Exception:
            is_running = False

        return {
            "available": is_running,
            "cli_installed": is_cli_installed,
            "endpoint": self.ollama_url,
            "models": models,
            "recommended_model": models[0] if models else "llama3.2:1b"
        }

    async def scan_lmstudio(self) -> Dict[str, Any]:
        """Probe LM Studio server on localhost:1234."""
        models = []
        is_running = False

        try:
            async with httpx.AsyncClient(timeout=1.5) as client:
                resp = await client.get(f"{self.lmstudio_url}/v1/models")
                if resp.status_code == 200:
                    is_running = True
                    data = resp.json()
                    models = [m.get("id") for m in data.get("data", [])]
        except Exception:
            is_running = False

        return {
            "available": is_running,
            "endpoint": f"{self.lmstudio_url}/v1",
            "models": models,
            "recommended_model": models[0] if models else "local-model"
        }

    async def pull_model(self, model_name: str = "llama3.2:1b") -> Dict[str, Any]:
        """Request Ollama to pull a model or launch background pull."""
        # 1. Try Ollama HTTP API first
        try:
            async with httpx.AsyncClient(timeout=5.0) as client:
                resp = await client.post(
                    f"{self.ollama_url}/api/pull",
                    json={"name": model_name, "stream": False}
                )
                if resp.status_code == 200:
                    return {"status": "success", "message": f"Successfully pulled {model_name}"}
        except Exception:
            pass

        # 2. Try CLI fallback
        if shutil.which("ollama"):
            try:
                subprocess.Popen(["ollama", "pull", model_name])
                return {"status": "started", "message": f"Started pulling {model_name} in background via Ollama CLI."}
            except Exception as e:
                return {"status": "error", "message": f"Failed to start pull: {e}"}

        return {
            "status": "error",
            "message": "Ollama service is not running. Please start Ollama (`ollama serve`) first."
        }

model_scanner = ModelScanner()
