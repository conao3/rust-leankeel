#!/usr/bin/env bash
# Usage: compare.sh IN.jsonl COMMITTED.jsonl
# Fails when a record of IN.jsonl differs from the committed record of the same file,
# ignoring the elapsed time.
set -euo pipefail
in=${1:?usage: compare.sh IN.jsonl COMMITTED.jsonl}
committed=${2:?usage: compare.sh IN.jsonl COMMITTED.jsonl}
jq -n --slurpfile new "$in" --slurpfile old "$committed" -e '
  ($old | map({key: .Path, value: del(.Seconds)}) | from_entries) as $o
  | [$new[] | del(.Seconds) | select(. != $o[.Path]) | .Path] as $diff
  | if $new == [] then error("no records")
    elif $diff == [] then "\($new | length) records match" else error("differ: \($diff)") end
'
