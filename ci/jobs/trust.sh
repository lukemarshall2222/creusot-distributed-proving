#!/bin/sh
# ci/purity, ci/tcb and ci/traceability (R7.3, R7.8, R7.11): dp-core is a
# pure decision core, the TCB_Document agrees with the requirements, the
# Lock_File and dp-core, and the Traceability_Table agrees with the
# requirements and with the files of the commit.
#
# Run from the repository root. Needs cargo and git; no prover.
set -eu

status=0
cargo run --quiet --locked -p purity-check || status=1
cargo run --quiet --locked -p tcb-check || status=1
cargo run --quiet --locked -p traceability-check || status=1
exit "$status"
