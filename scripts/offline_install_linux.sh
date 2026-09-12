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
    "$SHARE_DIR/piper/voices" "$SHARE_DIR/whisper"

echo "📦 Installing the app..."
install -m 755 bin/aether1 "$BIN_DIR/aether1"

# If this machine previously had the *slim* bundle installed, its pip-installed piper-tts
# and faster-whisper packages are dead weight here -- the offline bundle brings its own
# Piper runtime and whisper.cpp below and never touches Python. Uninstalling is optional
# (it needs no network, pip uninstall works fully offline) so a failure here -- no
# python/pip on PATH, or nothing to uninstall -- is not fatal to the install.
PYTHON="$(command -v python3 || command -v python || true)"
if [ -n "$PYTHON" ] && "$PYTHON" -m pip show piper-tts >/dev/null 2>&1; then
    echo "🧹 Removing leftover pip packages from a previous slim-bundle install..."
    "$PYTHON" -m pip uninstall -y piper-tts faster-whisper >/dev/null 2>&1 || true
fi

echo "🔊 Installing Piper (local speech)..."
rm -rf "$SHARE_DIR/piper/runtime"
cp -r share/piper/runtime "$SHARE_DIR/piper/runtime"
cp share/piper/voices/*.onnx share/piper/voices/*.onnx.json "$SHARE_DIR/piper/voices/"
# A wrapper rather than the bare binary on PATH: piper looks for its shared libraries next
# to itself via rpath $ORIGIN, which only holds if something invoking it by a plain `piper`
# on PATH still resolves those libraries -- LD_LIBRARY_PATH here is what makes that true
# regardless of where this bundle was unpacked and installed from.
cat > "$BIN_DIR/piper" <<EOF
#!/usr/bin/env bash
export LD_LIBRARY_PATH="$SHARE_DIR/piper/runtime\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}"
exec "$SHARE_DIR/piper/runtime/piper" "\$@"
EOF
chmod +x "$BIN_DIR/piper"

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
echo "     $BIN_DIR/aether1"
echo "======================================================================"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) echo "Note: $BIN_DIR is not on your PATH yet -- add it to run 'aether1' by name." ;;
esac
