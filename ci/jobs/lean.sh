#!/bin/sh
# ci/lean (R7.1, P7.1.2): the Lean library builds, `verdict_sound` and
# `certified_sound` are proved without `sorry` and without axioms beyond
# those of Lean's logic, `Verdict.lean` depends on the model and the trace
# alone, and the Rust decision functions agree with the executable Lean
# references on every input.
#
# Run from the repository root inside a shell with Lean and cargo.
set -eu

cd proofs/lean
lake build

# No `sorry` in the sources: Lean would only warn.
if grep -n 'sorry' DP/*.lean DP/Ref/*.lean; then
  echo "ci/lean: a proof is left open" >&2
  exit 1
fi

# The theorems rest on nothing but Lean's own axioms.
audit=$(mktemp)
printf 'import DP\n#print axioms DP.verdict_sound\n#print axioms DP.certified_sound\n' > "$audit.lean"
lake env lean "$audit.lean" > "$audit" 2>&1 || { cat "$audit"; exit 1; }
cat "$audit"
for theorem in verdict_sound certified_sound; do
  grep -q "DP.$theorem" "$audit" || { echo "ci/lean: no axiom report for $theorem" >&2; exit 1; }
done
if tr -d '[],\n' < "$audit" | tr ' ' '\n' | grep -E '^[A-Za-z_.]+$' \
    | grep -vE "^(DP\.(verdict|certified)_sound|depends|does|not|depend|on|any|axioms|axioms:|'DP\.(verdict|certified)_sound'|propext|Classical\.choice|Quot\.sound)$"; then
  echo "ci/lean: an axiom outside propext, Classical.choice and Quot.sound" >&2
  exit 1
fi
rm -f "$audit" "$audit.lean"

# P7.1.2: `verdict_sound` is no consequence of the sharing theorems.
if grep -E '^import ' DP/Verdict.lean | grep -vE '^import DP\.(Trace|Model)$'; then
  echo "ci/lean: Verdict.lean imports more than the model and the trace" >&2
  exit 1
fi

# The differential test of the Rust functions against the references.
cd ../..
DP_LEAN_REF="$PWD/proofs/lean/.lake/build/bin/dp-lean-ref" cargo test -p dp-diff --test label_lean_vs_rust
