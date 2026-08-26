"""
Linux Notification Bar & System Tray Companion for Project AETHER1.
Renders Robot Emoticon (🤖) in CachyOS (Arch) & Fedora top panels.
Provides status indicator, quick actions menu, and desktop notifications.
"""

import os
import sys
import time
import webbrowser
import threading
import subprocess
import requests
from PIL import Image
import pystray
from pystray import MenuItem as item

BACKEND_URL = "http://localhost:8378"
ICONS_DIR = os.path.join(os.path.dirname(__file__), "icons")

class Aether1TrayApp:
    def __init__(self):
        self.icon_cyan = Image.open(os.path.join(ICONS_DIR, "icon_cyan.png"))
        self.icon_gold = Image.open(os.path.join(ICONS_DIR, "icon_gold.png"))
        self.icon_green = Image.open(os.path.join(ICONS_DIR, "icon_green.png"))
        self.icon_purple = Image.open(os.path.join(ICONS_DIR, "icon_purple.png"))
        self.icon_amber = Image.open(os.path.join(ICONS_DIR, "icon_amber.png"))
        
        self.current_state = "offline"
        self.agent_name = "HALCY"
        self.active_theme = "halcy"
        self.tray = None
        self.running = True

    def send_notification(self, title: str, message: str, urgency: str = "normal"):
        icon_path = os.path.join(ICONS_DIR, "icon.png")
        try:
            subprocess.run(
                ["notify-send", "-a", self.agent_name, "-i", icon_path, "-u", urgency, title, message],
                check=False
            )
        except Exception:
            print(f"[{title}] {message}")

    def on_open_hud(self, icon=None, item=None):
        webbrowser.open(BACKEND_URL)

    def on_trigger_diagnostics(self, icon=None, item=None):
        try:
            resp = requests.get(f"{BACKEND_URL}/api/diagnostics", timeout=3.0)
            if resp.status_code == 200:
                report = resp.json().get("report", "")
                self.send_notification(f"{self.agent_name} Diagnostics", report)
            else:
                self.send_notification(f"{self.agent_name} Diagnostics", "Failed to retrieve telemetry.")
        except Exception as e:
            self.send_notification(f"{self.agent_name} Diagnostics", f"Backend connection error: {e}", urgency="critical")

    def on_open_settings(self, icon=None, item=None):
        webbrowser.open(f"{BACKEND_URL}#settings")

    def on_quit(self, icon=None, item=None):
        self.running = False
        self.send_notification(self.agent_name, "AI Companion system shutting down...")
        if self.tray:
            self.tray.stop()
        sys.exit(0)

    def poll_backend_status(self):
        while self.running:
            try:
                resp = requests.get(f"{BACKEND_URL}/api/health", timeout=2.0)
                if resp.status_code == 200:
                    data = resp.json()
                    self.agent_name = data.get("agent_name", "HALCY")
                    uptime = data.get("uptime", "")

                    if self.current_state != "online":
                        self.current_state = "online"
                        if self.tray:
                            # Choose icon based on persona
                            if "limes" in self.agent_name.lower():
                                self.tray.icon = self.icon_gold
                            else:
                                self.tray.icon = self.icon_cyan
                            self.tray.title = f"{self.agent_name} 🤖 ONLINE [{uptime}]"
                else:
                    self._set_offline()
            except Exception:
                self._set_offline()
            time.sleep(3.0)

    def _set_offline(self):
        if self.current_state != "offline":
            self.current_state = "offline"
            if self.tray:
                self.tray.icon = self.icon_amber
                self.tray.title = f"{self.agent_name} 🤖 OFFLINE"

    def run(self):
        self.send_notification(
            f"{self.agent_name} 🤖 Online",
            "Holographic companion active in notification bar.\nClick icon for HUD."
        )

        menu = pystray.Menu(
            item(f"🌐 Open {self.agent_name} HUD", self.on_open_hud, default=True),
            item("📊 Run System Diagnostics", self.on_trigger_diagnostics),
            item("⚙️ Settings & Themes", self.on_open_settings),
            pystray.Menu.SEPARATOR,
            item(f"🚪 Quit {self.agent_name}", self.on_quit)
        )

        self.tray = pystray.Icon(
            "AETHER1",
            self.icon_cyan,
            f"{self.agent_name} 🤖 Online",
            menu
        )

        monitor_thread = threading.Thread(target=self.poll_backend_status, daemon=True)
        monitor_thread.start()

        self.tray.run()

if __name__ == "__main__":
    app = Aether1TrayApp()
    app.run()
