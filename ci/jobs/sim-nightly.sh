#!/bin/sh
# sched/sim-nightly (R1.5, R1.6): the simulation scenarios on many more
# schedules than ci/sim.
set -eu
: "${DP_PROPTEST_FACTOR:=50}"
export DP_PROPTEST_FACTOR
cargo test --release -p dp-sim
