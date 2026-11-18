#!/bin/sh
# ci/tlc (R1.5): TLC checks every specification under verification/tla that
# has a configuration next to it.
#
# Run from the repository root inside `nix develop .#ci`.
set -eu

cd verification/tla
checked=0
for cfg in *.cfg; do
  spec=${cfg%.cfg}.tla
  out=$(tlc -workers auto -deadlock "$spec" -config "$cfg" 2>&1) || true
  echo "$out" | grep -E 'states generated|No error|Error' || true
  if ! echo "$out" | grep -q 'Model checking completed. No error has been found.'; then
    echo "$out" | tail -40
    echo "ci/tlc: $spec failed" >&2
    exit 1
  fi
  checked=$((checked + 1))
done
rm -rf states
[ "$checked" -gt 0 ] || { echo "ci/tlc: no configuration found" >&2; exit 1; }

# ci/trace-validation (R1.5 criterion 10): traces of the Coordinator's shell
# from simulated runs with faults are behaviours of Cluster.tla, and a
# corrupted trace is not.
cd ../..
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
