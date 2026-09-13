#!/usr/bin/env bash
# Every avatar file the frontend ships has to be named in the loader's list, and every file
# the list names has to exist.
#
# Avatar files are fetched on demand now (see frontend/js/hologram/avatar-loader.js), which
# means nothing in the page refers to them by name any more. A new avatar whose line was
# never added to that list is not a syntax error and not a missing file -- it is an avatar
# that silently never appears, and a picker button that does nothing when pressed. A line
# naming a file that was since renamed is the same failure from the other direction: a
# button that fetches a 404.
#
# Checked rather than generated: which avatars the app offers is a decision, not a
# side-effect of what happens to be in the folder.

set -euo pipefail
cd "$(dirname "$0")/.."

LOADER="frontend/js/hologram/avatar-loader.js"
DIR="frontend/js/hologram"

status=0

# avatar-template.js is documentation -- a working example to copy, deliberately not loaded.
for path in "$DIR"/avatar-*.js; do
    file="$(basename "$path")"
    [ "$file" = "avatar-template.js" ] && continue
    [ "$file" = "avatar-loader.js" ] && continue
    if ! grep -q "file: '${file}'" "$LOADER"; then
        echo "error: ${path} is not listed in ${LOADER}, so nothing can ever load it."
        echo "       Add a line to its AVATARS list, e.g. 'my-id': { file: '${file}', v: 1 }"
        status=1
    fi
done

while read -r file; do
    if [ ! -f "$DIR/$file" ]; then
        echo "error: ${LOADER} names ${file}, which does not exist in ${DIR}."
        status=1
    fi
done < <(grep -oE "file: '[^']+'" "$LOADER" | sed "s/file: '//; s/'//")

if [ "$status" -eq 0 ]; then
    echo "Every avatar file is listed in the loader, and every listed file exists."
fi
exit "$status"
