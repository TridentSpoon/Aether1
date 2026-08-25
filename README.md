# 🌌 AETHER1 AI Companion

An advanced, self-hosted, cybernetic AI companion and HUD inspired by the UNSC AI **Cortana** (Halo & original OS concept) and the agentic OS philosophy of **Omarchy**.

Designed natively for **CachyOS (Arch)** and **Fedora (GNOME / KDE / Wayland / X11)**, featuring real-time system telemetry, an interactive 3D holographic particle avatar, neural speech synthesis, voice recognition, persistent memory, and a system tray companion in your notification bar.

---

## ✨ Features

- **3D Holographic Particle Avatar**: Three.js WebGL avatar featuring 2,800+ harmonic particle nodes, 3 orbital energy rings, and a pulsing neural core that deforms dynamically to speech and audio frequencies.
- **Cyberpunk HUD & Live Telemetry**: Real-time monitoring of CPU usage (per-core load), RAM allocation, storage, network upload/download throughput, and battery status.
- **System Notification Bar Integration**:
  - Displays a dynamic glowing Cortana status icon in your top panel / system tray (Cyan = Active, Green = Listening, Purple = Processing, Amber = Alert/Offline).
  - Quick action menu to open the HUD, trigger instant system diagnostics, or adjust settings.
  - Native desktop notifications (`libnotify` / `notify-send`) for health updates and briefings.
- **Multi-Provider AI Engine**:
  - **Local Offline AI**: Connect to **Ollama** (`http://localhost:11434`) or **LM Studio** (`http://localhost:1234/v1`) with zero cloud dependencies.
  - **Cloud AI APIs**: Supports **Google Gemini**, **OpenAI (ChatGPT)**, **Groq** (ultra-fast voice chat), and **Anthropic Claude**.
  - **Offline Standby Mode**: Intelligent built-in contextual fallback for instant diagnostics and commands even without an LLM connected.
- **Neural Voice Synthesis & Speech Recognition**:
  - Neural text-to-speech using `edge-tts` (featuring Cortana's signature clear tone, customizable pitch, and rates).
  - Speech-to-Text via Web Speech API with auto-send voice chat.
  - Built-in Web Audio API sci-fi synthesizer for UI sound effects.
- **Persistent Long-Term Memory**:
  - SQLite persistent database storing chat sessions, user preferences, and memories across system reboots.
  - Type `remember that [fact]` or `save memory [key]: [value]` to store data permanently.
- **1-Click Packaging & Portability**:
  - Auto-installer for **CachyOS/Arch** (`pacman`) and **Fedora** (`dnf`).
  - Distribution script (`package_dist.sh`) to package the entire system into a single `.tar.gz` bundle to transfer to your second system.

---

## 🚀 Quick Start (New Machine)

This repo is **private**, so cloning it needs your GitHub account's SSH key set
up on the new machine first (one-time, per machine):

### 1. Add an SSH key to GitHub (skip if this machine already has one)
```bash
ssh-keygen -t ed25519 -f ~/.ssh/id_ed25519 -N "" -C "$(hostname)"
cat ~/.ssh/id_ed25519.pub
```
Copy the printed key into **[github.com/settings/ssh/new](https://github.com/settings/ssh/new)**.

### 2. Clone, set up, and launch — one line
```bash
git clone git@github.com:TridentSpoon/Aether1.git ~/Aether1 && cd ~/Aether1 && ./setup.sh && ./start_daemon.sh
```
`setup.sh` will:
1. Install system dependencies for your distro (CachyOS/Arch, Fedora, or Debian/Ubuntu — auto-detected via `/etc/os-release`).
2. Create the Python virtual environment and install all backend dependencies.
3. Generate the tray icons and install the desktop launcher entry.

It's safe to re-run `./setup.sh` any time (e.g. after pulling updates) — every step is idempotent.

`start_daemon.sh` then launches the backend, system tray app, and opens the Holographic HUD in your browser — all in the background, no terminal window needed. Use `./start.sh` instead if you'd rather keep it in the foreground of a terminal.

### Updating an existing install
```bash
cd ~/Aether1 && git pull && ./setup.sh && ./stop.sh && ./start_daemon.sh
```

### Alternative: offline `.tar.gz` transfer (no GitHub access needed)
If the new machine can't reach GitHub, package a portable bundle from a machine
that already has the project instead:
```bash
./package_dist.sh                                # produces dist/cortana-ai-portable.tar.gz
scp dist/cortana-ai-portable.tar.gz user@new-pc:~/
```
Then on the new machine:
```bash
tar -xzf cortana-ai-portable.tar.gz
cd cortana-ai-portable
./setup.sh && ./start_daemon.sh
```

---

## ⚙️ Connecting Local AI (Ollama) or Cloud Models

Click the **⚙️ Settings** button in the top right of the HUD:
1. **For Local 100% Offline AI (Ollama)**:
   - Make sure Ollama is installed and running (`ollama serve`).
   - In Settings, select **Ollama**, set Model to `llama3` (or `mistral`, `deepseek-r1`, `phi3`), and Endpoint to `http://localhost:11434`.
2. **For Cloud AI (Gemini, Groq, OpenAI)**:
   - In Settings, select **Google Gemini API**, **Groq**, or **OpenAI**.
   - Paste your API key into the API Key field and click **Save Changes**.

---

## ⌨️ Instant HUD Quick Commands

You can type or speak these commands directly into the terminal:
- `status` or `diagnostics` — Generates an instant tactical diagnostic readout of CPU, RAM, disk, network, and top processes.
- `who are you` — Identifies the assistant and active subsystem direct line.
- `remember that [fact]` — Archives a fact into persistent SQLite memory (e.g. `remember that my second PC is Fedora 41`).
- `show memories` — Lists all currently remembered items from the neural database.
- `help` — Lists all available voice triggers and features.

---

## 🐳 Optional Docker Deployment

If you prefer running via Docker on either machine:
```bash
docker compose up --build -d
```
Navigate to `http://localhost:8378` in your browser.
