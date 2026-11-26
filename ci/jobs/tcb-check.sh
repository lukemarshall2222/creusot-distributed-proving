#!/bin/sh
# ci/tcb-check (R7.8): the TCB_Document agrees with the requirements, the
# Lock_File and dp-core.
set -eu
cargo run --quiet -p tcb-check
# R7.7 criterion 6: every tool of the list is pinned, with a version and a
# hash. (The evidence for the hashes is reported by ci/repro-build.)
cargo run --quiet -p lock-check
