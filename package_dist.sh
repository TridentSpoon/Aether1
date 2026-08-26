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

# Copy source directories and files
echo "📂 Copying project components..."
cp -r backend "$TEMP_BUILD/"
cp -r desktop "$TEMP_BUILD/"
cp -r frontend "$TEMP_BUILD/"
cp setup.sh start.sh start_daemon.sh stop.sh start.bat README.md Dockerfile docker-compose.yml "$TEMP_BUILD/"

# Remove Python bytecode and audio cache from bundle to keep package light
find "$TEMP_BUILD" -type d -name "__pycache__" -exec rm -rf {} + 2>/dev/null || true
find "$TEMP_BUILD" -type d -name "*.pyc" -exec rm -rf {} + 2>/dev/null || true
rm -rf "$TEMP_BUILD/backend/audio_cache/*" 2>/dev/null || true

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
