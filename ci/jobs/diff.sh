#!/bin/sh
# ci/diff: the differential tests, one suite per directory of tests/diff.
# The comparison with the Lean references runs in ci/lean, which builds
# them; here it passes with a notice.
set -eu
. ci/jobs/common.sh
registered tests/diff tests/diff/Cargo.toml
cargo test -p dp-diff
