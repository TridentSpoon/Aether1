#!/usr/bin/env bash
# Builds a single-file AppImage: the app, the webview and the GTK libraries it links
# against, in one executable that runs on any x86_64 Linux without installing anything.
#
# This is the third Linux shape, and the one to hand somebody who just wants to try it:
#
#   package_offline_linux.sh  -- tar.gz + installer, Piper and whisper.cpp and their models
#                                inside it, so it installs on a machine with no network at
#                                all. Around half a gigabyte.
#   package_slim_linux.sh     -- tar.gz + installer, binary only; the installer pip-installs
#                                the voice stack on the target machine.
#   this script               -- one file, no installer, no voice stack. Double-click it.
#
# What it does not contain is as much the point as what it does. There is no Piper, no
# whisper.cpp and no models: an AppImage carrying those would be the size of the offline
# bundle while still being a file you cannot update in place, which is the worst of both.
# Voice input and output are therefore unavailable in this build until the operator installs
# them, exactly as in the slim bundle, and everything else works.
#
# The bundling is Tauri's own `--bundles appimage`, not a hand-built AppDir. That matters:
# the hard part of an AppImage is collecting webkit2gtk and the forty-odd libraries under
# it, and Tauri drives linuxdeploy to do that. A hand-rolled AppDir that merely looks right
# runs on the machine that built it and fails on somebody else's distro, which is the one
# failure mode this format exists to prevent.
#
# Usage: ./scripts/package_appimage_linux.sh [output-dir]
#
# Output: <output-dir>/Aurora1-x86_64.AppImage
#
# Needs: the Linux build dependencies from the README (webkit2gtk, GTK) and Node, for the
# Tauri CLI, which is run through npx rather than installed -- `cargo install tauri-cli`
# compiles it from source on every cold runner, and nothing else in this repo needs it.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

OUT_DIR="${1:-dist}"

# Pinned, for the same reason the Piper and whisper.cpp versions in the offline script are:
# a bundler that changes under a release is a difference nobody chose.
TAURI_CLI_VERSION="${TAURI_CLI_VERSION:-2.9.2}"

echo "======================================================================"
echo "📦 Packaging AURORA1 AppImage (Linux x86_64)"
echo "======================================================================"

if ! command -v npx >/dev/null 2>&1; then
    echo "npx was not found, and the Tauri CLI is run through it. Install Node 20 or newer." >&2
    exit 1
fi

# `--bundles appimage` rather than a target list in tauri.conf.json: bundling stays off by
# default there, so `cargo build` and the other two packaging scripts are untouched by this
# one existing.
echo "🦀 Building and bundling (this compiles the release binary first)..."
npx -y "@tauri-apps/cli@${TAURI_CLI_VERSION}" build --bundles appimage

BUILT="$(find src-tauri/target/release/bundle/appimage -maxdepth 1 -name '*.AppImage' -print -quit)"
if [ -z "$BUILT" ]; then
    echo "The bundler reported success but produced no .AppImage." >&2
    exit 1
fi

# Renamed on the way out. Tauri names it from the version in tauri.conf.json
# (Aurora1_0.4.0_amd64.AppImage), which is not the version AURORA1 reports about itself --
# that is the `rev0.yymmdd.PR` scheme in build.rs, and a file named for the other one would
# be read as the release's version by whoever downloads it. The release's own name comes
# from its tag, so the asset carries no number at all.
mkdir -p "$OUT_DIR"
APPIMAGE="$OUT_DIR/Aurora1-x86_64.AppImage"
cp "$BUILT" "$APPIMAGE"
chmod +x "$APPIMAGE"

echo ""
echo "======================================================================"
echo "✔ Wrote $APPIMAGE ($(du -h "$APPIMAGE" | cut -f1))"
echo "  On the target machine: chmod +x $(basename "$APPIMAGE") && ./$(basename "$APPIMAGE")"
echo "  A machine with no FUSE: ./$(basename "$APPIMAGE") --appimage-extract-and-run"
echo "======================================================================"
