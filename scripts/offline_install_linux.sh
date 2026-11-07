#!/usr/bin/env bash
# Installs AETHER1 from this offline bundle onto the machine it is run on. Touches nothing
# outside $HOME (no sudo, no package manager, no network) -- everything it needs is already
# sitting next to this script, which is the whole point of an *offline* installer.
#
# This is the counterpart to ./setup.sh for a machine with no internet access: setup.sh
# builds from source and fetches its own dependencies as it goes, which needs a network at
# every step; this unpacks pre-built binaries and models that were fetched once, elsewhere,
# by scripts/package_offline_linux.sh.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "======================================================================"
echo "⚡ INSTALLING AETHER1 (OFFLINE BUNDLE)"
echo "======================================================================"

BIN_DIR="$HOME/.local/bin"
SHARE_DIR="$HOME/.local/share"
mkdir -p "$BIN_DIR" "$SHARE_DIR/applications" "$SHARE_DIR/icons/hicolor/256x256/apps" \
    "$SHARE_DIR/whisper"

echo "📦 Installing the app..."
install -m 755 bin/aether1 "$BIN_DIR/aether1"

# **The second command name.** `aurora1` is the companion's command and `aether1` is the one
# behind the curtain; every subcommand works under both, and the help text names whichever one
# you typed (see cli::invoked_as). A hard link rather than a copy: it is the same binary, so a
# copy would double the disk for nothing and could drift from the real one after an update.
#
# Linked through a temporary name and renamed into place, for the same reason the binary above
# is: an `aurora1` that is currently running cannot be replaced where it stands ("Text file
# busy"), while a rename swaps the directory entry and leaves the running process holding the
# old file until it exits. `ln -f` is tried first and a copy is the fallback, so a filesystem
# that refuses hard links still ends up with both names.
TMP_ALT="$BIN_DIR/aurora1.new.$$"
if ln -f "$BIN_DIR/aether1" "$TMP_ALT" 2>/dev/null || cp "$BIN_DIR/aether1" "$TMP_ALT"; then
    chmod 755 "$TMP_ALT"
    mv -f "$TMP_ALT" "$BIN_DIR/aurora1"
else
    rm -f "$TMP_ALT"
    echo "Note: could not create the 'aurora1' command; 'aether1' works the same way."
fi

# If this machine previously had the *slim* bundle installed, its pip-installed
# faster-whisper package is optional dead weight here -- this bundle brings whisper.cpp
# and never touches Python. Any cleanup is optional and non-fatal.
PYTHON="$(command -v python3 || command -v python || true)"
if [ -n "$PYTHON" ] && "$PYTHON" -m pip show faster-whisper >/dev/null 2>&1; then
    echo "🧹 Removing leftover pip packages from a previous slim-bundle install..."
    "$PYTHON" -m pip uninstall -y faster-whisper >/dev/null 2>&1 || true
fi

if ! command -v espeak-ng >/dev/null 2>&1 && ! command -v espeak >/dev/null 2>&1; then
    echo "⚠ Linux system TTS is unavailable. Install eSpeak NG from your distro package manager."
else
    echo "🔊 Using Linux system speech."
fi

echo "🎤 Installing whisper.cpp (local listening)..."
install -m 755 bin/whisper-cli "$BIN_DIR/whisper-cli"
cp share/whisper/*.bin "$SHARE_DIR/whisper/"

echo "🖥️ Installing the desktop launcher..."
sed "s|__AETHER1_INSTALLED_BIN__|$BIN_DIR/aether1|g" share/applications/Aether1.desktop \
    > "$SHARE_DIR/applications/Aether1.desktop"
chmod +x "$SHARE_DIR/applications/Aether1.desktop"
cp share/icons/aether1.png "$SHARE_DIR/icons/hicolor/256x256/apps/aether1.png" 2>/dev/null || true

echo ""
echo "======================================================================"
echo "✨ INSTALL COMPLETE"
echo "   Use the Aether1 Platform entry in your app launcher, or run:"
echo "     $BIN_DIR/aurora1   (and $BIN_DIR/aether1, the same binary under its other name)"
echo "======================================================================"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) echo "Note: $BIN_DIR is not on your PATH yet -- add it to run 'aurora1' by name." ;;
esac
