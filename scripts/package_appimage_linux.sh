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
#         <output-dir>/Aurora1-x86_64.AppImage.zsync
#
# The second file is what makes the first one updatable by anything other than AURORA1
# itself. An AppImage carries a string in an ELF section called `.upd_info` saying where its
# next version will be; an AppImage manager reads it and offers the update. Without it a
# manager shows "Update Strategy: None" and the file is a dead end -- which is what AURORA1
# shipped until now.
#
# Needs: the Linux build dependencies from the README (webkit2gtk, GTK), Node, for the
# Tauri CLI, which is run through npx rather than installed -- `cargo install tauri-cli`
# compiles it from source on every cold runner, and nothing else in this repo needs it --
# and `zsyncmake`, from the `zsync` package.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

OUT_DIR="${1:-dist}"

# Pinned, for the same reason the Piper and whisper.cpp versions in the offline script are:
# a bundler that changes under a release is a difference nobody chose.
TAURI_CLI_VERSION="${TAURI_CLI_VERSION:-2.9.2}"

# Where this AppImage should look for its own next version.
#
# `gh-releases-zsync` means "the newest release of this repository", so the string survives
# every future tag without being rebuilt -- which it has to, since it is baked into a file
# that is already on somebody's disk by the time the next release exists. The last field is
# the *published* name of the zsync, not the one the bundler happens to write.
#
# Read by `linuxdeploy-plugin-appimage`, which Tauri's bundler drives, and turned by it into
# `appimagetool -u`. Set here rather than in the workflow so that a build made by hand is
# the same artifact as a build made by CI; `GITHUB_REPOSITORY` only ever overrides the owner
# and name, for a fork that should point at itself rather than at this repository.
UPDATE_REPO="${AURORA1_UPDATE_REPO:-${GITHUB_REPOSITORY:-TridentSpoon/Aether1}}"
APPIMAGE_NAME="Aurora1-x86_64.AppImage"
export UPDATE_INFORMATION="gh-releases-zsync|${UPDATE_REPO%%/*}|${UPDATE_REPO##*/}|latest|${APPIMAGE_NAME}.zsync"

echo "======================================================================"
echo "📦 Packaging AURORA1 AppImage (Linux x86_64)"
echo "======================================================================"

if ! command -v npx >/dev/null 2>&1; then
    echo "npx was not found, and the Tauri CLI is run through it. Install Node 20 or newer." >&2
    exit 1
fi

# Checked here rather than where it is used, because the thing it would cost to find out
# late is a full Tauri release build.
if ! command -v zsyncmake >/dev/null 2>&1; then
    echo "zsyncmake was not found, and without it this AppImage cannot be updated by \
anything but AURORA1 itself." >&2
    echo "  Debian/Ubuntu: sudo apt-get install zsync      Fedora: sudo dnf install zsync" >&2
    echo "  To build one anyway, without update information:" >&2
    echo "    AURORA1_APPIMAGE_NO_UPDATE_INFO=1 $0 ${OUT_DIR}" >&2
    exit 1
fi

# `--bundles appimage` rather than a target list in tauri.conf.json: bundling stays off by
# default there, so `cargo build` and the other two packaging scripts are untouched by this
# one existing.
if [ -n "${AURORA1_APPIMAGE_NO_UPDATE_INFO:-}" ]; then
    # Said out loud. The difference between the two builds is invisible in the file listing
    # and decides whether anyone who installs this ever sees another version.
    echo "⚠ AURORA1_APPIMAGE_NO_UPDATE_INFO is set: this AppImage will not be updatable."
    unset UPDATE_INFORMATION
else
    echo "🔗 Update information: $UPDATE_INFORMATION"
fi

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
APPIMAGE="$OUT_DIR/$APPIMAGE_NAME"
cp "$BUILT" "$APPIMAGE"
chmod +x "$APPIMAGE"

if [ -n "${UPDATE_INFORMATION:-}" ]; then
    # Checked, not assumed. The bundler reads `UPDATE_INFORMATION` through two layers of
    # tooling, and a release that quietly stopped embedding it would look exactly like one
    # that did -- until somebody's manager showed "Update Strategy: None" again.
    if ! grep -aqF "$UPDATE_INFORMATION" "$APPIMAGE"; then
        echo "The AppImage carries no update information. UPDATE_INFORMATION was set, so \
the bundler ignored it: check whether this Tauri CLI still drives linuxdeploy." >&2
        exit 1
    fi

    # Made here from the finished file rather than kept from the bundler's own, and the two
    # are not interchangeable. A zsync names the file it describes, the bundler makes one
    # for `Aurora1_0.4.0_amd64.AppImage`, and the release publishes
    # `Aurora1-x86_64.AppImage` -- so the bundler's copy would send every updater looking
    # for a name that is not there.
    echo "🧩 Writing the zsync..."
    ( cd "$OUT_DIR" && zsyncmake -u "$APPIMAGE_NAME" -o "${APPIMAGE_NAME}.zsync" "$APPIMAGE_NAME" )
fi

echo ""
echo "======================================================================"
echo "✔ Wrote $APPIMAGE ($(du -h "$APPIMAGE" | cut -f1))"
if [ -f "$OUT_DIR/${APPIMAGE_NAME}.zsync" ]; then
    echo "✔ Wrote $OUT_DIR/${APPIMAGE_NAME}.zsync -- publish it beside the AppImage, or"
    echo "  every manager that reads the update information will look for it and find none."
fi
echo "  On the target machine: chmod +x $(basename "$APPIMAGE") && ./$(basename "$APPIMAGE")"
echo "  A machine with no FUSE: ./$(basename "$APPIMAGE") --appimage-extract-and-run"
echo "======================================================================"
