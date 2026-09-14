#!/usr/bin/env bash
# Regenerates frontend/vendor/three.min.js as a bundle of only the parts of three.js
# this project actually uses.
#
# The published three.js build is one file containing the whole library -- every geometry,
# every material, every loader, every post-processing pass. Aether1 uses 49 of those things.
# This script reads which 49 by scanning the source for "THREE.Something", writes an entry
# file that re-exports exactly those, and lets esbuild discard everything unreachable from
# them.
#
# The symbol list is DERIVED, never written down here. A new THREE.Something in an avatar
# file is picked up by the next build with nothing to remember -- which is the only version
# of this that survives contact with a year of edits. (scripts/check_vendor_three.sh fails
# the build if the committed bundle is missing a symbol the source uses, so a forgotten
# rebuild is caught rather than shipped.)
#
# The revision is pinned and is the same one that was vendored by hand before this script
# existed. three changed its colour handling in r152 in a way that visibly alters what is
# already on screen, so moving off r128 is a deliberate decision about how the avatar looks,
# not a routine dependency bump. Do not bump it casually.
#
# Node and npm are required for this script alone; the app itself needs neither.
set -euo pipefail

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

THREE_VERSION="0.128.0"
ESBUILD_VERSION="0.25.0"
WORK="${TMPDIR:-/tmp}/aether1-three-build"
OUT="frontend/vendor/three.min.js"

if ! command -v npm >/dev/null 2>&1; then
    echo "npm is needed to rebuild three.js (only for this script -- not to run Aether1)." >&2
    exit 1
fi

mkdir -p "$WORK"
echo '{"name":"aether1-three-build","private":true}' > "$WORK/package.json"
(cd "$WORK" && npm install --silent --no-package-lock \
    "three@${THREE_VERSION}" "esbuild@${ESBUILD_VERSION}")

# Every THREE.Something named anywhere in the frontend, deduplicated.
SYMBOLS=$(grep -rhoE "THREE\.[A-Za-z0-9_]+" frontend/js frontend/*.html \
    | sed 's/THREE\.//' | sort -u | paste -sd, -)

if [ -z "$SYMBOLS" ]; then
    echo "Found no THREE.* uses -- refusing to write an empty bundle." >&2
    exit 1
fi

# Bundled from src/ rather than build/three.module.js on purpose: the published build is a
# single module, so nothing inside it can be dropped. src/ is the real module graph, which
# is what makes the unused two thirds of the library reachable for removal.
printf 'export { %s } from "three/src/Three.js";\n' "$SYMBOLS" > "$WORK/entry.js"

"$WORK/node_modules/.bin/esbuild" "$WORK/entry.js" \
    --bundle \
    --minify \
    --format=iife \
    --global-name=THREE \
    --legal-comments=none \
    --outfile="$OUT" \
    --log-level=warning

# The licence notice the minifier was told to drop. three is MIT; the notice has to ship.
printf '/*! three.js r%s | (c) 2010-2021 three.js authors | MIT | https://threejs.org
 * Bundled by scripts/build_vendor_three.sh -- only the parts Aether1 uses. */\n' \
    "$(echo "${THREE_VERSION}" | cut -d. -f2)" | cat - "$OUT" > "$OUT.tmp" && mv "$OUT.tmp" "$OUT"

echo "Wrote $OUT ($(wc -c < "$OUT") bytes) with $(echo "$SYMBOLS" | tr ',' '\n' | wc -l) exports"
