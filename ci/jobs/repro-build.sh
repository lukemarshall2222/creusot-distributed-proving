#!/bin/sh
# ci/repro-build (R7.7 criteria 2 and 9): builds each package a second time
# and fails if the result differs from the first build.
#
# Run from the repository root. Needs Nix; does not need a dev shell.
set -u

status=0
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
