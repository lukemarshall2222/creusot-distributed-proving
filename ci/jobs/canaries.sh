#!/bin/sh
# ci/canaries (R7.5): no mode of the System gives a Canary_Goal a proof
# label. The harness reads its modes from tests/canaries/modes.toml and
# fails on a listed mode that did not run.
set -eu
. ci/jobs/common.sh
cargo test -p dp-canaries
