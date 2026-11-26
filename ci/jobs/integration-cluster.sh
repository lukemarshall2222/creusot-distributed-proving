#!/bin/sh
# ci/integration-cluster (R1): a Coordinator, Workers and the Front_End as
# processes on one machine, with real provers.
set -eu
. ci/jobs/common.sh
for file in tests/integration/cluster/*.rs; do
  name=$(basename "$file" .rs)
  [ "$name" = main ] || grep -q "^mod $name;" tests/integration/cluster/main.rs || {
    echo "ci/integration-cluster: $file is no module of the suite" >&2; exit 1; }
done
cargo test -p dp-integration --test cluster
