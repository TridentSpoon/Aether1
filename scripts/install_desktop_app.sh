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
    echo "   Install Rust with:  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo "   then re-run ./setup.sh."
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

# **The second command name.** `aurora1` is the companion's command and `aether1` is the one
# behind the curtain; every subcommand works under both, and the help text names whichever one
# you typed (see cli::invoked_as). A hard link rather than a copy: it is the same binary, so a
# copy would double the disk for nothing and could drift from the real one after an update.
# Through a temporary name and renamed into place for exactly the reason above -- this script
# is what the self-update flow runs, and a running `aurora1` cannot be replaced where it sits.
ALT_BIN="$HOME/.local/bin/aurora1"
TMP_ALT="$ALT_BIN.new.$$"
if ln -f "$INSTALL_BIN" "$TMP_ALT" 2>/dev/null || cp "$INSTALL_BIN" "$TMP_ALT"; then
    chmod +x "$TMP_ALT"
    mv -f "$TMP_ALT" "$ALT_BIN"
else
    rm -f "$TMP_ALT"
    echo "⚠ Could not create the 'aurora1' command; 'aether1' works the same way."
fi

mkdir -p "$HOME/.local/share/applications"
sed "s|__AETHER1_INSTALLED_BIN__|$INSTALL_BIN|g" desktop/Aether1.desktop > "$HOME/.local/share/applications/Aether1.desktop"
chmod +x "$HOME/.local/share/applications/Aether1.desktop"

mkdir -p "$HOME/.local/share/icons/hicolor/256x256/apps"
cp desktop/icons/icon.png "$HOME/.local/share/icons/hicolor/256x256/apps/aether1.png" 2>/dev/null || true

echo "✔ Desktop app rebuilt and installed to $INSTALL_BIN; launcher entry refreshed."
echo "  The same binary is the CLI, under either name: 'aurora1 prompt \"...\"', 'aurora1 status', 'aurora1 say \"...\"' (aurora1 --help)."
echo "  'aether1' runs all of the same commands -- it is the name behind the curtain."
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) echo "  Note: $HOME/.local/bin is not on your PATH, so neither 'aurora1' nor 'aether1' will resolve as a command yet." ;;
esac
