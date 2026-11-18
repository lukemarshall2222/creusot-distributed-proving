#!/bin/sh
# ci/creusot (R7.3, R7.1): Creusot proves dp-core, with unmodified pinned
# why3find, so the verification of the backend never depends on the backend.
#
# Run from the repository root inside `nix develop .#ci`.
set -eu

cd crates/dp-core
# Translate even if cargo thinks the crate is up to date.
touch src/lib.rs
out=$(cargo creusot 2>&1) || { echo "$out" | tail -40; exit 1; }
echo "$out" | tail -3
# cargo creusot must have proved something, and everything.
echo "$out" | grep -Eq '^Proved \([1-9][0-9]* files\)'
