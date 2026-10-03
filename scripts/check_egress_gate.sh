#!/usr/bin/env bash
# Local-only mode is a promise, and this is what keeps it one.
#
# src/local_only.rs is the operator's switch: with it on, nothing in Aether1 leaves this
# machine. Before src/net.rs existed, that held because nineteen separate places each
# remembered to ask before reaching out -- a property of the people who wrote those
# places, not of the program. It decays in one direction: the next subsystem is the one
# that forgets, and when it forgets, traffic leaves a machine whose operator was told it
# would not. Nothing about that looks wrong in a diff.
#
# So the promise is checked here instead of remembered. Two properties make it true, and
# both are the kind a well-meaning refactor breaks silently -- a `ureq::get` added to a new
# module, a second HTTP crate pulled in for one convenient API, an entry point that forgets
# to load the policy.
#
#   1. There is one way to build a request.   Only src/net.rs may name ureq's request
#                                             verbs, so every outbound request in the
#                                             program is built by net::get or net::post,
#                                             both of which refuse first.
#   2. There is one HTTP client.              A second one (reqwest, hyper's client,
#                                             isahc, curl bindings) would be a second way
#                                             out that property 1 says nothing about.
#   3. The policy is actually loaded.         net::install has to be called on the real
#                                             settings database, or the gate refuses
#                                             everything and property 1 holds for the
#                                             worst possible reason.
#
# The local network is deliberately not what this protects. Reaching an Ollama server on
# loopback or on the LAN is the whole point of the platform and local-only mode has always
# allowed it; net::require_online makes that distinction by endpoint. This script is about
# there being one place that decision is made, not about what the decision is.
set -euo pipefail

cd "$(dirname "$0")/.."

status=0

# The gate itself. Nothing else may name ureq's request verbs.
gate='src-tauri/src/net.rs'

# ureq 3's request constructors. `ureq::Agent` is here too: an agent is a client with its
# own config and its own `get`/`post`, so one built outside the gate is a way around it.
verbs='ureq::(get|post|put|patch|delete|head|request|Agent)'

if [ ! -f "$gate" ]; then
    echo "FAIL: $gate is missing -- there is no egress gate to enforce."
    exit 1
fi

# Every .rs file but the gate, including test modules: a test that builds its own request
# is a line somebody will copy into real code, and an exception here is an exception a
# future reader has to know about.
offenders=$(grep -REln "$verbs" src-tauri/src --include='*.rs' | grep -v "^${gate}$" || true)
if [ -n "$offenders" ]; then
    echo "FAIL: an outbound request is built outside the egress gate."
    echo "      Every request goes through net::get or net::post, which ask the operator's"
    echo "      local-only setting first. A bare ureq call does not, and with the mode on it"
    echo "      would send traffic off a machine whose operator was told nothing would."
    echo "      Replace it with crate::net::get(url, \"what did not happen\")? and keep the"
    echo "      refusal written from the operator's side of the screen."
    for file in $offenders; do
        grep -REn "$verbs" "$file" | sed "s|^|      $file:|"
    done
    status=1
fi

# A second HTTP client is a second door, and property 1 above would still pass with one in
# the tree. Checked against the manifest rather than the source so it fails when the
# dependency is added, not later when somebody first calls it.
others='^(reqwest|isahc|curl|attohttpc|surf|hyper-tls|hyper-util)\b'
if grep -REn "$others" src-tauri/Cargo.toml >/dev/null 2>&1; then
    echo "FAIL: a second HTTP client is in src-tauri/Cargo.toml."
    echo "      The egress gate covers ureq. Another client is another way off this machine"
    echo "      that local-only mode knows nothing about. Route it through src/net.rs and"
    echo "      extend the verb list above, or drop it."
    grep -REn "$others" src-tauri/Cargo.toml | sed 's/^/      /'
    status=1
fi

# The gate needs the settings database, and build_llm_engine is the one place every entry
# point -- the native app, --serve, and each CLI subcommand -- gets its engine. If that
# call goes, the gate refuses everything and the check above passes vacuously.
# Comment lines are stripped first: the doc comment above build_llm_engine says the words
# "net::install", and a check that prose can satisfy is not a check.
#
# `grep -c` rather than `grep -q`, because -q closes the pipe on its first match and the
# SIGPIPE that gives sed fails the whole pipeline under `pipefail` -- which would report
# this as missing on a tree where it is present. check_terminal_isolation.sh avoids the
# same trap by redirecting instead of using -q.
installs=$(sed 's|//.*||' src-tauri/src/main.rs | grep -cE 'net::install\(' || true)
if [ "$installs" -eq 0 ]; then
    echo "FAIL: main.rs never calls net::install, so no entry point loads the egress policy."
    echo "      The gate then refuses every non-local request and the rest of this script"
    echo "      passes for the worst possible reason. Install it in build_llm_engine."
    status=1
fi

# And the gate has to actually consult the operator's setting rather than having been
# hollowed out into a pass-through. Comments stripped for the same reason as above: both of
# these names appear in the module's own prose, which must not be what satisfies the check.
code=$(sed 's|//.*||' "$gate")
if ! printf '%s' "$code" | grep -E 'local_only::enabled' >/dev/null 2>&1; then
    echo "FAIL: $gate does not read local_only::enabled, so it is not a gate."
    status=1
fi
if ! printf '%s' "$code" | grep -E 'local_only::is_local_endpoint' >/dev/null 2>&1; then
    echo "FAIL: $gate does not call local_only::is_local_endpoint."
    echo "      Without it the gate cannot tell the local engine from the internet, and"
    echo "      local-only mode would refuse the loopback and LAN traffic it exists to"
    echo "      allow -- which breaks the product rather than protecting it."
    status=1
fi

if [ "$status" -eq 0 ]; then
    echo "One door out: every outbound request is built by src/net.rs, which asks the operator first."
fi

exit "$status"
