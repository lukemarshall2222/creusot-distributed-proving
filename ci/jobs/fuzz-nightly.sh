#!/bin/sh
# sched/fuzz-nightly (R7.10): the fuzz harnesses on many more inputs than
# ci/fuzz-smoke, from a start that the caller changes every night
# (DP_FUZZ_SEED). DP_FUZZ_ROUNDS multiplies the number of inputs.
set -eu
: "${DP_FUZZ_ROUNDS:=200}"
export DP_FUZZ_ROUNDS
cargo test --release -p dp-fuzz
