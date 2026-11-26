#!/bin/sh
# ci/repro-build (R7.7 criteria 2 and 9): builds each package a second time
# and fails if the result differs from the first build.
#
# Run from the repository root. Needs Nix; does not need a dev shell.
set -u

status=0
# The static part (criteria 6 and 9): the Lock_File is complete, lists
# every platform, and has an upstream checksum or two rebuilds for every
# solver and checker.
if command -v cargo > /dev/null; then
  cargo run --quiet -p lock-check -- --evidence || status=1
else
  nix develop .#ci --command cargo run --quiet -p lock-check -- --evidence || status=1
fi
for package in workspace alt-ergo z3 cvc5 ethos carcara why3find-dispatch; do
  nix build ".#$package" --no-link || { echo "ci/repro-build: $package does not build" >&2; status=1; continue; }
  if nix build ".#$package" --no-link --rebuild 2> rebuild.err; then
    echo "reproducible: $package"
  else
    echo "NOT reproducible: $package"
    grep -E 'differs|deterministic' rebuild.err | head -3
    status=1
  fi
done
rm -f rebuild.err
exit "$status"
