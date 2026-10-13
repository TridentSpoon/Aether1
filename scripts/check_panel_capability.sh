#!/usr/bin/env bash
# Every undockable panel's window must be covered by a Tauri capability.
#
# This is the exact shape of the bug this check was written after. A window whose label
# matches nothing in capabilities/default.json gets no core API at all, and nothing says so
# until a human opens it: the page loads, the HUD draws, and the first
# `window.__TAURI__.event.listen` throws `plugin:event:listen not allowed by ACL`. The
# AETHER CODE board shipped that way -- the panel opened, looked right, and answered every
# question with the ACL error, because asking is what listens for `code-chat-delta`. The
# build, clippy, the tests and every other check here passed on it.
#
# Derived, not declared: the panel list is read out of PANEL_WINDOWS at run time, so adding
# a panel that nobody granted the API fails here rather than in front of the operator.
set -euo pipefail
cd "$(dirname "$0")/.."

rust="src-tauri/src/main.rs"
caps="src-tauri/capabilities"

[ -f "$rust" ] || { echo "error: $rust is missing; this check cannot run."; exit 1; }
[ -d "$caps" ] || { echo "error: $caps is missing; this check cannot run."; exit 1; }

# The first string of each PANEL_WINDOWS row -- the panel id, which main.rs turns into the
# window label `panel-<id>`.
panels=$(awk '/^const PANEL_WINDOWS/{on=1; next} on && /^\];/{exit} on' "$rust" \
    | grep -oE '^\s+\("[a-z-]+"' \
    | grep -oE '"[a-z-]+"' \
    | tr -d '"' \
    | sort -u)

if [ -z "$panels" ]; then
    echo "error: found no panels in PANEL_WINDOWS -- has it been renamed or moved?"
    echo "       $rust is where this check reads the list of panel windows from."
    exit 1
fi

# Every window pattern granted by any capability file, one per line.
patterns=$(grep -hoE '"(main|sprite|panel[a-z*-]*)"' "$caps"/*.json | tr -d '"' | sort -u)

status=0
for panel in $panels; do
    label="panel-$panel"
    covered=0
    while read -r pattern; do
        [ -n "$pattern" ] || continue
        # shellcheck disable=SC2254 -- the pattern is a glob on purpose; Tauri treats it as one.
        case "$label" in $pattern) covered=1 ;; esac
    done <<<"$patterns"
    if [ "$covered" -eq 0 ]; then
        echo "error: the window \"$label\" is not covered by any capability in $caps."
        echo "       It will open, draw, and then fail on its first core API call with"
        echo "       \"plugin:event:listen not allowed by ACL\" -- in front of the operator."
        echo "       Add \"$label\" (or a glob covering it) to the capability's windows list."
        status=1
    fi
done

if [ "$status" -eq 0 ]; then
    echo "Panel windows: every undockable panel is covered by a capability."
fi
exit "$status"
