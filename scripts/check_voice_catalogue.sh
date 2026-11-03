#!/usr/bin/env bash
# Every voice Aurora1 offers has to be a voice that exists.
#
# src/voice_download.rs carries a fixed table of Piper voices: a name, a label, a size and
# the folder the two files sit in. Being fixed is what makes the download path safe -- no
# name the page sends ever becomes part of a URL -- but it also means the table is a claim
# about somebody else's file server, and nothing in the crate can check a claim like that.
#
# It was wrong. `en_US-kathleen-medium` sat in the table while the Piper collection only
# publishes kathleen at `low`, so pressing Download produced a progress bar that ended in
# "Could not fetch the voice: http status 404". There is no way to reach that from a unit
# test, and no way to see it by reading the table either: the entry is perfectly
# well-formed, the folder matches the name, and every voice around it is real.
#
# So this asks. The Piper project publishes an index of every voice it has, with the files
# each one consists of, and a sample clip per voice beside it; both are checked here,
# because the picker now offers to play that sample and a preview button that 404s is the
# same failure wearing a different hat.
#
# **It skips rather than fails when there is no network.** A guard that goes red on an
# aeroplane teaches people to stop running the guards. CI has a network, which is where
# this is meant to bite.

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
table="$root/src-tauri/src/voice_download.rs"
index_url="https://raw.githubusercontent.com/rhasspy/piper-samples/master/voices.json"
sample_base="https://raw.githubusercontent.com/rhasspy/piper-samples/master/samples"

if ! command -v curl >/dev/null 2>&1; then
  echo "check_voice_catalogue: no curl, skipping."
  exit 0
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "check_voice_catalogue: no python3, skipping."
  exit 0
fi

index="$(mktemp)"
trap 'rm -f "$index"' EXIT

if ! curl -fsS --max-time 60 -o "$index" "$index_url"; then
  echo "check_voice_catalogue: could not reach the Piper voice index, skipping."
  echo "  (that is a network problem here, not a fault in the table)"
  exit 0
fi

# The names and folders exactly as the Rust table spells them. Pulled out with a regex
# rather than by parsing Rust, because the table is a list of string literals and the day
# it stops being one is the day this should be rewritten anyway.
mapfile -t entries < <(python3 -I - "$table" <<'PY'
import re
import sys

source = open(sys.argv[1], encoding="utf-8").read()
start = source.index("const CATALOGUE")
end = source.index("\n];", start)
block = source[start:end]
rows = re.findall(
    r'\(\s*"([^"]+)",\s*"([^"]*)",\s*"([^"]*)",\s*"([^"]+)",\s*\)',
    block,
)
if not rows:
    sys.exit("check_voice_catalogue: could not read the table out of voice_download.rs")
for name, _label, _size, folder in rows:
    print(f"{name}\t{folder}")
PY
)

if [ "${#entries[@]}" -eq 0 ]; then
  echo "check_voice_catalogue: the catalogue came back empty -- has the table moved?" >&2
  exit 1
fi

failures=0
for entry in "${entries[@]}"; do
  name="${entry%%$'\t'*}"
  folder="${entry##*$'\t'}"

  if ! python3 -I - "$index" "$name" "$folder" <<'PY'
import json
import sys

index_path, name, folder = sys.argv[1], sys.argv[2], sys.argv[3]
voices = json.load(open(index_path, encoding="utf-8"))
voice = voices.get(name)
if voice is None:
    near = sorted(k for k in voices if k.rsplit("-", 1)[0] == name.rsplit("-", 1)[0])
    hint = f" Published as: {', '.join(near)}." if near else ""
    sys.exit(f"{name} is not a published Piper voice.{hint}")
expected = f"{folder}/{name}.onnx"
if expected not in voice.get("files", {}):
    sys.exit(f"{name} is published, but not at {expected}.")
PY
  then
    failures=$((failures + 1))
    continue
  fi

  sample="$sample_base/$folder/speaker_0.mp3"
  if ! curl -fsS --max-time 60 -o /dev/null --range 0-0 "$sample"; then
    echo "$name: no preview clip at $sample" >&2
    failures=$((failures + 1))
  fi
done

if [ "$failures" -ne 0 ]; then
  echo >&2
  echo "check_voice_catalogue: $failures voice(s) in the table cannot be downloaded or previewed." >&2
  echo "The Piper project's own list is at $index_url" >&2
  exit 1
fi

echo "check_voice_catalogue: ${#entries[@]} voices, all published and all with a preview clip."
