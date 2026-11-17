---
inclusion: always
---

## Verification policy (the most important part)

Everything must be verified. A requirement counts as verified only when a mechanical check exists for it, runs in CI (or a scheduled job if expensive), and is named in the requirement. Manual review never counts on its own.

Assurance levels, strongest first:

- A1 Mechanized proof: Lean 4, Rocq or Isabelle for metatheory (e.g., soundness of sharing); deductive verification of implementation code, preferring Creusot itself for Rust (dogfooding), or Verus.

- A2 Model checking: TLA+ with TLC or Apalache for protocols; Kani for bounded checks of Rust code.

- A3 Per-run checking: an independent checker validates each individual result or translation at runtime.

- A4 Testing: property-based tests, differential tests (sharing on vs. off, distributed vs. local), fuzzing, deterministic simulation with fault injection (crashes, partitions, delays, duplicate messages), and mutation testing to show the tests actually catch bugs.

- A5 Empirical: reproducible experiment scripts for performance claims (pinned versions, fixed seeds, repeated runs, confidence intervals, report numbers generated from raw data).

Rules:

- Soundness-critical requirements (any whose failure could mark a false goal as proved) need A1 or A3, plus A4 on the implementation.

- Liveness, robustness and performance requirements need at least one of A2, A4 or A5.

- Untrusted by default: solvers, solver patches, the coordinator, the network and the lemma pool. Checks must catch their mistakes.

- Keep decision logic (scheduling, cancellation, lemma admission, verdict recording) separate from I/O so it can be verified at A1 or A2.

- If a requirement can't be verified, rewrite it until it can, or move it to Non-goals with a reason.
