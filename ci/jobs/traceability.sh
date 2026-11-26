#!/bin/sh
# ci/traceability (R7.11, R7.13): the Traceability_Table agrees with the
# requirements, with the files of the commit and with the jobs of the
# workflows; the functions of the Soundness-critical rows are mutation
# targets. The workflow starts this job after every other ci/ job.
set -eu
status=0
cargo run --quiet -p traceability-check || status=1
cargo run --quiet -p mutants-gate -- --check || status=1
exit "$status"
