"""
Neural Voice Synthesis Engine using edge-tts.
Provides voice generation with hAlcy-style pitch and cadence.
"""

import os
import hashlib
from typing import Optional

CACHE_DIR = os.path.join(os.path.dirname(__file__), "audio_cache")
os.makedirs(CACHE_DIR, exist_ok=True)

# Recommended voices for hAlcy/Cyberpunk persona
VOICES = {
    "halcy_neural": "en-US-AriaNeural",          # Clear, intelligent, authoritative yet warm
    "halcy_alt": "en-US-JennyNeural",            # Expressive, crisp
    "cyber_tactical": "en-US-GuyNeural",         # Deep tactical HUD voice
    "cyber_uk": "en-GB-SoniaNeural",             # Sophisticated British AI
    "cyber_jp": "ja-JP-NanamiNeural"             # Cyberpunk Neo-Tokyo style
}

class TTSEngine:
    def __init__(self):
        self.default_voice = VOICES["halcy_neural"]
        self.default_rate = "+5%"
        self.default_pitch = "+2Hz"

    async def generate_speech(self, text: str, voice: Optional[str] = None, rate: Optional[str] = None, pitch: Optional[str] = None) -> Optional[str]:
        """
        Synthesize speech from text and return the local path to the mp3 file.
        Uses caching based on text + voice params hash.
        """
        # Clean text for speech (strip markdown characters)
        clean_text = self._sanitize_text(text)
        if not clean_text.strip():
            return None

        selected_voice = voice or self.default_voice
        selected_rate = rate or self.default_rate
        selected_pitch = pitch or self.default_pitch

        # Compute cache key
        hash_key = hashlib.md5(f"{clean_text}_{selected_voice}_{selected_rate}_{selected_pitch}".encode("utf-8")).hexdigest()
        output_file = os.path.join(CACHE_DIR, f"{hash_key}.mp3")

        if os.path.exists(output_file) and os.path.getsize(output_file) > 0:
            return output_file

        try:
            import edge_tts
            communicate = edge_tts.Communicate(
                text=clean_text,
                voice=selected_voice,
                rate=selected_rate,
                pitch=selected_pitch
            )
            await communicate.save(output_file)
            return output_file
        except Exception as e:
            print(f"[TTSEngine Error] Failed to generate speech: {e}")
            return None

    def _sanitize_text(self, text: str) -> str:
        """Strip markdown syntax and formatting characters to make speech natural."""
        import re
        # Remove code blocks
        text = re.sub(r'```[\s\S]*?```', 'code block omitted', text)
        # Remove inline code
        text = re.sub(r'`([^`]+)`', r'\1', text)
        # Remove markdown links [text](url) -> text
        text = re.sub(r'\[([^\]]+)\]\([^\)]+\)', r'\1', text)
        # Remove headers, bold, italics, bullets
        text = re.sub(r'[#*_~>━─═]', ' ', text)
        # Collapse multiple whitespaces
        text = re.sub(r'\s+', ' ', text).strip()
        return text

# Global instance
tts_engine = TTSEngine()
