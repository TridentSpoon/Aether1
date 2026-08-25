#!/usr/bin/env bash
# ==============================================================================
# Project AETHER1 AI Companion - Setup Script
# Installs system dependencies and configures the Python virtualenv.
# Safe to re-run at any time (idempotent).
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "======================================================================"
echo "⚡ INITIALIZING AETHER1 AI SETUP"
echo "======================================================================"

# 1. Detect Linux Distribution
DISTRO="unknown"
if [ -f /etc/os-release ]; then
    . /etc/os-release
    DISTRO="$ID"
    echo "Detected Operating System: $NAME ($ID)"
fi

SUDO=""
if [ "$(id -u)" -ne 0 ]; then
    SUDO="sudo"
fi

# 2. Install system dependencies (best-effort; the venv/pip steps below are what
# actually matter, so a failure here is a warning, not a hard stop).
# The only non-pip runtime dependency this project needs is `notify-send`
# (libnotify) for desktop notifications. pystray's tray icon, Pillow, and
# everything else are pure pip packages -- no GTK/AppIndicator system libs
# required.
echo "📦 Installing system dependencies ($DISTRO)..."
case "$DISTRO" in
    cachyos|arch|manjaro|endeavouros)
        $SUDO pacman -Sy --needed --noconfirm python python-pip git libnotify \
            || echo "⚠ System package install failed or was skipped -- continuing anyway."
        ;;
    fedora|nobara|rhel|centos)
        $SUDO dnf install -y python3 python3-pip git libnotify \
            || echo "⚠ System package install failed or was skipped -- continuing anyway."
        ;;
    ubuntu|debian|pop|linuxmint)
        $SUDO apt-get update && $SUDO apt-get install -y python3 python3-pip python3-venv git libnotify-bin \
            || echo "⚠ System package install failed or was skipped -- continuing anyway."
        ;;
    *)
        echo "⚠ Unrecognized distro ($DISTRO). Please ensure python3, pip, git, and libnotify are installed manually."
        ;;
esac

# 3. Check for Python 3 (hard requirement -- stop here if still missing)
if ! command -v python3 &> /dev/null; then
    echo "❌ Python 3 is not installed and could not be auto-installed."
    echo "   Install it manually, then re-run ./setup.sh"
    exit 1
fi
echo "✔ Python 3 found: $(python3 --version)"

set -e

# 4. Create Python Virtual Environment
if [ ! -d "venv" ]; then
    echo "📦 Creating isolated Python virtual environment (venv)..."
    python3 -m venv venv
else
    echo "✔ Python virtual environment already exists."
fi

# 5. Install Dependencies
echo "📥 Installing backend & desktop dependencies..."
./venv/bin/pip install --upgrade pip
./venv/bin/pip install -r backend/requirements.txt

# 6. Generate Hologram Tray Icons
echo "🎨 Generating holographic tray & app icons..."
./venv/bin/python desktop/generate_icons.py

# 7. Install Linux Desktop Launcher & Autostart (Optional)
mkdir -p "$HOME/.local/share/applications"
sed "s|%k/..|$SCRIPT_DIR|g" desktop/aether-cortana.desktop > "$HOME/.local/share/applications/aether-cortana.desktop"
chmod +x "$HOME/.local/share/applications/aether-cortana.desktop"

# Copy Icon
mkdir -p "$HOME/.local/share/icons/hicolor/256x256/apps"
cp desktop/icons/icon.png "$HOME/.local/share/icons/hicolor/256x256/apps/cortana-ai.png" 2>/dev/null || true

echo ""
echo "======================================================================"
echo "✨ SETUP COMPLETE!"
echo "To start AETHER1 AI:"
echo "   ./start.sh          (foreground, opens a terminal + browser tab)"
echo "   ./start_daemon.sh   (background, no terminal window needed)"
echo "======================================================================"
