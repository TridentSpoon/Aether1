#!/usr/bin/env bash
# Builds the *slim* Linux install bundle: just the release binary, the desktop launcher,
# and an install script -- no Piper, no whisper.cpp, no models. Counterpart to
# package_offline_linux.sh, which bundles everything including those; this one instead has
# install-slim-linux.sh pip-install Piper (piper-tts) and Whisper (faster-whisper) on the
# target machine, trading "works with the network unplugged" for a much smaller download.
#
# Usage: ./scripts/package_slim_linux.sh [output-dir]
#
# Output: <output-dir>/aether1-slim-linux-x86_64.tar.gz containing:
#   bin/aether1
#   share/applications/Aether1.desktop, share/icons/aether1.png
#   install-slim-linux.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

OUT_DIR="${1:-dist}"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

echo "======================================================================"
echo "📦 Packaging AETHER1 slim bundle (Linux x86_64)"
echo "======================================================================"

mkdir -p "$STAGE"/{bin,share/applications,share/icons}

echo "🦀 Building the release binary..."
(cd src-tauri && cargo build --release)
cp src-tauri/target/release/aether1 "$STAGE/bin/aether1"

sed "s|__AETHER1_INSTALLED_BIN__|\$HOME/.local/bin/aether1|g" desktop/Aether1.desktop \
    > "$STAGE/share/applications/Aether1.desktop"
cp desktop/icons/icon.png "$STAGE/share/icons/aether1.png"

cp scripts/install_slim_linux.sh "$STAGE/install-slim-linux.sh"
chmod +x "$STAGE/install-slim-linux.sh"
# No THIRD_PARTY_NOTICES.md here (unlike the offline bundle) -- this bundle doesn't
# actually contain Piper, whisper.cpp, or espeak-ng; install-slim-linux.sh fetches them
# separately via pip and the system package manager, each under its own license as always.

mkdir -p "$OUT_DIR"
BUNDLE="$OUT_DIR/aether1-slim-linux-x86_64.tar.gz"
tar -czf "$BUNDLE" -C "$STAGE" .

echo ""
echo "======================================================================"
echo "✔ Wrote $BUNDLE ($(du -h "$BUNDLE" | cut -f1))"
echo "  On the target machine: tar -xzf $(basename "$BUNDLE") && ./install-slim-linux.sh"
echo "======================================================================"
