#!/bin/sh
# sched/mutants-weekly (R7.13): cargo-mutants on the files of the
# soundness-critical targets and on the crates with a score threshold
# (tools/mutants/targets.toml), and the gate on the outcomes: no mutant of
# a target survives the tests of the target crates, and no crate's score
# is below its threshold.
#
# Run from the repository root inside `nix develop` (the default shell has
# cargo-mutants). MUTANTS_OUT names the output directory; MUTANTS_JOBS the
# number of mutants tested at once.
set -eu

# A test that cannot find its tools fails; it does not pass by skipping,
# which would let every mutant survive unseen or, worse, be counted for
# nothing.
export DP_REQUIRE_TOOLS=1

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
cargo mutants $packages $files --no-shuffle -j "${MUTANTS_JOBS:-2}" -o "$out/targets" || true
outcomes="$out/targets/mutants.out/outcomes.json"

# The crates that are judged by their score: every mutant of the crate,
# against the tests of the packages that the target list names for it.
cargo run --quiet -p mutants-gate -- --crates > "$out/crates.txt"
while read -r crate tests; do
  judges=""
  for package in $(echo "$tests" | tr ',' ' '); do
    judges="$judges --test-package $package"
  done
  # shellcheck disable=SC2086
  # cargo-mutants sets its time limit from the tests of the crate alone,
  # and the mutants also run the tests of the other packages: without a
  # floor, a run that is only slow counts as a mutant that hangs, and a
  # mutant that hangs counts as caught.
  cargo mutants -p "$crate" $judges --no-shuffle --minimum-test-timeout "${MUTANTS_TIMEOUT:-120}" \
    -j "${MUTANTS_JOBS:-2}" -o "$out/$crate" || true
  outcomes="$outcomes $out/$crate/mutants.out/outcomes.json"
done < "$out/crates.txt"

# shellcheck disable=SC2086
cargo run --quiet -p mutants-gate -- $outcomes
