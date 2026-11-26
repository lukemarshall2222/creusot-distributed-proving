#!/bin/sh
# ci/lean-partition (R3.5): the partitioned soundness theorem, which has a
# Lean target of its own so that ci/lean does not depend on it.
set -eu
[ -f proofs/lean/DP/Partition.lean ] || { echo "ci/lean-partition: no inputs yet (proofs/lean/DP/Partition.lean); nothing was checked" >&2; exit 0; }
cd proofs/lean
lake build DP.Partition
if grep -n sorry DP/Partition.lean; then echo "ci/lean-partition: a proof is left open" >&2; exit 1; fi
