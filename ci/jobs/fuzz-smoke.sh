#!/bin/sh
# ci/fuzz-smoke (R7.10): every fuzz harness on its seeds, on mutations of
# them and on random bytes, briefly. Every directory under fuzz/ with a
# harness is a target of the run.
set -eu
for harness in fuzz/*/harness.rs; do
  target=$(basename "$(dirname "$harness")")
  grep -q "(\"$target\"," fuzz/lib.rs || { echo "ci/fuzz-smoke: fuzz/$target is no target of fuzz/lib.rs" >&2; exit 1; }
done
cargo test -p dp-fuzz
