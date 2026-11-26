#!/bin/sh
# sched/canaries-full (R7.5): the canary harness with every mode, as
# ci/canaries, and its property tests with more cases. (More seeds per
# mode are not a setting of the harness yet.)
set -eu
. ci/jobs/common.sh
: "${DP_PROPTEST_FACTOR:=20}"
export DP_PROPTEST_FACTOR
cargo test -p dp-canaries
