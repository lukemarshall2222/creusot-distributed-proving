#!/bin/sh
# sched/apalache-nightly (R1.5): Apalache on the specifications that have
# an Apalache configuration next to them (`<Spec>.apalache.json`). Without
# one, no run is recorded: the rows that name this job stay pending.
set -eu
found=0
for config in verification/tla/*.apalache.json; do
  [ -f "$config" ] || continue
  spec=${config%.apalache.json}.tla
  apalache-mc check --config="$config" "$spec"
  found=$((found + 1))
done
[ "$found" -gt 0 ] || { echo "sched/apalache-nightly: no inputs yet (verification/tla/*.apalache.json); no run is recorded" >&2; exit 78; }
