#!/bin/sh
# sched/mutants-weekly (R7.13): cargo-mutants on the files of the
# soundness-critical targets (tools/mutants/targets.toml), and the gate on
# its outcomes: no mutant of a target survives the tests of the target
# crates.
#
# Run from the repository root inside `nix develop` (the default shell has
# cargo-mutants). MUTANTS_OUT names the output directory; MUTANTS_JOBS the
# number of mutants tested at once.
set -eu

out=${MUTANTS_OUT:-$(mktemp -d)}
cargo run --quiet -p mutants-gate -- --check

files=""
for file in $(cargo run --quiet -p mutants-gate -- --files); do
  files="$files --file $file"
done
packages=""
for package in $(sed -n 's/^packages = \[\(.*\)\]/\1/p' tools/mutants/targets.toml | tr -d '",'); do
  packages="$packages -p $package"
done

# cargo-mutants ends with a status of its own when mutants survive; the
# gate decides, from the outcomes.
# shellcheck disable=SC2086
cargo mutants $packages $files --no-shuffle -j "${MUTANTS_JOBS:-2}" -o "$out" || true
cargo run --quiet -p mutants-gate -- "$out/mutants.out/outcomes.json"
