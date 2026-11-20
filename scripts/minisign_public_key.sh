#!/usr/bin/env bash
# Prints the bare base64 line of the minisign public key, whatever shape it was pasted in.
#
# `minisign.pub` is two lines -- an "untrusted comment:" line and the key itself -- and
# `minisign -V -P` takes only the second. Pasting the whole file into the
# AETHER1_MINISIGN_PUBLIC_KEY variable is the obvious mistake to make, and what it produces
# is `base64 conversion failed - was an actual public key given?` at the end of a release
# run, plus binaries whose compiled-in key is a comment. Both halves of the pipeline read
# the key through here so that either shape works and neither has its own idea of what a
# key looks like.
#
# Reads MINISIGN_PUB, or AETHER1_MINISIGN_PUBLIC_KEY, in that order. Prints nothing and
# succeeds when neither is set: "no keypair configured" is a state this repository supports,
# and it is the caller that decides what to do about it.
set -euo pipefail

raw="${MINISIGN_PUB:-${AETHER1_MINISIGN_PUBLIC_KEY:-}}"
[ -n "$raw" ] || exit 0

# The last line that is neither blank nor a comment. The last, not the first: a file read in
# order ends on the key, and anything above it is the comment minisign writes there.
line="$(printf '%s\n' "$raw" \
  | tr -d '\r' \
  | grep -v '^[[:space:]]*$' \
  | grep -v '^[[:space:]]*untrusted comment:' \
  | tail -n 1 \
  | tr -d '[:space:]')"

if [ -z "$line" ]; then
  echo "AETHER1_MINISIGN_PUBLIC_KEY is set but holds no key line" >&2
  exit 1
fi

printf '%s\n' "$line"
