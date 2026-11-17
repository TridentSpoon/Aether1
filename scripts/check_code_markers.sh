#!/usr/bin/env bash
# The markers that decide a model was built for code live in two places: hf::CODE_MARKERS
# in Rust (which code_setup::is_coding_model now calls through to, so the hub and the
# coding panel cannot disagree) and HUB_CODE_MARKERS in the browser, which answers the
# same question about a name the server reported with no round trip to hang a list on.
#
# They cannot be one list across that boundary, so this fails the build when they drift.
set -euo pipefail
cd "$(dirname "$0")/.."

markers_from() {           # a quoted-string list out of a named const/array
    sed -n "/$1/,/];/p" "$2" \
        | grep -oE "[\"'][a-z0-9_.-]+[\"']" \
        | tr -d "\"'" \
        | sort -u
}

rust=$(markers_from 'const CODE_MARKERS' src-tauri/src/hf.rs)
js=$(markers_from 'const HUB_CODE_MARKERS' frontend/js/app.js)

if [ "$rust" != "$js" ]; then
    echo "check_code_markers: hf::CODE_MARKERS and HUB_CODE_MARKERS disagree." >&2
    diff <(echo "$rust") <(echo "$js") >&2 || true
    exit 1
fi

# There is no third list to check: code_setup::is_coding_model now calls hf::purpose, and
# hf.rs's own cross-check test proves the two agree on real repository names.

echo "check_code_markers: ok ($(echo "$rust" | wc -l) markers)"
