#!/usr/bin/env bash
# Builds a self-contained Linux offline-install bundle: the release binary plus Piper (TTS)
# and whisper.cpp (STT) with a voice and a model already in the box, so the result installs
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
# Output: <output-dir>/aether1-offline-linux-x86_64.tar.gz containing:
#   bin/aether1, bin/whisper-cli
#   share/piper/runtime/ (piper binary + the shared libraries it needs)
#   share/piper/voices/<voice>.onnx(.json)
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
PIPER_VERSION="${PIPER_VERSION:-2023.11.14-2}"
PIPER_VOICE="${PIPER_VOICE:-en_US-lessac-medium}"
WHISPER_CPP_REF="${WHISPER_CPP_REF:-v1.7.2}"
WHISPER_MODEL="${WHISPER_MODEL:-small}"

echo "======================================================================"
echo "📦 Packaging AETHER1 offline bundle (Linux x86_64)"
echo "======================================================================"

mkdir -p "$STAGE"/{bin,share/piper/voices,share/whisper,share/applications,share/icons}

echo "🦀 Building the release binary..."
(cd src-tauri && cargo build --release)
cp src-tauri/target/release/aether1 "$STAGE/bin/aether1"

echo "🔊 Fetching Piper $PIPER_VERSION (TTS)..."
curl -fL "https://github.com/rhasspy/piper/releases/download/${PIPER_VERSION}/piper_linux_x86_64.tar.gz" \
    -o "$STAGE/piper.tar.gz"
tar -xzf "$STAGE/piper.tar.gz" -C "$STAGE"
# The release tarball unpacks to a piper/ directory containing the binary plus the shared
# libraries (onnxruntime, piper_phonemize) it needs at runtime -- those have to travel with
# it, so the whole directory goes into share/piper/ and only the binary itself is symlinked
# into bin/ (which is what ends up on PATH).
mkdir -p "$STAGE/share/piper/runtime"
mv "$STAGE/piper"/* "$STAGE/share/piper/runtime/"
rm -f "$STAGE/piper.tar.gz"
# The real binary stays in share/piper/runtime/ next to the shared libraries it needs at
# runtime (rpath $ORIGIN expects them there) -- install-offline.sh puts a small wrapper
# script on PATH instead of the binary itself, so LD_LIBRARY_PATH gets set correctly no
# matter where this bundle ends up installed.

echo "🔊 Fetching the $PIPER_VOICE voice..."
VOICE_URL_BASE="https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium"
curl -fL "$VOICE_URL_BASE/${PIPER_VOICE}.onnx" -o "$STAGE/share/piper/voices/${PIPER_VOICE}.onnx"
curl -fL "$VOICE_URL_BASE/${PIPER_VOICE}.onnx.json" -o "$STAGE/share/piper/voices/${PIPER_VOICE}.onnx.json"

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
BUNDLE="$OUT_DIR/aether1-offline-linux-x86_64.tar.gz"
tar -czf "$BUNDLE" -C "$STAGE" .

echo ""
echo "======================================================================"
echo "✔ Wrote $BUNDLE ($(du -h "$BUNDLE" | cut -f1))"
echo "  On the target machine: tar -xzf $(basename "$BUNDLE") && ./install-offline.sh"
echo "======================================================================"
