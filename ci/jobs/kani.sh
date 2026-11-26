#!/bin/sh
# ci/kani (R7.3): the Kani harnesses of verification/kani/, which are
# modules of dp-core under cfg(kani). Every harness must be verified, and
# none may be missing from the run.
#
# Run from the repository root in a shell with Kani (`nix develop`).
set -eu

expected=$(cat verification/kani/*.rs | grep -c '^#\[kani::proof\]')
out=$(cargo kani -p dp-core 2>&1) || { echo "$out" | tail -40; exit 1; }
echo "$out" | grep -E '^(Checking harness|VERIFICATION|Complete)' 
echo "$out" | grep -q "^Complete - $expected successfully verified harnesses, 0 failures, $expected total" || {
  echo "ci/kani: not every one of the $expected harnesses was verified" >&2
  exit 1
}
