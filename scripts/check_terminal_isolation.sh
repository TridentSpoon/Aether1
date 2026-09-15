#!/usr/bin/env bash
# The terminal is the operator's alone, and this is what keeps it that way.
#
# src-tauri/src/terminal.rs runs a real shell on a real pty. That is safe to ship for
# exactly one reason: nothing but the native desktop window can reach it. Three properties
# make that true, and all three are the kind that a well-meaning refactor breaks silently --
# a `use crate::terminal` added to the wrong file, a convenience wrapper moved into the
# shared command layer, a tool added "just to read the scrollback". None of those look wrong
# in a diff. So they are checked here instead of remembered.
#
#   1. The companion cannot reach it.   Nothing in tools/ may mention it, so there is no
#                                       name the model can emit that arrives at a shell.
#   2. The network cannot reach it.     server.rs may not mention it, so --serve and --lan
#                                       expose nothing here.
#   3. Both transports cannot reach it. commands.rs is the shared layer; anything put there
#                                       is reachable from server.rs by construction.
#
# The terminal's commands therefore live only in main.rs, which `--serve` returns from
# before ever constructing, and the isolation is a fact about the process rather than a
# check that has to hold at runtime.
set -euo pipefail

cd "$(dirname "$0")/.."

status=0

# Word-boundary matched so a file merely containing the word "terminal" in prose (an error
# message, a comment about terminal output) is not a failure -- what is forbidden is
# reaching the module.
forbidden='(crate::terminal|use +terminal|terminal::(open|write|resize|close|count|Terminals|Handle))'

check_clean() {
    local path="$1" why="$2"
    if grep -REn "$forbidden" "$path" >/dev/null 2>&1; then
        echo "FAIL: $path reaches src/terminal.rs."
        echo "      $why"
        grep -REn "$forbidden" "$path" | sed 's/^/      /'
        status=1
    fi
}

check_clean src-tauri/src/tools \
    "A tool is a name the model can emit. The terminal must have no such name."
check_clean src-tauri/src/server.rs \
    "server.rs is the HTTP surface; with --lan that is the network. The terminal is not on it."
check_clean src-tauri/src/commands.rs \
    "commands.rs is shared with server.rs, so anything here is reachable over HTTP too."

# The module has to actually be wired into the native app, or the checks above pass
# vacuously for the worst possible reason.
if ! grep -q "terminal_open_rust" src-tauri/src/main.rs; then
    echo "FAIL: main.rs does not register the terminal commands -- the isolation above is vacuous."
    status=1
fi

# The other half of "nothing is stored": output that reached the database or the vault would
# be readable by the tools that already read those, which is the boundary going around the
# back. terminal.rs must not know what a database is.
if grep -REn '(MemoryDb|db\(\)|add_message|log_action|vault::)' src-tauri/src/terminal.rs >/dev/null 2>&1; then
    echo "FAIL: src/terminal.rs touches persistent storage."
    echo "      Terminal output that lands in the database or the vault is readable by the"
    echo "      tools that read those, which defeats the whole isolation."
    grep -REn '(MemoryDb|db\(\)|add_message|log_action|vault::)' src-tauri/src/terminal.rs | sed 's/^/      /'
    status=1
fi

if [ "$status" -eq 0 ]; then
    echo "The terminal is reachable from the native window and nowhere else: no tool, no route, no store."
fi

exit "$status"
