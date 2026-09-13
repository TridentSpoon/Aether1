#!/usr/bin/env bash
# Tailwind's `hidden` has to be stronger than every component rule that sets a display of
# its own, and weaker than the responsive utilities that are meant to override it. Those two
# requirements pull in opposite directions, and css/A1theme.css resolves them by hand -- see
# the ".hidden must actually hide" block at the end of that file.
#
# The hand-written half is the part that rots: markup gains a `hidden lg:flex` somewhere, no
# line exists for it, and that element is now hidden at every width instead of appearing on a
# wide window. Nothing throws; it is simply never seen. This finds the pairing in the markup
# and insists the stylesheet has an answer for it.

set -euo pipefail
cd "$(dirname "$0")/.."

THEME="frontend/css/A1theme.css"
status=0

# e.g. "hidden md:block", in either order within the same class attribute.
pairings=$(grep -ohE '\bhidden\b[^"]*\b(sm|md|lg|xl|2xl):(block|flex|grid|inline|inline-block|inline-flex|table|contents)\b|\b(sm|md|lg|xl|2xl):(block|flex|grid|inline|inline-block|inline-flex|table|contents)\b[^"]*\bhidden\b' frontend/*.html \
    | grep -ohE '\b(sm|md|lg|xl|2xl):(block|flex|grid|inline|inline-block|inline-flex|table|contents)\b' \
    | sort -u)

for utility in $pairings; do
    # .hidden.hidden.md\:block -- the escaped colon is how the class is written in CSS.
    escaped=".hidden.hidden.${utility/:/\\:}"
    if ! grep -qF "$escaped" "$THEME"; then
        echo "error: the markup uses \"hidden ${utility}\", but ${THEME} has no rule for it."
        echo "       Without one, that element is hidden at every width instead of appearing"
        echo "       on a wider window. Add, inside the matching @media block:"
        echo "           ${escaped} { display: ${utility#*:}; }"
        status=1
    fi
done

if [ "$status" -eq 0 ]; then
    echo "Every responsive \"hidden <breakpoint>:<display>\" pairing has a rule in ${THEME}."
fi
exit "$status"
