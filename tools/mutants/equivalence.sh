#!/bin/sh
# The equivalence artifacts of tools/mutants/exclusions.toml (R7.13
# criterion 7): applies each diff of tools/mutants/equivalent/ in turn to a copy
# of the repository and has Creusot prove dp-core there. The proof of a
# mutated function against its unchanged contract is what shows that the
# mutant is equivalent, as far as the contract fixes the result.
#
# Run from the repository root inside `nix develop .#ci`. The repository
# itself is not changed.
set -eu

copy=$(mktemp -d)
trap 'rm -rf "$copy"' EXIT
# The sources and the recorded proofs; not the build directories.
tar -cf - --exclude ./target --exclude ./.git --exclude ./verif --exclude './proofs/lean/.lake' . | (cd "$copy" && tar -xf -)

listed=$(grep -c '^\[\[exclusion\]\]' tools/mutants/exclusions.toml || true)
count=$(ls tools/mutants/equivalent/*.diff | wc -l | tr -d ' ')
if [ "$count" -ne "$listed" ]; then
  echo "equivalence: $listed exclusions, $count diffs" >&2
  exit 1
fi

# One mutant at a time: each is proved on its own.
root=$PWD
cd "$copy"
for diff in "$root"/tools/mutants/equivalent/*.diff; do
  file=$(sed -n '1s/^--- //p' "$diff")
  cp "$root/$file" "$file.original"
  # A diff that no longer applies is an exclusion without its artifact.
  patch --quiet "$file" < "$diff" || { echo "equivalence: $diff does not apply to $file" >&2; exit 1; }
  ci/jobs/creusot.sh || { echo "equivalence: Creusot does not prove the mutant $(basename "$diff")" >&2; exit 1; }
  echo "equivalence: proved with $(basename "$diff")"
  mv "$file.original" "$file"
done
echo "equivalence: Creusot proves dp-core with each of the $count excluded mutants"
