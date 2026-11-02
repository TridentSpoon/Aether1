#!/usr/bin/env bash
# The content security policy is the last thing standing between a stray string and the
# whole program, and this is what keeps it standing.
#
# Escaping text before it reaches the DOM is the real defence. The policy is what makes the
# next slip in that escaping survivable: markup that gets into a page cannot run a script,
# and a script in this window reaches every #[tauri::command] there is -- the consent model,
# the network policy, the device list. docs/SECURITY_AUDIT_S4.md has the two places where
# text from off the machine was already being rendered as HTML, which is why this is not
# hypothetical.
#
# Three properties hold it up, and all three are the kind a reasonable change breaks without
# looking wrong in a diff:
#
#   1. No inline script or style.    `script-src 'self'` has no inline exception, so an
#                                    inline <script> or <style> added to a page would simply
#                                    stop running -- in the browser immediately, and in the
#                                    desktop shell only until someone touched the file,
#                                    because Tauri hashes inline scripts at build time and
#                                    the HTTP server does not. That difference is the trap:
#                                    the desktop shell would keep working and the LAN browser
#                                    would not, which is the hardest kind of break to find.
#   2. The two copies agree.         The policy is set twice, because the frontend is served
#                                    twice: tauri.conf.json covers the asset protocol and
#                                    server.rs covers the HTTP server. A policy on one of
#                                    them is a policy on neither in practice.
#   3. Nothing is unsafe.            'unsafe-inline' or 'unsafe-eval' anywhere in it gives
#                                    back exactly what it was set up to take away.
#
# Run from anywhere; paths are resolved from this script's own location.
set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
conf="$root/src-tauri/tauri.conf.json"
server="$root/src-tauri/src/server.rs"
status=0

fail() {
    printf '%s\n' "$1" >&2
    status=1
}

# ---------------------------------------------------------------- 1. no inline blocks
# `<script>` with no attributes, and `<style` in any form. A <script src=...> is fine: that
# is a file, which is what 'self' allows.
for page in "$root"/frontend/*.html; do
    name="${page##*/}"
    if grep -nE '<script[[:space:]]*>' "$page" >/dev/null; then
        fail "$name has an inline <script> block, which 'script-src self' will not run:"
        grep -nE '<script[[:space:]]*>' "$page" | sed 's/^/    /' >&2
        fail "    Move the code into frontend/js/ and load it with <script src=...>."
    fi
    if grep -nE '<style' "$page" >/dev/null; then
        fail "$name has a <style> block, which 'style-src self' will not apply:"
        grep -nE '<style' "$page" | sed 's/^/    /' >&2
        fail "    Move the rules into frontend/css/ and link them with <link rel=stylesheet>."
    fi
    # A `style="..."` attribute is inline CSS, and `style-src 'self'` blocks it exactly as it
    # blocks a <style> block -- the browser logs a violation and drops the declaration, so the
    # element renders without it. Caught the hard way: a progress bar was given
    # `style="width:0%"` as its starting state and the console carried a CSP violation from
    # the moment the page loaded. Writing the same property from JavaScript is fine and is
    # what the eight other bars in this app already do -- CSSOM is not inline CSS.
    if grep -noE '[[:space:]]style=' "$page" >/dev/null; then
        fail "$name has an inline style attribute, which 'style-src self' will not apply:"
        grep -noE '[[:space:]]style=' "$page" | sed 's/^/    /' >&2
        fail "    Put it in a class in frontend/css/, or set it from JavaScript (element.style.x = ...)."
    fi
    # An inline handler attribute is script too, and there has never been one here.
    if grep -noE '[[:space:]]on(click|change|input|load|error|submit|mouseover|keydown)=' "$page" >/dev/null; then
        fail "$name has an inline event-handler attribute, which the policy will not run:"
        grep -noE '[[:space:]]on(click|change|input|load|error|submit|mouseover|keydown)=' "$page" | sed 's/^/    /' >&2
        fail "    Attach it from JavaScript instead (element.onclick = ... is fine)."
    fi
done

# ---------------------------------------------------------------- 2. the two copies agree
# Pulled out of each file as text rather than parsed: the Rust side is a line-continued
# string literal, so the comparison is made on the policy the two files mean, with runs of
# whitespace flattened and nothing else touched.
flatten() { tr '\n' ' ' | tr -s ' \t' ' ' | sed 's/^ //; s/ $//'; }

from_conf="$(python3 - "$conf" <<'PY' | flatten
import json, sys
with open(sys.argv[1]) as f:
    print(json.load(f)["app"]["security"].get("csp", ""))
PY
)"

from_rust="$(python3 - "$server" <<'PY' | flatten
import re, sys
source = open(sys.argv[1]).read()
match = re.search(r'pub const CONTENT_SECURITY_POLICY: &str = "(.*?)";\n', source, re.S)
if not match:
    print("")
    raise SystemExit
# Rust's line continuation: a trailing backslash eats the newline and the indentation after it.
print(re.sub(r'\\\s*\n\s*', '', match.group(1)))
PY
)"

if [ -z "$from_conf" ]; then
    fail "tauri.conf.json has no app.security.csp. The desktop shell would serve no policy."
fi
if [ -z "$from_rust" ]; then
    fail "server.rs has no CONTENT_SECURITY_POLICY. The HTTP server would serve no policy."
fi
if [ -n "$from_conf" ] && [ -n "$from_rust" ] && [ "$from_conf" != "$from_rust" ]; then
    fail "the two policies have drifted apart:"
    fail "    tauri.conf.json: $from_conf"
    fail "    server.rs:       $from_rust"
fi

# ---------------------------------------------------------------- 3. nothing unsafe
for policy in "$from_conf" "$from_rust"; do
    case "$policy" in
        *unsafe-inline*) fail "the policy contains 'unsafe-inline', which gives back what it exists to take away." ;;
        *unsafe-eval*)   fail "the policy contains 'unsafe-eval', which gives back what it exists to take away." ;;
    esac
done

# The header has to actually be attached, not merely defined.
if ! grep -q "with_content_security_policy" "$server"; then
    fail "server.rs defines a policy but nothing applies it to the responses."
fi

if [ "$status" -eq 0 ]; then
    echo "Content security policy: one policy, set on both paths, with no inline exception."
fi
exit "$status"
