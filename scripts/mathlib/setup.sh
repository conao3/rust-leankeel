#!/usr/bin/env bash
# Usage: setup.sh WORKDIR
# Clones the pinned Mathlib into WORKDIR/mathlib4, fetches its build cache, and builds the
# adapter with the upstream toolchain into WORKDIR/adapter and Leankeel in release mode.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
. "$here/env.sh"
work=${1:?usage: setup.sh WORKDIR}
mkdir -p "$work"
work=$(cd "$work" && pwd)

if [ ! -d "$work/mathlib4/.git" ]; then
  git init -q "$work/mathlib4"
  git -C "$work/mathlib4" remote add origin "$MATHLIB_REPO"
fi
git -C "$work/mathlib4" fetch -q --depth 1 origin "$MATHLIB_COMMIT"
git -C "$work/mathlib4" checkout -q --detach "$MATHLIB_COMMIT"
test "$(cat "$work/mathlib4/lean-toolchain")" = "$(cat "$root/native/lean-toolchain")"
(cd "$work/mathlib4" && lake exe cache get)

rm -rf "$work/adapter"
mkdir -p "$work/adapter"
cp "$root"/native/{Main.lean,lakefile.toml,lake-manifest.json,lean-toolchain} "$work/adapter/"
(cd "$work/adapter" && lake build)

(cd "$root" && LEANFRAN_OFFLINE=1 cargo build --release)
