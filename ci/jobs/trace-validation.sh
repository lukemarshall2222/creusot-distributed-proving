#!/bin/sh
# ci/trace-validation (R1.5 criterion 10): traces of the Coordinator's shell
# from simulated runs with faults are behaviours of Cluster.tla, and a
# corrupted trace is not.
set -eu
traces=$(mktemp -d)
cargo build -q -p dp-sim --example trace
for seed in 1 2 3 4 5 6 7 8; do
  cargo run -q -p dp-sim --example trace -- "$traces/$seed.trace" "$seed" 2>/dev/null
  python3 verification/tla/trace_check.py "$traces/$seed.trace"
done
if python3 verification/tla/trace_check.py --corrupt "$traces/1.trace"; then
  echo "ci/trace-validation: a corrupted trace was accepted" >&2
  exit 1
fi
rm -r "$traces"
