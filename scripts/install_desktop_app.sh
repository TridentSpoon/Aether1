#!/usr/bin/env bash
# Builds the native Tauri desktop app and (re)installs its Linux launcher entry
# and icon. Shared by ./setup.sh and the post-commit git hook (see
# scripts/git-hooks/post-commit) so the installed desktop app never drifts
# from what's actually committed in the repo. Best-effort: never hard-fails,
# since it can run automatically after a commit and shouldn't block that.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

CARGO_BIN="$HOME/.cargo/bin/cargo"
if [ -x "$CARGO_BIN" ] && [ -d "src-tauri" ]; then
    echo "🦀 Building native desktop app..."
    if ! (cd src-tauri && "$CARGO_BIN" build --release); then
        echo "⚠ Native app build failed -- installed app launcher left pointing at the previous build."
        exit 0
    fi
else
    echo "⚠ Rust/Cargo not found at $CARGO_BIN -- skipping native app build/install."
    echo "   See README's 'Native Desktop App' section to install Rust, then re-run ./setup.sh."
    exit 0
fi

mkdir -p "$HOME/.local/share/applications"
sed "s|%k/..|$SCRIPT_DIR|g" desktop/Aether1.desktop > "$HOME/.local/share/applications/Aether1.desktop"
chmod +x "$HOME/.local/share/applications/Aether1.desktop"

mkdir -p "$HOME/.local/share/icons/hicolor/256x256/apps"
cp desktop/icons/icon.png "$HOME/.local/share/icons/hicolor/256x256/apps/aether1.png" 2>/dev/null || true

echo "✔ Desktop app rebuilt and launcher entry refreshed."
