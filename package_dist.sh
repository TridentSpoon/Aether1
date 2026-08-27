#!/usr/bin/env bash
# ==============================================================================
# Project AETHER1 AI - Distribution Packager
# Builds clean portable .tar.gz and .zip packages for transfer to your second machine (Fedora)
# ==============================================================================

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

DIST_DIR="$SCRIPT_DIR/dist"
PKG_NAME="aether1-portable"
TEMP_BUILD="$DIST_DIR/$PKG_NAME"

echo "======================================================================"
echo "📦 PACKAGING AETHER1 AI FOR DISTRIBUTION"
echo "======================================================================"

# Clean previous build
rm -rf "$DIST_DIR"
mkdir -p "$TEMP_BUILD"

# Copy source directories and files -- src-tauri/ ships as SOURCE (not a prebuilt binary),
# so the target machine builds its own release binary via ./setup.sh / ./start.sh's
# auto-build-if-missing step. This is deliberate: a prebuilt binary is tied to the build
# machine's glibc/ABI and can fail to run on a different distro, whereas building on the
# target works on any Linux distro with Rust installed.
echo "📂 Copying project components..."
cp -r src-tauri "$TEMP_BUILD/"
cp -r desktop "$TEMP_BUILD/"
cp -r frontend "$TEMP_BUILD/"
cp -r scripts "$TEMP_BUILD/"
cp setup.sh start.sh start_daemon.sh stop.sh start.bat README.md "$TEMP_BUILD/"

# Don't ship a stale/huge build artifact -- the target machine builds its own.
rm -rf "$TEMP_BUILD/src-tauri/target"

# Remove any local audio cache from the bundle to keep the package light.
mkdir -p "$TEMP_BUILD/backend/audio_cache"
rm -rf "$TEMP_BUILD/backend/audio_cache"/* 2>/dev/null || true

# Set execute permissions
chmod +x "$TEMP_BUILD/setup.sh" "$TEMP_BUILD/start.sh" "$TEMP_BUILD/start_daemon.sh" "$TEMP_BUILD/stop.sh"

# Create Archives
echo "🗜️  Compressing into tar.gz and zip..."
cd "$DIST_DIR"
tar -czf "$PKG_NAME.tar.gz" "$PKG_NAME"
zip -rq "$PKG_NAME.zip" "$PKG_NAME" 2>/dev/null || true

cd "$SCRIPT_DIR"

echo "======================================================================"
echo "✨ DISTRIBUTION PACKAGES CREATED SUCCESSFULLY:"
echo "  📦 Tarball:  $DIST_DIR/$PKG_NAME.tar.gz"
if [ -f "$DIST_DIR/$PKG_NAME.zip" ]; then
    echo "  📦 Zip file: $DIST_DIR/$PKG_NAME.zip"
fi
echo ""
echo "HOW TO TRANSFER & RUN ON YOUR OTHER SYSTEM (CachyOS/Arch, Fedora, or Debian/Ubuntu):"
echo "  1. Copy '$DIST_DIR/$PKG_NAME.tar.gz' to the target machine (via USB, SCP, or Syncthing)."
echo "  2. Extract it:"
echo "     tar -xzf $PKG_NAME.tar.gz && cd $PKG_NAME"
echo "  3. Set up and launch:"
echo "     ./setup.sh && ./start_daemon.sh   (background, no terminal needed)"
echo "     ./setup.sh && ./start.sh          (foreground)"
echo "======================================================================"
