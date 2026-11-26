#!/bin/sh
# ci/test: the unit and property tests of every crate and tool, and the
# end-to-end tests of the Front_End on one machine with real provers. The
# suites with jobs of their own (canaries, certs, diff, fuzz-smoke,
# integration-cluster, replay, sim) run there.
set -eu
. ci/jobs/common.sh
cargo test --workspace \
  --exclude dp-canaries --exclude dp-certs-tests --exclude dp-diff --exclude dp-fuzz \
  --exclude dp-integration --exclude dp-replay-tests --exclude dp-sim
cargo test -p dp-integration --test cli_labels --test pins
