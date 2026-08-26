"""
Auto-Discovery & Model Scanner Module for Project AETHER1.
Discovers cloud API keys in the environment, probes local Ollama and LM Studio
services, and provides 1-click small model downloads.
"""

import os
import shutil
import httpx
import subprocess
from typing import Dict, Any

class ModelScanner:
    def __init__(self):
        self.ollama_url = "http://localhost:11434"
        self.lmstudio_url = "http://localhost:1234"

    async def scan_all(self) -> Dict[str, Any]:
        """Scan environment for cloud API keys, and probe Ollama and LM Studio."""
        cloud_keys_info = self.detect_cloud_api_keys()
        ollama_info = await self.scan_ollama()
        lmstudio_info = await self.scan_lmstudio()

        return {
            "cloud_keys": cloud_keys_info,
            "ollama": ollama_info,
            "lmstudio": lmstudio_info,
            "has_local_provider": ollama_info["available"] or lmstudio_info["available"],
            "has_cloud_key": bool(cloud_keys_info.get("detected_key"))
        }

    def detect_cloud_api_keys(self) -> Dict[str, Any]:
        """Detect Gemini, OpenAI, Groq, and Anthropic API keys in the environment."""
        # Check direct environment variables
        env_keys = {
            "GEMINI_API_KEY": os.environ.get("GEMINI_API_KEY"),
            "GOOGLE_API_KEY": os.environ.get("GOOGLE_API_KEY"),
            "OPENAI_API_KEY": os.environ.get("OPENAI_API_KEY"),
            "GROQ_API_KEY": os.environ.get("GROQ_API_KEY"),
            "ANTHROPIC_API_KEY": os.environ.get("ANTHROPIC_API_KEY")
        }

        detected_env_keys = [k for k, v in env_keys.items() if v]
        active_key = (
            env_keys.get("GEMINI_API_KEY") or
            env_keys.get("GOOGLE_API_KEY") or
            env_keys.get("GROQ_API_KEY") or
            env_keys.get("OPENAI_API_KEY")
        )

        return {
            "detected_env_keys": detected_env_keys,
            "detected_key": active_key or "",
            "detected_provider": "gemini" if ("GEMINI_API_KEY" in detected_env_keys or "GOOGLE_API_KEY" in detected_env_keys) else ("groq" if "GROQ_API_KEY" in detected_env_keys else "openai" if "OPENAI_API_KEY" in detected_env_keys else "")
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
