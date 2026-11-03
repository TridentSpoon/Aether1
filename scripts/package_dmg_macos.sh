#!/usr/bin/env bash
# Builds the macOS disk image: Aurora1.app inside a .dmg, for one processor at a time.
#
# Usage: ./scripts/package_dmg_macos.sh [output-dir] [rust-target]
#
#   rust-target defaults to this machine's own. The two that are released are
#   aarch64-apple-darwin (Apple Silicon) and x86_64-apple-darwin (Intel), and both can be
#   built from an Apple Silicon machine -- `rustup target add x86_64-apple-darwin` is all
#   the second one needs, because the SDK carries both architectures.
#
# Output: <output-dir>/Aurora1-aarch64.dmg or <output-dir>/Aurora1-x86_64.dmg
#
# Two warnings that belong on the download page rather than in a comment, but are worth
# knowing here:
#
#   * **The app is not notarized and not signed**, because that needs a paid Apple Developer
#     account. macOS will refuse to open it on a double-click; the way in is right-click ->
#     Open, once, or `xattr -dr com.apple.quarantine /Applications/Aurora1.app`. Same shape
#     as the unsigned Windows installer, and the same fix: a certificate, when there is one.
#   * **There is no voice stack in here.** Piper and whisper.cpp are fetched per platform by
#     the two Linux bundles and the Windows installer, and nothing has ever fetched the macOS
#     builds of them. Everything else works; speech does not, and says so.
#
# Nothing in AURORA1 has been run on a Mac. The sandbox for AETHER CODE is bubblewrap, which
# is Linux-only, so a command there refuses itself with a message the same way it does on
# Windows -- see docs/SECURITY_MODEL.md. This script producing a disk image is not a claim
# that the app has been exercised on that platform.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

OUT_DIR="${1:-dist}"
TARGET="${2:-}"

TAURI_CLI_VERSION="${TAURI_CLI_VERSION:-2.9.2}"

if [ "$(uname -s)" != "Darwin" ]; then
    echo "A .dmg can only be built on macOS: the bundler uses hdiutil, and the app links" >&2
    echo "against WKWebView. Run this on a Mac, or let release.yml's macos-dmg job do it." >&2
    exit 1
fi

if ! command -v npx >/dev/null 2>&1; then
    echo "npx was not found, and the Tauri CLI is run through it. Install Node 20 or newer." >&2
    exit 1
fi

echo "======================================================================"
echo "📦 Packaging AURORA1 disk image (macOS${TARGET:+ / $TARGET})"
echo "======================================================================"

ARGS=(build --bundles dmg)
if [ -n "$TARGET" ]; then
    rustup target add "$TARGET"
    ARGS+=(--target "$TARGET")
fi

npx -y "@tauri-apps/cli@${TAURI_CLI_VERSION}" "${ARGS[@]}"

# `--target` puts the output under target/<triple>/release; without it, under target/release.
BUNDLE_DIR="src-tauri/target/${TARGET:+$TARGET/}release/bundle/dmg"
BUILT="$(find "$BUNDLE_DIR" -maxdepth 1 -name '*.dmg' -print -quit 2>/dev/null || true)"
if [ -z "$BUILT" ]; then
    echo "The bundler reported success but produced no .dmg in $BUNDLE_DIR." >&2
    exit 1
fi

# Named by processor rather than by the version in tauri.conf.json, for the same reason as
# the AppImage: the version AURORA1 reports about itself is the `rev0.yymmdd.PR` scheme from
# build.rs, and a file carrying the other number would be read as the release's version.
# The architecture is in the name because this is the one asset that ships twice.
case "${TARGET:-$(uname -m)}" in
    aarch64* | arm64) ARCH="aarch64" ;;
    x86_64*) ARCH="x86_64" ;;
    *)
        echo "Unknown architecture ${TARGET:-$(uname -m)}; the two released images are" >&2
        echo "aarch64-apple-darwin and x86_64-apple-darwin." >&2
        exit 1
        ;;
esac

mkdir -p "$OUT_DIR"
DMG="$OUT_DIR/Aurora1-$ARCH.dmg"
cp "$BUILT" "$DMG"

echo ""
echo "======================================================================"
echo "✔ Wrote $DMG ($(du -h "$DMG" | cut -f1))"
echo "  It is unsigned, so the first launch is right-click -> Open, not a double-click."
echo "======================================================================"
