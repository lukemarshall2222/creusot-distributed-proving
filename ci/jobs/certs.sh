#!/bin/sh
# ci/certs (R5): certificates with the real proof producer and the real
# checker: the tamper cases, the Certificate_Store, and Sessions whose
# store fails.
set -eu
. ci/jobs/common.sh
registered tests/certs tests/certs/Cargo.toml
cargo test -p dp-cert -p dp-certs-tests
cargo test -p dp-integration --test certs_store_faults
