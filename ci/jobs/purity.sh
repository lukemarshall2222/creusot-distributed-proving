#!/bin/sh
# ci/purity (R7.3): dp-core is a pure decision core.
set -eu
cargo run --quiet -p purity-check
