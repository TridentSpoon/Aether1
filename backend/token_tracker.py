"""
Token Usage & Availability Telemetry Tracker for Project AETHER1.
Tracks prompt tokens, completion tokens, tokens-per-second, session totals,
and historical consumption for HUD visualization.
"""

import time
import math
from datetime import datetime
from typing import Dict, Any, List

class TokenTracker:
    def __init__(self):
        self.session_prompt_tokens = 0
        self.session_completion_tokens = 0
        self.total_requests = 0
        self.last_tps = 0.0
        self.history: List[Dict[str, Any]] = []
        self.max_history = 30
        self.daily_budget_tokens = 100000 # Configurable availability ceiling

    def estimate_tokens(self, text: str) -> int:
        """Heuristic token estimator (~4 chars per token for English/code)."""
        if not text:
            return 0
        return max(1, math.ceil(len(text) / 4))

    def record_usage(self, prompt: str, completion: str, duration_sec: float, model: str = "core"):
        prompt_tok = self.estimate_tokens(prompt)
        comp_tok = self.estimate_tokens(completion)
        total_tok = prompt_tok + comp_tok

        self.session_prompt_tokens += prompt_tok
        self.session_completion_tokens += comp_tok
        self.total_requests += 1

        tps = round(comp_tok / max(duration_sec, 0.05), 1)
        self.last_tps = tps

        entry = {
            "timestamp": datetime.now().strftime("%H:%M:%S"),
            "prompt_tokens": prompt_tok,
            "completion_tokens": comp_tok,
            "total_tokens": total_tok,
            "tps": tps,
            "model": model
        }

        self.history.append(entry)
        if len(self.history) > self.max_history:
            self.history.pop(0)

    def get_telemetry(self) -> Dict[str, Any]:
        total_session = self.session_prompt_tokens + self.session_completion_tokens
        used_pct = min(100.0, round((total_session / max(self.daily_budget_tokens, 1)) * 100, 1))
        available_tokens = max(0, self.daily_budget_tokens - total_session)

        # Recent sparkline data (array of token totals)
        sparkline = [h["total_tokens"] for h in self.history[-15:]]
        if not sparkline:
            sparkline = [0]

        return {
            "total_session_tokens": total_session,
            "session_prompt_tokens": self.session_prompt_tokens,
            "session_completion_tokens": self.session_completion_tokens,
            "total_requests": self.total_requests,
            "last_tps": self.last_tps,
            "daily_budget": self.daily_budget_tokens,
            "available_tokens": available_tokens,
            "used_percent": used_pct,
            "history": self.history[-12:],
            "sparkline": sparkline
        }

    def reset_session(self):
        self.session_prompt_tokens = 0
        self.session_completion_tokens = 0
        self.total_requests = 0
        self.history.clear()

token_tracker = TokenTracker()
