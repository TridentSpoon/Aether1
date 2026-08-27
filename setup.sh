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

# 2. Install system dependencies (best-effort; the Rust build step below is what
# actually matters, so a failure here is a warning, not a hard stop). `notify-send`
# (libnotify) is for desktop notifications; see README's "Native Desktop App" section
# for the Tauri Linux build prerequisites (webkit2gtk, libappindicator-gtk3, etc.) if
# the build step below fails on a missing system library.
echo "📦 Installing system dependencies ($DISTRO)..."
case "$DISTRO" in
    cachyos|arch|manjaro|endeavouros)
        $SUDO pacman -Sy --needed --noconfirm git libnotify \
            || echo "⚠ System package install failed or was skipped -- continuing anyway."
        ;;
    fedora|nobara|rhel|centos)
        $SUDO dnf install -y git libnotify \
            || echo "⚠ System package install failed or was skipped -- continuing anyway."
        ;;
    ubuntu|debian|pop|linuxmint)
        $SUDO apt-get update && $SUDO apt-get install -y git libnotify-bin \
            || echo "⚠ System package install failed or was skipped -- continuing anyway."
        ;;
    *)
        echo "⚠ Unrecognized distro ($DISTRO). Please ensure git and libnotify are installed manually."
        ;;
esac

set -e

# 3. Build the native desktop app (Tauri) and install its Linux launcher entry +
# icon -- this also produces the src-tauri/target/release/aether1 binary that
# ./start.sh / ./start_daemon.sh run headlessly via `--serve`.
chmod +x scripts/install_desktop_app.sh
./scripts/install_desktop_app.sh || echo "⚠ Native app install step failed -- see README's 'Native Desktop App' section to install Rust, then re-run ./setup.sh."

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
