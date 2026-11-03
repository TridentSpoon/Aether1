#!/usr/bin/env bash
# Every process AETHER1 starts has to be built through paths::quiet_command, because on
# Windows the ones that are not flash a black console window over the HUD.
#
# AETHER1 is a windowed program (`windows_subsystem = "windows"` in main.rs), so Windows
# gives every console-mode child it spawns a brand new console of its own unless the parent
# passes CREATE_NO_WINDOW. That is one black rectangle per PowerShell probe, per git call,
# per speech synthesis -- and the watcher and theme probes run on timers and on focus, so
# what an operator sees is windows appearing over their work several times a minute.
# Aurora1 rev0.261101.276 shipped exactly that: the flag existed and twelve of the fifty
# call sites remembered to pass it.
#
# That is the shape of mistake a guard is for. A new `Command::new` is not wrong-looking in
# a diff, it cannot fail in CI (every job here is Linux), and on the machine that has the
# bug nothing says which of fifty call sites opened the window. So the rule is mechanical:
# nothing outside paths.rs builds a Command directly.
#
# Test code is exempt -- a test that spawns `sleep` is not something an operator ever sees --
# so each file is only checked down to its `#[cfg(test)]` line.

set -euo pipefail
cd "$(dirname "$0")/.."

status=0

while read -r file; do
    # paths.rs is where the quiet constructor lives; it is the one place allowed to say it.
    [ "$file" = "src-tauri/src/paths.rs" ] && continue
    cutoff="$(grep -n '^#\[cfg(test)\]' "$file" | head -1 | cut -d: -f1 || true)"
    if [ -n "$cutoff" ]; then
        body="$(head -n "$((cutoff - 1))" "$file")"
    else
        body="$(cat "$file")"
    fi
    hits="$(printf '%s\n' "$body" | grep -n 'Command::new(' || true)"
    if [ -n "$hits" ]; then
        while read -r hit; do
            echo "error: ${file}:${hit%%:*} builds a Command directly:"
            echo "       ${hit#*:}"
        done <<< "$hits"
        status=1
    fi
done < <(find src-tauri/src -name '*.rs' | sort)

if [ "$status" -ne 0 ]; then
    cat <<'MSG'

Use crate::paths::quiet_command("program") instead of Command::new("program").
It is the same Command with CREATE_NO_WINDOW already set, and a no-op off Windows.
MSG
else
    echo "ok: every spawned process is built through paths::quiet_command"
fi

exit "$status"
