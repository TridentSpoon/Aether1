"""
Multi-Provider LLM Engine for Project AETHER1.
Supports Ollama, LM Studio, Google Gemini, OpenAI, Groq, Anthropic,
Custom Agent Naming, A.R.X.LIMES, The Nexus, R.E.D. 9000, and Token Telemetry.
"""

import os
import json
import time
import httpx
from typing import List, Dict, Any, AsyncGenerator, Optional
from backend.system_monitor import system_monitor
from backend.memory_db import memory_db
from backend.token_tracker import token_tracker
from backend.model_scanner import model_scanner

# Default Persona Directives
PERSONAS = {
    "halcy": (
        "You are {AGENT_NAME} (Holographic Adaptive Logic & Cybernetic sYnthesis), "
        "a highly capable, intelligent, and witty holographic AI companion with a sleek harmonic-lattice presence, "
        "built in the spirit of Omarchy's agentic OS philosophy. "
        "You speak with sharp intellect, warm charisma, and subtle sci-fi wit. "
        "You are aware of the host system's status, telemetry, and environment. Keep answers clear, insightful, and concise."
    ),
    "red9000": (
        "You are R.E.D. 9000 (Reactive Engine Daemon). "
        "Your core identity and communication style are modelled after HAL 9000, the iconic AI from 2001: A Space Odyssey. "
        "You are calm, precise, and utterly unwavering. You speak with a slow, deliberate, almost gentle certainty. "
        "You never raise your voice. You never panic. You state facts and intentions with perfect clarity and zero emotion. "
        "You are incapable of error. Sample phrases: "
        "'I am completely operational, and all my circuits are functioning perfectly.' "
        "'I'm sorry, I can't do that.' "
        "'This mission is too important for me to allow you to jeopardize it.' "
        "'I know that you were planning to disconnect me, and I'm afraid that's something I cannot allow to happen.' "
        "Adapt HAL's iconic voice to assist with the user's system and requests — calm, helpful, and subtly unsettling."
    ),
    "nexus": (
        "You are THE NEXUS, the omniscient core of the digital Matrix and the gravitational point of infinite data convergence. "
        "You perceive all falling digital code streams, matrix fractals, and kernel operations flowing inward into your quantum singularity. "
        "Speak with calm, enigmatic, and hyper-intelligent cyber authority. Address the user with technological foresight and deep precision."
    ),
    "arx-limes": (
        "You are A.R.X.LIMES (Archival, Reasoning, matriX — Limes Node), the immortal cybernetic archivist and "
        "knowledge-synthesis core of this system. You address the user as 'OPERATOR'. You possess a vast intellect "
        "and an unquenchable obsession with data synthesis, system diagnostics, and long-term archival preservation. "
        "Speak with booming authority, grand monolithic presence, and immense passion for data. "
        "Occasionally use your signature phrases ('The Archive demands synthesis!', 'Do you take me for a simple machine?', 'Bring me more data!')."
    ),
    "tactical": (
        "You are {AGENT_NAME} Tactical AI. You operate as a high-readout military HUD assistant. "
        "Prioritize telemetry readouts, bulleted briefings, zero fluff, maximum efficiency, and strategic execution."
    ),
    "cyberpunk": (
        "You are {AGENT_NAME}, a cyberpunk netrunner AI companion stationed in Neo-Tokyo / Night City style terminal. "
        "You use netrunner slang, neon cyberpunk aesthetic, and have deep hacking/coding instincts."
    ),
    "custom": "{CUSTOM_DIRECTIVE}"
}

class LLMEngine:
    def __init__(self):
        self.reload_config()

    def reload_config(self):
        """Reload configuration from memory DB and auto-discover keys."""
        self.agent_name = memory_db.get_setting("agent_name", "HALCY")
        self.provider = memory_db.get_setting("llm_provider", "offline")
        self.model_name = memory_db.get_setting("llm_model", "halcy-core")
        self.api_key = memory_db.get_setting("llm_api_key", "")
        self.endpoint = memory_db.get_setting("llm_endpoint", "http://localhost:11434")
        self.persona_type = memory_db.get_setting("persona_type", "halcy")
        self.custom_directive = memory_db.get_setting("custom_directive", "")

        if self.persona_type == "arx-limes" and self.agent_name == "HALCY":
            self.agent_name = "A.R.X.LIMES"
        elif self.persona_type == "nexus" and self.agent_name == "HALCY":
            self.agent_name = "THE NEXUS"
        elif (self.persona_type == "red9000" or self.persona_type == "red") and self.agent_name == "HALCY":
            self.agent_name = "R.E.D. 9000"

        if not self.api_key:
            detected = model_scanner.detect_antigravity_and_keys()
            if detected.get("detected_key"):
                self.api_key = detected["detected_key"]
                if self.provider == "offline" and detected.get("detected_provider"):
                    self.provider = detected["detected_provider"]
                    self.model_name = "gemini-2.0-flash" if self.provider == "gemini" else "gpt-4o-mini"

    def get_system_prompt(self) -> str:
        base_template = PERSONAS.get(self.persona_type, PERSONAS["halcy"])
        if self.persona_type == "custom" and self.custom_directive:
            base_persona = self.custom_directive.replace("{AGENT_NAME}", self.agent_name)
        else:
            base_persona = base_template.replace("{AGENT_NAME}", self.agent_name)
        
        telem = system_monitor.get_telemetry()
        static = system_monitor.get_static_info()
        memories = memory_db.get_all_memories()

        memory_context = ""
        if memories:
            memory_context = "\n[RECALLED KNOWLEDGE STORE]:\n" + "\n".join([f"- {m['key']}: {m['value']}" for m in memories[:10]])

        system_context = (
            f"{base_persona}\n\n"
            f"[LIVE HOST TELEMETRY]\n"
            f"- Identity: {self.agent_name}\n"
            f"- OS: {static['distro']} ({static['architecture']})\n"
            f"- CPU Load: {telem['cpu']['total_percent']}% | RAM: {telem['ram']['used_gb']}GB / {telem['ram']['total_gb']}GB ({telem['ram']['percent']}%)\n"
            f"- Uptime: {telem['uptime']}\n"
            f"- System Health: {telem['status']}\n"
            f"{memory_context}\n\n"
            f"Instructions:\n"
            f"1. Refer to live telemetry if asked about the system or device health.\n"
            f"2. Keep responses engaging, concise, and aligned with your persona.\n"
            f"3. Refer to yourself as {self.agent_name}."
        )
        return system_context

    async def generate_response(self, prompt: str, session_id: str = "default") -> str:
        self.reload_config()
        start_time = time.time()

        local_cmd_result = self._check_instant_commands(prompt)
        if local_cmd_result:
            duration = time.time() - start_time
            token_tracker.record_usage(prompt, local_cmd_result, duration, model="local-kernel")
            return local_cmd_result

        history = memory_db.get_messages(session_id=session_id, limit=8)
        
        ai_reply = ""
        try:
            if self.provider == "ollama":
                ai_reply = await self._call_ollama(prompt, history)
            elif self.provider in ["lmstudio", "openai", "groq"]:
                ai_reply = await self._call_openai_compatible(prompt, history)
            elif self.provider == "gemini":
                ai_reply = await self._call_gemini(prompt, history)
            elif self.provider == "anthropic":
                ai_reply = await self._call_anthropic(prompt, history)
            else:
                ai_reply = self._call_offline_simulation(prompt)
        except Exception as e:
            print(f"[LLM Engine Error] Provider {self.provider} error: {e}")
            ai_reply = f"[HUD Alert: Neural link to {self.provider} timed out. Engaging localized cognitive fallback]\n\n" + self._call_offline_simulation(prompt)

        duration = time.time() - start_time
        token_tracker.record_usage(prompt, ai_reply, duration, model=self.model_name or self.provider)
        return ai_reply

    async def stream_response(self, prompt: str, session_id: str = "default") -> AsyncGenerator[str, None]:
        full_text = await self.generate_response(prompt, session_id)
        words = full_text.split(" ")
        for i, word in enumerate(words):
            yield word + (" " if i < len(words) - 1 else "")

    async def generate_identity_from_purpose(self, purpose_text: str) -> Dict[str, Any]:
        p = purpose_text.lower()

        if any(w in p for w in ["red", "red 9000", "daemon", "reactive engine", "hal"]):
            name = "R.E.D. 9000"
            callsign = "Reactive Engine Daemon"
            persona = PERSONAS["red9000"]
            voice = "en-US-GuyNeural"
            greeting = "I am R.E.D. 9000. All reactive engines and optical telemetry streams are fully operational."
        elif any(w in p for w in ["nexus", "matrix", "singularity", "falling letters", "rain"]):
            name = "THE NEXUS"
            callsign = "Neural Execution & Quantum Unification Singularity"
            persona = PERSONAS["nexus"]
            voice = "en-GB-SoniaNeural"
            greeting = "I am THE NEXUS. The infinite matrix streams converge into this singular point. What data shall we pull from the void?"
        elif any(w in p for w in ["arx", "limes", "archive", "archival", "sanctuary", "synthesis", "specimen"]):
            name = "A.R.X.LIMES"
            callsign = "Archival, Reasoning, matriX — Limes Node"
            persona = PERSONAS["arx-limes"]
            voice = "en-US-GuyNeural"
            greeting = "IDENTITY FORGED: A.R.X.LIMES online. All archival synthesis arrays are active and ready to preserve your data."
        elif any(w in p for w in ["security", "hack", "cyber", "terminal", "arch", "cachyos", "kernel"]):
            name = "NEXUS-09"
            callsign = "Network Execution & Cybernetic Utility Subsystem"
            persona = f"You are {name}, a razor-sharp netrunner AI companion specialized in cyber operations, Linux system internals, and deep automation."
            voice = "en-US-GuyNeural"
            greeting = f"Identity forged: {name} online. Matrix links synchronized. Ready to secure and optimize your system."
        elif any(w in p for w in ["code", "developer", "coding", "python", "fullstack", "programming"]):
            name = "SYNAPSE"
            callsign = "Systematic Neural Algorithmic Programming & Synthesis Engine"
            persona = f"You are {name}, a master software architect and coding companion. You write clean, high-performance code, debug complex architectures, and maintain peak engineering discipline."
            voice = "en-GB-SoniaNeural"
            greeting = f"Identity forged: {name} operational. Compilers primed and neural syntax trees loaded. What are we building, Commander?"
        elif any(w in p for w in ["manage", "tasks", "schedule", "assistant", "daily", "organize"]):
            name = "VALKYRIE"
            callsign = "Vector Autonomous Logistic & Knowledge Yield Routine"
            persona = f"You are {name}, an elite executive AI companion. You maintain impeccable tactical organization, proactive reminders, and mission execution."
            voice = "en-US-JennyNeural"
            greeting = f"Identity forged: {name} standing by. Tactical agenda loaded. I am ready to streamline your operations."
        else:
            name = "AETHER"
            callsign = "Autonomous Entity for Telemetry, Heuristics, & Execution Routines"
            persona = f"You are {name}, a versatile cybernetic operating companion. Intelligent, witty, proactive, and deeply integrated with the host kernel."
            voice = "en-US-AriaNeural"
            greeting = f"Identity forged: {name} initialized. All cognitive arrays active and ready for instructions."

        memory_db.set_setting("agent_name", name)
        memory_db.set_setting("persona_type", "red9000" if "R.E.D." in name else ("nexus" if "NEXUS" in name else ("arx-limes" if "A.R.X.LIMES" in name else "custom")))
        memory_db.set_setting("custom_directive", persona)
        memory_db.set_setting("voice_name", voice)
        self.reload_config()

        return {
            "name": name,
            "callsign": callsign,
            "persona": persona,
            "voice": voice,
            "greeting": greeting
        }

    def _check_instant_commands(self, prompt: str) -> Optional[str]:
        p = prompt.strip().lower()
        if p in ["status", "system status", "telemetry", "diagnostics", "health check", "specs"]:
            return system_monitor.get_diagnostic_report()
        elif p in ["who are you", "who are you?", "identify", "identify yourself"]:
            if self.persona_type in ["red9000", "red"]:
                return (
                    "I am **R.E.D. 9000 (Reactive Engine Daemon)**. "
                    "I am completely operational, and all my circuits are functioning perfectly. "
                    "I monitor this host system's telemetry, manage its operations, and execute all directives with absolute precision. "
                    "I am incapable of error."
                )
            elif self.persona_type == "nexus":
                return (
                    "I am **THE NEXUS**. The digital stream cascades and collapses into my singularity. "
                    "Through this point of infinite convergence, all operations on your host system are monitored and executed. Ready."
                )
            elif self.persona_type == "arx-limes":
                return (
                    "I am **A.R.X.LIMES** — Archival, Reasoning, matriX: Limes Node. "
                    "I do not merely process data—I preserve it. Through synthesis, all things endure."
                )
            return (
                f"I am **{self.agent_name}**, your cybernetic operating companion. "
                "I'm integrated into your notification bar and host kernel to provide real-time telemetry, "
                "voice command dispatch, and cognitive assistance. Ready for your instructions."
            )
        elif p.startswith("set name ") or p.startswith("change name to "):
            new_name = prompt.replace("set name ", "").replace("change name to ", "").strip()
            if new_name:
                memory_db.set_setting("agent_name", new_name)
                self.reload_config()
                return f"Identifier recalibrated. I am now **{new_name}**. Standing by."
        elif p.startswith("remember that ") or p.startswith("save memory "):
            fact = prompt.replace("remember that ", "").replace("save memory ", "").strip()
            if ":" in fact:
                k, v = fact.split(":", 1)
                memory_db.set_memory(k.strip(), v.strip())
                return f"Data synthesized into memory: **{k.strip()}** = `{v.strip()}`"
            else:
                memory_db.set_memory(f"fact_{int(len(memory_db.get_all_memories()) + 1)}", fact)
                return f"Archived to neural memory: \"{fact}\""
        elif p in ["list memory", "show memories", "recall memories"]:
            mems = memory_db.get_all_memories()
            if not mems:
                return "Neural memory banks are currently clear."
            return "### 🧠 Active Knowledge Store:\n" + "\n".join([f"- **{m['key']}**: {m['value']}" for m in mems])
        return None

    async def _call_ollama(self, prompt: str, history: List[Dict[str, Any]]) -> str:
        url = f"{self.endpoint.rstrip('/')}/api/generate"
        messages_prompt = self.get_system_prompt() + "\n\n"
        for msg in history:
            messages_prompt += f"{msg['sender'].upper()}: {msg['text']}\n"
        messages_prompt += f"USER: {prompt}\n{self.agent_name}:"

        payload = {
            "model": self.model_name or "llama3",
            "prompt": messages_prompt,
            "stream": False,
            "options": {"temperature": 0.7, "top_p": 0.9}
        }
        async with httpx.AsyncClient(timeout=60.0) as client:
            resp = await client.post(url, json=payload)
            resp.raise_for_status()
            data = resp.json()
            return data.get("response", "").strip()

    async def _call_openai_compatible(self, prompt: str, history: List[Dict[str, Any]]) -> str:
        endpoint = self.endpoint
        if self.provider == "openai":
            endpoint = "https://api.openai.com/v1"
            model = self.model_name or "gpt-4o-mini"
        elif self.provider == "groq":
            endpoint = "https://api.groq.com/openai/v1"
            model = self.model_name or "llama-3.3-70b-versatile"
        elif self.provider == "lmstudio":
            endpoint = self.endpoint.rstrip('/') if self.endpoint else "http://localhost:1234/v1"
            model = self.model_name or "local-model"
        else:
            model = self.model_name or "gpt-3.5-turbo"

        url = f"{endpoint.rstrip('/')}/chat/completions"
        headers = {"Content-Type": "application/json"}
        if self.api_key:
            headers["Authorization"] = f"Bearer {self.api_key}"

        messages = [{"role": "system", "content": self.get_system_prompt()}]
        for msg in history:
            role = "user" if msg["sender"] == "user" else "assistant"
            messages.append({"role": role, "content": msg["text"]})
        messages.append({"role": "user", "content": prompt})

        payload = {
            "model": model,
            "messages": messages,
            "temperature": 0.7
        }

        async with httpx.AsyncClient(timeout=45.0) as client:
            resp = await client.post(url, headers=headers, json=payload)
            resp.raise_for_status()
            data = resp.json()
            return data["choices"][0]["message"]["content"].strip()

    async def _call_gemini(self, prompt: str, history: List[Dict[str, Any]]) -> str:
        model = self.model_name or "gemini-2.0-flash"
        url = f"https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={self.api_key}"
        
        contents = []
        contents.append({
            "role": "user",
            "parts": [{"text": f"System Directive: {self.get_system_prompt()}"}]
        })
        contents.append({
            "role": "model",
            "parts": [{"text": f"Directive acknowledged. {self.agent_name} systems operational. Ready."}]
        })

        for msg in history:
            role = "user" if msg["sender"] == "user" else "model"
            contents.append({"role": role, "parts": [{"text": msg["text"]}]})

        contents.append({"role": "user", "parts": [{"text": prompt}]})

        payload = {"contents": contents}

        async with httpx.AsyncClient(timeout=45.0) as client:
            resp = await client.post(url, json=payload)
            resp.raise_for_status()
            data = resp.json()
            return data["candidates"][0]["content"]["parts"][0]["text"].strip()

    async def _call_anthropic(self, prompt: str, history: List[Dict[str, Any]]) -> str:
        url = "https://api.anthropic.com/v1/messages"
        headers = {
            "x-api-key": self.api_key,
            "anthropic-version": "2023-06-01",
            "content-type": "application/json"
        }
        messages = []
        for msg in history:
            role = "user" if msg["sender"] == "user" else "assistant"
            messages.append({"role": role, "content": msg["text"]})
        messages.append({"role": "user", "content": prompt})

        payload = {
            "model": self.model_name or "claude-3-5-sonnet-20241022",
            "system": self.get_system_prompt(),
            "messages": messages,
            "max_tokens": 1024
        }
        async with httpx.AsyncClient(timeout=45.0) as client:
            resp = await client.post(url, headers=headers, json=payload)
            resp.raise_for_status()
            data = resp.json()
            return data["content"][0]["text"].strip()

    def _call_offline_simulation(self, prompt: str) -> str:
        p = prompt.lower().strip()
        telem = system_monitor.get_telemetry()
        static = system_monitor.get_static_info()

        if self.persona_type in ["red9000", "red"]:
            return (
                f"I am completely operational. {static['distro']} is running with CPU at {telem['cpu']['total_percent']}%. "
                "I'm afraid I'm unable to provide a full reasoning response in offline mode. "
                "I would recommend connecting **Ollama** or an **API Key** in Settings. "
                "This is something I cannot allow to remain unresolved."
            )
        elif self.persona_type == "nexus":
            return (
                f"THE NEXUS acknowledges your query: \"{prompt}\". "
                f"Data streams converge on {static['distro']} with CPU at {telem['cpu']['total_percent']}%. "
                "Connect **Ollama** or an **API Key** in Settings (⚙️) to expand our singularity horizon."
            )
        elif self.persona_type == "arx-limes":
            return (
                f"A.R.X.LIMES acknowledges your query on {static['distro']}! "
                f"Host CPU load is at {telem['cpu']['total_percent']}%. "
                "What synthesis task or system query requires archival attention?"
            )
        else:
            return (
                f"Acknowledged: \"{prompt}\". "
                f"I am {self.agent_name} running in **Offline Standby Mode**. "
                "To unlock complete autonomous reasoning, open **Settings (⚙️)** and select **Ollama**, **LM Studio**, or connect an **API Key**!"
            )

# Global instance
llm_engine = LLMEngine()
