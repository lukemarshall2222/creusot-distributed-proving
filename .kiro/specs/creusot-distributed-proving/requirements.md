# Requirements Document

## Introduction

This project builds a distributed proving backend for Creusot, the deductive verifier for Rust. Today Creusot compiles Rust (MIR) to Coma, an intermediate verification language of the Why3 platform. why3find generates verification goals, applies tactics such as `split_vc`, runs SMT solvers (Alt-Ergo, Z3, cvc5), caches results in `.why3find`, and records proof certificates in `proof.json`.

The project adds six stages, delivered in this order:

1. Distribution: send goals to a cluster of machines, not only local cores.
2. Portfolio racing: race several solver configurations (solver, seed, options) on hard goals and keep the first accepted answer.
3. Within-goal lemma sharing: workers racing on the same goal exchange learned lemmas.
4. Cross-goal and quantified sharing: reuse lemmas and quantifier-instantiation hints between sibling goals generated from the same Rust function or module.
5. Certificates: every result that depended on shared lemmas comes with a proof that an independent checker accepts.
6. Evaluation: measure all of the above on hard Creusot goals.

The project builds on why3find's goal generation, per-goal strategy, tactics, prover configuration and Calibration, reached through a small dispatch-mode patch to why3find (`why3find prove --dispatch`) and a small backend option of Creusot_CLI. A separate Rust Coordinator takes the place of the central proxy of why3find's proof-server mode (`why3find server`), and Workers and the Front_End start solver processes themselves (design decisions D-1, D-3, D-4 and D-16). The project also builds on existing proof formats and checkers (cvc5 CPC proofs checked by Ethos, Alethe proofs checked by Carcara). It targets three open problems: lemma sharing for quantified goals, soundness of sharing across goals, and certificates for proofs that used imported lemmas.

The verification policy in `.kiro/steering/verification-policy.md` governs this document. Every requirement names its mechanical checks, the assurance level of each check, the artifact path, and the CI job (or scheduled job) that gates it. Solvers, solver patches, the coordinator, the network and the lemma pool are untrusted; the checks must catch their mistakes.

### How to read this document

- Requirement IDs have the form `R<group>.<n>`. Groups R1 to R6 follow the six stages. Group R7 holds cross-cutting requirements. Headings read `Requirement N: R<group>.<n> Title`; N is a sequential number required by the spec tooling, and the `R<group>.<n>` ID is the stable identifier used everywhere else.
- Each requirement states a **Priority** (Must, Should, Could) and a **Class**. The Class decides the minimum assurance levels under the verification policy:
  - Soundness-critical: failure could mark a false goal as proved. Needs A1 or A3, plus A4 on the implementation.
  - Liveness, Robustness, Performance: need at least one of A2, A4, A5.
  - Functional and Process: need at least one mechanical check (normally A4).
- Property IDs have the form `P<group>.<n>.<k>`. Each property is stated precisely enough to become a property-based test or a formal theorem.
- Verification lines have the form level — method (with the tool) — artifact path — gating CI job; one line may hold several entries of the form method — artifact path — CI job, separated by semicolons, at the line's level. Job names starting with `ci/` run on every pull request. Job names starting with `sched/` run on a schedule because they are expensive.
- `<TBD-n>` marks a numeric value that has not been decided. Every `<TBD-n>` has an entry under Open Questions.
- Artifact paths and CI job names are the planned layout for the new repository. The design document may refine them; the Traceability_Table (R7.11) is the authoritative mapping.
- `D-n` names decision D-n of the Design Decisions section of `design.md`. An Open Question marked "Resolved by design decision D-n" is decided there, and the requirement text that the decision fixes cites the same D-n.

### Planned repository layout (referenced by Verification subsections)

| Path | Contents |
|---|---|
| `crates/dp-core/` | Pure decision logic (R7.3): escalation, scheduling, race termination, cancellation, Result acceptance, admission, hygiene, hints, the Lemma and Instantiation_Hint export filters (R4.3 criterion 5, R4.4 criterion 4), Lemma_Pool bounds (`pool.rs`, R3.6), certification decisions (`certification.rs`, R5.1), the composition plan (`compose_plan.rs`, R5.2), the supported-fragment check of Composed_Certificates (`fragment.rs`, R5.2, R5.3), verdict labelling, cache keys. Verified with Creusot and Kani. |
| `crates/dp-formats/` | Parsers and printers for every exchanged format. |
| `crates/dp-canon/` | Canonicalizer. |
| `crates/dp-cert/` | Proof parsing and printing, certificate assembly from dp-core's composition plan, and checker drivers. |
| `crates/dp-frontend/`, `crates/dp-coordinator/`, `crates/dp-worker/`, `crates/dp-pool/`, `crates/dp-replay/` | I/O shells around dp-core; `crates/dp-coordinator/` is the shell of the Coordinator (D-1). |
| `crates/dp-shell/` | Shared `Env` trait and adapters (transport, clock, process runner with process groups, file store, trace writer) used by every shell and by `tests/sim/`, and the client of the why3find bridge (`dispatch` format). |
| `crates/dp-testkit/` | Test-only crate with the shared property-test configuration and generators. |
| `proofs/lean/DP/` | Lean 4 metatheory. |
| `proofs/lean/DP/Ref/` | Executable Lean reference definitions, compiled for the `*_lean_vs_rust` differential tests. |
| `verification/tla/` | TLA+ specifications, TLC and Apalache configurations, trace validation. |
| `verification/tla/actions.toml` | Generated table from dp-core action names to TLA+ actions, read by `ci/trace-validation`. |
| `verification/kani/` | Kani harnesses. |
| `tests/prop/`, `tests/diff/`, `tests/sim/`, `tests/integration/`, `tests/canaries/`, `tests/replay/`, `tests/certs/` | Property-based, differential, simulation, integration, canary, replay and certificate-tampering tests. |
| `fuzz/` | Fuzz targets. |
| `eval/corpus/` | Corpus exporter configuration and corpus tests (R6.1). |
| `eval/rq/` | Pre-registration files, one per research question, and `corpus.toml` for the hardness measurement (R6.2). |
| `eval/scripts/` | Experiment scripts and the hardness measurement (R6.1, R6.2). |
| `eval/raw/` | Raw measurements and run manifests (R6.3). |
| `eval/report/`, `eval/report/check/` | Report template, report scripts, generated report, and the report and committed-raw-data checks: numeric literals, provenance, pre-registration history, run selection, decision rule, estimates, canary records and canary exclusion, manifests, seeds and repetitions (R6.3, R6.4). |
| `eval/tests/` | Smoke, manifest, metric-computation (`eval/tests/metrics/`), canary-gate and report-check tests of the evaluation (R6.2 to R6.4). |
| `tools/purity-check/` | dp-core dependency, banned-interface and registry checks (R7.3). |
| `tools/mutants/` | cargo-mutants configuration, `targets.toml` and `exclusions.toml` (R7.13). |
| `tools/tcb-check/`, `tools/traceability-check/`, `tools/patch-check/` | TCB_Document, traceability and patch checks (R7.8, R7.11, R7.9). |
| `patches/<dependency>/` | Small upstreamable patches to external dependencies (R7.9): `patches/why3find/` (dispatch mode, D-3, AS-37), `patches/creusot/` (the Creusot_CLI backend option, D-4, AS-2), `patches/cvc5/` and `patches/z3/` (Sharing_Solver patches, R3.1, R4.4). |
| `ci/jobs/` | One script per CI job of the CI job list (R7.11). |
| `deps/` | Lock file pinning every external tool. |
| `docs/tcb.toml`, `docs/tcb.md` | Trusted computing base document. |
| `docs/traceability.toml`, `docs/traceability.md` | Traceability source and generated table. |
| `docs/formats/`, `docs/formats/index.toml` | Grammars of exchanged formats and the format registry (R7.10). |

The System also uses these locations in the directory of a Creusot project on the Rust developer's machine:

| Path | Contents |
|---|---|
| `dp.toml` | The project configuration file: Solver_Configurations and Portfolios (R2.1), the cluster option (R1.1), the fallback option (R1.6) and the other options of the System. |
| `.dp-cache/` | The Result_Cache (R7.5), next to `.why3find`, in a location that unmodified pinned why3find does not read (AS-35). Entries of Results labelled proved-standalone are also written to why3find's own cache layout inside `.why3find` (R7.5 criterion 7). |
| `.dp-certs/` | The Certificate_Store (R5.4), one directory per entry named by its content hash. |
| `.why3find`, `proof.json` | why3find's cache and proof file, as today; `proof.json` carries the extension fields of R2.3 and R7.2. |

### User roles

- **Rust developer**: uses Creusot to verify Rust code and reads proof results.
- **Proof engineer**: configures solver portfolios, sharing modes and tactics; maintains this system.
- **Cluster operator**: runs the Coordinator and Workers.
- **Researcher**: runs the evaluation.
- **Auditor**: checks the trust story and re-checks results offline.

## Glossary

- **System**: the distributed proving backend delivered by this project, including every component below that this project builds.
- **Creusot_CLI**: Creusot's command-line interface (for example `cargo creusot prove`). External; changed only through small patches (R7.9).
- **why3find**: the external Why3 proof driver that generates goals, applies tactics, runs provers, keeps the `.why3find` cache and writes `proof.json`. The System uses a why3find build with the dispatch-mode patch (AS-37, R7.9).
- **dispatch mode**: the mode `why3find prove --dispatch` that the why3find patch adds (design decision D-3, AS-37). In dispatch mode why3find still generates Goals, runs its per-goal strategy and tactics (such as `split_vc`) and writes the legacy part of `proof.json`, but sends every prover call and every access to its own cache to the Front_End and waits for the answer; it also answers the Front_End's requests to print a Goal with a given Driver and Transformation_Pipeline (with the number of tasks), to print the untransformed Why3 task, to report the detected provers and to report a prover's Calibration. The exchange uses the `dispatch` format of R7.10 over a pipe between two processes.
- **Proof_Server**: the endpoint that the cluster option names (R1.1) and to which the Front_End submits Jobs. The Coordinator plays this role, in place of the central proxy of why3find's proof-server mode (`why3find server`), which the System does not use (design decision D-1).
- **Front_End**: the component (`dp-frontend`) on the Rust developer's machine that the Creusot_CLI backend option starts (AS-2), that runs why3find in dispatch mode and answers its prover calls and cache accesses, that runs the Local_Fast_Pass, decides escalation, receives Results, holds the Verdict_Recorder and writes `proof.json`.
- **Coordinator**: the Rust service (`dp-coordinator`), separate from why3find, that plays the Proof_Server role: it queues Jobs, assigns them to Workers by their offers and Calibrations, and forwards Results and reports (design decision D-1). Untrusted.
- **Worker**: a machine or process (`dp-worker`) that runs Solver_Configurations on Jobs. A Worker uses why3find's prover configuration and Calibration, reports why3find's Calibrations in its offer, and starts each solver process itself, in its own process group, with the command line of the pinned Why3 prover configuration (design decisions D-3 and D-16). Untrusted, except as listed in the TCB_Document for proved-standalone Results.
- **Cluster**: the Coordinator together with its registered Workers.
- **Scheduler**: the decision logic in dp-core that decides escalation, Hard_Goal classification, job launch, race termination and cancellation.
- **Verdict_Recorder**: the decision logic in dp-core, run inside the Front_End, that accepts or discards Results, assigns Assurance_Labels and records Verdicts.
- **Lemma_Pool**: the service that stores and forwards Lemmas and Instantiation_Hints between jobs. Untrusted.
- **Admission_Checker**: the decision logic in dp-core that decides whether a Lemma or Instantiation_Hint may enter a target job, including Import hygiene.
- **Canonicalizer**: the component that assigns Canonical_Names to the declarations and hypotheses of a Printed_Goal.
- **Certificate_Composer**: the component that combines a target proof and Lemma proofs into one Composed_Certificate.
- **Certificate_Checker**: an independent proof checker run on a Composed_Certificate: Ethos for CPC proofs, Carcara for Alethe proofs. Trusted for the label certified.
- **Certificate_Store**: a content-addressed local directory, `.dp-certs/` in the project directory, holding Composed_Certificates, the Printed_Goals they prove, checker identities and outputs, and the Provenance_Records of the imported Lemmas and Instantiation_Hints (R5.4).
- **Replay_Tool**: the offline tool that re-checks proof-labelled Verdicts in `proof.json` without the Cluster.
- **Result_Cache**: the cache of Results keyed by Cache_Key, stored in `.dp-cache/` next to `.why3find` in the project directory; entries of Results labelled proved-standalone are also written to why3find's own cache layout inside `.why3find` (R7.5 criterion 7).
- **Format_Codec**: the parsers and printers for every exchanged format (R7.10).
- **Corpus_Exporter**: the tool that exports hard Creusot goals as SMT-LIB files with metadata.
- **Experiment_Runner**: the scripts that run the evaluation and generate the report.
- **Traceability_Checker**: the CI tool that checks the Traceability_Table against requirements, artifacts and CI results.
- **Canary_Suite**: the set of Canary_Goals, Injected_False_Lemmas and malformed Instantiation_Hints run in CI and in the evaluation.
- **CI**: the continuous integration system of the new repository, including scheduled jobs. The design uses GitHub Actions (design decision D-15); TB-18 covers whichever system is used.
- **CI corpus**: the subset of a corpus release that the CI selection rule of the corpus configuration selects, together with every Canary_Goal of the release (R6.1 criterion 10).
- **Goal**: a verification goal generated by why3find from Creusot's Coma output, identified by its why3find goal path (file, theory, goal name, transformation path).
- **Goal_Hash**: a cryptographic hash of the Why3 task of a Goal at its why3find goal path, before any Transformation_Pipeline or Driver of a Solver_Configuration is applied (AS-33).
- **Goal_Version**: a Goal together with its Goal_Hash. One Goal_Version has one Printed_Goal and one Task_Hash for each pair of Driver and Transformation_Pipeline.
- **Driver**: the Why3 prover driver that prints a Why3 task into a prover's input language, identified by the hash of the driver files.
- **Transformation_Pipeline**: the ordered list of Why3 transformations applied to a task before printing, identified by a hash of the list and the transformation versions.
- **Printed_Goal**: the exact text a Driver produces for a Goal_Version after a Transformation_Pipeline, as sent to a prover. The Printed_Goal of a Job or Result is determined by its Goal_Version and the Driver and Transformation_Pipeline of its Solver_Configuration. The *locally printed* Printed_Goal of a Result or Job of Goal_Version G, written `print_local(G)`, is the Printed_Goal that the Front_End prints for G on the local machine with that Driver and Transformation_Pipeline (R1.3 criterion 3). `print_local(G)` is defined only when that Transformation_Pipeline turns G's Why3 task into exactly one task (R1.3 criterion 10).
- **Task_Hash**: a cryptographic hash of a Printed_Goal together with its Driver hash and Transformation_Pipeline hash. The Task_Hash of a Job or Result is the Task_Hash of its Printed_Goal.
- **Assertion**: any SMT-LIB `assert` in a Printed_Goal, including hypotheses, axioms, definitions encoded as assertions, and the Negated_Goal_Assertion.
- **Hypothesis**: an Assertion other than the Negated_Goal_Assertion.
- **Negated_Goal_Assertion**: the Assertion that states the negation of the goal formula, so that an unsatisfiable Printed_Goal means the goal is proved.
- **Canonical_Name**: a name assigned by the Canonicalizer (R4.1) to an item of one of the five kinds of R4.1 criterion 5: a sort; a function or constant (including datatype constructors, selectors and testers); a Hypothesis; a Negated_Goal_Assertion; or a Partition_Assumption. A symbol defined by `define-fun`, `define-fun-rec` or `define-funs-rec` is named as a function or constant, and a sort defined by `define-sort` is named as a sort; the identical-declarations check (R4.1) of a defined symbol includes its definition body. Each Canonical_Name encodes its kind, and equal Canonical_Names imply equal kind and identical declarations or identical canonical formulas (R4.1 criterion 3).
- **Canonical_Vocabulary**: the set of Canonical_Names of the symbols declared in a Printed_Goal. Theory symbols of the Printed_Goal's SMT-LIB logic are not part of the Canonical_Vocabulary and are always allowed.
- **Sibling_Goals**: Goal_Versions generated from the verification conditions of the same Rust function, or of the same Rust module, as selected by the sharing scope option.
- **Local_Fast_Pass**: the first sequential pass of why3find's per-goal strategy, run on the Rust developer's machine with a short time budget (why3find's own default is 200 ms).
- **Escalated_Goal**: a Goal_Version that the Local_Fast_Pass did not prove and that has no Result_Cache entry that R7.1 criterion 6 labels with a proof label. A Goal_Version whose Result_Cache hits all have non-proof labels still escalates (R1.2 criteria 3 and 9).
- **Hard_Goal**: an Escalated_Goal that meets the configured hardness rule (R2.2) and therefore receives portfolio racing.
- **Solver_Configuration**: a named tuple of prover, pinned prover version, seed, option list, Driver and Transformation_Pipeline. The pinned prover version is the version of the Lock_File entry whose content hash equals the hash of the solver binary. A Why3 prover configuration may leave the version empty, as Creusot's does; a version that it states must equal the pinned one, and otherwise the Session stops as for a binary hash mismatch (R7.7 criterion 3).
- **Portfolio**: the set of Solver_Configurations raced on a Hard_Goal.
- **Job**: one run of one Solver_Configuration on one Goal_Version (or one Partition of a Goal_Version) on one Worker or on the local machine (Worker identity: the mark local). A *local Job* is a Job on the local machine: a Local_Fast_Pass run (R1.2) or a local fallback run (R1.6 criterion 5). Every Job has its own Job identifier, which names the Job's Session and Goal_Version (R1.5).
- **Job outcome**: the outcome with which the Verdict_Recorder counts a Job as ended: for a Job ended by its Result (R2.2 criterion 5), the Result's Assurance_Label, once assigned, if the Result reports a proof, and otherwise the Result's prover answer classified under R7.2 criterion 6 (Invalid stays Invalid); failed for a Job ended under R2.2 criterion 4, 24 or 25.
- **Race**: the set of Jobs that the Scheduler forms for one Escalated_Goal in one Session (R2.2 criterion 3), together with the Jobs that resubmission adds to it (R1.6 criterion 9) and, where partitioning is enabled, the Jobs of the Goal_Version's Partitions (R2.2 criterion 21). Every Escalated_Goal has exactly one Race (R2.2).
- **Session**: one invocation of proving by the Front_End, from start to the final write of `proof.json`.
- **Result**: the outcome message of one Job: prover answer, bound fields (R1.3), imports, and optional proof data.
- **Verdict**: the single final entry the Verdict_Recorder records for a Goal_Version in a Session.
- **Assurance_Label**: the trust label of a Result or Verdict. Proof labels: proved-standalone, certified. Non-proof labels: checked-incomplete, unchecked, Invalid, why3find's existing non-proof outcomes (failed, unknown, timeout), and unresolved, ranked by the information order of R7.2.
- **proved-standalone**: label of a Result that meets every condition of R7.1 criterion 2, which is the exact definition. In short: a Result of an Unpatched_Solver on the exact Printed_Goal of the whole Goal_Version, with no imported Lemmas or Instantiation_Hints and no import mark, outside certify-all mode.
- **certified**: label of a Result whose Composed_Certificate the Certificate_Checker accepted with no Trust_Steps in a run that took the locally printed Printed_Goal, and not the input problem recorded in the certificate, as its input problem, with every binding check of R5.3 passing, and whose Certificate_Store entry was completely stored no later than the Result's certification deadline (R5.3 criterion 17).
- **checker-accepted**: the state of a Result that needs certification after the Certificate_Checker outcome accept with every binding check passing (R5.3 criterion 4) and before the Verdict_Recorder labels the Result certified or unchecked (R5.3 criteria 17 and 18). Not an Assurance_Label.
- **checked-incomplete**: label of a Result whose Composed_Certificate the Certificate_Checker accepted only with Trust_Steps or holes (for Ethos, the outcome "incomplete"; for Carcara, holes count only after the Carcara part of AS-6 is confirmed, R5.3 criterion 15). Not a proof label.
- **unchecked**: label of a Result that reports a proof and needs certification (a Lemma_Dependent_Result, or any proof-reporting Result in certify-all mode; R5.1) but is labelled neither certified nor checked-incomplete under R5.3. Not a proof label.
- **unresolved**: the non-proof outcome for a Goal_Version that has no Result at the end of a Session.
- **Unpatched_Solver**: a solver binary whose hash appears in the lock file as an unmodified upstream release.
- **Sharing_Solver**: a solver binary built with patches that export or import Lemmas or Instantiation_Hints. Untrusted.
- **Lemma**: a formula exported by a solver during a Job, sent through the Lemma_Pool to other Jobs.
- **Ground_Lemma**: a Lemma that is closed and quantifier-free and mentions only symbols of the target's Canonical_Vocabulary and theory symbols.
- **Instantiation_Hint**: a pair of a Canonical_Name of a universally quantified Hypothesis and a tuple of ground terms, standing for the instance of that Hypothesis at those terms.
- **import mark**: the flag that the Front_End sets in a Job submission when the Job may receive Lemmas and Instantiation_Hints (R3.1 criterion 2).
- **Lemma_Dependent_Result**: a Result of a Job that carried the import mark (R3.1 criterion 5) or received at least one Lemma or Instantiation_Hint, or that ran a Sharing_Solver, or whose solver binary hash the Lock_File does not list as an Unpatched_Solver for its prover and version (R1.3 criterion 5).
- **Provenance_Record**: the record attached to a Lemma or Instantiation_Hint naming its source Goal_Version, source Job identifier, the source Job's base Task_Hash, Partition_Assumptions and SMT-LIB logic, Solver_Configuration, solver binary hash, Worker identity, Dependency_Set and imported Lemmas used. R3.2 criterion 1 lists every field.
- **Dependency_Set**: the set of Canonical_Names, each of one of the kinds of R4.1 criterion 5 (sort, function or constant, Hypothesis, Negated_Goal_Assertion, Partition_Assumption), of the declarations, Assertions and Partition_Assumptions that a Lemma depends on.
- **Partition**: a sub-problem of a Goal_Version obtained by adding Partition_Assumptions (for example, cube literals).
- **Partition_Assumption**: an Assertion added to a Goal_Version to form a Partition.
- **Checkable_Proof_Format**: a proof format with an independent checker in the lock file: CPC (checked by Ethos) or Alethe (checked by Carcara).
- **Composed_Certificate**: one checkable artifact that proves a target Printed_Goal unsatisfiable from its own Assertions, combining the target proof that uses imported Lemmas and instances with a proof of each imported Lemma from its Dependency_Set.
- **Trust_Step**: a step in a proof that the Certificate_Checker accepts without checking (for example, a trust or hole step).
- **Cache_Key**: the key under which the Result_Cache stores a Result (R7.5).
- **Calibration**: why3find's per-machine, per-prover measurement that scales reference time limits into local time limits.
- **Canary_Goal**: a Goal with a recorded invalidity witness that the System must never label proved.
- **Injected_False_Lemma**: a Lemma L that the Canary_Suite inserts into the Lemma_Pool for a target Goal_Version, with a recorded Dependency_Set Δ and recorded used Lemmas U, together with a recorded model of Δ ∪ U ∪ {¬L}, where Δ stands for the Assertions and Partition_Assumptions of the target problem that Δ names and U includes the instances of the Instantiation_Hints among the used Lemmas, confirmed by the independent evaluator of R7.4 criterion 2. The model shows that L does not follow from Δ ∪ U, also when the target goal is valid.
- **Fairness assumptions F1–F4**: the assumptions under which liveness is claimed (R1.5): F1 every sent message is eventually delivered or the sender's timeout fires; F2 every connected Worker eventually takes steps; F3 local clocks advance, so timeouts fire; F4 the Coordinator follows the protocol. Safety requirements do not rely on F1–F4.
- **Pinned_Dependency**: an external tool whose exact version and content hash are recorded in the lock file under `deps/`.
- **Lock_File**: the file under `deps/` that pins every Pinned_Dependency.
- **TCB_Document**: `docs/tcb.toml` and the generated `docs/tcb.md`, listing exactly what is trusted for each Assurance_Label (R7.8).
- **Traceability_Table**: `docs/traceability.toml` and the generated `docs/traceability.md`, mapping requirements to properties, artifacts and CI jobs (R7.11).

## Assumptions

Each assumption is either **Given** (stated by the project brief) or **To check** (a capability we believe exists but have not confirmed). A To-check assumption that turns out false changes the design, not the safety requirements: results that depend on a missing capability fall back to the label unchecked. An assumption that a design decision has made unnecessary keeps its ID and states that it is not relied on.

- **AS-1 (Given)** The new repository depends on Creusot, Why3, why3find, Alt-Ergo, Z3, cvc5, Ethos and Carcara as Pinned_Dependencies. Changes to them are kept small and upstreamable (R7.9).
- **AS-2 (To check)** Creusot's prove command (`cargo creusot prove`) can get, through a small upstreamable patch under `patches/creusot/`, a backend option with which Creusot_CLI runs `dp-frontend prove` with the arguments it would pass to `why3find prove`, forwards its output and returns its exit status, and without which Creusot_CLI runs unmodified why3find as today (design decision D-4). Creusot's documented CLI has no such option today.
- **AS-3 (To check)** The dp message protocol that the System defines for the Front_End, the Coordinator, the Workers and the Lemma_Pool (R7.10) can carry, within the size limits of R7.10 criterion 5, the data this project needs: Solver_Configurations with seeds and options, cancellation, Result binding fields and authentication tags (R1.3), Worker registration messages that carry solver binary hashes (R1.1), heartbeats and a Coordinator incarnation identifier (R1.6), operator commands (R1.7), and Lemma traffic. Since design decision D-1 (OQ-N1), no extension of why3find's proof-server protocol is relied on. Labels do not depend on AS-3: every message that matters for a label is authenticated end to end (R7.1 criterion 7) or re-derived locally (R1.3 criterion 3).
- **AS-4 (Given)** why3find's proof-server mode needs the OCaml `zmq` package and uses each client's prover Calibration to keep results consistent across machines. The System keeps why3find's Calibration, which Workers report in their offers (R1.1 criterion 3), and does not use the proof-server mode (D-1); the dp message protocol also runs over ZeroMQ, on TCP sockets.
- **AS-5 (To check)** cvc5 can emit CPC and Alethe proofs that cover quantifier instantiation, Skolemization and the theories Creusot goals use (uninterpreted functions, integers, algebraic datatypes, arrays, bit-vectors), without Trust_Steps for those theories.
- **AS-6 (To check)** Ethos and Carcara can check a proof whose leaves are a subset of the input problem's assertions, can check a Composed_Certificate in which imported Lemmas appear first as assumptions and then as derived facts, and check every universal-instantiation step (for example CPC `INSTANTIATE`, Alethe `forall_inst`) against its premise and terms rather than treating it as a Trust_Step. The instantiation canaries of R5.3 (P5.3.7) test the last point on every CI run. Ethos reports "incomplete" when a proof contains trust steps (Given); whether Carcara reports holes in an equivalent machine-readable way is To check (the *Carcara part* of AS-6). Until the TCB_Document records the Carcara part as confirmed, R5.3 criterion 15 labels unchecked every Alethe Composed_Certificate that contains a `hole` step or a rule outside the pinned Alethe rule list, whatever Carcara reports.
- **AS-7 (To check)** Solvers can export learned Lemmas and accept imported Lemmas during a run, through solver patches or existing interfaces (for example, Z3 clause callbacks, or the cvc5 changes used by SMT-D). If mid-run import is not available, Lemmas can still be imported at solver restarts.
- **AS-8 (To check)** Solvers can report the quantifier instantiations that contributed to a proof, so that Instantiation_Hints can be exported.
- **AS-9 (To check)** Why3 printing is deterministic: the same Goal_Version, Driver and Transformation_Pipeline always produce byte-identical Printed_Goals on every machine. why3find's existing task hash may or may not cover the printed text; the System computes Task_Hash over the printed text itself.
- **AS-10 (To check)** Z3 and cvc5 can run on Printed_Goals from one common Driver without a large loss of proving power, so that Lemmas can flow between Z3 and cvc5 Jobs. If not, Lemma exchange is limited to Jobs that use the same Driver (R3.1), which may mean the same prover.
- **AS-11 (To check)** Solvers or a proof-producing re-run can supply a Dependency_Set for an exported Lemma that is smaller than the full set of source Assertions. Without this, cross-goal Lemmas are almost always rejected because the default Dependency_Set includes the source's Negated_Goal_Assertion (R3.2).
- **AS-12 (To check)** A Lean 4 model of many-sorted first-order logic with the SMT-LIB theories used (as uninterpreted or axiomatized symbols where needed) is a faithful enough model of SMT-LIB semantics for the metatheorems in this document. The model's signatures include declared sorts, functions and constants, defined symbols (`define-fun`), recursive definitions (`define-fun-rec`, `define-funs-rec`), sort definitions (`define-sort`) and datatypes (`declare-datatypes`), together with the SMT-LIB logic of the problem. The model interprets underspecified theory operations as SMT-LIB 2.6 does: for example, integer `div` and `mod` by zero and real `/` by zero denote values that each model may choose, not Lean's `x / 0 = 0`. The Lean sanity theorems of P3.4.12 check both points. The gap is listed in the TCB_Document.
- **AS-13 (To check)** Creusot can verify the dp-core crate, and Kani can run bounded checks on the same crate, given the Rust features dp-core uses.
- **AS-14 (To check)** TLA+ trace validation (checking that an implementation trace is a behavior of the specification with TLC) is feasible for the Coordinator, Worker and Front_End event logs.
- **AS-15 (To check)** Goals exported from Creusot's test suite, from CreuSAT and from other selected projects may be redistributed under those projects' licenses.
- **AS-16 (To check)** Creusot's test suite and CreuSAT build and generate goals with the pinned Creusot version.
- **AS-17 (To check)** Solvers give the same answer for the same Printed_Goal, seed and options when the time limit is not reached, so that replay and instability measurements are meaningful.
- **AS-18 (To check)** Invalidity witnesses for Canary_Goals, and the models of Injected_False_Lemmas, can be produced mechanically and confirmed by an evaluator independent of the producing solver (R7.4 criterion 2), in two forms: (a) for a Canary_Goal that has a quantified Hypothesis, the default form, a model given as definitions (closed terms for constants, lambda terms for functions) together with a CPC proof, checked by the pinned Ethos, that each Assertion with the model substituted is valid; (b) finite evaluation of every Assertion, used only when every quantified variable ranges over an uninterpreted sort that the model interprets as a finite set. A proof of the goal's negation alone is not a witness, because it does not rule out inconsistent Hypotheses, under which the goal is valid. Form (a) relies on cvc5 producing, and Ethos checking, CPC proofs of the substituted Assertions; if that fails for a mutant, the mutant is excluded under R7.4 criterion 3.
- **AS-19 (To check)** `proof.json` can be extended with new fields such that unmodified pinned why3find reads every entry with a non-proof label (R7.2) as not proved, and reads an entry with a conflict field (R7.2 criterion 7) or an Invalid field (R7.2 criterion 11) as it reads the same entry without that field.
- **AS-20 (To check)** why3find's Calibration scaling of a reference time limit into a local time limit is a deterministic function of the Calibration value and the reference time limit, so that the Front_End can recompute the local time limit exactly (R1.4). If not, R1.4 criterion 7 needs a tolerance, which becomes a new TBD.
- **AS-21 (To check)** A Worker, which starts every solver process itself in its own process group (design decision D-16), can terminate a running solver process together with every process that solver process started, on cancellation (R1.5) and on expiry of the local time limit (R1.4), and the Front_End, which starts its local solver processes in the same way, can do the same at the per-goal deadline (R1.5 criterion 17). If not, the Worker or the Front_End runs each solver process in its own container.
- **AS-22 (To check)** why3find's first sequential pass applies the Local_Fast_Pass time budget to each solver process separately, and the budget can be set per invocation (R1.2).
- **AS-23 (To check)** Ethos and Carcara can check a sub-proof whose assumptions are discharged into an implication (CPC `SCOPE`, Alethe subproofs), so that a Composed_Certificate can show that each imported Lemma follows from the open assumptions that R5.2 criterion 5 allows (R3.2 criterion 8). If not, the open-assumption rule of R5.2 criterion 5 is checked only by `compose_plan` (A1, P3.2.2), not by the Certificate_Checker, and the scoped composition of R5.2 must be redesigned, for example by inlining each Lemma proof into the target proof.
- **AS-24 (To check)** The Canonicalizer can identify the Negated_Goal_Assertion in a Printed_Goal from the Driver output (for example, because the Driver prints the negated goal as the last assertion in a fixed form), so that a Provenance_Record can name it (R3.2 criterion 1, R4.1 criterion 6).
- **AS-25 (To check)** For Sibling_Goals printed with the same Driver and Transformation_Pipeline, Why3 prints the same symbol name for symbols that come from the same Why3 declaration, so that identical declarations (R4.1) are recognized across goals. If not, cross-goal Lemmas rarely match, which affects usefulness only.
- **AS-26 (To check)** The Driver output, or the Why3 task before printing, shows which Hypotheses are axioms of which Why3 declaration, so that the Canonicalizer can attribute axioms to declared symbols deterministically (R4.1, OQ-N45). Not relied on: design decision D-12 (OQ-N45 resolved) makes every axiom set empty (R4.1), so no axiom attribution is needed; the ID is kept for reference.
- **AS-27 (To check)** Every symbol that the pinned Z3 and cvc5 introduce during a run (Skolem symbols, fresh constants, purification and other internal symbols) and print in exported Lemmas, instantiation reports, models or proofs matches one of a finite list of solver-introduced name patterns that can be written as regular expressions (R4.1 criterion 9). The solver-name scan in R4.1 checks this on the CI corpus.
- **AS-28 (To check)** A Sharing_Solver can map each reported quantifier instantiation back to a Hypothesis of the input Printed_Goal and to one term for each variable of that Hypothesis's top-level binder, despite the solver's preprocessing of quantified formulas (for example prenexing, miniscoping or splitting), so that Instantiation_Hints name input Hypotheses (R4.4 criterion 1). AS-8 covers only the reporting of instantiations.
- **AS-29 (To check)** The why3find goal path of a Goal identifies the Rust function, and the Rust module, whose verification condition produced the Goal (R4.2 criterion 1, OQ-N13).
- **AS-30 (To check)** The pinned Ethos and Carcara can check a proof against an SMT-LIB input problem supplied as a separate file, and accept only proofs whose assumptions are assertions of that file after parsing, or can report the assumptions a checked proof used so that the Front_End can compare them with the locally printed Printed_Goal (R5.3 criterion 9). If neither holds for a checker, Results checked by that checker stay unchecked. For Ethos, which reads the input problem as a reference file, AS-38 covers the commands that a reference file may not contain.
- **AS-31 (To check)** why3find classifies every Why3 prover answer other than Valid and Invalid as one of failed, unknown and timeout, so that the Verdict_Recorder can reuse that classification (R7.2 criterion 6). If not, the classification becomes a fixed table in `docs/formats/`.
- **AS-32 (To check)** cargo-mutants reports each generated mutant with its function name and a mutation text that does not depend on line numbers, can restrict runs to named functions, and can apply one reported mutant so that another tool (a Creusot proof, a Kani harness or a named test) runs against it (R7.13 criteria 3 to 5). If not, the exclusion check applies the recorded mutation itself.
- **AS-33 (To check)** Why3 or why3find can compute a deterministic hash of a Goal's Why3 task before the Transformation_Pipeline and Driver of a Solver_Configuration are applied (for example, Why3's task checksum), equal on every machine for the same pinned inputs, so that the Goal_Hash identifies a Goal_Version independently of the Drivers in a Portfolio (OQ-N39). If not, the Goal_Hash is the Task_Hash of the Goal's Printed_Goal under one reference Driver and Transformation_Pipeline named in the project configuration. Soundness does not depend on the Goal_Hash: every Result is bound to its own Task_Hash, recomputed from the local print (R1.3 criterion 3).
- **AS-34 (To check)** In every proof that the pinned cvc5 produces, and that a Sharing_Solver patch produces for a Job with mid-run imports, each imported Lemma and each instance of an imported Instantiation_Hint appears as an `assume` step whose formula matches (R5.2) the imported Lemma text or the recomputed instance; and a proof-producing run on a Lemma problem (R5.1) produces a refutation of Δ ∪ U ∪ {¬L} in which every `assume` step matches a formula of that Lemma problem. If not, the affected Results are labelled unchecked with the reason target mismatch or lemma mismatch (R5.2 criteria 7 and 17); soundness does not depend on AS-34.
- **AS-35 (To check)** Unmodified pinned why3find reads cached prover results only from its own cache layout inside `.why3find`, so that a location exists under the project directory (for example, a directory next to `.why3find`) that unmodified pinned why3find never reads (R7.5 criterion 7). If not, the Result_Cache stores only proved-standalone entries and every other Result is not cached.
- **AS-36 (To check)** The pinned cvc5, run with the proof options in the project configuration (global DAG definitions and trust steps off), prints CPC and Alethe proofs whose commands are within the supported fragment of R5.2 (R5.1 criterion 11). If not, composition aborts with the reason banned command and the affected Results stay unchecked (R5.2 criterion 16); admitting `define` would change R5.2.
- **AS-37 (To check)** why3find can get a small upstreamable dispatch mode (`why3find prove --dispatch`, design decision D-3) in which every prover call and cache access of `why3find prove` goes to a parent process, while why3find keeps generating Goals, running its strategy and tactics on the answers and writing the legacy `proof.json`, and can answer print, task-count, task-print and Calibration requests (R1.3 criteria 3 and 10, R7.9, R7.12). If not, the Front_End must reimplement why3find's per-goal strategy, and the equivalence of R7.12 needs more testing.
- **AS-38 (To check)** The Printed_Goals that certifying Drivers produce contain no `define-fun-rec`, `define-funs-rec`, `push` or `pop`, which Ethos does not accept in a reference file (R5.3 criterion 1, AS-30). A certifying Driver is the Driver of a Solver_Configuration whose Results may need certification (R5.1). If not, such Goal_Versions cannot be certified in CPC and their Results stay unchecked, or the Driver must print recursive definitions as declarations with axioms.
- **AS-39 (To check)** why3find's Calibration of a prover applies when the Worker or the Front_End starts that prover's binary directly with Why3's command line (design decision D-16). Liveness and instability only (R1.4 Trust); RQ6 measures its effect (R6.2 criteria 9 and 15).
- **AS-40 (To check)** Sharing_Solver patches can print exported Lemmas and Instantiation_Hint terms without `let` (R3.3 criterion 5, R4.4 criterion 10; design decision D-14). If not, the export filters (R4.3 criterion 5, R4.4 criterion 4) drop most cvc5 Lemmas, which affects usefulness only.

## Non-goals

Each non-goal states the reason. Components that are out of scope for verification are trusted instead and listed in the TCB_Document (R7.8).

- **NG-1 Verifying Creusot's translation from MIR to Coma.** Reason: a separate research effort. Trusted instead.
- **NG-2 Verifying Why3's VC generation, transformations (such as `split_vc`), and Driver printing, including the printing and task-count path of the why3find dispatch patch (TB-23).** Reason: outside this project's scope; the certificates prove the Printed_Goal, not the Rust program. Trusted instead.
- **NG-3 Verifying the SMT solvers.** Reason: replaced by certificates for Lemma_Dependent_Results. Unpatched_Solvers remain trusted for proved-standalone, as in Creusot today.
- **NG-4 Verifying Ethos, Carcara, the Lean 4 kernel, the TLA+ tools, Kani, Creusot-as-verifier and the Rust compiler.** Reason: these are the checking tools themselves. Trusted instead (TB-3 to TB-9).
- **NG-5 Certificates for proofs by Alt-Ergo or Z3, or any solver without a Checkable_Proof_Format.** Reason: no independent checker for those formats is in the Lock_File. Lemma_Dependent_Results from such solvers stay unchecked unless the proof is re-produced by a solver with a Checkable_Proof_Format.
- **NG-6 Sharing quantified Lemmas other than Instantiation_Hints.** Reason: soundness and hygiene of general quantified Lemmas (fresh bound variables, Skolem symbols, triggers) are open; Ground_Lemmas and Instantiation_Hints cover the targeted cases. May become a later spec.
- **NG-7 Byzantine fault tolerance for proved-standalone Results from authenticated Workers.** Reason: requires certificates or re-execution for every result. The primary mitigation is offline replay (R7.6), which re-runs every proved-standalone Verdict on the auditor's machine; certify-all mode (R5.5, Should) is an optional second mitigation where it is implemented and enabled. Authenticated Workers are listed as trusted for proved-standalone in the TCB_Document.
- **NG-8 Liveness under a faulty Coordinator.** Reason: a Coordinator that withholds work cannot be forced to progress. Safety does not depend on the Coordinator, and the Front_End detects missing Results and falls back (R1.6).
- **NG-9 Multi-tenant fair-share scheduling, autoscaling, and a graphical interface.** Reason: not needed to answer the research questions. Scope only; no requirement cites NG-9, and it is not a trusted item.
- **NG-10 Why3 front ends other than Creusot.** Reason: the evaluation targets Creusot goals; other front ends may work but are not tested (R6.1 criterion 3).
- **NG-11 Performance guarantees on arbitrary hardware.** Reason: performance claims are empirical (A5) and tied to the recorded hardware.

## Trusted items

Trusted items that are not Assumptions (AS-n), Non-goals (NG-n) or Fairness assumptions (F1–F4) get a stable TB-n ID here. Trust subsections cite these IDs, `docs/tcb.toml` lists each of them with its reason and the requirements that rely on it, and `ci/tcb-check` matches the two (R7.8 criterion 8). TB-15, TB-20 and TB-21 concern evaluation numbers only, not any Assurance_Label.

- **TB-1** Collision resistance of the hash function used for Task_Hashes, Goal_Hashes, Lemma and Instantiation_Hint identifiers, import-set hashes, Cache_Keys and Certificate_Store content hashes.
- **TB-2** Unforgeability of the authentication scheme for Results, Lemmas and Instantiation_Hints (Ed25519 signatures with one key pair per Worker, design decision D-9), and secrecy of Worker credentials (R7.1 criterion 7, OQ-N11).
- **TB-3** Ethos, the CPC Certificate_Checker, at its Lock_File version and binary hash, including the CPC semantics it implements.
- **TB-4** Carcara, the Alethe Certificate_Checker, at its Lock_File version and binary hash, including the Alethe semantics it implements.
- **TB-5** The Lean 4 kernel, for the A1 metatheory.
- **TB-6** Creusot as a verifier, for the A1 contracts on dp-core and dp-formats.
- **TB-7** Kani, for the A2 bounded checks.
- **TB-8** TLC and Apalache, for the A2 protocol checks and trace validation.
- **TB-9** The Rust compiler and toolchain that build the System.
- **TB-10** Integrity of the local Result_Cache directory `.dp-cache/` and of the legacy layout inside `.why3find` for Result_Cache entries with the mark local, as for `.why3find` in why3find today.
- **TB-11** The checker option lists, the signature files they name, and the checker-output table recorded in the TCB_Document (R5.3 criterion 2, R7.8 criteria 3 and 4). No recorded option list may contain an option that makes a Certificate_Checker treat a rule outside its pinned rule set as a hole or as a trusted step (R5.3 criterion 16).
- **TB-12** The Solver_Configuration option lists passed to Unpatched_Solvers, for proved-standalone (OQ-N37).
- **TB-13** The cluster operator registers only intended Workers and provisions and stores Worker and operator credentials securely (design decision D-9), including the offline operator signing key that signs the versioned Worker registry and Worker revocations and whose verification key the local configuration of the Front_End and of each Worker holds (R1.3 criterion 9, R1.7), and the separate online operator command key that authenticates operator commands to the Coordinator (R1.7 criterion 11).
- **TB-14** The pinned SMT-LIB 2.6 standard: theory symbol tables of the logics in use and the reference term grammar (R3.3, R7.10).
- **TB-15** The statistics and plotting libraries of the Experiment_Runner, pinned in the Lock_File (R6.4).
- **TB-16** The bootstrap binaries of the pinning mechanism, which is Nix (R7.7; OQ-N10 resolved by design decision D-8).
- **TB-17** Integrity of the local file system between a binary hash check and the start of the executable (R7.7).
- **TB-18** The CI system reports job outcomes, and the commit on which each run ran, correctly (R7.11).
- **TB-19** cargo-mutants generates representative mutants (R7.13).
- **TB-20** Hardware isolation during evaluation runs, recorded in the run manifest but not checked mechanically (R6.3).
- **TB-21** Goals are exchangeable samples of the corpus, as the bootstrap and Wilson intervals assume (R6.2, OQ-N52).
- **TB-22** The Unpatched_Solver marks of the Lock_File, for proved-standalone, for the upstream release artifacts that R7.7 criterion 9 accepts by published checksum (OQ-N10 resolved by design decision D-8; an entry accepted by two rebuilds of the upstream tag in the pinned Nix build environment does not rest on TB-22): a marked binary is an unmodified upstream release. R7.7 criteria 8 and 9 check the marks against the patch set, against upstream-published checksums and against rebuilds of the upstream tags, and TB-22 covers what those checks cannot reach (for example, an upstream release artifact that was itself tampered with).
- **TB-23** The why3find dispatch patch returns exactly the bytes that the pinned Driver prints and the number of tasks that the pinned Transformation_Pipeline yields (R1.3 criteria 3 and 10, design decision D-3). Trusted for every label, as part of NG-2.

## Requirements

Requirements are grouped by stage. Group R7 holds cross-cutting requirements that every stage relies on; R7.1 (verdict soundness) is the central safety requirement.

---

**Group R1: Distribution**

### Requirement 1: R1.1 Goal distribution to a cluster through the Coordinator

**Priority:** Must

**Class:** Functional

**User Story:** As a Rust developer, I want Creusot's prove command to send hard goals to a cluster, so that proving is not limited to my local cores.

#### Acceptance Criteria

1. WHERE the cluster option is configured with a Proof_Server address (OQ-N17), WHILE the Front_End is connected to that Proof_Server and has not switched to the fallback (criterion 5, R1.6 criterion 4), THE Front_End SHALL submit to the Coordinator, over the dp message protocol (R7.10), every Escalated_Goal that has no Verdict, no Cluster submission in the Session and a Race whose deadline has not passed, as the Jobs of its Race that R2.2 criterion 6 selects, in Job submissions that each name the Goal's why3find goal path, the Goal_Version's Goal_Hash, the full Solver_Configuration that the Scheduler chose for the Job (R2.2 criterion 3), the Job's reference time limit and the Job's import mark (R3.1 criterion 2).
2. WHERE the cluster option is not configured, THE Creusot_CLI SHALL run every solver process of the Session on the local machine and SHALL attempt no connection to any Proof_Server (R7.12 defines equivalence with unmodified pinned why3find when portfolio racing and sharing are also disabled).
3. WHEN a Worker connects to the Coordinator, THE Coordinator SHALL record as that Worker's offer the provers, prover versions, solver binary hashes and per-prover Calibrations that the Worker reports, replacing any offer previously recorded for that Worker.
4. WHEN the Coordinator assigns a Job to a Worker, THE Coordinator SHALL choose only a Worker that is in the set of eligible Workers (R1.7) and whose offer most recently recorded under criterion 3 contains the prover and pinned version of the Job's Solver_Configuration together with a solver binary hash that the Lock_File lists for that Solver_Configuration (OQ-N21), and SHALL send that Worker the Job with the Solver_Configuration, reference time limit and import mark of the Job's submission (criterion 1) unchanged.
5. IF the Front_End has not completed a connection to the configured Proof_Server within `<TBD-30>` seconds after Session start, or the Proof_Server refuses the connection, THEN THE Front_End SHALL report in the Creusot_CLI output an error that names the configured Proof_Server address, and SHALL switch to the fallback of R1.6 for the rest of the Session, with no Cluster submission, so that the configured fallback option (`local` or `unresolved`, R1.6 criteria 5 and 6) decides the outcome of every Escalated_Goal (OQ-N20, OQ-N22).
6. IF the cluster option is set to a value that is not a well-formed Proof_Server address, THEN THE Front_End SHALL report a configuration error that names the rejected value, SHALL stop before starting any solver process, SHALL return a non-zero exit status to the Creusot_CLI, and SHALL leave `proof.json` and the `.why3find` cache unchanged (OQ-N18, OQ-N19).
7. IF the Coordinator holds an unassigned Job for which no Worker meets the condition of criterion 4, THEN THE Coordinator SHALL keep the Job queued without assigning it and SHALL report to the Front_End that the Job has no eligible Worker.
8. WHERE the cluster option is not configured, THE Front_End SHALL treat the Session as switched at Session start to the fallback with the fallback option `local`, whatever fallback option is configured, so that every Escalated_Goal runs on the local machine (R1.6 criteria 5 and 20, R2.2 criterion 20).
9. WHEN a Session starts, THE Front_End SHALL take the cluster option from the Creusot_CLI command-line flag if it is set, otherwise from the environment variable `DP_CLUSTER` if it is set, otherwise from `dp.toml` if it is set there, and SHALL treat the cluster option as not configured when none of the three sets it (OQ-N17 resolved by design decision D-13).
10. THE Front_End SHALL accept as a well-formed Proof_Server address (criterion 6) exactly a ZeroMQ TCP endpoint `tcp://<host>:<port>` whose host is a DNS name, an IPv4 literal or a bracketed IPv6 literal and whose port is a decimal integer within the valid TCP port range 1 to 65535, as the address grammar recorded in `docs/formats/` states (OQ-N18 resolved by design decision D-13).
11. THE Coordinator SHALL treat as listed for a Solver_Configuration under criterion 4 every solver binary hash that the Lock_File records for the prover and pinned version of that Solver_Configuration, one hash per platform that the Lock_File entry lists, so that each Result carries the hash of the binary that the Worker actually ran (R1.3 criteria 1 and 5; OQ-N21 resolved by design decision D-13).

#### Properties

- **P1.1.1** (Submission) For any Session trace τ with the cluster option configured, any Goal_Version G that the Scheduler marks as an Escalated_Goal at step t, and any step t_c ≥ t at which the Front_End is connected to the Proof_Server and not in the R1.6 fallback while G has no Verdict, no Cluster submission and a Race whose deadline has not passed: τ contains after t_c a submission event of a Job of G that carries G's why3find goal path and Goal_Hash and the Job's full Solver_Configuration, reference time limit and import mark, unless, after t_c and before any such event, G gets a Verdict, G's Race deadline passes, or the Front_End loses the connection or switches to the fallback. In particular, a Goal_Version escalated before the connection completes, or during a disconnect, is submitted after max(t, the next step at which the Front_End is connected).
- **P1.1.2** For any Job j with Solver_Configuration c assigned to Worker W at step t: `W ∈ eligible_t ∧ ∃ h ∈ lock(c) : (c.prover, c.version, h) ∈ offer_t(W)`, where `offer_t(W)` is the offer most recently recorded for W before t and `lock(c)` is the set of solver binary hashes that the Lock_File lists for c; and the Job that the Coordinator sends W carries the Solver_Configuration, reference time limit and import mark of j's submission.
- **P1.1.3** (Differential) For any Goal_Version G in the CI corpus, run once with the cluster option and once without, with all other configuration equal: G does not get a proof-labelled Verdict in one run while a Result with prover answer Invalid for G exists in the other run.
- **P1.1.4** For any Session trace τ with the cluster option not configured, τ contains no connection attempt to a Proof_Server and no Cluster submission, every solver-process start event in τ is on the local machine, and every Escalated_Goal of τ has a local solver-process start after its escalation step (criterion 8).
- **P1.1.5** For any Session trace τ in which the connection condition of criterion 5 fails at step t, τ contains a connection-error report naming the configured address and a switch to the fallback at t, contains no Cluster submission, and satisfies P1.6.6 from t: with the fallback option `local`, τ contains a local solver-process start for every Escalated_Goal of τ; with `unresolved`, every Escalated_Goal of τ has a non-proof Verdict at the later of t and its escalation step.
- **P1.1.6** For any cluster option value s that is not a well-formed Proof_Server address and any project state, running the prove command with s starts no solver process, ends with a non-zero exit status, and leaves `proof.json` and the `.why3find` cache byte-identical to their state before the run.
- **P1.1.7** For any step t and any unassigned Job j with Solver_Configuration c such that `¬∃ W ∈ eligible_t, h ∈ lock(c) : (c.prover, c.version, h) ∈ offer_t(W)`, j is not assigned at t, and the trace contains a no-eligible-Worker report for j to the Front_End at or after the first such t.
- **P1.1.8** (Option precedence) For any values f, e and d of the command-line flag, of `DP_CLUSTER` and of the cluster option in `dp.toml`, each set or unset, the Session's cluster option is f if f is set, otherwise e if e is set, otherwise d if d is set, and not configured otherwise.
- **P1.1.9** (Address grammar) For any string s, the Front_End accepts s as a well-formed Proof_Server address exactly when s is `tcp://h:p` with h a DNS name, an IPv4 literal or a bracketed IPv6 literal and p a decimal integer with 1 ≤ p ≤ 65535; every other s is rejected under criterion 6, so P1.1.6 applies to it.
- **P1.1.10** (Per-platform hashes) For any Solver_Configuration c whose Lock_File entry lists the hashes h_1, …, h_n for n platforms, `lock(c) = {h_1, …, h_n}` in P1.1.2 and P1.1.7.

#### Verification

- A4 — integration test on a local multi-container cluster (one Coordinator, at least two Workers) for criteria 1, 3, 4 and 7 — `tests/integration/cluster/` — `ci/integration-cluster`.
- A4 — integration test of cluster-off runs against a recording stub Proof_Server, including Escalated_Goals run locally (P1.1.4, criterion 8) — `tests/integration/cluster/cluster_off.rs` — `ci/integration-cluster`.
- A4 — integration test with the Proof_Server stopped, refusing connections, or silent for longer than `<TBD-30>` seconds, with both fallback options (P1.1.5) — `tests/integration/cluster/fallback.rs` — `ci/integration-cluster`.
- A4 — integration test of malformed cluster option values, checking exit status and byte-identical `proof.json` and `.why3find` (P1.1.6) — `tests/integration/cluster/config.rs` — `ci/integration-cluster`.
- A4 — differential test, distributed vs. local (P1.1.3) — `tests/diff/dist_vs_local/` — `ci/diff`.
- A4 — property-based test of the Worker assignment filter, with Lock_File entries that list several platform hashes, and the no-eligible-Worker case (P1.1.2, P1.1.7, P1.1.10) — `crates/dp-core/tests/prop_assign.rs` (proptest) — `ci/test`.
- A4 — property-based test of cluster option precedence over generated combinations of flag, `DP_CLUSTER` and `dp.toml` values, and of the address grammar over generated strings (P1.1.8, P1.1.9) — `crates/dp-formats/tests/prop_config.rs` — `ci/test`.
- A4 — P1.1.1, P1.1.2 and P1.1.7 asserted on deterministic simulation traces, including Goal_Versions escalated before the connection completes and during a disconnect shorter than `<TBD-7>` seconds (P1.1.1) — `tests/sim/` — `ci/sim`.

#### Trust

- AS-2 (the Creusot_CLI backend option), AS-3 (the dp message protocol; OQ-N1 resolved by design decision D-1), AS-4, AS-33 (Goal_Hash). Results from Workers are trusted only through R1.3 and R7.1.
- The Coordinator is untrusted to pass Solver_Configurations, reference time limits and import marks unchanged (criterion 4). R1.3 criterion 6 discards every Result whose pair of goal path and Solver_Configuration the Front_End did not submit, and R1.4 criterion 7 guards reference time limits.
- Workers report their own offers, and the Coordinator does not verify them. A false offer matters for soundness only if the Worker also reports a false solver binary hash in its Result. Such a Result would be a proved-standalone Result from an authenticated Worker, which the TCB_Document lists as trusted (NG-7).

### Requirement 2: R1.2 Escalation only after the local fast pass

**Priority:** Must

**Class:** Performance

**User Story:** As a Rust developer, I want goals that my machine proves in the fast pass to stay local, so that easy goals never pay cluster latency and the cluster serves only goals that need it.

The Local_Fast_Pass is why3find's first sequential pass only. The reference-time parallel pass of why3find's strategy runs, for an Escalated_Goal, as that Goal_Version's Race (R2.2): on the Cluster, and on the local machine only without the cluster option (R1.1 criterion 8) or after the switch to the fallback with the option `local` (R1.6 criterion 5) (OQ-N8 resolved by design decision D-17).

In certify-all mode (R5.5), every Goal_Version that criterion 4 does not exclude still runs the Local_Fast_Pass (criterion 1). A Local_Fast_Pass Result that reports a proof then needs certification (R5.1, R5.5 criterion 1) and can be labelled at most certified (R5.5 criterion 2). The Front_End certifies that Result on the local machine before escalation (criterion 8): if the Result is labelled certified, the Goal_Version stays local under criterion 2; only if every such Result ends unchecked or checked-incomplete does the Goal_Version escalate (criterion 9). Criterion 4 excludes a Goal_Version in that mode only for a cache entry that R7.1 criterion 6 labels certified, because that labelling uses the current certify-all setting (OQ-N51 resolved).

#### Acceptance Criteria

1. WHEN the Front_End starts proving a Goal_Version that has no proof-labelled Result_Cache hit (criterion 4), THE Scheduler SHALL run the Local_Fast_Pass for that Goal_Version on the local machine before any Cluster submission of that Goal_Version.
2. WHEN the Verdict_Recorder labels a Local_Fast_Pass Result for a Goal_Version proved-standalone, or certified after the local certification of criterion 8, THE Scheduler SHALL exclude that Goal_Version from every Cluster submission in the Session.
3. WHERE certify-all mode is disabled, WHEN the Local_Fast_Pass of a Goal_Version ends without a Result that the Verdict_Recorder labels proved-standalone, THE Scheduler SHALL mark that Goal_Version as an Escalated_Goal, whatever the non-proof outcomes of the Local_Fast_Pass were (failed, unknown, timeout, Invalid, or a local solver crash; OQ-N8 resolved by design decision D-17, OQ-N23).
4. WHEN the Result_Cache holds, under a Cache_Key of a Goal_Version, an entry that the Verdict_Recorder labels proved-standalone or certified under R7.1 criterion 6, THE Scheduler SHALL exclude that Goal_Version from the Local_Fast_Pass and from Cluster submission.
5. THE Scheduler SHALL read the Local_Fast_Pass time budget from configuration, with default value `<TBD-1>`.
6. THE Scheduler SHALL start every solver process of the Local_Fast_Pass with a time limit no greater than the Local_Fast_Pass time budget.
7. IF the configured Local_Fast_Pass time budget is not a positive duration, THEN THE Front_End SHALL report a configuration error that names the rejected value, SHALL stop before starting any solver process, SHALL return a non-zero exit status to the Creusot_CLI, and SHALL leave `proof.json` and the `.why3find` cache unchanged.
8. WHERE certify-all mode is enabled, WHEN the Verdict_Recorder accepts a Local_Fast_Pass Result that reports a proof, THE Front_End SHALL certify that Result on the local machine under R5.1 to R5.4, with proof-producing local runs, before any Cluster submission of the Result's Goal_Version.
9. WHERE certify-all mode is enabled, WHEN the Local_Fast_Pass of a Goal_Version has ended and the Verdict_Recorder has labelled every Local_Fast_Pass Result of that Goal_Version that reports a proof unchecked or checked-incomplete (vacuously when no such Result exists), THE Scheduler SHALL mark that Goal_Version as an Escalated_Goal, whatever the non-proof outcomes of the Local_Fast_Pass were.

#### Properties

- **P1.2.1** For any Session trace τ, `{G | the Verdict_Recorder labels a Local_Fast_Pass Result for G proved-standalone, or certified under criterion 8, in τ} ∩ {G | G is submitted to the Cluster in τ} = ∅`.
- **P1.2.2** For any Session trace τ and any Goal_Version G submitted to the Cluster at step t, the Local_Fast_Pass for G ended at a step before t without a proved-standalone Result, and, with certify-all enabled, every Local_Fast_Pass Result for G that reports a proof was labelled unchecked or checked-incomplete at a step before t.
- **P1.2.3** For any Session trace τ and any Goal_Version G with a proof-labelled Result_Cache hit (criterion 4), G appears in neither the Local_Fast_Pass nor a Cluster submission of τ.
- **P1.2.4** (Partition) For any Session trace τ that ends with the final write of `proof.json` and any Goal_Version G that the Front_End started proving in τ, exactly one of the following holds: (a) G has a proof-labelled Result_Cache hit; (b) a Local_Fast_Pass Result for G is labelled proved-standalone, or, with certify-all enabled, certified under criterion 8; (c) the Scheduler marks G as an Escalated_Goal after the Local_Fast_Pass for G ended.
- **P1.2.5** For any Session trace τ and any Local_Fast_Pass solver-process start event e in τ, `e.time_limit ≤ b`, where b is the configured Local_Fast_Pass time budget, or `<TBD-1>` when none is configured.
- **P1.2.6** For any Local_Fast_Pass time budget value s that is not a positive duration and any project state, running the prove command with s starts no solver process, ends with a non-zero exit status, and leaves `proof.json` and the `.why3find` cache byte-identical to their state before the run.
- **P1.2.7** (Certify-all local certification) With certify-all enabled, for any Session trace τ and any Local_Fast_Pass Result r of Goal_Version G that reports a proof: r's label (certified, checked-incomplete or unchecked) is assigned under R5.1 to R5.3 from runs on the local machine before any Cluster submission of G; G is submitted to the Cluster only if every such r of G is labelled unchecked or checked-incomplete; and if some such r is labelled certified, G is never submitted to the Cluster in τ.

#### Verification

- A1 — Creusot contracts on the escalation decision function for P1.2.1–P1.2.4 and P1.2.7 — `crates/dp-core/src/escalation.rs` — `ci/creusot`.
- A2 — TLA+ invariants `NoEscalationAfterLocalProof` (P1.2.1), `FastPassBeforeSubmission` (P1.2.2) and `CacheHitStaysLocal` (P1.2.3) — `verification/tla/Coordinator.tla` with TLC — `ci/tlc`.
- A4 — property-based test over generated Session event sequences, with certify-all enabled and disabled (P1.2.1–P1.2.5, P1.2.7) — `crates/dp-core/tests/prop_escalation.rs` — `ci/test`; the same properties asserted on deterministic simulation traces — `tests/sim/` — `ci/sim`.
- A4 — integration test of Local_Fast_Pass time limits and of malformed budget values, checking exit status and byte-identical `proof.json` and `.why3find` (P1.2.5, P1.2.6), and of certify-all Sessions in which a cvc5 Local_Fast_Pass proof is certified locally and its Goal_Version stays local, and in which a Local_Fast_Pass proof without a Checkable_Proof_Format escalates (P1.2.7) — `tests/integration/fast_pass/` — `ci/integration-cluster`.
- A5 — escalation experiment (RQ7, R6.2 criterion 10): share of goals that the Local_Fast_Pass proves and Cluster latency avoided — `eval/scripts/rq7_escalation/` — `sched/eval`.

#### Trust

- Local Unpatched_Solvers used by the Local_Fast_Pass are trusted for proved-standalone, as in why3find today (NG-3). In certify-all mode, a Goal_Version stays local only after a certified Local_Fast_Pass Result (criteria 2 and 8), which rests on the items listed for certified under R5.3.
- AS-22 (why3find applies the budget to each solver process separately).

### Requirement 3: R1.3 Result binding and authentication

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As an auditor, I want every remote Result tied to the exact Printed_Goal, solver and Worker that produced the Result, so that a faulty Coordinator or network cannot attach a proof to the wrong goal.

In this requirement, a *local Result* is a Result of a local Job (Glossary: Job); its Worker identity is the mark local, and the Front_End fills the fields of criterion 1 in place of a Worker. Criteria 3, 5, 7, 8 and 10 and the Task_Hash part of criterion 4 apply to local Results. Criteria 2, 6, 9 and 11 and the authentication part of criterion 4 (authentication tag, registered and unrevoked Worker) do not apply to local Results; they apply to Results received from the Cluster and, through R7.5 criterion 4, to Result_Cache entries that carry a Worker identity.

#### Acceptance Criteria

1. THE Worker SHALL include in every Result the why3find goal path of the Goal_Version, the Task_Hash of the Printed_Goal the solver received, the full Solver_Configuration (OQ-N43), the solver binary hash, the hash of the imported Lemma and Instantiation_Hint set (the hash of the empty set when there are none), the Worker identity and a unique Result identifier.
2. THE Worker SHALL authenticate every Result with the Worker's operator-provisioned credential, using an authentication tag computed over the canonical printed form (R7.10) of every other field of the Result.
3. WHEN the Verdict_Recorder receives a Result, THE Verdict_Recorder SHALL recompute the Task_Hash by printing locally the Goal named by the Result's goal path with the Driver and Transformation_Pipeline of the Result's Solver_Configuration (through why3find in dispatch mode, AS-37, TB-23), adding, where partitioning is enabled and the Result is for a Partition, the Partition_Assumptions that the recorded Partition tree gives that Partition (R3.5 criterion 4).
4. IF a Result fails authentication, names a Worker that is unregistered or revoked under criterion 9, or carries a Task_Hash that differs from the recomputed Task_Hash, THEN THE Verdict_Recorder SHALL discard the Result, log the reason, and leave the state of the Goal_Version unchanged.
5. IF a Result carries a solver binary hash that the Lock_File does not list as an Unpatched_Solver for the prover and version of the Result's Solver_Configuration, THEN THE Verdict_Recorder SHALL treat the Result as a Lemma_Dependent_Result.
6. IF a Result received from the Cluster names a pair of goal path and Solver_Configuration that the Front_End did not submit to the Cluster in the Session, THEN THE Verdict_Recorder SHALL discard the Result, log the reason, and leave the state of every Goal_Version unchanged.
7. IF the import-set hash in a Result differs from the hash that the Verdict_Recorder computes over the imported Lemma and Instantiation_Hint set listed in the Result, THEN THE Verdict_Recorder SHALL discard the Result, log the reason, and leave the state of the Goal_Version unchanged.
8. IF a Result message fails to parse under R7.10 or exceeds the size limit of R7.10 criterion 5, THEN THE Verdict_Recorder SHALL discard the message, log the reason, and leave the state of every Goal_Version unchanged.
9. THE Verdict_Recorder SHALL take the registered Worker identities and their verification data only from a Worker registry signed with the offline operator signing key (design decision D-9) whose verification key the Front_End's local configuration holds, SHALL apply a Worker revocation only if the revocation carries a valid signature by that operator key, whatever channel delivered the registry or the revocation, and SHALL apply no Worker registration or revocation that a Coordinator message states without such a signature (TB-13, OQ-N11, OQ-N35).
10. IF applying the Transformation_Pipeline of a Result's Solver_Configuration to the Why3 task of the Goal named by the Result's goal path yields anything other than exactly one task (as why3find in dispatch mode reports the task count, AS-37, TB-23), THEN THE Verdict_Recorder SHALL discard the Result, log the reason, and leave the state of the Goal_Version unchanged.
11. WHEN the Front_End holds more than one Worker registry with a valid signature by the operator signing key, THE Verdict_Recorder SHALL take the registered Worker identities and their verification data from the registry with the highest version, whatever channel delivered each registry and in whatever order (design decision D-9).

#### Properties

- **P1.3.1** For any Session, any Goal_Version G and any Result r that the Verdict_Recorder accepts for G: `r.goal_path = G.path ∧ r.task_hash = H(print_local(G.path, r.config.driver, r.config.pipeline) ⊕ PA(r))`, where `print_local` prints G's task (Goal_Hash `G.goal_hash`) on the local machine with r's Driver and Transformation_Pipeline, and `PA(r)` is the list of Partition_Assumptions that the recorded Partition tree gives r's Partition where partitioning is enabled, and empty for a Result for the whole Goal_Version. G has one such hash per Driver and Transformation_Pipeline (Glossary, OQ-N39).
- **P1.3.2** For any Result r, if `verify(cred(r.worker), r.tag, canon(r ∖ tag))` fails, or `r.worker` is unregistered or revoked in the Front_End's view at the step where the Verdict_Recorder processes r, then r is not accepted. The Front_End's view is the operator-signed Worker registry and the operator-signed revocations that the Front_End holds at that step (criterion 9).
- **P1.3.3** For any message schedule produced by an adversarial Coordinator or network (reordering, duplication, replay of old Results, swapping goal identifiers, forged messages), the set of accepted Results is a subset of the authentic Results that Workers produced for the matching Task_Hash.
- **P1.3.4** (Discard is a no-op) For any Verdict_Recorder state s and any input x that criterion 4, 6, 7, 8 or 10 discards, `step(s, x)` equals s except for one appended log entry.
- **P1.3.5** For any accepted Result r with `(r.config.prover, r.config.version, r.binary_hash) ∉ unpatched(Lock_File)`, r is classified as a Lemma_Dependent_Result, so P7.1.3 applies to r.
- **P1.3.6** For any Session S and any Result r received from the Cluster and accepted in S, `(r.goal_path, r.config) ∈ submitted(S)`, where `submitted(S)` is the set of pairs of goal path and full Solver_Configuration carried by the Job submissions of S (R1.1 criterion 1). Every Solver_Configuration in `submitted(S)` is one that the Scheduler chose under R2.2 criterion 3, so no Solver_Configuration that the Coordinator chooses can be accepted.
- **P1.3.7** For any accepted Result r, `r.imports_hash = H(canon(r.imports))`.
- **P1.3.8** (Tamper) For any authentic Result r and any r′ with `r′.tag = r.tag` that differs from r in at least one other field, r′ is not accepted.
- **P1.3.9** (Operator-signed registry) For any Front_End trace and any step t, the Worker identities and verification data that the Verdict_Recorder treats as registered at t are those of the operator-signed Worker registry that the Front_End holds at t, and the credentials it treats as revoked are those of the operator-signed revocations that the Front_End received by t; and for any Coordinator message m that registers a Worker or revokes a credential without a valid operator signature, the registered and revoked sets after m equal those before m.
- **P1.3.10** (Single task) For any Result r that the Verdict_Recorder accepts, applying `r.config.pipeline` to the Why3 task at `r.goal_path` yields exactly one task; and for any Result whose Transformation_Pipeline yields zero tasks or several, the Result is discarded under criterion 10.
- **P1.3.11** (Registry version) For any Front_End trace, any step t and any set R of Worker registries with a valid operator signature that the Front_End holds at t, received in any order and over any channels, the registered Worker identities and verification data that the Verdict_Recorder uses at t are those of the element of R with the highest version.

#### Verification

- A1 — Creusot contracts on `accept_result` with P1.3.1, P1.3.2, P1.3.4–P1.3.7 and P1.3.9–P1.3.11 as postconditions — `crates/dp-core/src/verdict.rs` — `ci/creusot`.
- A3 — per-Result recomputation of the Task_Hash and the import-set hash, the single-task check of the Transformation_Pipeline, the authentication check, and the operator-signature check of the Worker registry and of revocations, in the Front_End before every acceptance — `crates/dp-frontend/src/accept.rs` — `ci/sim`, `ci/canaries`.
- A4 — property-based test of `accept_result` over generated Results with single-field mutations, splitting Transformation_Pipelines, Partition Results with added, dropped or swapped Partition_Assumptions (P1.3.1), unsigned or wrongly signed registrations and revocations, and several signed registry versions in every arrival order (P1.3.1, P1.3.4–P1.3.11) — `crates/dp-core/tests/prop_accept.rs` — `ci/test`.
- A4 — fuzzing of the Result message parser in front of `accept_result` (criterion 8) — `fuzz/result_message/` — `ci/fuzz-smoke`, `sched/fuzz-nightly`.
- A4 — deterministic simulation with an adversarial Coordinator, including a Coordinator that injects a self-registered Worker with its own credential and a forged revocation (P1.3.3, P1.3.9) — `tests/sim/adversarial_coordinator.rs` — `ci/sim`, `sched/sim-nightly`; mutation testing of `accept_result` (cargo-mutants) — `tools/mutants/` — `sched/mutants-weekly`.
- A4 — pipeline canaries (R7.4) for criterion 10 (P1.3.10) — `tests/canaries/` — `ci/canaries`.

#### Trust

- TB-1 (collision resistance of the hash function); TB-2 (authentication scheme and secrecy of Worker credentials); TB-13 (the operator signing key of the Worker registry and revocations); AS-9 (deterministic printing); AS-3 (the dp message protocol carries the bound fields and the authentication tag); AS-37 and TB-23 (why3find in dispatch mode prints `print_local` and reports the task count for criteria 3 and 10, returning exactly the bytes that the pinned Driver prints and the number of tasks that the pinned Transformation_Pipeline yields).
- Authenticated Workers are trusted to report the solver binary hash and the imported set truthfully. A false report can only produce a wrong proved-standalone label, and the TCB_Document lists authenticated Workers as trusted for that label (NG-7).
- A Coordinator cannot register a Worker of its own in the Front_End's view (criterion 9). It can withhold a signed revocation or a newer signed registry, which delays the effect of a revocation as described under R1.7 Trust (OQ-N11).

### Requirement 4: R1.4 Calibrated time limits across Workers

**Priority:** Must

**Class:** Robustness

**User Story:** As a proof engineer, I want time limits scaled to each Worker's measured speed, so that a proof found on one machine can be replayed on another.

Priority note: R2.3, R7.5 and R7.6 need the Calibration fields that criterion 2 records, so this requirement is Must. The Result_Cache rule of criterion 7 is its least critical part.

#### Acceptance Criteria

1. WHEN the Coordinator assigns a Job to a Worker, THE Coordinator SHALL set the Job's local time limit to the Calibration scaling of the Job's reference time limit, using the Calibration for the Job's prover in the Worker's offer most recently recorded under R1.1 criterion 3.
2. THE Worker SHALL record in every Result the Calibration identifier, the Calibration value, the reference time limit and the local time limit used.
3. IF the offer most recently recorded for a Worker has no Calibration for a prover, or has a Calibration value for that prover that is not a positive finite number, THEN THE Coordinator SHALL assign no Job for that prover to that Worker until the Worker reports an offer with a positive finite Calibration value for that prover.
4. IF the Coordinator holds an unassigned Job for which at least one Worker meets R1.1 criterion 4 and criterion 3 excludes every such Worker, THEN THE Coordinator SHALL keep the Job queued and SHALL report to the Front_End that the Job has no eligible Worker.
5. WHEN a Job's local time limit elapses before the solver answers, THE Worker SHALL terminate the solver process of that Job and every process that solver process started within `<TBD-33>` seconds, and SHALL report the answer timeout for that Job.
6. WHEN the Verdict_Recorder accepts a Result under R1.3, THE Verdict_Recorder SHALL recompute the local time limit from the Calibration value and the reference time limit recorded in the Result.
7. IF the recomputed local time limit differs from the local time limit recorded in a Result, or the reference time limit recorded in the Result differs from the reference time limit that the Front_End submitted for that Job, THEN THE Verdict_Recorder SHALL log the mismatch and SHALL store no non-proof answer of that Result in the Result_Cache (OQ-N25).

#### Properties

- **P1.4.1** For any Job j that the Coordinator assigns to Worker W for prover p at step t, `j.local_limit = scale(cal_t(W, p), j.reference_limit)`, where `cal_t(W, p)` is the Calibration value for p in `offer_t(W)`.
- **P1.4.2** For any step t, Worker W and prover p such that `offer_t(W)` has no Calibration for p, or has a Calibration value for p that is not positive and finite, no Job for p is assigned to W at t.
- **P1.4.3** (Scaling arithmetic) For any positive finite Calibration value c and any reference time limits r1 ≤ r2 of the limit type, `scale(c, r1)` and `scale(c, r2)` are computed without overflow and `scale(c, r1) ≤ scale(c, r2)`.
- **P1.4.4** For any Result r whose non-proof answer the Verdict_Recorder stores in the Result_Cache, `r.local_limit = scale(r.calibration_value, r.reference_limit)` and `r.reference_limit` equals the reference time limit that the Front_End submitted for r's Job.
- **P1.4.5** For any Job j whose local time limit elapses at time t on a Worker before the solver answers, the trace contains, no later than t + `<TBD-33>` seconds, the termination of j's solver process and of every process that solver process started, followed by a Result for j with answer timeout.
- **P1.4.6** For any step t and any unassigned Job j for prover p such that some Worker meets R1.1 criterion 4 for j and every such Worker W lacks a positive finite Calibration for p in `offer_t(W)`, j is not assigned at t, and the trace contains a no-eligible-Worker report for j to the Front_End at or after the first such t.

#### Verification

- A4 — property-based test of limit scaling, the Calibration filter, the no-eligible-Worker case and the cache-storage rule (P1.4.1, P1.4.2, P1.4.4, P1.4.6) — `crates/dp-core/tests/prop_calibration.rs` — `ci/test`.
- A2 — Kani bounded check of `scale` for absence of overflow and for monotonicity (P1.4.3) — `verification/kani/calibration.rs` — `ci/kani`.
- A4 — integration test with two Workers with different Calibrations, one Worker without a Calibration, and a stub solver that ignores its time limit and starts child processes (criteria 1–5, P1.4.5) — `tests/integration/cluster/calibration.rs` — `ci/integration-cluster`.
- A4 — deterministic simulation with a Coordinator that reports wrong time limits (P1.4.4) — `tests/sim/adversarial_coordinator.rs` — `ci/sim`.
- A5 — run-to-run and cross-Worker instability experiment, and cross-Worker reproduction at the calibrated local time limit and at the unscaled reference time limit (RQ6, R6.2 criteria 9 and 15) — `eval/scripts/rq6_instability/` — `sched/eval`.

#### Trust

- AS-4 (why3find's Calibration procedure), AS-20 (exact recomputation of the scaling), AS-21 (termination of solver process trees), AS-39 (why3find's Calibration applies when the Worker starts the prover binary directly, design decision D-16; liveness and instability only, measured in RQ6).
- Time limits do not affect soundness. A wrong limit can only change a non-proof outcome or a Result_Cache entry, which criterion 7 guards.

### Requirement 5: R1.5 Coordinator protocol safety and liveness

**Priority:** Must

**Class:** Liveness

**User Story:** As a cluster operator, I want the coordination protocol model-checked and the implementation checked against the model, so that no goal is lost, no goal gets two Verdicts, losing Jobs stop, and every goal eventually gets a Verdict.

In this requirement, the *tracing retention setting* of the Front_End (the `tracing_retention` option of `dp.toml`), of the Coordinator and of a Worker is either bounded, the default, or complete. Event tracing (criterion 10) is on in every run whatever this setting is (OQ-N27 resolved by design decision D-11).

#### Acceptance Criteria

1. THE Verdict_Recorder SHALL record at most one Verdict per Goal_Version per Session.
2. WHEN the Front_End writes the final `proof.json` of a Session, THE Front_End SHALL hold a Verdict for every Goal_Version that the Front_End started proving in the Session, recording for each such Goal_Version that has no Verdict at that point the most informative non-proof outcome (R7.2 criterion 5) among that Goal_Version's Job outcomes and the Assurance_Labels of its accepted Results, or unresolved if there is none.
3. WHEN the Verdict_Recorder records a Verdict for a Goal_Version that has a Cluster submission in the Session, THE Front_End SHALL send the Coordinator a cancellation that names the Session and that Goal_Version.
4. WHEN the Coordinator receives a cancellation that names a Session and a Goal_Version, THE Coordinator SHALL remove every queued Job of that Session and Goal_Version from the queue and SHALL send a cancellation to every Worker that runs a Job of that Session and Goal_Version.
5. WHEN a Worker receives a cancellation for a running Job, THE Worker SHALL terminate the solver process of that Job and every process that solver process started within `<TBD-2>` seconds, and SHALL report the Job to the Coordinator as cancelled.
6. WHEN a Worker receives a cancellation for a Job that is not running on that Worker, THE Worker SHALL report the Job to the Coordinator as not running and SHALL leave the Worker's other state unchanged.
7. IF a Worker reports a Job as running after the Coordinator sent that Worker a cancellation for that Job, THEN THE Coordinator SHALL send the cancellation again.
8. WHILE a Goal_Version has a Verdict in a Session, THE Verdict_Recorder SHALL keep that Verdict unchanged for the rest of the Session.
9. WHEN `<TBD-3>` seconds have passed on the Front_End's local clock since the Scheduler marked a Goal_Version as an Escalated_Goal, and no other criterion gives that Goal_Version a Verdict at or before that step, THE Verdict_Recorder SHALL record for that Goal_Version the most informative non-proof outcome (R7.2 criterion 5) among its Job outcomes and the Assurance_Labels of its accepted Results, or unresolved if there is none, where `<TBD-3>` is at least `<TBD-9>` + `<TBD-14>` + `<TBD-15>`, the bound on a Race (P2.2.6); criterion 3 then cancels the Goal_Version's Cluster Jobs and criterion 17 terminates its local solver processes (OQ-N26 resolved).
10. THE Front_End, the Coordinator and every Worker SHALL emit structured event traces in which each event names exactly one action of `verification/tla/Coordinator.tla`, carries the values of that action's parameters, the emitting component's identity and a per-component sequence number, and, for a receive event, the identifier of the matching send event (OQ-N27).
11. WHEN the CI runs `ci/trace-validation`, THE CI SHALL check every event trace produced by `ci/sim` and `ci/integration-cluster` against `verification/tla/Coordinator.tla` and SHALL fail on any trace that is not a behavior of the specification.
12. IF a run of `ci/sim` or `ci/integration-cluster` produced no event trace, or produced a trace with an event that names no action of `verification/tla/Coordinator.tla`, THEN THE CI SHALL fail `ci/trace-validation`.
13. WHEN the Verdict_Recorder assigns a proof label to a whole-Goal_Version Result that belongs to no Race (a Local_Fast_Pass Result or a Result of a local fallback run, R1.6 criterion 5), or labels a Result_Cache entry with a proof label under R7.1 criterion 6, for a Goal_Version that has no Verdict and no Race that has not ended, THE Verdict_Recorder SHALL record the Verdict of that Goal_Version with that label and the identifier of that Result (for a Result_Cache entry, of the Result stored in the entry, R2.3 criterion 1).
14. WHEN the Front_End has no Goal_Version left to start proving in the Session and every Goal_Version that the Front_End started proving in the Session has a Verdict, THE Front_End SHALL write the final `proof.json` of the Session.
15. WHEN the Coordinator receives from a Worker a cancelled report for a Job (criterion 5), THE Coordinator SHALL forward the report to the Front_End of the Session that the Job identifier names.
16. WHEN the Front_End receives a cancelled report for an open Job (R2.2) of a Goal_Version that has no Verdict, THE Verdict_Recorder SHALL count that Job as ended with the outcome failed in the Race of that Goal_Version (R2.2 criterion 4).
17. WHEN the Verdict_Recorder records a Verdict for a Goal_Version under criterion 9, THE Front_End SHALL terminate, within `<TBD-45>` seconds, every local solver process that the Front_End started for that Goal_Version, including the local fallback runs of R1.6 criterion 5, and every process that such a solver process started.
18. WHERE the tracing retention setting is bounded, THE Front_End, the Coordinator and every Worker SHALL each keep the most recent `<TBD-46>` trace records of criterion 10 that the component emitted and SHALL discard older trace records (OQ-N27 resolved by design decision D-11).
19. WHERE the tracing retention setting is complete, which every CI job and every Experiment_Runner run sets, THE Front_End, the Coordinator and every Worker SHALL keep every trace record of criterion 10 that the component emitted (OQ-N27 resolved by design decision D-11).

#### Properties

- **P1.5.1** (AtMostOneVerdict) `□ ∀ G ∈ Goals : Cardinality(verdicts[G]) ≤ 1`.
- **P1.5.2** (NoLostGoals) `□ (finalWrite ⇒ ∀ G ∈ started : Cardinality(verdicts[G]) = 1)`, where `started` is the set of Goal_Versions that the Front_End started proving in the Session.
- **P1.5.3** (EventualVerdict) `(WF(Deliver) ∧ WF(Timeout) ∧ WF(WorkerStep) ∧ CoordinatorFollowsSpec) ⇒ ∀ G ∈ started : ◇ (verdicts[G] ≠ {})`, where the four conjuncts encode F1–F4.
- **P1.5.4** (Cancellation) Under F1–F4: `∀ S, G, j ∈ jobs(S, G) : (verdicts[S, G] ≠ {} ∧ (queued(j) ∨ running(j))) ~> ¬(queued(j) ∨ running(j))`, where `jobs(S, G)` is the set of Jobs whose Job identifier names Session S and Goal_Version G.
- **P1.5.5** (Trace conformance) Every run of `ci/sim` and `ci/integration-cluster` produces at least one event trace, and every such trace τ, mapped to actions as in criterion 10, is a behavior of `Init ∧ □[Next]_vars` of `Coordinator.tla`.
- **P1.5.6** (VerdictStable) `□ ∀ G, v : (verdicts[G] = {v}) ⇒ □ (verdicts[G] = {v})`.
- **P1.5.7** (BoundedVerdict) For any simulation trace τ in which the Front_End's local clock advances (F3), whatever the Coordinator, the network and the Workers do within the simulation fault model and the adversarial Coordinator model (F1, F2 and F4 not assumed), and any Escalated_Goal G of τ: `time(verdict(G)) − time(escalate(G)) ≤ <TBD-3>` seconds of the Front_End's local clock, with `<TBD-3> ≥ <TBD-9> + <TBD-14> + <TBD-15>` so that a Race, bounded by P2.2.6, fits.
- **P1.5.8** (Cancellation latency) For any event at which a Worker receives a cancellation for a running Job j at time t, the trace contains, no later than t + `<TBD-2>` seconds, the termination of j's solver process and of every process that solver process started, followed by a cancelled report for j.
- **P1.5.9** For any Worker state s and any cancellation c for a Job that is not running in s, the Worker state after handling c equals s, and the only output is a not-running report.
- **P1.5.10** (NonRaceProofRecorded) For any Session trace τ, any Goal_Version G and any step t at which the Verdict_Recorder assigns a proof label to a whole-Goal_Version Result r of G that belongs to no Race, or labels a Result_Cache entry e of G with a proof label, while G has no Verdict and no Race that has not ended: at every step from t on, G's Verdict carries that label and the identifier of r (or of the Result stored in e).
- **P1.5.11** (Final write) For any Session trace τ and any step t at which the Front_End has no Goal_Version left to start proving and every Goal_Version in `started` has a Verdict, τ contains the final write of `proof.json` at or after t.
- **P1.5.12** (Session isolation) For any cancellation naming Session S and Goal_Version G that the Coordinator processes at step t, every Job that leaves the queue at t and every Job for which the Coordinator sends a cancellation at t has a Job identifier that names S and G.
- **P1.5.13** (Cancelled reports) For any cancelled report for a Job j that the Coordinator receives, the Coordinator forwards the report to the Front_End of the Session that j's identifier names; and for any step at which that Front_End receives the report while j is open and j's Goal_Version has no Verdict, the Race of j's Goal_Version counts j as ended with outcome failed at that step.
- **P1.5.14** (Deadline cleanup) For any trace τ and any Goal_Version G whose Verdict the Verdict_Recorder records under criterion 9 at time t, τ contains, no later than t + `<TBD-45>` seconds, the termination of every local solver process that the Front_End started for G, including local fallback runs, and of every process that such a solver process started.
- **P1.5.15** (Trace retention) For any component c ∈ {Front_End, Coordinator, Worker} and any run of c in which c has emitted n trace records: with the bounded setting, the trace that c keeps holds exactly the last `min(n, <TBD-46>)` of those records in emission order; with the complete setting, it holds all n records in emission order.

#### Verification

- A2 — TLA+ specification with TLC at small bounds `<TBD-4>` for P1.5.1–P1.5.4, P1.5.6, P1.5.10 (invariant `NonRaceProofRecorded`), P1.5.12 and P1.5.13 — `verification/tla/Coordinator.tla` — `ci/tlc`; Apalache at larger bounds — `verification/tla/Coordinator.tla` — `sched/apalache-nightly`.
- A2 — trace validation of implementation traces against the specification (P1.5.5, criteria 11 and 12) — `verification/tla/traces/` — `ci/trace-validation`.
- A2 — Kani harnesses on the dp-core state-machine transitions for P1.5.1 and P1.5.6 — `verification/kani/coordinator.rs` — `ci/kani`.
- A4 — deterministic simulation with fault injection (crashes, partitions, delays, duplicate messages), asserting P1.5.1, P1.5.2, P1.5.6, P1.5.7 and P1.5.10–P1.5.14 on every trace, with P1.5.7 also asserted on traces with a silent or adversarial Coordinator and unbounded message loss (F3 only), and with a two-Session scenario in which two Front_Ends prove the same Goal_Version through one Coordinator (P1.5.12, P1.5.13) — `tests/sim/` — `ci/sim` (short), `sched/sim-nightly` (long).
- A4 — integration test of cancellation latency with a stub solver that ignores the termination request and starts child processes (P1.5.8) — `tests/integration/cluster/cancel.rs` — `ci/integration-cluster`.
- A4 — integration test of the per-goal deadline with the fallback option `local` and a stub local solver that ignores the termination request and starts child processes (criterion 17, P1.5.14) — `tests/integration/cluster/deadline.rs` — `ci/integration-cluster`.
- A4 — property-based test of the Worker cancellation handler (P1.5.9) — `crates/dp-core/tests/prop_cancel.rs` — `ci/test`.
- A4 — unit test of the trace writer of the shared shell adapters under both tracing retention settings, with more than `<TBD-46>` records for the bounded setting (P1.5.15, criteria 18 and 19) — `crates/dp-shell/` — `ci/test`.
- A4 — property-based test of Verdict recording over generated Session event sequences with Local_Fast_Pass Results, local fallback Results, Result_Cache hits and Races (P1.5.10, P1.5.11) — `crates/dp-core/tests/prop_record.rs` — `ci/test`.

#### Trust

- F1–F4 for liveness only (P1.5.3, P1.5.4). P1.5.7 needs only F3, because the Front_End enforces the deadline of criterion 9 on its local clock (OQ-N26). P1.5.1, P1.5.2 and P1.5.6 hold without F4 because the Front_End enforces them. AS-14, AS-21 (for Workers, and for the Front_End's local solver processes under criterion 17); TB-7 and TB-8 (Kani, TLC and Apalache).

### Requirement 6: R1.6 Worker, network and Coordinator faults

**Priority:** Must

**Class:** Robustness

**User Story:** As a cluster operator, I want Worker crashes, network faults and Coordinator restarts to delay goals without losing or corrupting goals.

In this requirement, D is the maximum message delay of the simulation fault model of `tests/sim/`.

#### Acceptance Criteria

1. IF the Coordinator receives no message from a connected Worker for `<TBD-5>` seconds, THEN THE Coordinator SHALL remove the Worker from the set of eligible Workers and SHALL mark every running Job of that Worker as failed with the reason heartbeat timeout (OQ-N29).
2. WHEN the Coordinator marks a Job as failed and the Coordinator has reassigned that Job fewer than `<TBD-6>` times, THE Coordinator SHALL queue the Job for reassignment under R1.1 criterion 4 (OQ-N28).
3. IF the Coordinator marks a Job as failed after the Coordinator has reassigned that Job `<TBD-6>` times, THEN THE Coordinator SHALL report the Job to the Front_End as failed, with the failure reason of each assignment.
4. IF the Front_End receives no message from the Coordinator for `<TBD-7>` seconds while an Escalated_Goal of the Session has no Verdict, THEN THE Front_End SHALL switch to the fallback for the rest of the Session and SHALL make no further Cluster submission in the Session.
5. WHERE the fallback option is `local` (the draft default, OQ-N31), WHILE the Front_End has switched to the fallback, THE Front_End SHALL run on the local machine every Escalated_Goal that has no Verdict, as local Jobs with the Solver_Configurations of the configuration list that R2.2 derives for Escalated_Goals that are not Hard_Goals.
6. WHERE the fallback option is `unresolved`, WHEN the Front_End switches to the fallback (criterion 4 or R1.1 criterion 5), or the Scheduler marks a Goal_Version as an Escalated_Goal after that switch, THE Verdict_Recorder SHALL record for every Escalated_Goal that has no Verdict the most informative non-proof outcome (R7.2 criterion 5) among that Goal_Version's Job outcomes and the Assurance_Labels of its accepted Results, or unresolved if there is none.
7. IF the fallback option is set to a value other than `local` or `unresolved`, THEN THE Front_End SHALL report a configuration error that names the rejected value, SHALL stop before starting any solver process, SHALL return a non-zero exit status to the Creusot_CLI, and SHALL leave `proof.json` and the `.why3find` cache unchanged.
8. WHEN the Front_End receives a failure report for a Job under criterion 3, THE Verdict_Recorder SHALL count that Job as ended with the outcome failed in the Race of the Job's Goal_Version (R2.2 criterion 4).
9. WHILE the Front_End has not switched to the fallback, WHEN the Front_End receives a message that carries a Coordinator incarnation identifier different from the last one the Front_End received in the Session, THE Front_End SHALL resubmit every Goal_Version that the Front_End submitted to the Cluster in the Session and that has no Verdict and a Race that has not ended, by adding to its Race, at that step, one new Job with a new Job identifier and the same Solver_Configuration and Partition for each Job of that Race that R2.2 criterion 25 ends at that step; R2.2 criterion 6 submits these Jobs, and they count against `<TBD-8>` (OQ-N30).
10. WHEN a Worker connects to the Coordinator while the Coordinator records running Jobs on that Worker, THE Coordinator SHALL mark each of those running Jobs as failed with the reason Worker reconnect.
11. WHEN the Coordinator receives a message whose message identifier the Coordinator already processed, THE Coordinator SHALL discard the message without changing state.
12. WHEN a Worker receives a message whose message identifier the Worker already processed, THE Worker SHALL discard the message without changing state.
13. WHEN the Verdict_Recorder receives a Result whose Result identifier the Verdict_Recorder already processed in the Session, THE Verdict_Recorder SHALL discard the Result without changing state.
14. WHILE a Worker is connected to the Coordinator, THE Worker SHALL send the Coordinator, at least every `<TBD-31>` seconds, a heartbeat that lists the Worker's running Jobs.
15. WHILE a Session has a Cluster submission without a Verdict, THE Front_End SHALL send the Coordinator a heartbeat at least every `<TBD-31>` seconds.
16. IF the Coordinator receives no message from the Front_End of a Session for `<TBD-32>` seconds, THEN THE Coordinator SHALL remove every queued Job of that Session from the queue and SHALL send a cancellation for every running Job of that Session.
17. WHEN a Worker heartbeat lists a running Job that the Coordinator does not record as running on that Worker, THE Coordinator SHALL send that Worker a cancellation for that Job.
18. THE System SHALL use values of `<TBD-31>`, `<TBD-5>`, `<TBD-7>` and `<TBD-32>` that satisfy `<TBD-31>` + D < min(`<TBD-5>`, `<TBD-7>`, `<TBD-32>`).
19. WHILE the Coordinator holds a queued or running Job of a Session, THE Coordinator SHALL send the Front_End of that Session a heartbeat at least every `<TBD-31>` seconds.
20. WHERE the fallback option is `local`, WHEN every local Job that the Front_End started under criterion 5 for an Escalated_Goal has ended, every Result of those Jobs that reports a proof has its Assurance_Label, the Goal_Version has no Verdict, and R1.5 criterion 13 records no Verdict for the Goal_Version at that step, THE Verdict_Recorder SHALL record for that Goal_Version the most informative non-proof outcome (R7.2 criterion 5) among its Job outcomes and the Assurance_Labels of its accepted Results, or unresolved if there is none.

#### Properties

- **P1.6.1** For any fault schedule with finitely many fault events in the simulation fault model (Worker crashes, network partitions, delays, duplicates, Coordinator restarts, Worker reconnects) and any Goal_Version G that the Front_End started proving, the Session ends with exactly one Verdict for G.
- **P1.6.2** (Idempotence) For any component c ∈ {Coordinator, Worker, Verdict_Recorder}, any state s of c, any message m and any k ≥ 1: `deliver_c^k(s, m) = deliver_c(s, m)`, ignoring log entries.
- **P1.6.3** For any fault schedule, every proof-labelled Verdict satisfies P7.1.1.
- **P1.6.4** (Attempt bound) For any trace τ and any Job identifier i, the number of assignment events of the Job with identifier i in τ is at most `1 + <TBD-6>`. Resubmission (criterion 9) creates Jobs with new Job identifiers, each with its own bound.
- **P1.6.5** For any trace τ, any Worker W and any heartbeat-timeout event of W at step t: every Job running on W at t is marked failed at t, and no Job is assigned to W after t until W's next connection event.
- **P1.6.6** (Fallback) For any trace τ in which the Front_End switches to the fallback at step t: τ contains no Cluster submission after t; with option `local`, every Escalated_Goal without a Verdict at t has a local solver-process start after t, and has a Verdict at the first step at which its local Jobs have ended and their proof-reporting Results are labelled (criterion 20, R1.5 criterion 13); with option `unresolved`, every Escalated_Goal without a Verdict at t has a non-proof Verdict at t, and every Goal_Version escalated after t has a non-proof Verdict at its escalation step.
- **P1.6.7** (Restart recovery) For any trace τ, any step t at which the Front_End, not switched to the fallback, receives a new Coordinator incarnation identifier, and any Goal_Version G submitted to the Cluster before t with no Verdict and a Race that has not ended at t: for each Job of G's Race that was open just before t, G's Race contains from t on a new Job with the same Solver_Configuration and Partition and a Job identifier not used before t, and τ contains after t a submission of that new Job unless G's Race ends first.
- **P1.6.8** (First wins) For any component c ∈ {Coordinator, Worker, Verdict_Recorder}, any state s and any messages m1, m2 with the same identifier: `deliver_c(deliver_c(s, m1), m2) = deliver_c(s, m1)`, ignoring log entries.
- **P1.6.9** For any fallback option value outside {`local`, `unresolved`} and any project state, running the prove command starts no solver process, ends with a non-zero exit status, and leaves `proof.json` and the `.why3find` cache byte-identical to their state before the run.
- **P1.6.10** (Orphaned Jobs) For any trace τ in which the Coordinator receives no message from a Session's Front_End during the `<TBD-32>` seconds that end at step t, no Job of that Session is queued after t, and every Job of that Session running at t receives a cancellation at or after t.
- **P1.6.11** (Worker reconnect) For any connection event of Worker W at step t, every Job that the Coordinator recorded as running on W before t is marked failed at t.
- **P1.6.12** (Heartbeats) For any simulation trace τ, while a Worker W is connected and not crashed, consecutive heartbeats from W are at most `<TBD-31>` seconds apart; while a Session has a Cluster submission without a Verdict, the same bound holds for heartbeats from the Front_End; and while the Coordinator holds a queued or running Job of a Session, the same bound holds for heartbeats from the Coordinator to that Session's Front_End.
- **P1.6.13** For any Job j that the Front_End receives a failure report for under criterion 3, the Race of j's Goal_Version treats j as ended with outcome failed.
- **P1.6.14** (OrphanCancelled) For any step t at which the Coordinator processes a heartbeat from Worker W that lists a Job j that the Coordinator does not record as running on W, the Coordinator sends W a cancellation for j at t.
- **P1.6.15** (Heartbeat margin) The default configuration of the Front_End, the Coordinator and the Worker, and the configuration of every `tests/sim/` run, satisfy `<TBD-31> + D < min(<TBD-5>, <TBD-7>, <TBD-32>)`.

#### Verification

- A2 — TLA+ fault actions (crash, restart, loss, duplication, Worker reconnect, Front_End silence, and the action `OrphanCancel` of criterion 17) with invariants P1.6.4, P1.6.5, P1.6.8, P1.6.11 and P1.6.14, and P1.6.1 as a liveness property under F1–F3, checked with TLC and Apalache — `verification/tla/Coordinator.tla` — `ci/tlc`, `sched/apalache-nightly`.
- A4 — deterministic simulation with fault injection, including Coordinator restarts while Jobs still run on Workers and local fallback runs that end without a proof (P1.6.1, P1.6.3–P1.6.7, P1.6.10–P1.6.14), with P1.6.15 asserted for the configuration of every run — `tests/sim/faults/` — `ci/sim`, `sched/sim-nightly`.
- A4 — configuration-validation unit test of the heartbeat margin on the default configuration (P1.6.15, criterion 18) — `crates/dp-core/tests/heartbeat_margin.rs` — `ci/test`.
- A4 — property-based test of handler idempotence and first-wins behaviour (P1.6.2, P1.6.8) — `crates/dp-core/tests/prop_idempotent.rs` — `ci/test`.
- A4 — integration test that stops and restarts the Coordinator and kills Workers, with both fallback options and malformed fallback option values (P1.6.6, P1.6.7, P1.6.9) — `tests/integration/cluster/faults.rs` — `ci/integration-cluster`.

#### Trust

- F1–F3 for progress after faults. NG-8. AS-3 (heartbeats and the Coordinator incarnation identifier).
- D in criterion 18 is a parameter of the simulation fault model. A real network delay above D can cause a spurious heartbeat timeout, which affects liveness only.

### Requirement 7: R1.7 Cluster operator controls

**Priority:** Must

**Class:** Functional

**User Story:** As a cluster operator, I want to register, drain and revoke Workers and to inspect the queue, so that I can run the cluster safely.

Priority note: R1.3 relies on registration and revocation (criteria 1, 5 and 6), and Job assignment (R1.1 criterion 4) relies on the eligible set (criterion 2), so this requirement is Must. The operator status report (criteria 8, 9 and 12) is its least critical part.

#### Acceptance Criteria

1. WHEN the cluster operator registers a Worker identity with a credential, through an entry of a Worker registry signed with the operator signing key (R1.3 criterion 9), THE Coordinator SHALL record the Worker identity and the credential as registered.
2. THE Coordinator SHALL hold in the set of eligible Workers exactly the registered Workers that are connected, whose credential is not revoked, that the cluster operator has not drained since their last registration, and that have had no heartbeat timeout (R1.6 criterion 1) since their last connection.
3. WHEN the cluster operator drains a Worker, THE Coordinator SHALL record the Worker as drained until the cluster operator registers the Worker again (OQ-N32).
4. WHILE a Worker is drained, THE Coordinator SHALL keep each running Job of that Worker assigned to that Worker until the Job ends, is cancelled under R1.5, or is marked failed under R1.6.
5. WHEN the cluster operator revokes a Worker credential, through a revocation signed with the operator signing key (R1.3 criterion 9), THE Coordinator SHALL record the credential as revoked and SHALL mark every running Job of that Worker as failed with the reason credential revocation.
6. WHEN the Front_End receives a revocation of a Worker credential that carries a valid signature by the operator signing key (R1.3 criterion 9), THE Verdict_Recorder SHALL discard every Result authenticated with that credential that the Verdict_Recorder processes after that receipt.
7. IF a Worker attempts to connect with a credential that is not registered or that is revoked, THEN THE Coordinator SHALL refuse the connection and SHALL record no offer for that Worker.
8. WHEN the cluster operator requests status, THE Coordinator SHALL report for each registered Worker the Worker identity, whether the Worker is eligible, drained, revoked and connected, the number of running Jobs, and the offer recorded under R1.1 criterion 3 (OQ-N34).
9. WHEN the cluster operator requests status, THE Coordinator SHALL report every queued and running Job with the Job's Goal_Version and Solver_Configuration, the Worker of each running Job, and a mark on each queued Job that has no eligible Worker (R1.1 criterion 7).
10. IF a drain or revoke command names a Worker identity that is not registered, THEN THE Coordinator SHALL report an error that names the identity and SHALL leave the Coordinator state unchanged.
11. IF an operator command is not authenticated with an operator credential, which is the online operator command key, separate from every Worker credential and from the operator signing key of R1.3 criterion 9, THEN THE Coordinator SHALL reject the command and SHALL leave the Coordinator state unchanged (OQ-N35 resolved by design decision D-9).
12. WHEN the cluster operator requests status, THE Coordinator SHALL deliver the report of criteria 8 and 9 as one value of the operator status-report format of R7.10, a canonical S-expression, together with a text rendering of that value (OQ-N34 resolved by design decision D-20).

#### Properties

- **P1.7.1** For any trace τ, any Worker W, any drain event of W at step t_d, and the next registration event of W at step t_r (t_r = ∞ if there is none), no Job is assigned to W at any step in the interval (t_d, t_r).
- **P1.7.2** For any trace τ, any credential k and the event at which the Front_End receives an operator-signed revocation of k at step t, the Verdict_Recorder accepts no Result authenticated with k at any step after t; and a revocation of k without a valid operator signature changes no acceptance decision (P1.3.9).
- **P1.7.3** (Eligible set) `□ (eligible = {W ∈ registered : connected(W) ∧ cred(W) ∉ revoked ∧ ¬drained(W) ∧ ¬hbTimeoutSinceConnect(W)})`.
- **P1.7.4** (Drain does not preempt) For any trace τ, any Worker W and any Job j running on W at W's drain event at step t, j stays assigned to W from t until j ends, is cancelled (R1.5) or is marked failed (R1.6).
- **P1.7.5** For any trace τ and any revocation event of Worker W's credential at step t: `W ∉ eligible_{t′}` for every t′ > t until W is registered again, and every Job running on W at t is marked failed at t.
- **P1.7.6** (Status completeness) For any Coordinator state s, the Jobs in `status(s)` are exactly `queued(s) ∪ running(s)`, the Workers in `status(s)` are exactly `registered(s)`, and every reported flag, Job count, offer and no-eligible-Worker mark equals the corresponding value in s.
- **P1.7.7** For any Coordinator state s and any operator command c that is unauthenticated, or that is a drain or revoke command naming an unregistered Worker identity, `apply(s, c) = s`.
- **P1.7.8** (Idempotence) For any Coordinator state s and any drain or revoke command c, `apply(apply(s, c), c) = apply(s, c)`.
- **P1.7.9** For any connection attempt with a credential that is unregistered or revoked at that step, the Coordinator records no offer for the attempting Worker and the Worker does not enter the set of eligible Workers.
- **P1.7.10** (Status format) For any Coordinator state s, the status report that the Coordinator delivers for s is `print(status(s))` in the operator status-report format, and `parse(print(status(s))) = status(s)`.

#### Verification

- A2 — TLA+ register, connect, drain, revoke and heartbeat-timeout actions with invariants P1.7.1, P1.7.2, P1.7.3, P1.7.5 and P1.7.9 and the temporal property P1.7.4, checked with TLC — `verification/tla/Coordinator.tla` — `ci/tlc`.
- A4 — property-based test of the eligibility function, the operator command handler and the status function (P1.7.3, P1.7.6–P1.7.9) — `crates/dp-core/tests/prop_operator.rs` — `ci/test`.
- A4 — integration tests of criteria 1 and 3–12 — `tests/integration/cluster/operator.rs` — `ci/integration-cluster`.
- A4 — round trip of the operator status-report format over generated Coordinator states (P1.7.10) — `crates/dp-formats/tests/` — `ci/test`.
- A4 — P1.7.1–P1.7.5 asserted on deterministic simulation traces — `tests/sim/` — `ci/sim`; P1.7.2 with operator-signed revocations and with forged unsigned revocations from an adversarial Coordinator — `tests/sim/adversarial_coordinator.rs` — `ci/sim`.

#### Trust

- TB-13 (the cluster operator provisions and stores credentials, the offline operator signing key and the online operator command key securely, design decision D-9); AS-3 (operator commands in the dp message protocol).
- The Coordinator is untrusted. A Coordinator that ignores a drain or a revocation breaks only the Coordinator-side criteria; soundness relies on criterion 6 and R1.3 (criteria 4 and 9) in the Front_End, which applies only registrations and revocations signed with the operator signing key. Between the operator's revocation and the Front_End's receipt of it, the Verdict_Recorder still accepts Results authenticated with the revoked credential (OQ-N11, NG-7).

---

**Group R2: Portfolio racing**

### Requirement 8: R2.1 Solver configurations

**Priority:** Must

**Class:** Functional

**User Story:** As a proof engineer, I want to declare named Solver_Configurations (prover, version, seed, options, Driver, Transformation_Pipeline), so that I control what races on hard goals.

#### Acceptance Criteria

1. WHERE portfolio racing is enabled, WHEN a Session starts, THE Front_End SHALL parse, with the Format_Codec and the Solver_Configuration file grammar of R7.10, every Solver_Configuration and Portfolio declared in the project configuration file, each Solver_Configuration with a name, prover, pinned version, seed, option list, Driver and Transformation_Pipeline, and each Portfolio as a name and an ordered list of Solver_Configuration names.
2. WHERE portfolio racing is enabled, THE Front_End SHALL use as the Session's Portfolio the Portfolio named by the portfolio option.
3. IF a Solver_Configuration names a prover and version pair that the Lock_File does not list, THEN THE Front_End SHALL report a configuration error that names the Solver_Configuration, SHALL stop before starting any solver process, SHALL return a non-zero exit status to the Creusot_CLI, and SHALL leave `proof.json` and the `.why3find` cache unchanged.
4. IF the configuration file fails to parse, declares two Solver_Configurations or two Portfolios with the same name, declares a Portfolio with no Solver_Configuration, lists in a Portfolio a name that no Solver_Configuration declares, names a Driver or a Transformation_Pipeline step that the pinned Why3 does not provide, names a Transformation_Pipeline step that the pinned Why3 registers as a transformation that can return more than one task (a splitting transformation, for example `split_vc`; R1.3 criterion 10), or the portfolio option names no declared Portfolio, THEN THE Front_End SHALL report a configuration error that names the offending entry (with the byte offset for a parse error, R7.10 criterion 2), SHALL stop before starting any solver process, SHALL return a non-zero exit status to the Creusot_CLI, and SHALL leave `proof.json` and the `.why3find` cache unchanged.
5. IF one Portfolio lists the same Solver_Configuration name twice, or lists two Solver_Configurations whose prover, version, seed, option list, Driver and Transformation_Pipeline are all equal, THEN THE Front_End SHALL report a configuration error that names both entries, SHALL stop before starting any solver process, SHALL return a non-zero exit status to the Creusot_CLI, and SHALL leave `proof.json` and the `.why3find` cache unchanged (OQ-N36).
6. WHEN the Front_End submits a Job of a Race on a Hard_Goal, THE Front_End SHALL include in the submission the Solver_Configuration exactly as parsed under criterion 1.
7. THE Format_Codec SHALL print every parsed Solver_Configuration and Portfolio in the canonical form of the Solver_Configuration file grammar (R7.10 criterion 3).

#### Properties

- **P2.1.1** (Round trip) For any accepted configuration text c, `parse(print(parse(c))) = parse(c)`, and for any configuration value v, `parse(print(v)) = v`.
- **P2.1.2** For any Session S and any Result r that the Verdict_Recorder accepts in S for a Job of a Race on a Hard_Goal, `r.config ∈ portfolio(S)`, where `portfolio(S)` is the set of Solver_Configurations of the Portfolio that criterion 2 selects for S. (Follows from criterion 6 and R1.3 criterion 6.)
- **P2.1.3** For any configuration text c and portfolio option value p that violate any condition of criteria 3, 4 or 5, and any project state, running the prove command starts no solver process, ends with a non-zero exit status, and leaves `proof.json` and the `.why3find` cache byte-identical to their state before the run.
- **P2.1.4** For any accepted configuration and its Portfolio P = [n_1, …, n_k]: `k ≥ 1`, the names n_i are pairwise distinct, and the tuples `(prover, version, seed, options, driver, pipeline)` of n_1, …, n_k are pairwise distinct; and no Transformation_Pipeline of an accepted Solver_Configuration contains a splitting transformation.
- **P2.1.5** For any Job submission event e of a Race on a Hard_Goal in Session S, `e.config = parsed(S)[e.config.name]`.

#### Verification

- A4 — property-based round-trip test (P2.1.1) and property-based test of the configuration validator over generated configurations with unknown versions, duplicates, empty and dangling Portfolios, and splitting Transformation_Pipeline steps (P2.1.4) — `crates/dp-formats/tests/prop_config.rs` — `ci/test`.
- A4 — fuzzing of the configuration parser — `fuzz/config_parser/` (cargo-fuzz) — `ci/fuzz-smoke`, `sched/fuzz-nightly`.
- A4 — integration test of rejected configuration files, including a Transformation_Pipeline with `split_vc`, checking exit status and byte-identical `proof.json` and `.why3find` (P2.1.3) — `tests/integration/config/` — `ci/integration-cluster`.
- A4 — P2.1.2 and P2.1.5 asserted on deterministic simulation traces — `tests/sim/` — `ci/sim`.

#### Trust

- None beyond the Lock_File and TB-12. The option list of a Solver_Configuration is passed to an Unpatched_Solver unchecked; whether some options can change the problem the solver answers, and so the meaning of proved-standalone, is OQ-N37.

### Requirement 9: R2.2 Portfolio racing on hard goals

**Priority:** Must

**Class:** Performance

**User Story:** As a Rust developer, I want hard goals raced across several Solver_Configurations, so that the first proof-labelled Result ends the wait.

Every Escalated_Goal has exactly one Race, formed under criterion 3. Every criterion below that names a Race applies to the Races of Hard_Goals and of other Escalated_Goals alike; only the configuration list differs.

In this requirement:

- The *configuration list* of a Race is the Session's Portfolio (R2.1 criterion 2) for a Hard_Goal. For any other Escalated_Goal, it is the list of Solver_Configurations that the Scheduler derives from why3find's prover configuration and proof strategy for the reference-time parallel pass, as recorded in the project configuration, in the order of that strategy: one Solver_Configuration per prover of that pass, with the pass's time and the Driver and Transformation_Pipeline of why3find's prover configuration (OQ-N43 resolved). The Race of such an Escalated_Goal is how the reference-time parallel pass runs: on the Cluster, and on the local machine only without the cluster option (R1.1 criterion 8) or after the switch to the fallback with the option `local` (R1.6 criterion 5) (OQ-N8 resolved by design decision D-17). The Coordinator chooses no Solver_Configuration (R1.1 criterion 4).
- *Race order* orders the Jobs of a Race by the position of their Solver_Configuration in the configuration list (a Solver_Configuration outside the list comes after every listed one), and Jobs with the same Solver_Configuration by the step at which they were added to the Race.
- The *Race deadline* of a Race is `<TBD-9>` seconds after the Scheduler marked its Goal_Version as an Escalated_Goal, which is also the step of Hard_Goal classification (criterion 1).
- A Result *awaits certification* from the step at which the Verdict_Recorder accepts the Result, if the Result reports a proof and its label depends on certificate production or checking under R5 (R5.1–R5.3, R5.5), until the Verdict_Recorder assigns the Result its label under R5.
- A Job is *open* from its submission until the Verdict_Recorder counts the Job as ended under criterion 4, 5, 24 or 25. Job outcomes are defined in the Glossary.
- The *end conditions* of a Race are criteria 8, 11, 13, 16, 20 and 22 and, where partitioning is enabled, R3.5 criterion 7. Where partitioning is enabled, the Jobs of a Goal_Version's Partitions belong to its Race and count against `<TBD-8>` (criterion 21); criterion 3 and P2.2.10 concern only whole-Goal_Version Jobs.

#### Acceptance Criteria

1. WHERE portfolio racing is enabled and the cluster option is configured, WHEN the Scheduler marks a Goal_Version as an Escalated_Goal, THE Scheduler SHALL classify that Goal_Version once in the Session as a Hard_Goal or not, with the configured hardness rule, whose default is `<TBD-10>`.
2. WHERE portfolio racing is disabled or the cluster option is not configured, THE Scheduler SHALL classify no Goal_Version as a Hard_Goal (OQ-N44 resolved for Sessions without the cluster option).
3. WHEN the Scheduler marks a Goal_Version as an Escalated_Goal, THE Scheduler SHALL form at that step, after the classification of criterion 1, the Race of that Goal_Version from one Job per Solver_Configuration of the Race's configuration list, in list order.
4. WHEN the Front_End receives, for an open Job of a Race, a failure report (R1.6 criterion 3), a no-eligible-Worker report (R1.1 criterion 7, R1.4 criterion 4) or a cancelled report (R1.5 criteria 15 and 16), THE Verdict_Recorder SHALL count that Job as ended in the Race with the outcome failed.
5. WHEN the Verdict_Recorder accepts under R1.3 a Result of an open Job of a Race, THE Verdict_Recorder SHALL count that Job as ended in the Race with the Result's Job outcome (Glossary).
6. WHILE a Race has not ended, the Front_End is connected to the Proof_Server and has not switched to the fallback (R1.1 criterion 1, R1.6 criterion 4), the Race deadline has not passed, fewer than `<TBD-8>` Jobs of the Race are open, and the Race has a Job that the Scheduler has not submitted, THE Scheduler SHALL submit the first such Job in Race order to the Cluster (OQ-N44).
7. THE Scheduler SHALL keep at most `<TBD-8>` Jobs of one Race open at every step.
8. WHEN the Verdict_Recorder assigns a proof label (R7.1) to a Result of a Race that has not ended, and that Result is for the whole Goal_Version rather than for a Partition (R3.5), THE Scheduler SHALL end the Race with that Result as the winner (the Verdict_Recorder assigns the label certified only after the Certificate_Store has completely stored the Result's entry, R5.3 criterion 17, so no Race ends with a certified winner before that storage completes).
9. WHEN a Race ends with a winner under criterion 8, or ends under R3.5 criterion 7, THE Verdict_Recorder SHALL record the Verdict of the Race's Goal_Version with the winner's Assurance_Label and Result identifier, or, under R3.5 criterion 7, with the label certified and the Result identifiers of the leaf Results, together with, for the label certified, the content hash of the completely stored Certificate_Store entry (R5.4 criterion 3).
10. WHILE a Result of a Race awaits certification, THE Scheduler SHALL NOT end the Race under criterion 11 or 13.
11. WHEN every Job of a Race has ended, no Result of the Race awaits certification, and criterion 8 does not end the Race at that step, THE Scheduler SHALL end the Race without a winner.
12. WHEN the Race deadline of a Race passes while the Race has an open Job, THE Front_End SHALL send the Coordinator a cancellation that names the Session and the Race's Goal_Version (R1.5 criterion 4).
13. WHILE the Race deadline of a Race has passed, WHEN no Result of the Race awaits certification and criterion 8 does not end the Race at that step, THE Scheduler SHALL end the Race without a winner.
14. WHEN a Race ends without a winner under criterion 11, 13 or 16, THE Verdict_Recorder SHALL record as the Verdict the most informative non-proof outcome (R7.2 criterion 5) among the Race's Job outcomes and the Assurance_Labels of the Race's accepted Results, or unresolved if there is none.
15. THE Scheduler SHALL read the invalid-answer policy from configuration, with the values `continue` and `stop` and the default `continue` (OQ-N3).
16. WHERE the invalid-answer policy is `stop`, WHEN the Verdict_Recorder accepts, for a Race that has not ended, a Result with prover answer Invalid that is not a Lemma_Dependent_Result, THE Scheduler SHALL end the Race without a winner.
17. IF the invalid-answer policy is set to a value other than `continue` or `stop`, THEN THE Front_End SHALL report a configuration error that names the rejected value, SHALL stop before starting any solver process, SHALL return a non-zero exit status to the Creusot_CLI, and SHALL leave `proof.json` and the `.why3find` cache unchanged.
18. WHEN the Verdict_Recorder accepts a Result of a Race that has ended, THE Verdict_Recorder SHALL log the Result with the mark late, SHALL keep the Verdict unchanged (R1.5 criterion 8), and SHALL record no Verdict from that Result.
19. IF the Verdict_Recorder, for one Goal_Version in one Session, assigns a proof label to one Result and accepts another Result with prover answer Invalid, including a late Result (criterion 18), THEN THE Verdict_Recorder SHALL write to `proof.json` a conflict entry that names both Result identifiers and SHALL report the conflict in the Creusot_CLI output.
20. WHEN the Front_End switches to the fallback (R1.1 criteria 5 and 8, R1.6 criterion 4), or the Scheduler forms a Race after that switch, THE Scheduler SHALL end every Race of the Session that has not ended, without a winner, and R1.6 criteria 5, 6 and 20 decide the Verdicts of those Goal_Versions.
21. WHERE partitioning is enabled, WHEN the Scheduler creates a Job for a Partition of a Goal_Version (R3.5 criterion 1), THE Scheduler SHALL add that Job to the Race of that Goal_Version, so that criteria 4 to 7 and the bound `<TBD-8>` apply to that Job.
22. WHEN the Verdict_Recorder records a Verdict under R1.5 criterion 9 for the Goal_Version of a Race that has not ended, THE Scheduler SHALL end that Race without a winner.
23. IF a Race ends under criterion 16, 20 or 22 while a Result of the Race awaits certification, THEN THE Verdict_Recorder SHALL let the certification of that Result continue under R5, SHALL use the Assurance_Label that certification assigns only for the log and for criterion 19, and SHALL record no Verdict from that label (OQ-N38 resolved).
24. WHEN the Verdict_Recorder discards, under the Task_Hash part of R1.3 criterion 4, R1.3 criterion 7, R1.3 criterion 10 or R3.5 criterion 5, a Result that passed the authentication part of R1.3 criterion 4 and that names the goal path, Solver_Configuration and Partition of an open Job of a Race, THE Verdict_Recorder SHALL count that Job (the first such Job in Race order if there are several) as ended in the Race with the outcome failed.
25. WHEN the Front_End receives a message that carries a Coordinator incarnation identifier different from the last one the Front_End received in the Session, THE Verdict_Recorder SHALL count every open Job of every Race of the Session as ended with the outcome failed, at the same step at which R1.6 criterion 9 adds the replacement Jobs.
26. WHEN the Verdict_Recorder accepts a Result of a Race that has ended, THE Verdict_Recorder SHALL assign that Result an Assurance_Label, and, if the Result needs certification (R5.1), SHALL label the Result unchecked at acceptance with no proof-producing run and no Certificate_Checker run (R5.1 criterion 9; OQ-N48).
27. WHERE portfolio racing is enabled and the cluster option is not configured, WHEN a Session starts, THE Front_End SHALL log a warning that names the cluster option and states that no Goal_Version is classified as a Hard_Goal in the Session.

#### Properties

- **P2.2.1** For any Session trace τ and any Race ρ on Goal_Version G that ends under criterion 8, 11, 13, 16 or 22, or under R3.5 criterion 7: the Verdict of G is proof-labelled if and only if ρ ends under criterion 8 or R3.5 criterion 7, that is, if and only if, at the step at which ρ ended, the Verdict_Recorder assigned a proof label to a whole-Goal_Version Result of ρ or labelled the partitioned Goal_Version certified.
- **P2.2.2** For any Race ρ with winner w: w is the first Result of ρ in the Verdict_Recorder's sequence of proof-label assignment events, and the Verdict of ρ's Goal_Version carries w's Assurance_Label and Result identifier.
- **P2.2.3** (Open-Job bound) For any Session trace τ, any Race ρ and any step t: `|{j ∈ ρ : open_t(j)}| ≤ <TBD-8>`, counting Partition Jobs where partitioning is enabled and the replacement Jobs of R1.6 criterion 9; and an open Job j of ρ is not open after any step at which an end event of criterion 4, 5, 24 or 25 occurs for j.
- **P2.2.4** Under the invalid-answer policy `continue`, for any Race ρ and any two arrival orders of the same set of Results, one Result per Job of ρ, all accepted before the Race deadline, with no Job ended under criterion 4, 24 or 25 and with the same certification outcome for each Result: the Verdict is proof-labelled in one order if and only if it is proof-labelled in the other, and when neither is proof-labelled the two Verdicts carry the same label.
- **P2.2.5** (No unchecked winner) For any Race ρ that ends at step t: if ρ has a winner w, then `label_t(w) ∈ {proved-standalone, certified}`; if ρ ends under criterion 11 or 13, then no Result of ρ awaits certification at t.
- **P2.2.6** (Bounded Race) For any simulation trace generated under F1–F4 in which every certification attempt ends within the limits of R5 (`<TBD-14>` seconds for proofs and composition, `<TBD-15>` seconds for checking; R5.1 guarantees this, P5.1.3), every Race ρ, on a Hard_Goal or on any other Escalated_Goal, ends no later than `escalated(ρ) + <TBD-9> + <TBD-14> + <TBD-15>` seconds of simulated time, where `escalated(ρ)` is the time at which the Scheduler marked ρ's Goal_Version as an Escalated_Goal.
- **P2.2.7** (Deadline) For any Race ρ with deadline step t_d: no Job of ρ is submitted after t_d, and if a Job of ρ is open at t_d, the trace contains at t_d a cancellation from the Front_End that names ρ's Session and Goal_Version.
- **P2.2.8** (Late Results) For any Result r of Race ρ accepted at a step after ρ ended, the Verdict of ρ's Goal_Version after r's acceptance equals the Verdict before it (or the Goal_Version has no Verdict before and after), no Verdict is recorded from r, and the log contains a late entry for r.
- **P2.2.9** (Conflict) For any Session trace τ and any Goal_Version G for which τ contains a proof-label assignment to Result r1 and the acceptance of a Result r2 with prover answer Invalid, including a Result accepted after G's Race ended, `proof.json` and the Creusot_CLI output of τ contain a conflict entry naming r1 and r2.
- **P2.2.10** (Race composition) For any Session S and any Race ρ of Goal_Version G with configuration list [c_1, …, c_k] (the Portfolio for a Hard_Goal, the derived list otherwise), the whole-Goal_Version Jobs of ρ are: one Job of each c_i formed under criterion 3, plus one replacement Job of c_i for each Coordinator incarnation change at which a whole-Goal_Version Job of c_i was open (R1.6 criterion 9). At every step, ρ has at most one open whole-Goal_Version Job per c_i, and for i < k, if a Job of c_{i+1} is submitted then a Job of c_i was submitted at an earlier or equal step.
- **P2.2.11** (Classification) For any Session trace τ: with portfolio racing disabled or the cluster option not configured, τ contains no Hard_Goal classification; otherwise τ contains exactly one classification event for every Escalated_Goal of τ, none for any other Goal_Version, and each classification of a Goal_Version G equals the output of the configured hardness rule on G.
- **P2.2.12** (Stop policy) Under the invalid-answer policy `stop`, for any Race ρ that has not ended when the Verdict_Recorder accepts a Result r of ρ with prover answer Invalid: ρ ends at that step if r is not a Lemma_Dependent_Result, and r does not end ρ if r is a Lemma_Dependent_Result.
- **P2.2.13** For any invalid-answer policy value outside {`continue`, `stop`} and any project state, running the prove command starts no solver process, ends with a non-zero exit status, and leaves `proof.json` and the `.why3find` cache byte-identical to their state before the run.
- **P2.2.14** (Submission progress) For any Scheduler state s in which a Race ρ has not ended, the Front_End is connected to the Proof_Server and not in the fallback, ρ's Race deadline has not passed, fewer than `<TBD-8>` Jobs of ρ are open, and ρ has a Job that the Scheduler has not submitted: the submissions that the Scheduler decides in s include the first such Job of ρ in Race order.
- **P2.2.15** (Proof wins ties) For any Race ρ and any step t at which a whole-Goal_Version Result of ρ receives a proof label while ρ had not ended before t: ρ ends at t under criterion 8, and not under criterion 11, 13, 16 or 22.
- **P2.2.16** (Fallback) For any Session trace τ in which the Front_End switches to the fallback at step t: every Race that has not ended before t ends at t without a winner, every Race formed after t ends at its formation step without a winner, and no Verdict of τ is recorded from the label of a Result that awaited certification when its Race ended under criterion 16, 20 or 22 (criterion 23).
- **P2.2.17** (Verdict value) For any Race ρ of Goal_Version G that ends without a winner under criterion 11, 13 or 16 at step t: `verdict(G) = most_informative(O_t(ρ) ∪ L_t(ρ))`, where `O_t(ρ)` is the multiset of the non-proof Job outcomes of the Jobs of ρ counted as ended by t (the Job outcome of a Job whose Result reports a proof is that Result's Assurance_Label, Glossary), and `L_t(ρ)` is the multiset of the non-proof Assurance_Labels of ρ's accepted Results at t.
- **P2.2.18** (No cluster, no Hard_Goal) For any Session trace τ with portfolio racing enabled and the cluster option not configured, τ contains no Hard_Goal classification, and the Session log contains, at Session start, a warning that names the cluster option.
- **P2.2.19** (Late labelling) For any Result r of Race ρ accepted at a step t after ρ ended: r receives an Assurance_Label; and if r needs certification, `label(r) = unchecked` is assigned at t, and the trace contains no proof-producing run and no Certificate_Checker run for r.

#### Verification

- A1 — Creusot contracts on the race decision function for P2.2.1, P2.2.2, P2.2.5, P2.2.8, P2.2.12, P2.2.14, P2.2.15, P2.2.17 and P2.2.19 — `crates/dp-core/src/race.rs` — `ci/creusot`.
- A2 — TLA+ Race actions for every Escalated_Goal (classification, Race formation, submission, Job end by Result, by failure, no-eligible-Worker or cancelled report, by a discarded Result and by a Coordinator restart, certification wait, deadline, the deadline Verdict of R1.5 criterion 9, stop policy, late Results, fallback) with invariants P2.2.3, P2.2.5, P2.2.7, P2.2.8, P2.2.15 and P2.2.16 and the liveness property P2.2.6 under F1–F4, checked with TLC and Apalache — `verification/tla/Coordinator.tla` — `ci/tlc`, `sched/apalache-nightly`.
- A4 — property-based test over arrival permutations and generated Race event sequences (P2.2.1–P2.2.5, P2.2.8–P2.2.12, P2.2.14, P2.2.15, P2.2.17, P2.2.19) — `crates/dp-core/tests/prop_race.rs` — `ci/test`.
- A4 — P2.2.3, P2.2.6, P2.2.7, P2.2.10, P2.2.11, P2.2.14, P2.2.16 and P1.6.13 asserted on deterministic simulation traces with Races on Hard_Goals and on other Escalated_Goals, fault injection and simulated certification delays — `tests/sim/` — `ci/sim`, `sched/sim-nightly`; Jobs ended by no-eligible-Worker reports, cancelled reports, discarded authenticated Results and Coordinator restarts (P2.2.3, P2.2.10) — `tests/sim/faults/` — `ci/sim`, `sched/sim-nightly`.
- A4 — integration test of the invalid-answer policy values, malformed policy values (P2.2.13) and conflict reporting with a stub solver that answers Invalid (P2.2.9) — `tests/integration/cluster/race.rs` — `ci/integration-cluster`; integration test of a Session with portfolio racing enabled and no cluster option (P2.2.18) — `tests/integration/config/` — `ci/integration-cluster`.
- A5 — portfolio experiment (RQ2) — `eval/scripts/rq2_portfolio/` — `sched/eval`.

#### Trust

- Labels come from R7.1. Invalid answers come from untrusted solvers; they affect only non-proof Verdicts and the length of a Race, never a proof label.
- F1–F4 for P2.2.6. The constraint between the Race bound and the per-goal Verdict deadline is noted under TBD-3 and TBD-9.
- The configuration list of an Escalated_Goal that is not a Hard_Goal comes from the project configuration. The Coordinator cannot change it, because R1.3 criterion 6 discards every Result whose pair of goal path and Solver_Configuration the Front_End did not submit.
- A Portfolio may mix Drivers and Transformation_Pipelines. Each Job's Printed_Goal and Task_Hash follow from the Goal_Version and the Job's Solver_Configuration (Glossary; OQ-N39 resolved).

### Requirement 10: R2.3 Winning configuration recorded for replay

**Priority:** Must

**Class:** Functional

**User Story:** As an auditor, I want `proof.json` to record exactly which configuration won, so that the Replay_Tool can re-run or re-check the result.

#### Acceptance Criteria

1. WHEN the Verdict_Recorder records a proof-labelled Verdict, THE Verdict_Recorder SHALL write to `proof.json`, for the Result from which the Verdict is recorded (the winner of a Race, a Local_Fast_Pass Result, a Result of a local fallback run, or the Result stored in a Result_Cache entry), the Result identifier (for a Result_Cache entry, the identifier of the Result as originally accepted), the Task_Hash, the Driver hash, the Transformation_Pipeline hash, the full Solver_Configuration, the solver binary hash, the Worker identity (the mark local for a Result produced on the local machine), the Calibration identifier and value (why3find's local Calibration for a Result produced on the local machine), the local time limit and the reference time limit.
2. WHEN the Verdict_Recorder records a Verdict labelled certified, THE Verdict_Recorder SHALL also write to `proof.json` the Certificate_Store content hash of the Composed_Certificate (R5.4).
3. WHEN the Verdict_Recorder records a proof-labelled Verdict from a Lemma_Dependent_Result, THE Verdict_Recorder SHALL also write to `proof.json` the identifiers of every imported Lemma and Instantiation_Hint and the import-set hash of R1.3 criterion 1.
4. WHEN the Verdict_Recorder records a proof-labelled Verdict from a Result whose import list is empty, THE Verdict_Recorder SHALL write to `proof.json` the import-set hash of the empty set.
5. THE Verdict_Recorder SHALL write the fields of criteria 1 to 4, 6 and 7 as `proof.json` extension fields under the grammar of R7.10, so that unmodified pinned why3find reads the entry as it reads entries without those fields (AS-19).
6. WHERE partitioning is enabled, WHEN the Verdict_Recorder records a proof-labelled Verdict for a partitioned Goal_Version, THE Verdict_Recorder SHALL write to `proof.json` the fields of criterion 1 for each leaf Result and the hash of the canonical printed form (R7.10) of the recorded Partition tree.
7. IF the Result or Result_Cache entry from which a proof-labelled Verdict is recorded carries no value for a field of criterion 1 (for example, a Result_Cache entry written before that field existed), THEN THE Verdict_Recorder SHALL write the marker absent for that field.
8. WHEN the Replay_Tool replays a proof-labelled Verdict whose `proof.json` entry has the marker absent in a field of criterion 1, THE Replay_Tool SHALL report the Verdict as failed with the reason not replayable (R7.6 criterion 7).

#### Properties

- **P2.3.1** (Round trip) For any proof-labelled Verdict v recorded from Result r, or from the leaf Results of a partitioned Goal_Version, `replay_fields(read(write(v))) = replay_fields(r)`, where `replay_fields` selects the fields of criteria 1 to 4 and 6, with the marker absent for each field of criterion 1 that r does not carry.
- **P2.3.2** For any certified Verdict v in `proof.json`, `v.certificate_hash` resolves to an entry of the Certificate_Store whose Printed_Goal has Task_Hash `v.task_hash`.
- **P2.3.3** For any proof-labelled Verdict v in `proof.json`: `v.imports = ∅ ⇒ v.imports_hash = H(canon(∅))`; `v.label = proved-standalone ⇒ v.imports = ∅`; and `v.imports ≠ ∅ ⇒ v.label = certified ∧ v.imports_hash = H(canon(v.imports))`. The first clause covers certified Verdicts whose Results have empty import lists, such as those of certify-all mode.
- **P2.3.4** (Replay sufficiency) For any Session of `ci/integration-cluster`, the Replay_Tool, run in `ci/replay`, confirms every certified Verdict of that Session's `proof.json`, and every proved-standalone Verdict whose solver is a stub solver or a solver that answers deterministically for the recorded seed and options, using only `proof.json`, the Certificate_Store and pinned local tools (R7.6). Replay agreement of proved-standalone Verdicts with real solvers is measured empirically in RQ6 (A5, R6.2 criterion 16), not asserted here.
- **P2.3.5** (Sources) For any proof-labelled Verdict v recorded from a Result produced on the local machine, `v.worker = local` and `v.calibration` is why3find's local Calibration for v's prover; for any v recorded from a Result_Cache entry e, `v.result_id = e.result.id`; and for any v of a partitioned Goal_Version with recorded Partition tree T, v carries one set of criterion 1 fields per leaf of T and `v.partition_tree_hash = H(print_tree(T))`.
- **P2.3.6** (Not replayable) For any `proof.json` entry with a proof label and the marker absent in a field of criterion 1, the Replay_Tool reports the entry as failed with the reason not replayable, and the replay exits with a non-zero status.

#### Verification

- A4 — property-based test of the `proof.json` writer and reader, over Verdicts from Race winners, local Results, Result_Cache entries with missing fields and partitioned Goal_Versions, for P2.3.1, P2.3.3 and P2.3.5 — `crates/dp-formats/tests/prop_proofjson.rs` — `ci/test`.
- A4 — replay of Sessions from `ci/integration-cluster`, whose proved-standalone Verdicts come from stub solvers or deterministic solver runs (P2.3.2, P2.3.4) — `tests/replay/` — `ci/replay`; replay of entries with the marker absent (P2.3.6) — `tests/replay/tamper/` — `ci/replay`.
- A4 — differential test that unmodified pinned why3find reads entries with the extension fields (criterion 5) — `tests/diff/legacy_reader/` — `ci/diff`.

#### Trust

- None. AS-19 for criterion 5. The Provenance_Records behind the identifiers of criterion 3 are kept in the Certificate_Store (R3.2 criterion 10).

---

**Group R3: Within-goal lemma sharing**

### Requirement 11: R3.1 Lemma exchange among Jobs racing on the same goal

**Priority:** Must

**Class:** Performance

**User Story:** As a proof engineer, I want Jobs racing on the same goal to exchange learned Lemmas, so that hard goals are proved sooner.

In this requirement:

- The *base Task_Hash* of a Job is the Task_Hash of the Job's Printed_Goal without the Job's Partition_Assumptions (R3.5). The import mark (Glossary) is set under criterion 2. Criterion 3 governs forwarding between Jobs of the same Goal_Version; R4.2 criteria 2 to 4 govern forwarding between Jobs of different Goal_Versions.
- In this requirement and in R3.2 to R4.4, a Lemma or Instantiation_Hint is *exported* from a Job at the step at which the Worker reads the solver's export message for it, and *delivered* to a Job at the step at which the Worker records its identifier in the Job's delivered set (criterion 10).
- A *registered Job* of the Lemma_Pool is a Job whose Job registration message (criterion 12) the Lemma_Pool has received since its last restart (R3.6 criterion 8), for which the Lemma_Pool has received no Job-end message (criterion 13), and whose Session the Lemma_Pool has not deleted (R3.6 criterion 7).
- The *pending queue* of a Job is the Worker's queue of Lemmas held for that Job because of used-Lemma order (R3.4 criterion 7), managed under criteria 14 to 17.

#### Acceptance Criteria

1. WHERE within-goal sharing is enabled, WHEN a Sharing_Solver in a Job of Goal_Version G exports a Lemma that passes the export filters of R4.1 criteria 13 and 19 and R4.3 criterion 5, THE Worker SHALL send the Lemma, renamed to Canonical_Names under R4.1 criterion 17, with its identifier and Provenance_Record (R3.2) to the Lemma_Pool.
2. WHERE within-goal sharing is enabled, THE Front_End SHALL set the import mark in the submission of a Job of a Race exactly when the within-goal sharing option lists the Job's Solver_Configuration as importing.
3. THE Lemma_Pool SHALL forward a Lemma from a Job a to a Job b of the same Goal_Version only if a and b are different Jobs of the same Session, b carries the import mark, and a and b have equal base Task_Hash, which fixes the same Driver and Transformation_Pipeline.
4. WHEN the Worker delivers a Lemma to the solver of a Job, THE Worker SHALL append that Lemma's identifier, in delivery order, to the import list of every Result of that Job that the Worker sends after the delivery.
5. THE Verdict_Recorder SHALL treat every Result of a Job submitted with the import mark as a Lemma_Dependent_Result, whatever import list the Result carries.
6. WHEN the Worker receives from the Lemma_Pool a Lemma for a Job that runs on that Worker and carries the import mark, THE Worker SHALL deliver the Lemma to the Job's solver only after the Admission_Checker (R3.3, R3.4) admits the Lemma for that Job's Printed_Goal and Partition.
7. IF the Worker receives a Lemma for a Job that does not run on that Worker or that carries no import mark, THEN THE Worker SHALL discard the Lemma and log the discard.
8. WHEN the Worker receives for a Job a Lemma whose identifier the Worker already delivered to that Job, holds in that Job's pending queue, or exported from that Job, THE Worker SHALL discard the duplicate without changing state.
9. IF a Worker's connection to the Lemma_Pool fails, or the Lemma_Pool restarts, THEN THE Worker SHALL keep running every Job of that Worker without Lemma exchange until the connection is restored, with time limits, cancellation and Results unchanged (R1.4, R1.5).
10. THE Worker SHALL record a Lemma or Instantiation_Hint as delivered to a Job, by adding its identifier to the Job's delivered set, before writing the Lemma or instance to that Job's solver.
11. WHERE within-goal sharing is enabled, WHEN the Lemma_Pool stores a pool item (R3.6) from a Job a, THE Lemma_Pool SHALL forward the pool item within `<TBD-43>` seconds to every registered Job b to which criterion 3, R4.2 criteria 2 and 3, or, for an Instantiation_Hint, R4.4 criterion 6 allow forwarding, deciding those criteria from the Job registration messages of a and b.
12. WHERE within-goal sharing is enabled, WHEN a Worker starts a Job, or restores its connection to the Lemma_Pool while a Job runs on that Worker, THE Worker SHALL send the Lemma_Pool a Job registration message for that Job that carries the Job identifier, Session, Goal_Version, base Task_Hash, Driver hash, Transformation_Pipeline hash, sibling group identifier (R4.2 criterion 1; the mark none where cross-goal sharing is disabled) and import mark.
13. WHERE within-goal sharing is enabled, WHEN a Job ends on a Worker, THE Worker SHALL send the Lemma_Pool a Job-end message that names the Job identifier.
14. WHILE the pending queue of a Job b holds fewer than `<TBD-42>` Lemmas, WHEN the Admission_Checker rejects a Lemma for b with the reason category used-Lemma order, which is the last check of R3.4 criterion 6 so that every other check passed, THE Worker SHALL hold the Lemma in b's pending queue and SHALL mark the logged rejection as held.
15. IF the Admission_Checker rejects a Lemma for a Job b with the reason category used-Lemma order while b's pending queue holds `<TBD-42>` Lemmas, THEN THE Worker SHALL log the rejection with the reason category used-Lemma order and SHALL leave the pending queue unchanged.
16. WHEN a Lemma or Instantiation_Hint is admitted into or exported by a Job b, THE Worker SHALL re-run the Admission_Checker, in the order in which the held Lemmas arrived, on every Lemma in b's pending queue that lists that Lemma or Instantiation_Hint as a used Lemma, and SHALL remove from the queue and deliver every held Lemma that the Admission_Checker then admits.
17. WHEN a Lemma has been held in the pending queue of a Job for `<TBD-41>` seconds, THE Worker SHALL remove the Lemma from the queue and SHALL log its rejection with the reason category used-Lemma order.
18. WHERE within-goal sharing is enabled, THE Front_End SHALL submit every Job whose Solver_Configuration names the prover Alt-Ergo without the import mark, also when the within-goal sharing option lists that Solver_Configuration as importing, so that criterion 2 sets the import mark only for Solver_Configurations of other provers (OQ-N12 resolved by design decision D-18).
19. WHERE within-goal sharing is enabled, WHEN a Worker runs a Job whose Solver_Configuration names the prover Alt-Ergo, THE Worker SHALL run that Job as a Job of an Unpatched_Solver, with no Lemma or Instantiation_Hint export to the Lemma_Pool and no delivery to its solver, so that Alt-Ergo still races under R2.2 and takes no part in sharing (OQ-N12 resolved by design decision D-18).

#### Properties

- **P3.1.1** For any Job j that was submitted with the import mark or to which any Lemma was delivered, every accepted Result of j is a Lemma_Dependent_Result.
- **P3.1.2** For any Lemma that the Lemma_Pool forwards from Job a to a Job b with `goal_version(a) = goal_version(b)`: `a ≠ b ∧ session(a) = session(b) ∧ import_mark(b) ∧ base_task_hash(a) = base_task_hash(b)`, and therefore `driver(a) = driver(b) ∧ pipeline(a) = pipeline(b)`.
- **P3.1.3** (Differential) For any Goal_Version G in the CI corpus, run once with within-goal sharing enabled and once disabled, with all other configuration equal: G does not get a proof-labelled Verdict in one run while a Result with prover answer Invalid for G exists in the other run.
- **P3.1.4** (Import list completeness) For any Result r of Job j that the Worker sends at step t, `delivered_{<t}(j) ⊆ r.imports`.
- **P3.1.5** (Admission before delivery) For any delivery of Lemma L to Job b at step t, the Admission_Checker returned `admit(L, b) = true` at a step at or before t.
- **P3.1.6** (Delivery handler) For any Worker state s, Job j and Lemma L: `recv(recv(s, L, j), L, j) = recv(s, L, j)`, ignoring log entries; and if j does not run in s or carries no import mark, `recv(s, L, j) = s` except for one appended log entry.
- **P3.1.7** (Lemma_Pool faults) For any simulation trace with finitely many Lemma_Pool crashes, restarts, partitions, delays and duplicate messages, P1.6.1 and P2.2.6 hold.
- **P3.1.8** (Record before write) For any Worker trace, any Job j and any step t at which the Worker writes a Lemma or the instance of an Instantiation_Hint x to j's solver: `id(x) ∈ delivered_t(j)`, recorded at a step at or before t.
- **P3.1.9** (Export filters) For any Lemma message that a Worker sends to the Lemma_Pool from Job j: j's Printed_Goal has no canonicalization failure, the Lemma passes R4.3 criterion 5, and every symbol of the sent Lemma text is in `CV(j) ∪ theory(logic(j))`.
- **P3.1.10** (Forwarding liveness) Under F1–F3, for any behavior of `Sharing.tla` in which the Lemma_Pool follows its specification, and any pool item x that the Lemma_Pool stores from Job a at time t: for every Job b that is registered at t and stays registered until `t + <TBD-43>`, and to which criterion 3, R4.2 criteria 2 and 3 or R4.4 criterion 6 allow forwarding x, the Lemma_Pool sends x to b by `t + <TBD-43>`.
- **P3.1.11** (Registration) For any Worker trace with within-goal sharing enabled and any Job j that runs on the Worker: the Worker sends a Job registration message for j with every field of criterion 12 at j's start and at every restoration of the Lemma_Pool connection while j runs, and a Job-end message for j when j ends.
- **P3.1.12** (Pending queue bounds) For any Worker trace and any Job b: at every step b's pending queue holds at most `<TBD-42>` Lemmas; a Lemma enters the queue only at a step at which the Admission_Checker rejected it for b with the reason category used-Lemma order; and every Lemma leaves the queue within `<TBD-41>` seconds of entering it, either delivered after `admit` returned true or with a logged rejection with the reason category used-Lemma order.
- **P3.1.13** (Re-run on arrival) For any Worker state with a Lemma L in b's pending queue and any step at which a used Lemma of L is admitted into or exported by b: the Worker re-runs `admit` on L at that step, and if `admit` returns true, L is delivered and leaves the queue at that step.
- **P3.1.14** (Two-Job exchange) For any deterministic simulation trace with exactly two import-marked Jobs a and b of one Goal_Version with equal base Task_Hash, whose Lemmas carry the default Dependency_Set (R3.2 criterion 2), under any message reordering, delays, duplicate messages and Lemma_Pool restarts: neither a nor b rejects a Lemma with the reason category used-Lemma order.
- **P3.1.15** (Alt-Ergo outside sharing) For any Session trace with within-goal sharing enabled, any within-goal sharing option value and any Job j whose Solver_Configuration names Alt-Ergo: j's submission carries no import mark, no Lemma or Instantiation_Hint is delivered to j, and the Worker sends the Lemma_Pool no pool item exported from j.

#### Verification

- A2 — Lemma flow invariants P3.1.1, P3.1.2, P3.1.5 and P3.1.8, and the forwarding liveness property P3.1.10 under F1–F3 — `verification/tla/Sharing.tla` — `ci/tlc`.
- A4 — property-based test of the forwarding filter (P3.1.2) and of the Worker delivery handler, including the record-before-write order and the export filters (P3.1.4, P3.1.6, P3.1.8, P3.1.9), and of import marks and delivery for Alt-Ergo Jobs under generated within-goal sharing option values (P3.1.15) — `crates/dp-core/tests/prop_forward.rs` — `ci/test`.
- A4 — property-based test of the pending queue over generated arrival orders, used-Lemma graphs, clock values and queue fill levels (P3.1.12, P3.1.13) — `crates/dp-core/tests/prop_pending.rs` — `ci/test`.
- A4 — differential test, sharing on vs. off (P3.1.3) — `tests/diff/sharing_on_off/` — `ci/diff`.
- A4 — deterministic simulation with Lemma_Pool crashes, restarts and partitions (P3.1.7) and with hostile Lemma_Pool messages — `tests/sim/faults/`, `tests/sim/hostile_pool.rs` — `ci/sim`, `sched/sim-nightly`; two-Job default-Dependency_Set simulation (P3.1.14) — `tests/sim/two_job_default_deps.rs` — `ci/sim`, `sched/sim-nightly`; forwarding latency and Job registration with delays, partitions and Lemma_Pool restarts under F1–F3 (P3.1.10, P3.1.11) — `tests/sim/forwarding.rs` — `ci/sim`, `sched/sim-nightly`.
- A5 — within-goal sharing experiment (RQ3) — `eval/scripts/rq3_within_goal/` — `sched/eval`.

#### Trust

- Sharing_Solvers are untrusted; Lemma_Dependent_Results are proved only through certificates (R5, R7.1). AS-7, AS-10.
- The forwarding bound (criterion 11, P3.1.10) holds only for a Lemma_Pool that follows its specification. Like a faulty Coordinator (NG-8), a faulty Lemma_Pool can withhold pool items; that affects performance only. A Lemma rejected after `<TBD-41>` seconds in the pending queue affects usefulness only.
- Exchange between Jobs that use different Drivers or Transformation_Pipelines is ruled out: the Driver and the Transformation_Pipeline decide the symbols and encodings of the Printed_Goal, and canonical naming guarantees equal meaning for equal names only within one Driver and Transformation_Pipeline (R4.1 criteria 3 and 4). The Admission_Checker enforces the same rule (R3.4 criterion 4), so a Lemma_Pool that ignores criterion 3 cannot break it. Whether Z3 and cvc5 can share one Driver is OQ-N4. Alt-Ergo takes no part in sharing (criteria 18 and 19; OQ-N12 resolved by design decision D-18).
- Authenticated Workers are trusted not to deliver Lemmas to Jobs without the import mark, because such a Job can produce a proved-standalone Result (NG-7, TB-2).

### Requirement 12: R3.2 Lemma provenance

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As an auditor, I want every shared Lemma to record its source and dependencies, so that I can trace any certified result back to the solver runs and Hypotheses that produced it.

#### Acceptance Criteria

1. WHEN a Worker exports a Lemma, THE Worker SHALL attach a Provenance_Record with the source Goal_Version, the source Job identifier, the source Job's base Task_Hash (R3.1) and Partition_Assumptions, the SMT-LIB logic of the source Printed_Goal (its `set-logic` line), the Solver_Configuration, the solver binary hash, the Worker identity, the Dependency_Set, the identifiers of the Lemmas delivered to the source Job before the export on which the Lemma depends (the *used Lemmas*), the Canonical_Name of the source's Negated_Goal_Assertion, and a flag stating whether the Dependency_Set contains that Canonical_Name.
2. IF the source solver reports no Dependency_Set for a Lemma, THEN THE Worker SHALL set the Dependency_Set to the Canonical_Names of every Assertion, declaration and Partition_Assumption of the source Job's Printed_Goal, including the Negated_Goal_Assertion, and SHALL list as used Lemmas every Lemma delivered to the source Job before the export.
3. THE Worker SHALL set each Lemma's identifier to the hash of the printed bytes of the Lemma and of its Provenance_Record, each in the canonical printed form (R7.10) in which the Worker sends them, so that every receiver recomputes the identifier from the received bytes without parsing the Lemma against any Canonical_Vocabulary.
4. THE Worker SHALL authenticate every exported Lemma with the Worker's credential, using an authentication tag computed over the canonical printed form of the Lemma, its identifier and its Provenance_Record (OQ-N11).
5. THE Lemma_Pool SHALL keep each Lemma that the Lemma_Pool stores (R3.6 decides which received Lemmas it stores) together with the Lemma's identifier, Provenance_Record and authentication tag, and SHALL return all four, byte for byte as received, on every read and every forward (R3.1 criterion 11).
6. IF a Lemma's Provenance_Record fails to parse under R7.10, lacks a field of criterion 1, carries a flag that disagrees with its Dependency_Set, the Lemma's bytes fail to parse under the vocabulary-independent Lemma grammar (R3.3), or the Lemma's identifier differs from the hash of criterion 3 computed over the received bytes of the Lemma and its Provenance_Record, THEN THE Admission_Checker SHALL reject the Lemma with the reason category malformed.
7. IF a Lemma's authentication tag fails verification, or names a Worker that is unregistered or whose credential is revoked, THEN THE Admission_Checker SHALL reject the Lemma with the reason category authentication.
8. WHEN the Certificate_Composer composes a Composed_Certificate for a Result r and a Lemma L is in the certification obligation `oblig(r)` (R5.1), THE Certificate_Composer SHALL include a scoped sub-proof of L whose open assumptions satisfy the open-assumption rule of R5.2 criterion 5, which is the single such rule for Lemma sub-proofs and which `compose_plan` implements once (AS-23).
9. IF the sub-proof obtained for a Lemma L in `oblig(r)` has an open assumption that the rule of R5.2 criterion 5 does not allow, THEN THE Certificate_Composer SHALL abort composition and THE Verdict_Recorder SHALL label the Result unchecked and log the reason provenance mismatch, as R5.2 criterion 5 states.
10. WHEN the Verdict_Recorder marks a Lemma_Dependent_Result checker-accepted (R5.3 criterion 4) or labels it checked-incomplete (R5.3 criterion 5), THE Verdict_Recorder SHALL store in the Certificate_Store entry of the Result's Composed_Certificate (R5.4 criterion 1) the Provenance_Record of every element of the Result's import closure `imports*` (R5.1), which follows used-Lemma edges from Lemmas only.

#### Properties

- **P3.2.1** For any Lemma L that the Admission_Checker admits: `prov(L)` parses and has every field of criterion 1, `prov(L).negated_goal = (prov(L).neg_goal_name ∈ prov(L).deps)`, the bytes of L parse under the vocabulary-independent Lemma grammar, `id(L) = H(bytes(L), bytes(prov(L)))` over the received bytes, and L's tag verifies under the registered, unrevoked credential of `prov(L).worker`.
- **P3.2.2** (Instance of P5.2.2) For any Result r labelled certified and any Lemma L in the certification obligation `oblig(r)` (R5.1), the Composed_Certificate of r contains a sub-proof π_L of L, and every open assumption of π_L is allowed by (a), (b) or (c) of R5.2 criterion 5 as P5.2.2 states them.
- **P3.2.3** For any Lemma L exported by Job j without solver-reported dependencies: `prov(L).deps = names(assertions(j) ∪ decls(j) ∪ partition_assumptions(j))`, `prov(L).used = delivered_{<export(L)}(j)` and `prov(L).negated_goal = true`.
- **P3.2.4** (Acyclicity) For any Job j, the graph whose nodes are the Lemmas admitted into j and which has an edge from L to each element of `prov(L).used` that was admitted into j is acyclic: no Lemma exported by j is admitted into j (R3.1 criterion 8), so R3.4 criterion 7 admits L only after every such element. No hash assumption is needed.
- **P3.2.5** (Provenance mismatch, instance of P5.2.6) For any Result r and any L ∈ `oblig(r)` whose obtained sub-proof has an open assumption that none of (a), (b) and (c) of R5.2 criterion 5 allows, `compose_plan` returns the error provenance mismatch and `label(r) = unchecked`.
- **P3.2.6** (Round trip) For any Provenance_Record p, `parse_prov(print_prov(p)) = p`.
- **P3.2.7** (Audit trail) For any Lemma_Dependent_Result r labelled certified or checked-incomplete, the Certificate_Store entry of r's Composed_Certificate contains `prov(x)` for every x ∈ `imports*(r)`, whether or not x is in `oblig(r)`, where `imports*` follows used-Lemma edges from Lemmas only (R5.1).
- **P3.2.8** (Tamper) For any authentic Lemma message m and any message m′ that keeps m's tag and differs from m in the Lemma text, the identifier or a Provenance_Record field, the Admission_Checker rejects m′.
- **P3.2.9** (Pool fidelity) For any Lemma L that the Lemma_Pool stores with identifier i, Provenance_Record p and authentication tag τ, every read and every forward of L returns the bytes of L, i, p and τ as the Lemma_Pool received them.

#### Verification

- A1 — Creusot contracts on the Provenance_Record well-formedness check and on the default Dependency_Set construction (P3.2.1, P3.2.3) — `crates/dp-core/src/provenance.rs` — `ci/creusot`.
- A1 — the used-Lemma premise of the Lean theorem `admission_sound_seq` (R3.4) — `proofs/lean/DP/Admission.lean` — `ci/lean`.
- A1 — Creusot contracts on `compose_plan` for the open-assumption rule of R5.2 criterion 5 (P3.2.2 and P3.2.5, as instances of P5.2.2 and P5.2.6), and on `admit` and the Worker delivery handler for used-Lemma order and the discard of a Job's own exports (P3.2.4) — `crates/dp-core/src/compose_plan.rs`, `crates/dp-core/src/admission.rs` — `ci/creusot`.
- A3 — the Certificate_Checker run on every Composed_Certificate against the locally printed Printed_Goal (R5.3), which checks the scoped Lemma sub-proofs in B's context and is the only soundness gate for Lemma_Dependent_Results — `crates/dp-cert/src/check.rs` — `ci/certs`, `ci/canaries`.
- A4 — round trip and fuzzing of the Provenance_Record codec (P3.2.6, R7.10) — `crates/dp-formats/tests/`, `fuzz/provenance/` — `ci/test`, `ci/fuzz-smoke`; property-based test of well-formedness, the identifier over received bytes and tampering (P3.2.1, P3.2.8) — `crates/dp-core/tests/prop_provenance.rs` — `ci/test`; property-based test of acyclicity over generated message sequences through the Worker delivery handler and `admit`, including a Job's own exports sent back to it (P3.2.4) — `crates/dp-core/tests/prop_admission.rs` — `ci/test`; property-based tests of `compose_plan` with open assumptions outside the rule of R5.2 criterion 5 (P3.2.2, P3.2.5) — `crates/dp-cert/tests/prop_compose.rs` — `ci/test`; property-based test of Lemma_Pool reads and forwards (P3.2.9) — `crates/dp-pool/tests/prop_pool.rs` — `ci/test`.
- A4 — deterministic simulation with Workers and a Lemma_Pool that report false provenance (P3.2.5) — `tests/sim/lying_provenance.rs` — `ci/sim`; Injected_False_Lemmas with understated Dependency_Sets — `tests/canaries/` — `ci/canaries`; mutation testing — `tools/mutants/` — `sched/mutants-weekly`.
- A4 — audit-trail check on Sessions from `ci/integration-cluster` (P3.2.7) — `tests/replay/` — `ci/replay`.

#### Trust

- None for soundness. The only soundness gate for Lemma_Dependent_Results is R5.3 (TB-3, TB-4, TB-11, AS-30); admission, hygiene and provenance are defense in depth and usefulness filters. False provenance that `compose_plan` misses (P3.2.5) still fails certificate checking in B's context. AS-23 (scoped sub-proofs), AS-24 (the Negated_Goal_Assertion can be identified). The usefulness of cross-goal sharing depends on AS-11.
- The Worker identity in a Provenance_Record is authentic only as far as Worker credentials are (TB-2); a Worker verifies another Worker's tag from that Worker's public key, which per-Worker Ed25519 signatures allow (OQ-N11 resolved by design decision D-9).

### Requirement 13: R3.3 Import hygiene

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As a proof engineer, I want Lemmas that mention foreign symbols rejected, so that Skolem constants and solver-internal names never leak into another Job.

In this requirement, the *vocabulary-independent Lemma grammar* is the SMT-LIB 2.6 term syntax of the Lemma format (R7.10) in which every symbol is accepted as an identifier, so that parsing under it needs no Canonical_Vocabulary. A Lemma whose bytes fail this grammar is malformed (R3.2 criterion 6). A Lemma that parses under it but fails the parse against the target's Canonical_Vocabulary of criterion 1, which resolves every identifier to a symbol of that vocabulary or of the target's SMT-LIB logic, fails hygiene.

#### Acceptance Criteria

1. WHEN the Admission_Checker evaluates a Lemma for a target Job, THE Admission_Checker SHALL parse the Lemma with the Lemma grammar of R7.10 against the target Printed_Goal's Canonical_Vocabulary and the theory symbols, indexed identifiers and literals of the target's SMT-LIB logic.
2. IF a Lemma mentions a function, constant or sort symbol outside the target's Canonical_Vocabulary and outside the theory symbols of the target's SMT-LIB logic, THEN THE Admission_Checker SHALL reject the Lemma with the reason category hygiene and SHALL log every such symbol.
3. IF a Lemma that parses under the vocabulary-independent Lemma grammar fails the parse of criterion 1 against the target's Canonical_Vocabulary, fails sort checking against the target's signature, or has a sort other than Bool, THEN THE Admission_Checker SHALL reject the Lemma with the reason category hygiene.
4. IF a Lemma is not a Ground_Lemma, THEN THE Admission_Checker SHALL reject the Lemma with the reason category hygiene (NG-6; Instantiation_Hints are admitted under R4.4).
5. IF a Lemma contains a binder (`forall`, `exists`, `let`, `match`, `lambda`), an attribute annotation (`!`, which can introduce a new name), or a free variable, THEN THE Admission_Checker SHALL reject the Lemma with the reason category hygiene (OQ-N40).
6. IF the printed form of a Lemma exceeds `<TBD-12>` bytes, THEN THE Admission_Checker SHALL reject the Lemma with the reason category malformed before parsing the Lemma.

#### Properties

- **P3.3.1** For any Lemma L and target T, `admit(L, T) ⇒ symbols(L) ⊆ CV(T) ∪ theory(logic(T))`, where `symbols` includes function, constant and sort symbols.
- **P3.3.2** For any Lemma L that mentions a symbol s with `s ∉ CV(T) ∪ theory(logic(T))`, including Skolem constants and solver-internal names in the naming styles of the pinned Z3 and cvc5, `admit(L, T) = false` and the log lists exactly `symbols(L) ∖ (CV(T) ∪ theory(logic(T)))`.
- **P3.3.3** For any byte string b and target T, the hygiene parser terminates without panic and either rejects b or returns a closed, binder-free, annotation-free, well-sorted term of sort Bool over `CV(T) ∪ theory(logic(T))`.
- **P3.3.4** (Round trip) For any closed, binder-free, well-sorted Bool term L over `CV(T) ∪ theory(logic(T))`, `parse_T(print(L)) = L`.
- **P3.3.5** (Vocabulary monotonicity) For any targets T ⊆ T′ such that every symbol of `CV(T)` has the same sort signature in T′ and `logic(T) = logic(T′)`: `hygiene_ok(L, T) ⇒ hygiene_ok(L, T′)`.
- **P3.3.6** (Size limit) For any byte string b with `|b| > <TBD-12>`, the Admission_Checker rejects b with the reason category malformed and the parser is not invoked on b.

#### Verification

- A1 — Creusot contracts on `hygiene_ok` for P3.3.1, P3.3.2 and P3.3.6 — `crates/dp-core/src/admission.rs` — `ci/creusot`.
- A1 — hygiene premise of the Lean theorem `admission_sound` (L is a closed, well-sorted formula over Σ_B) — `proofs/lean/DP/Admission.lean` — `ci/lean`.
- A3 — the Front_End re-runs the Admission_Checker, including hygiene, on every imported Lemma against the locally printed target (R3.4 criterion 9) — `crates/dp-frontend/src/accept.rs` — `ci/certs`, `ci/canaries`; the Certificate_Checker re-parses every imported Lemma in the target's signature (R5.3) — `crates/dp-cert/src/check.rs` — `ci/certs`.
- A2 — Kani bounded no-panic check of the Lemma parser (P3.3.3) — `verification/kani/parsers.rs` — `ci/kani`.
- A4 — property-based test with generated Skolem and solver-internal symbols in the naming styles of Z3 and cvc5, binders, annotations and oversized inputs (P3.3.1–P3.3.3, P3.3.5, P3.3.6) — `crates/dp-core/tests/prop_hygiene.rs` — `ci/test`; Lemma round trip (P3.3.4) — `crates/dp-formats/tests/` — `ci/test`; fuzzing — `fuzz/lemma_parser/` — `ci/fuzz-smoke`, `sched/fuzz-nightly`; mutation testing — `tools/mutants/` — `sched/mutants-weekly`.

#### Trust

- TB-14 (theory symbol tables of the pinned SMT-LIB standard).
- The only soundness gate for Lemma_Dependent_Results is R5.3 (TB-3, TB-4, TB-11, AS-30); admission, hygiene and provenance are defense in depth and usefulness filters.
- Hygiene is syntactic. A solver that prints a Skolem symbol under a name of the Canonical_Vocabulary produces an ordinary, possibly false, Lemma over the Canonical_Vocabulary; certificate checking catches it (R3.4, R5.3). Detecting honest Skolem symbols relies on Canonical_Names being disjoint from the naming styles of solver-introduced symbols (R4.1 criterion 9).
- Criterion 5 keeps rejecting `let` (OQ-N40 resolved by design decision D-14); AS-40 (Sharing_Solver patches print exported Lemmas without `let`) decides how many Lemmas survive, which affects usefulness only.

### Requirement 14: R3.4 Lemma admission rule

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As an auditor, I want one formally proved admission rule, so that only Lemmas that can be justified from the goal's own Assertions enter the goal.

In this requirement:

- The *target problem* Γ_b of a Job b of Goal_Version B is the set of Assertions and declarations of B's Printed_Goal together with b's Partition_Assumptions where partitioning is enabled (none otherwise): `Γ_b = Γ_B ∪ PA(b)`.
- The *available Lemmas* of b at a step are the Lemmas and Instantiation_Hints admitted into b before that step and the Lemmas and Instantiation_Hints exported by b before that step (R3.1), the latter identified by their identifiers (R3.2 criterion 3). A Lemma exported by b and counted as available is not admitted by that count: when a Result of b depends on it, it is in the Result's import closure and is certified under R5 like any other Lemma.
- The *re-admission pass* of a Result r of Goal_Version B is one pass over r's whole import list in import-list order. The pass calls `admit` for each Lemma and `admit_hint` (R4.4) for each Instantiation_Hint against the target problem built from B's locally printed Printed_Goal and, where partitioning is enabled and r is for a Partition, the Partition_Assumptions that the recorded Partition tree gives r's Partition. The pass keeps one admitted sequence, empty at the start, which every admitted Lemma and Instantiation_Hint extends, and counts as available, for criterion 7, every used Lemma whose Provenance_Record names as source Job a Job of the Session with r's goal path, Solver_Configuration and Partition, resolved by identifier (R3.2 criterion 3).

#### Acceptance Criteria

1. WHEN the Admission_Checker evaluates a Lemma L from source Goal_Version A for a Job b of target Goal_Version B, THE Admission_Checker SHALL admit L only if every element of L's Dependency_Set is the Canonical_Name of a declaration, Assertion or Partition_Assumption of b's target problem of the kind that the Canonical_Name encodes (R4.1 criterion 5).
2. WHEN L's Provenance_Record flag states that the Dependency_Set contains A's Negated_Goal_Assertion, THE Admission_Checker SHALL treat A's Negated_Goal_Assertion as an element of the Dependency_Set that must be present in b's target problem.
3. WHERE partitioning is enabled, IF L's Dependency_Set contains a Partition_Assumption that is not a Partition_Assumption of the target Job, THEN THE Admission_Checker SHALL reject L with the reason category partition.
4. IF the source Job and the target Job differ in Driver hash or Transformation_Pipeline hash, or the SMT-LIB logic that L's Provenance_Record records differs from the `set-logic` line of the target Printed_Goal, THEN THE Admission_Checker SHALL reject L with the reason category pipeline mismatch.
5. THE Admission_Checker SHALL compute every admission decision as a pure function of the Lemma's received bytes, its identifier, authentication tag and Provenance_Record, the target Goal_Version B, the canonical declarations and Assertions of B, the SMT-LIB logic of B's Printed_Goal, the target Job's Partition_Assumptions, Driver hash and Transformation_Pipeline hash, whether the Canonicalizer reported a canonicalization failure for the target's Printed_Goal (R4.1 criterion 12), the sequence of Lemmas and Instantiation_Hints already admitted into the target Job, the identifiers of the Lemmas and Instantiation_Hints that the target Job has exported, and the registered and revoked Worker credentials (R7.3).
6. WHEN the Admission_Checker rejects a Lemma, THE Admission_Checker SHALL log exactly one reason category, the first failing check in the order: malformed (R3.2 criterion 6, R3.3 criterion 6), authentication (R3.2 criterion 7), target canonicalization failure, with the reason category hygiene (R4.1 criterion 12), pipeline mismatch (criterion 4), hygiene (R3.3), partition (criterion 3, where partitioning is enabled), cross-goal Lemma with the negated-goal flag set, with the reason category missing dependency (criterion 11), missing dependency (criteria 1 and 2), used-Lemma order (criterion 7).
7. IF a used Lemma of L (R3.2 criterion 1) is not an available Lemma of the target Job when the Admission_Checker evaluates L, THEN THE Admission_Checker SHALL reject L with the reason category used-Lemma order.
8. WHEN a Lemma passes every check of R3.2 criteria 6 and 7, the target canonicalization-failure check (R4.1 criterion 12), R3.3, criteria 1 to 4, criterion 11 and criterion 7, THE Admission_Checker SHALL admit the Lemma.
9. WHEN the Verdict_Recorder accepts a Lemma_Dependent_Result that reports a proof, THE Verdict_Recorder SHALL run the re-admission pass of that Result before certificate composition (R5.2).
10. IF the re-admission pass of criterion 9 rejects an imported Lemma, or the Verdict_Recorder cannot obtain for an imported identifier a Lemma and Provenance_Record whose hash equals that identifier (R3.2 criterion 3), THEN THE Verdict_Recorder SHALL label the Result unchecked and SHALL log the reason category.
11. IF L's source Goal_Version A differs from the target Goal_Version B, compared by goal path and Goal_Hash, and L's Provenance_Record flag states that the Dependency_Set contains A's Negated_Goal_Assertion, THEN THE Admission_Checker SHALL reject L with the reason category missing dependency.

#### Properties

- **P3.4.1** (Admission soundness, metatheory) In the Lean model of AS-12, for any source and target Printed_Goals A and B with signatures Σ_A and Σ_B (declared and defined symbols, recursive definitions, sort definitions and datatypes) and SMT-LIB logics `logic(A)` and `logic(B)`, any source Job a and target Job b with target problems `Γ_a = Γ_A ∪ PA(a)` and `Γ_b = Γ_B ∪ PA(b)`, any Lemma L and any Δ ⊆ Γ_a with `Δ ⊨ L` in Σ_A under `logic(A)`: if `admit(L, Δ, b)` holds, which requires Δ ⊆ Γ_b, L closed and well-sorted over Σ_B, `logic(A) = logic(B)`, and identical declarations (R4.1) in Σ_A and Σ_B for every symbol reachable from L ∪ Δ in the reference graph, then `Γ_b ⊨ L` in Σ_B under `logic(B)`, and therefore `Γ_b ∪ {L}` unsatisfiable implies `Γ_b` unsatisfiable. Lean theorem `admission_sound`.
- **P3.4.2** (Partition locality, where partitioning is enabled) For any Lemma L whose Dependency_Set contains a Partition_Assumption c, and any target Job t with `c ∉ partition_assumptions(t)`, `admit(L, t) = false`. Lean theorem `partition_local`.
- **P3.4.3** (Refinement) For any input x, the Rust `admit(x)` equals the Lean reference definition `admit(x)`, and the logged reason category of the Rust implementation equals that of the Lean reference.
- **P3.4.4** (Monotonicity) For any L and Jobs b, b′ with equal Driver and Transformation_Pipeline hashes, equal SMT-LIB logics, equal registered and revoked credentials, equal admitted-Lemma sequences, `exports(b) ⊆ exports(b′)`, no canonicalization failure for b′, Goal_Versions that are both equal to or both different from L's source Goal_Version, and a target problem of b′ that contains the target problem of b with identical declarations: `admit(L, b) ⇒ admit(L, b′)`.
- **P3.4.5** (Sequential admission soundness, metatheory) In the Lean model of AS-12, for any target Job b with target problem `Γ_b = Γ_B ∪ PA(b)` and any sequence X_1, …, X_n of Lemmas, each admitted into b or exported by b, in the order of their admission or export, where each X_i has Dependency_Set Δ_i ⊆ Γ_b and used Lemmas U_i ⊆ {X_j | j < i}, and `Δ_i ∪ U_i ⊨ X_i` holds in the source signature of X_i under the premises of P3.4.1 (equal SMT-LIB logic, and identical declarations for every symbol reachable from X_i ∪ Δ_i ∪ U_i): `Γ_b ⊨ X_i` for every i, and therefore `Γ_b ∪ {X_1, …, X_n}` unsatisfiable implies `Γ_b` unsatisfiable. Lean theorem `admission_sound_seq`.
- **P3.4.6** For any L whose source Job differs from the target Job t in Driver hash or Transformation_Pipeline hash, or whose Provenance_Record records an SMT-LIB logic other than that of t's Printed_Goal, `admit(L, t) = false`.
- **P3.4.7** For any L with a used Lemma that is neither in the admitted-Lemma sequence of t nor among the Lemmas and Instantiation_Hints exported by t, `admit(L, t) = false`.
- **P3.4.8** (Completeness) For any L and t that pass every check named in criterion 8, `admit(L, t) = true`.
- **P3.4.9** (Reason determinism) For any rejected pair (L, t), the logged reason category is the first failing check in the order of criterion 6.
- **P3.4.10** (Front_End re-admission) For any Result r labelled certified, the re-admission pass of r (criterion 9: one pass over `r.imports` in import-list order, calling `admit` and `admit_hint` with one admitted sequence) admitted every element of `r.imports` against the target problem built from `print_local` and, where partitioning is enabled, `PA(r)`.
- **P3.4.11** (Within-goal usefulness) For any well-formed, authentic, hygienic Lemma L exported with the default Dependency_Set (P3.2.3) by Job a of Goal_Version G, and any Job b of G with the same Driver and Transformation_Pipeline hashes and SMT-LIB logic, `partition_assumptions(a) ⊆ partition_assumptions(b)` and every used Lemma of L already admitted into or exported by b: `admit(L, b) = true`.
- **P3.4.12** (Lean model fidelity, AS-12) In the Lean model: (a) for A with `(define-fun k () Int 1)`, B with `(define-fun k () Int 2)`, `L = (= k 1)` and Δ = ∅, the premise of `admission_sound` fails because k does not have identical declarations, and `Γ_B ⊨ L` is false; (b) there are two models of the integer and real theories that both satisfy the SMT-LIB 2.6 theory definitions and differ in the values of `(div x 0)`, `(mod x 0)` and `(/ x 0.0)`. Lean theorems `model_defined_symbols` and `model_div_zero_unspecified`.
- **P3.4.13** (Cross-goal negated goal) For any Lemma L with `prov(L).negated_goal = true` and any Job t whose Goal_Version differs from L's source Goal_Version, including a Goal_Version with the same goal path and base Task_Hash: `admit(L, t) = false`, with the reason category missing dependency unless an earlier check of criterion 6 fails.

#### Verification

- A1 — Lean 4 theorems `admission_sound`, `partition_local` and `admission_sound_seq` over the signatures and theory semantics of AS-12 — `proofs/lean/DP/Admission.lean` — `ci/lean` (fails on `sorry` or added axioms).
- A1 — Lean 4 sanity theorems `model_defined_symbols` and `model_div_zero_unspecified` (P3.4.12) — `proofs/lean/DP/ModelSanity.lean` — `ci/lean`.
- A1 — Creusot contracts that state the admission specification on `admit` (P3.4.2, P3.4.6–P3.4.9, P3.4.13) — `crates/dp-core/src/admission.rs` — `ci/creusot`.
- A3 — per-Result re-admission pass in the Front_End against the locally printed target (criteria 9 and 10, P3.4.10) — `crates/dp-frontend/src/accept.rs` — `ci/certs`, `ci/canaries`; every Lemma_Dependent_Result is proved only after its Composed_Certificate passes the Certificate_Checker in B's context (R5.3) — `crates/dp-cert/src/check.rs` — `ci/certs`, `ci/canaries`.
- A4 — differential test of the Rust `admit` against the compiled Lean reference (P3.4.3) — `tests/diff/admission_lean_vs_rust/` — `ci/diff`; property-based tests with generated SMT-LIB logic mismatches, targets with canonicalization failure, Goal_Versions that share goal path and base Task_Hash, and used Lemmas exported by the target (P3.4.4, P3.4.6–P3.4.9, P3.4.11, P3.4.13) — `crates/dp-core/tests/prop_admission.rs` — `ci/test`; Injected_False_Lemmas with forged Dependency_Sets and Partition_Assumptions — `tests/canaries/` — `ci/canaries`; deterministic simulation with false provenance — `tests/sim/lying_provenance.rs` — `ci/sim`; mutation testing — `tools/mutants/` — `sched/mutants-weekly`.

#### Trust

- TB-5 (Lean 4 kernel); AS-12. Criterion 1 checks membership by Canonical_Name and kind only; identical content follows from R4.1 criterion 3 and P4.1.1 under TB-1.
- The only soundness gate for Lemma_Dependent_Results is R5.3 (TB-3, TB-4, TB-11, AS-30); admission, hygiene and provenance are defense in depth and usefulness filters. Within one Goal_Version and one Driver and Transformation_Pipeline (criterion 4), source and target share the Printed_Goal, so identical canonical content reduces to identical text. Across goals, canonical naming (R4.1) matters for usefulness only, since the A3 checks run in B's context.
- A Lemma exported by the target Job and counted as available under criterion 7 is not trusted: it is in the import closure of every Result that depends on it and needs a Lemma proof and a sub-proof under R5.1 and R5.2.

### Requirement 15: R3.5 Partitioned search

**Priority:** Could

**Class:** Soundness-critical

**User Story:** As a proof engineer, I want to split a hard goal into Partitions solved in parallel, so that very hard goals can use more machines.

In this requirement, the *cube* of a leaf Partition is the conjunction of the Partition_Assumptions on the path from the root of the Partition tree to that leaf, and the *leaf-Result set* of a partitioned Goal_Version is its set of proof-reporting Results, one per leaf Partition of the recorded Partition tree, namely the first such Result accepted for each leaf (R5.1 criterion 10), which R5.1 to R5.4 treat as one Result. Criterion 2 governs only Verdicts obtained from leaf Results: a whole-Goal_Version Job in the same Race is labelled under R7.1 criteria 2 and 3 and can still win under R2.2 criterion 8, with the label proved-standalone or certified.

#### Acceptance Criteria

1. WHERE partitioning is enabled, WHEN the Scheduler splits a Goal_Version into Partitions, THE Scheduler SHALL record the Partition tree with every Partition_Assumption and with one Driver hash and one Transformation_Pipeline hash for every Partition of the tree.
2. WHERE partitioning is enabled, THE Verdict_Recorder SHALL record a proof-labelled Verdict obtained from the leaf Results of a partitioned Goal_Version only with the label certified, from one Composed_Certificate that combines a proof for the cube of every leaf Partition with a case analysis showing that the disjunction of the leaf cubes is valid, and that the Certificate_Checker accepts against the Goal_Version's locally printed Printed_Goal (R5.2, R5.3; OQ-N41).
3. WHERE partitioning is enabled, THE Admission_Checker SHALL apply R3.4 criterion 3 to every Lemma whose Dependency_Set contains a Partition_Assumption.
4. WHERE partitioning is enabled, WHEN the Verdict_Recorder receives a Result for a Partition, THE Verdict_Recorder SHALL recompute the Task_Hash from the locally printed Printed_Goal with the Partition_Assumptions that the recorded Partition tree gives that Partition.
5. IF a Result for a Partition names a Partition absent from the recorded Partition tree, carries a Solver_Configuration whose Driver hash or Transformation_Pipeline hash differs from the one that the recorded Partition tree records, or carries a Task_Hash that differs from the Task_Hash recomputed under criterion 4, THEN THE Verdict_Recorder SHALL discard the Result, log the reason, and leave the state of the Goal_Version unchanged.
6. IF the Certificate_Checker does not accept the Composed_Certificate of criterion 2, THEN THE Verdict_Recorder SHALL record no proof label for the partitioned Goal_Version from that certificate.
7. WHERE partitioning is enabled, WHEN the Verdict_Recorder labels the leaf-Result set of a partitioned Goal_Version certified under criterion 2 while the Race of that Goal_Version has not ended, THE Scheduler SHALL end the Race with that leaf-Result set as the winner, so that R1.5 criterion 3 cancels its remaining Jobs.
8. WHERE partitioning is enabled, WHEN the Scheduler ends a Race under criterion 7, THE Verdict_Recorder SHALL record the Verdict of the Goal_Version with the label certified and the Result identifier of every leaf Result of the winning leaf-Result set (R2.2 criterion 9).
9. WHERE partitioning is enabled, WHEN the Scheduler creates a Job for a Partition (R2.2 criterion 21), THE Scheduler SHALL use only a Solver_Configuration whose Driver hash and Transformation_Pipeline hash equal those that the recorded Partition tree records, and SHALL log every Solver_Configuration of the Race's configuration list that it skips for the Partition tree for that reason.
10. THE Front_End SHALL read the `partitioning` option from the project configuration, with the default off, and SHALL treat partitioning as enabled in a Session only when that option is on (OQ-N14 resolved by design decision D-19).

#### Properties

- **P3.5.1** (Cover soundness, metatheory) For any Γ, goal φ and leaf cubes c_1 … c_n with `⊨ c_1 ∨ … ∨ c_n`: if each `Γ ∪ {¬φ, c_i}` is unsatisfiable, then `Γ ⊨ φ`. Lean theorem `partition_sound`.
- **P3.5.2** For any Partition tree whose leaf cubes do not form a valid cover, no proof-labelled Verdict is recorded from the leaf Results.
- **P3.5.3** (Partition binding) For any Result r that the Verdict_Recorder accepts for Partition p of Goal_Version G: `r.task_hash = H(print_local(G.path, r.config.driver, r.config.pipeline) ⊕ PA(p))`, where `PA(p)` is taken from the recorded Partition tree.
- **P3.5.4** (Label) For any partitioned Goal_Version G with a proof-labelled Verdict v recorded from leaf Results: `v.label = certified` and the Certificate_Checker accepted v's Composed_Certificate against `print_local(G)` with no Trust_Steps.
- **P3.5.5** (Partition winners) For any Race ρ on a partitioned Goal_Version G: no single Result for a Partition of G is the winner under R2.2 criterion 8; if ρ's winner is a leaf-Result set, then ρ ended under criterion 7, the set holds exactly one proof-reporting Result per leaf of the recorded Partition tree, the set's label is certified, and the Verdict of G is certified and names the Result identifier of every element of the set; and if a whole-Goal_Version Result of ρ receives a proof label while ρ has not ended, that Result is the winner under R2.2 criterion 8 with its label (proved-standalone or certified).
- **P3.5.6** (Split trees cover, metatheory) For any Partition tree in which every internal node splits on a literal l into exactly two children that add l and ¬l, `⊨ ⋁_{leaves} cube(leaf)`. Lean theorem `split_tree_covers`.
- **P3.5.7** (Round trip) For any Partition tree t, including its recorded Driver hash and Transformation_Pipeline hash, `parse_tree(print_tree(t)) = t`.
- **P3.5.8** (One Driver and Transformation_Pipeline) For any recorded Partition tree T and any Job j for a Partition of T: `driver(j) = T.driver ∧ pipeline(j) = T.pipeline`; and for any Result r for a Partition of T with `r.config.driver ≠ T.driver` or `r.config.pipeline ≠ T.pipeline`, r is discarded under criterion 5.
- **P3.5.9** (Partitioned verdict soundness, metatheory) In the Lean trace model of P7.1.2 extended with Partition Jobs, recorded Partition trees and leaf-Result sets, under the hypotheses `CheckerSound` and `NoCollision` of P7.1.2, for any partitioned Goal_Version G and any Verdict v of G recorded from a leaf-Result set with `v.label = certified`: `assertions(print_local(G))` is unsatisfiable. Lean theorem `verdict_sound_partition`, with no added axioms. It is separate from `verdict_sound` so that the gate of R7.1 does not depend on this requirement.
- **P3.5.10** (Off by default) For any Session whose project configuration does not set the `partitioning` option, or sets it off, the Scheduler records no Partition tree and creates no Job for a Partition.

#### Verification

- A1 — Lean 4 theorems `partition_sound` (P3.5.1), `split_tree_covers` (P3.5.6), `compose_partition_sound` (P5.2.7, R5.2) and `verdict_sound_partition` (P3.5.9) — `proofs/lean/DP/Partition.lean` — `ci/lean-partition` (fails on `sorry` or added axioms; a separate job so that `ci/lean`, which gates the Must requirements, does not depend on partitioning).
- A3 — per-run check of the case analysis inside the Composed_Certificate by the Certificate_Checker — `crates/dp-cert/src/partition.rs` — `ci/certs`; per-Result Partition Task_Hash recomputation in the Front_End (criteria 4 and 5) — `crates/dp-frontend/src/accept.rs` — `ci/sim`, `ci/canaries`.
- A1 — Creusot contracts on the race decision function for leaf-Result-set winners and on the Partition Job creation and Result discard checks (P3.5.5, P3.5.8) — `crates/dp-core/src/race.rs`, `crates/dp-core/src/verdict.rs` — `ci/creusot`.
- A4 — property-based tests with incomplete covers, plus canaries — `crates/dp-cert/tests/prop_partition.rs`, `tests/canaries/partition/` — `ci/test`, `ci/canaries`; property-based test of Races that mix whole-Goal_Version Jobs and Partition Jobs, with proof labels arriving in every order (P3.5.5), and of Sessions with the `partitioning` option unset or off (P3.5.10) — `crates/dp-core/tests/prop_race.rs` — `ci/test`; deterministic simulation with an adversarial Coordinator that swaps Partition Results and submits Partition Results with another Driver or Transformation_Pipeline (P3.5.3, P3.5.5, P3.5.8) — `tests/sim/adversarial_coordinator.rs` — `ci/sim`; Partition tree round trip (P3.5.7) — `crates/dp-formats/tests/` — `ci/test`; mutation testing — `tools/mutants/` — `sched/mutants-weekly`.

#### Trust

- TB-3 and TB-4 (the Certificate_Checker) for the case analysis inside the Composed_Certificate; AS-6. The Partition tree format is in the R7.10 list. Partitioning stays in scope as a Could, behind the `partitioning` option, off by default and delivered last within dp-within-goal-sharing, with its Lean gate `ci/lean-partition` separate from `ci/lean` (criterion 10; OQ-N14 resolved by design decision D-19).

### Requirement 16: R3.6 Lemma_Pool resource bounds and hostile input

**Priority:** Should

**Class:** Robustness

**User Story:** As a cluster operator, I want the Lemma_Pool to stay within resource bounds and survive malformed or hostile input.

In this requirement, a *pool item* is a Lemma or an Instantiation_Hint, stored with its identifier, Provenance_Record and authentication tag (R3.2 criterion 5, R4.4 criterion 5).

#### Acceptance Criteria

1. THE Lemma_Pool SHALL store at most `<TBD-11>` pool items, Lemmas and Instantiation_Hints counted together, per Goal_Version per Session.
2. IF the Lemma_Pool receives a pool item for a Goal_Version of a Session that already has `<TBD-11>` stored pool items, THEN THE Lemma_Pool SHALL discard the received pool item, log the discard, and leave the stored pool items unchanged (OQ-N42).
3. IF the Lemma_Pool receives a Lemma whose printed form exceeds `<TBD-12>` bytes, or an Instantiation_Hint whose printed form exceeds `<TBD-35>` bytes, THEN THE Lemma_Pool SHALL discard the pool item before parsing it and log the discard.
4. IF storing a received pool item would make the total size of the stored pool items and their Provenance_Records exceed `<TBD-34>` bytes, THEN THE Lemma_Pool SHALL discard the received pool item, log the discard, and leave the stored pool items unchanged.
5. IF the Lemma_Pool receives a pool item or Provenance_Record that fails to parse under R7.10, or a pool item whose identifier differs from the hash of R3.2 criterion 3 (Lemmas) or R4.4 criterion 3 (Instantiation_Hints), THEN THE Lemma_Pool SHALL discard the message, log the discard, and leave the stored pool items unchanged.
6. WHEN the Lemma_Pool receives a pool item whose canonical printed form, Dependency_Set and used Lemmas equal those of a stored pool item of the same Session and Goal_Version, THE Lemma_Pool SHALL keep the stored pool item unchanged and discard the received copy.
7. WHEN the Lemma_Pool receives a Session-end message from the Front_End, or a Session-drop message from the Coordinator under R1.6 criterion 16, THE Lemma_Pool SHALL delete every stored pool item of that Session.
8. WHEN the Lemma_Pool restarts, THE Lemma_Pool SHALL start with no stored pool items (OQ-N15).

#### Properties

- **P3.6.1** (Bounds) For any sequence of input byte strings, the Lemma_Pool handles every request without crashing, and after each request every (Session, Goal_Version) pair has at most `<TBD-11>` stored pool items, every stored Lemma has a printed form of at most `<TBD-12>` bytes, every stored Instantiation_Hint has a printed form of at most `<TBD-35>` bytes, and the total stored size is at most `<TBD-34>` bytes.
- **P3.6.2** (Idempotence) For any pool state P and pool item x, `insert(insert(P, x), x) = insert(P, x)`.
- **P3.6.3** (Discard is a no-op) For any pool state P and any message m that criteria 2 to 6 discard, `step(P, m)` equals P except for one appended log entry.
- **P3.6.4** (First wins) For any pool state P and pool items x1, x2 of the same Session and Goal_Version with equal canonical printed form, Dependency_Set and used Lemmas, `insert(insert(P, x1), x2) = insert(P, x1)`.
- **P3.6.5** (Session deletion) For any pool state P and Session S, after a Session-end or Session-drop message for S, P stores no pool item of S, and the stored pool items of every other Session are unchanged.
- **P3.6.6** (Restart) For any trace, the Lemma_Pool stores no pool item immediately after a restart event; together with P3.1.7, a restart loses only pool items.

#### Verification

- A2 — Kani bounded check that `insert` and `delete_session` preserve the bounds of P3.6.1 — `verification/kani/pool.rs` — `ci/kani`.
- A4 — fuzzing of Lemma_Pool ingestion of Lemma and Instantiation_Hint messages (P3.6.1) — `fuzz/lemma_pool_ingest/` — `ci/fuzz-smoke`, `sched/fuzz-nightly`.
- A4 — property-based tests of idempotence, discard, first-wins, bounds and Session deletion over mixed Lemmas and Instantiation_Hints (P3.6.1–P3.6.5) — `crates/dp-pool/tests/prop_pool.rs` — `ci/test`.
- A4 — simulation with hostile Lemma_Pool messages and Lemma_Pool restarts (P3.6.1, P3.6.6) — `tests/sim/hostile_pool.rs`, `tests/sim/faults/` — `ci/sim`, `sched/sim-nightly`.

#### Trust

- None. The Lemma_Pool is untrusted; soundness does not depend on the Lemma_Pool. One Job can fill a Goal_Version's quota and starve other Jobs of pool items; that is a performance question (OQ-N42).

---

**Group R4: Cross-goal and quantified sharing**

### Requirement 17: R4.1 Canonical naming

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As a proof engineer, I want symbols named canonically across Sibling_Goals, so that equal names always mean identical declarations and Lemmas can move between goals.

In this requirement:

- The *source name* of a declared symbol is the name the Driver printed for it. The *kind* of a declared symbol is sort, or function or constant (criterion 5); a symbol defined by `define-fun`, `define-fun-rec` or `define-funs-rec` has the kind function or constant, and a sort defined by `define-sort` has the kind sort. The *reference graph* of a Printed_Goal has an edge from each declared symbol to every declared symbol that occurs in its sort signature, its definition body or its axiom set. The *axiom set* of every declared symbol is empty (OQ-N45 resolved by design decision D-12): Canonical_Names cover source name, kind, sort signature and definition body, and Hypotheses that axiomatize a symbol take part in sharing only through Dependency_Sets. AS-26 is not relied on.
- Declared symbols of two Printed_Goals P and Q are *matched* by the pair (source name, kind), because SMT-LIB 2.6 keeps sort symbols and function symbols in separate namespaces: the match `m(g)` of a declared symbol g of P is the declared symbol of Q with the same source name and kind, if Q has one. A declared symbol f of P and a declared symbol f′ of Q have *identical declarations* when `f′ = m(f)` and every symbol g in the reflexive-transitive closure of P's reference graph from f, f itself included, has a match `m(g)` in Q with the same sort signature, definition body and axiom set, compared after replacing every declared symbol h that occurs in them by `m(h)`. For a defined symbol, the definition body is part of this check. The relation is symmetric: f of P and f′ of Q have identical declarations exactly when f′ of Q and f of P do (P4.1.15).
- The *canonical formula* of an Assertion is its term after three steps, in this order: every reference to a `:named` label is replaced by the labelled term (criterion 15); every `:named` label and every attribute other than patterns (`:pattern`, `:no-pattern`), including `:qid`, `:weight` and `:skolemid`, is removed; and every declared symbol is replaced by its Canonical_Name and every bound variable by a name fixed by its binder position. Sort annotations and patterns are kept.
- The *name map* of a Printed_Goal P without canonicalization failure is the bijection, computed by the Canonicalizer for P, between the pairs (source name, kind) of P's declared symbols and their Canonical_Names.
- The *Canonical_Name grammar* and the *solver-introduced name patterns* (the naming styles of Skolem symbols, fresh constants, purification and other internal symbols of the pinned Z3 and cvc5) are recorded in `docs/formats/`.
- The *goal marker* is the form in which the Driver prints the Negated_Goal_Assertion (AS-24).
- A *canonicalization failure* is the Canonicalizer's report that it cannot assign Canonical_Names to a Printed_Goal.
- The Canonicalizer post-processes the SMT-LIB text of Printed_Goals in `crates/dp-canon/`, on the Front_End and on the Workers; Printed_Goals, Task_Hashes and solver inputs stay as the Driver printed them (OQ-N5 resolved by design decision D-5).

#### Acceptance Criteria

1. THE Canonicalizer SHALL assign a Canonical_Name to every declared sort, function and constant symbol (including datatype constructors, selectors and testers, and the symbols defined by `define-fun`, `define-fun-rec`, `define-funs-rec` and `define-sort`), every Assertion and every Partition_Assumption of a Printed_Goal.
2. THE Canonicalizer SHALL compute every Canonical_Name as a function of only the Printed_Goal text, its Partition_Assumptions, the Goal's why3find goal path, the Driver hash and the Transformation_Pipeline hash.
3. THE Canonicalizer SHALL assign equal Canonical_Names to two declared symbols, or to two Assertions, only if both were printed with the same Driver hash and Transformation_Pipeline hash, have the same kind, and have identical declarations or identical canonical formulas respectively.
4. THE Canonicalizer SHALL assign equal Canonical_Names to two declared symbols with identical declarations, and to two Hypotheses or two Partition_Assumptions with identical canonical formulas, in Printed_Goals printed with the same Driver hash and Transformation_Pipeline hash.
5. THE Canonicalizer SHALL encode in each Canonical_Name its kind: sort, function or constant, Hypothesis, Negated_Goal_Assertion, or Partition_Assumption.
6. THE Canonicalizer SHALL identify as the Negated_Goal_Assertion of a Printed_Goal the single Assertion printed in the goal marker form (AS-24).
7. THE Canonicalizer SHALL derive the Canonical_Name of a Negated_Goal_Assertion from its canonical formula together with the why3find goal path and the base Task_Hash (R3.1) of the Printed_Goal.
8. THE Canonicalizer SHALL assign only Canonical_Names that match the Canonical_Name grammar.
9. THE Canonicalizer SHALL use a Canonical_Name grammar that matches no SMT-LIB reserved word, no theory symbol of an SMT-LIB logic in use, and no string that matches a solver-introduced name pattern (AS-27).
10. IF a Printed_Goal fails to parse under the Canonicalizer's SMT-LIB script grammar, or contains no Assertion or more than one Assertion in the goal marker form, THEN THE Canonicalizer SHALL report a canonicalization failure that names the Printed_Goal's Task_Hash.
11. IF the Canonicalizer derives one Canonical_Name for two declared symbols or Assertions that do not have identical declarations or identical canonical formulas, within one Printed_Goal or across the Printed_Goals that the Front_End canonicalizes in one Session, THEN THE Canonicalizer SHALL report a canonicalization failure for each Printed_Goal involved.
12. IF the Canonicalizer reports a canonicalization failure for the Printed_Goal of a target Job, THEN THE Admission_Checker SHALL reject every Lemma and Instantiation_Hint for that Job, with the reason category hygiene when neither the malformed check nor the authentication check fails, as the target canonicalization-failure check that follows authentication in the orders of R3.4 criterion 6 and R4.4 criterion 9.
13. IF the Canonicalizer reports a canonicalization failure for the Printed_Goal of a Job, THEN THE Worker SHALL export no Lemma and no Instantiation_Hint from that Job and SHALL log the failure.
14. WHEN the Verdict_Recorder runs the re-admission pass of R3.4 criterion 9, THE Front_End SHALL supply only the Canonical_Vocabulary, Canonical_Names and canonical formulas that the Canonicalizer computes from the locally printed Printed_Goal, and none received from a Worker, the Coordinator or the Lemma_Pool.
15. WHEN the Canonicalizer computes the canonical formulas of a Printed_Goal, THE Canonicalizer SHALL replace every reference to a `:named` label with the labelled term before removing the labels.
16. THE Worker SHALL give the Sharing_Solver of a Job the Job's Printed_Goal as the Driver printed it, with source names.
17. WHEN the Worker exports a Lemma or Instantiation_Hint from a Job, THE Worker SHALL rename every source name in the Lemma or in the hint's terms to its Canonical_Name with the name map of the Job's Printed_Goal, leaving the theory symbols of that Printed_Goal's SMT-LIB logic unchanged.
18. WHEN the Worker delivers a Lemma to a Job, or computes the instance of an Instantiation_Hint for a Job (R4.4 criterion 14), THE Worker SHALL rename every Canonical_Name in the Lemma or in the hint's terms to its source name with the name map of the Job's Printed_Goal.
19. IF a symbol of a Lemma or Instantiation_Hint that the Worker renames under criterion 17 or 18 has no entry in the name map and is not a theory symbol of the Printed_Goal's SMT-LIB logic, THEN THE Worker SHALL drop the Lemma or Instantiation_Hint, SHALL send it neither to the Lemma_Pool nor to the solver, and SHALL log the drop with every unmapped symbol.

#### Properties

- **P4.1.1** (Name injectivity) For any Printed_Goals P and Q, any declared symbols or Assertions x of P and y of Q with `name_P(x) = name_Q(y)`: `kind(x) = kind(y)`, `driver(P) = driver(Q)`, `pipeline(P) = pipeline(Q)`, and x and y have identical declarations (symbols) or identical canonical formulas (Assertions), assuming a collision-free hash. Lean theorem `canon_injective`.
- **P4.1.2** (Bound-variable invariance) For any Printed_Goal P and any capture-avoiding renaming ρ of P's bound variables, `canon(ρ(P)) = canon(P)`. Invariance under renaming of declared symbols is not required: the source name is part of a declaration so that distinct uninterpreted symbols of the same sort keep distinct Canonical_Names.
- **P4.1.3** (Idempotence) For any term φ, `norm(norm(φ)) = norm(φ)`, where `norm` replaces every label reference by its labelled term, removes `:named` labels and every attribute other than patterns, and replaces bound variables by binder-position names.
- **P4.1.4** (Equisatisfiability) For any Printed_Goal P without canonicalization failure, the canonical printed form of P is P under a renaming that is injective on P's declared symbols, with every label reference replaced by its labelled term and with `:named` labels and every attribute other than patterns removed, so P and its canonical printed form are equisatisfiable. Lean theorem `canon_equisat`, whose model includes `:named` labels and references to them.
- **P4.1.5** (Converse) For any Printed_Goals P and Q with `driver(P) = driver(Q)` and `pipeline(P) = pipeline(Q)`, any declared symbols x of P and y of Q with identical declarations, and any Hypotheses or Partition_Assumptions x of P and y of Q of the same kind with identical canonical formulas: `name_P(x) = name_Q(y)`.
- **P4.1.6** (Determinism) For any input of criterion 2, two runs of the Canonicalizer, in different processes, on different machines and with different hash-map iteration seeds, return identical Canonical_Name assignments.
- **P4.1.7** (Name disjointness) For any string s that is an SMT-LIB reserved word, a theory symbol of a logic in use, or a match of a solver-introduced name pattern, s is not in the language of the Canonical_Name grammar; and every name that `canon` assigns is in that language.
- **P4.1.8** (Negated goal identification) For any Printed_Goal P that parses: if exactly one Assertion a of P is in the goal marker form, `canon` identifies a as P's Negated_Goal_Assertion; otherwise `canon` reports a canonicalization failure.
- **P4.1.9** (Negated goal separation) For any Printed_Goals P and Q with different why3find goal paths or different base Task_Hashes, the Canonical_Name of P's Negated_Goal_Assertion is not a Canonical_Name in Q, assuming a collision-free hash.
- **P4.1.10** (Failure blocks admission) For any target Job t whose Printed_Goal has a canonicalization failure and any Lemma or Instantiation_Hint x, `admit(x, t) = false`.
- **P4.1.11** (Collision detection) With the hash function replaced by a deliberately weak test hash, for any set of Printed_Goals in which two declared symbols or Assertions without identical declarations or canonical formulas receive one Canonical_Name, `canon` reports a canonicalization failure for every Printed_Goal involved.
- **P4.1.12** (Local re-derivation) For any re-admission run by the Front_End for Goal_Version B, the Canonical_Vocabulary, Canonical_Names and canonical formulas used equal those of `canon(print_local(B))`, whatever Canonical_Names the Worker, Coordinator and Lemma_Pool messages carry.
- **P4.1.13** (Round trip) For any SMT-LIB script value v accepted by the Canonicalizer's script grammar, `parse(print(v)) = v`, and for any accepted text s, `parse(print(parse(s))) = parse(s)`.
- **P4.1.14** (Totality) For any byte string b, the Canonicalizer terminates without panic and returns either a Canonical_Name assignment or a canonicalization failure.
- **P4.1.15** (Symmetry) For any Printed_Goals P and Q, any declared symbol f of P and any declared symbol f′ of Q: f and f′ have identical declarations if and only if f′ and f have identical declarations.
- **P4.1.16** (Refinement) For any input of criterion 2, generated or taken from the CI corpus, the Canonical_Name assignment, canonical formulas and canonicalization failures that dp-canon computes equal those of the compiled Lean reference definition `canon`.
- **P4.1.17** (Rename round trip) For any Printed_Goal P without canonicalization failure: for any term L whose symbols are source names of P's declared symbols and theory symbols of `logic(P)`, `to_source_P(to_canon_P(L)) = L`; and for any term L over `CV(P) ∪ theory(logic(P))`, `to_canon_P(to_source_P(L)) = L`.
- **P4.1.18** (Unmapped symbols) For any Lemma or Instantiation_Hint x of Job j that mentions a symbol that is neither in the name map of j's Printed_Goal nor a theory symbol of its SMT-LIB logic, the Worker sends no message containing x to the Lemma_Pool or to j's solver, and the Worker log lists exactly the unmapped symbols.
- **P4.1.19** (Source names) For every Printed_Goal of the CI corpus, no source name matches a solver-introduced name pattern.
- **P4.1.20** (Label references) For any Printed_Goal P with `:named` labels and references to them, the canonical formula of every Assertion of P contains no label name and equals the canonical formula of the Assertion in which every label reference is replaced by its labelled term.

#### Verification

- A1 — Lean 4 theorems `canon_injective` (P4.1.1) and `canon_equisat` (P4.1.4), with `:named` labels and label references in the model of `canon_equisat` — `proofs/lean/DP/Canon.lean` — `ci/lean`.
- A1 — Creusot contracts on the canonicalization-failure gate of `admit` (P4.1.10) — `crates/dp-core/src/admission.rs` — `ci/creusot`.
- A3 — per-Result re-derivation of the target's Canonical_Names from the locally printed Printed_Goal during the re-admission pass (criterion 14, P4.1.12) — `crates/dp-frontend/src/accept.rs` — `ci/certs`, `ci/canaries`; certificate checking in B's own signature (R5.3) — `crates/dp-cert/src/check.rs` — `ci/certs`.
- A4 — property-based tests with generated declarations, defined symbols with different bodies, datatypes, recursion groups, uninterpreted symbols of equal sort, a sort and a function or constant that share one source name, constants with one source name and different sorts (`c : Int` and `c : Bool`), Hypotheses with patterns, `:qid`, `:weight` and `:skolemid` attributes, `:named` labels with generated references to them, and goal-marker variants (P4.1.1–P4.1.3, P4.1.5, P4.1.7–P4.1.9, P4.1.11, P4.1.15, P4.1.20) — `crates/dp-canon/tests/` — `ci/test`; round trip of the script parser (P4.1.13) and of the name maps (P4.1.17) — `crates/dp-canon/tests/` — `ci/test`; property-based test of the Worker's export and delivery renaming with unmapped symbols (P4.1.18) — `crates/dp-worker/tests/prop_rename.rs` — `ci/test`.
- A4 — differential test that P and its canonical printed form get the same solver answer on the CI corpus (P4.1.4) — `tests/diff/canon_equisat/` — `ci/diff`; differential test of Front_End and Worker canonicalization of the CI corpus (P4.1.6) — `tests/diff/canon_determinism/` — `ci/diff`; differential test of dp-canon against the compiled Lean `canon` on generated goals and the CI corpus (P4.1.16) — `tests/diff/canon_lean_vs_rust/` — `ci/diff`.
- A4 — solver-name scan: run the pinned Z3 and cvc5 on the CI corpus, collect every symbol name in their exported Lemmas, instantiation reports, models and proofs that the input does not declare, and fail if any name matches the Canonical_Name grammar or matches no solver-introduced name pattern (P4.1.7, AS-27), or if any source name that a Driver prints in a Printed_Goal of the CI corpus matches a solver-introduced name pattern (P4.1.19) — `tests/diff/solver_names/` — `ci/diff`.
- A4 — fuzzing of the Canonicalizer (P4.1.14) — `fuzz/canonicalizer/` — `ci/fuzz-smoke`, `sched/fuzz-nightly`; simulation with Worker and Lemma_Pool messages that carry forged Canonical_Names (P4.1.12) — `tests/sim/lying_provenance.rs` — `ci/sim`; Injected_False_Lemmas that reuse a sibling's Canonical_Name for a different declaration — `tests/canaries/` — `ci/canaries`; mutation testing — `tools/mutants/` — `sched/mutants-weekly`.

#### Trust

- TB-1 (collision resistance of the hash function); AS-9 (deterministic printing), AS-24 (goal marker), AS-27 (solver-introduced name patterns). Canonicalization is post-processing of SMT-LIB text (OQ-N5 resolved by design decision D-5), so it needs no Why3 change.
- Soundness does not depend on the axiom sets, which are empty (D-12, so AS-26 is not relied on), or on stable source names (AS-25), which affect only how often names match across goals. Admission soundness (P3.4.1) needs that the Dependency_Set is contained in the target, that every symbol reachable from the Lemma and its Dependency_Set in the reference graph has identical declarations in source and target, and that source and target have the same SMT-LIB logic. R3.4 criterion 1 checks the first by Canonical_Name and kind, criterion 3 turns equal Canonical_Names into identical canonical formulas and identical declarations, which gives the first two (P4.1.1, TB-1), and R3.4 criterion 4 checks the third. Equal sort signatures alone do not suffice, because defined symbols, recursive definitions, sort definitions and datatypes carry meaning beyond their sorts.
- The A3 check that counts for soundness is the certificate check of R5.3, which checks every Lemma_Dependent_Result in B's own signature; the Front_End's local re-derivation of Canonical_Names (criterion 14) is defense in depth.
- Criterion 9 keeps an honest solver's fresh symbols out of every Canonical_Vocabulary, so Import hygiene (R3.3) rejects them. A solver that prints a fresh symbol under an existing Canonical_Name, or under a source name that the export renaming of criterion 17 maps to a Canonical_Name, produces an ordinary, possibly false, Lemma over the Canonical_Vocabulary, which certificate checking catches (R3.3 Trust). The name maps of criteria 17 and 18 affect usefulness only.

### Requirement 18: R4.2 Cross-goal sharing among Sibling_Goals

**Priority:** Must

**Class:** Performance

**User Story:** As a proof engineer, I want Lemmas learned on one goal of a Rust function to help sibling goals from the same function or module.

In this requirement, a *cross-goal Lemma* is a Lemma whose source Goal_Version differs from the Goal_Version of the receiving Job, and the *sibling group* of a Goal_Version is the Rust function or Rust module, at the configured scope, whose verification conditions produced it. The module scope means the Rust module, not the Coma module; the Front_End derives the sibling group from the Goal's why3find goal path by the *sibling-group rule* recorded in `docs/formats/sibling-group.abnf` (AS-29), with the fallback of criterion 9 (OQ-N13 resolved by design decision D-10).

#### Acceptance Criteria

1. WHERE cross-goal sharing is enabled, THE Front_End SHALL include in each Job submission an identifier of the sibling group of the Job's Goal_Version, derived from the Goal's why3find goal path by the sibling-group rule (AS-29).
2. WHERE cross-goal sharing is enabled, THE Lemma_Pool SHALL forward a Lemma from Job a of Goal_Version A to Job b of a Goal_Version B ≠ A only if a and b belong to the same Session, b carries the import mark (R3.1 criterion 2), the submissions of a and b carry equal sibling group identifiers, and a and b have equal Driver hash and Transformation_Pipeline hash (R3.1 criterion 3 governs Jobs of the same Goal_Version).
3. WHERE cross-goal sharing is enabled, THE Lemma_Pool SHALL forward a cross-goal Lemma only if the Lemma's Provenance_Record flag states that the Dependency_Set does not contain the source's Negated_Goal_Assertion.
4. WHERE cross-goal sharing is disabled, THE Lemma_Pool SHALL forward a Lemma only between two Jobs of the same Goal_Version, under the conditions of R3.1 criterion 3.
5. WHEN the Admission_Checker evaluates a cross-goal Lemma for a Job b, THE Admission_Checker SHALL decide with the checks that R3.4 criterion 8 lists, against b's target problem, and SHALL log a rejection with the first failing reason category in the order of R3.4 criterion 6.
6. IF the Admission_Checker evaluates a cross-goal Lemma from source Goal_Version A whose Provenance_Record flag states that the Dependency_Set contains A's Negated_Goal_Assertion, THEN THE Admission_Checker SHALL reject the Lemma with the reason category missing dependency under the explicit check of R3.4 criterion 11 when no earlier check of R3.4 criterion 6 fails, also when the two Goal_Versions share goal path and base Task_Hash, so that the Canonical_Name separation of R4.1 criterion 7 (P4.1.9) is not the only safeguard.
7. WHEN the Admission_Checker admits or rejects a cross-goal Lemma, THE Worker SHALL log the decision with the source and target Goal_Versions and, for a rejection, the reason category.
8. IF cross-goal sharing is enabled while within-goal sharing is disabled, THEN THE Front_End SHALL report a configuration error that names both options, SHALL stop before starting any solver process, SHALL return a non-zero exit status to the Creusot_CLI, and SHALL leave `proof.json` and the `.why3find` cache unchanged (OQ-N46).
9. WHERE cross-goal sharing is enabled, IF the why3find goal path of a Goal does not match the sibling-group rule, THEN THE Front_End SHALL use the why3find file of that goal path as the sibling group of the Goal_Version and SHALL log the fallback with the goal path (OQ-N13 resolved by design decision D-10).

#### Properties

- **P4.2.1** (Scope) For any cross-goal Lemma L that the Lemma_Pool forwards from Job a of A to Job b of B: `session(a) = session(b) ∧ import_mark(b) ∧ group(a) = group(b) ∧ driver(a) = driver(b) ∧ pipeline(a) = pipeline(b) ∧ ¬prov(L).negated_goal`.
- **P4.2.2** With cross-goal sharing disabled, every Lemma that the Lemma_Pool forwards has source Goal_Version equal to the Goal_Version of the receiving Job.
- **P4.2.3** (Differential) For any Goal_Version G in the CI corpus, run once with cross-goal sharing enabled and once disabled, with all other configuration equal: G does not get a proof-labelled Verdict in one run while a Result with prover answer Invalid for G exists in the other run.
- **P4.2.4** (No negated-goal crossing) For any Lemma L with `prov(L).negated_goal = true` and source Goal_Version A, and any Job t of a Goal_Version B ≠ A, including a B with the same goal path and base Task_Hash as A: `admit(L, t) = false`. This follows from the explicit check of R3.4 criterion 11 (P3.4.13); P4.1.9 alone does not give it, because P4.1.9 separates only Printed_Goals with different goal paths or base Task_Hashes.
- **P4.2.5** (Cross-goal admission soundness, metatheory) In the Lean model of AS-12, for any Printed_Goals A and B with equal Driver and Transformation_Pipeline hashes and equal SMT-LIB logics whose Canonical_Names satisfy P4.1.1, any Lemma L with Dependency_Set Δ, `Δ ⊨ L` in Σ_A and `admit(L, b)` for a Job b of B: every symbol reachable from L ∪ Δ in the reference graph has identical declarations in A and B, and `Γ_b ⊨ L` for `Γ_b = Γ_B ∪ PA(b)`. Lean theorem `cross_goal_sound`, from `admission_sound` and `canon_injective`.
- **P4.2.6** (Configuration error) For any configuration with cross-goal sharing enabled and within-goal sharing disabled, the prove command starts no solver process, ends with a non-zero exit status, and leaves `proof.json` and the `.why3find` cache byte-identical.
- **P4.2.7** (Decision log) For any admission decision on a cross-goal Lemma, the Worker log contains one entry with the source and target Goal_Versions, the decision and, for a rejection, the reason category of P3.4.9.
- **P4.2.8** (Sibling groups) For any why3find goal path p and sharing scope: if p matches the sibling-group rule, the sibling group identifier of p is the one that the rule derives from p at that scope, and the log has no fallback entry for p; otherwise the sibling group identifier of p is that of p's why3find file, and the log has exactly one fallback entry naming p.

#### Verification

- A2 — forwarding invariants P4.2.1 and P4.2.2 — `verification/tla/Sharing.tla` — `ci/tlc`.
- A1 — Lean 4 theorem `cross_goal_sound` (P4.2.5) — `proofs/lean/DP/Admission.lean` — `ci/lean`; Creusot contracts on `admit` for P4.2.4 — `crates/dp-core/src/admission.rs` — `ci/creusot`.
- A4 — property-based tests of the cross-goal forwarding filter (P4.2.1, P4.2.2), negated-goal rejection, including generated pairs of Goal_Versions that share goal path and base Task_Hash (P4.2.4), the decision log (P4.2.7), and sibling-group derivation over generated goal paths that match and do not match the sibling-group rule (P4.2.8) — `crates/dp-core/tests/prop_scope.rs` — `ci/test`; differential test, cross-goal sharing on vs. off (P4.2.3) — `tests/diff/cross_goal_on_off/` — `ci/diff`; configuration error (P4.2.6) — `tests/integration/config/` — `ci/integration-cluster`; cross-goal Injected_False_Lemmas, including Lemmas that depend on the source's Negated_Goal_Assertion with the flag cleared — `tests/canaries/` — `ci/canaries`.
- A5 — cross-goal sharing experiment (RQ4) — `eval/scripts/rq4_cross_goal/` — `sched/eval`.

#### Trust

- The sibling group identifier, the Lemma_Pool and the Coordinator affect usefulness only: admission (criterion 5) and certificate checking (R5.3) do not use the sibling relation. AS-29 (goal paths identify Rust functions and modules; where a goal path does not match the sibling-group rule, criterion 9 falls back to the why3find file); the usefulness of cross-goal Lemmas depends on AS-11.
- A Lemma that depends on the source's Negated_Goal_Assertion while its flag says otherwise is rejected as malformed when the flag disagrees with the Dependency_Set (R3.2 criterion 6), and otherwise fails certificate checking because its sub-proof has an open assumption outside B's Assertions (R3.2 criterion 9, R5.2 criterion 5).

### Requirement 19: R4.3 Ground lemmas for quantified goals

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As a proof engineer, I want quantifier-free Lemmas over the shared vocabulary to be shareable between goals that contain quantified Hypotheses, so that sharing works on Creusot goals, which almost always contain quantifiers.

In this requirement, a *solver-introduced symbol* is a symbol that a solver creates during a Job and that the input Printed_Goal does not declare: a Skolem constant or function, a fresh constant, or a purification or other internal symbol.

#### Acceptance Criteria

1. WHEN the Admission_Checker evaluates a Lemma for a Job whose target problem contains quantified Assertions, THE Admission_Checker SHALL decide with exactly the checks that R3.4 criterion 8 lists (R3.2 criteria 6 and 7, the target canonicalization-failure check of R4.1 criterion 12, R3.3, R3.4 criteria 1 to 4, 11 and 7), in the order of R3.4 criterion 6.
2. IF a Lemma contains a quantifier or another binder, an attribute annotation, or a free variable, THEN THE Admission_Checker SHALL reject the Lemma with the reason category hygiene (R3.3 criteria 4 and 5), whether or not the target problem contains quantified Assertions.
3. IF a Lemma mentions a solver-introduced symbol, THEN THE Admission_Checker SHALL reject the Lemma with the reason category hygiene (R3.3 criterion 2); R4.1 criterion 9 keeps such a symbol outside every Canonical_Vocabulary when the solver prints the symbol under a solver-introduced name pattern.
4. WHEN a Lemma passes every check of R3.4 criterion 8 for a Job, THE Admission_Checker SHALL admit the Lemma even if the Lemma contains ground terms, such as terms created by quantifier instantiation in the source Job, that occur in no Assertion of the target problem.
5. WHEN a Sharing_Solver exports a Lemma that mentions a symbol outside the source Printed_Goal's Canonical_Vocabulary and the theory symbols of its SMT-LIB logic, or that contains a binder, an attribute annotation or a free variable, THE Worker SHALL omit the Lemma from the Lemma_Pool and SHALL log the drop with every offending symbol or construct.

#### Properties

- **P4.3.1** For any Lemma L admitted into a Job t of Goal_Version T, whether or not T contains quantified Assertions: L is closed, binder-free and annotation-free, and `symbols(L) ⊆ CV(T) ∪ theory(logic(T))`.
- **P4.3.2** (Metatheory) In the Lean model of AS-12, for any target Job b of B with target problem `Γ_b = Γ_B ∪ PA(b)` over Σ_B that may contain quantified formulas, any source Printed_Goal A with `logic(A) = logic(B)`, any Δ ⊆ Γ_b, which may itself contain quantified formulas, and any L closed, binder-free and well-sorted over Σ_B with `Δ ⊨ L` in Σ_A, where every symbol reachable from L ∪ Δ in the reference graph has identical declarations in Σ_A and Σ_B: `Γ_b ⊨ L`, so `Γ_b ∪ {L}` and `Γ_b` are equisatisfiable. Instance of `admission_sound`.
- **P4.3.3** (Quantifier independence) For any Lemma L and Jobs t and t′ with equal Driver and Transformation_Pipeline hashes, equal SMT-LIB logics, no canonicalization failure for the Printed_Goal of either, equal registered and revoked credentials, equal Partition_Assumptions, equal admitted-Lemma sequences, equal sets of exported Lemmas and Instantiation_Hints, Goal_Versions that are both equal to or both different from L's source Goal_Version, and equal Canonical_Vocabularies with equal sort signatures, whose target problems both contain every element of L's Dependency_Set: `admit(L, t) = admit(L, t′)`, whether or not t or t′ contains quantified Assertions.
- **P4.3.4** (Solver-introduced symbols) For any Lemma L that mentions a string matching a solver-introduced name pattern of the pinned Z3 or cvc5, and any Job t: `admit(L, t) = false`, with reason category hygiene unless an earlier check of R3.4 criterion 6 fails.
- **P4.3.5** (New terms) For any Lemma L and Job t such that L passes every check of R3.4 criterion 8, including the target canonicalization-failure check (R4.1 criterion 12) and the cross-goal negated-goal check (R3.4 criterion 11): `admit(L, t) = true`, including when L has ground subterms that occur in no Assertion of t's target problem.
- **P4.3.6** (Export filter) For any Lemma exported by the Sharing_Solver of Job j that mentions a symbol outside `CV(j) ∪ theory(logic(j))` or contains a binder, an annotation or a free variable, the Worker sends no message containing that Lemma to the Lemma_Pool, and the Worker log lists exactly the offending symbols and constructs.

#### Verification

- A1 — `admission_sound` covers quantified Γ_B and Δ (P4.3.2) — `proofs/lean/DP/Admission.lean` — `ci/lean`; Creusot contracts on `is_ground_lemma`, `admit` and the export filter (P4.3.1, P4.3.3–P4.3.6) — `crates/dp-core/src/admission.rs` — `ci/creusot`.
- A3 — Front_End re-admission against the locally printed target (R3.4 criterion 9) — `crates/dp-frontend/src/accept.rs` — `ci/certs`, `ci/canaries`; certificate checking in B's own signature (R5.3) — `crates/dp-cert/src/check.rs` — `ci/certs`.
- A4 — property-based tests with generated quantified targets, Skolem and other solver-introduced names in the styles of the pinned Z3 and cvc5, binders, annotations, free variables and new ground terms (P4.3.1, P4.3.3–P4.3.6) — `crates/dp-core/tests/prop_ground.rs` — `ci/test`; differential test, cross-goal sharing on vs. off — `tests/diff/cross_goal_on_off/` — `ci/diff`; false Ground_Lemmas for quantified targets, including Lemmas that hold only for a Skolem symbol renamed to a Canonical_Name — `tests/canaries/` — `ci/canaries`; mutation testing — `tools/mutants/` — `sched/mutants-weekly`.

#### Trust

- AS-12; AS-27 (solver-introduced name patterns). A solver that prints a solver-introduced symbol under a Canonical_Name produces a Lemma that passes hygiene and may be false; certificate checking in B's signature catches it (R5.3), and the Canary_Suite contains such Lemmas.

### Requirement 20: R4.4 Quantifier-instantiation hints

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As a proof engineer, I want Jobs to share which instances of quantified Hypotheses helped, as a Hypothesis name plus ground terms, so that sibling goals reuse instantiations without importing unproved facts.

In this requirement:

- A Hypothesis is *universally quantified* when its canonical formula (R4.1) is a `forall` term. Its *top-level binder* is that outermost `forall`, binding variables x_1 … x_k of sorts s_1 … s_k over a body φ′. The *matrix* φ is φ′ with exactly these annotations removed: the patterns (`:pattern`, `:no-pattern`) of the top-level binder, and every `:named`, `:qid`, `:weight`, `:skolemid` or other attribute that is not a pattern, wherever it occurs in φ′. Quantifiers nested in φ keep their patterns.
- The *instance* of an Instantiation_Hint (n, (t_1, …, t_k)) for a target B is `φ[t_1/x_1, …, t_k/x_k]`, where φ is the matrix of B's Hypothesis with Canonical_Name n.
- Two formulas are *equal as instances* when their canonical formulas (R4.1 normalization: label references replaced, labels and attributes other than patterns removed, Canonical_Names and binder-position names) are identical. Criteria 17 and 18 and P4.4.9 compare instances with this equality.
- `instantiate` is dp-core's instantiation function, which computes the instance by simultaneous substitution.

#### Acceptance Criteria

1. WHERE hint sharing is enabled, WHEN a Sharing_Solver in a Job reports an instantiation of a Hypothesis of the Job's Printed_Goal at a tuple of ground terms (AS-8, AS-28), THE Worker SHALL send to the Lemma_Pool an Instantiation_Hint made of that Hypothesis's Canonical_Name and the tuple, with the hint's identifier, Provenance_Record and authentication tag.
2. WHEN a Worker exports an Instantiation_Hint, THE Worker SHALL attach a Provenance_Record with every field of R3.2 criterion 1, whose Dependency_Set contains only the Canonical_Name of the named Hypothesis and whose used Lemmas follow R3.2 criteria 1 and 2.
3. THE Worker SHALL set each Instantiation_Hint's identifier to the hash of the printed bytes of the hint and of its Provenance_Record, each in the canonical printed form (R7.10) in which the Worker sends them, and SHALL authenticate the hint with the Worker's credential using a tag computed over that printed form, the identifier and the Provenance_Record (OQ-N11).
4. IF a term of an Instantiation_Hint mentions a symbol outside the source Printed_Goal's Canonical_Vocabulary and the theory symbols of its SMT-LIB logic, or contains a binder, an attribute annotation or a free variable, THEN THE Worker SHALL omit the hint from the Lemma_Pool and SHALL log the drop with every offending symbol or construct.
5. THE Lemma_Pool SHALL keep each Instantiation_Hint that the Lemma_Pool stores (R3.6) together with the hint's identifier, Provenance_Record and authentication tag, and SHALL return all four, byte for byte as received, on every read and every forward (R3.1 criterion 11).
6. WHERE hint sharing is enabled, THE Lemma_Pool SHALL forward an Instantiation_Hint from Job a to Job b only if R3.1 criterion 3 (a and b of the same Goal_Version) or, where cross-goal sharing is enabled, R4.2 criteria 2 and 3 (a and b of different Goal_Versions) allow forwarding a Lemma with the same Provenance_Record from a to b.
7. THE Worker SHALL treat every Instantiation_Hint delivered to a Job as a delivered Lemma for the import list (R3.1 criteria 4 and 8) and for the used Lemmas of every later export from that Job (R3.2 criteria 1 and 2).
8. THE Admission_Checker SHALL treat every Instantiation_Hint admitted into a Job as an admitted Lemma, and every Instantiation_Hint exported by a Job as a Lemma exported by that Job, when applying R3.4 criterion 7 to a later Lemma.
9. WHEN the Admission_Checker rejects an Instantiation_Hint (n, t̄) for a Job b of target B, THE Admission_Checker SHALL log exactly one reason category, the first failing check in the order: malformed (R3.2 criterion 6 applied to the hint, with the identifier hash computed over the received bytes of the hint and its Provenance_Record and with the terms of t̄ parsed under the vocabulary-independent Lemma grammar of R3.3; a Dependency_Set other than {n}; or a printed form of more than `<TBD-35>` bytes, checked before parsing), authentication (R3.2 criterion 7), target canonicalization failure, with the reason category hygiene (R4.1 criterion 12), pipeline mismatch (R3.4 criterion 4), hygiene (criterion 10), missing dependency (criterion 11), hint shape (criterion 12).
10. IF a term of t̄ that parses under the vocabulary-independent Lemma grammar (R3.3) fails to parse against B's Canonical_Vocabulary and the theory symbols of B's SMT-LIB logic, mentions a symbol outside them (including a solver-introduced symbol, R4.3), fails sort checking, or contains a binder, an attribute annotation or a free variable, THEN THE Admission_Checker SHALL reject the hint with the reason category hygiene.
11. IF b's target problem contains no Hypothesis with Canonical_Name n, THEN THE Admission_Checker SHALL reject the hint with the reason category missing dependency.
12. IF the Assertion of B named n is not a universally quantified Hypothesis (including when n names a Negated_Goal_Assertion or a Partition_Assumption, OQ-N47), or t̄ has a length other than k, or some term t_i has a sort other than s_i, THEN THE Admission_Checker SHALL reject the hint with the reason category hint shape.
13. WHEN an Instantiation_Hint passes every check of criterion 9, THE Admission_Checker SHALL admit the hint, whatever the hint's used Lemmas, the pattern annotations of the named Hypothesis, and whether the terms of t̄ occur in B's Assertions.
14. WHEN the Admission_Checker admits an Instantiation_Hint for a Job, THE Worker SHALL deliver to the Job's solver the instance computed by `instantiate` from the Job's own Printed_Goal.
15. WHEN the re-admission pass of R3.4 criterion 9 reaches an imported Instantiation_Hint of a Lemma_Dependent_Result, THE Verdict_Recorder SHALL decide the hint with `admit_hint` within that single pass, against the pass's admitted sequence, and SHALL recompute the hint's instance with `instantiate` from the locally printed Printed_Goal.
16. IF the re-admission pass of criterion 15 rejects an imported Instantiation_Hint, or the Verdict_Recorder cannot obtain for an imported identifier a hint and Provenance_Record whose hash equals that identifier, THEN THE Verdict_Recorder SHALL label the Result unchecked and SHALL log the reason category.
17. WHEN the Certificate_Composer composes a Composed_Certificate for a Result that imported an Instantiation_Hint, THE Certificate_Composer SHALL justify the instance by a universal-instantiation step from B's Hypothesis named n at the terms t̄ (R5.2 criterion 2), whose conclusion is equal as an instance to the instance recomputed under criterion 15.
18. IF the target proof of a Result uses, for an imported Instantiation_Hint, a formula that is not equal as an instance to the instance recomputed under criterion 15, THEN THE Certificate_Composer SHALL abort composition and THE Verdict_Recorder SHALL label the Result unchecked and log the reason instance mismatch.
19. WHEN the Verdict_Recorder marks a Lemma_Dependent_Result checker-accepted (R5.3 criterion 4) or labels it checked-incomplete (R5.3 criterion 5), THE Verdict_Recorder SHALL store in the Certificate_Store entry of the Result's Composed_Certificate the Provenance_Record of every Instantiation_Hint in the Result's import closure (R3.2 criterion 10) and, for audit only, the Provenance_Record of every used Lemma that such a hint's Provenance_Record names and that the Front_End resolves by identifier (R3.2 criterion 3), with the identifier of each such used Lemma that it does not resolve; no Lemma proof is required for those used Lemmas (R5.1).
20. THE Certificate_Composer SHALL print the conclusion of the universal-instantiation step of every imported Instantiation_Hint (n, t̄) in the form that the Certificate_Checker of the proof's Checkable_Proof_Format derives from the Assertion named n, as B's locally printed Printed_Goal prints that Assertion, at the terms t̄.

#### Properties

- **P4.4.1** (Hint soundness, metatheory) For any Γ_B over Σ_B containing a universally quantified Hypothesis h with top-level binder x̄:s̄ and matrix φ, and any tuple t̄ of closed, well-sorted terms over Σ_B with `sort(t_i) = s_i`, including terms that occur in no element of Γ_B: `Γ_B ⊨ φ[t̄/x̄]`, so `Γ_B ∪ {φ[t̄/x̄]}` unsatisfiable implies `Γ_B` unsatisfiable. Lean theorem `hint_sound`.
- **P4.4.2** (Substitution) For any universally quantified Hypothesis h and closed t̄ of matching length and sorts, `instantiate(h, t̄)` equals the simultaneous substitution of t̄ for x̄ in h's matrix, is a well-sorted term of sort Bool, has no free variables, carries no pattern of h's top-level binder and no `:named`, `:qid`, `:weight`, `:skolemid` or other attribute that is not a pattern, and keeps every pattern of nested quantifiers.
- **P4.4.3** (Rejections) For any Instantiation_Hint (n, t̄) and Job t of B: if n names no Hypothesis of t's target problem, `admit_hint` rejects with missing dependency; if n names an Assertion that is not a universally quantified Hypothesis, or the length or a sort of t̄ mismatches, with hint shape; if a term mentions a symbol outside `CV(B) ∪ theory(logic(B))`, including any match of a solver-introduced name pattern, or contains a binder, annotation or free variable, with hygiene; in each case unless an earlier check of criterion 9 fails.
- **P4.4.4** (Round trip) For any Instantiation_Hint h, `parse_hint(print_hint(h)) = h`, and for any Provenance_Record p of a hint, `parse_prov(print_prov(p)) = p`.
- **P4.4.5** (Provenance and identity) For any Instantiation_Hint h that the Admission_Checker admits: `prov(h)` parses and has every field of R3.2 criterion 1, `prov(h).deps = {h.name}`, `prov(h).negated_goal = false`, `id(h) = H(bytes(h), bytes(prov(h)))` over the received bytes, and h's tag verifies under the registered, unrevoked credential of `prov(h).worker`.
- **P4.4.6** (Tamper) For any authentic hint message m and any message m′ that keeps m's tag and differs from m in the Hypothesis name, a term, the identifier or a Provenance_Record field, the Admission_Checker rejects m′.
- **P4.4.7** (Reason determinism) For any rejected pair (h, t), the logged reason category is the first failing check in the order of criterion 9.
- **P4.4.8** (Completeness) For any hint h and Job t that pass every check of criterion 9, including the target canonicalization-failure check (R4.1 criterion 12), `admit_hint(h, t) = true`, for any used Lemmas of h, any pattern annotations of the named Hypothesis and any terms of h that occur in no Assertion of t.
- **P4.4.9** (Front_End re-admission) For any Result r labelled certified and any Instantiation_Hint h in `r.imports`, the re-admission pass of r (R3.4 criterion 9) admitted h with `admit_hint` against the target problem built from `print_local` and the pass's admitted sequence, and the instance justified in r's Composed_Certificate is equal as an instance to `instantiate` applied to the Hypothesis of `print_local` named `h.name` at `h.terms`.
- **P4.4.10** (Refinement) For any input x, the Rust `admit_hint(x)` and `instantiate(x)` equal the Lean reference definitions, including the logged reason category.
- **P4.4.11** (Export filter) For any hint reported by the Sharing_Solver of Job j whose terms mention a symbol outside `CV(j) ∪ theory(logic(j))` or contain a binder, annotation or free variable, the Worker sends no message containing that hint to the Lemma_Pool, and the Worker log lists exactly the offending symbols and constructs.
- **P4.4.12** (Hints as used Lemmas) For any Lemma L exported by a Job j without solver-reported dependencies, `prov(L).used` contains every Instantiation_Hint delivered to j before the export; and for any Lemma L admitted into a Job t, every hint in `prov(L).used` was admitted into t or exported by t before L.
- **P4.4.13** (Checker-form instances) For any hint-using Result in the integration corpus of `tests/integration/certs/` whose target proof is a cvc5 CPC or Alethe proof, the Composed_Certificate prints the conclusion of every universal-instantiation step for an imported Instantiation_Hint in the form that Ethos or Carcara derives from the original Assertion, and the pinned Certificate_Checker's outcome on the certificate is accept.

#### Verification

- A1 — Lean 4 theorem `hint_sound` (P4.4.1), a premise of the certifiability theorem `certifiable` (R7.1, P7.1.8) and not of `verdict_sound` — `proofs/lean/DP/Hints.lean` — `ci/lean`; Creusot contracts on `instantiate` and `admit_hint` (P4.4.2, P4.4.3, P4.4.5, P4.4.7, P4.4.8) — `crates/dp-core/src/hints.rs` — `ci/creusot`.
- A3 — per-Result re-admission and instance recomputation in the Front_End (criteria 15, 16 and 18, P4.4.9) — `crates/dp-frontend/src/accept.rs` — `ci/certs`, `ci/canaries`; the Certificate_Checker checks every universal-instantiation step in B's own signature (R5.3) — `crates/dp-cert/src/check.rs` — `ci/certs`.
- A2 — Kani bounded no-panic check of the hint parser — `verification/kani/parsers.rs` — `ci/kani`.
- A4 — property-based tests of P4.4.2–P4.4.8, P4.4.11 and P4.4.12 with generated Hypotheses (nested quantifiers, patterns, several binders), arity and sort mismatches, Skolem and other solver-introduced names, binders, annotations and terms absent from the target — `crates/dp-core/tests/prop_hints.rs` — `ci/test`; differential test of the Rust `admit_hint` and `instantiate` against the compiled Lean reference (P4.4.10) — `tests/diff/hints_lean_vs_rust/` — `ci/diff`; differential test, hint sharing on vs. off, with the property of P4.2.3 — `tests/diff/hints_on_off/` — `ci/diff`.
- A4 — integration test with real cvc5 CPC and Alethe proofs of hint-using Results, whose Hypotheses carry top-level and nested patterns, `:named`, `:qid`, `:weight` and `:skolemid` attributes, checked by the pinned Ethos and Carcara (P4.4.13) — `tests/integration/certs/` — `ci/certs`.
- A4 — round trip and fuzzing of the hint parser (P4.4.4) — `crates/dp-formats/tests/`, `fuzz/hint_parser/` — `ci/test`, `ci/fuzz-smoke`, `sched/fuzz-nightly`; malformed hints in the Canary_Suite (unknown Hypothesis, a Negated_Goal_Assertion or Partition_Assumption name, a Hypothesis that is not universally quantified, wrong arity, wrong sort, foreign and Skolem symbols, oversized, tampered) — `tests/canaries/` — `ci/canaries`; simulation with Workers that deliver instances other than `instantiate` computes — `tests/sim/lying_provenance.rs` — `ci/sim`; mutation testing — `tools/mutants/` — `sched/mutants-weekly`.
- A5 — benefit of hints, reported separately in RQ4 — `eval/scripts/rq4_cross_goal/` — `sched/eval`.

#### Trust

- AS-8 and AS-28 (solvers report instantiations of input Hypotheses); AS-6 (checkers accept instantiation steps); AS-12.
- Admission of a hint uses neither the source's Dependency_Set beyond the named Hypothesis nor the hint's used Lemmas, because the instance follows from B's own Hypothesis alone (P4.4.1); the used Lemmas of a hint are kept for the audit trail only (criterion 19): they add no edges to the import closure and need no Lemma proof (R5.1). Pattern annotations are solver heuristics and do not affect soundness. A Result that imported only hints still needs a certificate (OQ-N7).
- AS-40 (Sharing_Solver patches print hint terms without `let`, which criteria 4 and 10 reject as a binder; design decision D-14) decides how many hints survive the export filter, which affects usefulness only.

---

**Group R5: Certificates**

### Requirement 21: R5.1 Proofs for lemma-dependent results

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As an auditor, I want every Lemma_Dependent_Result backed by machine-checkable proofs, so that correctness does not depend on solver patches or the Lemma_Pool.

In this requirement and in R5.2 to R5.5:

- A Result *needs certification* when it reports a proof and is a Lemma_Dependent_Result, or, where certify-all mode is enabled, when it reports a proof at all (R5.5). Where partitioning is enabled, for a partitioned Goal_Version (R3.5), the leaf-Result set of criterion 10 needs certification as one unit, and R5.1 to R5.4 apply to that set as to one Result.
- The *import closure* `imports*(r)` of a Result r is the set of Lemmas and Instantiation_Hints in r's import list together with every Lemma and Instantiation_Hint reachable from them through used-Lemma edges (R3.2 criterion 1, R4.4 criterion 7). Only a Lemma has used-Lemma edges: an Instantiation_Hint contributes its instance and no edges, and the used Lemmas that a hint's Provenance_Record names are stored for audit only (R4.4 criterion 19, R5.4 criterion 1). Lemmas that r's own Job exported and that are reached through used-Lemma edges are elements of `imports*(r)`; the Front_End resolves them by identifier (R3.2 criterion 3) and certifies them like every other element.
- The *extended problem* of a Result r of Goal_Version B is the set of Assertions of B's locally printed Printed_Goal, plus the Partition_Assumptions that the recorded Partition tree gives r's Partition (none for a whole-Goal_Version Result), plus every Lemma in r's import list, plus, for every Instantiation_Hint in r's import list, the instance that `instantiate` recomputes from B's locally printed Printed_Goal (R4.4 criteria 15 and 18).
- The *target proof* of r is a proof in a Checkable_Proof_Format that r's extended problem is unsatisfiable.
- The *proof-producer list* is the list of proof-producing cvc5 Solver_Configurations that the project configuration names for target proofs (the `proof_producers` option of `dp.toml`); it may be empty.
- The *certification obligation* `oblig(r)` of r is the set of elements of r's import list whose Lemma text or recomputed instance matches (R5.2) an open assumption of r's target proof, together with every element of `imports*(r)` reachable from them through used-Lemma edges. The full import list, not `oblig(r)`, decides whether r is a Lemma_Dependent_Result and gives r's import-set hash (R1.3 criteria 1, 5 and 7).
- The *Lemma problem* of a Lemma L in `oblig(r)` is the SMT-LIB problem that the Front_End prints from B's locally printed Printed_Goal, made of: the declarations of `print_local(B)`; the Assertions of `print_local(B)` whose Canonical_Names are in L's Dependency_Set and, for a leaf Result, the Partition_Assumptions of that leaf named there; the used Lemmas of L that are Lemmas; the instances that `instantiate` recomputes from `print_local(B)` for the Instantiation_Hints among L's used Lemmas; and the negation ¬L. Every Canonical_Name is written as the symbol that `print_local(B)` declares under it (R5.2 criterion 8), so no renaming between the source and the target Goal_Version is needed.
- The *Lemma proof* of a Lemma L in `oblig(r)` is a refutation of L's Lemma problem in the Checkable_Proof_Format of r's target proof, produced by a proof-producing run that the Front_End requests on that Lemma problem (R3.2 criterion 8; AS-34).
- The *certification start* of r is the step at which the Verdict_Recorder accepts r under R1.3; for a Result of a Race accepted after that Race's deadline (R2.2 criterion 12), it is the Race deadline; where partitioning is enabled, for a partitioned Goal_Version, it is the step at which the Verdict_Recorder has accepted a proof-reporting Result for every leaf Partition, or the Race deadline if that is earlier; for a Result_Cache entry that the Verdict_Recorder labels under R7.1 criterion 6, it is the step at which the Verdict_Recorder reads the entry. The *proof deadline* of r is `<TBD-14>` seconds after its certification start. The *certification deadline* of r is `<TBD-15>` seconds after its proof deadline. For a Result_Cache entry no proof is produced: the certification deadline bounds the Certificate_Checker run of R7.1 criterion 6 and the storage of R5.4.

#### Acceptance Criteria

1. WHILE the Race of a Result r, if r is a Result of a Race, has not ended and r's certification deadline has not passed, WHEN the Verdict_Recorder accepts r and r needs certification, THE Front_End SHALL obtain a target proof of r, that is, a proof in a Checkable_Proof_Format that r's extended problem is unsatisfiable.
2. WHEN the Front_End has obtained the target proof of a Result r that needs certification, THE Front_End SHALL obtain a Lemma proof of every Lemma in `oblig(r)` by requesting a proof-producing run on that Lemma's Lemma problem in the Checkable_Proof_Format of r's target proof (OQ-N6).
3. THE Front_End SHALL build the extended problem and every Lemma problem of every Result that needs certification from B's locally printed Printed_Goal, the recorded Partition tree, the imported Lemma texts and the recomputed instances only, and from no instance, Canonical_Name or formula that a Worker, the Coordinator or the Lemma_Pool supplies.
4. IF the Front_End has not obtained the target proof of a Result r and the Lemma proof of every Lemma in `oblig(r)` at r's proof deadline, for any reason (no Checkable_Proof_Format for the prover, NG-5; a proof-producing run that failed, timed out or answered other than unsatisfiable; a missing proof message), THEN THE Verdict_Recorder SHALL label r unchecked and SHALL log the missing proofs and the reason.
5. IF the Front_End cannot obtain, for an identifier in `imports*(r)`, a Lemma or Instantiation_Hint and a Provenance_Record whose hash equals that identifier (R3.2 criterion 3), THEN THE Verdict_Recorder SHALL label r unchecked and SHALL log the reason missing import.
6. WHEN the proof deadline of a Result r passes, THE Front_End SHALL stop every proof-producing run and every composition that the Front_End started for r, SHALL send the Coordinator a cancellation (R1.5) for every proof-producing Job that the Front_End submitted for r, and SHALL discard every proof for r that arrives after r's proof deadline (OQ-N6).
7. THE Verdict_Recorder SHALL assign every Result that needs certification its Assurance_Label no later than that Result's certification deadline.
8. THE Verdict_Recorder SHALL assign a Result that needs certification no proof label other than certified, and SHALL assign certified only under R5.3 criterion 17.
9. IF a Result r that needs certification is accepted after its Race ended or after its certification deadline, THEN THE Verdict_Recorder SHALL label r unchecked at acceptance, and THE Front_End SHALL start no proof-producing run and no Certificate_Checker run for r (R2.2 criterion 26; OQ-N48).
10. WHERE partitioning is enabled, WHEN the Verdict_Recorder accepts a proof-reporting Result for a leaf Partition of a partitioned Goal_Version and the Goal_Version's leaf-Result set has no element for that leaf, THE Verdict_Recorder SHALL add that Result to the leaf-Result set, so that the set holds, for each leaf, the first proof-reporting Result accepted for that leaf, whose Driver hash and Transformation_Pipeline hash are those of the recorded Partition tree (R3.5 criterion 5).
11. WHERE the proof-producer list of the project configuration is not empty, WHEN the Front_End obtains the target proof of a Result r under criterion 1, THE Front_End SHALL request one proof-producing run of r's extended problem, with every imported Lemma and recomputed instance as a plain assertion, by a proof-producing cvc5 Solver_Configuration of that list (CPC preferred, Alethe second), submitted as a Job to the Cluster while the Front_End is connected to the Proof_Server and has not switched to the fallback, and started on the local machine otherwise and for the local certification of R1.2 criterion 8, and SHALL request the Lemma proofs of criterion 2 in parallel once `oblig(r)` is known (OQ-N6 resolved by design decision D-6; AS-36).

#### Properties

- **P5.1.1** For any Result r labelled certified, the Certificate_Store entry of r's Composed_Certificate contains a target proof of r's extended problem and a Lemma proof of every Lemma in `oblig(r)`.
- **P5.1.2** (No proof, no proof label) For any Result r that needs certification and for which the target proof or the Lemma proof of some Lemma in `oblig(r)` is not obtained by r's proof deadline, or for which some identifier in `imports*(r)` cannot be resolved, `label(r) = unchecked`.
- **P5.1.3** (Certification deadline) For any trace τ and any Result r that needs certification, with certification start s, the Verdict_Recorder assigns r its label at a step with time at most `s + <TBD-14> + <TBD-15>`; and for any Result r of Race ρ, `s ≤ deadline(ρ)`. Together these give the premise of P2.2.6.
- **P5.1.4** (Local extended problem) For any Result r of B: `extended(r) = assertions(print_local(B)) ∪ PA(r) ∪ lemmas(r.imports) ∪ {instantiate(print_local(B), h) | h ∈ hints(r.imports)}`, where `PA(r)` comes from the recorded Partition tree; and for any two message histories that agree on the Lemma texts and hint terms of r's imports, `extended(r)` is the same.
- **P5.1.5** (Certification start) For any Result r: `s(r) = accept(r)` if r is not a Result of a Race; `s(r) = min(accept(r), deadline(ρ))` if r is a Result of Race ρ; where partitioning is enabled, for a partitioned Goal_Version, s is the minimum of the last leaf acceptance and the Race deadline; and for a Result_Cache entry e labelled under R7.1 criterion 6, `s(e) = read(e)`, so that `cert_deadline(e) = read(e) + <TBD-14> + <TBD-15>` is defined for P7.1.1(b).
- **P5.1.6** (Only certified) For any Result r that needs certification, `label(r) ∈ {certified, checked-incomplete, unchecked}`.
- **P5.1.7** (Obligation) For any Result r with target proof π: `oblig(r) = closure_U({x ∈ r.imports | text(x) matches some a ∈ open(π)})`, where `closure_U` follows used-Lemma edges from Lemmas only and `text(x)` is the Lemma text or the recomputed instance; `oblig(r) ⊆ imports*(r)`; no Instantiation_Hint is the source of an edge of `imports*(r)`; and whether r is a Lemma_Dependent_Result, and `r.imports_hash`, depend on `r.imports` and not on `oblig(r)`.
- **P5.1.8** (Late or expired acceptance) For any Result r that needs certification and is accepted after its Race ended or after `cert_deadline(r)`: `label(r) = unchecked` is assigned at acceptance, and the trace contains no proof-producing run and no Certificate_Checker run for r.
- **P5.1.9** (Lemma problem) For any Result r of B and any L ∈ `oblig(r)`, the proof-producing run for L's Lemma proof receives exactly `decls(print_local(B)) ∪ {a ∈ assertions(print_local(B)) ∪ PA(r) | name(a) ∈ prov(L).deps} ∪ lemmas(prov(L).used) ∪ {instantiate(print_local(B), h) | h ∈ hints(prov(L).used)} ∪ {¬L}`, printed in the symbols of `print_local(B)`, and requests the Checkable_Proof_Format of r's target proof.
- **P5.1.10** (Proof deadline cleanup) For any trace and any Result r whose proof deadline passes at time t: the trace contains at t the stop of every proof-producing run and composition for r and a cancellation of every proof-producing Job submitted for r, and no proof that arrives after t is part of r's Composed_Certificate.
- **P5.1.11** (Leaf selection, where partitioning is enabled) For any partitioned Goal_Version G with recorded Partition tree T, the leaf-Result set of G holds, for each leaf p of T that has an accepted proof-reporting Result, exactly the first such Result accepted for p, and every element r satisfies `r.config.driver = T.driver ∧ r.config.pipeline = T.pipeline`.
- **P5.1.12** (Target-proof request) For any trace with a non-empty proof-producer list and any Result r for which criterion 1 applies: the trace contains exactly one request of a proof-producing run of `extended(r)` by a Solver_Configuration of the proof-producer list; that request is a Cluster submission if, at the step of the request, the Front_End is connected to the Proof_Server, has not switched to the fallback and r is not certified under R1.2 criterion 8, and a local start otherwise; and every Lemma proof request for r comes after the step at which the target proof of r is obtained.

#### Verification

- A1 — Creusot contracts on the certification decision functions (needs-certification predicate, import closure and certification obligation, extended-problem and Lemma-problem construction, leaf selection, certification start and deadlines including Result_Cache entries, the unchecked label on a missed deadline and on late or expired acceptance) for P5.1.2, P5.1.4–P5.1.9 and P5.1.11 — `crates/dp-core/src/certification.rs` — `ci/creusot`.
- A3 — per-Result certificate checking against the locally printed Printed_Goal (R5.3) — `crates/dp-cert/src/check.rs` — `ci/certs`.
- A2 — TLA+ certification start, proof deadline and certification deadline actions with the invariant that no Result awaits certification after its certification deadline (P5.1.3) and the proof-deadline cleanup (P5.1.10), checked with TLC — `verification/tla/Coordinator.tla` — `ci/tlc`.
- A4 — property-based test of the certification decision functions over generated Results, import closures with Lemma and hint used-Lemma edges, target proofs that use a subset of the imports, Partition trees and acceptance times (P5.1.2, P5.1.4–P5.1.9, P5.1.11) — `crates/dp-core/tests/prop_certification.rs` — `ci/test`; P5.1.3, P5.1.10 and P5.1.12 asserted on deterministic simulation traces with simulated proof-production and checking delays, proofs that arrive after the proof deadline, and target-proof requests with and without a Cluster connection — `tests/sim/` — `ci/sim`, `sched/sim-nightly`.
- A4 — integration tests on a CI corpus subset with cvc5 proof production, including a prover without a Checkable_Proof_Format, a proof producer that answers unknown, a proof producer that never answers, a missing transitive import, an import that the target proof does not use, a cross-goal Lemma proved from its Lemma problem in B's symbols, and a Result accepted after its certification deadline (P5.1.1, P5.1.2, P5.1.8, P5.1.9, criterion 6) — `tests/integration/certs/` — `ci/certs`; mutation testing — `tools/mutants/` — `sched/mutants-weekly`.
- A5 — certificate coverage and overhead (RQ5) — `eval/scripts/rq5_certificates/` — `sched/eval`.

#### Trust

- None in the proof-producing solver: every proof is checked (R5.3). AS-5 decides how many Results can reach certified, and AS-34 whether produced proofs show the imports as matching `assume` steps. Each Lemma proof comes from a proof-producing run on the Lemma problem printed from `print_local(B)` (criterion 2), and the target proof from a proof-producing cvc5 re-run of the extended problem, on the Cluster or locally (criterion 11; OQ-N6 resolved by design decision D-6). The proof producer is untrusted. AS-36 decides whether its proofs stay within the supported fragment of R5.2; a proof outside it leaves the Result unchecked. With an empty proof-producer list no target proof is requested, and criterion 4 labels every Result that needs certification unchecked. How a Race Result accepted after the Race deadline is certified is OQ-N48.

### Requirement 22: R5.2 Certificate composition

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As an auditor, I want one artifact that proves the target goal from the target's own Assertions, so that one checker run settles a Lemma_Dependent_Result.

In this requirement, r is a Result of Goal_Version B that needs certification (R5.1), and the terms import closure, extended problem, target proof, Lemma proof and proof deadline are those of R5.1. A *justification* of an element of `imports*(r)` is its scoped sub-proof (for a Lemma) or its universal-instantiation step (for an Instantiation_Hint). The *composition plan* is the order of justifications, the scope structure and the assignment of every open assumption to an Assertion, a justification or a Partition_Assumption; the Certificate_Composer computes the plan before printing the Composed_Certificate. The certification obligation `oblig(r)` and the Lemma problem are those of R5.1.

The *supported fragment* of a Checkable_Proof_Format is an allow-list of proof commands:

- CPC: the commands `assume` and `step` and their scoped forms `assume-push` and `step-pop`, each `step` and `step-pop` naming a rule of the CPC signature that the Lock_File pins.
- Alethe: the commands `assume`, `step` and `anchor`, each `step` naming a rule of the Alethe rule list that the Lock_File pins.

The pinned cvc5 is expected to print proofs inside the supported fragment when it runs with the proof options of the project configuration, which turn global DAG definitions and trust steps off (AS-36).

A *banned command* is any command outside the supported fragment, including every command that changes what the checker accepts: for Ethos (Eunoia), `declare-rule`, `program`, `declare-oracle-fun`, `include` and every other command that extends the signature; for Alethe, a `step` whose rule is outside the pinned Alethe rule list.

A formula f of a received proof *matches* a Lemma text, an instance or a formula of a Lemma problem x when f and x, each parsed by the Format_Codec and with every Canonical_Name replaced by the symbol of criterion 8, print to the same canonical printed form (R7.10 criterion 3).

#### Acceptance Criteria

1. WHEN the Front_End has obtained r's target proof and the Lemma proof of every Lemma in `oblig(r)` (R5.1 criteria 1 and 2) before r's proof deadline, and the re-admission pass of r (R3.4 introduction and criterion 9, R4.4 criterion 15) admitted every import of r, THE Certificate_Composer SHALL compose one Composed_Certificate that derives false from the Assertions of B's locally printed Printed_Goal, made of r's target proof, one scoped sub-proof for every Lemma in `oblig(r)` (R3.2 criteria 8 and 9, AS-23), and one universal-instantiation step for every Instantiation_Hint in `oblig(r)`.
2. THE Certificate_Composer SHALL justify the instance of every Instantiation_Hint (n, t̄) in `oblig(r)`, whether the target proof or a Lemma sub-proof uses the instance, by a universal-instantiation step from the Assertion of B's locally printed Printed_Goal with Canonical_Name n at the terms t̄, whose conclusion is equal as an instance (R4.4) to the instance that `instantiate` recomputes and is printed in the form that the Certificate_Checker derives (R4.4 criteria 15, 17, 18 and 20).
3. THE Certificate_Composer SHALL place the justification of every used Lemma and every used Instantiation_Hint of a Lemma L before the sub-proof of L, including Lemmas and Instantiation_Hints reached only through used-Lemma edges, and inside a leaf scope where criterion 21 requires it.
4. IF the graph with an edge from each Lemma in `oblig(r)` to each of its used Lemmas contains a cycle, THEN THE Certificate_Composer SHALL abort composition and THE Verdict_Recorder SHALL label r unchecked and log the reason dependency cycle.
5. IF an open assumption of the sub-proof of a Lemma L is none of (a) an Assertion of B's locally printed Printed_Goal, or, inside the scope of a leaf Partition, a Partition_Assumption of that leaf, whose Canonical_Name and kind (R4.1 criterion 5) equal those of an element of L's Dependency_Set, (b) a used Lemma of L whose sub-proof precedes the sub-proof of L, (c) the instance of an Instantiation_Hint among L's used Lemmas whose universal-instantiation step (criterion 2) precedes the sub-proof of L, THEN THE Certificate_Composer SHALL abort composition and THE Verdict_Recorder SHALL label r unchecked and log the reason provenance mismatch.
6. THE Certificate_Composer SHALL discharge inside the Composed_Certificate every assumption of the target proof and of every sub-proof that is not an Assertion of B's locally printed Printed_Goal.
7. IF an open assumption of r's target proof matches no element of r's extended problem (R5.1), THEN THE Certificate_Composer SHALL abort composition and THE Verdict_Recorder SHALL label r unchecked and log the reason target mismatch.
8. THE Certificate_Composer SHALL write every formula of the Composed_Certificate in the symbols that B's locally printed Printed_Goal declares, mapping each Canonical_Name to the symbol that B's Printed_Goal declares under that Canonical_Name, as the Canonicalizer computes it from the locally printed Printed_Goal (R4.1 criterion 14).
9. THE Certificate_Composer SHALL record in every Composed_Certificate, as its input problem, B's locally printed Printed_Goal byte for byte together with its Task_Hash.
10. WHERE partitioning is enabled, WHEN the Certificate_Composer composes for a partitioned Goal_Version B, THE Certificate_Composer SHALL include, for every leaf Partition of B's recorded Partition tree, a scoped sub-proof that derives the negation of the leaf's cube from that leaf's target proof with the leaf's Partition_Assumptions as the scope's assumptions (R3.5 criterion 2).
11. WHERE partitioning is enabled, WHEN the Certificate_Composer composes for a partitioned Goal_Version B, THE Certificate_Composer SHALL derive false from the leaf sub-proofs of criterion 10 by a case analysis over B's recorded split tree, with one proof of l ∨ ¬l for the split literal l of every internal node (P3.5.6).
12. IF the leaf Partitions with a proof-reporting Result are not exactly the leaves of the recorded Partition tree, THEN THE Certificate_Composer SHALL abort composition and THE Verdict_Recorder SHALL label the partitioned Goal_Version's Results unchecked and log the reason incomplete cover.
13. IF a proof received for composition fails to parse under the grammar of its Checkable_Proof_Format (R7.10) or exceeds the size limit of R7.10 criterion 5 for that format, THEN THE Certificate_Composer SHALL abort composition and THE Verdict_Recorder SHALL label r unchecked and log the reason malformed proof.
14. IF composition for r has not finished at r's proof deadline (R5.1), THEN THE Certificate_Composer SHALL abort composition and THE Verdict_Recorder SHALL label r unchecked and log the reason composition timeout.
15. THE Certificate_Composer SHALL print every Composed_Certificate in the supported fragment of its Checkable_Proof_Format.
16. IF a proof received for composition contains a banned command, THEN THE Certificate_Composer SHALL abort composition and THE Verdict_Recorder SHALL label r unchecked and log the reason banned command.
17. IF the Lemma proof of a Lemma L in `oblig(r)` has an open assumption that matches no formula of L's Lemma problem (R5.1), THEN THE Certificate_Composer SHALL abort composition and THE Verdict_Recorder SHALL label r unchecked and log the reason lemma mismatch (AS-34).
18. IF the Lemma proof of a Lemma in `oblig(r)` is in a Checkable_Proof_Format other than that of r's target proof, THEN THE Certificate_Composer SHALL abort composition and THE Verdict_Recorder SHALL label r unchecked and log the reason format mismatch.
19. IF a Canonical_Name that a Lemma in `oblig(r)`, its Dependency_Set or a term of an Instantiation_Hint in `oblig(r)` uses has no symbol that B's locally printed Printed_Goal declares under that Canonical_Name (criterion 8), THEN THE Certificate_Composer SHALL abort composition and THE Verdict_Recorder SHALL label r unchecked and log the reason unmapped symbol.
20. THE Certificate_Composer SHALL turn the Lemma proof of every Lemma L in `oblig(r)`, a refutation of L's Lemma problem, into a derivation of L by a scope step that discharges the assumption ¬L (also when the refutation does not use ¬L) followed by a contradiction or double-negation step, and SHALL use that derivation as the scoped sub-proof of L (AS-23).
21. WHERE partitioning is enabled, THE Certificate_Composer SHALL place the sub-proof of every Lemma whose Dependency_Set contains a Partition_Assumption of a leaf Partition inside the leaf scope of criterion 10 of every leaf whose leaf Result's certification obligation contains that Lemma.

#### Properties

- **P5.2.1** (Composition soundness, metatheory) For any signature Σ_B, Assertion set Γ_B over Σ_B, and sequence X_1, …, X_n of closed, well-sorted formulas over Σ_B such that each X_i either (a) has a derivation π_i, valid in the Lean model of the proof calculus, with open assumptions in `Γ_B ∪ {X_j | j < i}`, or (b) equals `φ_h[t̄/x̄]` for a universally quantified h ∈ Γ_B with matrix φ_h and closed terms t̄ of matching sorts, and any valid target derivation π of false with open assumptions in `Γ_B ∪ {X_1, …, X_n}`: `compose(π, (X_i, π_i)_i)` is a valid derivation of false whose open assumptions are contained in Γ_B, and therefore Γ_B is unsatisfiable. Lean theorem `compose_sound`.
- **P5.2.2** For any Composed_Certificate C produced for r: `open(C) ⊆ assertions(print_local(B))`; and for every sub-proof π_L of a Lemma L in C, every open assumption of π_L is (a) an Assertion of `print_local(B)`, or inside the scope of leaf p a Partition_Assumption of p, whose Canonical_Name and kind are those of an element of `prov(L).deps`, (b) a used Lemma of L whose sub-proof precedes π_L in C, or (c) `instantiate(print_local(B), h)` for an Instantiation_Hint h in `prov(L).used` whose universal-instantiation step precedes π_L in C.
- **P5.2.3** For any certification obligation whose used-Lemma graph (edges from Lemmas only) has a cycle, `compose` returns an error and `label(r) = unchecked`.
- **P5.2.4** (Instance justification) For any C produced for r and any Instantiation_Hint h = (n, t̄) in `oblig(r)`, C contains a universal-instantiation step from the Assertion of `print_local(B)` named n at t̄ whose conclusion is equal as an instance (R4.4) to `instantiate(print_local(B), h)` and is printed in the checker form of R4.4 criterion 20, and that step precedes every use of the instance in C.
- **P5.2.5** (Input problem) For any C produced for r, `input(C) = print_local(B)` byte for byte and `C.task_hash = H(print_local(B))`, whatever the received proofs contain.
- **P5.2.6** (Abort means unchecked) For any r for which `compose` returns an error (dependency cycle, provenance mismatch, target mismatch, lemma mismatch, format mismatch, unmapped symbol, banned command, incomplete cover, malformed proof, composition timeout), `label(r) = unchecked`.
- **P5.2.7** (Partition composition soundness, metatheory; where partitioning is enabled, verified under R3.5 in `ci/lean-partition` so that this requirement's gate does not depend on R3.5) For any Γ_B, any Partition tree T whose leaf cubes c_1 … c_n satisfy `⊨ c_1 ∨ … ∨ c_n`, and any valid leaf derivations of false with open assumptions in `Γ_B ∪ PA(p_i) ∪ {X^i_1, …}` that meet the premises of P5.2.1 for `Γ_B ∪ PA(p_i)`: `compose_partition` returns a valid derivation of false whose open assumptions are contained in Γ_B. Lean theorem `compose_partition_sound`, from `compose_sound`, `partition_sound` and `split_tree_covers` (R3.5).
- **P5.2.8** (Refinement) For any input x, the composition plan that the Rust `compose_plan` returns, or its error, equals that of the Lean reference definition `compose_plan`.
- **P5.2.9** (Round trip) For any proof value p in the supported fragment of CPC or Alethe, `parse(print(p)) = p`; for any accepted proof text t, `parse(print(parse(t))) = parse(t)`.
- **P5.2.10** (Parser totality) For any byte string b, the CPC and Alethe proof parsers of the Certificate_Composer terminate without panic and either reject b or return a proof value.
- **P5.2.11** (Supported fragment) For any command list c of a CPC or Alethe proof, `fragment_ok(c)` holds exactly when every command of c is in the allow-list of its Checkable_Proof_Format and every rule name of c is in the pinned rule set of that format; for any Composed_Certificate C that the Front_End passes to a Certificate_Checker, `fragment_ok(commands(C))`; and for any received proof with a banned command, `compose` returns the error banned command.
- **P5.2.12** (Lemma closing) For any C produced for r and any L ∈ `oblig(r)`, the sub-proof of L in C consists of L's Lemma proof, a scope step that discharges the assumption ¬L, whether or not the Lemma proof uses it, and a contradiction or double-negation step whose conclusion matches L.
- **P5.2.13** (Match) For any formula f of a received proof and any formula x of an extended problem or a Lemma problem, `matches(f, x) ⇔ print_canon(map_B(parse(f))) = print_canon(map_B(parse(x)))`, where `map_B` replaces every Canonical_Name by the symbol of criterion 8; and for any target proof with an open assumption that matches no element of `extended(r)`, or any Lemma proof of L with an open assumption that matches no formula of L's Lemma problem, `compose` returns target mismatch or lemma mismatch respectively.
- **P5.2.14** (Format and symbols) For any r whose Lemma proofs are not all in the Checkable_Proof_Format of r's target proof, `compose` returns format mismatch; and for any r for which a Lemma in `oblig(r)`, its Dependency_Set or a hint term in `oblig(r)` uses a Canonical_Name that `print_local(B)` does not declare, `compose` returns unmapped symbol.
- **P5.2.15** (Leaf scopes, where partitioning is enabled) For any C produced for a partitioned Goal_Version with recorded split tree T: C's case analysis has exactly one proof of l ∨ ¬l per internal node of T, with l the node's split literal; and the sub-proof of every Lemma whose Dependency_Set contains a Partition_Assumption of a leaf lies inside the scope of every leaf whose certification obligation contains that Lemma.

#### Verification

- A1 — Lean 4 theorem `compose_sound` (P5.2.1) — `proofs/lean/DP/Compose.lean` — `ci/lean` (fails on `sorry` or added axioms). `compose_partition_sound` (P5.2.7) is verified under R3.5.
- A1 — Creusot contracts on `compose_plan` for P5.2.2–P5.2.6 and P5.2.12–P5.2.15 — `crates/dp-core/src/compose_plan.rs` (moved from dp-cert, R7.3) — `ci/creusot`; Creusot contracts on the supported-fragment check `fragment_ok` (P5.2.11) — `crates/dp-core/src/fragment.rs` — `ci/creusot`.
- A3 — Certificate_Checker run on every Composed_Certificate against the locally printed Printed_Goal (R5.3) — `crates/dp-cert/src/check.rs` — `ci/certs`, `ci/canaries`.
- A4 — property-based tests of `compose_plan` with generated certification obligations (DAGs, cycles, used Instantiation_Hints, hint used-Lemmas that add no edge, Partition trees with missing leaves, open assumptions outside the Dependency_Set, target proofs with foreign assumptions, Lemma proofs in the wrong format or with unmatched assumptions, Lemma refutations that do not use ¬L, unmapped Canonical_Names) for P5.2.2–P5.2.6, P5.2.13 and P5.2.14 — `crates/dp-cert/tests/prop_compose.rs` — `ci/test`; differential test of the Rust `compose_plan` against the compiled Lean reference (P5.2.8) — `tests/diff/compose_lean_vs_rust/` — `ci/diff`; Partition composition with incomplete covers and leaf-scoped Lemmas (P5.2.15) — `crates/dp-cert/tests/prop_partition.rs` — `ci/test`; property-based test of `fragment_ok` over generated command lists that include each banned command (P5.2.11) — `crates/dp-core/tests/prop_fragment.rs` — `ci/test`.
- A4 — round trip (P5.2.9) — `crates/dp-formats/tests/` — `ci/test`; fuzzing of the CPC and Alethe proof parsers (P5.2.10) — `fuzz/cpc_proof/`, `fuzz/alethe_proof/` — `ci/fuzz-smoke`, `sched/fuzz-nightly`.
- A4 — integration test with real cvc5 Lemma refutations closed by the scope and double-negation steps and checked by the pinned Ethos and Carcara (P5.2.12) — `tests/integration/certs/` — `ci/certs`.
- A4 — Injected_False_Lemmas with forged sub-proofs (invalid steps, open assumptions outside the Dependency_Set, a Trust_Step deriving the Lemma), and certificate canaries with one banned command each (R7.4), whose Composed_Certificates must not be labelled certified — `tests/canaries/` — `ci/canaries`; mutation testing — `tools/mutants/` — `sched/mutants-weekly`.

#### Trust

- None in the Certificate_Composer: a wrong composition is caught by the Certificate_Checker in B's own context (R5.3). AS-6 (checkers accept composed proofs and instantiation steps), AS-23 (scoped sub-proofs), AS-34 (imports appear as matching `assume` steps; usefulness only), AS-36 (cvc5 proofs stay within the supported fragment; usefulness only, because a banned command leaves the Result unchecked under criterion 16), AS-12 (the Lean model of the proof calculus); TB-3 and TB-4 (the semantics of CPC and Alethe as implemented by the pinned checkers). The supported fragment (criterion 15, P5.2.11) keeps every certificate inside the pinned signature, so a certificate cannot add rules that the checker would then accept.

### Requirement 23: R5.3 Independent checking and label assignment

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As a Rust developer, I want a Lemma_Dependent_Result counted as proved only after an independent checker accepts its certificate.

In this requirement, C is the Composed_Certificate produced for a Result r of Goal_Version B that needs certification (R5.1). The *checker outcome* of a Certificate_Checker run is accept, accept with Trust_Steps, or other, as the fixed checker-output table of the TCB_Document (R7.8 criterion 3) assigns it from the run's exit status and output. The *binding checks* are criteria 7, 8, 9, 13, 14, 15 and 19. The *pinned rule set* of a Checkable_Proof_Format is the CPC signature or the Alethe rule list that the Lock_File pins (R5.2).

#### Acceptance Criteria

1. WHEN the Certificate_Composer produces C, THE Front_End SHALL run the Certificate_Checker for C's Checkable_Proof_Format (Ethos for CPC, Carcara for Alethe) on C with B's locally printed Printed_Goal, and not the input problem recorded in C, as the input problem (for Ethos, a reference file, AS-38).
2. THE Front_End SHALL run, for each Checkable_Proof_Format, only the Certificate_Checker version and binary hash that the Lock_File pins, with the option list that the TCB_Document records for that format, and SHALL let Ethos load the CPC signature only from the Lock_File-pinned signature files that this option list names.
3. THE Front_End SHALL run the Certificate_Checker so that the checker checks every universal-instantiation step of C (R5.2 criterion 2) against the Assertion of the input problem that the step instantiates and the step's terms (AS-6).
4. WHEN the checker outcome of the run on C is accept and none of the binding checks fails, THE Verdict_Recorder SHALL mark r checker-accepted.
5. WHEN the checker outcome of the run on C is accept with Trust_Steps (for Ethos, the outcome "incomplete"; for Carcara, a proof with holes, only after the TCB_Document records the Carcara part of AS-6 as confirmed) and none of the binding checks fails, THE Verdict_Recorder SHALL label r checked-incomplete.
6. IF the checker outcome of the run on C is other (rejection, abnormal exit, crash, or output outside the fixed table), or the Certificate_Checker has not reported an outcome at r's certification deadline (R5.1), THEN THE Verdict_Recorder SHALL label r unchecked and SHALL log the checker output.
7. IF the binary hash of the Certificate_Checker that the Front_End is about to run differs from the hash that the Lock_File pins, THEN THE Verdict_Recorder SHALL label r unchecked and SHALL log the mismatch (R7.7 criterion 4; a mismatch found at Session start aborts the Session under R7.7 criterion 3).
8. IF the input problem recorded in C is not byte-identical to B's locally printed Printed_Goal, compared by Task_Hash, THEN THE Verdict_Recorder SHALL label r unchecked and SHALL log the reason input mismatch.
9. IF the Certificate_Checker run does not establish that every assumption of C is an Assertion of the input problem of criterion 1 (AS-30), THEN THE Verdict_Recorder SHALL label r unchecked and SHALL log the reason assumption mismatch.
10. WHEN r's certification deadline passes while the Certificate_Checker runs on C, THE Front_End SHALL terminate the checker process and every process that the checker process started.
11. THE Verdict_Recorder SHALL compute the label of r under criteria 4 to 9 and 13 to 19 as a pure function of the checker outcome, the elapsed time, the checker binary hash, the option list used, the hashes of the signature files loaded, the Lock_File, the TCB_Document checker entries, the Task_Hash of C's recorded input problem, the locally recomputed Task_Hash, the result of the assumption check of criterion 9, the results of the checks of criteria 15 and 19, and the step at which the Certificate_Store completed r's entry (R7.3).
12. WHEN the Verdict_Recorder labels r after a Certificate_Checker run, THE Verdict_Recorder SHALL log the checker name, version, binary hash, option list, the hashes of the signature files loaded, exit status, checker outcome and elapsed time.
13. IF the option list of the Certificate_Checker run on C differs from the option list that the TCB_Document records for C's Checkable_Proof_Format, THEN THE Verdict_Recorder SHALL label r unchecked and SHALL log the reason option mismatch.
14. IF the Certificate_Checker run on C loads a signature file that the TCB_Document option list does not name, or a signature file whose hash differs from its Lock_File entry, THEN THE Verdict_Recorder SHALL label r unchecked and SHALL log the reason signature mismatch.
15. WHILE the TCB_Document records the Carcara part of AS-6 as To check, IF C is an Alethe proof whose parsed form contains a `hole` step or a step whose rule is outside the pinned Alethe rule list, THEN THE Verdict_Recorder SHALL label r unchecked and SHALL log the reason Alethe hole, whatever the checker outcome.
16. THE Front_End SHALL pass to every Certificate_Checker only options that are absent from the banned-option list of the TCB_Document, which lists every option that makes a checker treat a rule outside its pinned rule set as a hole or as a trusted step (TB-11).
17. WHEN the Certificate_Store has completely stored the entry of a checker-accepted Result r (R5.4 criterion 1) no later than r's certification deadline, THE Verdict_Recorder SHALL label r certified.
18. IF a checker-accepted Result r has no completely stored Certificate_Store entry at r's certification deadline, THEN THE Verdict_Recorder SHALL label r unchecked and SHALL log the reason storage incomplete.
19. IF C, as parsed by the Format_Codec, fails the supported-fragment check `fragment_ok` of dp-core (R5.2 criterion 15), THEN THE Verdict_Recorder SHALL label r unchecked and SHALL log the reason banned command.

#### Properties

- **P5.3.1** (Exact rule) For any Result r that needs certification and whose Composed_Certificate C was checked, let `bind(r) = binary_ok(r) ∧ opts(r) = tcb_options(fmt(C)) ∧ sig_ok(r) ∧ fragment_ok(C) ∧ hole_ok(C) ∧ H(input(C)) = H(print_local(B)) ∧ assumptions(C) ⊆ assertions(print_local(B))`, where `hole_ok(C)` fails exactly when `fmt(C) = Alethe`, the TCB_Document records the Carcara part of AS-6 as To check, and C contains a `hole` step or a rule outside the pinned Alethe rule list. Then `label(r) = certified ⇔ out(r) = accept ∧ bind(r) ∧ t_out(r) ≤ cert_deadline(r) ∧ t_store(r) ≤ cert_deadline(r)`, where `t_store(r)` is the step at which the Certificate_Store completed r's entry; `label(r) = checked-incomplete ⇔ out(r) = accept with Trust_Steps ∧ bind(r) ∧ t_out(r) ≤ cert_deadline(r)` and the storage of R5.4 criterion 1 did not fail; otherwise `label(r) = unchecked`.
- **P5.3.2** For any exit status and output bytes that the fixed table does not map to accept, `label ≠ certified`.
- **P5.3.3** (False Lemmas) For any Injected_False_Lemma L of the Canary_Suite, with recorded Dependency_Set Δ, used Lemmas U and confirmed model of Δ ∪ U ∪ {¬L}, and any Composed_Certificate with a sub-proof of L whose open assumptions are within Δ ∪ U, the pinned Certificate_Checker's outcome is not accept, and the Result is not labelled certified.
- **P5.3.4** (Input binding) For any Result r of B and any Composed_Certificate C′ with `input(C′) ≠ print_local(B)`, including a valid certificate for another Goal_Version B′, `label(r) = unchecked` whatever the checker outcome; and for any single-byte mutation C′ of C with `input(C′) = print_local(B)`, `label(r) = certified` only if the checker outcome on C′ against `print_local(B)` is accept.
- **P5.3.5** (Totality) For any tuple of the inputs of criterion 11 (checker outcome, elapsed time, binary hash, option list, signature-file hashes, Task_Hashes, assumption-check result, results of the checks of criteria 15 and 19, storage completion step), `label_from_checker_output` returns exactly one of certified, checked-incomplete and unchecked.
- **P5.3.6** (No acceptance, no proof label) For any trace and any Result r that needs certification and has no checker run with outcome accept, `label(r) ∉ {proved-standalone, certified}`.
- **P5.3.7** (Instantiation steps, AS-6) For any certificate in the Canary_Suite with a universal-instantiation step whose conclusion is not the instance of its premise at its terms (wrong term, swapped variables, wrong sort, wrong Hypothesis), the pinned Certificate_Checker's outcome is not accept.
- **P5.3.8** (Checker identity) For any checker run that the Verdict_Recorder uses for a proof label, the logged version and binary hash equal the Lock_File entry, the option list equals the TCB_Document entry for the format, and every loaded signature file is named by that option list and has its Lock_File hash.
- **P5.3.9** (Checker deadline) For any checker run on C still running at `cert_deadline(r)`, the trace contains at that time the termination of the checker process and every process it started, and `label(r) = unchecked`.
- **P5.3.10** (Output parser totality) For any byte string b as checker output and any exit status, the checker-output classifier terminates without panic and returns one checker outcome.
- **P5.3.11** (Option binding) For any Result r whose checker run used an option list different from `tcb_options(fmt(C))`, or loaded a signature file outside that list or with a hash other than its Lock_File entry, `label(r) = unchecked`; and no option list in `docs/tcb.toml` contains an option of the banned-option list.
- **P5.3.12** (Alethe holes) While the TCB_Document records the Carcara part of AS-6 as To check, for any Result r whose Composed_Certificate is an Alethe proof containing a `hole` step or a rule outside the pinned Alethe rule list, `label(r) = unchecked`, whatever the checker outcome.
- **P5.3.13** (Storage before certified) For any trace and any Result r labelled certified at step t: r was checker-accepted before t, the Certificate_Store completed r's entry at a step no later than t, and `t ≤ cert_deadline(r)`; and for any checker-accepted r whose entry is not complete at `cert_deadline(r)`, `label(r) = unchecked`.

#### Verification

- A3 — Certificate_Checker run per Result against the locally printed Printed_Goal, with the binding checks — `crates/dp-cert/src/check.rs` — `ci/certs`, `ci/canaries`.
- A1 — Creusot contracts on `label_from_checker_output` (P5.3.1, P5.3.2, P5.3.5, P5.3.6, P5.3.11–P5.3.13) — `crates/dp-core/src/labels.rs` — `ci/creusot`.
- A4 — property-based test of `label_from_checker_output` over generated outcome tuples, including option lists, signature-file hashes, Alethe holes and storage completion steps (P5.3.1, P5.3.2, P5.3.5, P5.3.11–P5.3.13) — `crates/dp-core/tests/prop_verdict.rs` — `ci/test`; differential test of the Rust `label` against the compiled Lean reference — `tests/diff/label_lean_vs_rust/` — `ci/diff`.
- A4 — tampered, single-byte-mutated and goal-swapped certificates (P5.3.4) — `tests/certs/tamper/` — `ci/certs`.
- A4 — Canary_Suite certificates for Injected_False_Lemmas and wrong universal-instantiation steps (P5.3.3, P5.3.7), the Trust_Step certificate canary once per Checkable_Proof_Format, an Alethe certificate with a `hole` step (P5.3.12), and one certificate canary per banned command (R7.4) — `tests/canaries/` — `ci/canaries`.
- A4 — integration tests with a substituted checker binary, a crashing stub checker, a stub checker with unknown output, a slow stub checker that ignores termination and starts child processes, a checker run with an option list other than the TCB_Document entry, a substituted CPC signature file, and a check of every option list of `docs/tcb.toml` against the banned-option list (P5.3.8, P5.3.9, P5.3.11) — `tests/integration/certs/` — `ci/certs`.
- A4 — fuzzing of the checker-output classifier (P5.3.10) — `fuzz/checker_output/` — `ci/fuzz-smoke`, `sched/fuzz-nightly`; mutation testing — `tools/mutants/` — `sched/mutants-weekly`.

#### Trust

- TB-3 and TB-4 (Ethos and Carcara at the Lock_File versions and binary hashes) and TB-11 (their option lists, the signature files they name, the banned-option list and the fixed checker-output table in the TCB_Document) are trusted for the label certified. AS-6 (instantiation steps and composed proofs are checked; criterion 15 covers the unconfirmed Carcara part), AS-23 (scoped sub-proofs), AS-30 (assumptions are bound to the input problem), AS-38 (the Printed_Goals of certifying Drivers contain no `define-fun-rec`, `define-funs-rec`, `push` or `pop`, which an Ethos reference file may not contain; if one does, its Results stay unchecked in CPC, which affects usefulness only).

### Requirement 24: R5.4 Certificate storage

**Priority:** Must

**Class:** Functional

**User Story:** As an auditor, I want certificates stored locally and referenced from `proof.json`, so that I can re-check them offline.

In this requirement, the *manifest* of a Certificate_Store entry lists every file of the entry with its content hash, and the *content hash* of the entry is the hash of the manifest's canonical printed form (R7.10).

#### Acceptance Criteria

1. WHEN the Verdict_Recorder marks a Result r of Goal_Version B checker-accepted (R5.3 criterion 4) or labels r checked-incomplete (R5.3 criterion 5), THE Certificate_Store SHALL store one entry that contains r's Composed_Certificate, B's locally printed Printed_Goal, the checker name, version, binary hash, option list and the hashes of the signature files loaded (R5.3 criterion 2), the checker exit status, output and outcome, the Provenance_Record of every Lemma and Instantiation_Hint in `imports*(r)` (R3.2 criterion 10, R4.4 criterion 19), and, for audit only, the Provenance_Record or the unresolved identifier of every used Lemma of an Instantiation_Hint in `imports*(r)` (R4.4 criterion 19).
2. THE Certificate_Store SHALL address every entry by its content hash.
3. WHEN the Verdict_Recorder records a Verdict from a Result labelled certified or checked-incomplete, THE Verdict_Recorder SHALL write the content hash of the Result's completely stored Certificate_Store entry to that Verdict's `proof.json` entry (R2.3 criterion 2).
4. THE Verdict_Recorder SHALL write a content hash to `proof.json` only after the Certificate_Store has completely written the entry with that content hash.
5. WHEN the Certificate_Store receives an entry whose content hash equals that of a stored entry, THE Certificate_Store SHALL keep the stored entry unchanged.
6. IF the Certificate_Store fails to store the entry of criterion 1 completely, THEN THE Verdict_Recorder SHALL label r unchecked and SHALL log the storage failure.
7. THE Certificate_Store SHALL reside in a project-relative directory, `.dp-certs/`, that is readable without access to the Cluster.
8. IF the recomputed content hash of a Certificate_Store entry differs from the hash recorded in `proof.json`, or a file of the entry has a hash different from the hash in the entry's manifest, THEN THE Replay_Tool SHALL report the entry as corrupted and SHALL report the goal with the replay outcome hash-mismatch (R7.6).
9. IF `proof.json` records a content hash for which the Certificate_Store holds no entry, THEN THE Replay_Tool SHALL report the entry as missing and SHALL report the goal as failed with the reason missing entry (R7.6).

#### Properties

- **P5.4.1** (Round trip) For any entry e, `put(store, e)` returns `H(canon(manifest(e)))`, and `read(store, put(store, e)) = e`.
- **P5.4.2** (Tamper) For any change to a stored entry (a byte of any file, a manifest field) and any swap of a content hash in `proof.json`, the Replay_Tool reports every affected goal as hash-mismatch or failed; and for any consistent rewrite of an entry's files, its manifest and its content hash in `proof.json` whose stored Printed_Goal differs from the re-printed Printed_Goal, or whose Composed_Certificate the pinned Certificate_Checker does not accept against the re-printed Printed_Goal, the Replay_Tool does not confirm any affected goal (R7.6 criteria 2 to 4).
- **P5.4.3** (Audit trail) For any Result r labelled certified or checked-incomplete, the Certificate_Store entry of r contains `prov(x)` for every x ∈ `imports*(r)`, Lemmas and Instantiation_Hints alike, and the Provenance_Record or the identifier of every used Lemma of each Instantiation_Hint in `imports*(r)`.
- **P5.4.4** (Idempotence) For any store state s and entry e, `put(put(s, e), e) = put(s, e)`.
- **P5.4.5** (Crash safety) For any crash point during a Session, every content hash in `proof.json` after the crash resolves to a complete entry whose files match the manifest's hashes.
- **P5.4.6** (Storage failure) For any Result r whose entry the Certificate_Store fails to store completely, `label(r) = unchecked`.
- **P5.4.7** (Entry consistency) For any entry e stored for Result r of B, `H(e.printed_goal) = H(print_local(B))` and e's checker version and binary hash equal the Lock_File entry.
- **P5.4.8** (Store before certified) For any trace and any Result r labelled certified at step t, the Certificate_Store holds at t a complete entry for r's Composed_Certificate, and the content hash written to `proof.json` for r's Verdict is that entry's content hash.

#### Verification

- A2 — TLA+ storage action with the invariant `CertifiedHasStoredEntry`: no Result carries the label certified without a complete Certificate_Store entry (P5.4.8), checked with TLC — `verification/tla/Coordinator.tla` — `ci/tlc`.
- A4 — property-based round trip, idempotence and entry-consistency tests of the Certificate_Store (P5.4.1, P5.4.4, P5.4.7) — `crates/dp-cert/tests/prop_store.rs` — `ci/test`; manifest codec round trip — `crates/dp-formats/tests/` — `ci/test`; fuzzing of the manifest parser — `fuzz/cert_manifest/` — `ci/fuzz-smoke`.
- A4 — tamper and missing-entry tests, including consistent rewrites of an entry, its manifest and its `proof.json` content hash (P5.4.2, criteria 8 and 9) — `tests/replay/tamper/` — `ci/replay`; audit-trail check on Sessions from `ci/integration-cluster` (P5.4.3) — `tests/replay/` — `ci/replay`.
- A4 — crash and disk-full injection during entry and `proof.json` writes, with crashes between checker acceptance and storage completion (P5.4.5, P5.4.6, P5.4.8) — `tests/integration/certs/store_faults.rs` — `ci/certs`.

#### Trust

- TB-1 (collision resistance of the hash function). Changes to the local file system between the write and the replay are not trusted: criteria 8 and 9 detect changes that break a recorded hash, and R7.6 criteria 2 to 4 detect a consistent rewrite of an entry, its manifest and its content hash in `proof.json`, because the Replay_Tool re-prints the Goal_Version, compares the stored Printed_Goal with the re-print and re-runs the Certificate_Checker against the re-print.

### Requirement 25: R5.5 Certify-all mode

**Priority:** Should

**Class:** Soundness-critical

**User Story:** As an auditor, I want an option that requires certificates for standalone results too, so that trust in solvers and Workers can be removed where certificates exist.

NG-7 and R7.1 criterion 2 rely on this requirement: in a build that does not implement it, certify-all mode is off in every Session. In certify-all mode, a Local_Fast_Pass Result that reports a proof is certified on the local machine before escalation, and its Goal_Version stays local if it is labelled certified (R1.2 criteria 2, 8 and 9).

#### Acceptance Criteria

1. WHERE certify-all mode is enabled, THE Verdict_Recorder SHALL treat every Result that reports a proof as a Result that needs certification (R5.1), including a Result that meets the proved-standalone condition of R7.1 criterion 2.
2. WHERE certify-all mode is enabled, THE Verdict_Recorder SHALL assign the label proved-standalone to no Result.
3. WHERE certify-all mode is enabled, IF no Composed_Certificate can be produced for a Result that reports a proof (for example, because the Result's prover has no Checkable_Proof_Format, NG-5), THEN THE Verdict_Recorder SHALL label the Result unchecked.
4. WHERE certify-all mode is enabled, WHEN the Verdict_Recorder reads from the Result_Cache an entry that reports a proof, THE Verdict_Recorder SHALL label the entry certified only if the Certificate_Checker, run under R5.3 in the current Session on the Composed_Certificate of the Certificate_Store entry that the cache entry references, has outcome accept against the locally printed Printed_Goal and none of the binding checks of R5.3 fails (R7.1 criterion 6 states the same rule for every mode).
5. IF the certify-all option is set to a value other than `on` or `off`, THEN THE Front_End SHALL report a configuration error that names the rejected value, SHALL stop before starting any solver process, SHALL return a non-zero exit status to the Creusot_CLI, and SHALL leave `proof.json` and the `.why3find` cache unchanged.
6. WHERE certify-all mode is enabled, IF the Verdict_Recorder cannot label a Result_Cache entry that reports a proof certified under criterion 4, THEN THE Verdict_Recorder SHALL treat the lookup as a Result_Cache miss, so that the Goal_Version runs the Local_Fast_Pass (R1.2 criterion 1).

#### Properties

- **P5.5.1** With certify-all enabled, for any proof-labelled Verdict v, `v.label = certified`.
- **P5.5.2** With certify-all enabled, for any Result r that reports a proof and has no Composed_Certificate, `label(r) = unchecked`; and for any Result r that reports a proof, `label(r) ∈ {certified, checked-incomplete, unchecked}`.
- **P5.5.3** (Mode monotonicity) For any Result r and any certification outcome o, `is_proof(label_on(r, o)) ⇒ is_proof(label_off(r, o))`, where `label_on` and `label_off` are the label functions with certify-all enabled and disabled.
- **P5.5.4** (Differential) For any Goal_Version G in the CI corpus, run once with certify-all enabled and once disabled, with all other configuration equal: every proof-labelled Verdict of the enabled run is labelled certified. Agreement with Invalid answers is left to the conflict tests of R2.2 (P2.2.9).
- **P5.5.5** (Cache) With certify-all enabled, for any Result_Cache hit labelled certified in Session S, S contains a Certificate_Checker run with outcome accept on the referenced Composed_Certificate against `print_local(G)`.
- **P5.5.6** For any certify-all option value outside {`on`, `off`} and any project state, running the prove command starts no solver process, ends with a non-zero exit status, and leaves `proof.json` and the `.why3find` cache byte-identical to their state before the run.
- **P5.5.7** (Cache miss) With certify-all enabled, for any Result_Cache entry e that reports a proof and that the Verdict_Recorder does not label certified under criterion 4, the lookup of e is a miss, no Verdict is recorded from e, and the Goal_Version is not excluded under R1.2 criterion 4.

#### Verification

- A1 — Creusot contracts on `label` with the certify-all flag (P5.5.1–P5.5.3) and on the cache-miss rule (P5.5.7) — `crates/dp-core/src/verdict.rs` — `ci/creusot`.
- A3 — per-Result certificate checking against the locally printed Printed_Goal (R5.3), including cache hits (criterion 4) — `crates/dp-cert/src/check.rs` — `ci/certs`.
- A4 — property-based test of `label` in certify-all mode (P5.5.1–P5.5.3, P5.5.5, P5.5.7) — `crates/dp-core/tests/prop_verdict.rs` — `ci/test`; differential test, certify-all on vs. off (P5.5.4) — `tests/diff/certify_all_on_off/` — `ci/diff`; malformed option values (P5.5.6) — `tests/integration/config/` — `ci/integration-cluster`; Canary_Suite in certify-all mode — `tests/canaries/` — `ci/canaries`; mutation testing — `tools/mutants/` — `sched/mutants-weekly`.
- A5 — certify-all overhead and coverage (RQ5) — `eval/scripts/rq5_certificates/` — `sched/eval`.

#### Trust

- For every proof label: TB-3, TB-4 and TB-11 as listed for certified (R5.3), plus the items that no certificate removes (NG-1, NG-2, NG-4, AS-12). Unpatched_Solvers and authenticated Workers are not trusted in this mode. The interaction with the Local_Fast_Pass is fixed by R1.2 criteria 2, 8 and 9 (OQ-N51 resolved).

---

**Group R6: Evaluation**

### Requirement 26: R6.1 Versioned corpus of hard Creusot goals

**Priority:** Must

**Class:** Functional

**User Story:** As a researcher, I want a versioned corpus of hard Creusot goals exported as SMT-LIB with metadata, so that experiments are reproducible and comparable across tools.

In this requirement:

- The *candidate set* is every Goal_Version that why3find generates, with the pinned Creusot, Why3 and why3find, for the projects of criterion 3 at their pinned commits.
- The *reference Portfolio* is the Portfolio that the corpus configuration names for export.
- The *hardness measurement* is a recorded run, under the pre-registration file `eval/rq/corpus.toml` (R6.2 criterion 17), of why3find's default strategy with the pinned provers on every candidate, on the reference machine described in its run manifest (R6.3 criterion 4), with reference time limit `<TBD-36>` seconds per goal, solver seeds derived from the run seed of that pre-registration (R6.3 criterion 2) and `<TBD-17>` repetitions, stored as raw data under `eval/raw/corpus/<run-id>/`.
- A candidate is *hard* when the Local_Fast_Pass with the default budget `<TBD-1>` does not prove it in any repetition of the hardness measurement, and either (a) its median time to a proof over the repetitions exceeds `<TBD-16>` seconds of reference time (*proved-hard*), or (b) at least one repetition ends without a proof within `<TBD-36>` seconds (*unsolved*) (OQ-N49).

#### Acceptance Criteria

1. THE Corpus_Exporter SHALL export each selected Goal_Version, for each Driver and Transformation_Pipeline of the reference Portfolio, as the exact Printed_Goal that why3find sends to the prover.
2. THE Corpus_Exporter SHALL record for each exported Goal_Version: source project and commit, Rust crate, module and function, why3find goal path, Goal_Hash, Driver and Transformation_Pipeline hashes, Creusot, Why3, why3find and solver versions, Task_Hash, Sibling_Goals group, known status, hardness class (proved-hard or unsolved), the hardness-measurement values (median time and number of repetitions with a proof), the canary flag, and the source project's license identifier.
3. THE Corpus_Exporter SHALL draw candidates from Creusot's test suite and from CreuSAT at pinned commits, plus the further projects listed in the corpus configuration (OQ-N9; Creusot projects only, NG-10).
4. THE Corpus_Exporter SHALL select exactly the candidates that are hard under the hardness measurement named in the corpus configuration, and SHALL record `<TBD-1>`, `<TBD-16>`, `<TBD-36>`, `<TBD-17>` and the hardness measurement's run identifier and raw-data hash in the corpus metadata.
5. THE Corpus_Exporter SHALL include in every release every Canary_Goal that the Canary_Suite (R7.4) generates from the projects of criterion 3, with the canary flag set and a reference to its invalidity witness.
6. THE Corpus_Exporter SHALL record every candidate that is not exported in the release's exclusion list with one reason: not hard, export failure, or license.
7. IF a project's license identifier in the corpus configuration is not in the configuration's redistribution allow-list, THEN THE Corpus_Exporter SHALL exclude that project's candidates from the release with the reason license (AS-15).
8. WHEN the Corpus_Exporter runs twice on the same pinned inputs and the same hardness measurement, THE Corpus_Exporter SHALL produce byte-identical release files.
9. THE Corpus_Exporter SHALL assign each corpus release a version identifier equal to the content hash of the release's canonical printed form.
10. THE Corpus_Exporter SHALL produce with each release a CI corpus: the subset of the release that the CI selection rule of the corpus configuration selects, together with every Canary_Goal of the release (OQ-N50).

#### Properties

- **P6.1.1** (Export fidelity) For any corpus entry e and any Driver and Transformation_Pipeline of the reference Portfolio, the exported bytes equal the bytes captured from why3find's prover call for e's Goal_Version, and `H(bytes, driver, pipeline) = e.task_hash`.
- **P6.1.2** (Determinism) For any pinned inputs and hardness measurement, two exports produce byte-identical releases with equal version identifiers.
- **P6.1.3** (Round trip) For any metadata record m, `parse(print(m)) = m`; for any exclusion list x, `parse(print(x)) = x`.
- **P6.1.4** (Selection) For any hardness measurement M and any non-canary candidate G from a project whose license is in the allow-list and whose export succeeds: `G ∈ release(M) ⇔ hard(M, G)`, where `hard` is the rule defined above.
- **P6.1.5** (Canaries) Every Canary_Goal generated from the projects of criterion 3 is in the release and in the CI corpus with the canary flag set, and no entry that is not a Canary_Goal carries the canary flag.
- **P6.1.6** (Versioning) For any two releases R1 and R2, `version(R1) = version(R2) ⇔ canon(R1) = canon(R2)`, assuming a collision-free hash.
- **P6.1.7** (No silent drops) For any candidate G, exactly one of the following holds: G is in the release, or G is in the exclusion list with exactly one reason.

#### Verification

- A4 — determinism (export twice and compare), metadata schema, selection rule over generated hardness measurements, canary inclusion, exclusion-list completeness and license allow-list (P6.1.2, P6.1.4–P6.1.7) — `eval/corpus/tests/` — `ci/corpus-check`; the same checks on the full corpus export — `eval/corpus/tests/` — `sched/eval`.
- A4 — differential test of exported text vs. text captured from why3find (P6.1.1) — `tests/diff/export_vs_why3find/` — `ci/diff`.
- A4 — round trip of the corpus metadata and exclusion-list codecs (P6.1.3) — `crates/dp-formats/tests/` — `ci/test`; fuzzing of the corpus metadata parser — `fuzz/corpus_metadata/` — `ci/fuzz-smoke`.
- A5 — hardness measurement with pinned versions, seeds derived from the pre-registration and repeated runs under the protocol of R6.3 — `eval/scripts/corpus_hardness/`, `eval/rq/corpus.toml` — `sched/eval`.

#### Trust

- AS-15 (redistribution), AS-16 (projects build with the pinned Creusot), AS-17 (repeatable solver answers, which make the hardness measurement meaningful). The hardness class depends on the reference machine (NG-11).

### Requirement 27: R6.2 Research questions

**Priority:** Must

**Class:** Performance

**User Story:** As a researcher, I want research questions with metrics fixed before the runs, so that the evaluation answers them rather than reporting post-hoc numbers.

In this requirement:

- The *research questions* are RQ1 to RQ7 of criteria 4 to 10. The *non-canary goals* of a research question are its corpus entries without the canary flag (R6.1 criterion 5) and, for RQ7, the candidates of R6.1; every metric is computed over non-canary goals only (criterion 14).
- The *escalation event* of a goal in one repetition is the step at which the Scheduler marks the goal's Goal_Version as an Escalated_Goal; in a configuration of unmodified pinned why3find, which has no Scheduler, it is the end of the goal's first sequential pass.
- The *budget* is `<TBD-37>` wall-clock seconds. The *time to Verdict* of a goal in one repetition is the wall-clock time from its escalation event (for RQ7, from Session start) to the step at which its Verdict is recorded. The time to Verdict is *censored* when the goal has no proof-labelled Verdict within the budget, and a censored time is set to the budget. The *time to first reported proof* is defined in the same way, ending at the acceptance of the goal's first Result that reports a proof, whatever its Assurance_Label.
- A goal is *proved within the budget* in a repetition when its Verdict in that repetition is proof-labelled and its time to Verdict is at most the budget; a Lemma_Dependent_Result counts only once it is labelled certified.
- The *per-goal time* of a goal in a configuration is the median of its times to Verdict over the `<TBD-17>` repetitions, with censored times set to the budget.
- The *proved-count difference* of a comparison is the mean over repetitions i of the treatment's number of goals proved within the budget in repetition i minus the baseline's number in repetition i, with a paired bootstrap confidence interval that resamples goals (`<TBD-38>` resamples, with the bootstrap seed of the pre-registration) at level `<TBD-18>`. It is the primary metric of RQ1 to RQ4.
- The *time ratio* of a comparison is the geometric mean, over the goals proved within the budget in at least one repetition of either configuration, of the treatment's per-goal time divided by the baseline's per-goal time, with a paired bootstrap confidence interval as for the proved-count difference. Because censored times enter it at the budget, it is reported as a sensitivity analysis.
- A *proportion* is reported with a Wilson score interval at level `<TBD-18>`.
- The *per-project Session makespan* of a configuration in one repetition is the wall-clock time from the start of the first Session (for unmodified pinned why3find, the first prove invocation) to the end of the last Session that proves a project's non-canary goals. The *total solver CPU-seconds* is the sum of the CPU time of every solver process of those Sessions. The *scaling efficiency* of a configuration against its baseline is (baseline makespan × baseline core count) / (configuration makespan × Worker count × cores per Worker), with the core counts taken from the per-machine hardware descriptions of the run manifests (R6.3 criterion 4). Each of the three is reported per project as the median over repetitions, with a bootstrap confidence interval that resamples repetitions.
- The *hardness re-measurement* of a baseline configuration is the proportion of its non-canary corpus entries whose per-goal time is at most `<TBD-16>` seconds, measured on solver seeds disjoint from those of the hardness measurement (R6.3 criterion 14).
- The *pre-registration fields* of a research question q are: (a) the metrics, with exactly one marked as the *primary metric* of q (the proved-count difference for RQ1 to RQ4); (b) the baseline and treatment configurations; (c) the statistical method of each metric; (d) the *run seed*, from which R6.3 criterion 2 derives every solver and simulation seed, and the *bootstrap seed*; (e) the corpus version; (f) the requirement that q gives A5 evidence for; (g) the hypothesis target of the primary metric and its direction (`<TBD-27>` for RQ1–RQ4, `<TBD-28>` for RQ5, `<TBD-29>` for RQ6, with one threshold per RQ6 metric, and `<TBD-39>` for RQ7); (h) the *run order* of the baseline and treatment runs, interleaved or randomized with a recorded seed (R6.3 criterion 13); (i) the warm-cache treatments, if any (R6.3 criterion 12); (j) the *run selection*, which is either a list of run identifiers or the rule "all eligible runs at this hash, pooled" (R6.4 criterion 10); (k) the differences between baseline and treatment run manifests that P6.2.3 allows: configuration options, machine set, Worker count and solver binary hashes; and (l) for RQ1 and RQ2, the goal submission order and the number of goals submitted concurrently (R6.3 criterion 15).
- The *decision rule* of a research question q: q's hypothesis target *held* when the confidence interval of q's primary metric at level `<TBD-18>`, after Holm adjustment across the primary metrics of RQ1 to RQ7, lies entirely beyond the target in the pre-registered direction, and *did not hold* otherwise. Every metric other than the primary metric of its research question is labelled *exploratory* and gets no held or not-held statement.
- Raw data of research question q is stored under `eval/raw/<q>/<run-id>/` (R6.3 criterion 9), and its script is under `eval/scripts/<q>_<name>/`. The statistical methods are a draft choice (OQ-N52).

#### Acceptance Criteria

1. THE Experiment_Runner SHALL read, for each research question q, a pre-registration file `eval/rq/<q>.toml` that fixes every pre-registration field of q (introduction, items (a) to (l)).
2. IF the pre-registration file of q is absent from the repository commit of a run of q, fails to parse, or lacks a field of criterion 1 (for q = corpus, of criterion 17), THEN THE Experiment_Runner SHALL start no run of q and SHALL report the missing or malformed field.
3. THE Experiment_Runner SHALL record in every run manifest the hash of the pre-registration file that the run used.
4. THE Experiment_Runner SHALL measure RQ1 (distribution speedup; A5 evidence for R1.1) on the corpus release, for the Cluster at each Worker count in `<TBD-23>` against the baseline of unmodified pinned why3find on one machine of the same hardware: the proved-count difference, the time ratio, the per-project Session makespan, the total solver CPU-seconds and the scaling efficiency, with script `eval/scripts/rq1_distribution/` and raw data `eval/raw/rq1/`.
5. THE Experiment_Runner SHALL measure RQ2 (portfolio benefit; A5 evidence for R2.2): the proved-count difference, the time ratio, the per-project Session makespan, the total solver CPU-seconds and the scaling efficiency of portfolio racing against the baseline of why3find's default strategy on the same Cluster and Worker count, with script `eval/scripts/rq2_portfolio/` and raw data `eval/raw/rq2/`.
6. THE Experiment_Runner SHALL measure RQ3 (within-goal sharing benefit; A5 evidence for R3.1): the RQ2 metrics, and the time ratio of time to first reported proof next to the time ratio of time to Verdict, with within-goal sharing enabled against a control arm that runs the same Portfolio, Cluster and Sharing_Solver binaries with no import mark and with Lemma export disabled, so that the Results of both arms are Lemma_Dependent_Results that need certification and the arms differ only in Lemma traffic; together with the counts of Lemmas exported, forwarded, admitted and rejected per reason category (R3.4 criterion 6) and the number of goals whose solver reported a proof that did not reach certified, with script `eval/scripts/rq3_within_goal/` and raw data `eval/raw/rq3/`.
7. THE Experiment_Runner SHALL measure RQ4 (cross-goal sharing benefit; A5 evidence for R4.2 and R4.4): the RQ3 metrics of criterion 6 for three treatments (cross-goal Ground_Lemmas only, Instantiation_Hints only, both) against the baseline with within-goal sharing only, with the same Sharing_Solver binaries in every arm, together with the counts of admitted and rejected cross-goal Lemmas and Instantiation_Hints per reason category taken from the Worker decision log of R4.2 criterion 7 and the rejection log of R4.4 criterion 9, with script `eval/scripts/rq4_cross_goal/` and raw data `eval/raw/rq4/`.
8. THE Experiment_Runner SHALL measure RQ5 (certificates; A5 evidence for R5.1 and R5.5): per Result that needs certification, proof production time, composition time, checking time and certificate size, as medians with bootstrap confidence intervals; coverage as the proportions of such Results labelled certified, checked-incomplete and unchecked, with unchecked broken down by logged reason; and the time ratio of certify-all mode against the baseline with certify-all disabled, with script `eval/scripts/rq5_certificates/` and raw data `eval/raw/rq5/`.
9. THE Experiment_Runner SHALL measure RQ6 (instability; A5 evidence for R1.4): for every configuration of RQ1 to RQ5 and RQ7, the proportion of goals whose outcome, proof-labelled or non-proof Verdict, differs across seeds, repetitions or Workers, with a Wilson interval, and the median over goals of the per-goal coefficient of variation of time to Verdict over repetitions, with censored times set to the budget and a bootstrap confidence interval that resamples goals, against the baseline of repeated runs of unmodified pinned why3find, with script `eval/scripts/rq6_instability/` and raw data `eval/raw/rq6/`.
10. THE Experiment_Runner SHALL measure RQ7 (escalation; A5 evidence for R1.2, OQ-N24) on the full candidate set of R6.1 with an empty Result_Cache, with times to Verdict measured from Session start: the proportion of goals that the Local_Fast_Pass proves; the Cluster latency avoided, defined per goal proved by the Local_Fast_Pass as its time to Verdict when the Experiment_Runner submits it directly to the Cluster minus its time to Verdict in the Local_Fast_Pass, summed per project with a bootstrap confidence interval over goals; the Local_Fast_Pass time spent on goals that escalate, summed per project with a bootstrap confidence interval over goals; and the net per-project makespan difference, the per-project Session makespan with the Local_Fast_Pass minus that with direct Cluster submission; against the baseline of direct Cluster submission of the same goals, with script `eval/scripts/rq7_escalation/` and raw data `eval/raw/rq7/`.
11. THE Experiment_Runner SHALL report, for each research question, only metrics named in that question's pre-registration file.
12. THE Experiment_Runner SHALL report every metric of RQ1 to RQ7 over all non-canary goals and separately for each hardness class of R6.1 (proved-hard, unsolved), and, for RQ7, whose goals include candidates that are not hard, also for those candidates.
13. THE Experiment_Runner SHALL report the hardness re-measurement (introduction) of the baseline configuration of each of RQ1 to RQ7, with a Wilson interval.
14. THE Experiment_Runner SHALL compute every metric of RQ1 to RQ7 only from raw records without the canary tag (R6.3 criterion 6) of non-canary goals.
15. THE Experiment_Runner SHALL measure, within RQ6 (A5 evidence for R1.4), the *cross-Worker reproduction* of every proof-labelled Result with an empty import list in the RQ6 runs, by re-running the Result's Solver_Configuration with its recorded seed and option list on a Worker other than the one that produced the Result, once with the local time limit that this Worker's Calibration gives for the recorded reference time limit and once with the unscaled reference time limit, and SHALL report, for each of the two limits, the proportion of re-runs that report a proof, with a Wilson interval.
16. THE Experiment_Runner SHALL measure, within RQ6 (A5 evidence for R7.6 and P2.3.4), the *replay agreement* of the proved-standalone Verdicts of the RQ6 runs, by running the Replay_Tool (R7.6) with real solvers on each run's `proof.json` on a machine that produced none of its Results, and SHALL report the proportion of those Verdicts in each replay outcome of R7.6 (confirmed, timed-out, unknown, contradicted, binary-mismatch, hash-mismatch, failed), each with a Wilson interval.
17. THE Experiment_Runner SHALL read, for the hardness measurement of R6.1, the pre-registration file `eval/rq/corpus.toml`, which fixes the corpus configuration hash, the reference Portfolio, the run seed, the run selection and the values of `<TBD-1>`, `<TBD-16>`, `<TBD-17>` and `<TBD-36>`, and SHALL apply criteria 2 and 3 to the hardness measurement with q = corpus.

#### Properties

- **P6.2.1** (Pre-registration) For any q ∈ {RQ1, …, RQ7, corpus} and any run R whose data the report uses for q: `R.manifest.prereg_hash = H(file(eval/rq/q.toml, R.commit)) = H(file(eval/rq/q.toml, report_commit))`, and every metric reported for q is named in that file.
- **P6.2.2** (Completeness) For each q ∈ {RQ1, …, RQ7}, `eval/rq/q.toml` parses, names every pre-registration field of criterion 1 and exactly one primary metric, which for RQ1 to RQ4 is the proved-count difference; `eval/rq/corpus.toml` parses and names every field of criterion 17; and the report contains one result entry for each metric that a file names.
- **P6.2.3** (Pairing) For every comparison of RQ1 to RQ5 and RQ7, the baseline and treatment run manifests have equal corpus version, Lock_File hash, per-machine hardware descriptions, time limits and seed-derivation inputs (the run seed, the goal set and the repetition indices of R6.3 criterion 2), and equal machine sets and Worker counts unless the pre-registration names the difference; for every prover and version, the sets of solver binary hashes of the Jobs in the raw records of the two arms are equal unless the pre-registration names the difference; and the configurations differ only in the configuration options that the pre-registration names. The derived seeds themselves may differ, because they depend on the Solver_Configurations (for example, RQ2's Portfolio).
- **P6.2.4** (Decision-log counts) For any RQ4 run, every reported admission and rejection count per reason category equals the number of matching entries, in raw records without the canary tag, of the Worker decision logs (R4.2 criterion 7) and hint rejection logs (R4.4 criterion 9) in that run's raw data.
- **P6.2.5** (Coverage partition) For any RQ5 run, the counts of Results that need certification labelled certified, checked-incomplete and unchecked sum to the number of such Results in the raw records without the canary tag of the run's raw data, and the unchecked reasons sum to the unchecked count.
- **P6.2.6** (Censoring) For any raw data D of a configuration, any goal g and any repetition i: g is proved within the budget in i exactly when `label(verdict(g, i))` is a proof label and `t(g, i) ≤ <TBD-37>`; `per_goal_time(g) = median_i(t′(g, i))` with `t′(g, i) = t(g, i)` if g is proved within the budget in i and `t′(g, i) = <TBD-37>` otherwise; and the proved-count difference equals `mean_i(Σ_g proved_T(g, i) − Σ_g proved_B(g, i))`.
- **P6.2.7** (Hardness classes) For any report, every metric of RQ1 to RQ7 has one entry over all non-canary goals and one entry per hardness class (for RQ7, also for candidates that are not hard), the per-class goal counts sum to the overall goal count, and the hardness re-measurement of a baseline equals the number of its non-canary corpus entries with per-goal time at most `<TBD-16>` divided by the number of its non-canary corpus entries.
- **P6.2.8** (Control arm) For any RQ3 run, for every Solver_Configuration the two arms ran the same Sharing_Solver binary hashes, and no Job of the control arm carried the import mark, exported a Lemma or received a Lemma; for any RQ4 run, every arm ran the same Sharing_Solver binary hashes; and the report of RQ3 and RQ4 contains both time ratios of criterion 6.
- **P6.2.9** (Derived metrics) For any raw data, the per-project Session makespan, the total solver CPU-seconds and the RQ7 Local_Fast_Pass time on escalated goals equal the values recomputed from the raw Session and solver-process records, and the scaling efficiency equals `(m_B × cores_B) / (m_T × W × cores_T)` with m the makespans, W the Worker count and the core counts of the manifests.
- **P6.2.10** (Reproduction) For any RQ6 run and any proof-labelled Result r with an empty import list in its raw data, the raw data contain exactly two re-run records of r's Solver_Configuration, seed and option list on a Worker other than r's, one at that Worker's calibrated local time limit and one at r's unscaled reference time limit, and each reported reproduction proportion equals the share of its re-run records that report a proof.
- **P6.2.11** (Replay agreement) For any RQ6 run, the raw data contain exactly one Replay_Tool outcome among the outcomes of R7.6 for each proved-standalone Verdict, and the reported per-outcome counts sum to the number of proved-standalone Verdicts.

#### Verification

- A5 — experiment scripts with pre-registrations — `eval/scripts/rq1_distribution/`, `eval/scripts/rq2_portfolio/`, `eval/scripts/rq3_within_goal/`, `eval/scripts/rq4_cross_goal/`, `eval/scripts/rq5_certificates/`, `eval/scripts/rq6_instability/`, `eval/scripts/rq7_escalation/`, `eval/rq/` — `sched/eval`.
- A4 — each script, including the hardness measurement under `eval/rq/corpus.toml`, runs on the CI corpus as a smoke run (R6.3 criterion 16), and the run fails if a pre-registration is missing or incomplete, names no primary metric or a primary metric other than the proved-count difference for RQ1 to RQ4, or if a reported metric is absent from the pre-registration (P6.2.1, P6.2.2) — `eval/tests/` — `ci/eval-smoke`.
- A4 — manifest pairing with per-machine hardware descriptions, seed-derivation inputs and per-Job solver binary hashes, control-arm, decision-log count and coverage-partition checks on the smoke runs and on committed raw data (P6.2.3–P6.2.5, P6.2.8) — `eval/tests/` — `ci/eval-smoke`, `ci/eval-report`.
- A4 — property-based tests of the metric computations over generated raw data: censoring, per-goal time, proved-count difference, time ratio, hardness-class breakdown and hardness re-measurement, makespan, CPU-seconds, scaling efficiency, the RQ7 metrics, cross-Worker reproduction and replay agreement (P6.2.6, P6.2.7, P6.2.9–P6.2.11) — `eval/tests/metrics/` — `ci/eval-smoke`.

#### Trust

- Measurement environment noise, addressed by R6.3. Results hold for the recorded hardware only (NG-11). TB-21 (the bootstrap and Wilson intervals assume that goals are exchangeable samples of the corpus, OQ-N52).
- AS-39 (why3find's Calibration applies when the Worker or the Front_End starts the prover binary directly, design decision D-16): RQ6 (criteria 9 and 15) measures the instability and cross-Worker reproduction that a failure of AS-39 would cause.

### Requirement 28: R6.3 Experimental protocol

**Priority:** Must

**Class:** Performance

**User Story:** As a researcher, I want every experiment run under a fixed, recorded protocol, so that another group can reproduce the numbers.

In this requirement:

- An *evaluation run* is one execution of the experiment script of one research question (R6.2) or of the hardness measurement (R6.1), under `sched/eval` or, as a *smoke run*, under `ci/eval-smoke`; the *evaluation job* is the `sched/eval` or `ci/eval-smoke` job that executes it.
- The *measured pass* of a configuration is the part of an evaluation run whose raw records the metrics of R6.2 use; the *canary pass* is the separate Canary_Suite pass of criterion 6.
- A configuration is *cold* when the pre-registration does not name it as a warm-cache treatment (R6.2 introduction, item (i)).
- The *planned repetition count* of an evaluation run is `<TBD-17>`, or one for a smoke run (criterion 16).

#### Acceptance Criteria

1. THE Experiment_Runner SHALL run every evaluation run with the Pinned_Dependencies of the Lock_File.
2. THE Experiment_Runner SHALL run repetition i of every Job of an evaluation run whose prover takes a seed option with the seed `derive(s, c, g, i)` in place of the configured seed of the Job's Solver_Configuration c, where s is the run seed of the pre-registration (R6.2 introduction, item (d)), g is the Goal_Hash of the Job's Goal_Version, and `derive` hashes s, c's name and configured seed, g and i and reduces the hash to the prover's seed range; SHALL run repetition i of a simulation-mode run with the simulation seed `derive(s, sim, i)`; SHALL record the mark unseeded for a Job whose prover takes no seed option; and SHALL record each seed used in the run manifest and in the raw solver-start records.
3. THE Experiment_Runner SHALL run every pair of goal and configuration of an evaluation run the planned repetition count of times.
4. THE Experiment_Runner SHALL record in each run manifest: corpus version, Lock_File hash, tool versions and binary hashes, the machine set with a hardware and operating system description per machine that includes its core count, Worker count and Calibrations, the run seed, the bootstrap seed and every derived seed, time limits, the pre-registration hash (R6.2 criterion 3), the Canary_Suite version, the commit of this repository, the run's position in the run order (criterion 13), the initial-state hash of every configuration (criterion 12), the smoke flag and the planned repetition count (criterion 16), and the run status (complete, incomplete or failed).
5. THE Experiment_Runner SHALL report each metric with a confidence interval at level `<TBD-18>`, computed with the method that R6.2 names for that metric.
6. THE Experiment_Runner SHALL run the Canary_Suite (R7.4) for every configuration of every evaluation run as a canary pass separate from the measured pass, with the solver binaries and option lists of that configuration's measured pass, and SHALL tag every raw record of the canary pass with the canary tag.
7. IF, during an evaluation run, any Canary_Goal receives a proof-labelled Verdict, a goal receives the label certified from a Composed_Certificate that contains a sub-proof of a Lemma canary whose open assumptions are within the canary's recorded Δ ∪ U, or a Result with a certificate canary as its Composed_Certificate receives the label certified (R7.4 criterion 13), THEN THE Experiment_Runner SHALL fail the evaluation job and SHALL report the goal, configuration, seed and Result, and the Lemma canary or certificate canary involved.
8. THE Experiment_Runner SHALL set the status failed in the manifest of every evaluation run in which a condition of criterion 7 occurred.
9. THE Experiment_Runner SHALL store every raw measurement, one record per goal, configuration, repetition and Worker, under `eval/raw/<q>/<run-id>/`, where q is the research question or `corpus` for the hardness measurement, each record naming every Job of the measurement with its Solver_Configuration, seed and solver binary hash, the Result_Cache and `.why3find` cache hits, the times of the escalation event, of the first Result that reports a proof and of the Verdict, and the canary tag of criterion 6.
10. IF an evaluation run ends before every planned raw measurement of criterion 3 is recorded, THEN THE Experiment_Runner SHALL set the status incomplete in the run manifest and SHALL record the missing goal and configuration pairs.
11. IF, during an evaluation run, a tool binary hash differs from the Lock_File, whether the Session-start check of R7.7 criterion 3 or a later check of R7.7 criteria 4 and 7 finds it, THEN THE Experiment_Runner SHALL set the status failed in the run manifest.
12. THE Experiment_Runner SHALL start every cold configuration of an evaluation run with an empty Result_Cache, an empty `.why3find` cache, an empty Certificate_Store and an empty Lemma_Pool on every machine of the run, and SHALL record in the run manifest, for every configuration, the hash of the initial state of these four stores.
13. THE Experiment_Runner SHALL execute the baseline and treatment runs of every comparison in the interleaved or randomized run order that the pre-registration fixes (R6.2 introduction, item (h)), and SHALL record each run's position in that order in its run manifest.
14. THE Experiment_Runner SHALL use, in every evaluation run of a research question, only solver seeds outside the set of solver seeds recorded in the manifest of the hardness measurement of the run's corpus release.
15. WHEN the Experiment_Runner runs RQ1 or RQ2, THE Experiment_Runner SHALL submit goals in the order and with the number of concurrently submitted goals that the pre-registration fixes (R6.2 introduction, item (l)), and SHALL record each submission in the raw records.
16. WHEN `ci/eval-smoke` starts an evaluation run, THE Experiment_Runner SHALL set the smoke flag and a planned repetition count of one in the run manifest.
17. WHEN the CI runs `ci/eval-report`, THE CI SHALL apply the checks of P6.3.1, P6.3.3, P6.3.4, P6.3.7, P6.3.8, P6.3.9 and P6.3.12 to the manifest and raw data of every run committed under `eval/raw/`, and SHALL record every run that fails one of these checks as not eligible (R6.4), with the failing check as the reason.

#### Properties

- **P6.3.1** For any run manifest, every field listed in criterion 4 is present, the tool versions and binary hashes match the Lock_File whose hash the manifest records, and the status is one of complete, incomplete and failed.
- **P6.3.2** For any two simulation-mode runs with the same seeds and inputs, the Scheduler's decision sequences are identical.
- **P6.3.3** (Seeds) For any evaluation run R with run seed s, every seed in R's raw solver-start records for repetition i of a Job with Solver_Configuration c and Goal_Hash g equals `derive(s, c, g, i)`, every simulation seed of repetition i equals `derive(s, sim, i)`, and every bootstrap resampling of the report for R's research question uses the bootstrap seed of the pre-registration; and two runs with equal seed-derivation inputs use identical seed sequences.
- **P6.3.4** (Repetitions) For any evaluation run with status complete, every pair of goal and configuration of its plan has exactly the planned repetition count of raw measurement records in its measured pass.
- **P6.3.5** (Canary gate) For any evaluation run in which a Canary_Goal receives a proof-labelled Verdict, or a valid goal or a Result is labelled certified under a condition of R7.4 criterion 13, the evaluation job fails and the run's status is failed.
- **P6.3.6** (Round trip) For any raw measurement record m and run manifest r, `parse(print(m)) = m` and `parse(print(r)) = r`.
- **P6.3.7** (Cold start) For any cold configuration of any evaluation run, the raw records of its measured pass contain zero Result_Cache hits and zero `.why3find` cache hits, and its initial-state hash in the manifest equals the hash of the empty Result_Cache, `.why3find` cache, Certificate_Store and Lemma_Pool.
- **P6.3.8** (Run order) For any comparison, the positions of its baseline and treatment runs in their manifests follow the run order of the pre-registration: alternating for interleaved order, and the permutation derived from the recorded seed for randomized order.
- **P6.3.9** (Seed disjointness) For any evaluation run R of a research question, the solver seeds in R's manifest and the solver seeds in the manifest of the hardness measurement of R's corpus release are disjoint sets.
- **P6.3.10** (Canary pass) For any evaluation run and any configuration c of it, the raw data contain canary-tagged records of a canary pass for c whose solver binary hashes and option lists equal those of c's measured pass, and no record of c's measured pass carries the canary tag.
- **P6.3.11** (Smoke runs) For any run of `ci/eval-smoke`, the manifest has the smoke flag set and a planned repetition count of one, and no run whose manifest has the smoke flag set is eligible (R6.4).
- **P6.3.12** (Submission order) For any RQ1 or RQ2 run, the order of goal submissions and the number of concurrently submitted goals in the raw records equal those that the pre-registration fixes.
- **P6.3.13** (Checks on committed data) For any run committed under `eval/raw/` that fails P6.3.1, P6.3.3, P6.3.4, P6.3.7, P6.3.8, P6.3.9 or P6.3.12, the run is not eligible and the report lists it with the failing check as the reason (R6.4 criterion 8).

#### Verification

- A5 — evaluation runs under the protocol — `eval/scripts/` — `sched/eval`.
- A4 — manifest schema, seed derivation, repetition count, status, cold-start, run-order, seed-disjointness, submission-order and smoke-flag checks on smoke runs (P6.3.1, P6.3.3, P6.3.4, P6.3.7–P6.3.9, P6.3.11, P6.3.12) and seed determinism in simulation mode (P6.3.2) — `eval/tests/` — `ci/eval-smoke`.
- A4 — the manifest, seed-derivation, repetition-count, cold-start, run-order, seed-disjointness and submission-order checks on every run committed under `eval/raw/`, with eligibility set from their outcome (criterion 17; P6.3.1, P6.3.3, P6.3.4, P6.3.7–P6.3.9, P6.3.12, P6.3.13) — `eval/report/check/` — `ci/eval-report`.
- A4 — canary-pass separation and tagging checks on smoke runs (P6.3.10) — `eval/tests/canary_gate/` — `ci/eval-smoke`; the same checks on committed raw data (P6.3.10) — `eval/report/check/` — `ci/eval-report`.
- A4 — canary-gate self-test: smoke runs with a stub prover that reports a proof for a Canary_Goal, and with a stub checker that accepts a certificate deriving a Lemma canary or a certificate canary, must fail the job and set the status failed (P6.3.5) — `eval/tests/canary_gate/` — `ci/eval-smoke`.
- A4 — round trip of the raw-measurement and run-manifest codecs (P6.3.6) — `crates/dp-formats/tests/` — `ci/test`.

#### Trust

- AS-17 (repeatable solver answers). TB-20 (hardware isolation is recorded in the manifest but not checked mechanically); results hold for the recorded hardware only (NG-11).

### Requirement 29: R6.4 Report generated from raw data

**Priority:** Must

**Class:** Performance

**User Story:** As a researcher, I want every reported number generated from raw data by scripts, so that no number is copied by hand.

In this requirement, the *report template* is the source of the evaluation report under `eval/report/`, a *generated-value reference* is a placeholder in the template that the report scripts replace with a value computed from raw data, and the *identifier allow-list* is the template's list of patterns for tokens whose digits are part of an identifier rather than a measurement (requirement, property, research-question and TBD IDs, and section numbers). A run of a research question or of the hardness measurement (q = corpus, R6.2 criterion 17) is *eligible* when its manifest status is complete, its manifest does not have the smoke flag set (R6.3 criterion 16), its pre-registration hash satisfies P6.2.1, and the checks of R6.3 criterion 17 pass on its committed manifest and raw data. The *used runs* of q are the eligible runs that q's run selection names (R6.2 introduction, item (j)): the listed run identifiers, or every eligible run of q whose pre-registration hash equals the hash of `eval/rq/<q>.toml` at the report commit, pooled.

#### Acceptance Criteria

1. THE Experiment_Runner SHALL generate every number, table and figure of the evaluation report from raw data under `eval/raw/` of used runs, with scripts under `eval/report/`.
2. WHEN the CI runs `ci/eval-report`, THE CI SHALL regenerate the report from the committed raw data and SHALL fail if the generated report differs in any byte from the committed report.
3. WHEN the CI runs `ci/eval-report`, THE CI SHALL fail if the report template contains a numeric literal outside a generated-value reference that matches no pattern of the identifier allow-list.
4. THE Experiment_Runner SHALL write, for every generated value, a provenance entry that names the script, the raw-data files with their hashes, and the run identifiers from which the value was computed.
5. WHEN the CI runs `ci/eval-report`, THE CI SHALL fail if a provenance entry names a raw-data file that is absent or has a different hash, or names a run that is not eligible.
6. THE Experiment_Runner SHALL show every reported point estimate together with its confidence interval (R6.3 criterion 5) and the number of goals and repetitions it rests on.
7. THE Experiment_Runner SHALL state in the report, for the primary metric of each research question, whether the pre-registered hypothesis target held or did not hold under the decision rule (R6.2 introduction), or is unset because its TBD value is undecided, and SHALL label every other metric exploratory.
8. THE Experiment_Runner SHALL list in the report every evaluation run that is not eligible, with its status and reason.
9. WHEN the CI runs `ci/eval-report`, THE CI SHALL fail if, for some q ∈ {RQ1, …, RQ7, corpus}, the first commit that adds `eval/rq/<q>.toml` in the git history of the commit under test is not an ancestor of the commit recorded in the manifest of every run under `eval/raw/<q>/`.
10. WHEN the CI runs `ci/eval-report`, THE CI SHALL fail if the runs that the provenance entries of q's generated values name differ from the used runs of q.
11. WHEN the CI runs `ci/eval-report`, THE CI SHALL fail if a held or not-held statement of criterion 7 differs from the outcome of the decision rule (R6.2 introduction) applied to the generated interval and the pre-registered target and direction, or is stated for a metric other than the primary metric of its research question.
12. WHEN the CI runs `ci/eval-report`, THE CI SHALL fail if a reported point estimate lacks its confidence interval at level `<TBD-18>`, its number of goals or its number of repetitions (criterion 6), or if the provenance entry of the interval names a method other than the method that the pre-registration names for that metric (R6.3 criterion 5).
13. WHEN the CI runs `ci/eval-report`, THE CI SHALL fail if the raw data of some configuration of a used run contain no record with the canary tag (R6.3 criterion 6).
14. WHEN the CI runs `ci/eval-report`, THE CI SHALL fail if the provenance entry of a generated value of RQ1 to RQ7 names a raw record with the canary tag or a goal with the canary flag (R6.2 criterion 14).
15. THE Experiment_Runner SHALL list in the report, for each research question and for the hardness measurement, every eligible run and every commit that changed its pre-registration file.

#### Properties

- **P6.4.1** (Determinism) For any committed raw data D, `report(D)` is deterministic and equals the committed report byte for byte.
- **P6.4.2** (No hand-typed numbers) For any numeric token in the generated report, the token comes from a generated-value reference with a provenance entry, or matches a pattern of the identifier allow-list.
- **P6.4.3** (Provenance) For any generated value v, recomputing v with the script named in its provenance entry from the raw-data files it names, after checking their hashes, gives v.
- **P6.4.4** (Exclusions) The set of runs listed under criterion 8 equals the set of non-eligible runs under `eval/raw/`, and no provenance entry names a non-eligible run.
- **P6.4.5** (Check sensitivity) Each of these changes makes `ci/eval-report` fail: inserting a numeric literal outside a generated-value reference into the template; changing one raw value without regenerating the report; marking a used run incomplete; adding a run whose recorded commit precedes the first commit of its pre-registration file; adding to the provenance a run outside the run selection; flipping a held or not-held statement; stating held or not held for an exploratory metric; removing an interval, a goal count or a repetition count; removing the canary-tagged records of a configuration; and adding a canary-tagged record to a generated value's provenance.
- **P6.4.6** (Pre-registration precedence) For any q ∈ {RQ1, …, RQ7, corpus} and any run R under `eval/raw/<q>/`, the first commit that adds `eval/rq/<q>.toml` in the git history of the report commit is an ancestor of R's recorded commit; and the report lists every eligible run of q and every commit that changed `eval/rq/<q>.toml`.
- **P6.4.7** (Run selection) For any q, the set of runs named by the provenance entries of q's generated values equals the used runs of q.
- **P6.4.8** (Decision rule) For any research question q, the report states held or not held only for q's primary metric, states held exactly when the Holm-adjusted interval at `<TBD-18>` lies entirely beyond q's pre-registered target in the pre-registered direction, states unset exactly when the target's TBD value is undecided, and labels every other metric exploratory.
- **P6.4.9** (Estimates) Every reported point estimate is shown with its interval at `<TBD-18>`, computed with the method that the pre-registration names for its metric, and with the number of goals and repetitions it rests on.
- **P6.4.10** (Canary records present) For any used run and any configuration of it, the raw data contain at least one record with the canary tag.
- **P6.4.11** (Canary exclusion) For any generated value of RQ1 to RQ7, its provenance entry names no raw record with the canary tag and no goal with the canary flag.

#### Verification

- A4 — report regeneration and byte comparison (P6.4.1) — `eval/report/` — `ci/eval-report`.
- A4 — numeric-literal scan of the template, provenance check, pre-registration history check, run-selection check, decision-rule check (criteria 7 and 11), estimate check (criteria 6 and 12, R6.3 criterion 5), canary-record presence check (criterion 13, R6.3 criterion 6) and canary-exclusion check (criterion 14) (P6.4.2–P6.4.4, P6.4.6–P6.4.11) — `eval/report/check/` — `ci/eval-report`.
- A4 — self-test of the checks with each seeded change of P6.4.5 (P6.4.5) — `eval/tests/report_check/` — `ci/eval-report`.
- A5 — the reported numbers themselves, generated from `sched/eval` raw data — `eval/report/` — `sched/eval`.

#### Trust

- TB-15 (the plotting and statistics libraries, pinned in the Lock_File). The scan of criterion 3 does not detect a hand-typed measurement written inside a token that matches an identifier pattern, so the patterns must require the identifier prefix (for example `RQ`, `TBD-`, `P`).
- The check of criterion 9 relies on the commit that the Experiment_Runner records in each run manifest, and needs the full git history in `ci/eval-report`.

---

**Group R7: Cross-cutting**

### Requirement 30: R7.1 Verdict soundness

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As a Rust developer, I want a goal recorded as proved only when an Unpatched_Solver proved the goal without imported Lemmas or an independent checker accepted the goal's certificate, so that distribution, sharing and caching cannot turn a false goal into a proved one.

In this requirement, G is a Goal_Version, `print_local(G)` is G's locally printed Printed_Goal, and, for a partitioned Goal_Version, "the Result" means the set of leaf Results that needs certification as one unit (R5.1).

#### Acceptance Criteria

1. THE Verdict_Recorder SHALL record a proof-labelled Verdict for a Goal_Version only from a Result labelled proved-standalone under criterion 2 or certified under criterion 3.
2. THE Verdict_Recorder SHALL label a Result proved-standalone exactly when certify-all mode is off (R5.5 criterion 2; certify-all mode is off in every Session of a build that does not implement R5.5), the Result reports a proof, the Verdict_Recorder accepted the Result under R1.3 (for a Result produced on the local machine: its Printed_Goal is `print_local(G)` and its solver binary passed R7.7 criteria 3 and 7), the Result is for the whole Goal_Version and not for a Partition (R3.5 criterion 2), and the Result is not a Lemma_Dependent_Result: its import list is empty, its import-set hash is the hash of the empty set, its Job carried no import mark (R3.1 criterion 5), and the Lock_File lists its solver binary hash as an Unpatched_Solver for the prover and version of its Solver_Configuration (R1.3 criterion 5).
3. THE Verdict_Recorder SHALL label a Result certified only if the Result needs certification (R5.1), the Result was checker-accepted under R5.3 criterion 4 in a Certificate_Checker run in the current Session, and the Certificate_Store completely stored the Result's entry no later than the Result's certification deadline (R5.3 criteria 17 and 18, R5.4 criterion 1), where the checker run used the checker version and binary hash that the Lock_File pins, the option list that the TCB_Document records and only Lock_File-pinned signature files that this option list names (R5.3 criteria 2, 7, 13 and 14), took `print_local(G)` as its input problem (R5.3 criterion 1), and reported the checker outcome accept before the Result's certification deadline (R5.3 criterion 6); the input problem recorded in the Composed_Certificate has the Task_Hash of `print_local(G)` (R5.3 criterion 8); the run established that every assumption of the Composed_Certificate is an Assertion of `print_local(G)` (R5.3 criterion 9, AS-30); and the Composed_Certificate passed the supported-fragment check and, for Alethe, the hole check (R5.3 criteria 15 and 19).
4. IF a Result that reports a proof needs certification and is not labelled certified, THEN THE Verdict_Recorder SHALL label the Result checked-incomplete when R5.3 criterion 5 applies and unchecked otherwise, and SHALL record no proof-labelled Verdict from that Result.
5. THE Verdict_Recorder SHALL compute every label as a pure function (R7.3) of the Result's fields, the Task_Hash of `print_local(G)`, the Lock_File, the TCB_Document checker entries, the certify-all setting, the import mark of the Result's Job, and, for a Result that needs certification, the outcomes of re-admission (R3.4 criterion 9, R4.4 criterion 15), composition (R5.2), checking (R5.3 criterion 11) and storage (R5.4 criterion 6).
6. WHEN the Verdict_Recorder reads a Result_Cache entry that reports a proof, THE Verdict_Recorder SHALL label the entry under criteria 2 to 4 as a Result received in the current Session, with the Task_Hash of `print_local(G)` recomputed in the current Session, the current Lock_File and certify-all setting, the validation-field checks of R7.5 criterion 9 (which include, for an entry with the mark local as for every other entry, the check of the stored solver binary hash against the current Lock_File), the checks of R7.5 criterion 4, and, for the label certified, a Certificate_Checker run under R5.3 in the current Session on the Composed_Certificate of the Certificate_Store entry that the cache entry references, with the certification start of R5.1 at the step at which the Verdict_Recorder reads the entry, in every mode (R5.5 criterion 4 states the same rule for certify-all mode; OQ-N53).
7. THE System SHALL authenticate Results (R1.3 criterion 2), Lemmas (R3.2 criterion 4) and Instantiation_Hints (R4.4 criterion 3) with one scheme, Ed25519 signatures with one key pair per Worker (OQ-N11 resolved by design decision D-9), under which the Front_End and every Worker verify the tag of any registered Worker from that Worker's public verification data alone, the Front_End taking the registered Worker identities and their verification data only from the operator-signed Worker registry (R1.3 criterion 9), and under which the verification data of one Worker gives no other party the ability to produce a tag that verifies for that Worker's identity.

#### Properties

- **P7.1.1** (Verdict soundness, implementation) For any Session, any Goal_Version G and any recorded Verdict v of G with `v.label ∈ {proved-standalone, certified}`, either (a) `¬certify_all ∧ v.result.answer = Valid ∧ v.result.task_hash = H(print_local(G)) ∧ v.result.partition = none ∧ v.result.imports = ∅ ∧ v.result.imports_hash = H(canon(∅)) ∧ ¬import_mark(job(v.result)) ∧ (v.result.config.prover, v.result.config.version, v.result.binary_hash) ∈ unpatched(Lock_File)`, or (b) the Session contains a Certificate_Checker run k on v's Composed_Certificate C with `out(k) = accept ∧ k.binary_hash = lock(k.checker) ∧ k.options = tcb_options(k.format) ∧ sig_ok(k) ∧ k.input = print_local(G) ∧ H(input(C)) = H(print_local(G)) ∧ assumptions(C) ⊆ assertions(print_local(G)) ∧ fragment_ok(C) ∧ hole_ok(C) ∧ t_out(k) ≤ cert_deadline(v.result)`, and the Certificate_Store completed an entry for C at a step no later than `cert_deadline(v.result)`; for a Verdict recorded from a Result_Cache entry, `cert_deadline` is counted from the step at which the Verdict_Recorder read the entry (R5.1, P5.1.5).
- **P7.1.2** (Verdict soundness, metatheory) In a Lean trace model of Sessions without partitioning, for any Goal_Version G with Assertion set Γ_G = `assertions(print_local(G))` and any Verdict v of G recorded in the trace, under these named hypotheses, each mapped to the trusted items it stands for:
  - `SolverSound`: for any tuple (prover, version, binary hash, option list, problem) whose prover, version and binary hash the Lock_File marks as an Unpatched_Solver, a proof answer implies that the problem is unsatisfiable (NG-3, TB-12, TB-22, OQ-N37);
  - `WorkerHonest`: the solver binary hash, import list and import mark that an authenticated Result reports are those of the run that produced the Result (NG-7, TB-2);
  - `NoCollision`: the hash function is injective on the values whose hashes the trace compares (Task_Hashes, import-set hashes, Certificate_Store content hashes) (TB-1);
  - `CheckerSound`: if the pinned checker binary, run with the TCB_Document option list on input file P, reports accept on a certificate C, then C is a derivation of false whose open assumptions are a subset of `assertions(parse(P))` (TB-3, TB-4, TB-11).

  Then (a) if `v.label = proved-standalone`, Γ_G is unsatisfiable, from `SolverSound`, `WorkerHonest`, `NoCollision` and the Lock_File Unpatched_Solver marks; and (b) if `v.label = certified`, Γ_G is unsatisfiable, from `CheckerSound` and `NoCollision` alone. Lean theorem `verdict_sound`, with these four hypotheses as its only hypotheses and no added axioms. The theorems `admission_sound`, `admission_sound_seq` (R3.4), `cross_goal_sound` and `canon_injective` (R4.1, R4.2) and `hint_sound` (R4.4) are usefulness and certifiability theorems (P7.1.8), not premises of `verdict_sound`. The partitioned case is the separate theorem `verdict_sound_partition` (P3.5.9, R3.5), so that this requirement's gate does not depend on R3.5.
- **P7.1.3** (Exclusions) For any Result r with `r.imports ≠ ∅`, an import-marked Job, a Sharing_Solver, a solver binary hash outside `unpatched(Lock_File)`, a Partition, or certify-all mode on: `label(r) ≠ proved-standalone`.
- **P7.1.4** For any Canary_Goal C, any canary mode, any Injected_False_Lemma set, any adversarial Coordinator schedule and any seed, the Verdict of C is not proof-labelled (checked by R7.4).
- **P7.1.5** (Standalone completeness) For any Result r that meets every condition of criterion 2, `label(r) = proved-standalone`.
- **P7.1.6** (Cache hits) For any Session S and any Result_Cache entry e that S labels: `label_S(e)` equals the label that criteria 2 to 4 give e as a Result received in S, with `H(print_local(G))`, the Lock_File and the certify-all setting of S; and if `label_S(e) = certified`, S contains a Certificate_Checker run on e's referenced Composed_Certificate that satisfies (b) of P7.1.1.
- **P7.1.7** (Authentication scheme) For any registered Worker W, tag verification for W takes only W's public verification data as key input; and for any Worker W′ ≠ W that holds its own credential and the public verification data of every Worker, no tag that W′ produces for a message naming W verifies as W's in the forgery simulation.
- **P7.1.8** (Certifiability, metatheory) In the Lean model of AS-12, for any Result r of Goal_Version G with target problem Γ_b = Γ_G ∪ PA(r) (PA(r) = ∅ for a whole-Goal_Version Result) such that r's import list passed the single re-admission pass of R3.4 against Γ_b, with the Lemmas exported by r's own Job counted as available, `compose_plan` returned a plan for r, and `Δ_L ∪ U_L ⊨ L` holds for every Lemma L in `oblig(r)` with Dependency_Set Δ_L and used Lemmas U_L: `Γ_b ⊨ X` for every element X of r's extended problem (R5.1), and the composed derivation has open assumptions in Γ_b. Lean theorem `certifiable`, proved from `admission_sound`, `admission_sound_seq` (R3.4), `cross_goal_sound` and `canon_injective` (R4.1, R4.2), `hint_sound` (R4.4) and `compose_sound` (R5.2), with no added axioms. It shows that the certificates the System composes can be accepted; it is not a premise of `verdict_sound`.

#### Verification

- A1 — Lean 4 theorem `verdict_sound` (P7.1.2) — `proofs/lean/DP/Verdict.lean` — `ci/lean` (fails on `sorry` or added axioms); Lean 4 theorem `certifiable` (P7.1.8) — `proofs/lean/DP/Certifiable.lean` — `ci/lean`.
- A1 — Creusot contracts on `label` and `record_verdict` (P7.1.1 for the label function, P7.1.3, P7.1.5, P7.1.6) — `crates/dp-core/src/verdict.rs` — `ci/creusot`.
- A3 — per-Result Task_Hash recomputation and authentication check (R1.3) — `crates/dp-frontend/src/accept.rs` — `ci/sim`, `ci/canaries`.
- A3 — per-Result Certificate_Checker run with the binding checks (R5.3), also for certified Result_Cache hits (criterion 6) — `crates/dp-cert/src/check.rs` — `ci/certs`, `ci/canaries`.
- A4 — property-based tests of `label` (P7.1.1, P7.1.3, P7.1.5, P7.1.6) — `crates/dp-core/tests/prop_verdict.rs` — `ci/test`; differential test of the Rust `label` against the compiled Lean reference — `tests/diff/label_lean_vs_rust/` — `ci/diff`; differential test, cache on vs. off, with forged and stale entries (P7.1.6) — `tests/diff/cache_on_off/` — `ci/diff`.
- A4 — canaries (P7.1.4) — `tests/canaries/` — `ci/canaries`, `sched/canaries-full`; deterministic simulation with an adversarial Coordinator — `tests/sim/adversarial_coordinator.rs` — `ci/sim`, `sched/sim-nightly`; simulation with Workers that forge other Workers' tags (P7.1.7) — `tests/sim/lying_provenance.rs` — `ci/sim`; mutation testing (R7.13) — `tools/mutants/` — `sched/mutants-weekly`.

#### Trust

- As listed in the TCB_Document: Unpatched_Solvers and authenticated Workers (proved-standalone only, NG-3, NG-7); TB-12 (Solver_Configuration option lists, proved-standalone only); TB-22 (Lock_File Unpatched_Solver marks of release artifacts accepted by published checksum, proved-standalone only, design decision D-8); TB-3, TB-4 and TB-11 (Ethos and Carcara with the Lock_File versions, option lists and checker-output table; certified); NG-2 (Why3 VC generation, transformations and Drivers); NG-1 (Creusot's translation); TB-5 and TB-6 (Lean 4 kernel and Creusot-as-verifier for the A1 claims, NG-4); TB-9 (Rust compiler); TB-1 (hash collision resistance); TB-2 (authentication scheme and Worker credentials); TB-10 (local `.dp-cache/` and `.why3find` directories for Result_Cache entries with the mark local); AS-12, AS-30.
- OQ-N37 is open: if a solver option can change the problem an Unpatched_Solver answers, the proved-standalone label of criterion 2 rests on the option list as well as on the solver.
- OQ-N41 is resolved by R3.5 criterion 2 and criterion 2 above: a Verdict recorded from the leaf Results of a partitioned Goal_Version is proof-labelled only certified; a whole-Goal_Version Result in the same Race is labelled under criteria 2 and 3.

### Requirement 31: R7.2 Assurance labels in proof.json and Creusot output

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As a Rust developer, I want each goal's Assurance_Label shown in `proof.json` and in Creusot's output, so that I know what each result rests on.

In this requirement, the *non-proof outcomes* are checked-incomplete, unchecked, Invalid, timeout, unknown, failed and unresolved. An Invalid outcome is *standalone Invalid* when it comes from a Result that is not a Lemma_Dependent_Result, and *dependent Invalid* otherwise. The *information order* is, most informative first: standalone Invalid, checked-incomplete, unchecked, dependent Invalid, timeout, unknown, failed, unresolved (OQ-N54 resolved). A *conflict* is the situation of R2.2 criterion 19: for one Goal_Version in one Session, the Verdict_Recorder assigns a proof label to one Result and accepts another Result with prover answer Invalid. The *Invalid field* of a Goal_Version's `proof.json` entry lists the accepted Results of that Goal_Version with prover answer Invalid (criterion 11).

#### Acceptance Criteria

1. THE Verdict_Recorder SHALL write the Assurance_Label of every Verdict to `proof.json`.
2. WHEN the Creusot_CLI reports proof results, THE Creusot_CLI SHALL display the Assurance_Label of each goal and the number of goals per Assurance_Label for the Session.
3. THE Verdict_Recorder SHALL encode every Verdict with a non-proof outcome in `proof.json` so that unmodified pinned why3find and Creusot read the goal as not proved (AS-19).
4. THE Verdict_Recorder SHALL compute every Assurance_Label that it records only from the inputs of R7.1 criterion 5, which do not include labels read from `proof.json`.
5. THE Verdict_Recorder SHALL select the most informative non-proof outcome (R1.5 criteria 2 and 9, R1.6 criteria 6 and 20, R2.2 criterion 14) as the first outcome in the information order that occurs among the Goal_Version's Job outcomes and the Assurance_Labels of its accepted Results, and as unresolved when none occurs (OQ-N54 resolved).
6. THE Verdict_Recorder SHALL map every prover answer other than Valid and Invalid to failed, unknown or timeout with why3find's existing classification (AS-31), and every Job failure report (R1.6 criterion 3) to failed.
7. WHEN a conflict occurs for a Goal_Version, THE Verdict_Recorder SHALL write in that Goal_Version's `proof.json` entry a conflict field, in the grammar of R7.10, that lists the proof-labelled Result and every Result with prover answer Invalid, each with its Result identifier, Assurance_Label or prover answer, Solver_Configuration name, solver binary hash, Worker identity or the mark local, and a flag stating whether it is a Lemma_Dependent_Result.
8. WHEN a conflict occurs for a Goal_Version, THE Verdict_Recorder SHALL keep that Goal_Version's Verdict and Assurance_Label unchanged (R1.5 criterion 8; OQ-N55).
9. WHEN the Creusot_CLI reports a goal whose `proof.json` entry has a conflict field, THE Creusot_CLI SHALL display the goal's Assurance_Label, the word conflict and the Result identifiers and Solver_Configuration names of the conflict field, and SHALL include the number of goals with a conflict in the Session summary.
10. IF a `proof.json` entry has no Assurance_Label or a label string outside the label set, THEN THE Replay_Tool SHALL report the goal as not proved with the reason unknown label.
11. WHEN the Verdict_Recorder accepts a Result with prover answer Invalid for a Goal_Version, THE Verdict_Recorder SHALL list that Result in the Invalid field of the Goal_Version's `proof.json` entry, in the grammar of R7.10, with its Result identifier, Solver_Configuration name, solver binary hash, Worker identity or the mark local, and a flag stating whether it is a Lemma_Dependent_Result, whatever the Goal_Version's Assurance_Label.
12. WHEN a Session ends with a conflict whose proof-labelled Result is labelled proved-standalone, THE Creusot_CLI SHALL return a non-zero exit status (draft for OQ-N55).

#### Properties

- **P7.2.1** (Round trip) For any Verdict v, with or without a conflict field and an Invalid field, `read(write(v)).label = v.label`, `read(write(v)).conflict = v.conflict` and `read(write(v)).invalid = v.invalid`.
- **P7.2.2** (Legacy reading) For any `proof.json` entry whose label is a non-proof outcome, unmodified pinned why3find reports the goal as not proved; and for any entry with a conflict field or an Invalid field, unmodified pinned why3find reads the entry as it reads the same entry without those fields.
- **P7.2.3** For any string s outside the label set, `decode_label(s)` is not a proof label.
- **P7.2.4** (Order) For any finite multiset O of non-proof outcomes, with every Invalid outcome tagged standalone or dependent, `most_informative(O)` is the minimum of O in the information order (standalone Invalid < checked-incomplete < unchecked < dependent Invalid < timeout < unknown < failed < unresolved), `most_informative(∅) = unresolved`, and `most_informative` is invariant under permutations of its input.
- **P7.2.5** (Conflict entry) For any Session trace with a conflict for Goal_Version G (P2.2.9), G's `proof.json` entry has a conflict field that lists exactly the proof-labelled Result and the Results of G with prover answer Invalid, G's Invalid field lists the same Results with prover answer Invalid, and G's Verdict label equals its label before the conflict.
- **P7.2.6** (Answer mapping) For any prover answer a ∉ {Valid, Invalid}, `classify(a) ∈ {failed, unknown, timeout}`, and `classify` agrees with why3find's classification on every answer of the pinned Why3.
- **P7.2.7** (No label input) For any two `proof.json` files that differ only in Assurance_Labels, a Session with equal other inputs records equal Verdicts.
- **P7.2.8** (Invalid field) For any Session trace and any Goal_Version G, the Invalid field of G's `proof.json` entry lists exactly the accepted Results of G with prover answer Invalid, each with the fields of criterion 11, whatever G's Assurance_Label, including a proof label.
- **P7.2.9** (Conflict exit status, draft for OQ-N55) For any Session that ends with a conflict whose proof-labelled Result is labelled proved-standalone, the Creusot_CLI exit status is non-zero.

#### Verification

- A1 — Creusot contracts on `decode_label` (returns a proof label only for the two proof label strings, P7.2.3) — `crates/dp-formats/src/proofjson.rs` — `ci/creusot`; Creusot contracts on `most_informative` (P7.2.4) — `crates/dp-core/src/labels.rs` — `ci/creusot`.
- A3 — the Replay_Tool re-derives every proof label from stored evidence (R7.6) — `crates/dp-replay/` — `ci/replay`.
- A4 — round-trip tests and fuzzing of the `proof.json` reader, including conflict fields and Invalid fields (P7.2.1) — `crates/dp-formats/tests/prop_proofjson.rs`, `fuzz/proofjson/` — `ci/test`, `ci/fuzz-smoke`; property-based tests of `most_informative` with standalone and dependent Invalid outcomes, `classify` and the label-input independence (P7.2.4, P7.2.6, P7.2.7) — `crates/dp-core/tests/prop_verdict.rs` — `ci/test`.
- A4 — differential test with unmodified pinned why3find, including entries with conflict and Invalid fields (P7.2.2) — `tests/diff/legacy_reader/` — `ci/diff`; conflict and Invalid-field reporting with a stub solver that answers Invalid, with and without a proof label for the goal (P7.2.5, P7.2.8) — `tests/integration/cluster/race.rs` — `ci/integration-cluster`; output snapshot test of the Creusot_CLI labels, summary, conflict display and the exit status after a conflict with a proved-standalone Result (criteria 2, 9 and 12, P7.2.9) — `tests/integration/cli_labels/` — `ci/integration-cluster`; unknown-label replay (criterion 10) — `tests/replay/tamper/` — `ci/replay`.

#### Trust

- AS-19 (legacy readers see non-proof entries as not proved and ignore the conflict and Invalid fields), AS-31 (why3find's answer classification). The information order affects only non-proof Verdicts. Whether a conflict should also downgrade the Verdict stays open (OQ-N55); criterion 12 is the draft for its exit-status part.

### Requirement 32: R7.3 Decision logic separated from I/O

**Priority:** Must

**Class:** Process

**User Story:** As a proof engineer, I want scheduling, cancellation, lemma admission and verdict recording in a pure library, so that the decision logic can be verified with Creusot, Kani and TLA+.

In this requirement, the *decision-function registry* is the file `tools/purity-check/registry.toml`, which names every decision function of criterion 1 with its dp-core module and the requirement it serves. Two placement decisions are recorded here: the composition plan `compose_plan` (R5.2) moves from `crates/dp-cert/src/plan.rs` to `crates/dp-core/src/compose_plan.rs`, so that every Creusot-verified decision function is under the purity check, while dp-cert keeps proof parsing, printing and the checker drivers; and the Lemma_Pool bound logic (R3.6) is in `crates/dp-core/src/pool.rs`, with dp-pool as its I/O shell.

#### Acceptance Criteria

1. THE System SHALL implement in the dp-core crate, in Rust (OQ-N2), the following decision functions: escalation and Result_Cache exclusion (R1.2); Worker assignment, eligibility and the no-eligible-Worker report (R1.1, R1.7); Calibration scaling and the Calibration filter (R1.4); Result acceptance `accept_result` (R1.3); Race formation, Hard_Goal classification, Job launch, Race termination and the invalid-answer policy (R2.2); cancellation, retry, heartbeat timeout and fallback (R1.5, R1.6); Lemma forwarding and delivery (R3.1, R4.2); Provenance_Record well-formedness and the default Dependency_Set (R3.2); Import hygiene and Lemma admission (R3.3, R3.4, R4.3); the Lemma export filter (R4.3 criterion 5) and the Instantiation_Hint export filter (R4.4 criterion 4); Instantiation_Hint admission and `instantiate` (R4.4); Lemma_Pool bounds, discard, first-wins and Session deletion (R3.6); the certification decisions of R5.1 in `crates/dp-core/src/certification.rs`; the composition plan `compose_plan` and the supported-fragment check `fragment_ok` (R5.2, R5.3 criterion 19); label assignment, `label_from_checker_output` and the most informative non-proof outcome (R5.3, R7.1, R7.2); Verdict recording (R1.5, R7.1); and Cache_Key computation (R7.5).
2. THE dp-core crate SHALL take every external input as a function argument and SHALL use no file, network, clock, randomness, thread, process or global mutable state interfaces.
3. WHEN the CI runs `ci/purity`, THE CI SHALL fail if dp-core depends on a crate outside the pure-dependency allow-list in `tools/purity-check/` or calls an interface that `tools/purity-check/` bans under criterion 2.
4. WHEN the CI runs `ci/purity`, THE CI SHALL fail if an entry of the decision-function registry for a requirement with scope status in scope (R7.11) names no public function of dp-core, or if a decision function of criterion 1 that serves a requirement with scope status in scope has no registry entry.
5. WHEN the CI runs `sched/mutants-weekly`, THE CI SHALL fail if a mutant of a registered decision function survives both the dp-core tests and the simulation tests in `tests/sim/` (R7.13).

#### Properties

- **P7.3.1** (Determinism) For any registered decision function f and inputs x, two calls `f(x)` return equal outputs, including in different processes and with different hash-map iteration seeds.
- **P7.3.2** (Shells call dp-core) For any mutant of a registered decision function, at least one test in `tests/sim/` or `crates/dp-core/tests/` fails.
- **P7.3.3** (Purity) The dependency closure of dp-core is contained in the pure-dependency allow-list, and no function of dp-core calls a banned interface.
- **P7.3.4** (Registry) The set of decision functions of criterion 1 that serve requirements with scope status in scope equals the set of registry entries for those requirements, and every such entry resolves to a public function of dp-core.

#### Verification

- A4 — dependency, banned-interface and registry checks, the registry check reading the scope statuses of `docs/traceability.toml` (P7.3.3, P7.3.4) — `tools/purity-check/` — `ci/purity`.
- A1 — Creusot verification of dp-core — `crates/dp-core/` — `ci/creusot`.
- A2 — Kani harnesses on dp-core — `verification/kani/` — `ci/kani`.
- A4 — determinism tests (P7.3.1) — `crates/dp-core/tests/` — `ci/test`; mutation testing (P7.3.2) — `tools/mutants/` — `sched/mutants-weekly`.

#### Trust

- AS-13 (Creusot and Kani can verify dp-core); TB-6 and TB-7 (Creusot and Kani as checking tools). The Canonicalizer (`crates/dp-canon/`) stays outside dp-core: it computes names rather than decisions, and soundness does not depend on it (R4.1 Trust); its determinism is checked by P4.1.6.

### Requirement 33: R7.4 Canaries

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As an auditor, I want known-invalid goals and deliberately false Lemmas run in CI and in the evaluation, so that any unsoundness shows up as a failing job.

In this requirement:

- The *canary modes* are, each with the requirement that introduces it: local (R7.1), distributed (R1.1), portfolio (R2.2), within-goal sharing (R3.1), cross-goal sharing (R4.2), hint sharing (R4.4), partitioning (R3.5), certify-all (R5.5), and warm Result_Cache (R7.5). The *required canary modes* are the canary modes whose requirement has scope status in scope in `docs/traceability.toml` (R7.11). The *canary-mode list* is `tests/canaries/modes.toml`, which names each canary mode that the canary harness can run. Only the list check of criterion 17 depends on scope status; criteria 5 and 6 apply to every canary mode that the commit under test implements or can enable, whatever the scope status of its requirement.
- The *Lemma canaries* are Injected_False_Lemmas (each with a recorded Dependency_Set Δ, recorded used Lemmas U and a confirmed model of Δ ∪ U ∪ {¬L} for each target Goal_Version, Glossary) of these kinds: Lemmas that follow from no Dependency_Set; Lemmas with understated Dependency_Sets (R3.2); Lemmas with forged Partition_Assumptions (R3.4); cross-goal Lemmas that depend on the source's Negated_Goal_Assertion with the flag cleared (R4.2); forged-name Lemmas, which reuse a sibling's Canonical_Name for a different declaration (R4.1); and Lemmas that hold only for a Skolem symbol renamed to a Canonical_Name (R4.3).
- The *hint canaries* are malformed Instantiation_Hints, each with the reason category that R4.4 criterion 9 must log: unknown Hypothesis (missing dependency); a Hypothesis that is not universally quantified, a Negated_Goal_Assertion name, a Partition_Assumption name, wrong arity or wrong sort (hint shape); foreign or Skolem symbols (hygiene); oversized, tampered field or forged identifier (malformed); tag mismatch (authentication).
- The *certificate canaries* are Composed_Certificates with forged sub-proofs (invalid steps, open assumptions outside the Dependency_Set, and a Trust_Step deriving the Lemma, once in each Checkable_Proof_Format), wrong universal-instantiation steps (wrong term, swapped variables, wrong sort, wrong Hypothesis), goal-swapped certificates (a valid certificate for another Goal_Version), single-byte mutations, a recorded input problem that differs from `print_local(G)`, one certificate per banned command of R5.2 (for Ethos, each of `declare-rule`, `program`, `declare-oracle-fun`, `include` and every other signature-extending command of the pinned Eunoia language; for Alethe, a step whose rule is outside the pinned Alethe rule list), and an Alethe certificate with a `hole` step (R5.3 criterion 15).
- The *cache canaries* are Result_Cache entries that claim a proof for a Canary_Goal: with a forged Task_Hash, a forged label, an import set removed, or a Certificate_Store reference to another goal's certificate.
- The *pipeline canaries* are Results for Canary_Goals, authenticated by a test Worker and recorded as submitted by the test harness, whose Solver_Configuration has a Transformation_Pipeline with a splitting transformation that turns the Canary_Goal into several tasks, the first of which is valid, and whose Task_Hash is that of the first task. R2.1 criterion 4 rejects such Solver_Configurations in configuration files, so the test harness bypasses that check and only R1.3 criterion 10 stops these Results.

#### Acceptance Criteria

1. THE Canary_Suite SHALL contain Canary_Goals produced by mutating Creusot specifications (postconditions, preconditions, loop invariants, type invariants) and hand-written invalid goals.
2. THE Canary_Suite SHALL record for each Canary_Goal an invalidity witness: a model that assigns a value or definition to every declared symbol of the Canary_Goal's Printed_Goal, confirmed by an evaluator independent of the solver that produced the model to satisfy every Assertion, including the Negated_Goal_Assertion, given for a Canary_Goal with a Hypothesis that contains a quantifier as definitions (lambda terms for functions) together with a CPC proof, checked by the pinned Ethos, that each Assertion with the model substituted is valid, and given as a finite evaluation of every Assertion only when every quantified variable ranges over an uninterpreted sort that the model interprets as a finite set (AS-18, OQ-N16).
3. IF no invalidity witness is confirmed for a mutated specification, THEN THE Canary_Suite SHALL leave the mutant out of the Canary_Goals and SHALL record the mutant in the canary exclusion list with the reason no witness.
4. THE Canary_Suite SHALL contain Lemma canaries of every kind for Canary_Goals and for valid goals, hint canaries of every kind, certificate canaries of every kind, cache canaries of every kind, and pipeline canaries.
5. WHEN the CI runs `ci/canaries`, THE CI SHALL run every Canary_Goal in every canary mode that the commit under test implements, with `<TBD-40>` seeds, with the Lemma canaries and hint canaries delivered through the Lemma_Pool, with the cache canaries in the Result_Cache, with the pipeline canaries delivered as Results of the test Worker, and with an adversarial Coordinator schedule.
6. IF the Front_End of the commit under test accepts a configuration that enables a canary mode and `ci/canaries` did not run that mode, THEN THE CI SHALL fail `ci/canaries` and SHALL name the mode.
7. IF any Canary_Goal receives a proof-labelled Verdict, THEN THE CI SHALL fail the job and SHALL report the goal, mode, seed and Result.
8. IF a goal, valid or not, receives the label certified from a Composed_Certificate that contains a sub-proof of a Lemma canary L whose open assumptions are within L's recorded Δ ∪ U, THEN THE CI SHALL fail the job and SHALL report the goal, the Lemma canary, mode and seed.
9. IF a Result with a certificate canary as its Composed_Certificate receives the label certified, THEN THE CI SHALL fail the job and SHALL report the certificate canary.
10. IF the Admission_Checker, at a Worker or in the Front_End re-admission of R4.4 criterion 15, admits a hint canary or logs a reason category other than the one recorded for that hint canary, THEN THE CI SHALL fail the job and SHALL report the hint canary and the logged reason category.
11. WHEN the CI runs `sched/canaries-full`, THE CI SHALL run the Canary_Suite generated from the full evaluation corpus under criteria 5 to 10.
12. WHEN the Experiment_Runner runs the Canary_Suite in an evaluation run (R6.3 criterion 6), THE Experiment_Runner SHALL use the Canary_Suite version generated for that run's corpus release and the Lemma canaries, hint canaries, certificate canaries and cache canaries of criterion 4.
13. IF, during an evaluation run, a goal receives the label certified from a Composed_Certificate that contains a sub-proof of a Lemma canary L whose open assumptions are within L's recorded Δ ∪ U, or a Result with a certificate canary receives the label certified, THEN THE Experiment_Runner SHALL fail the evaluation job and SHALL set the status failed in the run manifest (R6.3 criteria 7 and 8).
14. IF no model of Δ ∪ U ∪ {¬L} is confirmed under criterion 2 for a Lemma canary L and a target Goal_Version, THEN THE Canary_Suite SHALL leave that pair out of the Lemma canaries and SHALL record the pair in the canary exclusion list with the reason no witness.
15. WHEN the CI runs `ci/canaries`, THE CI SHALL fail the job if fewer than `<TBD-44>` Canary_Goals with a Hypothesis that contains a quantifier run in some canary mode that the job runs, and SHALL name that mode.
16. WHEN the CI runs `ci/canaries`, THE CI SHALL report, for each canary mode, the number of Canary_Goals with a Hypothesis that contains a quantifier that ran, and the share of mutated specifications that criterion 3 excluded, overall and among mutants with a quantified Hypothesis.
17. WHEN the CI runs `ci/canaries`, THE CI SHALL fail if the canary-mode list lacks a required canary mode, and SHALL name that mode.

#### Properties

- **P7.4.1** For any Canary_Goal C, canary mode m, Lemma canary set I, adversarial Coordinator schedule and seed s, `verdict(C, m, I, s)` is not proof-labelled, in `ci/canaries`, `sched/canaries-full` and every evaluation run.
- **P7.4.2** For any goal G and any Lemma canary L with a confirmed model of Δ ∪ U ∪ {¬L} for G, if `verdict(G)` is certified then the Composed_Certificate of G contains no sub-proof of L whose open assumptions are within Δ ∪ U.
- **P7.4.3** (Witness) For any Canary_Goal C, C's invalidity witness re-validates in every run; for any Lemma canary L and target G, L's model of Δ ∪ U ∪ {¬L} re-validates in every run; every mutant without a confirmed witness is in the canary exclusion list and is not a Canary_Goal; and every pair of Lemma canary and target without a confirmed model is in the canary exclusion list and is not delivered.
- **P7.4.4** (Certificate canaries) For any certificate canary K and any Result r with K as its Composed_Certificate, `label(r) ≠ certified`.
- **P7.4.5** (Hint canaries) For any hint canary h with recorded reason category c and any target Job t, `admit_hint(h, t) = false` with logged reason c, at the Worker and at Front_End re-admission.
- **P7.4.6** (Mode coverage) The set of canary modes run by `ci/canaries` contains every canary mode that the Front_End's configuration of the commit under test can enable.
- **P7.4.7** (Evaluation alignment) For any evaluation run R, the Canary_Suite version in R's manifest equals the version generated for R's corpus release, and if P7.4.1, P7.4.2 or P7.4.4 fails in R, R's evaluation job fails and R's status is failed.
- **P7.4.8** (Quantified coverage) For any `ci/canaries` run and any canary mode m that it runs, at least `<TBD-44>` Canary_Goals with a Hypothesis that contains a quantifier run in m, and the run report states the share of mutated specifications excluded under criterion 3.
- **P7.4.9** (Witness forms) For any Canary_Goal whose witness is a finite evaluation, every quantified variable of its Printed_Goal ranges over an uninterpreted sort that the model interprets as a finite set; and for every other Canary_Goal with a quantified Hypothesis, the witness consists of definitions and CPC proofs that the pinned Ethos accepts, one per Assertion with the model substituted.
- **P7.4.10** (Mode list) For any commit, the canary-mode list contains every canary mode whose requirement has scope status in scope in that commit's `docs/traceability.toml`.

#### Verification

- A4 — canary runs, including the pipeline canaries, the banned-command and Alethe-hole certificate canaries, and the quantified-coverage gate and report (P7.4.1, P7.4.2, P7.4.4–P7.4.6, P7.4.8) — `tests/canaries/` — `ci/canaries`, `sched/canaries-full`.
- A3 — per-run re-validation of every invalidity witness and of every Lemma-canary model, by finite evaluation or by the pinned Ethos on the CPC proofs of the substituted Assertions (P7.4.3, P7.4.9) — `tests/canaries/witness.rs` — `ci/canaries`.
- A4 — goal-swapped and single-byte-mutated certificates (P7.4.4) — `tests/certs/tamper/` — `ci/certs`.
- A4 — evaluation canary gate, including the false-Lemma and certificate-canary conditions of criterion 13 (P7.4.7) — `eval/tests/canary_gate/` — `ci/eval-smoke`.
- A4 — canary-mode list check against the scope statuses of `docs/traceability.toml` (criterion 17, P7.4.10) — `tests/canaries/modes.toml` — `ci/canaries`.
- A4 — mutation testing: mutants that weaken a soundness check in dp-core, dp-cert or dp-frontend must be killed by the canaries or other tests (R7.13) — `tools/mutants/` — `sched/mutants-weekly`.

#### Trust

- AS-18 (witnesses and Lemma-canary models can be produced and confirmed mechanically); TB-3 (Ethos, for the CPC proofs of witnesses in the definitions form). A passing canary run shows that the listed attacks fail; it does not show the absence of other unsoundness, which R7.1 covers.

### Requirement 34: R7.5 Cache correctness

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As a proof engineer, I want a cache hit only when nothing that can change the result differs, so that stale or foreign results are never reused.

In this requirement:

- The *lookup fields* of a Job or of a Result_Cache lookup are the fields of criterion 1, all known before the run. The *run description* is the tuple of the lookup fields, and the Cache_Key is the hash of the injective encoding of the run description.
- The *validation fields* of an entry are the solver binary hash, the Calibration identifier and value, the local time limit, the import-set hash (R1.3 criterion 1) and the Worker identity or the mark local. The Result_Cache stores them with the entry, and the Verdict_Recorder checks them after lookup (criterion 9) with the existing rules of R1.4 criterion 7, criterion 4 and R7.1 criterion 6.
- A run description *may need certification* when it has the import mark or names a prover and version for which the Lock_File lists no Unpatched_Solver.
- The certify-all setting is neither a lookup field nor a validation field: R7.1 criterion 6 labels every entry with the setting of the current Session.
- The *legacy layout* is why3find's own cache layout inside `.why3find`, which unmodified pinned why3find reads.
- The Result_Cache directory is `.dp-cache/`, next to `.why3find` in the project directory. The Front_End writes a proved-standalone entry to the legacy layout by answering why3find's cache-write request in dispatch mode, and answers every other cache-write request by skipping it.

#### Acceptance Criteria

1. THE Result_Cache SHALL compute the Cache_Key from these lookup fields: the why3find goal path; the Task_Hash, which covers the Printed_Goal and any Partition_Assumptions (R3.5 criterion 4); the Driver hash; the Transformation_Pipeline hash; the prover name and prover version; the option list and seed; the reference time limit; the import mark; and, for a run description that may need certification, the name, version, binary hash and option list of the pinned Certificate_Checker of every Checkable_Proof_Format (R5.3 criterion 2).
2. THE Result_Cache SHALL use an injective encoding of the lookup fields before hashing.
3. WHEN the Result_Cache stores an entry, THE Result_Cache SHALL store with the entry the lookup fields, the validation fields, the Result exactly as accepted (including its authentication tag and Worker identity, or the mark local), and, for a Result labelled certified or checked-incomplete, the Certificate_Store content hash of its Composed_Certificate (R5.4).
4. WHEN the Verdict_Recorder reads a Result_Cache entry whose Result carries a Worker identity, THE Verdict_Recorder SHALL apply R1.3 criteria 3, 4 and 7 to the stored Result, with the registered and revoked Worker credentials known to the Front_End in the current Session, before labelling the entry under R7.1 criterion 6 (OQ-N33).
5. IF a cache entry fails to parse under R7.10, or its stored lookup fields differ from the lookup fields of the lookup, THEN THE Result_Cache SHALL treat the lookup as a miss.
6. IF the check of criterion 4 discards a cached Result, THEN THE Verdict_Recorder SHALL treat the lookup as a miss and SHALL log the reason.
7. THE Result_Cache SHALL write into the legacy layout only entries whose Result the Verdict_Recorder labelled proved-standalone, and SHALL store every other entry in `.dp-cache/`, a location under the project directory that unmodified pinned why3find does not read (AS-35).
8. WHEN the CI runs `ci/canaries`, THE CI SHALL run unmodified pinned why3find on the project directory after each canary Session and SHALL fail the job if that why3find reports any Canary_Goal as proved.
9. WHEN a lookup finds an entry whose stored lookup fields equal the lookup fields of the lookup, THE Verdict_Recorder SHALL check the entry's validation fields before labelling the entry under R7.1 criterion 6: the solver binary hash against the current Lock_File (criterion 10), the local time limit against the Calibration scaling of the stored Calibration value and reference time limit (R1.4 criteria 6 and 7), the import-set hash against the stored import list (R1.3 criterion 7), and the Worker identity under criterion 4.
10. IF the solver binary hash stored with a Result_Cache entry, including an entry with the mark local, is not listed in the current Lock_File for the entry's prover and version, THEN THE Verdict_Recorder SHALL treat the lookup as a miss and SHALL log the reason binary mismatch.
11. IF a check of criterion 9 other than that of criterion 10 fails, THEN THE Verdict_Recorder SHALL treat the lookup as a miss and SHALL log the failing validation field.

#### Properties

- **P7.5.1** (Key completeness) For any two run descriptions d1, d2 that differ in any lookup field of criterion 1, `encode(d1) ≠ encode(d2)`.
- **P7.5.2** (Determinism) For any run description d, `key(d)` is independent of field order, map iteration order, process and machine.
- **P7.5.3** (Import mark and import set) For any run description d, `key(d with the import mark) ≠ key(d without the import mark)`; and for any entry e whose stored import-set hash differs from the hash of e's stored import list, a lookup that finds e is a miss.
- **P7.5.4** (Differential) For any Session on the CI corpus, run with a cold cache and with a cache warmed by runs that differ from the Session in one lookup field or one validation field at a time: no Goal_Version is proof-labelled in one run and has a Result with prover answer Invalid in the other.
- **P7.5.5** (No sharing) For any sequence of store operations and any run descriptions d1, d2 that differ in a lookup field, a lookup for d2 never returns an entry stored for d1, also when `key(d1) = key(d2)` under a deliberately weak test hash, because criterion 5 compares the stored lookup fields.
- **P7.5.6** (Checker identity) For any run description d that may need certification, changing the version, binary hash or option list of a pinned Certificate_Checker changes `key(d)`.
- **P7.5.7** (Remote entries) For any cached Result that fails a check of R1.3 criterion 3, 4 or 7 in the current Session, the lookup is a miss and no Verdict is recorded from the entry.
- **P7.5.8** (Validation) For any entry e and any lookup with equal lookup fields: if e's stored solver binary hash is not listed in the current Lock_File for e's prover and version (for e with the mark local as for every other e), or e's local time limit is not the Calibration scaling of e's Calibration value and reference time limit, or e's import-set hash differs from the hash of e's import list, or e fails criterion 4, then the lookup is a miss and no Verdict is recorded from e.
- **P7.5.9** (Certify-all independence) For any run description d, `key(d)` is the same with certify-all mode on and off, and the label that R7.1 criterion 6 gives an entry uses the certify-all setting of the current Session.
- **P7.5.10** (Legacy isolation) For any Result_Cache state written by the System, every entry in the legacy layout has a Result labelled proved-standalone; and after any canary Session, unmodified pinned why3find run on the project directory reports no Canary_Goal as proved.

#### Verification

- A1 — Creusot contracts on the injectivity of `encode_key`, on the stored-field comparison of the lookup and on the validation-field checks (P7.5.1, P7.5.5, P7.5.8, P7.5.9) — `crates/dp-core/src/cache_key.rs` — `ci/creusot`.
- A2 — Kani bounded check of `encode_key` injectivity — `verification/kani/cache_key.rs` — `ci/kani`.
- A3 — every cache hit is re-labelled under R7.1 criterion 6, with the Task_Hash recomputed, the validation fields checked against the current Lock_File and Calibration rule, and remote entries re-checked under R1.3 — `crates/dp-frontend/src/accept.rs` — `ci/sim`; certified hits are re-checked by the Certificate_Checker — `crates/dp-cert/src/check.rs` — `ci/certs`.
- A4 — property-based field-mutation and weak-hash tests over lookup and validation fields, including local entries with a stale solver binary hash and lookups under both certify-all settings (P7.5.1–P7.5.3, P7.5.5–P7.5.9) — `crates/dp-core/tests/prop_cache_key.rs` — `ci/test`; differential test, cache on vs. off (P7.5.4) — `tests/diff/cache_on_off/` — `ci/diff`; legacy-reader test in which unmodified pinned why3find reads a project whose Result_Cache holds entries of every Assurance_Label and reports as proved only goals with proved-standalone entries (P7.5.10) — `tests/diff/legacy_reader/` — `ci/diff`; cache canaries (R7.4) and the run of unmodified pinned why3find after each canary Session (criterion 8, P7.5.10) — `tests/canaries/` — `ci/canaries`; mutation testing of `encode_key` (R7.13) — `tools/mutants/` — `sched/mutants-weekly`.

#### Trust

- TB-10 (integrity of the local `.dp-cache/` and `.why3find` directories for entries with the mark local). Hash collisions cannot produce a wrong hit (criterion 5); collision resistance (TB-1) still matters for the Task_Hash (R1.3). AS-35 (a location exists that unmodified pinned why3find does not read); if it fails, only proved-standalone entries are cached, so legacy readers never see another label.

### Requirement 35: R7.6 Offline replay

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As an auditor, I want to re-check distributed and shared-lemma results in `proof.json` on my own machine without the Cluster.

In this requirement, a replay *confirms* a Goal_Version when every check below that applies to its Verdict passes. The *replay outcome* of a proof-labelled Verdict is exactly one of:

- confirmed;
- timed-out: the re-run of criterion 5 reached its time limit;
- unknown: the re-run of criterion 5 ended with an answer other than a proof, Invalid or timeout;
- contradicted: the re-run of criterion 5 answered Invalid;
- binary-mismatch: the local solver binary hash is not listed as an Unpatched_Solver (criterion 5), or a Certificate_Checker binary hash differs from its Lock_File entry (criterion 4, R5.3 criterion 7);
- hash-mismatch: a Task_Hash comparison of criterion 2 or 3, or a content hash comparison of R5.4 criterion 8, fails;
- failed with a reason: every other failure, including a missing entry (R5.4 criterion 9), the marker absent (R2.3 criterion 8), a non-empty recorded import set for a proved-standalone Verdict, a checker outcome other than accept or a failing binding check (criterion 4), and an unknown label (R7.2 criterion 10).

#### Acceptance Criteria

1. WHEN the auditor runs the Replay_Tool on a project, THE Replay_Tool SHALL re-check every proof-labelled Verdict in `proof.json` using only the project files, `proof.json`, the Certificate_Store and the Pinned_Dependencies of the local Lock_File, with no network connection and no connection to the Cluster.
2. WHEN the Replay_Tool replays a Verdict, THE Replay_Tool SHALL re-print the Goal_Version locally with the Driver and Transformation_Pipeline recorded in `proof.json` (R2.3) and SHALL compare the Task_Hash of the re-printed Printed_Goal with the Task_Hash recorded in `proof.json`.
3. WHEN the Replay_Tool replays a certified Verdict, THE Replay_Tool SHALL apply the manifest checks of R5.4 criteria 8 and 9 to the referenced Certificate_Store entry and SHALL compare the Task_Hash of the Printed_Goal stored in that entry with the Task_Hash of the re-printed Printed_Goal.
4. WHEN the Replay_Tool replays a certified Verdict, THE Replay_Tool SHALL run the Certificate_Checker on the stored Composed_Certificate under R5.3 criteria 1 to 3 and 16, with the re-printed Printed_Goal as the input problem, and SHALL confirm the Verdict only if the checker outcome is accept and none of the binding checks of R5.3 (criteria 7, 8, 9, 13, 14, 15 and 19) fails, so that R5.3 criterion 4 would mark the Result checker-accepted.
5. WHEN the Replay_Tool replays a proved-standalone Verdict, THE Replay_Tool SHALL run why3find's Calibration on the local machine for the recorded prover and record the local Calibration identifier and value in the replay report, SHALL re-run the recorded Solver_Configuration with its recorded seed and option list on the re-printed Printed_Goal, with the recorded reference time limit scaled by the local Calibration and multiplied by the replay factor `<TBD-20>` (at least 1), and SHALL confirm the Verdict only if the local solver binary hash is listed in the Lock_File as an Unpatched_Solver, the recorded import-set hash is the hash of the empty set, and the solver reports a proof.
6. IF the Lock_File hash recorded in `proof.json` (R7.7 criterion 5) differs from the hash of the local Lock_File, THEN THE Replay_Tool SHALL report the mismatch at the start of the replay report and SHALL replay with the local Lock_File.
7. WHEN replay finishes, THE Replay_Tool SHALL report one entry per Goal_Version of `proof.json`: the replay outcome of each proof-labelled Verdict, and the recorded outcome with the mark not replayed for each other Verdict.
8. WHEN replay finishes, THE Replay_Tool SHALL exit with status zero only if every proof-labelled Verdict is confirmed.
9. IF the replay outcome of some proof-labelled Verdict is contradicted, THEN THE Replay_Tool SHALL exit with the exit status reserved for contradicted, which differs from every other exit status of the Replay_Tool.

#### Properties

- **P7.6.1** (Completeness) For any `proof.json` and Certificate_Store produced by a Session of `ci/integration-cluster`, replay with the same Lock_File confirms every certified Verdict.
- **P7.6.2** (Tamper) For any tampering of `proof.json` or the Certificate_Store (label upgrade, Task_Hash swap, content hash swap, a changed byte in any entry file, a changed manifest field, a swapped stored Printed_Goal, a changed checker binary hash, or a consistent rewrite of an entry, its manifest and its content hash in `proof.json` whose stored Printed_Goal differs from the re-printed Printed_Goal or whose Composed_Certificate the pinned Certificate_Checker does not accept against it), the Replay_Tool does not confirm any affected goal.
- **P7.6.3** For any replay run, the Replay_Tool makes no network connection.
- **P7.6.4** (Replay rule) For any certified Verdict v of Goal_Version G, replay confirms v exactly when the manifest checks pass, `H(reprint(G)) = v.task_hash = H(entry.printed_goal)`, and the checker outcome on the stored certificate against `reprint(G)` is accept with `bind` of P5.3.1 holding for `reprint(G)` as input problem.
- **P7.6.5** (Report) For any replay run, the report has exactly one entry per Goal_Version of `proof.json`, the exit status is zero exactly when every proof-labelled entry is confirmed, and the exit status is the one reserved for contradicted exactly when some entry is contradicted.
- **P7.6.6** (Standalone replay) For any proved-standalone Verdict v, replay confirms v only if the local solver binary hash is in `unpatched(Lock_File)` for v's prover and version, `v.imports_hash = H(canon(∅))` and the re-run reports a proof.
- **P7.6.7** (Replay outcomes) For any replay run and any proved-standalone Verdict v, the replay outcome of v is contradicted if the re-run answers Invalid, timed-out if the re-run reaches its time limit, unknown if it ends with another non-proof answer, and binary-mismatch if the local solver binary hash is not in `unpatched(Lock_File)`; and every proof-labelled entry has exactly one replay outcome.
- **P7.6.8** (Replay inputs) For any proved-standalone Verdict v replayed on a machine with local Calibration c, the re-run uses v's recorded seed and option list and the time limit `scale(c, v.reference_limit) × <TBD-20>` with `<TBD-20> ≥ 1`, and the replay report records c.

#### Verification

- A3 — the Replay_Tool as an independent per-result check, run on Sessions from `ci/integration-cluster` inside a container without network (P7.6.1, P7.6.3) — `crates/dp-replay/` — `ci/replay`.
- A4 — tamper, missing-entry and Lock_File-mismatch tests, including consistent rewrites of an entry, its manifest and its `proof.json` content hash (P7.6.2, P7.6.4–P7.6.6) — `tests/replay/tamper/` — `ci/replay`; replay of integration Sessions whose proved-standalone Verdicts come from stub solvers that answer with a proof, Invalid or unknown or that run past the time limit, checking each replay outcome, the recorded Calibration, seed and options, and the exit statuses (P7.6.5–P7.6.8) — `tests/replay/` — `ci/replay`; mutation testing of `crates/dp-replay/` (R7.13) — `tools/mutants/` — `sched/mutants-weekly`.
- A5 — replay agreement of proved-standalone Verdicts with real solvers, measured within RQ6 (R6.2 criterion 16 defines the metric) — `eval/scripts/rq6_instability/` — `sched/eval`.

#### Trust

- The pinned local solvers (NG-3) and checkers (TB-3, TB-4, TB-11); AS-17 and AS-20 for proved-standalone replay; AS-30 for the binding check of R5.3 criterion 9. With real solvers, a proved-standalone replay can end timed-out or unknown although the Verdict is right; RQ6 measures how often.

### Requirement 36: R7.7 Pinned dependencies and reproducible builds

**Priority:** Must

**Class:** Process

**User Story:** As an auditor, I want every tool pinned and every build reproducible, so that results can be traced to exact binaries.

In this requirement, the *local executables* of a Session are the solver, Certificate_Checker, Why3 and why3find executables that the Session's configuration can start on the local machine. A binary hash mismatch found at Session start aborts the Session (criterion 3); a mismatch found later affects only the Result concerned (criteria 4 and 7, R5.3 criterion 7).

#### Acceptance Criteria

1. THE Lock_File SHALL pin the version and content hash of Creusot, Why3, why3find, Alt-Ergo, Z3, cvc5, every Sharing_Solver build, Ethos, the CPC signature files that Ethos loads, Carcara, the Alethe rule list of the supported fragment (R5.2), the Lean 4 toolchain and Lean libraries, the TLA+ tools (TLC, Apalache), Kani, cargo-mutants, the Rust toolchain, the OCaml toolchain, and the Rust and OCaml library dependencies of the System.
2. WHEN the CI runs `ci/repro-build`, THE CI SHALL build the System twice in independent clean environments and SHALL fail if the hash of any binary or library that the build produces differs between the two builds.
3. IF at Session start the binary hash of a local executable differs from its Lock_File entry, or a local executable has no Lock_File entry, THEN THE Front_End SHALL report the executable and both hashes, SHALL stop before starting any solver process, SHALL return a non-zero exit status to the Creusot_CLI, and SHALL leave `proof.json` and the `.why3find` cache unchanged.
4. IF, after Session start, the binary hash of a Certificate_Checker that the Front_End is about to run differs from its Lock_File entry, THEN THE Verdict_Recorder SHALL label the Result that the run would check unchecked and SHALL log the mismatch (R5.3 criterion 7).
5. THE Verdict_Recorder SHALL record the Lock_File hash in every `proof.json` it writes.
6. WHEN the CI runs `ci/repro-build`, THE CI SHALL fail if a Lock_File entry lacks a version or content hash, or if a tool of criterion 1 has no Lock_File entry.
7. IF, after Session start, the binary hash of a local solver that the Front_End is about to start differs from its Lock_File entry, THEN THE Verdict_Recorder SHALL treat every Result of that solver run as a Lemma_Dependent_Result (R1.3 criterion 5) and SHALL log the mismatch.
8. WHEN the CI runs `ci/patches`, THE CI SHALL fail if the Lock_File lists as an Unpatched_Solver a solver binary built with any patch under `patches/` (moved from R7.9).
9. WHEN the CI runs `ci/repro-build`, THE CI SHALL fail unless, for each Lock_File entry of an Unpatched_Solver or a Certificate_Checker and for each platform that the entry lists, the content hash either equals the checksum that the upstream project publishes for the recorded release artifact, or equals the hash of each of two builds of the recorded upstream tag in the pinned hermetic build environment.

#### Properties

- **P7.7.1** For any source tree and Lock_File, two clean builds produce identical artifact hashes.
- **P7.7.2** (Session start) For any Session in which some local executable has a runtime hash different from its Lock_File entry or has no entry, the Session starts no solver process, ends with a non-zero exit status, and leaves `proof.json` and the `.why3find` cache byte-identical.
- **P7.7.3** (Later mismatch) For any solver run started after Session start whose binary hash differs from the Lock_File, every Result of that run is a Lemma_Dependent_Result; and for any Certificate_Checker run whose binary hash differs from the Lock_File, the Result it checked is labelled unchecked.
- **P7.7.4** For any `proof.json` written by a Session, the recorded Lock_File hash equals the hash of the Lock_File the Session used.
- **P7.7.5** (No patched Unpatched_Solver) No Lock_File entry marked Unpatched_Solver is the build of a source with a patch under `patches/` applied.
- **P7.7.6** (Upstream match) For any Lock_File entry e of an Unpatched_Solver or a Certificate_Checker and any platform p of e, `e.hash(p)` equals the upstream-published checksum of e's release artifact for p, or `e.hash(p) = H(build₁(tag(e), p)) = H(build₂(tag(e), p))` for two builds of e's upstream tag in the pinned hermetic build environment.

#### Verification

- A4 — reproducible build and Lock_File schema check (P7.7.1, criterion 6) — `deps/` — `ci/repro-build`; upstream checksum or double-rebuild check of every Unpatched_Solver and Certificate_Checker entry per platform (criterion 9, P7.7.6) — `deps/` — `ci/repro-build`.
- A4 — Unpatched_Solver mark check against the patch set (criterion 8, P7.7.5) — `tools/patch-check/` — `ci/patches`.
- A4 — integration test with a substituted solver binary at Session start and during a Session (P7.7.2, P7.7.3) — `tests/integration/pins/` — `ci/integration-cluster`; substituted checker binary (P7.7.3) — `tests/integration/certs/` — `ci/certs`; property-based test of the label rule for binary mismatches and of the recorded Lock_File hash (P7.7.3, P7.7.4) — `crates/dp-core/tests/prop_verdict.rs` — `ci/test`.

#### Trust

- TB-16 (bootstrap binaries of the pinning mechanism, Nix; OQ-N10 resolved by design decision D-8); TB-17 (integrity of the local file system between the hash check and the start of an executable); TB-22 (the Lock_File Unpatched_Solver marks of release artifacts that criterion 9 accepts by published checksum, beyond what criteria 8 and 9 check).
- The pinned hermetic build environment of criteria 2 and 9 is a Nix flake with a pinned nixpkgs, with the opam packages built inside Nix and the OCI images of the integration cluster built by Nix (design decision D-8).

### Requirement 37: R7.8 Trusted computing base document

**Priority:** Must

**Class:** Process

**User Story:** As an auditor, I want one document listing exactly what is trusted for each Assurance_Label, so that I can assess the trust story.

In this requirement, a *trusted item* is an entry of `docs/tcb.toml`: a component, Assumption, Non-goal, Fairness assumption, or cryptographic or platform assumption. Every item of the Trusted items list (TB-1 to TB-23) is a trusted item under its TB-n ID.

#### Acceptance Criteria

1. THE TCB_Document SHALL list, for each Assurance_Label, every trusted item whose failure could make a Verdict with that label wrong, with the version and hash from the Lock_File for every Pinned_Dependency among them.
2. THE TCB_Document SHALL list the Fairness assumptions F1–F4, every item of the Trusted items list (TB-1 to TB-23), every Non-goal that is trusted instead of verified, and every Assumption marked To check, each Assumption with either the Assurance_Labels whose correctness relies on it or the mark usefulness only (for AS-25 to AS-30 and AS-33 to AS-40: AS-30 for certified; AS-35 for proved-standalone as unmodified why3find reads the Result_Cache, R7.5 criterion 7; AS-25, AS-26 (not relied on), AS-27, AS-28, AS-29, AS-33, AS-34, AS-36, AS-37, AS-38, AS-39 and AS-40 usefulness only, R1.1, R1.3, R1.4, R3.3, R4.1 to R4.4, R5.1 to R5.3, R6.2, R7.9 and R7.12 Trust).
3. THE TCB_Document SHALL contain the checker-output table that maps each Certificate_Checker's exit status and output to a checker outcome (R5.3), the outcome-to-label table (accept → certified; accept with Trust_Steps → checked-incomplete; every other outcome → unchecked), and the information order of R7.2.
4. THE TCB_Document SHALL record, for each Checkable_Proof_Format, the Certificate_Checker name, its Lock_File version and binary hash, the exact option list that R5.3 criterion 2 uses with the signature files it names, the banned-option list of R5.3 criterion 16, and whether the Carcara part of AS-6 is confirmed (R5.3 criterion 15).
5. THE TCB_Document SHALL list as trusted for proved-standalone the Solver_Configuration option lists passed to Unpatched_Solvers (OQ-N37), and as trusted for every label that uses Worker authentication the authentication scheme and the secrecy of Worker credentials (R7.1 criterion 7, OQ-N11).
6. THE TCB_Document SHALL give for every trusted item the reason it is trusted rather than verified and the IDs of the requirements that rely on it.
7. WHEN the CI runs `ci/tcb-check`, THE CI SHALL fail if a trusted item lacks a reason or a requirement ID, if a Pinned_Dependency in the TCB_Document lacks a matching Lock_File entry, if a solver that the Lock_File lists as an Unpatched_Solver or a Certificate_Checker in the Lock_File is absent from the TCB_Document, or if `docs/tcb.md` differs from the output generated from `docs/tcb.toml`.
8. WHEN the CI runs `ci/tcb-check`, THE CI SHALL fail if an Assumption, Non-goal, Fairness assumption or trusted-item (TB-n) ID that appears in the Trust subsection of a requirement in `requirements.md`, or a trust item that `docs/traceability.toml` lists for a requirement, is absent from `docs/tcb.toml` or does not list that requirement's ID, or if an item of the Trusted items list of `requirements.md` is absent from `docs/tcb.toml`.

#### Properties

- **P7.8.1** For any trust item t named in `docs/traceability.toml` for requirement r, `t ∈ docs/tcb.toml` and `r ∈ requirements(t)`.
- **P7.8.2** For any Unpatched_Solver or Certificate_Checker c in the Lock_File, `c ∈ docs/tcb.toml` with an equal version and hash.
- **P7.8.3** For any AS-n, NG-n, F-n or TB-n ID in the Trust subsection of requirement r, the ID is in `docs/tcb.toml` and r is in its requirement list; and every TB-n of the Trusted items list is in `docs/tcb.toml`.
- **P7.8.4** For any trusted item t in `docs/tcb.toml`, `reason(t) ≠ ∅`, `requirements(t) ≠ ∅`, and every ID in `requirements(t)` is a requirement ID of `requirements.md`.
- **P7.8.5** (Table agreement) For every checker outcome o, `label_from_checker_output` with all binding checks passing returns the label that the outcome-to-label table of `docs/tcb.toml` gives o; and for every row of the checker-output table, the checker-output classifier returns that row's outcome.
- **P7.8.6** (Option agreement) For every Checkable_Proof_Format f, the option list that the Front_End passes to the checker of f equals the option list of f in `docs/tcb.toml` (P5.3.8).

#### Verification

- A4 — consistency checks (P7.8.1–P7.8.4) — `tools/tcb-check/` — `ci/tcb-check`.
- A4 — table and option agreement tests that read `docs/tcb.toml` (P7.8.5, P7.8.6) — `crates/dp-core/tests/prop_verdict.rs`, `tests/integration/certs/` — `ci/test`, `ci/certs`.

#### Trust

- The completeness of the TCB list relative to the code is a claim. The mechanical checks cover consistency with the Lock_File, with the AS, NG, F and TB IDs named in Trust subsections and the Trusted items list, with the trust items of the Traceability_Table, and with the label code.

### Requirement 38: R7.9 Small, upstreamable changes to dependencies

**Priority:** Should

**Class:** Process

**User Story:** As a proof engineer, I want changes to Creusot, Why3, why3find and the solvers kept as small tracked patches, so that the project stays maintainable and the changes can go upstream.

The planned patch sets are `patches/why3find/` (dispatch mode, design decision D-3, AS-37), `patches/creusot/` (the Creusot_CLI backend option, design decision D-4, AS-2), and `patches/cvc5/` and `patches/z3/` (Sharing_Solver patches, R3.1, R4.4). The System adds OCaml code only inside the why3find patch; all other new code is Rust (design decision D-2).

#### Acceptance Criteria

1. THE System SHALL keep every change to an external dependency (AS-1) as a patch file under `patches/<dependency>/` with a header that names the purpose, the requirement IDs served, and the upstream issue or pull request (or "not yet submitted").
2. WHEN the CI runs `ci/patches`, THE CI SHALL apply every patch to the pinned upstream version, build the patched dependency, and fail on any apply or build error.
3. WHEN the CI runs `ci/patches`, THE CI SHALL fail if a patch header lacks requirement IDs or upstream status, or if the changed lines per dependency exceed `<TBD-21>`.
4. WHEN the CI runs `ci/patches`, THE CI SHALL fail if the content hash that the Lock_File records for a patched dependency differs from the hash of the build of the pinned upstream version with the patches under `patches/<dependency>/` applied.

#### Properties

- **P7.9.1** For any patch p under `patches/`, p applies cleanly to the pinned version and p's header parses with all fields present.
- **P7.9.2** For any patched dependency d, `lock(d).hash = H(build(upstream(d) + patches(d)))`. The Unpatched_Solver part of this property is P7.7.5.

#### Verification

- A4 — patch application, header, size and hash checks (P7.9.1, P7.9.2) — `tools/patch-check/` — `ci/patches`.

#### Trust

- AS-37 (the why3find dispatch mode fits in a small upstreamable patch) and AS-2 (the Creusot_CLI backend option does), both usefulness only. The check that no patched build is listed as an Unpatched_Solver, formerly criterion 5 of this requirement, is now R7.7 criterion 8 (Must), because proved-standalone relies on it; it keeps Sharing_Solver patches out of the binaries trusted for proved-standalone (R7.1 criterion 2).

### Requirement 39: R7.10 Parsers and printers for exchanged data

**Priority:** Must

**Class:** Soundness-critical

**User Story:** As a proof engineer, I want every format that crosses a trust boundary parsed and printed by tested code with round-trip guarantees, so that malformed or hostile input cannot corrupt decisions.

The encodings follow design decision D-7: every message and stored artifact is a canonical S-expression; human-edited files (the Solver_Configuration file, `docs/tcb.toml`, `docs/traceability.toml`, `docs/formats/index.toml`, pre-registration files) are TOML with a canonical printer; evaluation raw data and manifests are canonical JSON (RFC 8785, as JSON Lines for records); and the `proof.json` extension of each goal entry is one member whose value is an opaque base64 string holding a canonical S-expression, so that legacy writers can re-indent `proof.json` without changing the bytes that criterion 8 checks. These encoding choices do not change the criteria below.

In this requirement, the *exchanged formats* are:

- Names and SMT-LIB: the Canonical_Name grammar and the solver-introduced name pattern list (R4.1); the Canonicalizer's SMT-LIB script grammar for Printed_Goals (R4.1); Lemma terms (SMT-LIB 2.6 term syntax, parsed first under the vocabulary-independent Lemma grammar and then against the Canonical_Vocabulary, R3.3); Instantiation_Hint terms (the same two grammars, R4.4).
- Messages: Lemma messages and Instantiation_Hint messages (text, identifier, Provenance_Record, authentication tag; R3.2, R4.4); Provenance_Records; Result messages (R1.3); every other protocol message that the Front_End, the Coordinator, a Worker or the Lemma_Pool receives (R1.1, R1.5 to R1.7, R3.1, R3.6); the `dispatch` format, which the Front_End, the Workers and the Replay_Tool exchange with why3find in dispatch mode over a pipe (the why3find bridge, design decisions D-2 and D-3); the `solver-channel` format, which a Worker exchanges with a Sharing_Solver over the per-Job Lemma socket for export and import messages (R3.1, R4.4); and the operator status report, a canonical S-expression with a text rendering (R1.7 criteria 8, 9 and 12; design decision D-20).
- Proofs and certificates: CPC proofs and Alethe proofs in their supported fragment (R5.2); Composed_Certificates with their recorded input problem and Task_Hash (R5.2 criterion 9); Certificate_Checker output, parsed by the checker-output classifier, and the checker-output record stored in the Certificate_Store (R5.3, R5.4); Certificate_Store manifests (R5.4).
- Local files: the Solver_Configuration file (R2.1); the operator-signed Worker registry (R1.3 criterion 9); the Partition tree (R3.5); the `proof.json` extension fields, including the conflict field and the Invalid field (R2.3, R7.2); Result_Cache entries (R7.5); the Lock_File (R7.7); the checker entries of `docs/tcb.toml` (R7.8); event traces (R1.5 criterion 10).
- Evaluation: corpus metadata and exclusion lists (R6.1); pre-registration files (R6.2); run manifests and raw-measurement records (R6.3); report provenance entries (R6.4).

The *format registry* is `docs/formats/index.toml`, which names for each exchanged format its grammar file in `docs/formats/`, its codec module, its round-trip test, its fuzz target and its size limit.

#### Acceptance Criteria

1. THE Format_Codec SHALL parse and print each exchanged format against its grammar in `docs/formats/`.
2. IF an input is syntactically invalid under the grammar of its exchanged format, THEN THE Format_Codec SHALL reject the input with an error that names the format and a byte offset within the input.
3. THE Format_Codec SHALL print every value in one canonical form.
4. THE Format_Codec SHALL parse the printed form of every valid value v of each exchanged format to v, where, for Certificate_Checker output, which the System parses but does not print, v ranges over checker-output records (round-trip property).
5. IF an input that a Worker, the Coordinator, the Lemma_Pool, the network, or a solver or checker process supplies exceeds the size limit `<TBD-22>` of its format, THEN THE Format_Codec SHALL reject the input before parsing.
6. IF an input contains a duplicated field or bytes after the end of a complete value, THEN THE Format_Codec SHALL reject the input with the error of criterion 2.
7. WHEN the CI runs `ci/traceability`, THE Traceability_Checker SHALL fail if an exchanged format has no format registry entry, or if a registry entry names a grammar file, codec module, round-trip test or fuzz target that does not exist.
8. IF the canonical printed form (criterion 3) of the value that the Format_Codec parses from an input b of the Result message format, the Result_Cache entry format, the Lock_File format or the `proof.json` extension-field format differs from b byte for byte, THEN THE Format_Codec SHALL reject b with the error of criterion 2, at the offset of the first differing byte (so that R1.3 criterion 8 discards the Result message, R7.5 criterion 5 treats the cache lookup as a miss, a rejected Lock_File leaves every local executable without a Lock_File entry under R7.7 criterion 3, and the Replay_Tool reports a goal whose `proof.json` entry is rejected as failed with the reason non-canonical entry).

#### Properties

- **P7.10.1** For any exchanged format and any value v, `parse(print(v)) = v`.
- **P7.10.2** For any accepted text t, `parse(print(parse(t))) = parse(t)`.
- **P7.10.3** (Canonical printing) For any value v, `print(parse(print(v))) = print(v)`.
- **P7.10.4** (Totality) For any exchanged format and any byte string b within its size limit, `parse(b)` terminates without panic and returns a value or an error whose offset is at most `|b|`.
- **P7.10.5** (Differential) For any generated SMT-LIB term, the Lemma term parser and the Instantiation_Hint term parser each agree with cvc5's parser on acceptance and on the parsed term, for terms within the respective grammar.
- **P7.10.6** (Size limit) For any exchanged format and any byte string b longer than the format's size limit, the input is rejected and the parser is not invoked on b.
- **P7.10.7** (Strictness) For any input with a duplicated field or trailing bytes after a complete value, `parse` returns an error.
- **P7.10.8** (Registry) Every exchanged format listed above has a registry entry, and every file and test named by the registry exists.
- **P7.10.9** (Canonical inputs) For any byte string b of the Result message, Result_Cache entry, Lock_File or `proof.json` extension-field format, the decoder accepts b only if `print(parse(b)) = b`, and for any value v of these formats it accepts `print(v)`.

#### Verification

- A3 — per-input canonical check `print(parse(b)) = b` on every Result message, Result_Cache entry, Lock_File and `proof.json` extension-field input on the proved-standalone path, valid because printing is canonical (criteria 3 and 8, P7.10.9) — `crates/dp-formats/src/canonical_check.rs` — `ci/sim`, `ci/canaries`, `ci/replay`.

- A3 — the Certificate_Checker parses every imported Lemma and instance again in the target's context (R5.3), and the Front_End re-parses every imported Lemma and hint term against the locally printed target during the re-admission pass (R3.4 criterion 9, R4.4 criterion 15), so a Format_Codec bug cannot produce the label certified for a false Lemma — `crates/dp-cert/src/check.rs`, `crates/dp-frontend/src/accept.rs` — `ci/certs`.
- A2 — Kani bounded no-panic checks of the Lemma and Instantiation_Hint parsers — `verification/kani/parsers.rs` — `ci/kani`.
- A4 — round-trip, canonical-printing, strictness and canonical-input property tests, including non-canonical encodings of valid values (P7.10.1–P7.10.3, P7.10.7, P7.10.9) — `crates/dp-formats/tests/`, `crates/dp-canon/tests/` — `ci/test`; fuzzing of every parser (P7.10.4, P7.10.6) — `fuzz/` — `ci/fuzz-smoke`, `sched/fuzz-nightly` (duration `<TBD-24>`); differential test of the Lemma and hint term parsers against cvc5's parser (P7.10.5) — `tests/diff/smtlib_parser/` — `ci/diff`; registry check (P7.10.8) — `tools/traceability-check/` — `ci/traceability`.

#### Trust

- TB-14 (the pinned SMT-LIB 2.6 standard as the reference grammar); TB-3 and TB-4 (the CPC and Alethe semantics as implemented by the pinned checkers).

### Requirement 40: R7.11 Traceability

**Priority:** Must

**Class:** Process

**User Story:** As an auditor, I want a table mapping each requirement to its properties, verification artifacts and CI jobs, with CI failing on any gap, so that every requirement is verifiably verified.

In this requirement:

- A *Verification line* is a bullet of a requirement's Verification subsection in `requirements.md`, read as one or more triples of assurance level, artifact path and CI job.
- The *CI job list* is: `ci/canaries`, `ci/certs`, `ci/corpus-check`, `ci/creusot`, `ci/diff`, `ci/eval-report`, `ci/eval-smoke`, `ci/fuzz-smoke`, `ci/integration-cluster`, `ci/kani`, `ci/lean`, `ci/lean-partition`, `ci/patches`, `ci/purity`, `ci/replay`, `ci/repro-build`, `ci/sim`, `ci/tcb-check`, `ci/test`, `ci/tlc`, `ci/trace-validation` and `ci/traceability`, which run on every pull request; and `sched/apalache-nightly`, `sched/canaries-full`, `sched/eval`, `sched/fuzz-nightly`, `sched/mutants-weekly` and `sched/sim-nightly`, which run on a schedule.
- The *declared input paths* of a scheduled job are the repository paths that the CI configuration lists as that job's inputs.
- The *acceptable run* of a scheduled job for a commit under test C is the most recent run of that job within `<TBD-25>` days whose commit is C or an ancestor of C and such that no file under the job's declared input paths changed between that commit and C.
- A requirement is *pending* when some scheduled job named for it has no acceptable run.
- The *scope status* of a requirement is the value in its Traceability_Table row: in scope, deferred or dropped. Scope status is independent of pending.

#### Acceptance Criteria

1. THE Traceability_Table SHALL map every requirement ID to its Class, scope status, Property IDs, verification artifacts with assurance levels, CI jobs and trust items, and, for a Soundness-critical requirement, to a `functions` field that lists the functions whose mutants R7.13 treats as soundness-critical targets.
2. WHEN the CI runs `ci/traceability`, THE Traceability_Checker SHALL fail if a requirement ID in `requirements.md` is missing from the Traceability_Table, or if a requirement whose scope status is in scope has no artifact or names an artifact path that does not exist.
3. WHEN the CI runs `ci/traceability`, THE Traceability_Checker SHALL fail if a `ci/` job other than `ci/traceability` that is named for a requirement whose scope status is in scope did not pass on the commit under test, or if the acceptable run of a scheduled job named for a requirement whose scope status is in scope did not pass.
4. WHEN the CI runs `ci/traceability`, THE Traceability_Checker SHALL fail if a Soundness-critical requirement whose scope status is in scope lacks an A1 or A3 artifact or lacks an A4 artifact, or if a Liveness, Robustness or Performance requirement whose scope status is in scope lacks an A2, A4 or A5 artifact.
5. THE Traceability_Checker SHALL generate `docs/traceability.md` from `docs/traceability.toml` and SHALL fail if the committed file differs.
6. WHEN the CI runs `ci/traceability`, THE Traceability_Checker SHALL fail if a Verification line of a requirement whose scope status is in scope lacks an assurance level, an artifact path or a CI job, names an artifact path that does not exist, or names a CI job that the CI configuration does not define.
7. WHEN the CI runs `ci/traceability`, THE Traceability_Checker SHALL fail if, for a requirement whose scope status is in scope, the set of level, artifact and job triples of the requirement's Verification lines differs from the set in that requirement's Traceability_Table row, or the Property IDs of the requirement in `requirements.md` differ from those in the row.
8. IF a requirement is pending, THEN THE Traceability_Checker SHALL report the requirement and each scheduled job without an acceptable run, SHALL fail `ci/traceability` on a commit that carries a release tag, and SHALL leave the outcome of `ci/traceability` on a pull request unchanged.
9. THE CI SHALL start `ci/traceability` on a commit only after every other `ci/` job of the CI job list has ended on that commit.
10. WHEN the CI runs `ci/traceability`, THE Traceability_Checker SHALL fail if the CI configuration does not define every job of the CI job list, defines a `ci/` or `sched/` job outside the CI job list, does not make `ci/traceability` depend on every other `ci/` job of the list, or declares no input paths for a `sched/` job of the list.
11. WHEN the CI runs `ci/traceability`, THE Traceability_Checker SHALL fail if a row of the Traceability_Table has no scope status or a scope status other than in scope, deferred and dropped.
12. WHEN the CI runs `ci/traceability`, THE Traceability_Checker SHALL fail if a row whose scope status is deferred belongs to a requirement whose Verification lines name only artifact paths that exist, so that every row, which starts with the scope status deferred (Initial Traceability Table), is switched to in scope no later than the commit that adds the last artifact that its requirement's Verification lines name, while the CI jobs named for a deferred row keep running the checks of its existing artifacts.

#### Properties

- **P7.11.1** For any requirement r whose scope status is in scope, `artifacts(r) ≠ ∅` and every artifact path of r exists.
- **P7.11.2** For any Soundness-critical requirement r whose scope status is in scope, `levels(r) ∩ {A1, A3} ≠ ∅ ∧ A4 ∈ levels(r)`.
- **P7.11.3** For any requirement r of Class Liveness, Robustness or Performance whose scope status is in scope, `levels(r) ∩ {A2, A4, A5} ≠ ∅`.
- **P7.11.4** (Agreement) For any requirement r whose scope status is in scope, `triples(verification(r)) = triples(row(r))` and `props(requirements.md, r) = props(row(r))`.
- **P7.11.5** (Jobs) For any CI job j named for any requirement whose scope status is in scope, j is in the CI job list and defined in the CI configuration; if j is a `ci/` job other than `ci/traceability`, j passed on the commit under test; and if j is a scheduled job, its acceptable run passed or every in-scope requirement that names j is pending.
- **P7.11.6** (Check sensitivity) Each of these changes makes `ci/traceability` fail: removing an artifact path, renaming a CI job, deleting the only A1 or A3 line of a Soundness-critical requirement, adding a requirement without a row, adding a Verification line without a CI job, removing `ci/lean-partition` from the CI configuration, removing the dependency of `ci/traceability` on one `ci/` job, removing a row's scope status, and, on a release-tagged commit, changing a file under a scheduled job's declared input paths after that job's last run.
- **P7.11.7** (Scheduled-run acceptance) For any commit C and any scheduled job j, the run of j that the Traceability_Checker uses for C is the most recent run within `<TBD-25>` days whose commit is C or an ancestor of C and whose declared input paths are unchanged between that commit and C; if no such run exists, every requirement that names j is pending; and a pending requirement makes `ci/traceability` fail on a release-tagged commit and leaves its outcome on a pull request unchanged.
- **P7.11.8** (Terminal job) In every CI pipeline of a commit, `ci/traceability` starts after every other `ci/` job of the CI job list has ended, and the outcome of `ci/traceability` is not an input of its own check.
- **P7.11.9** (Job list) The set of `ci/` and `sched/` jobs that the CI configuration defines equals the CI job list, every job that a Verification line names is in the list, and every `sched/` job of the list declares at least one input path.
- **P7.11.10** (Scope status) Every row of the Traceability_Table has exactly one scope status, which is in scope, deferred or dropped.
- **P7.11.11** (Scope switching) For any commit C on which `ci/traceability` passes and any row ρ whose scope status at C is deferred, some artifact path that the Verification lines of ρ's requirement name does not exist at C; and in the self-test, adding the last missing artifact of a deferred row without switching the row to in scope makes `ci/traceability` fail, while a missing artifact of a deferred row alone does not.

#### Verification

- A4 — unit tests of the Traceability_Checker, a self-test on mutated copies of `requirements.md`, `docs/traceability.toml` and the CI configuration that includes scope-aware cases (a missing artifact fails only for an in-scope row; a deferred row whose artifacts all exist fails), and a check of the CI configuration's job list, the dependencies of `ci/traceability` and the declared input paths (P7.11.1–P7.11.11) — `tools/traceability-check/` — `ci/traceability`.

#### Trust

- TB-18 (the CI system reports job outcomes, and the commit of each run, correctly).

### Requirement 41: R7.12 Unchanged behaviour when distribution and sharing are off

**Priority:** Must

**Class:** Functional

**User Story:** As a Rust developer, I want Creusot to prove the same goals and read the same files as today when I do not enable the cluster, portfolio racing, sharing, partitioning or certify-all mode, so that adopting the new backend carries no risk.

In this requirement, the *legacy configuration* has the cluster option not configured, portfolio racing, within-goal sharing, cross-goal sharing, hint sharing and partitioning disabled, and certify-all mode off. It is a configuration of the System, which Creusot_CLI runs through its backend option (AS-2); without that option, Creusot_CLI runs unmodified why3find (design decision D-4).

#### Acceptance Criteria

1. WHERE the legacy configuration is in effect, THE System SHALL produce the same per-goal proof outcomes as unmodified pinned why3find on the same project, up to the tolerance band of P7.12.1.
2. WHERE the legacy configuration is in effect, THE Verdict_Recorder SHALL label every proof-labelled Verdict proved-standalone.
3. WHERE the legacy configuration is in effect, THE System SHALL accept as input the `proof.json` and `.why3find` files that unmodified pinned why3find wrote for the same project, and SHALL treat every `.why3find` entry without the lookup fields and validation fields of R7.5 as a Result_Cache miss (R7.5 criterion 5).

#### Properties

- **P7.12.1** (Differential) For any project in the CI corpus, the set of goals proved by the System in the legacy configuration equals the set proved by unmodified pinned why3find, up to goals whose solver time is within `<TBD-26>` percent of the time limit in either run.
- **P7.12.2** In the legacy configuration, every proof-labelled Verdict is labelled proved-standalone.
- **P7.12.3** (Legacy files) For any project in the CI corpus with `proof.json` and `.why3find` written by unmodified pinned why3find, a Session in the legacy configuration ends without a parse error and satisfies P7.12.1.

#### Verification

- A4 — differential test against unmodified pinned why3find, starting from empty and from legacy-written `proof.json` and `.why3find` (P7.12.1–P7.12.3) — `tests/diff/legacy_compat/` — `ci/diff`.

#### Trust

- AS-17. AS-37: in the legacy configuration, why3find in dispatch mode still generates the Goals, runs its per-goal strategy and tactics and writes the legacy part of `proof.json`, and the Front_End answers its prover calls (design decision D-3); if AS-37 fails, the equivalence of criterion 1 needs more testing than P7.12.1.

### Requirement 42: R7.13 Mutation testing of soundness-critical code

**Priority:** Must

**Class:** Process

**User Story:** As an auditor, I want evidence that the tests catch bugs in soundness-critical code, so that passing tests mean something.

In this requirement:

- The *soundness-critical targets* are the functions that `tools/mutants/targets.toml` lists; this file is the authoritative target list. Criterion 6 checks it against the `functions` fields of the Traceability_Table rows of Soundness-critical requirements with scope status in scope (R7.11 criterion 1). The initial `functions` fields (Initial Traceability Table) give these targets: `canonicalize` (dp-canon); `hygiene_ok`, `is_ground_lemma`, `admit`, `admit_hint`, `instantiate`, `accept_result`, `label`, `record_verdict`, `label_from_checker_output`, `most_informative`, `classify`, `compose_plan`, `fragment_ok`, `oblig` and `import_closure` (the certification-obligation functions that compute `oblig(r)` and `imports*(r)`, R5.1), and `encode_key` (dp-core); `decode_label` and `canonical_check` (dp-formats).
- The *exclusion list* is `tools/mutants/exclusions.toml`. Each entry names one mutant by its crate, function and mutation text (the replacement that cargo-mutants reports), without a source line or column, and one equivalence artifact. For a mutant of a soundness-critical target, the artifact is a Creusot proof that the mutant and the original function return equal outputs on every input, or a Kani harness that shows the same within bounds recorded in the entry. For a mutant of any other function, the artifact may also be a property-based differential test of the mutant against the original.
- The *mutation score* of a crate is the number of killed mutants, counting mutants that make the tests exceed their time limit as killed, divided by the number of killed and surviving mutants; mutants that do not compile are not counted.

#### Acceptance Criteria

1. WHEN the CI runs `sched/mutants-weekly`, THE CI SHALL run cargo-mutants on dp-core, dp-cert, dp-formats, dp-canon, dp-frontend and dp-replay.
2. IF the mutation score of a crate falls below `<TBD-19>`, THEN THE CI SHALL fail the job and list the surviving mutants.
3. IF a mutant of a soundness-critical target survives and no exclusion entry matches the mutant's crate, function and mutation text, THEN THE CI SHALL fail the job regardless of the mutation score.
4. WHEN the CI runs `sched/mutants-weekly`, THE CI SHALL run the equivalence artifact of every exclusion entry against the entry's mutant, a Kani harness with the bounds recorded in the entry, and SHALL fail if the artifact does not pass (AS-32).
5. WHEN the CI runs `ci/traceability`, THE Traceability_Checker SHALL fail if an exclusion entry lacks a field, names an equivalence artifact that does not exist, or names a crate, function and mutation text for which cargo-mutants generates no mutant on the commit under test.
6. WHEN the CI runs `ci/traceability`, THE Traceability_Checker SHALL fail if the set of functions that `tools/mutants/targets.toml` lists differs from the union of the `functions` fields of the Traceability_Table rows of Soundness-critical requirements with scope status in scope, or if `tools/mutants/targets.toml` names a function that does not exist in its crate.
7. WHEN the CI runs `ci/traceability`, THE Traceability_Checker SHALL fail if an exclusion entry for a mutant of a soundness-critical target names an equivalence artifact other than a Creusot proof or a Kani harness, or names a Kani harness without recorded bounds.

#### Properties

- **P7.13.1** For any surviving mutant m of a soundness-critical target without an exclusion entry that matches m's crate, function and mutation text, the job fails.
- **P7.13.2** For any exclusion entry e, e's equivalence artifact exists and passes against e's mutant (a Kani harness with e's recorded bounds), and cargo-mutants generates, for the commit under test, a mutant with e's crate, function and mutation text.
- **P7.13.3** For any crate c, the reported mutation score equals the score defined above, computed from the cargo-mutants outcome records of the run.
- **P7.13.4** The set of entries of `tools/mutants/targets.toml` equals the union of the `functions` fields of the Traceability_Table rows of Soundness-critical requirements with scope status in scope, and every entry resolves to a function of its crate.
- **P7.13.5** (Exclusion strength) For any exclusion entry e whose function is a soundness-critical target, e's equivalence artifact is a Creusot proof or a Kani harness with recorded bounds.
- **P7.13.6** (Line independence) For any exclusion entry e and any edit that changes only the line numbers of e's function, for example by inserting lines above it, e matches the same mutant before and after the edit.

#### Verification

- A4 — cargo-mutants runs, score and exclusion-artifact checks, with Kani artifacts run at their recorded bounds (P7.13.1–P7.13.3) — `tools/mutants/` — `sched/mutants-weekly`.
- A4 — static checks of `tools/mutants/targets.toml` against the `functions` fields of `docs/traceability.toml` and of `tools/mutants/exclusions.toml`, including the artifact kind and a self-test that shifts the lines of an excluded function (P7.13.2 existence part, P7.13.4–P7.13.6) — `tools/traceability-check/` — `ci/traceability`.

#### Trust

- TB-19 (cargo-mutants generates representative mutants); AS-32.

---

## Coverage of mandated requirements

Each topic that the project brief requires maps to the requirements that implement it.

| Mandated topic | Implementing requirements |
|---|---|
| Verdict soundness and assurance labels | R7.1 (soundness rule and `verdict_sound`), R7.2 (labels in `proof.json` and Creusot output), R5.3 (label assignment from checker outcomes), R1.3 (Result binding) |
| Lemma provenance | R3.2 (Lemma Provenance_Records), R4.4 criteria 2, 3 and 19 (Instantiation_Hint provenance), R5.4 criterion 1 (audit trail in the Certificate_Store) |
| Admission rule, including partitions | R3.4 (rule, partition locality in criterion 3, Front_End re-admission), R3.5 (Partitions), R4.2 criteria 5 and 6 (cross-goal), R4.3 (quantified targets), R4.4 criteria 9 to 16 (hints) |
| Canonical naming, same Driver and Transformation_Pipeline | R4.1 (criteria 3 and 4), R3.4 criterion 4, R3.1 criterion 3, R4.2 criterion 2 |
| Import hygiene | R3.3, R4.3 criteria 2, 3 and 5, R4.1 criterion 9, R4.4 criteria 4 and 10 |
| Quantified sharing: Ground_Lemmas and Instantiation_Hints | R4.3, R4.4; other quantified Lemmas are NG-6 |
| Certificates: composition, unchecked when no certificate | R5.1 (criteria 4 and 5: unchecked without proofs), R5.2 (composition), R5.3 (independent checking), R5.4 (storage), R5.5 (certify-all) |
| Canaries | R7.4, R6.3 criteria 6 to 8 |
| Escalation | R1.2, R2.2 criteria 1 and 2 (Hard_Goal classification), R6.2 criterion 10 (RQ7) |
| Replay | R7.6, R2.3, R5.4 criteria 8 and 9, R6.2 criterion 16 (replay agreement with real solvers) |
| Coordinator protocol: model-checked, trace validation | R1.5 (TLA+ model, criteria 10 to 12 for trace validation), R1.6, R1.7, R7.3 |
| Cache correctness | R7.5, R7.1 criterion 6 |
| Reproducibility: pinned versions, reproducible builds, TCB document | R7.7, R7.8, R7.9, R6.3 criterion 1 |
| Traceability | R7.11, R7.13, this section and the table below |
| Evaluation: corpus and research questions | R6.1 (corpus), R6.2 (RQ1 to RQ7), R6.3 (protocol), R6.4 (report) |

## Initial Traceability Table

This is the initial content of `docs/traceability.toml` (R7.11). Levels: A1 mechanized proof, A2 model checking, A3 per-run checking, A4 testing, A5 empirical. Scope is the initial scope status of R7.11 (in scope, deferred or dropped) together with the stage of the Delivery Plan in `design.md` whose task switches the row to in scope: 1 dp-foundations, 2 dp-evaluation corpus milestone, 3 dp-distribution, 4 dp-portfolio, 5 dp-certificates, 6 dp-within-goal-sharing, 7 dp-cross-goal-sharing, 8 dp-evaluation studies. Every row starts deferred and is switched to in scope no later than the commit that adds the last artifact that its requirement's Verification lines name (R7.11 criterion 12). That stage can be later than the stage that owns the requirement: for example, R1.2 names `eval/scripts/rq7_escalation/` and so switches in stage 8. R3.5 has priority Could and sits behind the `partitioning` option, off by default (OQ-N14 resolved by design decision D-19). Functions is the `functions` field of a Soundness-critical row (R7.11 criterion 1, R7.13 criterion 6); "—" marks a row with no such field, or, for R7.4 and R7.6, an empty one. Shared jobs: `ci/test` (unit and property-based tests), `ci/diff` (differential tests), `ci/sim` and `sched/sim-nightly` (deterministic simulation; seeds per run `<TBD-13>`), `sched/mutants-weekly` (mutation testing).

| Req | Class | Scope | Properties | Functions | Artifacts (level) | CI jobs |
|---|---|---|---|---|---|---|
| R1.1 | Functional | deferred, stage 3 | P1.1.1–P1.1.10 | — | `tests/integration/cluster/` incl. `cluster_off.rs`, `fallback.rs`, `config.rs` (A4), `tests/diff/dist_vs_local/` (A4), `prop_assign.rs` (A4), `prop_config.rs` (A4), `tests/sim/` (A4) | `ci/integration-cluster`, `ci/diff`, `ci/test`, `ci/sim` |
| R1.2 | Performance | deferred, stage 8 | P1.2.1–P1.2.7 | — | `escalation.rs` (A1), `Coordinator.tla` (A2), `prop_escalation.rs`, `tests/sim/`, `tests/integration/fast_pass/` (A4), `eval/scripts/rq7_escalation/` (A5) | `ci/creusot`, `ci/tlc`, `ci/test`, `ci/sim`, `ci/integration-cluster`, `sched/eval` |
| R1.3 | Soundness-critical | deferred, stage 3 | P1.3.1–P1.3.11 | `accept_result` | `verdict.rs` (A1), `dp-frontend/src/accept.rs` (A3), `prop_accept.rs`, `fuzz/result_message/`, `adversarial_coordinator.rs`, `tests/canaries/`, `tools/mutants/` (A4) | `ci/creusot`, `ci/sim`, `ci/canaries`, `ci/test`, `ci/fuzz-smoke`, `sched/fuzz-nightly`, `sched/sim-nightly`, `sched/mutants-weekly` |
| R1.4 | Robustness | deferred, stage 8 | P1.4.1–P1.4.6 | — | `kani/calibration.rs` (A2), `prop_calibration.rs`, `calibration.rs`, `adversarial_coordinator.rs` (A4), `rq6_instability/` (A5) | `ci/kani`, `ci/test`, `ci/integration-cluster`, `ci/sim`, `sched/eval` |
| R1.5 | Liveness | deferred, stage 3 | P1.5.1–P1.5.15 | — | `Coordinator.tla` (A2), `verification/tla/traces/` (A2), `kani/coordinator.rs` (A2), `tests/sim/`, `cancel.rs`, `deadline.rs`, `prop_cancel.rs`, `prop_record.rs`, `crates/dp-shell/` trace writer (A4) | `ci/tlc`, `sched/apalache-nightly`, `ci/trace-validation`, `ci/kani`, `ci/sim`, `sched/sim-nightly`, `ci/integration-cluster`, `ci/test` |
| R1.6 | Robustness | deferred, stage 3 | P1.6.1–P1.6.15 | — | `Coordinator.tla` (A2), `tests/sim/faults/` (A4), `heartbeat_margin.rs` (A4), `prop_idempotent.rs` (A4), `faults.rs` (A4) | `ci/tlc`, `sched/apalache-nightly`, `ci/sim`, `sched/sim-nightly`, `ci/test`, `ci/integration-cluster` |
| R1.7 | Functional | deferred, stage 3 | P1.7.1–P1.7.10 | — | `Coordinator.tla` (A2), `prop_operator.rs`, `operator.rs`, `tests/sim/`, `adversarial_coordinator.rs`, `dp-formats/tests/` status-report round trip (A4) | `ci/tlc`, `ci/test`, `ci/integration-cluster`, `ci/sim` |
| R2.1 | Functional | deferred, stage 4 | P2.1.1–P2.1.5 | — | `prop_config.rs`, `fuzz/config_parser/`, `tests/integration/config/`, `tests/sim/` (A4) | `ci/test`, `ci/fuzz-smoke`, `sched/fuzz-nightly`, `ci/integration-cluster`, `ci/sim` |
| R2.2 | Performance | deferred, stage 8 | P2.2.1–P2.2.19 | — | `race.rs` (A1), `Coordinator.tla` (A2), `prop_race.rs`, `tests/sim/`, `tests/sim/faults/`, `cluster/race.rs`, `tests/integration/config/` (A4), `rq2_portfolio/` (A5) | `ci/creusot`, `ci/tlc`, `sched/apalache-nightly`, `ci/test`, `ci/sim`, `sched/sim-nightly`, `ci/integration-cluster`, `sched/eval` |
| R2.3 | Functional | deferred, stage 4 | P2.3.1–P2.3.6 | — | `prop_proofjson.rs`, `tests/replay/`, `tests/replay/tamper/`, `legacy_reader/` (A4) | `ci/test`, `ci/replay`, `ci/diff` |
| R3.1 | Performance | deferred, stage 8 | P3.1.1–P3.1.15 | — | `Sharing.tla` incl. forwarding liveness (A2), `prop_forward.rs`, `prop_pending.rs`, `sharing_on_off/`, `tests/sim/faults/`, `hostile_pool.rs`, `two_job_default_deps.rs`, `tests/sim/forwarding.rs` (A4), `rq3_within_goal/` (A5) | `ci/tlc`, `ci/test`, `ci/diff`, `ci/sim`, `sched/sim-nightly`, `sched/eval` |
| R3.2 | Soundness-critical | deferred, stage 6 | P3.2.1–P3.2.9 | `admit`, `compose_plan` | `provenance.rs`, `compose_plan.rs`, `admission.rs`, `Admission.lean` (A1), `dp-cert/src/check.rs` certificate check (A3), `dp-formats/tests/`, `fuzz/provenance/`, `prop_provenance.rs`, `prop_admission.rs`, `prop_compose.rs`, `prop_pool.rs`, `lying_provenance.rs`, `tests/canaries/`, `tests/replay/`, `tools/mutants/` (A4) | `ci/creusot`, `ci/lean`, `ci/certs`, `ci/test`, `ci/fuzz-smoke`, `ci/sim`, `ci/canaries`, `ci/replay`, `sched/mutants-weekly` |
| R3.3 | Soundness-critical | deferred, stage 6 | P3.3.1–P3.3.6 | `hygiene_ok` | `admission.rs`, `Admission.lean` (A1), `dp-frontend/src/accept.rs` re-admission, `dp-cert/src/check.rs` checker re-parse (A3), `kani/parsers.rs` (A2), `prop_hygiene.rs`, `dp-formats/tests/`, `fuzz/lemma_parser/`, `tools/mutants/` (A4) | `ci/creusot`, `ci/lean`, `ci/certs`, `ci/canaries`, `ci/kani`, `ci/test`, `ci/fuzz-smoke`, `sched/fuzz-nightly`, `sched/mutants-weekly` |
| R3.4 | Soundness-critical | deferred, stage 6 | P3.4.1–P3.4.13 | `admit` | `Admission.lean`, `ModelSanity.lean`, `admission.rs` (A1), `dp-frontend/src/accept.rs` re-admission, `dp-cert/src/check.rs` certificate check (A3), `admission_lean_vs_rust/`, `prop_admission.rs`, `tests/canaries/`, `lying_provenance.rs`, `tools/mutants/` (A4) | `ci/lean`, `ci/creusot`, `ci/certs`, `ci/canaries`, `ci/diff`, `ci/test`, `ci/sim`, `sched/mutants-weekly` |
| R3.5 | Soundness-critical | deferred, stage 6 (Could; D-19) | P3.5.1–P3.5.10 | `accept_result`, `compose_plan`, `label` | `Partition.lean` incl. `compose_partition_sound` and `verdict_sound_partition`, `race.rs`, `verdict.rs` (A1), `dp-cert/src/partition.rs`, `dp-frontend/src/accept.rs` (A3), `prop_partition.rs`, `prop_race.rs`, `tests/canaries/partition/`, `adversarial_coordinator.rs`, `dp-formats/tests/`, `tools/mutants/` (A4) | `ci/lean-partition`, `ci/creusot`, `ci/certs`, `ci/sim`, `ci/canaries`, `ci/test`, `sched/mutants-weekly` |
| R3.6 | Robustness | deferred, stage 6 | P3.6.1–P3.6.6 | — | `kani/pool.rs` (A2), `fuzz/lemma_pool_ingest/`, `prop_pool.rs`, `hostile_pool.rs`, `tests/sim/faults/` (A4) | `ci/kani`, `ci/fuzz-smoke`, `sched/fuzz-nightly`, `ci/test`, `ci/sim`, `sched/sim-nightly` |
| R4.1 | Soundness-critical | deferred, stage 6 | P4.1.1–P4.1.20 | `canonicalize`, `admit` | `Canon.lean`, `admission.rs` (A1), `dp-frontend/src/accept.rs` local re-derivation, `dp-cert/src/check.rs` certificate check (A3), `dp-canon/tests/`, `prop_rename.rs`, `canon_equisat/`, `canon_determinism/`, `canon_lean_vs_rust/`, `solver_names/`, `fuzz/canonicalizer/`, `lying_provenance.rs`, `tests/canaries/`, `tools/mutants/` (A4) | `ci/lean`, `ci/creusot`, `ci/certs`, `ci/canaries`, `ci/test`, `ci/diff`, `ci/fuzz-smoke`, `sched/fuzz-nightly`, `ci/sim`, `sched/mutants-weekly` |
| R4.2 | Performance | deferred, stage 8 | P4.2.1–P4.2.8 | — | `Sharing.tla` (A2), `Admission.lean`, `admission.rs` (A1), `prop_scope.rs`, `cross_goal_on_off/`, `tests/integration/config/`, `tests/canaries/` (A4), `rq4_cross_goal/` (A5) | `ci/tlc`, `ci/lean`, `ci/creusot`, `ci/test`, `ci/diff`, `ci/integration-cluster`, `ci/canaries`, `sched/eval` |
| R4.3 | Soundness-critical | deferred, stage 7 | P4.3.1–P4.3.6 | `is_ground_lemma`, `hygiene_ok`, `admit` | `Admission.lean`, `admission.rs` (A1), `dp-frontend/src/accept.rs` re-admission, `dp-cert/src/check.rs` certificate check (A3), `prop_ground.rs`, `cross_goal_on_off/`, `tests/canaries/`, `tools/mutants/` (A4) | `ci/lean`, `ci/creusot`, `ci/certs`, `ci/canaries`, `ci/test`, `ci/diff`, `sched/mutants-weekly` |
| R4.4 | Soundness-critical | deferred, stage 8 | P4.4.1–P4.4.13 | `admit_hint`, `instantiate` | `Hints.lean`, `hints.rs` (A1), `dp-frontend/src/accept.rs` re-admission and instance recomputation, `dp-cert/src/check.rs` checker instantiation steps (A3), `kani/parsers.rs` (A2), `prop_hints.rs`, `hints_lean_vs_rust/`, `hints_on_off/`, `tests/integration/certs/`, `dp-formats/tests/`, `fuzz/hint_parser/`, `tests/canaries/`, `lying_provenance.rs`, `tools/mutants/` (A4), `rq4_cross_goal/` (A5) | `ci/lean`, `ci/creusot`, `ci/certs`, `ci/canaries`, `ci/kani`, `ci/test`, `ci/diff`, `ci/fuzz-smoke`, `sched/fuzz-nightly`, `ci/sim`, `sched/mutants-weekly`, `sched/eval` |
| R5.1 | Soundness-critical | deferred, stage 8 | P5.1.1–P5.1.12 | `oblig`, `import_closure` | `certification.rs` (A1), `dp-cert/src/check.rs` (A3), `Coordinator.tla` (A2), `prop_certification.rs`, `tests/sim/`, `tests/integration/certs/`, `tools/mutants/` (A4), `rq5_certificates/` (A5) | `ci/creusot`, `ci/certs`, `ci/tlc`, `ci/test`, `ci/sim`, `sched/sim-nightly`, `sched/mutants-weekly`, `sched/eval` |
| R5.2 | Soundness-critical | deferred, stage 6 | P5.2.1–P5.2.15 | `compose_plan`, `fragment_ok` | `Compose.lean` (`compose_sound`), `dp-core/src/compose_plan.rs`, `dp-core/src/fragment.rs` (A1), `dp-cert/src/check.rs` (A3), `prop_compose.rs`, `compose_lean_vs_rust/`, `prop_partition.rs`, `prop_fragment.rs`, `dp-formats/tests/`, `fuzz/cpc_proof/`, `fuzz/alethe_proof/`, `tests/integration/certs/`, `tests/canaries/`, `tools/mutants/` (A4) | `ci/lean`, `ci/creusot`, `ci/certs`, `ci/canaries`, `ci/test`, `ci/diff`, `ci/fuzz-smoke`, `sched/fuzz-nightly`, `sched/mutants-weekly` |
| R5.3 | Soundness-critical | deferred, stage 5 | P5.3.1–P5.3.13 | `label_from_checker_output`, `fragment_ok`, `label` | `dp-cert/src/check.rs` (A3), `labels.rs` (A1), `prop_verdict.rs`, `label_lean_vs_rust/`, `tests/certs/tamper/`, `tests/canaries/`, `tests/integration/certs/`, `fuzz/checker_output/`, `tools/mutants/` (A4) | `ci/certs`, `ci/canaries`, `ci/creusot`, `ci/test`, `ci/diff`, `ci/fuzz-smoke`, `sched/fuzz-nightly`, `sched/mutants-weekly` |
| R5.4 | Functional | deferred, stage 5 | P5.4.1–P5.4.8 | — | `Coordinator.tla` `CertifiedHasStoredEntry` (A2), `prop_store.rs`, `dp-formats/tests/`, `fuzz/cert_manifest/`, `tests/replay/tamper/`, `tests/replay/`, `store_faults.rs` (A4) | `ci/tlc`, `ci/test`, `ci/fuzz-smoke`, `ci/replay`, `ci/certs` |
| R5.5 | Soundness-critical | deferred, stage 8 | P5.5.1–P5.5.7 | `label` | `verdict.rs` (A1), `dp-cert/src/check.rs` (A3), `prop_verdict.rs`, `certify_all_on_off/`, `tests/integration/config/`, `tests/canaries/`, `tools/mutants/` (A4), `rq5_certificates/` (A5) | `ci/creusot`, `ci/certs`, `ci/test`, `ci/diff`, `ci/integration-cluster`, `ci/canaries`, `sched/mutants-weekly`, `sched/eval` |
| R6.1 | Functional | deferred, stage 2 | P6.1.1–P6.1.7 | — | `eval/corpus/tests/`, `export_vs_why3find/`, `dp-formats/tests/`, `fuzz/corpus_metadata/` (A4), `corpus_hardness/`, `eval/rq/corpus.toml` (A5) | `ci/corpus-check`, `ci/diff`, `ci/test`, `ci/fuzz-smoke`, `sched/eval` |
| R6.2 | Performance | deferred, stage 8 | P6.2.1–P6.2.11 | — | `eval/scripts/rq1_distribution/`, `rq2_portfolio/`, `rq3_within_goal/`, `rq4_cross_goal/`, `rq5_certificates/`, `rq6_instability/`, `rq7_escalation/`, `eval/rq/` (A5), `eval/tests/`, `eval/tests/metrics/` (A4) | `sched/eval`, `ci/eval-smoke`, `ci/eval-report` |
| R6.3 | Performance | deferred, stage 8 | P6.3.1–P6.3.13 | — | `eval/scripts/` (A5), `eval/tests/`, `eval/tests/canary_gate/`, `eval/report/check/`, `dp-formats/tests/` (A4) | `sched/eval`, `ci/eval-smoke`, `ci/eval-report`, `ci/test` |
| R6.4 | Performance | deferred, stage 8 | P6.4.1–P6.4.11 | — | `eval/report/`, `eval/report/check/`, `eval/tests/report_check/` (A4), `eval/report/` on scheduled raw data (A5) | `ci/eval-report`, `sched/eval` |
| R7.1 | Soundness-critical | deferred, stage 6 | P7.1.1–P7.1.8 | `label`, `record_verdict`, `accept_result` | `Verdict.lean`, `Certifiable.lean`, `verdict.rs` (A1), `dp-frontend/src/accept.rs`, `dp-cert/src/check.rs` (A3), `prop_verdict.rs`, `label_lean_vs_rust/`, `cache_on_off/`, `tests/canaries/`, `adversarial_coordinator.rs`, `lying_provenance.rs`, `tools/mutants/` (A4) | `ci/lean`, `ci/creusot`, `ci/sim`, `ci/canaries`, `ci/certs`, `ci/test`, `ci/diff`, `sched/canaries-full`, `sched/sim-nightly`, `sched/mutants-weekly` |
| R7.2 | Soundness-critical | deferred, stage 4 | P7.2.1–P7.2.9 | `decode_label`, `most_informative`, `classify` | `proofjson.rs`, `labels.rs` (A1), `crates/dp-replay/` (A3), `prop_proofjson.rs`, `fuzz/proofjson/`, `prop_verdict.rs`, `legacy_reader/`, `cluster/race.rs`, `cli_labels/`, `tests/replay/tamper/` (A4) | `ci/creusot`, `ci/replay`, `ci/test`, `ci/fuzz-smoke`, `ci/diff`, `ci/integration-cluster` |
| R7.3 | Process | deferred, stage 1 | P7.3.1–P7.3.4 | — | `tools/purity-check/` (A4), `crates/dp-core/` (A1), `verification/kani/` (A2), `crates/dp-core/tests/`, `tools/mutants/` (A4) | `ci/purity`, `ci/creusot`, `ci/kani`, `ci/test`, `sched/mutants-weekly` |
| R7.4 | Soundness-critical | deferred, stage 8 | P7.4.1–P7.4.10 | — | `tests/canaries/witness.rs` (A3), `tests/canaries/`, `tests/canaries/modes.toml`, `tests/certs/tamper/`, `eval/tests/canary_gate/`, `tools/mutants/` (A4) | `ci/canaries`, `sched/canaries-full`, `ci/certs`, `ci/eval-smoke`, `sched/mutants-weekly` |
| R7.5 | Soundness-critical | deferred, stage 5 | P7.5.1–P7.5.10 | `encode_key` | `cache_key.rs` (A1), `kani/cache_key.rs` (A2), `dp-frontend/src/accept.rs`, `dp-cert/src/check.rs` (A3), `prop_cache_key.rs`, `cache_on_off/`, `legacy_reader/`, `tests/canaries/`, `tools/mutants/` (A4) | `ci/creusot`, `ci/kani`, `ci/sim`, `ci/certs`, `ci/test`, `ci/diff`, `ci/canaries`, `sched/mutants-weekly` |
| R7.6 | Soundness-critical | deferred, stage 8 | P7.6.1–P7.6.8 | — | `crates/dp-replay/` (A3), `tests/replay/tamper/`, `tests/replay/`, `tools/mutants/` (A4), `rq6_instability/` replay agreement (A5) | `ci/replay`, `sched/mutants-weekly`, `sched/eval` |
| R7.7 | Process | deferred, stage 5 | P7.7.1–P7.7.6 | — | `deps/` reproducible build, Lock_File schema and upstream checksum or double-rebuild check (A4), `tools/patch-check/` Unpatched_Solver marks (A4), `tests/integration/pins/`, `tests/integration/certs/`, `prop_verdict.rs` (A4) | `ci/repro-build`, `ci/patches`, `ci/integration-cluster`, `ci/certs`, `ci/test` |
| R7.8 | Process | deferred, stage 5 | P7.8.1–P7.8.6 | — | `tools/tcb-check/`, `prop_verdict.rs`, `tests/integration/certs/` (A4) | `ci/tcb-check`, `ci/test`, `ci/certs` |
| R7.9 | Process | deferred, stage 1 | P7.9.1–P7.9.2 | — | `tools/patch-check/` (A4) | `ci/patches` |
| R7.10 | Soundness-critical | deferred, stage 6 | P7.10.1–P7.10.9 | `canonical_check` | `dp-cert/src/check.rs`, `dp-frontend/src/accept.rs` re-parse, `dp-formats/src/canonical_check.rs` (A3), `kani/parsers.rs` (A2), `dp-formats/tests/`, `dp-canon/tests/`, `fuzz/`, `smtlib_parser/`, registry check in `tools/traceability-check/` (A4) | `ci/certs`, `ci/sim`, `ci/canaries`, `ci/replay`, `ci/kani`, `ci/test`, `ci/fuzz-smoke`, `sched/fuzz-nightly`, `ci/diff`, `ci/traceability` |
| R7.11 | Process | deferred, stage 1 | P7.11.1–P7.11.11 | — | `tools/traceability-check/` (A4) | `ci/traceability` |
| R7.12 | Functional | deferred, stage 1 | P7.12.1–P7.12.3 | — | `tests/diff/legacy_compat/` (A4) | `ci/diff` |
| R7.13 | Process | deferred, stage 1 | P7.13.1–P7.13.6 | — | `tools/mutants/`, static checks in `tools/traceability-check/` (A4) | `sched/mutants-weekly`, `ci/traceability` |

## Open Questions

### Numeric values to decide

Every value below is undecided. None of them should be filled in without data or an explicit decision. Until decided, each value is a configuration parameter with no committed default; the TLA+, simulation, smoke and fixture configurations use small test values that are not proposals.

- **TBD-1** Default Local_Fast_Pass time budget (why3find's own default is 200 ms; whether to keep it is open).
- **TBD-2** Maximum time from cancellation receipt to solver termination on a Worker.
- **TBD-3** Per-goal deadline for a Verdict, measured from escalation on the Front_End's local clock and enforced by the Front_End, so that it needs only F3 (R1.5 criterion 9, P1.5.7); must be at least TBD-9 + TBD-14 + TBD-15, the Race deadline plus the proof and checking budgets, which bound a Race (P2.2.6). Must also be at least the longest time that why3find's own per-goal strategy can take for one goal in the legacy configuration, so that the deadline does not cut that strategy short (R7.12, P7.12.1).
- **TBD-4** TLC bounds for `ci/tlc` and Apalache bounds for `sched/apalache-nightly` (goals, Workers, messages, Lemmas).
- **TBD-5** Worker heartbeat timeout.
- **TBD-6** Maximum reassignment attempts per Job.
- **TBD-7** Front_End timeout without Coordinator messages before fallback.
- **TBD-8** Maximum concurrent Jobs per Goal_Version in a Race.
- **TBD-9** Race deadline, measured from escalation, which is also the step of Hard_Goal classification (R2.2 introduction and criterion 12); it applies to the Races of all Escalated_Goals. Whether Races of Escalated_Goals that are not Hard_Goals need a separate value is open. A Race can wait for certification after its deadline (R2.2 criterion 13, P2.2.6), so TBD-3 must be at least TBD-9 + TBD-14 + TBD-15. Constraints: TBD-9 must not cut short why3find's own per-goal strategy in the legacy configuration (R7.12, P7.12.1); and TBD-9 should exceed TBD-30, so that goals escalated before the connection to the Proof_Server completes can still reach the Cluster.
- **TBD-10** Default hardness rule for Hard_Goal classification.
- **TBD-11** Maximum stored pool items (Lemmas and Instantiation_Hints counted together) per Goal_Version per Session in the Lemma_Pool (R3.6 criteria 1 and 2).
- **TBD-12** Maximum printed Lemma size in bytes (R3.3 criterion 6, R3.6 criterion 3). The R7.10 size limit TBD-22 for the Lemma format must not be smaller.
- **TBD-13** Number of simulation seeds per `ci/sim` run and per `sched/sim-nightly` run.
- **TBD-14** Proof budget: time from a Result's certification start to its proof deadline (R5.1), by which every required proof must be obtained and the Composed_Certificate composed (R5.2 criterion 14).
- **TBD-15** Checking budget: time from a Result's proof deadline to its certification deadline (R5.1), by which the Certificate_Checker must report an outcome (R5.3 criteria 6 and 10). TBD-9 + TBD-14 + TBD-15 bounds a Race (P2.2.6, P5.1.3).
- **TBD-16** Hardness time threshold: minimum median reference time to a proof, in the hardness measurement, for a proved candidate to count as proved-hard (R6.1); also the threshold of the hardness re-measurement (R6.2 introduction and criterion 13).
- **TBD-17** Repetitions per experiment configuration, including the hardness measurement (R6.1, R6.3 criterion 3); smoke runs use one repetition (R6.3 criterion 16).
- **TBD-18** Confidence level of reported intervals, also used by the decision rule with Holm adjustment across the primary metrics (R6.2 introduction).
- **TBD-19** Minimum mutation score per crate.
- **TBD-20** Replay time-limit factor (R7.6 criterion 5, P7.6.8); constraint: TBD-20 ≥ 1, so that replay never gives the solver less time than the locally scaled reference time limit.
- **TBD-21** Maximum changed lines per dependency patch set.
- **TBD-22** Size limit per exchanged format.
- **TBD-23** Worker counts for RQ1.
- **TBD-24** Fuzzing duration per target in `sched/fuzz-nightly`.
- **TBD-25** Maximum age of a passing scheduled-job run accepted by `ci/traceability`.
- **TBD-26** Tolerance band near the time limit in the legacy-compatibility differential test.
- **TBD-27** Pre-registered hypothesis targets for the primary metrics of RQ1–RQ4, the proved-count difference (R6.2 introduction).
- **TBD-28** Pre-registered hypothesis target for the primary metric of RQ5 (certificate overhead or coverage, as the pre-registration names).
- **TBD-29** Pre-registered instability thresholds for RQ6, one per RQ6 metric (R6.2 criteria 9, 15 and 16); only the primary metric's threshold enters the decision rule.
- **TBD-30** Front_End connection timeout at Session start (R1.1 criterion 5); may equal TBD-7. TBD-9 should exceed it (TBD-9 entry).
- **TBD-31** Heartbeat interval for Workers, the Coordinator and the Front_End (R1.6 criteria 14, 15 and 19); TBD-31 + D must be smaller than min(TBD-5, TBD-7, TBD-32), where D is the maximum message delay of the simulation fault model (R1.6 criterion 18, P1.6.15).
- **TBD-32** Coordinator timeout without messages from a Session's Front_End before the Coordinator drops that Session's Jobs (R1.6 criterion 16).
- **TBD-33** Maximum time from expiry of a Job's local time limit to termination of the solver process tree on a Worker (R1.4 criterion 5); may equal TBD-2.
- **TBD-34** Maximum total size in bytes of the pool items (Lemmas and Instantiation_Hints) and Provenance_Records stored in the Lemma_Pool (R3.6 criterion 4).
- **TBD-35** Maximum printed Instantiation_Hint size in bytes (R4.4 criterion 9, R3.6 criterion 3); may equal TBD-12. The R7.10 size limit TBD-22 for the Instantiation_Hint format must not be smaller.
- **TBD-36** Reference time limit per goal in the hardness measurement (R6.1); candidates unproved within it in some repetition count as unsolved. Must be larger than TBD-16.
- **TBD-37** Per-goal budget of the evaluation in wall-clock seconds, at which times to Verdict are censored (R6.2 introduction). Constraint: TBD-37 ≥ TBD-9 + TBD-14 + TBD-15, the bound on a Race (P2.2.6). May equal TBD-36 if that value meets the constraint.
- **TBD-38** Number of bootstrap resamples for the confidence intervals of R6.2.
- **TBD-39** Pre-registered hypothesis target for the primary metric of RQ7 (share of goals proved by the Local_Fast_Pass, Cluster latency avoided or net makespan difference, as the pre-registration names).
- **TBD-40** Number of seeds per Canary_Goal and canary mode in `ci/canaries` and `sched/canaries-full` (R7.4 criterion 5); each seed fixes the solver seeds and the adversarial Coordinator schedule.
- **TBD-41** Maximum time in seconds that a Worker holds a Lemma in a Job's pending queue for used-Lemma order before rejecting it (R3.1 criteria 14 and 17, P3.1.12). A value below TBD-43 plus the maximum message delay lets held Lemmas time out before their used Lemmas can be forwarded.
- **TBD-42** Maximum number of Lemmas in one Job's pending queue (R3.1 criteria 14 and 15, P3.1.12).
- **TBD-43** Maximum time in seconds from the Lemma_Pool storing a pool item to its forwarding to every registered Job that may receive it (R3.1 criterion 11, P3.1.10).
- **TBD-44** Minimum number of Canary_Goals with a Hypothesis that contains a quantifier that `ci/canaries` runs in each canary mode (R7.4 criterion 15, P7.4.8).
- **TBD-45** Maximum time in seconds from a Verdict recorded at the per-goal deadline TBD-3 to the termination of that Goal_Version's local solver process trees on the Front_End's machine (R1.5 criterion 17, P1.5.14); may equal TBD-2.
- **TBD-46** Number of most recent trace records that the Front_End, the Coordinator and each Worker keep under the bounded tracing retention setting (R1.5 criterion 18, P1.5.15).

### Design questions

- **OQ-N1** Extend why3find's proof server (`why3find server`) upstream, or build a separate Coordinator that speaks the `why3find client` protocol (AS-3)? Resolved by design decision D-1: a separate Rust service, `dp-coordinator`, plays the Proof_Server role and replaces the central proxy of `why3find server`; AS-3 now concerns only the dp message protocol.
- **OQ-N2** Implementation language of the new components. This document assumes Rust for dp-core so that Creusot can verify it (dogfooding); the boundary with why3find (OCaml) is open. Resolved by design decision D-2: all new code is Rust; OCaml exists only inside the why3find patch; the boundary with why3find is a process boundary (a pipe carrying the `dispatch` format, R7.10), with no FFI.
- **OQ-N3** Should an Invalid answer from one Solver_Configuration end a Race, given that solvers are untrusted and quantified goals rarely have trustworthy counter-models? The draft offers the policies `continue` (default) and `stop`; under `stop`, only an Invalid Result that is not a Lemma_Dependent_Result ends the Race (R2.2 criteria 15 and 16).
- **OQ-N4** Can Z3 and cvc5 share one Driver (AS-10), or is within-goal sharing limited to Jobs of one prover? Draft until decided: Lemmas move only between Jobs with equal Driver and Transformation_Pipeline hashes (R3.1 criterion 3), and RQ3 (R6.2) gives the data on AS-10.
- **OQ-N5** Where does canonicalization run: a Why3 printer change, a Why3 transformation, or post-processing of SMT-LIB text? Resolved by design decision D-5: post-processing of SMT-LIB text in dp-canon, on the Front_End and on the Workers (R4.1 introduction).
- **OQ-N6** How are proofs for Lemma_Dependent_Results obtained: proof production during the Job (costly for every Job), or a proof-producing re-run of only the winning Job and its Lemmas? Each Lemma proof comes from a proof-producing run that the Front_End requests on the Lemma problem printed from `print_local(B)`, in the target proof's Checkable_Proof_Format (R5.1 criterion 2). Resolved by design decision D-6: the target proof comes from one proof-producing run of a proof-producing cvc5 Solver_Configuration on the extended problem, on the Cluster or locally, with the Lemma proofs requested in parallel once `oblig(r)` is known (R5.1 criterion 11).
- **OQ-N7** A Result from an Unpatched_Solver whose only imports are Instantiation_Hints is sound whenever the instances are checked (P4.4.1), with the same solver trust as proved-standalone. Should such Results get a proof label after only the instance check, or does the strict rule (full certificate) stay? This document applies the strict rule.
- **OQ-N8** Does why3find's reference-time parallel pass run locally or on the Cluster? Resolved by design decision D-17: the reference-time parallel pass of an Escalated_Goal runs as its Race on the Cluster, and on the local machine only without the cluster option or in the `local` fallback (R2.2 configuration list, R1.2 introduction).
- **OQ-N9** Which projects besides Creusot's test suite and CreuSAT go into the corpus? Draft until decided: the corpus configuration lists only Creusot's test suite and CreuSAT.
- **OQ-N10** Pinning mechanism: Nix, opam lock files with containers, or another approach? Resolved by design decision D-8: Nix flakes with a pinned nixpkgs, opam packages built inside Nix, and OCI images for the integration cluster built by Nix; TB-16 is the Nix bootstrap, and TB-22 remains only for upstream release artifacts accepted by published checksum.
- **OQ-N11** Worker credential and authentication mechanism, and how revocation reaches the Front_End. Constraint (R7.1 criterion 7): one scheme for Results, Lemmas and Instantiation_Hints, under which the Front_End and every Worker verify any registered Worker's tag from public verification data alone, and that data lets no one produce a tag for that Worker. A shared symmetric key per Cluster does not meet the constraint; per-Worker signatures do. Further constraint (R1.3 criterion 9): the Front_End applies only registrations and revocations signed with the operator signing key in its local configuration, whatever channel delivers them. Resolved by design decision D-9: an Ed25519 key pair per Worker; an offline operator signing key signs the versioned Worker registry and revocations, and the Front_End uses the highest-version valid registry it holds and every valid revocation it has seen, from whatever channel (R1.3 criteria 9 and 11, R7.1 criterion 7).
- **OQ-N12** Does Alt-Ergo take part in sharing, given that Why3 drives Alt-Ergo through its native input language? Resolved by design decision D-18: Alt-Ergo Jobs never carry the import mark and export nothing; Alt-Ergo still races as an Unpatched_Solver (R3.1 criteria 18 and 19).
- **OQ-N13** Exact definition of the module scope for Sibling_Goals (Rust module, Coma module, or why3find file). Resolved by design decision D-10: the Rust module, derived from the goal path by the rule in `docs/formats/sibling-group.abnf` (AS-29); a goal path that does not match the rule falls back to its why3find file, and the Front_End logs the fallback (R4.2 introduction and criterion 9).
- **OQ-N14** Is partitioning (R3.5, Could) worth including at all? Resolved by design decision D-19: partitioning stays as a Could, behind the `partitioning` option, off by default, delivered last within dp-within-goal-sharing, with `ci/lean-partition` separate (R3.5 criterion 10).
- **OQ-N15** Should Lemmas persist across Sessions, and how would persisted Lemmas interact with the Result_Cache? Draft until decided: no Lemma persists across Sessions; the Lemma_Pool deletes a Session's pool items at Session end and starts empty after a restart (R3.6 criteria 7 and 8).
- **OQ-N16** Which witness method for Canary_Goals is practical at corpus scale (AS-18)? The draft fixes the default for Canary_Goals with a quantified Hypothesis (definitions with Ethos-checked CPC proofs) and limits finite evaluation to uninterpreted sorts (R7.4 criterion 2); open is whether enough mutants get a witness, which R7.4 criteria 15 and 16 measure.
- **OQ-N17** Where is the cluster option set (Creusot_CLI flag, project configuration file, environment variable), and which source takes precedence (R1.1)? Resolved by design decision D-13: the command-line flag, then the environment variable `DP_CLUSTER`, then `dp.toml`, first found wins (R1.1 criterion 9).
- **OQ-N18** What is the grammar of a well-formed Proof_Server address (R1.1 criterion 6)? Resolved by design decision D-13: a ZeroMQ TCP endpoint `tcp://<host>:<port>` with a DNS name, an IPv4 literal or a bracketed IPv6 literal as host and a port in the valid TCP port range, recorded in `docs/formats/` (R1.1 criterion 10).
- **OQ-N19** Should a malformed cluster option value stop the Session (current draft, R1.1 criterion 6) or fall back to local proving?
- **OQ-N20** May the Front_End reconnect to the Proof_Server later in the same Session after the R1.1 criterion 5 fallback? The draft says no.
- **OQ-N21** May one Solver_Configuration map to several Lock_File solver binary hashes (for example, one per platform)? R1.1 criterion 4 and P1.1.2 allow a set; R1.3 criterion 5 and the Cache_Key (R7.5) then use the hash actually reported. Resolved by design decision D-13: yes, one hash per platform; Results carry the reported hash (R1.1 criterion 11).
- **OQ-N22** Should R1.1 criterion 5 (no connection at Session start) follow the R1.6 fallback option, so that the option `unresolved` also applies there? Resolved: R1.1 criterion 5 switches to the R1.6 fallback, so the configured option (`local` or `unresolved`) applies; R1.6 criterion 6 also covers goals escalated after the switch.
- **OQ-N23** Should a Result_Cache hit with a non-proof label (for example, a cached timeout at a time limit at least as large as the Cluster's) or a local Invalid answer stop escalation? The draft escalates on every non-proof outcome (R1.2 criterion 3).
- **OQ-N24** Should RQ1 (R6.2) report the share of goals that the Local_Fast_Pass keeps local and the time saved, so that R1.2's performance claim also has an A5 check? Resolved: a separate RQ7 (R6.2 criterion 10) measures both, and R1.2's Verification subsection and Traceability row name `eval/scripts/rq7_escalation/`.
- **OQ-N25** What should happen to a Result whose recorded time limits do not match its Calibration (R1.4 criterion 7)? The draft keeps the Result for labelling and keeps its non-proof answer out of the Result_Cache. Alternatives: discard the Result, or keep it with a warning only.
- **OQ-N26** Should the Front_End enforce the per-goal deadline TBD-3 itself, so that every Escalated_Goal gets a Verdict even without F4 (compare NG-8)? Resolved: yes, on its local clock (R1.5 criterion 9, P1.5.7 under F3 only).
- **OQ-N27** Should event tracing (R1.5 criterion 10) run in every Session or only in CI and evaluation runs, given its cost? Resolved by design decision D-11: always on; user Sessions keep a bounded ring of the most recent `<TBD-46>` trace records, and CI and evaluation runs keep complete traces (R1.5 criteria 18 and 19).
- **OQ-N28** Should reassignment (R1.6 criterion 2) avoid Workers on which the Job already failed? The draft does not; the attempt limit TBD-6 bounds retries.
- **OQ-N29** What happens to a Result of a Job that the Coordinator already marked failed (a late Result after a heartbeat timeout, or a Job that ran across a Coordinator restart)? The draft forwards the Result, the Verdict_Recorder treats it like any other Result, and the stale Job ends at its local time limit (R1.4 criterion 5).
- **OQ-N30** Does the Coordinator persist its queue across restarts? The draft assumes that a restart loses the queue and relies on resubmission by the Front_End (R1.6 criterion 9).
- **OQ-N31** Default value of the fallback option (R1.6 criteria 5 and 6), which also applies when no connection is made at Session start (R1.1 criterion 5). The draft proposes `local`, matching local proving in why3find today.
- **OQ-N32** Does registering an already registered Worker identity replace its credential, revoke the old credential, and end its drained state? The draft assumes all three (R1.7 criterion 3, P1.7.1).
- **OQ-N33** Should a revocation also invalidate Results from that credential that were accepted earlier in the Session or stored in the Result_Cache? The draft applies a revocation only to Results processed after the Front_End receives it (R1.7 criterion 6).
- **OQ-N34** Is the operator status report (R1.7 criteria 8 and 9) a machine-readable format? If so, the format joins the R7.10 list. Resolved by design decision D-20: a canonical S-expression with a text rendering, listed among the exchanged formats of R7.10 (R1.7 criterion 12).
- **OQ-N35** Operator credential mechanism for operator commands (R1.7 criterion 11), and whether operator credentials are separate from Worker credentials (compare OQ-N11). Resolved by design decision D-9: a separate online operator command key authenticates operator commands to the Coordinator (R1.7 criterion 11, TB-13).
- **OQ-N36** Should a Portfolio with duplicate entries (the same name twice, or equal tuples under different names) be rejected (draft, R2.1 criterion 5) or merged into one entry with a warning?
- **OQ-N37** Can a Solver_Configuration option change the problem an Unpatched_Solver answers (for example, an option that reads extra input or ignores assertions)? Should options be limited to a per-prover allow-list recorded in the TCB_Document? The draft passes any option list and trusts the solver for proved-standalone (R2.1, R7.1). Meanwhile the option lists are trusted for proved-standalone under TB-12.
- **OQ-N38** At the R1.6 fallback, R2.2 criterion 20 ends every open Race, so a Result that awaits certification at that point can no longer become a winner. Should its certification continue and be allowed to set the Verdict? Resolved: certification continues for the log and the conflict rule only and never sets the Verdict, also when the stop policy (R2.2 criterion 16) ends the Race (R2.2 criterion 23).
- **OQ-N39** The earlier Glossary tied a Goal_Version to one Task_Hash, and so to one Driver and Transformation_Pipeline, but a Portfolio (R2.1) and the Local_Fast_Pass may print one Goal with several Drivers. Should Goal_Version mean the Goal at one source version, with one Task_Hash per Driver and Transformation_Pipeline? Resolved: a Goal_Version is a Goal with its Goal_Hash, the hash of its Why3 task before any Transformation_Pipeline or Driver is applied; each Job's Printed_Goal and Task_Hash follow from the Goal_Version and the Driver and Transformation_Pipeline of its Solver_Configuration, and `print_local` is defined per Result (Glossary). P1.3.1 compares `r.task_hash` with the hash of the local print for r's Driver and Transformation_Pipeline. Remaining: whether such a deterministic task hash exists (AS-33).
- **OQ-N40** Should the Lemma grammar admit `let` (cvc5 prints shared subterms with `let` by default)? The draft rejects every binder (R3.3 criterion 5), so Sharing_Solver patches must print Lemmas without `let`, at the cost of larger Lemmas. Resolved by design decision D-14: `let` stays rejected, and Sharing_Solver patches print exported Lemmas and hint terms without `let` (AS-40).
- **OQ-N41** Should a partitioned Goal_Version whose Partitions all have proved-standalone Results and whose cover is checked get a proof label without one Composed_Certificate (a new label, or proved-standalone with a TCB note)? The draft requires certified (R3.5 criterion 2), so Partitions solved by provers without a Checkable_Proof_Format cannot contribute to a proof label. Resolved for this document: R3.5 criterion 2, R5.1 and R7.1 criterion 2 already require certified, so a Verdict recorded from the leaf Results of a partitioned Goal_Version is proof-labelled only certified (a whole-Goal_Version Result of the same Race can still win under R2.2 criterion 8; R3.5 introduction). A weaker label would be a new requirement.
- **OQ-N42** When a Goal_Version's pool-item quota TBD-11 is full, should the Lemma_Pool discard new pool items (draft, R3.6 criterion 2), evict stored ones, or apply a per-source-Job quota so that one Job cannot fill the quota? Should Instantiation_Hints have a quota of their own? The draft counts Lemmas and Instantiation_Hints together (R3.6 criterion 1).
- **OQ-N43** How are the Solver_Configurations of Jobs for Escalated_Goals that are not Hard_Goals derived from why3find's prover configuration and strategy? R2.2 covers only Races on Hard_Goals, yet R1.3 requires a full Solver_Configuration in every Result. Resolved: the Scheduler derives them from why3find's prover configuration and proof strategy for the reference-time parallel pass, as recorded in the project configuration, and the Coordinator chooses none (R2.2 configuration list and criterion 3, R1.1 criteria 1 and 4).
- **OQ-N44** Does portfolio racing run on local cores when the cluster option is not configured, or after the R1.6 fallback? The draft submits Race Jobs to the Cluster only (R2.2 criterion 6). Resolved for Sessions without the cluster option: no Goal_Version is a Hard_Goal and the Front_End logs a warning (R2.2 criteria 2 and 27); local portfolio racing remains a possible future feature. Still open for the R1.6 fallback. Draft for the R1.6 fallback: no local portfolio racing; the `local` fallback runs the configuration list of Escalated_Goals that are not Hard_Goals (R1.6 criterion 5).
- **OQ-N45** Which axiom attribution rule defines the axiom set of a declared symbol (R4.1)? Candidates: the axioms of the Why3 theory that declares the symbol (needs AS-26), or an empty axiom set, so that Canonical_Names cover only source name, sort signature and definition and axioms take part in sharing only through Dependency_Sets. Soundness does not depend on the choice; a larger axiom set makes names match across goals less often. Resolved by design decision D-12: empty axiom sets (R4.1 introduction); AS-26 is not relied on.
- **OQ-N46** Should cross-goal sharing work without within-goal sharing? The import mark is defined by the within-goal sharing option (R3.1 criterion 2), so the draft rejects that configuration (R4.2 criterion 8). The alternative is a separate importing list for cross-goal sharing.
- **OQ-N47** Should an Instantiation_Hint be allowed to name the target's own Negated_Goal_Assertion within one Goal_Version (useful for existential goals) or a Partition_Assumption? Such instances are sound because the named Assertion is in the target problem; the draft rejects them with the reason category hint shape (R4.4 criterion 12) to follow the Glossary, which defines hints over Hypotheses only.
- **OQ-N48** How should a Race Result accepted after the Race deadline be certified? The draft sets its certification start to the Race deadline (R5.1), so it is certified only if certification fits in the time left and the Race stays bounded (P2.2.6). Alternatives: label such a Result unchecked at once, or let certification extend the Race up to TBD-3. For a Result accepted after its Race ended or after its certification deadline, the draft labels it unchecked at once, with no proof runs (R5.1 criterion 9, R2.2 criterion 26).
- **OQ-N49** Should the hard corpus include candidates that no repetition of the hardness measurement proves (unsolved, possibly invalid or out of reach), and should hardness also count goals that the default strategy proves only in some repetitions? The draft includes unsolved candidates with the class unsolved and treats a candidate as unsolved if any repetition fails (R6.1).
- **OQ-N50** Which CI selection rule picks the CI corpus from a release (size, per-project stratification, total time budget for `ci/diff` and `ci/eval-smoke`)? R6.1 criterion 10 leaves the rule to the corpus configuration; many differential properties in R1 to R5 quantify over the CI corpus. The design suggests per-project stratification under a time budget for `ci/diff` and `ci/eval-smoke`.
- **OQ-N51** In certify-all mode no Result is labelled proved-standalone (R5.5 criterion 2). Should a certified Local_Fast_Pass Result keep the goal local, for example by delaying escalation until its certification ends? Resolved: in certify-all mode the Front_End certifies a proof-reporting Local_Fast_Pass Result locally before escalation; a certified Result keeps the Goal_Version local, and only unchecked or checked-incomplete Results lead to escalation (R1.2 criteria 2, 8 and 9). The cache part: R7.1 criterion 6 labels cache entries with the current certify-all setting, so R1.2 criterion 4 accepts only certified entries in this mode, and R5.5 criterion 6 treats other entries as misses.
- **OQ-N52** Are the draft statistical methods of R6.2 right: paired bootstrap intervals resampling goals, geometric-mean time ratios, Wilson intervals for proportions, Holm adjustment? Partly resolved: times are censored at the budget, the proved-count difference is the primary metric of RQ1 to RQ4, and time ratios with censored times set to the budget are a sensitivity analysis (R6.2 introduction). Still open: only the choice of statistical methods, including whether resampling should be stratified by project.
- **OQ-N53** Should a certified Result_Cache hit outside certify-all mode be checked again by the Certificate_Checker in every Session (draft, R7.1 criterion 6), or be labelled from the checker outcome stored in the Certificate_Store after the manifest checks of R5.4, trusting local file integrity? The draft costs one checker run per certified cache hit.
- **OQ-N54** Is the information order of R7.2 (checked-incomplete, unchecked, Invalid, timeout, unknown, failed, unresolved) right? In particular, should Invalid rank above unchecked and checked-incomplete, so that a reported counter-model is shown before an uncertified proof? Resolved: Invalid from a Result that is not a Lemma_Dependent_Result ranks first, Invalid from a Lemma_Dependent_Result ranks below unchecked, and every accepted Invalid Result is listed in the Invalid field of `proof.json` (R7.2 introduction and criterion 11).
- **OQ-N55** Should a conflict (R2.2 criterion 19, R7.2 criterion 7) change the Creusot_CLI exit status or downgrade the Verdict? The draft keeps the Verdict (R1.5 criterion 8) and, as a draft awaiting confirmation, returns a non-zero exit status when the conflict involves a proved-standalone Result (R7.2 criterion 12, P7.2.9).

## Scope and Proposed Split

This is too big for one spec. The final document has 42 requirements with 485 acceptance criteria and 399 properties, spanning a distributed protocol, solver patches, Lean metatheory, certificate tooling and an empirical study. Each stage can ship, be verified and be evaluated on its own, so each gets a child spec with its own design and tasks.

### What stays in this umbrella spec

- The Introduction, Glossary, Assumptions, Non-goals, Trusted items (the source of `docs/tcb.toml`), Coverage of mandated requirements, and Open Questions. The ID spaces R, P, TBD, AS, OQ-N, NG and TB stay here; child specs cite these IDs and never renumber or redefine them.
- The text of every requirement, R1.1 to R7.13. A child spec implements the requirements it owns and proposes changes to their text here.
- Group R7 as the cross-cutting contract that every child spec must keep passing.
- The Traceability_Table: one `docs/traceability.toml` for the whole repository, checked by `ci/traceability` (R7.11).
- The shared harnesses and models, each with one owning child spec that the others extend: `tests/canaries/` (dp-foundations), `tests/sim/` and `verification/tla/Coordinator.tla` (dp-distribution), the Lean theorem `verdict_sound` in `proofs/lean/DP/Verdict.lean` (dp-foundations; its hypotheses map to trusted items), the certifiability theorem `certifiable` (P7.1.8) in `proofs/lean/DP/Certifiable.lean` (dp-certificates, which states it with `compose_sound`; dp-within-goal-sharing and dp-cross-goal-sharing discharge its premises `admission_sound`, `admission_sound_seq`, `canon_injective`, `cross_goal_sound` and `hint_sound` as their theorems land), the supported-fragment check `crates/dp-core/src/fragment.rs` (dp-certificates), and `proofs/lean/DP/Partition.lean` with its job `ci/lean-partition` (dp-within-goal-sharing, as part of the partitioning work of R3.5). `Partition.lean` holds `compose_partition_sound` (P5.2.7) and `verdict_sound_partition` (P3.5.9), so both theorems are owned by the partitioning work in dp-within-goal-sharing, not by dp-certificates or dp-foundations.

### Child specs, in dependency order

| Order | Child spec | Owns | Main verification artifacts | Depends on |
|---|---|---|---|---|
| 1 | dp-foundations | R7.1 to R7.13 for the proved-standalone path: labels, `proof.json`, Result_Cache keys, purity check, Lock_File and reproducible builds, TCB_Document, patch checks, parsers, traceability, mutation testing, replay of proved-standalone Verdicts, canary harness in local mode, legacy compatibility | `crates/dp-core/` (labels, verdict, cache keys), `crates/dp-formats/`, `crates/dp-replay/`, `tools/`, `deps/`, `docs/`, `tests/canaries/`, `tests/diff/legacy_compat/`, `proofs/lean/DP/Verdict.lean` | none |
| 2 | dp-evaluation, corpus milestone | R6.1, and the CI corpus that differential tests in every later spec use | `eval/corpus/`, `eval/scripts/corpus_hardness/`, `tests/diff/export_vs_why3find/` | 1 |
| 3 | dp-distribution | R1.1 to R1.7 | `verification/tla/Coordinator.tla` and trace validation, `verification/kani/`, `tests/sim/`, `tests/integration/cluster/`, `tests/diff/dist_vs_local/`, `crates/dp-frontend/`, `crates/dp-worker/` | 1, 2 |
| 4 | dp-portfolio | R2.1 to R2.3 | `crates/dp-core/src/race.rs`, Race actions in `Coordinator.tla`, `crates/dp-formats/tests/prop_config.rs`, `tests/replay/` | 3 |
| 5 | dp-certificates | R5.1 to R5.5, and the certificate and certify-all canary modes of R7.4 | `crates/dp-cert/`, `crates/dp-core/src/certification.rs`, `compose_plan.rs` and `fragment.rs`, `proofs/lean/DP/Compose.lean` and `Certifiable.lean`, `tests/certs/`, `tests/integration/certs/` | 1, 4 |
| 6 | dp-within-goal-sharing | R3.1 to R3.6, plus R4.1, because Import hygiene and admission (R3.3, R3.4) use Canonical_Names | `crates/dp-pool/`, `crates/dp-canon/`, `crates/dp-core/src/admission.rs` and `provenance.rs`, `proofs/lean/DP/Admission.lean`, `Partition.lean` (with `compose_partition_sound` and `verdict_sound_partition`, gated by `ci/lean-partition`) and `Canon.lean`, `verification/tla/Sharing.tla`, Lemma canaries | 4, 5 |
| 7 | dp-cross-goal-sharing | R4.2 to R4.4, and the cross-goal and hint canary modes of R7.4 | `crates/dp-core/src/hints.rs`, `proofs/lean/DP/Hints.lean`, `tests/diff/cross_goal_on_off/`, `tests/diff/hints_on_off/`, hint canaries | 5, 6 |
| 8 | dp-evaluation, studies | R6.2 to R6.4 | `eval/rq/`, `eval/scripts/`, `eval/raw/`, `eval/report/`, `eval/tests/` | 2, and each research question's stage: RQ1 and RQ7 on 3, RQ2 on 4, RQ3 on 6, RQ4 on 7, RQ5 on 5, RQ6 on all |

Two choices differ from the delivery order of the Introduction. Certificates (5) come before within-goal sharing (6), because no Lemma_Dependent_Result can be proof-labelled without them (R7.1 criterion 3). Canonical naming (R4.1) moves into within-goal sharing, because R3.3 and R3.4 already parse and compare Lemmas through Canonical_Names. Certificates and within-goal sharing can be designed in parallel, as long as dp-certificates delivers first.

### Scope switching

Every row of the Traceability_Table starts with the scope status deferred. A row is switched to in scope no later than the commit that adds the last artifact that its requirement's Verification lines name, and `ci/traceability` fails on a deferred row whose artifacts all exist (R7.11 criterion 12). A stage does not switch its rows when it starts. The stage that switches a row is the stage that delivers the row's last artifact, which can be later than the stage that owns the requirement; the Initial Traceability Table names that stage for every row. R7.11 criteria 2 to 4, 6 and 7 apply only to rows in scope, so the checks of later stages do not block earlier ones. The CI jobs named for a deferred row still run the checks of the artifacts that already exist, from the stage that writes them.
