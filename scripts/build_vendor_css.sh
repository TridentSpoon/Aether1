#!/usr/bin/env bash
# Regenerates frontend/vendor/tailwind.css from the classes actually used in the frontend.
#
# Only needed when Tailwind classes are added to the HTML or JS and turn out to have no
# effect -- a static build contains exactly the classes the scanner found, where the old
# CDN build compiled anything on demand in the browser. Everything else in vendor/ is a
# plain copy of a published file and needs no build at all.
#
# Node and npm are required for this script alone; the app itself needs neither.
set -euo pipefail

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

TAILWIND_VERSION="3.4.17"
WORK="${TMPDIR:-/tmp}/aether1-tailwind-build"

if ! command -v npm >/dev/null 2>&1; then
    echo "npm is needed to rebuild the stylesheet (only for this script -- not to run Aether1)." >&2
    exit 1
fi

mkdir -p "$WORK"
(cd "$WORK" && npm install --silent --no-package-lock "tailwindcss@${TAILWIND_VERSION}")

"$WORK/node_modules/.bin/tailwindcss" \
    -c frontend/vendor/tailwind.config.js \
    -i frontend/vendor/tailwind.input.css \
    -o frontend/vendor/tailwind.css \
    --minify

echo "Wrote frontend/vendor/tailwind.css ($(wc -c < frontend/vendor/tailwind.css) bytes)"
