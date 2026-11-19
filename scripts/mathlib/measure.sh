#!/usr/bin/env bash
# Usage: measure.sh WORKDIR OUT.jsonl [SHARD SHARDS]
# Runs `leankeel source fix-source` on every Mathlib/**/*.lean file of the pinned Mathlib
# (or on shard SHARD of SHARDS) and writes one JSON object per file to OUT.jsonl.
# JOBS sets the parallelism (default: nproc), LIMIT the number of files (default: all),
# FILE_TIMEOUT the seconds allowed per file (default: 3600), RULES the fix rules (default:
# MATHLIB_RULES) and CONFIG a Leankeel configuration file with extra Lean options.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
. "$here/env.sh"
work=$(cd "${1:?usage: measure.sh WORKDIR OUT.jsonl [SHARD SHARDS]}" && pwd)
out=${2:?usage: measure.sh WORKDIR OUT.jsonl [SHARD SHARDS]}
shard=${3:-0}
shards=${4:-1}
mathlib=$work/mathlib4
if ! [ "$shards" -ge 1 ] || ! [ "$shard" -ge 0 ] || ! [ "$shard" -lt "$shards" ]; then
  echo "measure.sh: SHARD must be in 0..SHARDS-1" >&2
  exit 252
fi
if [ -n "${LIMIT:-}" ] && ! [[ $LIMIT =~ ^[1-9][0-9]*$ ]]; then
  echo "measure.sh: LIMIT must be a positive integer" >&2
  exit 252
fi
test "$(git -C "$mathlib" rev-parse HEAD)" = "$MATHLIB_COMMIT"

CONFIG=${CONFIG:+$(realpath -e "$CONFIG")}
export MATHLIB=$mathlib RULES=${RULES:-$MATHLIB_RULES} CONFIG
export LEANKEEL=$root/target/release/leankeel
export LEANKEEL_ADAPTER=$work/adapter/.lake/build/bin/leankeel-adapter
export FILE_TIMEOUT=${FILE_TIMEOUT:-3600}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export TMPOUT=$tmp

one() {
  local file=$1 key start stop code
  key=$(printf '%s' "$file" | tr '/' '_')
  start=$(date +%s.%N)
  set +e
  # shellcheck disable=SC2086
  (cd "$MATHLIB" && timeout "$FILE_TIMEOUT" "$LEANKEEL" source fix-source --file "$file" \
    --project-dir . ${CONFIG:+--config "$CONFIG"} --rules $RULES) >"$TMPOUT/$key.json" 2>"$TMPOUT/$key.err"
  code=$?
  set -e
  stop=$(date +%s.%N)
  jq -cn --arg path "$file" --argjson code "$code" \
    --argjson seconds "$(echo "$stop - $start" | bc)" \
    --rawfile out "$TMPOUT/$key.json" --rawfile err "$TMPOUT/$key.err" '
    ($out | try fromjson catch null) as $r
    | {Path: $path, Exit: $code, Seconds: $seconds}
      + if $r == null or ($r | has("Error")) then
          {Error: (($r // {}).Error // ($err | .[-2000:]))}
        else
          {Summary: $r.Summary,
           Diagnostics: ([$r.Diagnostics[] | {Rule, Level}] | group_by(.Rule)
             | map({Rule: .[0].Rule, Level: .[0].Level, Count: length})),
           Applied: [$r.Applied[] | {Rule, Title}],
           Rejected: [$r.Rejected[] | {Rule, Title, Reason}]}
        end'
  rm -f "$TMPOUT/$key.json" "$TMPOUT/$key.err"
}
export -f one

git -C "$mathlib" ls-files 'Mathlib/*.lean' | LC_ALL=C sort \
  | awk -v s="$shard" -v n="$shards" -v l="${LIMIT:-0}" '(NR - 1) % n == s && (l == 0 || ++c <= l)' \
  | xargs -P "${JOBS:-$(nproc)}" -I{} bash -c 'one "$1"' _ {} >"$out"
