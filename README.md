**The Aether1 Platform**

A basic project for self-hosted or API connected AI companions with a sleek holographic presence, built in the spirit of the the future we saw in Sci-Fi growing up.

Designed natively on Linux and with future OS compatibility in mind, featuring real-time system and AI rellevant telemetry, an interactive 3D holographic avatar, with speech capabilities, a local persistent memory, and a system tray companion in or hovering next to your notification bar.

Intended features

- **3D Holographic Avatar**: 5 inspired avatars with what will hopefully be familiar personas.
- **Futuristic HUD & Live Telemetry**: Real-time monitoring of Hardware Telemetry.
- **System Notification Bar Integration**:
  - Displays a dynamic glowing status icon in your system tray (Cyan = Active, Green = Listening, Purple = Processing, Amber = Alert/Offline).
  - Quick action menu on click to open the HUD, Trigger instant diagnostics, show the avatar or close out the program.
- **Multi-Provider AI Engine**:
  - **Local Offline AI**: Connect or Install a Local AI with zero cloud dependencies.
  - **Cloud AI APIs**: Support using different popular services via API or Local App installation.
  - **Offline Standby Mode**: While offline the Platform can still use TTS to read out the basic local system stats/information.
- **Neural Voice Synthesis & Speech Recognition**:
  - Neural text-to-speech. With Persona specific voices
  - Speech-to-Text via Web Speech API with auto-send voice chat.
  - Built-in Web Audio API sci-fi synthesizer for UI sound effects.
- **Persistent Long-Term Memory**:
  - Persistent database storing chat sessions, user preferences, and memories across system reboots.
  - Type `remember that [fact]` or `save memory [key]: [value]` to store data permanently and locally.
- **1-Click Packaging & Portability**:
  - Auto-installer for Linux and Windows.
  - Easy installation from GitHub via script or via packaging the entire system into a single `.tar.gz` bundle to transfer to a separate system.

_*_*_*_*_*_*_*_*_*_*_*_*_*_*_*_*_*_*_*_*_*_*_*_*_*_*_
TEXT TO STILL REVIEW
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
./package_dist.sh                                # produces dist/aether1-portable.tar.gz
scp dist/aether1-portable.tar.gz user@new-pc:~/
```
Then on the new machine:
```bash
tar -xzf aether1-portable.tar.gz
cd aether1-portable
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

## 🖥️ Native Desktop App (No Browser Needed)

The **Aether1 Platform** entry in your app launcher opens AETHER1 as a native desktop
window via [Tauri](https://tauri.app/) (Rust) instead of a Python server + browser tab. The
app is self-contained: it launches its own backend automatically, shuts it down when you
close the window (or reuses one that's already running, if you started it separately), and
shows a system tray icon while running (left-click for a Show/Update/Quit menu -- the tray
checks for updates on launch and lets you install one with a click; see "Updating" below).

**One-time setup:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # installs Rust (no sudo)
cargo install tauri-cli --version "^2.0.0" --locked
./setup.sh   # if you haven't already, to create the Python venv the backend still runs in
```
On Linux you'll also need `webkit2gtk`, `libappindicator-gtk3`, `appmenu-gtk-module`, and
`patchelf` from your package manager (already covered by `setup.sh`'s dependency step on
Arch/Fedora/Debian).

**Run it:**
```bash
cd src-tauri && cargo tauri dev
```

This is an early, incremental migration — the window itself and the 3D avatars are fully
native Rust/Tauri, but the backend logic (chat, TTS, telemetry, memory) is still the same
Python/FastAPI server under the hood for now, launched automatically rather than something
you start by hand.

**Updating:** the tray icon checks GitHub for a newer commit on `main` on launch, and again
any time you click "Check for Updates." If one's available, the menu item turns into
"⬆ Update Available" — click it to pull, rebuild, and relaunch automatically. This repo is
currently private, so that update check and the `git pull` it triggers both authenticate
however your machine's `git`/`gh` are already set up (an SSH key with push access, in
practice) — there's no separate credential involved. If this project ever goes public,
that needs to be replaced with a real public update mechanism (e.g. Tauri's signed-updater
plugin against public release artifacts) before anyone without repo access could use it.

---

## 🐳 Optional Docker Deployment

If you prefer running via Docker on either machine:
```bash
docker compose up --build -d
```
Navigate to `http://localhost:8378` in your browser.
