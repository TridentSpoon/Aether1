#!/usr/bin/env bash
# ==============================================================================
# Project AETHER1 AI Companion - Setup Script
# Installs system dependencies and builds the native Rust app (also used headlessly by
# ./start.sh / ./start_daemon.sh's browser-dev flow via `aether1 --serve`).
# Safe to re-run at any time (idempotent).
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "======================================================================"
echo "⚡ INITIALIZING AETHER1 AI SETUP"
echo "======================================================================"

# This script is for Linux. Windows has its own, which needs no system libraries at all --
# only Rust -- and creates Start Menu and desktop shortcuts.
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*)
        echo ""
        echo "This looks like Windows. Run setup.bat instead (from cmd or PowerShell)."
        echo ""
        exit 1
        ;;
esac

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

# 2. Install system dependencies.
#
# Aether1 builds a Tauri app, which links against the system's webview and GTK stack, so
# these are not optional extras -- without them the build in step 3 fails on a missing
# header and there is no app at the end of this script. They used to be left to the reader
# via a README section that did not exist, which is how "setup completed" and "nothing
# installed" managed to be true at the same time.
#
# The list is Tauri's own Linux prerequisites, libnotify for desktop notifications, and
# espeak-ng -- the always-works speech fallback (see llm/tts.rs's Engine::Auto) that needs
# no separate voice download the way Piper does, so it's the one TTS engine this script can
# actually guarantee rather than just document.
#
# The GStreamer "good" plugins and gst-libav are on this list for a reason that costs hours
# to find on your own. Every engine can synthesize perfectly and the machine still stays
# silent, because the last step is the webview playing a file: Tauri's Linux webview is
# WebKitGTK, and WebKitGTK decodes <audio> through GStreamer. Distributions treat the
# plugins that do the decoding as *optional* for WebKitGTK -- on Arch, gst-plugins-good
# and gst-libav are optdepends, so `pacman -S webkit2gtk-4.1` does not pull them in. The
# WAV that Piper and espeak-ng produce is parsed by wavparse, which lives in
# gst-plugins-good; the MP3 the online voice returns needs gst-libav. Without them the
# <audio> element reports no error and plays nothing, which is indistinguishable from
# "voice doesn't work" and is not visible anywhere in this app's own logs.
echo "📦 Installing build dependencies ($DISTRO)..."
DEPS_OK=1
case "$DISTRO" in
    cachyos|arch|manjaro|endeavouros|garuda)
        $SUDO pacman -Sy --needed --noconfirm \
            git base-devel curl wget file openssl \
            webkit2gtk-4.1 gtk3 libappindicator-gtk3 librsvg xdotool libnotify espeak-ng \
            gst-plugins-good gst-plugins-bad gst-libav \
            || DEPS_OK=0
        ;;
    fedora|nobara|rhel|centos|bazzite)
        $SUDO dnf install -y \
            git curl wget file openssl-devel \
            webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel \
            xdotool libnotify espeak-ng \
            gstreamer1-plugins-good gstreamer1-plugins-bad-free \
            && $SUDO dnf group install -y "c-development" \
            || DEPS_OK=0
        ;;
    ubuntu|debian|pop|linuxmint|zorin|elementary)
        $SUDO apt-get update && $SUDO apt-get install -y \
            git build-essential pkg-config curl wget file libssl-dev \
            libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev \
            libxdo-dev libnotify-bin espeak-ng \
            gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav \
            || DEPS_OK=0
        ;;
    opensuse*|suse|sles)
        $SUDO zypper install -y \
            git curl wget file libopenssl-devel \
            webkit2gtk3-soup2-devel gtk3-devel libappindicator3-devel librsvg-devel \
            xdotool libnotify-tools espeak-ng \
            gstreamer-plugins-good gstreamer-plugins-bad \
            || DEPS_OK=0
        ;;
    *)
        DEPS_OK=0
        echo "⚠ Unrecognized distro ($DISTRO)."
        ;;
esac

if [ "$DEPS_OK" -ne 1 ]; then
    echo ""
    echo "⚠ Could not install the build dependencies automatically."
    echo "  Aether1 needs your distribution's equivalents of:"
    echo "    webkit2gtk 4.1, gtk3, libappindicator-gtk3, librsvg, openssl, xdotool,"
    echo "    a C toolchain (gcc/make/pkg-config), libnotify, and espeak-ng."
    echo "    Also the GStreamer 'good' plugins and gst-libav -- without them the webview"
    echo "    plays no sound at all, however well speech synthesis itself works."
    echo "  Install those, then re-run ./setup.sh. Continuing anyway in case they are"
    echo "  already present."
    echo ""
fi

# Rust is what actually builds the app, and it is not in any of the lists above because
# rustup is the supported way to get it. Say so plainly rather than failing later with a
# confusing "cargo: command not found" from inside a build script.
if [ ! -x "$HOME/.cargo/bin/cargo" ] && ! command -v cargo >/dev/null 2>&1; then
    echo ""
    echo "❌ Rust is not installed, and Aether1 is a Rust application."
    echo "   Install it with:"
    echo ""
    echo "     curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo ""
    echo "   Then open a new terminal (or run: source \"\$HOME/.cargo/env\") and re-run ./setup.sh."
    exit 1
fi

set -e

# 3. Build the native desktop app (Tauri) and install its Linux launcher entry +
# icon -- this also produces the src-tauri/target/release/aether1 binary that
# ./start.sh / ./start_daemon.sh run headlessly via `--serve`.
chmod +x scripts/install_desktop_app.sh
if ! ./scripts/install_desktop_app.sh; then
    echo ""
    echo "======================================================================"
    echo "❌ SETUP FAILED -- the app did not build, so nothing was installed."
    echo ""
    echo "   The build output above says which piece is missing. Most often it is a"
    echo "   system library this script could not install: look for a line mentioning"
    echo "   a package name ending in -dev or -devel, install it, and re-run ./setup.sh."
    echo ""
    echo "   See the README's 'Building from source' section for the full dependency"
    echo "   list per distribution."
    echo "======================================================================"
    exit 1
fi

# 4. Install git hooks so the installed desktop app rebuilds automatically no
# matter how the working tree changes -- a direct commit (post-commit), a
# `git pull` / `git merge` bringing in a PR merged elsewhere (post-merge), or
# switching branches (post-checkout). "Update the installed app" stops being
# a separate manual step for any of these.
if [ -d ".git" ]; then
    mkdir -p .git/hooks
    for hook in post-commit post-merge post-checkout; do
        cp "scripts/git-hooks/$hook" ".git/hooks/$hook"
        chmod +x ".git/hooks/$hook"
    done
    echo "✔ Installed git hooks (post-commit/post-merge/post-checkout) -- keeps the desktop app in sync automatically."
fi

echo ""
echo "======================================================================"
echo "✨ SETUP COMPLETE!"
echo "To start AETHER1 AI:"
echo "   Use the Aether1 Platform entry in your app launcher (native app + tray icon)"
echo "   ./start.sh          (foreground, opens a terminal + browser tab)"
echo "   ./start_daemon.sh   (background, no terminal window needed)"
echo "======================================================================"
