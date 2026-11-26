#!/bin/sh
# ci/sim (R1.5, R1.6): the real shells on the simulated network: the fault
# schedules and the adversarial Coordinator. Every scenario file of
# tests/sim is a test target.
set -eu
for scenario in tests/sim/*.rs; do
  name=$(basename "$scenario" .rs)
  case "$name" in lib|adversary) continue ;; esac
  grep -q "path = \"$name.rs\"" tests/sim/Cargo.toml || { echo "ci/sim: $scenario is no test target" >&2; exit 1; }
done
cargo test -p dp-sim
