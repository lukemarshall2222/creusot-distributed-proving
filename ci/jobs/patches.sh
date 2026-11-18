#!/bin/sh
# ci/patches (R7.9): every patch under patches/ has its header, applies to
# the pinned upstream version and builds, and the patched why3find passes
# its harness against the unpatched one with real provers.
#
# Run from the repository root. Needs Nix; does not need a dev shell.
set -eu

status=0
for patch in patches/*/*.patch; do
  for field in 'Purpose:' 'Requirements: R' 'Upstream:'; do
    if ! sed '/^---$/q' "$patch" | grep -q "^$field"; then
      echo "ci/patches: $patch: header lacks \"$field\"" >&2
      status=1
    fi
  done
done
[ "$status" -eq 0 ] || exit "$status"

# Building applies each patch to the pinned source; an apply or build error
# fails here.
patched=$(nix build .#why3find-dispatch --no-link --print-out-paths)
unpatched=$(nix build .#why3find --no-link --print-out-paths)
nix build .#creusot --no-link
z3=$(nix build .#z3 --no-link --print-out-paths | head -1)
cvc5=$(nix build .#cvc5 --no-link --print-out-paths | head -1)
why3=$(nix build .#why3 --no-link --print-out-paths | head -1)
python=$(nix build .#python --no-link --print-out-paths)

project=$(mktemp -d)
trap 'rm -rf "$project"' EXIT
cat > "$project/arith.mlw" <<'MLW'
module Arith
  use int.Int

  let rec function sum (n: int) : int
    requires { n >= 0 }
    variant { n }
    ensures { 2 * result = n * (n + 1) }
  = if n = 0 then 0 else n + sum (n - 1)

  goal Easy: forall x y: int. x + y = y + x

  goal Split: (forall x: int. x * 0 = 0) /\ (forall a b: int. a < b -> a + 1 <= b)

  goal Wrong: forall x: int. x * x > x
end
MLW
echo '{ "fast": 0.2, "time": 1.0, "depth": 4, "packages": [], "provers": [ "z3", "cvc5" ], "tactics": [ "split_vc" ], "drivers": [], "warnoff": [] }' \
  > "$project/why3find.json"

PATH="$z3/bin:$cvc5/bin:$why3/bin:$PATH" "$python/bin/python3" \
  patches/why3find/test/dispatch_harness.py \
  --patched "$patched/bin/why3find" --baseline "$unpatched/bin/why3find" \
  "$project" arith.mlw
