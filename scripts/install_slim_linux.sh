#!/usr/bin/env bash
# Installs AETHER1 from this slim bundle onto the machine it is run on, then fetches
# Piper (piper-tts) and Whisper (faster-whisper) via pip instead of unpacking prebuilt
# binaries -- this is the counterpart to offline_install_linux.sh for someone who would
# rather have a small download than a fully offline-forever install. It does need a
# network (for the package manager and pip) and, unlike the offline bundle, does use sudo
# -- once, to install Python/pip/espeak-ng if they are not already present.
#
# Speech works after this finishes: Piper for text-to-speech, faster-whisper for
# speech-to-text, both installed for this user only (pip --user, no system Python
# touched). Neither downloads its actual model until first used -- that happens once,
# automatically, the first time you speak to or with AETHER1.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "======================================================================"
echo "⚡ INSTALLING AETHER1 (SLIM BUNDLE)"
echo "======================================================================"

BIN_DIR="$HOME/.local/bin"
SHARE_DIR="$HOME/.local/share"
mkdir -p "$BIN_DIR" "$SHARE_DIR/applications" "$SHARE_DIR/icons/hicolor/256x256/apps"

# If this machine previously had the *offline* bundle installed, its Piper runtime/voices
# and whisper.cpp binary/model are dead weight here -- the slim bundle gets Piper and
# Whisper from pip instead (see below) and never reads $SHARE_DIR/piper or
# $SHARE_DIR/whisper. Safe to remove unconditionally: neither holds anything but those
# prebuilt binaries/models, and app settings/conversation history live entirely separately
# under $SHARE_DIR/aether1 (see project_root() in main.rs), which this never touches.
if [ -d "$SHARE_DIR/piper" ] || [ -d "$SHARE_DIR/whisper" ] || [ -f "$BIN_DIR/whisper-cli" ]; then
    echo "🧹 Removing leftover files from a previous offline-bundle install..."
    rm -rf "$SHARE_DIR/piper" "$SHARE_DIR/whisper"
    rm -f "$BIN_DIR/whisper-cli"
fi

echo "📦 Installing the app..."
install -m 755 bin/aether1 "$BIN_DIR/aether1"

echo "🖥️ Installing the desktop launcher..."
sed "s|__AETHER1_INSTALLED_BIN__|$BIN_DIR/aether1|g" share/applications/Aether1.desktop \
    > "$SHARE_DIR/applications/Aether1.desktop"
chmod +x "$SHARE_DIR/applications/Aether1.desktop"
cp share/icons/aether1.png "$SHARE_DIR/icons/hicolor/256x256/apps/aether1.png" 2>/dev/null || true

# Same distro detection as ./setup.sh, but a much shorter list: aether1 itself is already
# built (this bundle's whole point), so none of the Tauri build-time -dev/-devel headers or
# a C toolchain are needed here -- only what the *installed* app links against at runtime
# (webkit2gtk, gtk3, etc., which desktop Linux almost always already has) plus Python/pip
# for the two engines below and espeak-ng as the always-works fallback that needs no model
# download the way Piper and whisper do.
#
# The GStreamer plugins are on the runtime list for the reason setup.sh spells out at
# length: WebKitGTK plays <audio> through GStreamer and distributions make the decoding
# plugins optional, so without them every voice engine works perfectly and the machine
# stays silent.
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

echo "📦 Installing Python, pip, espeak-ng and the audio decoders ($DISTRO)..."
DEPS_OK=1
case "$DISTRO" in
    cachyos|arch|manjaro|endeavouros|garuda)
        $SUDO pacman -Sy --needed --noconfirm python python-pip espeak-ng \
            gst-plugins-good gst-plugins-bad gst-libav || DEPS_OK=0
        ;;
    fedora|nobara|rhel|centos|bazzite)
        $SUDO dnf install -y python3 python3-pip espeak-ng \
            gstreamer1-plugins-good gstreamer1-plugins-bad-free || DEPS_OK=0
        ;;
    ubuntu|debian|pop|linuxmint|zorin|elementary)
        $SUDO apt-get update && $SUDO apt-get install -y python3 python3-venv \
            python3-pip espeak-ng gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
            gstreamer1.0-libav || DEPS_OK=0
        ;;
    opensuse*|suse|sles)
        $SUDO zypper install -y python3 python3-pip espeak-ng \
            gstreamer-plugins-good gstreamer-plugins-bad || DEPS_OK=0
        ;;
    *)
        DEPS_OK=0
        echo "⚠ Unrecognized distro ($DISTRO)."
        ;;
esac

if [ "$DEPS_OK" -ne 1 ]; then
    echo ""
    echo "⚠ Could not install Python/pip/espeak-ng automatically."
    echo "  Install your distribution's equivalents of python3, python3-pip, espeak-ng,"
    echo "  and the GStreamer 'good' plugins plus gst-libav (without those last ones the" 
    echo "  app plays no sound at all, however well speech synthesis itself works),"
    echo "  then re-run this script -- it is safe to run again."
fi

PYTHON="$(command -v python3 || command -v python || true)"
if [ -z "$PYTHON" ]; then
    echo ""
    echo "❌ No python3/python found on PATH -- Piper and Whisper cannot be installed."
    echo "   Install Python for your distribution, then re-run this script."
    exit 1
fi

# Not `pip install --user`. Arch, Debian 12+, Ubuntu 23.04+ and Fedora all mark the system
# Python as externally managed (PEP 668), so that line now stops with
# `error: externally-managed-environment` and this script would fail on exactly the
# distributions it is aimed at. A virtual environment is what that error recommends;
# Aether1 knows to look in this one (see paths::managed_python_env in src-tauri), so
# nothing here needs to go on PATH.
PYENV_DIR="$HOME/.local/share/aether1/pyenv"
echo "🔊🎤 Installing Piper and Whisper into $PYENV_DIR ..."
if [ ! -x "$PYENV_DIR/bin/python" ]; then
    "$PYTHON" -m venv "$PYENV_DIR" || {
        echo "❌ Could not create the Python environment at $PYENV_DIR."
        echo "   On Debian and Ubuntu this usually means python3-venv is missing:"
        echo "     sudo apt install python3-venv"
        exit 1
    }
fi
"$PYENV_DIR/bin/pip" install --upgrade piper-tts faster-whisper

echo ""
echo "======================================================================"
echo "✨ INSTALL COMPLETE"
echo "   Use the Aether1 Platform entry in your app launcher, or run:"
echo "     $BIN_DIR/aether1"
echo "======================================================================"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) echo "Note: $BIN_DIR is not on your PATH yet -- add it to run 'aether1' by name." ;;
esac
