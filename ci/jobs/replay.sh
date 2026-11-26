#!/bin/sh
# ci/replay (R7.6): dp-replay on the files of honest Sessions and on
# tampered ones; and dp-replay has no networking code.
set -eu
. ci/jobs/common.sh
registered tests/replay tests/replay/Cargo.toml
cargo test -p dp-replay -p dp-replay-tests
# R7.6 criterion 1: it builds on its own without the `cluster` feature of
# dp-shell, which holds the transport.
if cargo tree -p dp-replay -e features --prefix none | grep -q 'dp-shell feature "cluster"'; then
  echo "ci/replay: dp-replay depends on the cluster feature of dp-shell" >&2
  exit 1
fi
cargo check -p dp-replay
