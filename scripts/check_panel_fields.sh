#!/usr/bin/env bash
# Every field the Model Performance panel reads must be one the engine actually sends.
#
# This is the exact shape of the bug this check was written after. The panel read
# `tokens.used_percent`, `tokens.available_tokens` and `tokens.sparkline` for months after
# they stopped meaning anything, and nothing complained: JavaScript hands back `undefined`
# for a field that was never sent, and `undefined` rendered into a gauge width is a bar of
# zero width rather than an error. The panel showed a confident number derived from nothing
# at all, and the only way to find out was to look at it.
#
# Derived, not declared: the expected field list is read out of the Rust struct at run time,
# so adding a field to UsageSnapshot needs no change here, and removing one fails the build
# that still reads it.
set -euo pipefail
cd "$(dirname "$0")/.."

rust="src-tauri/src/llm/mod.rs"
js="frontend/js/app.js"

for f in "$rust" "$js"; do
    [ -f "$f" ] || { echo "error: $f is missing; this check cannot run."; exit 1; }
done

# The field names of `pub struct UsageSnapshot`, taken from the struct body itself: from the
# line declaring it to the closing brace in the first column-plus-nothing position.
sent=$(awk '/^pub struct UsageSnapshot \{/{on=1; next} on && /^\}/{exit} on' "$rust" \
    | grep -oE '^\s+pub [a-z_]+:' \
    | sed -E 's/^\s+pub ([a-z_]+):/\1/' \
    | sort -u)

if [ -z "$sent" ]; then
    echo "error: found no fields in UsageSnapshot -- has the struct been renamed?"
    echo "       $rust is where this check reads the list of legitimate fields from."
    exit 1
fi

# What the panel reads. `tokens` is the snapshot's name throughout app.js's telemetry code.
read_by_panel=$(grep -oE '\btokens\.[a-z_]+' "$js" | sed 's/^tokens\.//' | sort -u)

status=0
for field in $read_by_panel; do
    if ! grep -qx "$field" <<<"$sent"; then
        echo "error: $js reads tokens.$field, which UsageSnapshot does not send."
        echo "       It will be undefined at run time -- silently, and usually as a zero."
        echo "       Either add the field to UsageSnapshot in $rust, or stop reading it."
        status=1
    fi
done

if [ "$status" -eq 0 ]; then
    echo "Model Performance panel: every field it reads is one the engine sends."
fi
exit "$status"
