#!/usr/bin/env bash
# ==============================================================================
# Project AETHER / CORTANA AI Companion - Setup Script
# Auto-configures Python virtualenv and system dependencies for CachyOS / Fedora
# ==============================================================================

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "======================================================================"
echo "⚡ INITIALIZING AETHER / CORTANA AI SETUP"
echo "======================================================================"

# 1. Detect Linux Distribution
DISTRO="unknown"
if [ -f /etc/os-release ]; then
    . /etc/os-release
    DISTRO=$ID
    echo "Detected Operating System: $NAME ($ID)"
fi

# 2. Check for Python 3
if ! command -v python3 &> /dev/null; then
    echo "❌ Python 3 is not installed."
    if [ "$DISTRO" = "cachyos" ] || [ "$DISTRO" = "arch" ]; then
        echo "Install via: sudo pacman -S python python-pip"
    elif [ "$DISTRO" = "fedora" ]; then
        echo "Install via: sudo dnf install python3 python3-pip"
    fi
    exit 1
fi

echo "✔ Python 3 found: $(python3 --version)"

# 3. Create Python Virtual Environment
if [ ! -d "venv" ]; then
    echo "📦 Creating isolated Python virtual environment (venv)..."
    python3 -m venv venv
else
    echo "✔ Python virtual environment already exists."
fi

# 4. Install Dependencies
echo "📥 Installing backend & desktop dependencies..."
./venv/bin/pip install --upgrade pip
./venv/bin/pip install -r backend/requirements.txt

# 5. Generate Hologram Tray Icons
echo "🎨 Generating holographic tray & app icons..."
./venv/bin/python desktop/generate_icons.py

# 6. Install Linux Desktop Launcher & Autostart (Optional)
mkdir -p "$HOME/.local/share/applications"
sed "s|%k/..|$SCRIPT_DIR|g" desktop/aether-cortana.desktop > "$HOME/.local/share/applications/aether-cortana.desktop"
chmod +x "$HOME/.local/share/applications/aether-cortana.desktop"

# Copy Icon
mkdir -p "$HOME/.local/share/icons/hicolor/256x256/apps"
cp desktop/icons/icon.png "$HOME/.local/share/icons/hicolor/256x256/apps/cortana-ai.png" 2>/dev/null || true

echo ""
echo "======================================================================"
echo "✨ SETUP COMPLETE!"
echo "To start AETHER / CORTANA AI:"
echo "   ./start.sh"
echo "======================================================================"
