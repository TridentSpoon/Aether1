#!/usr/bin/env bash
# Builds the native Tauri desktop app and (re)installs its Linux launcher entry
# and icon. Shared by ./setup.sh and the post-commit/post-merge/post-checkout
# git hooks (see scripts/git-hooks/) so the installed desktop app never drifts
# from what's actually committed in the repo.
#
# Callers decide how to handle failure: setup.sh and the git hooks both
# tolerate a non-zero exit here (this step is best-effort -- Rust isn't a hard
# requirement for the rest of the project, and a hook must never block a
# commit/merge/checkout), but this script itself must exit non-zero on a
# genuine build failure so callers that DO care (the app's own self-update,
# perform_update() in src-tauri/src/main.rs) can tell a real failure apart
# from a no-op success.
#
# The compiled binary is copied to $HOME/.local/bin/aether1 -- a stable,
# XDG-conventional per-user install location -- and the launcher's Exec line
# points there directly, instead of `cd`-ing into this checkout and running a
# path relative to it. That's what lets the launcher keep working even if this
# checkout is later moved or renamed (as happened once already this project);
# only a rebuild (which every git hook here triggers) needs this checkout's
# current path, and only to know where to copy *from*.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

CARGO_BIN="$HOME/.cargo/bin/cargo"
if [ -x "$CARGO_BIN" ] && [ -d "src-tauri" ]; then
    echo "🦀 Building native desktop app..."
    if ! (cd src-tauri && "$CARGO_BIN" build --release); then
        echo "⚠ Native app build failed -- installed app launcher left pointing at the previous build."
        exit 1
    fi
else
    echo "⚠ Rust/Cargo not found at $CARGO_BIN -- skipping native app build/install."
    echo "   See README's 'Native Desktop App' section to install Rust, then re-run ./setup.sh."
    exit 0
fi

INSTALL_BIN="$HOME/.local/bin/aether1"
mkdir -p "$HOME/.local/bin"
# Copy to a temp file in the same directory and rename it into place, rather than copying
# straight over $INSTALL_BIN. This is almost always still running while this script runs --
# the self-update flow (perform_update() in main.rs) invokes this from inside the very
# process being replaced -- and overwriting a running executable's file in place fails with
# "Text file busy". A rename instead swaps the directory entry atomically: the running
# process keeps its already-open (now unlinked) old binary until it exits, and the new file
# takes over the path for the next launch.
TMP_BIN="$INSTALL_BIN.new.$$"
if ! cp src-tauri/target/release/aether1 "$TMP_BIN" || ! chmod +x "$TMP_BIN" || ! mv -f "$TMP_BIN" "$INSTALL_BIN"; then
    echo "⚠ Could not install the built binary to $INSTALL_BIN."
    rm -f "$TMP_BIN"
    exit 1
fi

mkdir -p "$HOME/.local/share/applications"
sed "s|__AETHER1_INSTALLED_BIN__|$INSTALL_BIN|g" desktop/Aether1.desktop > "$HOME/.local/share/applications/Aether1.desktop"
chmod +x "$HOME/.local/share/applications/Aether1.desktop"

mkdir -p "$HOME/.local/share/icons/hicolor/256x256/apps"
cp desktop/icons/icon.png "$HOME/.local/share/icons/hicolor/256x256/apps/aether1.png" 2>/dev/null || true

echo "✔ Desktop app rebuilt and installed to $INSTALL_BIN; launcher entry refreshed."
echo "  The same binary is the CLI: 'aether1 prompt \"...\"', 'aether1 status', 'aether1 say \"...\"' (aether1 --help)."
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) echo "  Note: $HOME/.local/bin is not on your PATH, so 'aether1' won't resolve as a command yet." ;;
esac
