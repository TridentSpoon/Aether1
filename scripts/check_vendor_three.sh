#!/usr/bin/env bash
# Fails if frontend/vendor/three.min.js is missing a THREE.Something the frontend uses.
#
# The bundle contains only the parts of three.js the source asked for at build time, so a
# new THREE.Something added to an avatar file without re-running build_vendor_three.sh is
# not a missing feature -- it is an avatar that throws the moment somebody picks it, which
# is the kind of thing that ships because it only breaks on the one avatar nobody tested.
#
# Needs no npm: it reads the committed bundle as text.
set -euo pipefail

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

BUNDLE="frontend/vendor/three.min.js"
[ -f "$BUNDLE" ] || { echo "$BUNDLE is missing. Run scripts/build_vendor_three.sh." >&2; exit 1; }

# esbuild writes the export table as `Name: () => Symbol` (or `Name: () => Symbol,`), so a
# symbol the bundle really exports appears with that arrow. Matching the name alone would
# pass on any coincidental mention inside the library's own source.
missing=()
while read -r sym; do
    [ -z "$sym" ] && continue
    grep -qE "\b${sym}: *\(\) *=>" "$BUNDLE" || missing+=("$sym")
done < <(grep -rhoE "THREE\.[A-Za-z0-9_]+" frontend/js frontend/*.html | sed 's/THREE\.//' | sort -u)

if [ ${#missing[@]} -gt 0 ]; then
    echo "frontend/vendor/three.min.js does not export: ${missing[*]}" >&2
    echo "The frontend uses these but the committed bundle was built without them." >&2
    echo "Run ./scripts/build_vendor_three.sh and commit the result." >&2
    exit 1
fi

echo "three.js bundle exports every THREE.* the frontend uses."
