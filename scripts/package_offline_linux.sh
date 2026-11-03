#!/usr/bin/env bash
# Builds a self-contained Linux offline-install bundle: the release binary plus whisper.cpp
# (STT) and its speech-recognition model, so the result installs
# on a machine with no internet access at all -- this script is the only part of the whole
# offline story that needs a network, and it is meant to be run once, by whoever is cutting
# a release (a maintainer's machine, or the release.yml GitHub Actions workflow).
#
# The native Tauri app has its frontend compiled into the binary (see tauri.conf.json's
# frontendDist) -- that is what makes a single executable enough here. This bundle is for
# the desktop app specifically; the browser-based dev flow (./start.sh) still needs a full
# checkout and is not what this produces.
#
# Usage: ./scripts/package_offline_linux.sh [output-dir]
#
# Output: <output-dir>/aurora1-offline-linux-x86_64.tar.gz containing:
#   bin/aether1, bin/whisper-cli
#   share/whisper/<model>.bin
#   share/applications/Aether1.desktop, share/icons/aether1.png
#   install-offline.sh, THIRD_PARTY_NOTICES.md
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

OUT_DIR="${1:-dist}"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

# Pinned rather than "latest" so a release built today and one built in six months bundle
# the same thing -- bump these deliberately, not by surprise on the next run.
#
# WHISPER_CPP_REF must be new enough that the CLI example is named `whisper-cli`
# (examples/cli/) rather than the older `main` (examples/main/) -- confirmed present at
# v1.9.3, confirmed absent at v1.7.2 (the --target whisper-cli build below fails there with
# "No rule to make target"). If bumping this ever breaks the same way, check
# examples/cli/CMakeLists.txt exists at the new ref before assuming something else broke.
WHISPER_CPP_REF="${WHISPER_CPP_REF:-v1.9.3}"
WHISPER_MODEL="${WHISPER_MODEL:-small}"

echo "======================================================================"
echo "📦 Packaging AETHER1 offline bundle (Linux x86_64)"
echo "======================================================================"

mkdir -p "$STAGE"/{bin,share/whisper,share/applications,share/icons}

echo "🦀 Building the release binary..."
(cd src-tauri && cargo build --release)
cp src-tauri/target/release/aether1 "$STAGE/bin/aether1"

# Linux has no single built-in speech API shared by distributions. The system voice is
# eSpeak NG, installed by the slim setup through the distribution package manager. The
# offline installer cannot install system packages and reports if eSpeak NG is missing.

echo "🎤 Building whisper.cpp $WHISPER_CPP_REF (STT)..."
git clone --depth 1 --branch "$WHISPER_CPP_REF" https://github.com/ggml-org/whisper.cpp "$STAGE/whisper.cpp-src"
cmake -S "$STAGE/whisper.cpp-src" -B "$STAGE/whisper.cpp-build" \
    -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_SHARED_LIBS=OFF \
    -DGGML_NATIVE=OFF
cmake --build "$STAGE/whisper.cpp-build" --config Release -j"$(nproc)" --target whisper-cli
cp "$STAGE/whisper.cpp-build/bin/whisper-cli" "$STAGE/bin/whisper-cli"
rm -rf "$STAGE/whisper.cpp-src" "$STAGE/whisper.cpp-build"

echo "🎤 Fetching the $WHISPER_MODEL Whisper model..."
curl -fL "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-${WHISPER_MODEL}.bin" \
    -o "$STAGE/share/whisper/ggml-${WHISPER_MODEL}.bin"

sed "s|__AETHER1_INSTALLED_BIN__|\$HOME/.local/bin/aether1|g" desktop/Aether1.desktop \
    > "$STAGE/share/applications/Aether1.desktop"
cp desktop/icons/icon.png "$STAGE/share/icons/aether1.png"

cp scripts/offline_install_linux.sh "$STAGE/install-offline.sh"
chmod +x "$STAGE/install-offline.sh"
cp THIRD_PARTY_NOTICES.md "$STAGE/THIRD_PARTY_NOTICES.md"

mkdir -p "$OUT_DIR"
BUNDLE="$OUT_DIR/aurora1-offline-linux-x86_64.tar.gz"
tar -czf "$BUNDLE" -C "$STAGE" .

echo ""
echo "======================================================================"
echo "✔ Wrote $BUNDLE ($(du -h "$BUNDLE" | cut -f1))"
echo "  On the target machine: mkdir aurora1-offline && tar -xzf $(basename "$BUNDLE") -C aurora1-offline"
echo "  then:                 cd aurora1-offline && ./install-offline.sh"
echo "======================================================================"
