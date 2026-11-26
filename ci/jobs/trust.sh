#!/bin/sh
# ci/purity, ci/tcb-check and ci/traceability (R7.3, R7.8, R7.11): dp-core is a
# pure decision core, the TCB_Document agrees with the requirements, the
# Lock_File and dp-core, and the Traceability_Table agrees with the
# requirements and with the files of the commit.
#
# Run from the repository root. Needs cargo and git; no prover.
set -eu

status=0
cargo run --quiet -p purity-check || status=1
cargo run --quiet -p tcb-check || status=1
cargo run --quiet -p traceability-check || status=1
# The static part of the mutation gate (R7.13 criterion 6): the functions
# of the Soundness-critical rows are targets.
cargo run --quiet -p mutants-gate -- --check || status=1
exit "$status"
