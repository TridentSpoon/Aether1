#!/usr/bin/env bash
# The Settings pane and the engine have to be running with the same default, and nothing
# else would notice when they stop.
#
# `commands::get_settings` holds a literal block of defaults. The pane renders every switch
# from that block and writes every field back on Save. The engine reads the same settings
# through `get_setting_bool("key", <default>)`, which declares the default again, in another
# file. When the two disagree, three things follow in order, and none of them looks like a
# bug in a diff:
#
#   1. the pane draws the switch at the *declared* value while the engine runs at *its*
#      value, so the operator is shown the opposite of what is happening;
#   2. the operator saves anything at all -- a voice, a theme, a key -- and the declared
#      value is written for real, because Save writes every field;
#   3. a one-time flip keyed to a marker (`tools::apply_default_on`) has already spent
#      itself on the first open, so nothing rescues it afterwards.
#
# That is exactly what `tools_enabled` did: `false` declared against a `true` engine default,
# so a fresh install showed Tools off while the companion was reading files and searching
# happily, and the first Save turned them off in earnest (PR #245, then #261 from the other
# end). It was found by driving the binary and comparing two endpoints, not by reading. This
# script is that comparison, made cheap enough to run on every push.
#
# **It compares booleans only, and that is deliberate.** A string or a number default can
# differ for honest reasons (an empty box against a named fallback), and a check that cried
# about those would be turned off within a week. A boolean has two values and no middle, so a
# disagreement is always the fault above.
#
# Test callers are excluded by path, not by eye: a test that deliberately reads a setting
# with the other default is a test doing its job.

set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
commands="$here/src-tauri/src/commands.rs"
src="$here/src-tauri/src"

[ -f "$commands" ] || { echo "check_setting_defaults: $commands is missing"; exit 1; }

# The declared block: every `"key": true,` / `"key": false,` inside get_settings, plus the
# `defaults_obj.insert("key", Value::Bool(x))` calls that live just past it because the
# `json!` literal is at the macro's recursion limit.
declared="$(mktemp)"
inserted="$(mktemp)"
engine="$(mktemp)"
trap 'rm -f "$declared" "$inserted" "$engine"' EXIT

awk '/^pub fn get_settings/{inside=1} inside' "$commands" \
  | awk '/^}/{if (seen) exit} {seen=1; print}' \
  | { grep -oE '"[a-z0-9_]+": (true|false),' || true; } \
  | tr -d '",:' \
  | awk '{print $1, $2}' \
  | sort -u > "$declared"

awk '/^pub fn get_settings/{inside=1} inside' "$commands" \
  | awk '/^}/{if (seen) exit} {seen=1; print}' \
  | { grep -oE 'insert\("[a-z0-9_]+"\.to_string\(\), Value::Bool\((true|false)\)' || true; } \
  | sed -E 's/insert\("([a-z0-9_]+)"\.to_string\(\), Value::Bool\((true|false)\)/\1 \2/' \
  | sort -u >> "$declared"

sort -u -o "$declared" "$declared"

# A refactor that moved the block out of `get_settings`, or renamed the function, would leave
# this script passing while checking nothing -- the failure mode every guard script has to
# rule out about itself.
if [ ! -s "$declared" ]; then
  echo "check_setting_defaults: found no boolean defaults declared in get_settings."
  echo "Either the block moved or this script's patterns went stale. Fix the script;"
  echo "a guard that silently checks nothing is worse than no guard."
  exit 1
fi

# The engine side: every non-test `get_setting_bool("key", default)`. Files under tests/ and
# blocks guarded by #[cfg(test)] are out -- the latter by dropping anything after the file's
# `mod tests`, which is where every test in this codebase lives.
for file in $(grep -rl 'get_setting_bool("' "$src" --include='*.rs'); do
  awk '/^#\[cfg\(test\)\]/{exit} {print}' "$file" \
    | grep -oE 'get_setting_bool\("[a-z0-9_]+",[[:space:]]*(true|false)\)' \
    | sed -E 's/get_setting_bool\("([a-z0-9_]+)",[[:space:]]*(true|false)\)/\1 \2/' || true
done | sort -u > "$engine"

# A key read with two different defaults in non-test code is its own fault, and a worse one:
# whichever call site runs first decides what the program does.
twice="$(cut -d' ' -f1 "$engine" | uniq -d || true)"
if [ -n "$twice" ]; then
  echo "check_setting_defaults: these settings are read with more than one default in"
  echo "non-test code, so what the program does depends on which call site runs first:"
  for key in $twice; do
    echo "  $key: $(grep "^$key " "$engine" | cut -d' ' -f2 | tr '\n' ' ')"
  done
  echo
  echo "Pick one default and have the other call site read it from there."
  exit 1
fi

bad=0
# Counted because the first version of this script passed while comparing nothing: the keys
# came out of the declared block with their colon still attached, so no key ever matched the
# engine's list and every run agreed. A guard that cannot fail is not a guard, so the count
# is checked at the end against the one setting this exists for.
compared=0
while read -r key value; do
  [ -n "$key" ] || continue
  mine="$(grep "^$key " "$engine" | cut -d' ' -f2 | head -1 || true)"
  # A key the engine never reads as a boolean is not a disagreement: plenty of the declared
  # block is read by the page alone.
  [ -n "$mine" ] || continue
  compared=$((compared + 1))
  if [ "$mine" != "$value" ]; then
    if [ "$bad" = 0 ]; then
      echo "check_setting_defaults: the Settings pane and the engine disagree about what an"
      echo "install that has never chosen actually does."
      echo
      bad=1
    fi
    echo "  $key: get_settings declares $value, get_setting_bool defaults to $mine"
  fi
done < "$declared"

if [ "$bad" != 0 ]; then
  echo
  echo "The pane draws its own value and writes every field back on Save, so this does not"
  echo "stay cosmetic: the first Save of anything at all makes the declared value real."
  echo "Change one side so they agree, and say in a comment which way round it is and why."
  exit 1
fi

# `tools_enabled` is the setting this whole check exists for, and it is read with a literal
# key in non-test code, so it must be on both lists. If it is not, the extraction has gone
# stale and everything above agreed about nothing.
if ! grep -q '^tools_enabled ' "$declared" || ! grep -q '^tools_enabled ' "$engine"; then
  echo "check_setting_defaults: tools_enabled is missing from one of the two lists, so this"
  echo "check is not reading what it thinks it is. Fix the patterns in this script."
  exit 1
fi

echo "check_setting_defaults: $compared declared default(s) match the engine's."
