#!/usr/bin/env bash
# Every Tailwind text colour used in the HUD has to be remapped to a theme variable.
#
# Tailwind's palette is hard-coded: `text-slate-400` is rgb(148, 163, 184) whatever theme is
# on, and it was picked to sit on a dark ground. On Solar's near-white page that same grey
# measures around 2:1, which is text you can see is there and cannot read. A1theme.css
# therefore remaps every one of these classes onto --text-main, --text-dim, --text-accent and
# friends, which follow the theme.
#
# A class added to the markup and forgotten here is invisible in the dark themes the app is
# usually developed in, and only shows up when somebody on a light theme tries to read it.
# So the list is not written down anywhere: it is read out of the markup every time this
# runs, and checked against the stylesheet.
set -euo pipefail

cd "$(dirname "$0")/.."

css=frontend/css/A1theme.css
status=0

# Every text-<colour>-<shade> in the pages, optionally with an /<alpha> suffix.
used=$(grep -oh 'text-[a-z]\+-[0-9]\{2,3\}\(/[0-9]\+\)\?' frontend/*.html | sort -u)

for cls in $used; do
    # The stylesheet escapes the slash in a selector: text-cyan-400/80 is .text-cyan-400\/80.
    selector=".${cls//\//\\/}"
    if ! grep -qF "$selector" "$css"; then
        echo "error: $cls is used in the markup but never remapped in $css."
        echo "       Add it to one of the [data-theme] :is(...) groups there -- --text-main"
        echo "       for body copy, --text-dim for labels and captions, --text-accent or"
        echo "       --text-highlight for headings."
        status=1
    fi
done

if [ "$status" -eq 0 ]; then
    echo "Every Tailwind text colour in the markup is remapped to a theme variable."
fi

exit $status
