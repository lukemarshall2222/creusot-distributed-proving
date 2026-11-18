# Implementation Plan: Distributed proving backend for Creusot

## Overview

The plan follows the Delivery Plan of `design.md`: dp-foundations, the corpus milestone, dp-distribution, dp-portfolio, dp-certificates, dp-within-goal-sharing (with canonical naming and, last, partitioning), dp-cross-goal-sharing, and the evaluation studies. Implementation is in Rust (all new components), OCaml only inside the why3find patch, Lean 4 for metatheory, TLA+ for protocols and Python for the Experiment_Runner, as the design fixes (D-2, C19).

Each stage writes its verification artifacts next to the code they check: Creusot contracts and Kani harnesses with the dp-core modules, Lean theorems before or with the decision functions they justify, TLA+ actions before the shells that implement them, and property, differential, simulation, integration, canary and fuzz tests in the same task group as the component. A Traceability_Table row is switched to in scope by the sub-task that adds the last artifact its Verification lines name (R7.11 criterion 12, see Notes). Each stage ends by wiring its components into the Front_End, Coordinator, Worker and Lemma_Pool shells and the Creusot_CLI backend path, extending the scope-switch self-test to the rows the stage switched, and a checkpoint. Task text cites the design's component IDs (C1–C20) and decision IDs (D-1 to D-20) where a task implements them.

Requirement references use the spec tooling's numbering (`N.c` is criterion c of "Requirement N"; R1.1–R1.7 are 1–7, R2.1–R2.3 are 8–10, R3.1–R3.6 are 11–16, R4.1–R4.4 are 17–20, R5.1–R5.5 are 21–25, R6.1–R6.4 are 26–29, R7.1–R7.13 are 30–42). Task text keeps R-IDs. Numeric parameters stay configuration named `<TBD-n>`; no task picks a value.

## Tasks

### Stage 1: dp-foundations

- [ ] 1. Scaffold the repository, pinned toolchain and CI skeleton
  - [x] 1.1 Create the Cargo workspace and crate skeletons
    - Crates `dp-core`, `dp-formats`, `dp-shell`, `dp-testkit`, `dp-frontend`, `dp-coordinator`, `dp-worker`, `dp-pool`, `dp-canon`, `dp-cert`, `dp-replay`; the other directories of the planned layout of `requirements.md` (`tests/`, `fuzz/`, `verification/tla/`, `verification/kani/`, `proofs/lean/DP/`, `eval/`, `tools/`, `docs/formats/`, `patches/`, `deps/`) are created by the first sub-task that adds a file to them, with no placeholder files, so a directory that a Verification line names exists exactly from that sub-task (R7.11 criterion 12)
    - dp-core has no dependency outside the pure-dependency allow-list of task 2.6 (R7.3)
    - _Requirements: 32.1, 32.2_
    - _Properties: 35_

  - [-] 1.2 Write the Nix flake that pins every tool (decision D-8 on OQ-N10)
    - Pinned nixpkgs; Rust toolchain; Why3, why3find and Creusot built from opam packages inside Nix; Alt-Ergo, Z3, cvc5, Ethos with the CPC signature files, Carcara with the Alethe rule list, Lean 4, TLC, Apalache, Kani, cargo-mutants, Python with the statistics libraries (TB-15)
    - OCI images for the integration cluster built by Nix; everything under `deps/` and `flake.nix`
    - As built: Ethos is pinned ten commits after the commit that cvc5 1.4.1 names, where `--reference` is not enforced; the Rust nightly of Kani comes from a second rust-overlay input; Alt-Ergo 2.6.4 is allowed as an unfree package. Open: the Alethe rule list (defined in stage 5), the two Linux platforms and the OCI images, which are not built yet
    - _Requirements: 36.1, 36.2, 36.9_
    - _Properties: 36_

  - [-] 1.3 Generate the Lock_File and implement the `ci/repro-build` checks
    - `deps/lock.toml` with the schema of Data Models (tool kinds, per-platform hashes, upstream checksum or tag with two rebuild hashes), generated from the Nix build
    - `ci/jobs/repro-build.sh`: two clean builds with equal hashes; schema and completeness against the tool list of R7.7 criterion 1; upstream checksum or double rebuild of every Unpatched_Solver and Certificate_Checker entry
    - As built: `deps/gen-lock.sh` and a partial `deps/lock.toml` (four solvers and patched why3find, aarch64-darwin). Finding: the Nix builds of the solvers and checkers are not bit-for-bit reproducible on macOS, so the two-rebuild evidence of R7.7 criterion 9 has to come from Linux or from upstream checksums. Open: the other tools and platforms, `ci/jobs/repro-build.sh`
    - _Requirements: 36.1, 36.2, 36.6, 36.9_
    - _Properties: 36_

  - [-] 1.4 Write the CI workflow skeleton (GitHub Actions, D-15)
    - Every `ci/` and `sched/` job of the R7.11 CI job list with the dependencies of the design's CI job graph, including `sched/fuzz-nightly`, which runs every target under `fuzz/` for `<TBD-24>` per target (the targets `ci/fuzz-smoke` runs briefly); `ci/traceability` depends on every other `ci/` job; every `sched/` job declares input paths and a schedule
    - One script per job under `ci/jobs/`; each discovers its inputs (specifications and `.cfg` files, Kani harnesses, fuzz targets, simulation scenarios, diff suites, canary modes), so later tasks add files without editing the workflow; a `ci/` job whose inputs do not exist yet passes with a notice (`ci/traceability` catches in-scope rows that name missing artifacts), while a `sched/` job without inputs records no run, so the rows that name it are pending (R7.11 criterion 8) rather than backed by a run that checked nothing; every job runs the components with the tracing retention setting complete (R1.5 criterion 19)
    - `ci/creusot` proves dp-core and `decode_label` with unmodified pinned why3find (legacy configuration); `ci/kani` runs every harness with the bounds recorded next to it
    - As built: `.github/workflows/ci.yml` on Linux with `ci/jobs/{patches,test,creusot,tlc,repro-build}.sh`, a `ci` development shell, and tests that fail when their tools are missing under `DP_REQUIRE_TOOLS`; `ci/repro-build` and the build of the remaining tools and images report without gating until they have passed once. Open: the other jobs of the R7.11 job list, the `sched/` jobs, `ci/traceability` as the last job, input discovery for jobs without inputs
    - _Requirements: 40.9, 40.10, 5.19_
    - _Properties: 37_

  - [ ] 1.5 Implement dp-shell with the `Env` abstraction and `SimEnv`
    - `Env` trait: transport, clock, process runner with process groups and tree kill, file store with temp-sync-rename writes, buffered append-only trace writer that is always on (D-11)
    - Tracing retention setting of each component (the `tracing_retention` option of `dp.toml` for the Front_End, the Coordinator's and each Worker's configuration): bounded, the default, keeps the most recent `<TBD-46>` trace records of the component and discards older ones; complete, which every CI job and every Experiment_Runner run sets, keeps every record; `<TBD-46>` is a named configuration parameter with no committed default
    - Production adapters: ZeroMQ TCP, OS clock, POSIX process groups, filesystem
    - `SimEnv`: one seeded discrete-event scheduler, virtual clocks, simulated network with the R1.6 fault model (crashes and restarts, partitions, delays up to D, duplicates, reordering, loss), scripted stub solvers, stub proof producers and stub checkers, exact seed replay and fault-schedule shrinking
    - _Requirements: 32.2, 5.10, 4.5, 5.5, 5.18, 5.19_
    - _Properties: 10, 11_

  - [ ] 1.6 Write unit tests for the dp-shell production adapters
    - Atomic writes under injected crash points, tree kill of a child that ignores termination and spawns children
    - Trace writer under both tracing retention settings with more than `<TBD-46>` records (a test value): the bounded setting keeps exactly the last `min(n, <TBD-46>)` records in emission order, the complete setting keeps all n (P1.5.15); this is the `crates/dp-shell/` check that R1.5's Verification lines name (`ci/test`)
    - _Requirements: 4.5, 5.5, 5.10, 5.18, 5.19_

  - [ ] 1.7 Create dp-testkit with the shared test configuration
    - Shared proptest configuration (at least 100 cases, raised by nightly jobs through an environment variable), committed `proptest-regressions/` location, Hypothesis profile in `eval/tests/conftest.py` for `eval/tests/metrics/`
    - One module per generator of the design's generator table; each module is filled by the first task below that names it
    - _Requirements: 40.4_
    - _Properties: 37_

- [ ] 2. Implement the traceability, TCB, purity, patch and mutation tooling
  - [ ] 2.1 Write `docs/traceability.toml` and the `docs/traceability.md` generator
    - One row per requirement from the Initial Traceability Table of `requirements.md` with Class, Properties, functions, trust items and the level-artifact-job triples of its Verification lines; every row starts with scope status deferred and is switched to in scope by the sub-task that delivers the last artifact its Verification lines name (R7.11 criterion 12, see Notes)
    - Canonical TOML printer; generator of `docs/traceability.md`
    - _Requirements: 40.1, 40.5, 40.11_
    - _Properties: 37_

  - [ ] 2.2 Implement `tools/traceability-check/` with scope-aware checking (C20)
    - Parse Verification lines of `requirements.md` into (level, artifact, job) triples; compare with the table rows and Property IDs (R7.11 criteria 2, 4, 6, 7); job outcomes on the commit and acceptable scheduled runs within `<TBD-25>` days from the CI API (criterion 3, TB-18); pending report and release-tag failure (criterion 8); job list, `ci/traceability` dependencies and declared input paths (criterion 10); scope status values (criterion 11)
    - Apply criteria 2, 3, 4, 6 and 7 only to rows whose scope status is in scope (R7.11); fail on a deferred row whose Verification lines name only existing artifact paths (criterion 12); an artifact path exists on a commit when it is a tracked file or a directory that holds a tracked file
    - Format registry check (R7.10 criterion 7): every entry of `docs/formats/index.toml` names an existing grammar file, codec module, round-trip test and fuzz target; an exchanged format of R7.10's list without an entry fails once a requirement that the list cites for it is in scope, or once R7.10 is in scope for a format that cites none, such as `dispatch` (see Notes, format registry)
    - Mutation target and exclusion checks (R7.13 criteria 5 to 7)
    - Switch rows R7.11 and R7.13 to in scope with the switch step (Notes): `tools/traceability-check/` is the last artifact of both rows (R7.11 criterion 12)
    - _Requirements: 40.2, 40.3, 40.4, 40.6, 40.7, 40.8, 40.10, 40.11, 40.12, 39.7, 42.5, 42.6, 42.7_
    - _Properties: 37_

  - [ ] 2.3 Write the traceability-checker self-test with seeded mutations
    - **Property 37: Every requirement is mechanically verified and the tests catch bugs** (traceability part)
    - Unit tests plus a self-test on mutated copies of `requirements.md`, `docs/traceability.toml` and the workflow: every change of P7.11.6 must fail the check, including scope-aware cases (a missing artifact fails only for in-scope rows)
    - Scope-switch cases (P7.11.11): adding the last missing artifact of a deferred row without switching the row fails `ci/traceability`, a missing artifact of a deferred row alone does not, and a directory artifact counts as existing only when it holds a tracked file
    - **Validates: Requirements 40.1, 40.2, 40.3, 40.4, 40.5, 40.6, 40.7, 40.8, 40.9, 40.10, 40.11, 40.12, 42.5, 42.6, 42.7**

  - [ ] 2.4 Write `docs/tcb.toml` and implement `tools/tcb-check/` (C20)
    - Items from Trusted items, Assumptions, Non-goals and F1–F4 of `requirements.md`, including AS-35 to AS-40 with status To check and TB-23 as a trusted item (part of NG-2, every label); checker entries, `checker_output`, `outcome_label` and `information_order` tables (Data Models)
    - `tools/tcb-check/` matches items against the Lock_File, the Trust subsections and the trust items of `docs/traceability.toml`, and regenerates `docs/tcb.md`
    - _Requirements: 37.1, 37.2, 37.3, 37.4, 37.5, 37.6, 37.7, 37.8_
    - _Properties: 36_

  - [ ] 2.5 Write self-tests for `ci/repro-build`, `tools/tcb-check/` and `tools/patch-check/`
    - **Property 36: Builds are reproducible and the trust story is consistent**
    - Seeded mutations: a Lock_File entry without hash, a missing tool, a patched build marked Unpatched_Solver, a trusted item without reason or requirement ID, a Trust-subsection ID missing from `docs/tcb.toml`, a patch without header fields or over `<TBD-21>` lines
    - **Validates: Requirements 36.1, 36.2, 36.6, 36.8, 36.9, 37.1, 37.2, 37.5, 37.6, 37.7, 37.8, 38.1, 38.2, 38.3, 38.4**

  - [ ] 2.6 Implement `tools/purity-check/` (C20)
    - Dependency closure of dp-core against the allow-list; syntax scan for banned interfaces (file, network, clock, randomness, threads, processes, global mutable state); `registry.toml` against the public functions of dp-core and the in-scope rows (R7.3)
    - _Requirements: 32.2, 32.3, 32.4_
    - _Properties: 35_

  - [ ] 2.7 Implement `tools/patch-check/` (C20)
    - Patch headers (purpose, requirement IDs, upstream status), application to the pinned upstream version, build, changed lines per dependency at most `<TBD-21>`, Lock_File hashes of patched builds, no patched build marked Unpatched_Solver (R7.9, R7.7 criterion 8)
    - Switch row R7.9 to in scope with the switch step (Notes): `tools/patch-check/` is its only artifact (R7.11 criterion 12)
    - _Requirements: 38.1, 38.2, 38.3, 38.4, 36.8_
    - _Properties: 36_

  - [ ] 2.8 Implement `tools/mutants/` and the `sched/mutants-weekly` gate (C20)
    - cargo-mutants configuration for dp-core, dp-cert, dp-formats, dp-canon, dp-frontend and dp-replay; per-crate score threshold `<TBD-19>`; `targets.toml` generated from the `functions` fields of in-scope Soundness-critical rows; `exclusions.toml` schema whose equivalence artifacts (Creusot proof or Kani harness with recorded bounds) are run against their mutant; registered decision functions must be killed by dp-core or `tests/sim/` tests (R7.3 criterion 5)
    - _Requirements: 42.1, 42.2, 42.3, 42.4, 32.5_
    - _Properties: 35, 37_

  - [ ] 2.9 Write self-tests for `tools/purity-check/`
    - **Property 35: The decision core is pure, deterministic and exercised** (purity part)
    - Seeded violations: a dp-core dependency outside the allow-list, a call to each banned interface, a registry entry naming no public function, an in-scope decision function without entry
    - **Validates: Requirements 32.2, 32.3, 32.4**

  - [ ] 2.10 Write self-tests for the mutation gate
    - **Property 37: Every requirement is mechanically verified and the tests catch bugs** (mutation part)
    - A fixture crate with a surviving mutant of a soundness-critical target (gate fails), a matching exclusion with a passing Kani artifact (gate passes), a failing artifact (gate fails), a score below the threshold (gate fails)
    - **Validates: Requirements 42.1, 42.2, 42.3, 42.4**

- [ ] 3. Implement dp-formats for the stage-1 formats
  - [-] 3.1 Implement the canonical S-expression codec and hashing (C16, D-7, TB-1)
    - Length-prefixed atoms, strict parser with the size limit `<TBD-22>` checked before parsing, duplicate-field and trailing-byte rejection, errors naming the format and a byte offset; canonical printer; pretty printer
    - `H(tag, x…)` with the domain tags of Data Models; `docs/formats/index.toml` registry with grammar file, codec module, round-trip test, fuzz target and size limit per format; each format's entry is added by the sub-task that adds the last of its four artifacts (R7.10 criterion 7, see Notes)
    - As built: `sexp.rs` (strict parser, canonical and pretty printers, stream prefix parser), `hash.rs`, `docs/formats/canonical-sexp.abnf`, property tests. Open: the fuzz target, `docs/formats/index.toml`, and `<TBD-22>`, which callers pass as a parameter
    - _Requirements: 39.1, 39.2, 39.3, 39.5, 39.6_
    - _Properties: 34_

  - [ ] 3.2 Implement the stage-1 format modules
    - `proof.json` extension in `crates/dp-formats/src/proofjson.rs` (`DpEntry` as a base64 member `dp`, `dp-lock`, conflict and Invalid fields, absent markers), Lock_File, Result_Cache entry, Result message, project configuration `dp.toml` (canonical TOML, holding the Solver_Configuration file grammar and the `tracing_retention` option), `dispatch` (the why3find bridge requests and replies, D-2, D-3), event trace record, canonical JSON (RFC 8785) helpers; grammars in `docs/formats/`
    - `decode_label` with its Creusot contract: any string outside the label set decodes to a non-proof outcome
    - _Requirements: 39.1, 39.4, 10.5, 31.1, 31.3, 31.7, 31.11, 34.3, 36.5_
    - _Properties: 30, 34_

  - [ ] 3.3 Implement the canonical-input check (A3)
    - `crates/dp-formats/src/canonical_check.rs`: `print(parse(b)) = b` for Result messages, Result_Cache entries, the Lock_File and `proof.json` extension fields; failures discard the input (R1.3 criterion 8)
    - _Requirements: 39.8, 3.8_
    - _Properties: 34_

  - [ ] 3.4 Write round-trip, strictness and fuzz tests for the stage-1 formats
    - **Property 34: Exchanged formats parse strictly and print canonically**
    - `crates/dp-formats/tests/roundtrip.rs` (the three round-trip laws, offsets, size limit, duplicates, trailing bytes, canonical-only acceptance) for every stage-1 format, `dispatch` included; fuzz targets `fuzz/proofjson/`, `fuzz/config_parser/`, `fuzz/cache_entry/`, `fuzz/lock_file/`, `fuzz/result_message/` (the parser alone; task 15.13 puts it in front of `accept_result`), `fuzz/dispatch/` and `fuzz/trace_record/`, run by `ci/fuzz-smoke` and `sched/fuzz-nightly`
    - Registry entries in `docs/formats/index.toml` for the stage-1 formats, each naming its grammar file, codec module, round-trip test and fuzz target
    - **Validates: Requirements 39.1, 39.2, 39.3, 39.4, 39.5, 39.6, 39.7, 39.8**

  - [ ] 3.5 Write property tests for `proof.json` encoding
    - **Property 30: proof.json and the Creusot_CLI report labels faithfully** (encoding part)
    - `crates/dp-formats/tests/prop_proofjson.rs`: label, conflict, Invalid, replay fields with absent markers and import fields survive write and read; unknown labels decode to non-proof
    - **Validates: Requirements 31.1, 31.3, 31.7, 31.10, 31.11, 10.5, 10.7**

  - [ ] 3.6 Write snapshot tests for the pretty printer
    - Pretty-printed forms of each stage-1 format re-parse to the same value
    - _Requirements: 39.3_

- [ ] 4. Implement the dp-core decision core for the proved-standalone path
  - [-] 4.1 Implement the machine interface, trace actions and the Front_End machine skeleton (C4)
    - Create `machine.rs` (`Machine`, `Step`), `trace.rs` (`TlaAction`, generator of `verification/tla/actions.toml`) and `frontend.rs` (the Front_End machine of the Scheduler and Verdict_Recorder) handling Session start, Goal announced, print facts, cache facts, prover call, local process exit and final write; inputs carry `now`; ordered collections only, as strictly increasing vectors (Creusot has no `BTreeMap` contracts), with state rules as Creusot type invariants
    - Open: the generator of `verification/tla/actions.toml`, and action names taken from `Coordinator.tla` (task 14)
    - _Requirements: 32.1, 32.2, 5.10_
    - _Properties: 10, 35_

  - [x] 4.2 Implement `labels.rs` (`classify`, `most_informative`) with Creusot contracts
    - why3find's classification of prover answers (AS-31); information order read from the TCB_Document
    - Open: the information order is fixed in `labels.rs`; comparing it with the TCB_Document waits for `docs/tcb.toml` (task 2.4)
    - _Requirements: 31.5, 31.6_
    - _Properties: 30_

  - [x] 4.3 Implement `verdict.rs` for the proved-standalone path with Creusot contracts (C4)
    - `accept_result` for local Results, `label` with the fail-safe rule (proved-standalone only under every condition of R7.1 criterion 2; every Result that needs certification is unchecked until stage 5), `record_verdict` (at most one Verdict, never changed), `cache_label`, `final_verdicts`; contracts state P7.1.1, P7.1.3, P7.1.5, P7.1.6
    - As built: `final_verdicts` is `final_label`, the end-of-Session label of one Goal; four planted weakenings of `label` and `record_verdict` were each rejected by Creusot
    - _Requirements: 30.1, 30.2, 30.4, 30.5, 30.6, 31.4, 5.1, 5.8, 5.13, 41.2_
    - _Properties: 1_

  - [ ] 4.4 Implement `cache_key.rs` with Creusot contracts and a Kani harness (C15)
    - `encode_key` (length-prefixed canonical S-expression of the R7.5 lookup fields), `lookup_ok`, `validate_entry`; `verification/kani/cache_key.rs` for injectivity
    - _Requirements: 34.1, 34.2, 34.5, 34.9, 34.10, 34.11_
    - _Properties: 29_

  - [ ] 4.5 Implement `config.rs` `validate_config` for the project configuration
    - Create `crates/dp-core/src/config.rs`; checks: the cluster option value (well-formedness through the address parser that task 4.11 adds, D-13), Local_Fast_Pass budget with default `<TBD-1>`, fallback option, invalid-answer policy, certify-all value, cross-goal without within-goal sharing; heartbeat margin `<TBD-31>` + D < min(`<TBD-5>`, `<TBD-7>`, `<TBD-32>`)
    - _Requirements: 1.6, 2.5, 2.7, 6.7, 6.18, 9.15, 9.17, 18.8, 25.5_
    - _Properties: 5_

  - [ ] 4.6 Write property tests for labelling and Verdict recording
    - **Property 1: Proof labels come only from the two routes** (proved-standalone and legacy parts)
    - `crates/dp-core/tests/prop_verdict.rs` with umbrella `property_1_proof_label_routes` and `prop_record.rs`; fills `arb_session_events` in dp-testkit
    - **Validates: Requirements 30.1, 30.2, 30.4, 30.5, 41.2**

  - [ ] 4.7 Write property tests and run the Kani harness for the Cache_Key
    - **Property 29: Cache hits reuse only what nothing could have changed** (key part)
    - `crates/dp-core/tests/prop_cache_key.rs` with a weak test hash; fills `arb_cache_entry`
    - **Validates: Requirements 34.1, 34.2, 34.5, 34.9, 34.10, 34.11**

  - [ ] 4.8 Write property tests for configuration validation
    - **Property 5: Invalid configurations and pin mismatches fail closed** (configuration part)
    - `crates/dp-formats/tests/prop_config.rs` and `crates/dp-core/tests/heartbeat_margin.rs`; fills `arb_config_text`
    - **Validates: Requirements 1.6, 2.7, 6.7, 6.18, 9.17, 18.8, 25.5**

  - [ ] 4.9 Write determinism tests for dp-core
    - **Property 35: The decision core is pure, deterministic and exercised** (determinism part)
    - `crates/dp-core/tests/determinism.rs`, the first file under `crates/dp-core/tests/`: every registered function and every stage-1 decision function (`classify`, `most_informative`, `accept_result`, `label`, `record_verdict`, `cache_label`, `final_verdicts`, `encode_key`, `lookup_ok`, `validate_entry`) returns equal outputs across processes and hash seeds; each later switch step adds the functions it registers
    - Switch row R7.3 to in scope with the switch step (Notes): `crates/dp-core/tests/` is its last artifact (R7.11 criterion 12)
    - **Validates: Requirements 32.1, 32.2**

  - [ ] 4.10 Write property tests for `most_informative` and `classify`
    - **Property 30: proof.json and the Creusot_CLI report labels faithfully** (label-function part)
    - In `prop_verdict.rs`: minimum in the information order, independent of input order; every other prover answer maps as why3find maps it; recorded labels never depend on labels read from `proof.json`
    - **Validates: Requirements 31.4, 31.5, 31.6**

  - [ ] 4.11 Implement cluster option precedence and the Proof_Server address grammar (D-13)
    - `docs/formats/cluster-address.abnf`: `tcp://<host>:<port>` with a DNS name, an IPv4 literal or a bracketed IPv6 literal as host and a decimal port from 1 to 65535; address parser in the dp-formats project-configuration module, wired into `validate_config` for the cluster option check of task 4.5
    - Precedence function over the command-line flag, `DP_CLUSTER` and `dp.toml`: the first one set wins, and the cluster option is not configured when none is set; the Session shell calls it (task 6.1)
    - Property tests in `crates/dp-formats/tests/prop_config.rs` over generated combinations of set and unset flag, `DP_CLUSTER` and `dp.toml` values (P1.1.8) and over generated strings for the address grammar, every rejected string falling under R1.1 criterion 6 (P1.1.9)
    - _Requirements: 1.9, 1.10, 1.6_
    - _Properties: 5_

- [ ] 5. Write the dependency patches and the why3find bridge
  - [x] 5.1 Write the why3find dispatch patch (`patches/why3find/`, C2, D-3, AS-37, TB-23)
    - `why3find prove --dispatch`: Goal announcements with goal path and Goal_Hash input, prover calls tagged with the strategy step, cache reads and writes, legacy `proof.json` written only from dp-frontend's answers; bridge requests for printing with a Driver and Transformation_Pipeline with task count, deterministic untransformed task print, detected provers with command lines, Calibration; a closed pipe is fatal
    - As built: Calibration runs stay inside why3find and use its own cache; the bridge answers only for Goals announced in the current run
    - Since extended: the prover list carries the input file extension, and `read-answer` has Why3 read the answer of a recorded run with the prover's driver
    - _Requirements: 3.3, 3.10, 10.5, 31.3, 34.7, 41.1, 38.1_
    - _Properties: 30, 33_

  - [x] 5.2 Write the Creusot_CLI backend patch (`patches/creusot/`, C1, D-4, AS-2)
    - Backend option of `cargo creusot prove` that runs `dp-frontend prove` with the arguments of `why3find prove`, forwards labels, summary and conflicts, and returns dp-frontend's exit status; without the option Creusot_CLI runs unmodified why3find
    - As built: the option is `cargo creusot --backend PROGRAM` (Creusot 0.15 has no `prove` sub-command); Creusot proves dp-core through dp-frontend with it
    - _Requirements: 31.2, 31.9, 31.12, 38.1_
    - _Properties: 30_

  - [-] 5.3 Add the patched builds to the Lock_File and implement the bridge client
    - Lock_File entries for patched why3find and Creusot (never Unpatched_Solver kinds); `crates/dp-shell/src/why3find.rs`: client of the `dispatch` format over a pipe to the why3find child process, with no FFI (D-2), used by dp-frontend, dp-worker and dp-replay
    - Bridge request that addresses a Goal by its goal path without a proving run, for dp-replay (7.1); dp-frontend starts the patched why3find by its explicit path (`DP_WHY3FIND` in the development shell), because Creusot's wrapper puts the unpatched one on `PATH`
    - As built: `dp-formats/src/dispatch.rs`, `dp-shell/src/why3find.rs` with tests against the patched why3find, and a Lock_File entry for patched why3find. Open: the Lock_File entry for patched Creusot, the goal-by-path request, and use by dp-worker and dp-replay
    - _Requirements: 38.2, 38.4, 36.1_
    - _Properties: 36_

- [ ] 6. Implement dp-frontend in local mode
  - [-] 6.1 Implement the Session shell with configuration loading and pin checks (C3)
    - Cluster option precedence through the function of task 4.11 (flag, `DP_CLUSTER`, `dp.toml`; D-13), `validate_config`, tracing retention setting from `tracing_retention`, Lock_File hash check of every local executable at Session start and before each start, TCB_Document and registry loading, start of `why3find prove --dispatch` and the dispatch loop driving the Front_End machine; errors stop the Session before any solver starts and leave `proof.json` and `.why3find` unchanged
    - As built: a thin `dp-frontend prove` with the Lock_File checks at Session start and before each solver start, configured by `DP_WHY3FIND` and `DP_LOCK`. Open: `dp.toml`, `validate_config`, the cluster option, tracing, the TCB_Document and registry, and the loop over the dp-core Front_End machine
    - _Requirements: 36.3, 36.7, 1.6, 1.2, 1.8, 1.9, 5.18_
    - _Properties: 5_

  - [-] 6.2 Implement local solver runs and dispatch answers
    - Solvers started directly in process groups with Why3's prover command lines and why3find's Calibration (D-16, AS-39); answer Valid only for proof-labelled Verdicts and the classified answer otherwise
    - As built: solvers run in process groups with Why3's command lines; Valid is answered only for proved-standalone. The version of a Solver_Configuration is the one the Lock_File pins for the binary, because Creusot's Why3 configuration states none. Open: why3find's Calibration (reference limits are used as local ones); answer patterns and input file extensions are restated from the pinned drivers for cvc5, cvc4, Z3 and Alt-Ergo 2.6 and should come from the bridge
    - Since changed: answers are read by Why3 through `read-answer`, and why3find's Calibration scales local time limits; the restated answer patterns are gone
    - _Requirements: 41.1, 41.2, 36.7_
    - _Properties: 1, 33_

  - [-] 6.3 Implement `crates/dp-frontend/src/accept.rs` facts for local Results (A3)
    - `print_local` through the bridge, Task_Hash, task count, canonical-form check, solver binary hash against `unpatched(Lock_File)`
    - As built, inside `session.rs`: `print_local` through the bridge, Task_Hash, task count and the solver binary hash. Open: the separate `accept.rs` module and the canonical-form check
    - _Requirements: 3.3, 3.5, 3.10, 39.8, 30.5_
    - _Properties: 3_

  - [ ] 6.4 Implement the Result_Cache (`crates/dp-frontend/src/cache.rs`, C15)
    - Entries in `.dp-cache/`; legacy-layout writes only for proved-standalone (answer why3find's cache-write request with store or skip, AS-35); lookup compares stored lookup fields, then validation fields, then labels the entry under R7.1 criterion 6; entries written by unmodified why3find are misses
    - _Requirements: 34.3, 34.4, 34.6, 34.7, 34.9, 34.10, 34.11, 30.6, 41.3_
    - _Properties: 29_

  - [ ] 6.5 Implement the final `proof.json` write and the Creusot_CLI report
    - Atomic rewrite after why3find's legacy write with `dp` members and `dp-lock`; conflict and Invalid fields; absent markers; report of labels, counts per label and conflicts; non-zero exit for a conflict whose proof is proved-standalone
    - _Requirements: 10.1, 10.4, 10.5, 10.7, 31.1, 31.2, 31.3, 31.7, 31.8, 31.9, 31.11, 31.12, 36.5, 5.2, 5.14_
    - _Properties: 30_

- [ ] 7. Implement offline replay for proved-standalone Verdicts
  - [ ] 7.1 Implement `crates/dp-replay/` without networking code (C17)
    - Canonical decode of `proof.json`; Lock_File hash comparison at the start of the report; re-print through the bridge and Task_Hash comparison; local Calibration and re-run with the recorded seed and options at the scaled limit times `<TBD-20>`; one entry per Goal_Version; exit status zero only when every proof-labelled Verdict is confirmed, reserved status for contradicted; absent markers and unknown labels fail; certified Verdicts are reported not confirmed until task 23.7
    - _Requirements: 35.1, 35.2, 35.5, 35.6, 35.7, 35.8, 35.9, 10.8, 31.10_
    - _Properties: 31_

  - [ ] 7.2 Write the replay and tamper tests for proved-standalone Sessions
    - **Property 31: Offline replay confirms exactly what the evidence supports** (proved-standalone part)
    - `tests/replay/` and `tests/replay/tamper/`, run by `ci/replay` in a container without network
    - **Validates: Requirements 35.1, 35.2, 35.5, 35.6, 35.7, 35.8, 35.9, 10.8, 31.10**

- [ ] 8. Write the Lean foundations
  - [ ] 8.1 Prove `verdict_sound` in the Lean trace model
    - **Property 2: Verdict soundness in the Lean trace model** (unpartitioned part)
    - `proofs/lean/DP/Model.lean` (AS-12), `Trace.lean` with the reference `label`, `Verdict.lean` with `SolverSound`, `WorkerHonest`, `NoCollision`, `CheckerSound` and `verdict_sound` (P7.1.2)
    - **Validates: Requirements 30.1, 30.2, 30.3**

  - [ ] 8.2 Implement the `ci/lean` and `ci/lean-partition` checks
    - Lake project whose main library globs `DP/*.lean` except `Partition.lean`, which has its own target; fail on `sorry` and on axioms outside `propext`, `Classical.choice`, `Quot.sound` in `#print axioms`; import-closure check that `Verdict.lean` imports none of `Admission`, `Canon`, `Hints`, `Compose`, `Partition`, `Certifiable`
    - _Requirements: 30.1, 30.2, 30.3_
    - _Properties: 2_

  - [ ] 8.3 Build the executable Lean references (`dp-lean-ref`)
    - `proofs/lean/DP/Ref/Main.lean` reads canonical S-expressions, runs a named reference definition and prints the result; first entry: `label`; Nix package for the binary
    - _Requirements: 30.5, 23.11_
    - _Properties: 1, 27_

  - [ ] 8.4 Write the `label` differential test against the Lean reference
    - **Property 1: Proof labels come only from the two routes** (Lean reference part)
    - `tests/diff/label_lean_vs_rust/` on generated Session events
    - **Validates: Requirements 30.2, 30.4, 30.5**

- [ ] 9. Build the canary harness and the legacy differential tests
  - [ ] 9.1 Implement the canary generators and registry (C18)
    - `tests/canaries/`: specification mutator for Creusot postconditions, preconditions, loop invariants and type invariants; hand-written invalid goals; witness production (cvc5 models as definitions with Ethos-checked CPC validity proofs, form a; finite evaluation only for finite uninterpreted sorts, form b); exclusion list with reason no witness; cache canaries; `registry.toml`, `modes.toml` listing every required mode, `exclusions.toml`
    - _Requirements: 33.1, 33.2, 33.3, 33.4, 33.14_
    - _Properties: 32_

  - [ ] 9.2 Implement per-run witness re-validation (A3)
    - **Property 32: Canaries never reach a proof label** (witness part)
    - `tests/canaries/witness.rs` re-validates every witness and Lemma-canary model in its recorded form on every run
    - **Validates: Requirements 33.2, 33.3, 33.14**

  - [ ] 9.3 Implement the canary harness for the local and warm-cache modes
    - **Property 32: Canaries never reach a proof label** (local and warm-cache modes)
    - Every Canary_Goal in every implemented mode with `<TBD-40>` seeds; cache canaries through `.dp-cache/`; unmodified pinned why3find after each Session (R7.5 criterion 8); failure on any proof label, on an enabled mode that did not run, on a missing required mode, and on fewer than `<TBD-44>` quantified Canary_Goals per mode; per-mode report of quantified goals and exclusion shares
    - **Validates: Requirements 33.5, 33.6, 33.7, 33.15, 33.16, 33.17, 34.8**

  - [-] 9.4 Write the legacy-compatibility differential test
    - **Property 33: Turning a feature on or off never flips a proof against a counter-model** (legacy part)
    - `tests/diff/legacy_compat/`: legacy configuration against unmodified pinned why3find within the `<TBD-26>` band, also from files that unmodified why3find wrote; runs on fixture projects from Creusot's test suite until task 12.7 provides the CI corpus
    - Switch row R7.12 to in scope with the switch step (Notes): `tests/diff/legacy_compat/` is its only artifact (R7.11 criterion 12)
    - **Validates: Requirements 41.1, 41.3**
    - As built: `tests/diff/legacy_compat/` compares the goals proved by dp-frontend and by unmodified why3find on Creusot's example files; all 24 files and 836 goals agree. Open: the CI corpus selection, the tolerance band `<TBD-26>`, cache and `proof.json` comparisons, and the row switch

  - [ ] 9.5 Write the legacy-reader differential test
    - **Property 30: proof.json and the Creusot_CLI report labels faithfully** (legacy reader part)
    - `tests/diff/legacy_reader/`: unmodified why3find reads non-proof entries as not proved and ignores extension fields; the legacy layout holds only proved-standalone entries
    - Switch row R2.3 to in scope with the switch step (Notes): `tests/diff/legacy_reader/` is its last artifact, after `prop_proofjson.rs` (3.5) and `tests/replay/` with `tamper/` (7.2); the portfolio cases of tasks 19.8 and 19.9 extend these files later (R7.11 criterion 12)
    - **Validates: Requirements 31.3, 10.5, 34.7**

  - [ ] 9.6 Write the warm-cache differential test
    - **Property 33: Turning a feature on or off never flips a proof against a counter-model** (cache part)
    - `tests/diff/cache_on_off/`: runs with a warm cache that differs in one lookup or validation field
    - **Validates: Requirements 34.5, 34.9**

- [ ] 10. Wire stage 1 end to end
  - [ ] 10.1 Wire the Creusot_CLI backend path to dp-frontend local mode
    - `cargo creusot prove` with the backend option runs `dp-frontend prove`, which drives dispatch-mode why3find, the Result_Cache and the final `proof.json`; with every feature off this is the legacy configuration (R7.12)
    - _Requirements: 31.2, 31.12, 41.1, 1.2_
    - _Properties: 30, 33_

  - [ ] 10.2 Write the Creusot_CLI output snapshot test
    - **Property 30: proof.json and the Creusot_CLI report labels faithfully** (display part)
    - `tests/integration/cli_labels/`: labels, counts per label, conflict display and exit status
    - **Validates: Requirements 31.2, 31.9, 31.12**

  - [ ] 10.3 Write the configuration and pin integration tests
    - **Property 5: Invalid configurations and pin mismatches fail closed** (Session part)
    - `tests/integration/config/` and `tests/integration/pins/`: no solver process starts, the offending value or executable and both hashes are reported, the exit is non-zero, `proof.json` and `.why3find` stay byte-identical
    - **Validates: Requirements 1.6, 2.7, 6.7, 9.17, 25.5, 36.3**

  - [ ] 10.4 Extend the scope-switch self-test to the stage-1 rows
    - In the self-test of task 2.3, seeded mutations on a copy of the repository for each row that stage 1 switched (R7.11 and R7.13 in 2.2, R7.9 in 2.7, R7.3 in 4.9, R7.12 in 9.4, R2.3 in 9.5): setting the row back to deferred fails `ci/traceability` (R7.11 criterion 12), and deleting one of its artifacts fails it (criterion 2)
    - _Requirements: 40.2, 40.12_
    - _Properties: 37_

- [ ] 11. Checkpoint: dp-foundations gate
  - `ci/repro-build`, `ci/patches`, `ci/purity`, `ci/tcb-check`, `ci/lean` (`verdict_sound` with its import-closure check), `ci/creusot` (labels, verdict, cache key, `decode_label`), `ci/kani` (cache key), `ci/test`, `ci/fuzz-smoke`, `ci/diff` (`legacy_compat/`, `legacy_reader/`, `cache_on_off/`, `label_lean_vs_rust/`), `ci/replay`, `ci/canaries` (local and warm-cache modes), `ci/integration-cluster` (local-mode `cli_labels/`, `config/`, `pins/`) and `ci/traceability` pass, with rows R7.3, R7.9, R7.11, R7.12, R7.13 and R2.3 in scope, and `sched/mutants-weekly` has a passing run
  - Ensure all tests pass, ask the user if questions arise.

### Stage 2: dp-evaluation, corpus milestone

- [ ] 12. Implement the Corpus_Exporter and the CI corpus
  - [-] 12.1 Implement the corpus configuration and metadata formats
    - Corpus configuration (projects at pinned commits, license allow-list, CI selection rule field of OQ-N50, reference Portfolio); corpus metadata and exclusion-list codecs in dp-formats with their grammars, round-trip cases in `crates/dp-formats/tests/roundtrip.rs` (P6.1.3), `fuzz/corpus_metadata/` (run by `ci/fuzz-smoke` and `sched/fuzz-nightly`) and registry entries; pre-registration template `eval/rq/templates/corpus.toml` with the R6.2 criterion 17 fields and `<TBD-1>`, `<TBD-16>`, `<TBD-36>`, `<TBD-17>` as named placeholders, which the parser reports as missing fields for any non-smoke run
    - As built so far: `eval/corpus/corpus.toml` (Creusot's examples and CreuSAT at pinned commits), `eval/scripts/corpus_hardness.py`, `DP_RECORD` and `DP_EXPORT_DIR` in dp-frontend, and an exploratory run in `eval/raw/exploratory-2026-11-17/`. Finding: every goal of both sources is proved by the default strategy (CreuSAT: 601 goals, 587 by the first sequential pass, 14 after it, none unsolved), so neither source has goals beyond the default strategy's reach. Open: the exporter proper, hardness classes with `<TBD-16>` and `<TBD-36>`, metadata, releases, licenses
    - _Requirements: 26.2, 26.3, 27.2, 27.17, 39.1, 39.4, 39.7_
    - _Properties: 34, 38_

  - [ ] 12.2 Implement the Corpus_Exporter (`eval/corpus/`, C19)
    - Drives dp-frontend in dispatch mode to capture the exact Printed_Goal of every candidate for each Driver and Transformation_Pipeline of the reference Portfolio; records metadata; applies hardness selection and the license rule; exclusion list with one reason each; every Canary_Goal with the canary flag; canonical release whose version is its content hash; CI corpus by the selection rule plus every Canary_Goal
    - _Requirements: 26.1, 26.2, 26.3, 26.5, 26.6, 26.7, 26.8, 26.9, 26.10_
    - _Properties: 38_

  - [ ] 12.3 Write the hardness measurement script (`eval/scripts/corpus_hardness/`)
    - Pinned dependencies, derived seeds, `<TBD-17>` repetitions, thresholds `<TBD-16>` and `<TBD-36>`, run manifest `eval/raw/corpus/<run-id>/manifest.json` and raw records `eval/raw/corpus/<run-id>/records.jsonl` (canonical JSON); reads `eval/rq/corpus.toml` and refuses a non-smoke run while that file is absent
    - _Requirements: 26.4, 28.1, 28.2, 28.3, 28.4, 28.9_
    - _Properties: 38_

  - [ ] 12.4 Write the corpus tests
    - **Property 38: The corpus is exact, deterministic and complete** (P-ID tests)
    - `eval/corpus/tests/` for P6.1.2 and P6.1.4 to P6.1.7: byte-identical exports, version identifiers, hard-exactly selection, canary inclusion, one exclusion reason per candidate
    - **Validates: Requirements 26.2, 26.3, 26.4, 26.5, 26.6, 26.7, 26.8, 26.9, 26.10**

  - [ ] 12.5 Write the export differential test
    - **Property 38: The corpus is exact, deterministic and complete** (exactness part)
    - `tests/diff/export_vs_why3find/`: dispatch-mode prints equal the bytes captured from unmodified why3find and hash to the recorded Task_Hash
    - **Validates: Requirements 26.1**

  - [ ] 12.6 Write the umbrella property test for Property 38
    - **Property 38: The corpus is exact, deterministic and complete**
    - `property_38_corpus_exact` in `eval/corpus/tests/` over generated pinned-input fixtures
    - **Validates: Requirements 26.1, 26.4, 26.8, 26.9**

  - [ ] 12.7 Generate the bootstrap CI corpus and the full-corpus canary job
    - Script that runs the exporter with the smoke flag and the test-only parameter file `eval/tests/fixtures/smoke-params.toml` (test values, not proposals for the TBDs, never eligible) and commits the CI corpus; one configuration entry that every `tests/diff/` suite reads; `sched/canaries-full` generates the Canary_Suite from the full corpus release
    - _Requirements: 26.10, 33.11_
    - _Properties: 32, 38_

  - [ ] 12.8 Extend the scope-switch self-test to R6.1's deferred state
    - R6.1's last artifact is the filled pre-registration `eval/rq/corpus.toml`, which needs decided `<TBD-1>`, `<TBD-16>`, `<TBD-17>` and `<TBD-36>` values and is outside this plan, so R6.1 stays deferred; seeded cases in the self-test of task 2.3: with the file absent, R6.1 deferred passes and R6.1 in scope fails (R7.11 criterion 2); adding a fixture `eval/rq/corpus.toml` without switching R6.1 fails (criterion 12)
    - _Requirements: 40.2, 40.12_
    - _Properties: 37_

- [ ] 13. Checkpoint: corpus milestone gate
  - `ci/corpus-check`, `ci/fuzz-smoke` (`fuzz/corpus_metadata/`) and `ci/diff` (including `export_vs_why3find/`) pass and `ci/traceability` is green for the in-scope rows
  - The design's gate also names an eligible `sched/eval` corpus run, and its deliverables name the first full release; both need a filled `eval/rq/corpus.toml` with decided TBD values, which is a decision rather than a coding task, so they are outside this checkpoint. The commit that adds that file must also switch R6.1 to in scope (R7.11 criterion 12)
  - Ensure all tests pass, ask the user if questions arise.

### Stage 3: dp-distribution

- [ ] 14. Model the Coordinator protocol in TLA+
  - [-] 14.1 Write `verification/tla/Coordinator.tla` with its safety invariants
    - **Property 8: Coordinator protocol safety**
    - Front_End, Coordinator, Workers, network as a bag, escalation, fallback, operator actions and the R1.6 fault actions; one action per `TlaAction` variant; invariants `AtMostOneVerdict`, `NonRaceProofRecorded`, eligibility, scaled limits, the reassignment bound, drain and revocation rules, first-wins (Property 4) and `NoEscalationAfterLocalProof`, `FastPassBeforeSubmission`, `CacheHitStaysLocal` (Property 6); TLC configuration with small test bounds (not proposals for `<TBD-4>`) and an Apalache configuration for `sched/apalache-nightly`
    - **Validates: Requirements 1.3, 1.4, 1.7, 4.1, 4.3, 4.4, 5.1, 5.2, 5.4, 5.7, 5.8, 5.13, 5.14, 5.15, 5.16, 6.1, 6.2, 6.3, 6.10, 6.16, 6.17, 7.1, 7.2, 7.3, 7.4, 7.5, 7.7, 7.8, 7.9, 2.1, 2.2, 2.4**
    - As built: a model of the first distributed slice with an adversarial Coordinator; TLC checks `VerdictSound`, `VerdictFinal` and `JobsEndOnce` at small test bounds, and finds a counterexample when the task binding or the signature check is removed. Open: the invariants this task names for heartbeats, Races, certification, fallback and operator actions

  - [ ] 14.2 Add the liveness properties to `Coordinator.tla`
    - **Property 9: Every Escalated_Goal gets a Verdict in bounded time**
    - `EventualVerdict` and Job stop under F1 to F4, per-goal deadline under F3 alone, submission after connection, fallback decides every Escalated_Goal, replacement Jobs after an incarnation change
    - **Validates: Requirements 1.1, 1.5, 5.9, 6.4, 6.5, 6.6, 6.9, 6.20**

- [ ] 15. Implement the dp-core decision functions for distribution
  - [ ] 15.1 Implement `escalation.rs` with Creusot contracts (C4)
    - `escalate`, `cache_excludes`; strategy-step mapping in `frontend.rs` (first sequential pass is the Local_Fast_Pass with its limit clamped to the budget); contracts for P1.2.1 to P1.2.4 and P1.2.7
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_
    - _Properties: 6_

  - [ ] 15.2 Write the escalation property tests
    - **Property 6: Easy goals stay local** (P-ID tests)
    - `crates/dp-core/tests/prop_escalation.rs` with the P1.2 tests; extends `arb_session_events`
    - **Validates: Requirements 2.1, 2.2, 2.3, 2.4, 2.6**

  - [ ] 15.3 Write the umbrella property test for Property 6
    - **Property 6: Easy goals stay local**
    - `property_6_easy_goals_stay_local` in `prop_escalation.rs`
    - **Validates: Requirements 2.1, 2.2, 2.3, 2.4, 2.6**

  - [ ] 15.4 Implement `calibration.rs` with a Kani harness (C5)
    - Create `crates/dp-core/src/calibration.rs`: `scale` without overflow and monotone, `calibration_ok`, `limits_match`; `verification/kani/calibration.rs` (P1.4.3)
    - _Requirements: 4.1, 4.3, 4.4, 4.6, 4.7_
    - _Properties: 8, 29_

  - [ ] 15.5 Write the Calibration property tests
    - **Property 8: Coordinator protocol safety** (Calibration part)
    - `crates/dp-core/tests/prop_calibration.rs`; fills `arb_coordinator_ops`
    - **Validates: Requirements 4.1, 4.3, 4.4**

  - [-] 15.6 Implement the Coordinator machine `coord.rs` with a Kani harness (C5)
    - Create `crates/dp-core/src/coord.rs`: `eligible`, `assign`, `on_heartbeat_timeout`, `on_failed`, `cancel`, `orphan_cancel`, `drop_session`, `apply_operator`, `status`, incarnation identifier, message deduplication; `verification/kani/coordinator.rs` for P1.5.1 and P1.5.6 on single transitions
    - `assign` treats every per-platform solver binary hash that the Lock_File entry of a Solver_Configuration lists as listed (`lock(c)` is the set of those hashes, OQ-N21, D-13), so each Result carries the hash of the binary the Worker ran
    - As built: a pure machine with unit tests in `crates/dp-coordinator/src/machine.rs` (queueing, assignment, cancellation, lost peers). Open: moving it into dp-core in the subset Creusot verifies, Calibration, heartbeats, retries, operator commands, the Kani harness
    - _Requirements: 1.3, 1.4, 1.7, 1.11, 5.4, 5.7, 5.15, 6.1, 6.2, 6.3, 6.10, 6.11, 6.16, 6.17, 7.1, 7.2, 7.3, 7.4, 7.5, 7.7, 7.8, 7.9, 7.10, 7.11_
    - _Properties: 4, 8_

  - [ ] 15.7 Write the assignment and operator property tests
    - **Property 8: Coordinator protocol safety** (assignment and operator part)
    - `crates/dp-core/tests/prop_assign.rs`, `prop_operator.rs`; `prop_assign.rs` generates Lock_File entries that list several platform hashes (P1.1.10) besides the no-eligible-Worker cases (P1.1.2, P1.1.7)
    - **Validates: Requirements 1.3, 1.4, 1.7, 1.11, 7.1, 7.2, 7.3, 7.4, 7.5, 7.7, 7.8, 7.9**

  - [ ] 15.8 Write the idempotence property tests for the Coordinator and operator handler
    - **Property 4: Discards are no-ops and handlers are idempotent** (Coordinator, operator and Verdict_Recorder part)
    - `crates/dp-core/tests/prop_idempotent.rs` and the repeated-command cases of `prop_operator.rs`
    - **Validates: Requirements 6.11, 6.13, 7.3, 7.5, 7.10, 7.11**

  - [ ] 15.9 Write the umbrella property test for Property 8
    - **Property 8: Coordinator protocol safety**
    - `property_8_coordinator_safety` in `prop_assign.rs`
    - **Validates: Requirements 5.1, 5.2, 6.1, 6.2, 6.3**

  - [ ] 15.10 Implement the Worker machine `worker.rs` for Unpatched_Solvers (C6)
    - Create `crates/dp-core/src/worker.rs`: `on_assign`, `on_cancel`, `on_exit`, not-running report, deduplication (the sharing functions `on_export`, `on_pool_item` and `pending_release` follow in task 28.1)
    - _Requirements: 4.5, 5.5, 5.6, 6.12, 6.14_
    - _Properties: 4, 11_

  - [ ] 15.11 Write the Worker cancellation property tests
    - **Property 4: Discards are no-ops and handlers are idempotent** (Worker part)
    - `crates/dp-core/tests/prop_cancel.rs`
    - **Validates: Requirements 5.6, 6.12**

  - [-] 15.12 Extend `verdict.rs` `accept_result` to remote Results with Creusot contracts
    - Decisions from A3 facts: canonical form, size, Ed25519 tag under the operator-signed registry view and held revocations, Task_Hash of `print_local`, task count, submitted pair, import-set hash, recomputed local limit; an authenticated Result naming an open Job that is discarded ends that Job failed
    - Registry view: of the Worker registries with a valid operator signature that the Front_End holds, the one with the highest version supplies the registered identities and verification data, whatever channel delivered each and in whatever order (D-9)
    - Contracts for P1.3.1, P1.3.2, P1.3.4 to P1.3.7 and P1.3.9 to P1.3.11
    - As built: `accept_remote` with its contract (authentic, registered, not revoked, submitted and bound to the local print), proved by Creusot. Open: the remaining criteria this task names (Partition, import-set hash, Calibration limits)
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.6, 3.7, 3.8, 3.9, 3.10, 3.11, 4.6, 4.7, 6.13, 7.6, 9.24, 30.7_
    - _Properties: 3_

  - [ ] 15.13 Write the Result acceptance property and fuzz tests
    - **Property 3: Accepted Results are bound to the local print, the submission and an authentic Worker**
    - `crates/dp-core/tests/prop_accept.rs` with umbrella `property_3_result_binding` (single-field mutations, forged tags, unregistered and revoked Workers, splitting pipelines, several signed registry versions delivered in every arrival order, P1.3.11); `fuzz/result_message/` extended to drive `accept_result` behind the parser (`ci/fuzz-smoke`, `sched/fuzz-nightly`); fills `arb_result`
    - **Validates: Requirements 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 3.7, 3.9, 3.10, 3.11, 7.6, 30.7**

- [ ] 16. Implement the Cluster shells
  - [-] 16.1 Implement Worker and operator authentication (D-9)
    - Ed25519 Worker keys and tags with domain tags; operator registry key signing the versioned Worker registry and revocations; operator command key; `dp-operator` binary in `crates/dp-coordinator/` for key generation, registry signing, revocation, drain and status
    - Operator-signed Worker registry and revocation format: grammar in `docs/formats/`, codec in dp-formats, round-trip cases in `crates/dp-formats/tests/roundtrip.rs`, `fuzz/worker_registry/` (run by `ci/fuzz-smoke` and `sched/fuzz-nightly`) and its registry entry in `docs/formats/index.toml`
    - As built: Ed25519 tags on Results, the operator-signed registry, key generation and registry signing commands, with property tests. Open: revocations, registry versions from several channels, the operator command key
    - _Requirements: 3.2, 3.9, 3.11, 7.1, 7.5, 7.6, 7.11, 30.7, 39.1, 39.7_
    - _Properties: 3, 8, 34_

  - [-] 16.2 Implement the dp-coordinator shell (C5, D-1)
    - Separate Rust Coordinator service in the Proof_Server role around the dp-core Coordinator machine (D-1): ZeroMQ endpoints; Worker connections with offers and Calibrations; task blob store by Task_Hash; heartbeats every `<TBD-31>`; timeouts `<TBD-5>` and `<TBD-32>`; incarnation identifier at start; authenticated operator commands; status report printed with the codec of task 16.6 together with its text rendering (D-20); Session-drop to the Lemma_Pool
    - As built: a TCP shell around the Coordinator machine. Open: ZeroMQ, task blobs by Task_Hash, incarnation identifiers, traces
    - _Requirements: 1.3, 1.4, 1.7, 6.14, 6.16, 6.17, 6.19, 7.7, 7.8, 7.9, 7.12_
    - _Properties: 8_

  - [-] 16.3 Implement the dp-worker shell for Unpatched_Solvers (C6, D-16)
    - Offer with why3find Calibrations through the bridge; task blob fetch with Task_Hash check; solver start by dp-worker itself in its own process group with Why3's prover command line and the Job's seed, options and local limit (D-16, AS-39), binary hash checked just before and reported in the Result; tree termination within `<TBD-33>` after limit expiry and `<TBD-2>` after cancellation; signed Results with the fields of R1.3 criteria 1 and 2 and R1.4 criterion 2; heartbeats; deduplication
    - As built: runs only commands of its own Why3 configuration, checks the solver binary against its Lock_File before each start, signs Results that carry the solver's output and exit status. Open: why3find's Calibration, heartbeats, message deduplication, traces
    - _Requirements: 1.3, 3.1, 3.2, 4.2, 4.5, 5.5, 5.6, 6.12, 6.14, 36.7_
    - _Properties: 3, 11_

  - [-] 16.4 Implement the cluster mode of dp-frontend (C3)
    - Connection within `<TBD-30>`; submission of Escalated_Goals with task blobs (configuration list of a non-Hard Escalated_Goal from why3find's reference-time parallel pass, D-17); cancellation after a Verdict; heartbeats; fallback after `<TBD-7>` silence (`local` or `unresolved`); incarnation change with one replacement Job per open Job; per-goal deadline `<TBD-3>` with local tree kill within `<TBD-45>`; remote facts in `accept.rs`
    - As built: prover calls after the first sequential pass go to the Cluster; Results are checked by `accept_remote` and read by the pinned driver; the Session proves locally without a Coordinator. Tested end to end with real provers, an unregistered Worker and a Coordinator that forges answers. Open: `dp.toml`, heartbeats and the silence timeout, Races, traces
    - _Requirements: 1.1, 1.2, 1.5, 1.8, 5.3, 5.9, 5.16, 5.17, 6.4, 6.5, 6.6, 6.8, 6.9, 6.15, 6.20, 9.25_
    - _Properties: 3, 9, 33_

  - [ ] 16.5 Write unit tests for the status-report text rendering
    - The text rendering that the Coordinator delivers with the canonical status report (task 16.6) lists every registered Worker and every queued and running Job, with the values of the canonical value it renders (R1.7 criterion 12)
    - _Requirements: 7.8, 7.9, 7.12_

  - [ ] 16.6 Implement the stage-3 message formats and the operator status-report format (C16, D-20)
    - Grammars in `docs/formats/`, codec modules in dp-formats and registry entries in `docs/formats/index.toml` for the dp protocol messages that the Front_End, the Coordinator and the Workers exchange (Job submissions, task blobs, assignments, cancellations, heartbeats, failure, no-eligible-Worker and cancelled reports, incarnation identifiers; task 27.2 adds the Lemma_Pool messages) and for the operator status report, one canonical S-expression value of `status(s)` (R1.7 criteria 8, 9 and 12)
    - Round-trip, strictness and size-limit cases in `crates/dp-formats/tests/roundtrip.rs`, including `parse(print(status(s))) = status(s)` over Coordinator states generated with `arb_coordinator_ops` (P1.7.10); fuzz targets `fuzz/protocol_message/` and `fuzz/status_report/`, run by `ci/fuzz-smoke` and `sched/fuzz-nightly`
    - _Requirements: 7.12, 7.8, 7.9, 39.1, 39.2, 39.3, 39.4, 39.5, 39.6, 39.7_
    - _Properties: 34, 8_

- [ ] 17. Build simulation, trace validation and the integration cluster
  - [ ] 17.1 Implement the deterministic simulation harness and its scenarios
    - **Property 9: Every Escalated_Goal gets a Verdict in bounded time** (simulation part)
    - `tests/sim/` instantiates the production shells with `SimEnv`; scenarios: normal Sessions, `faults/`, two-Session; invariant checkers for the P-IDs that R1.1 to R1.7 assign to `tests/sim/`; `<TBD-13>` seeds in `ci/sim`, more in `sched/sim-nightly`; fills `arb_fault_schedule`
    - Switch row R2.1 to in scope with the switch step (Notes): `tests/sim/` is its last artifact, after `prop_config.rs`, `fuzz/config_parser/` and `tests/integration/config/` of stage 1; tasks 19.1 to 19.3 add the R2.1 checks to these files while the row is in scope (R7.11 criterion 12)
    - **Validates: Requirements 1.1, 1.5, 5.9, 6.4, 6.5, 6.6, 6.9, 6.20**

  - [ ] 17.2 Write the adversarial-Coordinator simulation scenario
    - **Property 3: Accepted Results are bound to the local print, the submission and an authentic Worker** (adversarial part)
    - `tests/sim/adversarial_coordinator.rs`: any message of the grammar, replays, swapped Results, self-registered Workers and forged revocations (P1.3.3, P1.3.9, P1.7.2); no proof label without the R7.1 conditions
    - Switch rows R1.3 and R1.7 to in scope with the switch step (Notes): this file is the last artifact of both, after `prop_accept.rs`, `fuzz/result_message/`, `prop_operator.rs` and `operator.rs`; `targets.toml` gains `accept_result`; registry entries for `accept_result` and the `coord.rs` functions of R1.7 (`eligible`, `apply_operator`, `status`) (R7.11 criterion 12)
    - **Validates: Requirements 3.4, 3.6, 3.9, 7.6, 30.2**

  - [ ] 17.3 Write the simulation determinism test
    - **Property 35: The decision core is pure, deterministic and exercised** (simulation part)
    - Two runs with equal seeds and inputs make identical Scheduler decisions and traces
    - **Validates: Requirements 32.1, 32.2**

  - [ ] 17.4 Implement trace validation
    - **Property 10: Implementation traces are behaviors of the specification**
    - `verification/tla/traces/CoordinatorTrace.tla`, merging of per-component traces into the partial order of sequence numbers and send-receive pairs, `ci/jobs/trace-validation.sh`; fails on a run without traces or a record whose action is absent from `actions.toml`
    - Switch row R1.5 to in scope with the switch step (Notes): `verification/tla/traces/` is its last artifact, after `cancel.rs` and `deadline.rs` (17.5) and the dp-shell trace-writer test (1.6); registry entries for `cancel`, `on_cancel`, `record_verdict` and `final_verdicts` (R7.11 criterion 12)
    - **Validates: Requirements 5.10, 5.11, 5.12**

  - [ ] 17.5 Build the integration cluster and its cluster tests
    - **Property 11: Process trees end and heartbeats flow within their bounds**
    - Nix-built containers (one Coordinator, at least two Workers) with stub solvers that ignore termination and spawn children; `tests/integration/cluster/` with `cancel.rs`, `deadline.rs`, `calibration.rs`, `faults.rs`, `fallback.rs`, `config.rs`, `operator.rs` (criteria 1 and 3 to 12 of R1.7, including the canonical status report and its text rendering) and the basic submission tests of R1.1 criteria 1, 3, 4 and 7; traces and Sessions kept for `ci/trace-validation` and `ci/replay`
    - Switch row R1.6 to in scope with the switch step (Notes): `faults.rs` is its last artifact, after `tests/sim/faults/` (17.1) and `prop_idempotent.rs`; registry entries for `on_heartbeat_timeout`, `on_failed`, `orphan_cancel`, `drop_session` and the fallback decision (R7.11 criterion 12)
    - **Validates: Requirements 4.5, 5.5, 5.17, 6.14, 6.15, 6.19, 7.12**

  - [ ] 17.6 Write the Local_Fast_Pass integration tests
    - **Property 6: Easy goals stay local** (integration part)
    - `tests/integration/fast_pass/`: proved goals never reach the Cluster, budget clamp observed on started processes
    - **Validates: Requirements 2.1, 2.2, 2.6**

  - [ ] 17.7 Write the cluster-off and distributed-versus-local tests
    - **Property 33: Turning a feature on or off never flips a proof against a counter-model** (cluster option part)
    - `tests/integration/cluster/cluster_off.rs` (no connection, every solver local) and `tests/diff/dist_vs_local/` on the CI corpus
    - Switch row R1.1 to in scope with the switch step (Notes): these two are its last artifacts; registry entries for `eligible` and `assign` serving R1.1 (R7.11 criterion 12)
    - **Validates: Requirements 1.2, 1.8**

  - [ ] 17.8 Add the distributed canary mode and pipeline canaries
    - **Property 32: Canaries never reach a proof label** (distributed mode)
    - Distributed mode under adversarial Coordinator schedules; pipeline canaries delivered as Results of a test Worker
    - **Validates: Requirements 33.4, 33.5, 33.6, 33.7**

  - [ ] 17.9 Write the umbrella property test for Property 9
    - **Property 9: Every Escalated_Goal gets a Verdict in bounded time**
    - `property_9_bounded_verdict` in `tests/sim/` over generated fault schedules
    - **Validates: Requirements 1.1, 5.9, 6.4**

  - [ ] 17.10 Extend the scope-switch self-test to the stage-3 rows
    - Seeded mutations in the self-test of task 2.3 for each row that stage 3 switched (R2.1 in 17.1, R1.6 in 17.5, R1.3 and R1.7 in 17.2, R1.5 in 17.4, R1.1 in 17.7): back to deferred fails `ci/traceability` (R7.11 criterion 12); a deleted artifact fails it (criterion 2); a deleted registry entry of one of the row's decision functions fails `ci/purity` (R7.3 criterion 4); `accept_result` removed from `targets.toml` fails `ci/traceability` (R7.13 criterion 6)
    - _Requirements: 40.2, 40.12, 32.4, 42.6_
    - _Properties: 37_

- [ ] 18. Checkpoint: dp-distribution gate
  - `ci/tlc`, `ci/trace-validation`, `ci/sim`, `ci/integration-cluster`, `ci/kani` (coordinator and calibration harnesses), `ci/diff` (`dist_vs_local/`), `ci/canaries` (distributed mode), `ci/fuzz-smoke` (`result_message/`, `worker_registry/`, `protocol_message/`, `status_report/`) and every earlier job pass; `ci/traceability` is green with R1.1, R1.3, R1.5, R1.6, R1.7 and R2.1 in scope; `sched/apalache-nightly` and `sched/sim-nightly` have acceptable runs
  - Ensure all tests pass, ask the user if questions arise.

### Stage 4: dp-portfolio

- [ ] 19. Implement Solver_Configurations, Portfolios and Races
  - [-] 19.1 Implement the Solver_Configuration file and its validation (C3, C16)
    - Canonical TOML grammar and codec for Solver_Configurations and Portfolios, extending the `dp.toml` grammar and codec of task 3.2; Portfolio chosen by the portfolio option; `validate_config` for unknown prover-version pairs, parse errors, duplicate names, empty Portfolios, unknown names, duplicate entries and splitting transformations; warning when portfolio racing is on without the cluster option
    - As built so far, as a slice: seeds only. Jobs and signed Results carry a seed; a Worker changes nothing of its command line but the solver's seed options; `DP_PORTFOLIO_SEEDS` races that many Jobs on each prover call on the Cluster, the first proof label wins, and otherwise the call gets the most informative non-proof outcome. Open: the Solver_Configuration file, option lists (OQ-N37), Hard_Goal classification, `race.rs` in dp-core, the replay fields
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.7, 9.15, 9.17, 9.27_
    - _Properties: 5, 34_

  - [ ] 19.2 Extend the configuration property and integration tests
    - **Property 5: Invalid configurations and pin mismatches fail closed** (Solver_Configuration part)
    - `prop_config.rs` and `tests/integration/config/` cases for R2.1 criteria 3 to 5 and R2.2 criterion 17; extends `arb_config_text`
    - **Validates: Requirements 8.3, 8.4, 8.5, 9.17**

  - [ ] 19.3 Write round-trip and fuzz tests for the Solver_Configuration codec
    - **Property 34: Exchanged formats parse strictly and print canonically** (configuration codec)
    - `crates/dp-formats/tests/roundtrip.rs` cases and `fuzz/config_parser/` seeds for the configuration file (`ci/fuzz-smoke`, `sched/fuzz-nightly`)
    - **Validates: Requirements 8.1, 8.7, 39.4**

  - [ ] 19.4 Add the Race actions and invariants to `Coordinator.tla`
    - **Property 7: Races decide by their first proof label** (model part)
    - Race formation, submission in Race order with at most `<TBD-8>` open, Race deadline `<TBD-9>`, Waiting state, stop and continue policies, fallback, per-goal deadline, late Results
    - **Validates: Requirements 9.3, 9.6, 9.7, 9.8, 9.10, 9.11, 9.12, 9.13, 9.14, 9.22**

  - [ ] 19.5 Implement `race.rs` with Creusot contracts (C4)
    - `classify_hard` with the hardness rule `<TBD-10>`, `form_race`, `next_submissions`, `on_job_end`, `end_race`; contracts for P2.2.1, P2.2.2, P2.2.5, P2.2.8, P2.2.12, P2.2.14, P2.2.15, P2.2.17, P2.2.19
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.7, 9.8, 9.9, 9.10, 9.11, 9.12, 9.13, 9.14, 9.16, 9.18, 9.20, 9.22, 9.23, 9.24, 9.25, 9.26_
    - _Properties: 7_

  - [ ] 19.6 Write the Race property and fault-simulation tests
    - **Property 7: Races decide by their first proof label**
    - `crates/dp-core/tests/prop_race.rs` with umbrella `property_7_first_proof_label_wins` and the P2.2 tests; Race scenarios in `tests/sim/faults/`
    - **Validates: Requirements 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.7, 9.8, 9.9, 9.10, 9.11, 9.12, 9.13, 9.14, 9.15, 9.16, 9.18, 9.19, 9.20, 9.22, 9.23, 9.24, 9.25, 9.26, 9.27, 6.8**

  - [ ] 19.7 Wire Races into the Front_End and `proof.json`
    - Hard_Goal classification at escalation; Race Jobs submitted with the Solver_Configuration exactly as parsed; conflict detection with accepted Invalid Results; late Results; replay fields of R2.3 for the recorded Result
    - _Requirements: 8.2, 8.6, 9.1, 9.12, 9.19, 10.1, 10.4, 10.7, 31.7, 31.11_
    - _Properties: 7, 30_

  - [ ] 19.8 Extend the `proof.json` and conflict tests for portfolio Sessions
    - **Property 30: proof.json and the Creusot_CLI report labels faithfully** (replay fields and conflicts)
    - `prop_proofjson.rs` replay-field cases; `tests/integration/cluster/race.rs` for conflict entries and the non-zero exit status
    - Switch row R7.2 to in scope with the switch step (Notes): `tests/integration/cluster/race.rs` is its last artifact; `targets.toml` gains `decode_label`, `most_informative`, `classify`; registry entries for `most_informative` and `classify` (R7.11 criterion 12)
    - **Validates: Requirements 10.1, 10.4, 10.5, 10.7, 9.19, 31.7, 31.8, 31.12**

  - [ ] 19.9 Extend the replay tests to portfolio winners
    - **Property 31: Offline replay confirms exactly what the evidence supports** (portfolio part)
    - `tests/replay/` with Race winners from `ci/integration-cluster` Sessions and absent-marker cases
    - **Validates: Requirements 10.8, 35.2, 35.5, 35.7**

  - [ ] 19.10 Add the portfolio canary mode
    - **Property 32: Canaries never reach a proof label** (portfolio mode)
    - **Validates: Requirements 33.5, 33.6**

  - [ ] 19.11 Extend the scope-switch self-test to the stage-4 row
    - Seeded mutations in the self-test of task 2.3 for R7.2 (switched in 19.8): back to deferred fails `ci/traceability` (R7.11 criterion 12); a deleted artifact fails it (criterion 2); a deleted registry entry of `most_informative` or `classify` fails `ci/purity` (R7.3 criterion 4); `decode_label` removed from `targets.toml` fails `ci/traceability` (R7.13 criterion 6)
    - _Requirements: 40.2, 40.12, 32.4, 42.6_
    - _Properties: 37_

- [ ] 20. Checkpoint: dp-portfolio gate
  - `ci/creusot` (`race.rs`), `ci/tlc` (Race invariants), `prop_race.rs`, `prop_config.rs`, `ci/replay`, `ci/canaries` (portfolio mode) and every earlier job pass; `ci/traceability` is green with R7.2 in scope (R2.1 and R2.3 switched earlier, in 17.1 and 9.5)
  - Ensure all tests pass, ask the user if questions arise.

### Stage 5: dp-certificates

- [ ] 21. Implement the certification decisions and their metatheory
  - [ ] 21.1 Implement `label_from_checker_output` and `certification.rs` with Creusot contracts (C4, C11)
    - `labels.rs` `label_from_checker_output` (certified exactly for accept with every binding check passing and storage complete by the deadline); `certification.rs` `needs_cert`, `import_closure`, `oblig`, `extended_problem`, `lemma_problem`, `cert_start`, `deadlines` (proof and certification deadlines from `<TBD-14>` and `<TBD-15>`), `leaf_select`; `label` routes Results that need certification through them
    - _Requirements: 21.1, 21.3, 21.4, 21.5, 21.6, 21.7, 21.8, 21.9, 23.4, 23.5, 23.6, 23.11, 23.17, 23.18_
    - _Properties: 1, 17, 23, 27_

  - [ ] 21.2 Write the certification-deadline property tests
    - **Property 23: Missing, late or unstored certification gives unchecked**
    - `crates/dp-core/tests/prop_certification.rs` with umbrella `property_23_missing_late_unstored_unchecked`
    - **Validates: Requirements 21.4, 21.5, 21.6, 21.7, 21.9, 9.26**

  - [ ] 21.3 Write the local re-derivation property tests
    - **Property 17: Certification inputs are re-derived locally** (certification.rs part)
    - In `prop_certification.rs`: extended and Lemma problems built only from `print_local(B)`, the recorded tree, imported texts and recomputed instances; `oblig` is the used-Lemma closure of matched imports
    - **Validates: Requirements 21.1, 21.2, 21.3**

  - [ ] 21.4 Add certification deadlines and `CertifiedHasStoredEntry` to `Coordinator.tla`
    - **Property 23: Missing, late or unstored certification gives unchecked** (model part)
    - Abstract certification outcomes and deadlines; `CertifiedHasStoredEntry` also serves Properties 27 and 28
    - **Validates: Requirements 21.6, 21.7, 23.17, 24.4**

  - [ ] 21.5 Prove `compose_sound` and state `certifiable` in Lean
    - **Property 24: Composed certificates follow the plan and derive false from B's Assertions** (metatheory part)
    - `proofs/lean/DP/Proof.lean`, `Compose.lean` (`compose_sound`, P5.2.1), `Certifiable.lean` stating `certifiable` (P7.1.8) with `compose_sound` and its sharing premises as hypotheses until tasks 26.1, 31.1 and 33.1 discharge them
    - **Validates: Requirements 22.1, 22.6, 30.3**

- [ ] 22. Implement certificate composition and the supported fragment
  - [ ] 22.1 Implement `fragment.rs` with Creusot contracts (C12)
    - `fragment_ok`: exactly the allow-lists of R5.2 with rule names from the Lock_File-pinned CPC signature or Alethe rule list; everything else banned, including `declare-rule`, `program`, `declare-oracle-fun`, `include`, `reference`, `reset`, `exit`, `set-option`, `define` (AS-36: if cvc5 needs `define`, the affected Results stay unchecked)
    - _Requirements: 22.15, 22.16, 23.15, 23.19_
    - _Properties: 25_

  - [ ] 22.2 Write the fragment property tests
    - **Property 25: Certificates stay inside the supported fragment**
    - `crates/dp-core/tests/prop_fragment.rs`; fills `arb_proof_skeleton`
    - **Validates: Requirements 22.15, 22.16, 23.15, 23.19**

  - [ ] 22.3 Implement the dp-cert proof parsers, skeletons and certificate printer (C12)
    - CPC and Alethe parsers with size limits into skeletons with canonical assumption keys; printer of Composed_Certificates (Lemma sub-proofs as `assume-push` of ¬L with namespaced steps, scope step and contradiction or double-negation step; Alethe `anchor` subproofs; target proof with references to justifications); container recording `print_local(B)` and its Task_Hash; `fuzz/cpc_proof/`, `fuzz/alethe_proof/` (`ci/fuzz-smoke`, `sched/fuzz-nightly`)
    - The printed certificate declares no symbol of `print_local(B)`; Ethos rejects every assumption of a proof that declares them again
    - _Requirements: 22.8, 22.9, 22.13, 22.15, 22.20, 39.1, 39.4, 39.5_
    - _Properties: 24, 34_

  - [ ] 22.4 Write round-trip tests for the proof codecs
    - **Property 34: Exchanged formats parse strictly and print canonically** (proof codecs)
    - `crates/dp-cert/tests/roundtrip.rs` for CPC proofs, Alethe proofs and Composed_Certificates; their registry entries in `docs/formats/index.toml`
    - **Validates: Requirements 39.1, 39.2, 39.4, 39.5, 39.7, 22.13**

  - [ ] 22.5 Implement `compose_plan.rs` with Creusot contracts and its Lean reference
    - Steps 1 to 6 and 8 of C12 with the named abort reasons (step 7 is added by task 29.2); Lean reference `compose_plan` added to `dp-lean-ref`
    - _Requirements: 22.1, 22.4, 22.5, 22.6, 22.7, 22.13, 22.14, 22.16, 22.17, 22.18, 22.19, 12.8, 12.9_
    - _Properties: 24_

  - [ ] 22.6 Write the composition property and differential tests
    - **Property 24: Composed certificates follow the plan and derive false from B's Assertions**
    - `crates/dp-cert/tests/prop_compose.rs` with umbrella `property_24_composed_certificates`; `tests/diff/compose_lean_vs_rust/`
    - **Validates: Requirements 22.1, 22.4, 22.5, 22.6, 22.7, 22.8, 22.9, 22.13, 22.14, 22.16, 22.17, 22.18, 22.19, 22.20**

- [ ] 23. Implement checking, storage and certify-all, and wire certification end to end
  - [ ] 23.1 Implement the Certificate_Checker driver `crates/dp-cert/src/check.rs` (A3, C13)
    - Ethos and Carcara command lines with the `docs/tcb.toml` option lists; binding facts (binary hash just before start, option list, signature-file hashes, recorded input Task_Hash against `print_local(B)`, independent assumption check, `fragment_ok`, Alethe hole check while the Carcara part of AS-6 is To check); banned options never passed; process group killed at the certification deadline; output classifier from the fixed table; logged checker identity
    - Codecs, round-trip cases, fuzz targets (`ci/fuzz-smoke`, `sched/fuzz-nightly`) and registry entries for the checker entries of `docs/tcb.toml` and the checker-output record that the Certificate_Store keeps
    - Switch row R7.5 to in scope with the switch step (Notes): `check.rs` is its last artifact, after `cache_on_off/` (9.6); `targets.toml` gains `encode_key`; registry entries for `encode_key`, `lookup_ok`, `validate_entry` (R7.11 criterion 12)
    - Ethos outcome from the exit status and standard output only; warnings on standard error (for example for `:pattern`) are not part of it
    - _Requirements: 23.1, 23.2, 23.3, 23.6, 23.7, 23.8, 23.9, 23.10, 23.12, 23.13, 23.14, 23.15, 23.16, 23.19, 36.4, 37.3, 37.4, 39.7_
    - _Properties: 27, 34_

  - [ ] 23.2 Write the checker-outcome tests
    - **Property 27: Checker outcomes map to labels by one exact rule**
    - `prop_verdict.rs` checker-tuple cases with umbrella `property_27_checker_outcome_rule`; `fuzz/checker_output/` (`ci/fuzz-smoke`, `sched/fuzz-nightly`) with the registry entry of the Certificate_Checker output format; `tests/certs/tamper/` (other input, option list, signature file); table and option agreement with `docs/tcb.toml`; `label_lean_vs_rust/` extended to checker tuples; fills `arb_checker_tuple`
    - Switch row R5.3 to in scope with the switch step (Notes): `tests/certs/tamper/` and `fuzz/checker_output/` are its last artifacts, after `check.rs` (23.1) and `tests/integration/certs/` (23.4); `targets.toml` gains `label_from_checker_output`, `fragment_ok`, `label`; registry entries for these three (R7.11 criterion 12)
    - **Validates: Requirements 23.1, 23.2, 23.3, 23.4, 23.5, 23.6, 23.7, 23.8, 23.9, 23.11, 23.12, 23.13, 23.14, 23.16, 23.17, 23.18, 37.3, 37.4**

  - [ ] 23.3 Implement the Certificate_Store `crates/dp-cert/src/store.rs` (C14)
    - `.dp-certs/<content hash>/` written through a temporary directory with synced files and manifest, rename as the complete event, existing entries kept, storage failure reported; `proof.json` written only after the rename
    - Manifest codec with its grammar, round-trip cases in `crates/dp-formats/tests/roundtrip.rs`, `fuzz/cert_manifest/` (`ci/fuzz-smoke`, `sched/fuzz-nightly`) and its registry entry
    - _Requirements: 24.1, 24.2, 24.3, 24.4, 24.5, 24.6, 24.7, 39.7_
    - _Properties: 28, 34_

  - [ ] 23.4 Write the Certificate_Store property and fault tests
    - **Property 28: The Certificate_Store is content-addressed and crash-safe**
    - `crates/dp-cert/tests/prop_store.rs` with umbrella `property_28_store_crash_safe`; `tests/integration/certs/store_faults.rs` with crash points
    - Switch rows R5.4, R7.7 and R7.8 to in scope with the switch step (Notes): `store_faults.rs` creates `tests/integration/certs/`, the last artifact of R7.7 and R7.8 and, with `prop_store.rs`, of R5.4 (R7.11 criterion 12)
    - **Validates: Requirements 24.1, 24.2, 24.5, 24.7**

  - [ ] 23.5 Wire certification and certify-all mode into the shells
    - Front_End: target proof request by a proof-producing cvc5 Solver_Configuration (D-6; the configured proof-producer list and its selection rule are task 23.16) as a proof Job on the Cluster or locally; Lemma proof requests in parallel once `oblig(r)` is known; cancellation at the proof deadline; compose, check, store, label; certify-all treats every proof-reporting Result as needing certification and certifies Local_Fast_Pass Results locally before escalation
    - Coordinator and Worker: proof Job kind with proof format and proof bytes in Results
    - _Requirements: 21.1, 21.2, 21.4, 21.6, 21.7, 21.9, 2.4, 2.8, 2.9, 10.2, 24.3, 24.4, 24.6, 25.1, 25.2, 25.3_
    - _Properties: 1, 6, 23_

  - [ ] 23.6 Extend the labelling tests to the certified route and certify-all
    - **Property 1: Proof labels come only from the two routes** (certified and certify-all parts)
    - `prop_verdict.rs` and `tests/sim/adversarial_coordinator.rs` with certification outcomes, delays and certify-all on and off
    - **Validates: Requirements 30.3, 30.4, 21.8, 23.4, 23.17, 25.1, 25.2, 25.3, 9.8, 9.10**

  - [ ] 23.7 Implement the certified branch of the Replay_Tool
    - Manifest and content-hash checks, stored Printed_Goal against the re-print and `proof.json`, checker run on the stored certificate against the re-print with the same binding checks and `label_from_checker_output`
    - _Requirements: 35.3, 35.4, 24.8, 24.9_
    - _Properties: 28, 31_

  - [ ] 23.8 Extend the replay and tamper tests to certified Verdicts
    - **Property 31: Offline replay confirms exactly what the evidence supports** (certified part)
    - `tests/replay/` and `tests/replay/tamper/` with changed entries, changed content hashes and missing entries
    - **Validates: Requirements 35.3, 35.4, 24.8, 24.9**

  - [ ] 23.9 Extend the Result_Cache to certified entries (C15)
    - Cache_Key with the pinned checker identities for run descriptions that may need certification; fresh checker run for certified hits; in certify-all mode a hit that cannot be labelled certified is a miss
    - _Requirements: 34.1, 30.6, 25.4, 25.6_
    - _Properties: 29_

  - [ ] 23.10 Extend the cache tests to certified entries
    - **Property 29: Cache hits reuse only what nothing could have changed** (certified part)
    - `prop_cache_key.rs` checker-identity cases; certified cache canaries
    - **Validates: Requirements 34.1, 30.6, 25.4, 25.6**

  - [ ] 23.11 Write the certification integration tests with real tools
    - **Property 11: Process trees end and heartbeats flow within their bounds** (checker part)
    - `tests/integration/certs/` with real cvc5, Ethos and Carcara (`ci/certs`): checker killed at the certification deadline, changed checker binary gives unchecked; records the outcomes for AS-5, AS-6, AS-23, AS-30, AS-34, AS-36 and AS-38 in `docs/tcb.toml`
    - **Validates: Requirements 23.10, 36.4**

  - [ ] 23.12 Write the certify-all differential test
    - **Property 33: Turning a feature on or off never flips a proof against a counter-model** (certify-all part)
    - `tests/diff/certify_all_on_off/`
    - **Validates: Requirements 25.2**

  - [ ] 23.13 Add the certify-all canary mode and certificate canaries
    - **Property 32: Canaries never reach a proof label** (certify-all mode and certificate canaries)
    - One certificate canary per banned command, the Alethe hole canary, wrong-input and wrong-option certificates
    - **Validates: Requirements 33.4, 33.9, 23.3**

  - [ ] 23.14 Extend the Local_Fast_Pass tests to certify-all mode
    - **Property 6: Easy goals stay local** (certify-all part, P1.2.7)
    - `tests/integration/fast_pass/` and `prop_escalation.rs` cases: a locally certified Local_Fast_Pass Result keeps the Goal_Version local; only unchecked or checked-incomplete Results escalate
    - **Validates: Requirements 2.2, 2.8, 2.9**

  - [ ] 23.15 Extend the scope-switch self-test to the stage-5 rows
    - Seeded mutations in the self-test of task 2.3 for each row that stage 5 switched (R7.5 in 23.1, R5.4, R7.7 and R7.8 in 23.4, R5.3 in 23.2): back to deferred fails `ci/traceability` (R7.11 criterion 12); a deleted artifact fails it (criterion 2); a deleted registry entry of one of the rows' decision functions fails `ci/purity` (R7.3 criterion 4); `encode_key` or `label_from_checker_output` removed from `targets.toml` fails `ci/traceability` (R7.13 criterion 6)
    - _Requirements: 40.2, 40.12, 32.4, 42.6_
    - _Properties: 37_

  - [ ] 23.16 Implement the proof-producer list and the target-proof requests (D-6)
    - `proof_producers` option of `dp.toml`: the proof-producing cvc5 Solver_Configurations for target proofs, CPC preferred and Alethe second, possibly empty; for each Result that needs certification, exactly one proof-producing run of its extended problem with every imported Lemma and recomputed instance as a plain assertion, submitted as a proof Job while the Front_End is connected and not in the fallback, and started locally otherwise and for the local certification of R1.2 criterion 8; Lemma proof requests only after the target proof is obtained; with an empty list no request is made and the Result is labelled unchecked under R5.1 criterion 4
    - P5.1.12 asserted on `tests/sim/` traces with and without a Cluster connection, after the switch to the fallback, in certify-all Sessions and with an empty proof-producer list
    - Proof-producing runs also set `--dag-thresh=0`; the Transformation_Pipeline of a certifying Driver includes `eliminate_algebraic` or `eliminate_match`, because Ethos cannot read `match` in a reference file
    - _Requirements: 21.11, 21.1, 21.2, 21.4, 2.8_
    - _Properties: 17, 23_

- [ ] 24. Checkpoint: dp-certificates gate
  - `ci/lean` (`compose_sound`; `Certifiable.lean` states `certifiable` with its sharing premises as hypotheses), `ci/creusot` (certification, plan, fragment, labels), `ci/certs` with real cvc5, Ethos and Carcara, `tests/certs/tamper/`, `ci/canaries` (certify-all mode and certificate canaries), `ci/diff` (`certify_all_on_off/`, `compose_lean_vs_rust/`), `ci/tcb-check` (with the outcomes for AS-5, AS-6, AS-23, AS-30, AS-34, AS-36 and AS-38 that task 23.11 records in `docs/tcb.toml`) and every earlier job pass; `ci/traceability` is green with R5.3, R5.4, R7.5, R7.7 and R7.8 in scope
  - Ensure all tests pass, ask the user if questions arise.

### Stage 6: dp-within-goal-sharing

- [ ] 25. Implement canonical naming
  - [ ] 25.1 Prove `canon_injective` and `canon_equisat` in Lean
    - **Property 16: Canonical names mean identical declarations** (metatheory part)
    - `proofs/lean/DP/Canon.lean` with reference `canon` including `:named` labels and references; reference added to `dp-lean-ref`
    - **Validates: Requirements 17.3, 17.4**

  - [ ] 25.2 Implement the Canonicalizer `crates/dp-canon/` (C8, D-5)
    - Post-processing of SMT-LIB text in Rust, so Printed_Goals, Task_Hashes and solver inputs stay unchanged (D-5); steps 1 to 9 of C8: SMT-LIB script grammar keeping goal-marker comments (AS-24), declared symbols by (source name, kind), goal marker from `docs/formats/goal-marker.toml`, reference graph with empty axiom sets (D-12), SCC-based symbol names, canonical formulas, name grammar `docs/formats/canonical-name.abnf`, solver-name patterns `docs/formats/solver-names.toml` (AS-27), Session and per-Job collision maps, `CanonMap` output; sorted iteration only
    - _Requirements: 17.1, 17.2, 17.3, 17.4, 17.5, 17.6, 17.7, 17.8, 17.9, 17.10, 17.11, 17.15_
    - _Properties: 16_

  - [ ] 25.3 Write the Canonicalizer tests
    - **Property 16: Canonical names mean identical declarations**
    - `crates/dp-canon/tests/` with umbrella `property_16_canonical_names`; `tests/diff/canon_equisat/`, `canon_determinism/`, `canon_lean_vs_rust/`, `solver_names/` (regular-language disjointness and solver-name scan); `fuzz/canonicalizer/` (`ci/fuzz-smoke`, `sched/fuzz-nightly`); fills `arb_printed_goal`
    - **Validates: Requirements 17.1, 17.2, 17.3, 17.4, 17.5, 17.6, 17.7, 17.8, 17.9, 17.10, 17.11, 17.15**

  - [ ] 25.4 Implement renaming between source names and Canonical_Names
    - `crates/dp-canon/src/rename.rs`: bijection-based renaming in both directions; Lemmas and hints with unmapped symbols are dropped and logged with exactly those symbols; no export from a Job whose Printed_Goal has a canonicalization failure
    - _Requirements: 17.13, 17.16, 17.17, 17.18, 17.19_
    - _Properties: 18_

  - [ ] 25.5 Write the renaming property tests
    - **Property 18: Renaming between source names and Canonical_Names round-trips**
    - `crates/dp-worker/tests/prop_rename.rs` with umbrella `property_18_renaming_round_trips`; SMT-LIB script parser round trip in `crates/dp-canon/tests/`
    - Registry entries for the Canonical_Name grammar, the solver-introduced name pattern list and the SMT-LIB script grammar, each with its round-trip test and fuzz target (R7.10 criterion 7)
    - **Validates: Requirements 17.16, 17.17, 17.18, 17.19, 39.7**

- [ ] 26. Implement provenance and admission
  - [ ] 26.1 Prove the admission theorems in Lean
    - **Property 12: Admitted Lemmas follow from the target problem (metatheory)** (within-goal part)
    - `proofs/lean/DP/ModelSanity.lean` (`model_defined_symbols`, `model_div_zero_unspecified`) and `Admission.lean` (`admission_sound`, `partition_local`, `admission_sound_seq`)
    - **Validates: Requirements 14.1, 14.2, 14.8**

  - [ ] 26.2 Implement Provenance_Records and `provenance.rs` with Creusot contracts
    - Provenance and Lemma message formats in dp-formats; `prov_wf`, `default_deps`; identifiers over received bytes and tags; `fuzz/provenance/` (`ci/fuzz-smoke`, `sched/fuzz-nightly`)
    - _Requirements: 12.1, 12.2, 12.3, 12.4, 12.5, 12.6, 12.7_
    - _Properties: 15_

  - [ ] 26.3 Write the provenance property tests
    - **Property 15: Provenance is well-formed, authentic and preserved** (Lemma part)
    - `crates/dp-core/tests/prop_provenance.rs` with umbrella `property_15_provenance`; fills `arb_provenance`
    - **Validates: Requirements 12.1, 12.2, 12.3, 12.4, 12.5, 12.6, 12.7, 12.10**

  - [ ] 26.4 Implement the Lemma grammar parser with a Kani harness
    - Vocabulary-independent Lemma grammar and the parse against a Canonical_Vocabulary in dp-formats; size limit `<TBD-12>` before parsing; `verification/kani/parsers.rs` (no panic); `fuzz/lemma_parser/` (`ci/fuzz-smoke`, `sched/fuzz-nightly`)
    - _Requirements: 13.1, 13.3, 13.6, 39.1, 39.5_
    - _Properties: 14, 34_

  - [ ] 26.5 Implement `admission.rs` with Creusot contracts (C9)
    - `hygiene_ok` (rejects `let` and every other binder, D-14), `is_ground_lemma`, `export_filter`, `admit` (checks in the order of C9 with one reason category), `readmit`; signature and registry facts as inputs
    - _Requirements: 13.1, 13.2, 13.3, 13.4, 13.5, 13.6, 14.1, 14.2, 14.4, 14.5, 14.6, 14.7, 14.8, 14.11, 17.12_
    - _Properties: 13, 14_

  - [ ] 26.6 Write the hygiene property tests
    - **Property 14: Admitted Lemmas are hygienic**
    - `crates/dp-core/tests/prop_hygiene.rs` with umbrella `property_14_admitted_lemmas_hygienic` and export-filter cases
    - Switch row R3.3 to in scope with the switch step (Notes): `prop_hygiene.rs` is its last artifact, after `admission.rs`, `kani/parsers.rs`, `fuzz/lemma_parser/` and the Lemma-term registry entry (26.8); `targets.toml` gains `hygiene_ok`; registry entry for `hygiene_ok` (R7.11 criterion 12)
    - **Validates: Requirements 13.1, 13.2, 13.3, 13.4, 13.5, 13.6, 11.1**

  - [ ] 26.7 Write the admission property and differential tests
    - **Property 13: The admission procedure is the specified function**
    - `crates/dp-core/tests/prop_admission.rs` with umbrella `property_13_admission_function`; reference `admit` added to `dp-lean-ref`; `tests/diff/admission_lean_vs_rust/`
    - **Validates: Requirements 14.1, 14.2, 14.4, 14.5, 14.6, 14.7, 14.8, 14.11, 17.12**

  - [ ] 26.8 Write round-trip tests for the Lemma and provenance codecs
    - **Property 34: Exchanged formats parse strictly and print canonically** (Lemma and provenance codecs)
    - `crates/dp-formats/tests/roundtrip.rs` cases and `tests/diff/smtlib_parser/` (Lemma term parser against cvc5's parser); fills `arb_lemma`; registry entries for Lemma terms, Lemma messages and Provenance_Records
    - Switch row R7.10 to in scope with the switch step (Notes): `tests/diff/smtlib_parser/` is its last artifact, after `kani/parsers.rs` (26.4) and `crates/dp-canon/tests/` (25.3); `targets.toml` gains `canonical_check` (R7.11 criterion 12)
    - **Validates: Requirements 39.1, 39.2, 39.4, 39.5, 39.6, 39.7**

- [ ] 27. Implement the Lemma_Pool and the sharing protocol model
  - [ ] 27.1 Write `verification/tla/Sharing.tla` and include it in `Coordinator.tla`
    - **Property 20: Lemmas flow only within their scope** (model part)
    - Export, pool storage, forwarding, admission as an abstract predicate, pending queue, delivery, Job registration, Lemma_Pool restarts; invariants P3.1.1, P3.1.2, P3.1.5, P3.1.8, P4.2.1, P4.2.2; forwarding liveness P3.1.10 under F1 to F3 (Property 9); `INSTANCE` under `SharingEnabled`
    - **Validates: Requirements 11.3, 11.4, 11.5, 11.6, 11.10, 11.11**

  - [ ] 27.2 Implement `pool.rs`, `forward.rs` and the dp-pool shell with a Kani harness (C7)
    - Create `crates/dp-core/src/pool.rs` (`insert` with size, parse and identifier checks and bounds `<TBD-11>`, `<TBD-12>`, `<TBD-35>`, `<TBD-34>`; `delete_session` on Session-end and Session-drop; `restart`, after which the pool is empty, OQ-N15) and `crates/dp-core/src/forward.rs` (`forward_ok`, same-Goal_Version branch); dp-pool shell with byte-for-byte storage, Job registrations, forwarding within `<TBD-43>` and reads by identifier; Lemma_Pool messages (Job registration, Job end, Session-end, Session-drop, pool items) added to the protocol-message grammar of task 16.6; `verification/kani/pool.rs`
    - _Requirements: 16.1, 16.2, 16.3, 16.4, 16.5, 16.6, 16.7, 16.8, 11.3, 11.11, 12.5_
    - _Properties: 4, 20, 22_

  - [ ] 27.3 Write the Lemma_Pool bound tests
    - **Property 22: The Lemma_Pool stays within its bounds** (P-ID tests)
    - `crates/dp-pool/tests/prop_pool.rs` bound cases; `fuzz/lemma_pool_ingest/` (`ci/fuzz-smoke`, `sched/fuzz-nightly`)
    - **Validates: Requirements 16.1, 16.3, 16.4, 16.7, 16.8**

  - [ ] 27.4 Write the Lemma_Pool idempotence tests
    - **Property 4: Discards are no-ops and handlers are idempotent** (Lemma_Pool part)
    - `prop_pool.rs`: discards change nothing but the log, first of two equal identifiers wins, inserting twice equals once
    - **Validates: Requirements 16.2, 16.5, 16.6**

  - [ ] 27.5 Write the forwarding property tests
    - **Property 20: Lemmas flow only within their scope** (forwarding part)
    - `crates/dp-core/tests/prop_forward.rs` with umbrella `property_20_lemma_scope`
    - **Validates: Requirements 11.3, 11.4, 11.10, 11.12, 11.13**

  - [ ] 27.6 Write the umbrella property test for Property 22
    - **Property 22: The Lemma_Pool stays within its bounds**
    - `property_22_pool_bounds` in `prop_pool.rs` over arbitrary input byte strings
    - **Validates: Requirements 16.1, 16.2, 16.3, 16.4, 16.7, 16.8**

- [ ] 28. Wire within-goal sharing into the Worker, the Front_End and the solvers
  - [ ] 28.1 Implement the Worker sharing path (C6)
    - `worker.rs` gains `on_export` (export filters, renaming, Provenance_Record, pool item), `on_pool_item` (admission and delivery or hold) and `pending_release` (re-run and timeout of the pending queue)
    - Per-Job Unix socket with the `solver-channel` format of task 28.13; canonicalization of the Job's Printed_Goal; export filters and renaming; Provenance_Records with the default Dependency_Set; identifiers and tags; Job registration and Job-end messages; admission of received Lemmas; pending queue with `<TBD-41>` and `<TBD-42>`; delivered set recorded before each write; import list and import-set hash in Results; continue without exchange when the Lemma_Pool fails
    - _Requirements: 11.1, 11.4, 11.6, 11.7, 11.8, 11.9, 11.10, 11.12, 11.13, 11.14, 11.15, 11.16, 11.17, 12.1, 12.2, 12.3, 12.4, 17.13, 17.16, 17.17, 17.18, 17.19, 19.5_
    - _Properties: 14, 20, 21_

  - [ ] 28.2 Write the Sharing_Solver patches
    - `patches/cvc5/` and `patches/z3/`: export and import over the socket in the `solver-channel` format, let-free printing of Lemmas and Instantiation_Hint terms (AS-40, D-14); Lock_File entries of kind sharing-solver; Alt-Ergo stays an Unpatched_Solver without sharing (D-18)
    - _Requirements: 11.1, 38.1, 38.2, 38.3, 38.4, 36.1, 36.8_
    - _Properties: 36_

  - [ ] 28.3 Implement the Front_End sharing path
    - Import marks from the within-goal sharing option; import resolution by identifier from the Lemma_Pool with hash checks; re-admission pass with Canonical_Names from `print_local(B)` only; extended problem with imported Lemma texts; Lemma proof requests; Session-end; Lemma_Pool container added to the integration cluster
    - _Requirements: 11.2, 11.5, 14.9, 14.10, 17.14, 21.2, 21.3, 21.5, 16.7, 10.3, 12.10_
    - _Properties: 17, 20_

  - [ ] 28.4 Write the pending-queue tests
    - **Property 21: The pending queue is bounded and drains** (P-ID tests)
    - `crates/dp-core/tests/prop_pending.rs` and `tests/sim/two_job_default_deps.rs`
    - **Validates: Requirements 11.14, 11.15, 11.16, 11.17**

  - [ ] 28.5 Write the umbrella property test for Property 21
    - **Property 21: The pending queue is bounded and drains**
    - `property_21_pending_queue` in `prop_pending.rs`
    - **Validates: Requirements 11.14, 11.15, 11.16, 11.17**

  - [ ] 28.6 Write the forwarding simulation scenario
    - **Property 9: Every Escalated_Goal gets a Verdict in bounded time** (forwarding and Lemma_Pool faults)
    - `tests/sim/forwarding.rs`: forwarding within `<TBD-43>` under F1 to F3; Lemma_Pool faults end with one Verdict per Goal_Version
    - **Validates: Requirements 11.9, 11.11**

  - [ ] 28.7 Write the hostile Lemma_Pool simulation scenario
    - **Property 22: The Lemma_Pool stays within its bounds** (hostile input)
    - `tests/sim/hostile_pool.rs`
    - Switch row R3.6 to in scope with the switch step (Notes): `hostile_pool.rs` is its last artifact, after `kani/pool.rs`, `prop_pool.rs` and `fuzz/lemma_pool_ingest/`; registry entries for `insert`, `delete_session` and `restart` (R7.11 criterion 12)
    - **Validates: Requirements 16.2, 16.3, 16.5, 16.7, 16.8**

  - [ ] 28.8 Write the lying-provenance simulation scenario
    - **Property 17: Certification inputs are re-derived locally** (shell part)
    - `tests/sim/lying_provenance.rs`: Workers, Coordinator and Lemma_Pool send wrong Canonical_Names, instances and formulas; labels use only local re-derivations
    - Switch rows R3.2, R3.4, R4.1 and R7.1 to in scope with the switch step (Notes): `lying_provenance.rs` is the last artifact of all four, after `prop_provenance.rs`, `prop_admission.rs`, `admission_lean_vs_rust/`, `prop_rename.rs` and `Certifiable.lean`; `targets.toml` gains `admit`, `compose_plan`, `canonicalize`, `record_verdict`; registry entries for `prov_wf`, `default_deps`, `admit`, `readmit`, `label`, `record_verdict` and `accept_result` serving these rows (R7.11 criterion 12)
    - **Validates: Requirements 17.14, 14.9, 14.10, 21.3**

  - [ ] 28.9 Write the within-goal sharing differential test
    - **Property 33: Turning a feature on or off never flips a proof against a counter-model** (within-goal part)
    - `tests/diff/sharing_on_off/`
    - **Validates: Requirements 11.2, 11.5**

  - [ ] 28.10 Extend the real-tool certification tests to Lemma-dependent Results
    - **Property 24: Composed certificates follow the plan and derive false from B's Assertions** (real proofs)
    - `tests/integration/certs/`: Lemma refutations closed by scope steps (P5.2.12) and accepted by Ethos and Carcara
    - **Validates: Requirements 22.3, 22.20, 12.8**

  - [ ] 28.11 Add the within-goal canary mode and Lemma canaries
    - **Property 32: Canaries never reach a proof label** (within-goal mode)
    - Lemma canaries of every kind with confirmed models of Δ ∪ U ∪ {¬L}, delivered through the Lemma_Pool
    - **Validates: Requirements 33.4, 33.8, 33.14**

  - [ ] 28.12 Keep Alt-Ergo Jobs out of Lemma sharing (D-18)
    - Front_End: a Job whose Solver_Configuration names Alt-Ergo is submitted without the import mark, also when the within-goal sharing option lists that Solver_Configuration as importing; Worker: such a Job runs on the Unpatched_Solver path of `worker.rs`, with no Lemma or Instantiation_Hint export to the Lemma_Pool and no delivery to its solver; Alt-Ergo still races under R2.2
    - `prop_forward.rs` cases over generated within-goal sharing option values and Portfolios that include Alt-Ergo: no import mark, no delivery, no pool item exported from an Alt-Ergo Job (P3.1.15)
    - _Requirements: 11.18, 11.19, 11.2_
    - _Properties: 20_

  - [ ] 28.13 Implement the `solver-channel` format (C6, C16)
    - Grammar in `docs/formats/`, codec module in dp-formats and registry entry in `docs/formats/index.toml` for the export and import messages that a Worker exchanges with a Sharing_Solver over the per-Job Lemma socket (exported Lemmas and Instantiation_Hint terms in source names, admitted Lemmas and instances to write)
    - Round-trip, strictness and size-limit cases in `crates/dp-formats/tests/roundtrip.rs`; fuzz target `fuzz/solver_channel/`, run by `ci/fuzz-smoke` and `sched/fuzz-nightly`
    - _Requirements: 11.1, 11.10, 20.1, 20.14, 39.1, 39.2, 39.3, 39.4, 39.5, 39.6, 39.7_
    - _Properties: 34_

- [ ] 29. Implement partitioning behind the `partitioning` option (Could, OQ-N14, D-19)
  - [ ] 29.1 Prove the partition theorems in Lean
    - **Property 26: Partitioned Goal_Versions are proved only by a checked cover** (metatheory part)
    - `proofs/lean/DP/Partition.lean` (`partition_sound`, `split_tree_covers`, `compose_partition_sound`, `verdict_sound_partition`, which also covers Property 2 for leaf-Result sets), built by `ci/lean-partition`
    - **Validates: Requirements 15.2, 22.10, 22.11**

  - [ ] 29.2 Implement Partitions across dp-core, dp-cert and the Front_End
    - Partition tree format with its grammar, codec, round-trip cases in `crates/dp-formats/tests/roundtrip.rs` (P3.5.7), `fuzz/partition_tree/` (`ci/fuzz-smoke`, `sched/fuzz-nightly`) and registry entry, and `tree_hash`; `race.rs` `partition_jobs`; Partition facts in `accept.rs`; R3.4 criterion 3 for Partition_Assumptions in admission; `compose_plan` step 7; `crates/dp-cert/src/partition.rs` (A3, one excluded-middle step per internal node, leaf scopes); `label_of_leaf_set`; replay fields per leaf in `proof.json`
    - _Requirements: 15.1, 15.2, 15.3, 15.4, 15.5, 15.6, 15.7, 15.8, 15.9, 9.21, 10.6, 14.3, 21.10, 22.10, 22.11, 22.12, 22.21_
    - _Properties: 7, 26_

  - [ ] 29.3 Write the partition property tests and canaries
    - **Property 26: Partitioned Goal_Versions are proved only by a checked cover**
    - `crates/dp-cert/tests/prop_partition.rs` with umbrella `property_26_checked_cover`; `tests/canaries/partition/` and the partitioning canary mode; fills `arb_partition_tree`
    - Switch rows R3.5 and R5.2 to in scope with the switch step (Notes): `prop_partition.rs` is the last artifact of both, and `tests/canaries/partition/` of R3.5, after `Partition.lean`, `dp-cert/src/partition.rs` and the `partitioning` option tests (29.7); registry entries for `partition_jobs`, `compose_plan` and `fragment_ok` serving these rows (R7.11 criterion 12)
    - **Validates: Requirements 15.2, 15.6, 22.10, 22.11, 22.12, 22.21**

  - [ ] 29.4 Extend the Race property tests to Partitions
    - **Property 7: Races decide by their first proof label** (Partition part, P3.5.5)
    - `prop_race.rs`: a single Partition Result never wins; a certified leaf-Result set ends the Race
    - **Validates: Requirements 9.21, 15.7, 15.8**

  - [ ] 29.5 Extend the adversarial simulation to Partition Results
    - **Property 3: Accepted Results are bound to the local print, the submission and an authentic Worker** (Partition part)
    - Partition Results with added, dropped or swapped Partition_Assumptions, unknown Partitions and wrong Drivers (P3.5.3, P3.5.8); extends `arb_result`
    - **Validates: Requirements 15.4, 15.5, 15.9**

  - [ ] 29.6 Extend the scope-switch self-test to the stage-6 rows
    - Seeded mutations in the self-test of task 2.3 for each row that stage 6 switched (R7.10 in 26.8, R3.3 in 26.6, R3.6 in 28.7, R3.2, R3.4, R4.1 and R7.1 in 28.8, R3.5 and R5.2 in 29.3): back to deferred fails `ci/traceability` (R7.11 criterion 12); a deleted artifact fails it (criterion 2); a deleted registry entry of one of the rows' decision functions fails `ci/purity` (R7.3 criterion 4); `admit`, `canonicalize` or `canonical_check` removed from `targets.toml` fails `ci/traceability` (R7.13 criterion 6)
    - _Requirements: 40.2, 40.12, 32.4, 42.6_
    - _Properties: 37_

  - [ ] 29.7 Gate partitioning behind the `partitioning` option, off by default (D-19)
    - `partitioning` option of `dp.toml` with the default off in the project-configuration codec and `validate_config`; the Front_End treats partitioning as enabled only when the option is on, and otherwise the Scheduler records no Partition tree and `partition_jobs` creates no Job for a Partition
    - `prop_race.rs` cases with the option unset, off and on (P3.5.10)
    - _Requirements: 15.10, 15.1, 9.21_
    - _Properties: 7_

- [ ] 30. Checkpoint: dp-within-goal-sharing gate
  - `ci/lean` (`admission_sound`, `admission_sound_seq`, `canon_injective`, `canon_equisat`), `ci/lean-partition`, `ci/creusot` (admission, provenance), `ci/tlc` (`Sharing.tla`), `ci/diff` (`sharing_on_off/`, `canon_*`, `solver_names/`, `admission_lean_vs_rust/`), `ci/sim` (hostile pool, lying provenance, two-Job, forwarding), `ci/canaries` (within-goal and partitioning modes), `ci/kani` (`pool.rs`, `parsers.rs`), `ci/fuzz-smoke` (the stage-6 targets, `solver_channel/` included) and every earlier job pass; `ci/traceability` is green with R3.2, R3.3, R3.4, R3.5, R3.6, R4.1, R5.2, R7.1 and R7.10 in scope; no shipped configuration enables import marks unless the stage-5 gate passed
  - Ensure all tests pass, ask the user if questions arise.

### Stage 7: dp-cross-goal-sharing

- [ ] 31. Implement cross-goal sharing among Sibling_Goals and ground Lemmas for quantified goals
  - [ ] 31.1 Prove `cross_goal_sound` in Lean
    - **Property 12: Admitted Lemmas follow from the target problem (metatheory)** (cross-goal part)
    - `cross_goal_sound` (P4.2.5) in `Admission.lean`, with quantified Γ_b and Δ (P4.3.2)
    - **Validates: Requirements 18.5, 19.1, 19.4**

  - [ ] 31.2 Implement sibling groups and cross-goal forwarding and admission
    - Sibling-group rule recorded in `docs/formats/sibling-group.abnf`; the Front_End derives the sibling group from the goal path by that rule at the configured scope and, for a goal path that does not match it, uses the why3find file of the goal path and logs the fallback with the goal path (D-10, AS-29); sibling group in submissions and Job registrations; `forward_ok` cross-goal branch; cross-goal checks in `admit` (negated-goal flag); one log entry per cross-goal decision; cross-goal requires within-goal sharing
    - _Requirements: 18.1, 18.2, 18.3, 18.4, 18.5, 18.6, 18.7, 18.8, 18.9_
    - _Properties: 13, 20_

  - [ ] 31.3 Write the scope property tests and the cross-goal model invariants
    - **Property 20: Lemmas flow only within their scope** (cross-goal part)
    - `crates/dp-core/tests/prop_scope.rs`; cross-goal invariants P4.2.1 and P4.2.2 in `Sharing.tla`
    - **Validates: Requirements 18.2, 18.3, 18.4, 18.7**

  - [ ] 31.4 Extend the admission tests to cross-goal Lemmas
    - **Property 13: The admission procedure is the specified function** (cross-goal part)
    - `prop_admission.rs` and `admission_lean_vs_rust/` cases with equal goal path and base Task_Hash but different Goal_Versions
    - **Validates: Requirements 18.5, 18.6, 19.1, 19.4**

  - [ ] 31.5 Write the Ground_Lemma property tests
    - **Property 14: Admitted Lemmas are hygienic** (Ground_Lemma part)
    - `crates/dp-core/tests/prop_ground.rs`
    - **Validates: Requirements 19.2, 19.3, 19.5**

  - [ ] 31.6 Write the cross-goal differential test
    - **Property 33: Turning a feature on or off never flips a proof against a counter-model** (cross-goal part)
    - `tests/diff/cross_goal_on_off/`
    - Switch row R4.3 to in scope with the switch step (Notes): `cross_goal_on_off/` is its last artifact, after `prop_ground.rs` (31.5); `targets.toml` gains `is_ground_lemma`; registry entries for `is_ground_lemma`, `export_filter`, `hygiene_ok` and `admit` serving R4.3 (R7.11 criterion 12)
    - **Validates: Requirements 18.2, 18.3**

  - [ ] 31.7 Write the sibling-group derivation tests
    - **Property 20: Lemmas flow only within their scope** (sibling-group part)
    - `prop_scope.rs` cases over generated why3find goal paths that match and do not match `docs/formats/sibling-group.abnf`, at the function and module scopes: a matching path gets the identifier that the rule derives and no fallback log entry; a non-matching path gets its why3find file and exactly one fallback log entry naming it (P4.2.8)
    - **Validates: Requirements 18.9, 18.1**

- [ ] 32. Implement Instantiation_Hints
  - [ ] 32.1 Prove `hint_sound` in Lean
    - **Property 19: Instantiation_Hints yield only entailed instances** (metatheory part)
    - `proofs/lean/DP/Hints.lean` with reference `instantiate` and `admit_hint`, added to `dp-lean-ref`
    - **Validates: Requirements 20.1**

  - [ ] 32.2 Implement `hints.rs` and the hint parser with Creusot contracts (C9, C10)
    - `admit_hint` (checks of R4.4 criterion 9 in order), `instantiate` (simultaneous substitution of closed terms, top-level patterns and non-pattern attributes removed, nested patterns kept), `hint_export_filter`; hint format with size limit `<TBD-35>`; Kani `parsers.rs` extended; `fuzz/hint_parser/` (`ci/fuzz-smoke`, `sched/fuzz-nightly`)
    - Round-trip cases in `crates/dp-formats/tests/roundtrip.rs` and registry entries for Instantiation_Hint terms and messages
    - _Requirements: 20.1, 20.4, 20.9, 20.10, 20.11, 20.12, 20.13, 39.5, 39.7_
    - _Properties: 19, 34_

  - [ ] 32.3 Write the hint property and differential tests
    - **Property 19: Instantiation_Hints yield only entailed instances**
    - `crates/dp-core/tests/prop_hints.rs` with umbrella `property_19_hint_instances`; `tests/diff/hints_lean_vs_rust/`; fills `arb_hint`
    - **Validates: Requirements 20.1, 20.4, 20.9, 20.10, 20.11, 20.12, 20.13**

  - [ ] 32.4 Wire hints through the Worker, Front_End, composer and store
    - Worker: hint export with provenance, instance computed from the Job's own Printed_Goal and written as an assertion; hints counted as delivered Lemmas. Front_End: hint re-admission and instance recomputation on `print_local(B)`, extended problem. `compose_plan` step 6 and dp-cert universal-instantiation steps in the checker's form (R4.4 criterion 20). Certificate_Store audit records of hints' used Lemmas
    - _Requirements: 20.2, 20.3, 20.5, 20.6, 20.7, 20.8, 20.14, 20.15, 20.16, 20.17, 20.18, 20.19, 20.20, 22.2, 22.3_
    - _Properties: 15, 19, 24_

  - [ ] 32.5 Extend the provenance tests to hints and audit records
    - **Property 15: Provenance is well-formed, authentic and preserved** (hint and audit part)
    - `prop_provenance.rs` hint cases (Dependency_Set {n}, flag false); audit-trail checks in `tests/replay/`
    - **Validates: Requirements 20.2, 20.3, 20.5, 20.7, 20.8, 20.19**

  - [ ] 32.6 Extend the real-tool certification tests to hint-using Results
    - **Property 19: Instantiation_Hints yield only entailed instances** (checker form, P4.4.13)
    - `tests/integration/certs/` with hint-using Results accepted by the pinned checkers
    - **Validates: Requirements 20.17, 20.18, 20.20**

  - [ ] 32.7 Write the hint-sharing differential test
    - **Property 33: Turning a feature on or off never flips a proof against a counter-model** (hint part)
    - `tests/diff/hints_on_off/`
    - **Validates: Requirements 20.13, 20.14**

- [ ] 33. Complete the certifiability theorem and the cross-goal canary modes
  - [ ] 33.1 Discharge every premise of `certifiable` in Lean
    - **Property 12: Admitted Lemmas follow from the target problem (metatheory)** (certifiability)
    - `Certifiable.lean` uses `admission_sound`, `admission_sound_seq`, `canon_injective`, `cross_goal_sound` and `hint_sound`; `ci/lean` checks that `certifiable` has no hypotheses beyond AS-12
    - **Validates: Requirements 14.8, 18.5, 19.4, 30.3**

  - [ ] 33.2 Add the cross-goal and hint canary modes
    - **Property 32: Canaries never reach a proof label** (cross-goal and hint modes)
    - Hint canaries of every kind rejected with their recorded reason at the Worker and at re-admission; `modes.toml` complete
    - **Validates: Requirements 33.4, 33.6, 33.10, 33.17**

  - [ ] 33.3 Extend the scope-switch self-test to the stage-7 row
    - Seeded mutations in the self-test of task 2.3 for R4.3 (switched in 31.6): back to deferred fails `ci/traceability` (R7.11 criterion 12); a deleted artifact fails it (criterion 2); a deleted registry entry of `is_ground_lemma` fails `ci/purity` (R7.3 criterion 4); `is_ground_lemma` removed from `targets.toml` fails `ci/traceability` (R7.13 criterion 6)
    - _Requirements: 40.2, 40.12, 32.4, 42.6_
    - _Properties: 37_

- [ ] 34. Checkpoint: dp-cross-goal-sharing gate
  - `ci/lean` (`hint_sound`, `cross_goal_sound`, `certifiable` with every premise proved and no hypotheses beyond AS-12), `ci/creusot` (hints), `ci/diff` (`cross_goal_on_off/`, `hints_on_off/`, `hints_lean_vs_rust/`), `ci/certs` (P4.4.13), `ci/canaries` (cross-goal and hint modes) and every earlier job pass; `ci/traceability` is green with R4.3 in scope (R4.2 and R4.4 switch in task 35.3, because their last artifact is `eval/scripts/rq4_cross_goal/`)
  - Ensure all tests pass, ask the user if questions arise.

### Stage 8: dp-evaluation, studies

- [ ] 35. Implement the Experiment_Runner, the research-question scripts and the report pipeline
  - [ ] 35.1 Write the pre-registration templates and parser
    - `eval/rq/templates/rq1.toml` to `rq7.toml` with fields (a) to (l) of R6.2 and the hypothesis targets as named placeholders `<TBD-27>`, `<TBD-28>`, `<TBD-29>`, `<TBD-39>`; parser that refuses a run when `eval/rq/<q>.toml` is absent, fails to parse, lacks a field or holds a placeholder (smoke runs read the templates); pre-registration hash recorded in manifests; pre-registration codec with round-trip case, fuzz target and registry entry (R7.10 criterion 7)
    - _Requirements: 27.1, 27.2, 27.3, 27.11, 27.17, 39.7_
    - _Properties: 39_

  - [ ] 35.2 Implement the Experiment_Runner core (`eval/scripts/runner/`, C19)
    - Python pinned through the Lock_File (TB-15); `derive(s, c, g, i)` seeds disjoint from the hardness measurement's; `<TBD-17>` repetitions; cold starts with empty caches, Certificate_Store and Lemma_Pool; run and submission orders from the pre-registration; every component run with the tracing retention setting complete (R1.5 criterion 19); statuses failed and incomplete; binary-hash failures; smoke flag with one repetition; separate canary pass per configuration that fails the run on any canary hit
    - Raw-data layout of the design: run manifest `eval/raw/<q>/<run-id>/manifest.json` (canonical JSON with the fields of R6.3 criterion 4) and raw records `eval/raw/<q>/<run-id>/records.jsonl` (one canonical JSON record per goal, configuration, repetition and Worker, with the canary tag); manifest and record codecs with round-trip cases in `crates/dp-formats/tests/roundtrip.rs` (P6.3.6), fuzz targets and registry entries
    - _Requirements: 28.1, 28.2, 28.3, 28.4, 28.5, 28.6, 28.7, 28.8, 28.9, 28.10, 28.11, 28.12, 28.13, 28.14, 28.15, 28.16, 33.12, 33.13, 5.19, 39.7_
    - _Properties: 32, 39_

  - [ ] 35.3 Write the research-question scripts RQ1 to RQ7
    - `eval/scripts/rq1_distribution/` (Worker counts `<TBD-23>`), `rq2_portfolio/`, `rq3_within_goal/`, `rq4_cross_goal/`, `rq5_certificates/`, `rq6_instability/` (cross-Worker reproduction and replay agreement through dp-replay), `rq7_escalation/`; each writes `eval/raw/<q>/<run-id>/manifest.json` and `records.jsonl` through the runner core; metrics per hardness class from non-canary records only; hardness re-measurement with Wilson intervals; budget `<TBD-37>` for censoring
    - Switch rows R1.2, R1.4, R2.2, R3.1, R4.2, R4.4, R5.1, R5.5 and R7.6 to in scope with the switch step (Notes): an RQ script is the last artifact of each; `targets.toml` gains `admit_hint`, `instantiate`, `oblig`, `import_closure`; registry entries for `escalation.rs`, `calibration.rs`, `race.rs` (R2.2 functions), the sharing functions of `worker.rs`, `forward_ok`, `hints.rs` and `certification.rs`, and `label` serving R5.5 (R7.11 criterion 12); these rows name `sched/eval` and stay pending until it has an acceptable run
    - _Requirements: 27.4, 27.5, 27.6, 27.7, 27.8, 27.9, 27.10, 27.12, 27.13, 27.14, 27.15, 27.16_
    - _Properties: 39_

  - [ ] 35.4 Write the evaluation pipeline tests
    - **Property 39: Evaluation numbers follow the pre-registration and come from raw data** (pipeline part)
    - `eval/tests/` (pre-registrations, manifests, seeds, cold starts, run order, smoke never eligible) and `eval/tests/metrics/` with Hypothesis; fills `arb_raw_eval` fixtures
    - Switch row R6.2 to in scope with the switch step (Notes): `eval/tests/metrics/` is its last artifact, after the RQ scripts (35.3) (R7.11 criterion 12)
    - **Validates: Requirements 27.1, 27.2, 27.3, 27.11, 27.12, 27.14, 28.2, 28.3, 28.4, 28.9, 28.10, 28.12, 28.13, 28.14, 28.15, 28.16**

  - [ ] 35.5 Write the evaluation canary-gate tests
    - **Property 32: Canaries never reach a proof label** (evaluation runs)
    - `eval/tests/canary_gate/`: the release's Canary_Suite is used and any canary hit fails the run and sets the manifest status
    - Switch row R7.4 to in scope with the switch step (Notes): `eval/tests/canary_gate/` is its last artifact, after `tests/certs/tamper/` (23.2) (R7.11 criterion 12)
    - **Validates: Requirements 28.6, 28.7, 28.8, 33.12, 33.13**

  - [ ] 35.6 Implement the report generator (`eval/report/`, C19)
    - Every number from raw data of used runs; `eval/report/provenance.jsonl` with the report-provenance codec, round-trip case, fuzz target and registry entry; intervals at `<TBD-18>` with bootstrap resamples `<TBD-38>`; held or not held for primary metrics under the decision rule with Holm adjustment; counts of goals and repetitions; ineligible runs with reasons; eligible runs and pre-registration commits
    - _Requirements: 29.1, 29.4, 29.6, 29.7, 29.8, 29.15_
    - _Properties: 39_

  - [ ] 35.7 Write the report checks with seeded changes
    - **Property 39: Evaluation numbers follow the pre-registration and come from raw data** (report part)
    - `eval/report/check/` (byte-for-byte regeneration, numeric-literal allow-list, provenance hashes and eligibility, pre-registration ancestry, used runs, decision-rule agreement, intervals and counts, canary-tag checks, P6.3 checks of committed runs under `eval/raw/<q>/<run-id>/`) and `eval/tests/report_check/` with each seeded change of P6.4.5
    - Switch rows R6.3 and R6.4 to in scope with the switch step (Notes): `eval/report/check/` and `eval/tests/report_check/` are their last artifacts, after `eval/tests/canary_gate/` (35.5) and `eval/report/` (35.6) (R7.11 criterion 12)
    - **Validates: Requirements 28.17, 29.1, 29.2, 29.3, 29.4, 29.5, 29.6, 29.7, 29.8, 29.9, 29.10, 29.11, 29.12, 29.13, 29.14, 29.15**

  - [ ] 35.8 Implement the `ci/eval-smoke`, `ci/eval-report` and `sched/eval` job scripts
    - `ci/eval-smoke` runs every script and the hardness measurement once on the CI corpus with the smoke flag; `ci/eval-report` re-checks committed raw data and regenerates the report; `sched/eval` runs the pre-registered experiments and declares its input paths; without any pre-registration `eval/rq/<q>.toml` it records no run, so the rows that name it stay pending (R7.11 criterion 8) instead of passing on no input
    - _Requirements: 28.16, 28.17, 29.2, 40.10_
    - _Properties: 39_

  - [ ] 35.9 Write the umbrella property test for Property 39
    - **Property 39: Evaluation numbers follow the pre-registration and come from raw data**
    - `eval/tests/test_property_39.py` with Hypothesis over generated raw data and manifests
    - **Validates: Requirements 27.14, 28.2, 29.2**

  - [ ] 35.10 Extend the scope-switch self-test to the stage-8 rows and check full scope
    - Seeded mutations in the self-test of task 2.3 for each row that stage 8 switched (the nine rows of 35.3, R6.2 in 35.4, R7.4 in 35.5, R6.3 and R6.4 in 35.7): back to deferred fails `ci/traceability` (R7.11 criterion 12); a deleted artifact fails it (criterion 2); a deleted registry entry fails `ci/purity` (R7.3 criterion 4); `admit_hint`, `instantiate`, `oblig` or `import_closure` removed from `targets.toml` fails `ci/traceability` (R7.13 criterion 6)
    - A repository test in `tools/traceability-check/` that no row other than R6.1 is deferred, so the purity registry now covers every decision function of R7.3 criterion 1
    - _Requirements: 40.2, 40.12, 32.4, 42.6_
    - _Properties: 37_

- [ ] 36. Final checkpoint: evaluation gate and full traceability
  - `ci/eval-smoke`, `ci/eval-report` and every earlier job pass; `ci/traceability` is green with every row except R6.1 in scope (R6.1 switches in the commit that adds a filled `eval/rq/corpus.toml`, see checkpoint 13)
  - Rows that name `sched/eval` or `sched/canaries-full` stay pending until those jobs have acceptable runs; `ci/traceability` reports them and fails only release-tagged commits (R7.11 criterion 8). The design's gate for each research question also names an eligible `sched/eval` run; that needs filled pre-registrations with decided TBD values, which are decisions rather than coding tasks, so it is outside this checkpoint
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- **Optional sub-tasks.** No sub-task is marked optional (`*`). Every sub-task was checked against the Verification lines of `requirements.md`: each check that a Verification line names, each sub-task that is the only check of a criterion, and each A1 proof, A2 model check, A3 per-run check, canary, fuzz target and mutation gate is required, because `ci/traceability` fails for an in-scope row whose artifact is missing. The dp-shell adapter tests (1.6) and the status-report rendering test (16.5) back checks that R1.5 and R1.7 now name. The umbrella property tests of Properties 6, 8, 9, 21, 22, 38 and 39 and the pretty-printer snapshot test (3.6) are not named by a Verification line on their own and stay required as well.
- **Switch step.** Switching a row to in scope means, in one commit: set the row's scope status in `docs/traceability.toml`; regenerate `docs/traceability.md` (R7.11 criterion 5); add `tools/purity-check/registry.toml` entries, with cases in `crates/dp-core/tests/determinism.rs`, for the decision functions of R7.3 criterion 1 that serve the row (R7.3 criterion 4); and, for a Soundness-critical row, regenerate `tools/mutants/targets.toml` from the `functions` fields (R7.13 criterion 6). Sub-tasks that perform a switch step write these shared files, so the dependency graph puts them in different waves.
- **Scope switching.** `requirements.md` (R7.11 criterion 12 and its Scope switching section) fails a deferred row whose artifacts all exist, so each row switches in the sub-task that adds the last artifact its Verification lines name: stage 1: R7.11 and R7.13 (2.2), R7.9 (2.7), R7.3 (4.9), R7.12 (9.4), R2.3 (9.5); stage 2: none; stage 3: R2.1 (17.1), R1.6 (17.5), R1.3 and R1.7 (17.2), R1.5 (17.4), R1.1 (17.7); stage 4: R7.2 (19.8); stage 5: R7.5 (23.1), R5.4, R7.7 and R7.8 (23.4), R5.3 (23.2); stage 6: R7.10 (26.8), R3.3 (26.6), R3.6 (28.7), R3.2, R3.4, R4.1 and R7.1 (28.8), R3.5 and R5.2 (29.3); stage 7: R4.3 (31.6); stage 8: R1.2, R1.4, R2.2, R3.1, R4.2, R4.4, R5.1, R5.5 and R7.6 (35.3), R6.2 (35.4), R7.4 (35.5), R6.3 and R6.4 (35.7); outside the plan: R6.1, which switches in the commit that adds a filled `eval/rq/corpus.toml` (checkpoint 13). design.md's Delivery Plan sentence about switching at stage start disagrees with requirements.md R7.11 and should be updated.
- **Rows that switch outside their Initial Traceability Table stage.** R2.3 switches in stage 1 and R2.1 in stage 3, not stage 4, because every artifact path that their Verification lines name exists earlier; R6.1 waits for `eval/rq/corpus.toml`. Because an artifact path can be a directory, or a file that later tasks extend, a row can be in scope before every check that its Verification lines describe has been written (for example R2.1 before tasks 19.1 to 19.3); the named CI jobs gate those checks from the commit that adds them. Checks of deferred rows still run in their CI jobs from the stage that writes them. Each stage's self-test sub-task (10.4, 12.8, 17.10, 19.11, 23.15, 29.6, 33.3, 35.10) seeds mutations for the rows that stage switched.
- **Format registry.** R7.10 criterion 7 fails on an exchanged format without a registry entry whatever the scope status of its requirements, which read literally fails `ci/traceability` until the last format (report provenance entries, stage 8) is registered. Task 2.2 therefore checks on every commit that each registry entry names an existing grammar file, codec module, round-trip test and fuzz target, and applies the missing-entry part once a requirement that R7.10's format list cites for the format is in scope (R7.10 itself for `dispatch`, which cites none). Each format's entry is added in the sub-task that adds the last of its four artifacts, before the switch of any requirement it cites. This is an interpretation: requirements.md R7.10 criterion 7 should state the same scope rule, or the user may choose another one.
- **TBD values.** Every numeric parameter is configuration named `<TBD-n>`, including `<TBD-46>`, the number of trace records each component keeps under the bounded tracing retention setting. TLA+, simulation, smoke and fixture configurations use test values that are marked as not proposals.
- **CI corpus.** Until an eligible `sched/eval` corpus run exists, differential tests read the bootstrap CI corpus of task 12.7, which is generated with the smoke flag and is never eligible evidence.
- **Generators.** dp-testkit declares every generator module in task 1.7; the task that first needs a generator fills it, and later tasks only extend it in later waves.

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1"] },
    { "id": 1, "tasks": ["1.2", "1.4", "1.5", "1.7"] },
    { "id": 2, "tasks": ["1.3", "1.6", "2.1", "2.6", "2.8", "3.1"] },
    { "id": 3, "tasks": ["2.2", "2.4", "3.2"] },
    { "id": 4, "tasks": ["2.3", "2.7", "2.9", "2.10", "3.3", "3.6", "4.1"] },
    { "id": 5, "tasks": ["2.5", "3.4", "3.5", "4.2", "4.4", "4.5", "5.1", "5.2", "8.1"] },
    { "id": 6, "tasks": ["4.3", "4.11", "5.3", "8.2"] },
    { "id": 7, "tasks": ["4.9", "6.1", "8.3"] },
    { "id": 8, "tasks": ["4.6", "4.7", "4.8", "6.2", "6.3", "8.4"] },
    { "id": 9, "tasks": ["4.10", "6.4"] },
    { "id": 10, "tasks": ["6.5", "7.1"] },
    { "id": 11, "tasks": ["7.2", "9.1", "10.1"] },
    { "id": 12, "tasks": ["9.2", "9.4", "10.2", "10.3"] },
    { "id": 13, "tasks": ["9.3", "9.5", "9.6"] },
    { "id": 14, "tasks": ["10.4"] },
    { "id": 15, "tasks": ["12.1"] },
    { "id": 16, "tasks": ["12.2"] },
    { "id": 17, "tasks": ["12.3", "12.5"] },
    { "id": 18, "tasks": ["12.4", "12.7"] },
    { "id": 19, "tasks": ["12.6", "12.8"] },
    { "id": 20, "tasks": ["14.1", "15.1", "15.4", "16.1"] },
    { "id": 21, "tasks": ["14.2", "15.2", "15.5", "15.6"] },
    { "id": 22, "tasks": ["15.3", "15.7", "15.10", "15.12", "16.6"] },
    { "id": 23, "tasks": ["15.8", "15.9", "15.11", "15.13", "16.2"] },
    { "id": 24, "tasks": ["16.3", "16.4"] },
    { "id": 25, "tasks": ["16.5", "17.1"] },
    { "id": 26, "tasks": ["17.3", "17.5", "17.9"] },
    { "id": 27, "tasks": ["17.2", "17.6"] },
    { "id": 28, "tasks": ["17.4", "17.8"] },
    { "id": 29, "tasks": ["17.7"] },
    { "id": 30, "tasks": ["17.10"] },
    { "id": 31, "tasks": ["19.1", "19.4"] },
    { "id": 32, "tasks": ["19.2", "19.3", "19.5"] },
    { "id": 33, "tasks": ["19.6", "19.7"] },
    { "id": 34, "tasks": ["19.8", "19.9", "19.10"] },
    { "id": 35, "tasks": ["19.11"] },
    { "id": 36, "tasks": ["21.1", "21.5", "22.1", "22.3", "23.3"] },
    { "id": 37, "tasks": ["21.2", "21.4", "22.2", "22.4", "22.5"] },
    { "id": 38, "tasks": ["21.3", "22.6", "23.1"] },
    { "id": 39, "tasks": ["23.4", "23.5"] },
    { "id": 40, "tasks": ["23.2", "23.7", "23.9", "23.16"] },
    { "id": 41, "tasks": ["23.6", "23.8", "23.10", "23.11", "23.12", "23.14"] },
    { "id": 42, "tasks": ["23.13"] },
    { "id": 43, "tasks": ["23.15"] },
    { "id": 44, "tasks": ["25.1", "25.2", "26.1", "27.1"] },
    { "id": 45, "tasks": ["25.3", "25.4", "26.2"] },
    { "id": 46, "tasks": ["25.5", "26.3", "26.4", "27.2"] },
    { "id": 47, "tasks": ["26.5", "27.3", "27.5", "28.13"] },
    { "id": 48, "tasks": ["26.7", "26.8", "27.4"] },
    { "id": 49, "tasks": ["26.6", "27.6", "28.1", "28.2"] },
    { "id": 50, "tasks": ["28.3", "28.4"] },
    { "id": 51, "tasks": ["28.5", "28.6", "28.7", "28.9", "28.10", "28.12"] },
    { "id": 52, "tasks": ["28.8", "28.11"] },
    { "id": 53, "tasks": ["29.1", "29.2"] },
    { "id": 54, "tasks": ["29.5", "29.7"] },
    { "id": 55, "tasks": ["29.3", "29.4"] },
    { "id": 56, "tasks": ["29.6"] },
    { "id": 57, "tasks": ["31.1", "31.2", "32.1"] },
    { "id": 58, "tasks": ["31.3", "31.4", "31.5", "32.2"] },
    { "id": 59, "tasks": ["31.6", "31.7", "32.3", "32.4"] },
    { "id": 60, "tasks": ["32.5", "32.6", "32.7", "33.1"] },
    { "id": 61, "tasks": ["33.2"] },
    { "id": 62, "tasks": ["33.3"] },
    { "id": 63, "tasks": ["35.1"] },
    { "id": 64, "tasks": ["35.2"] },
    { "id": 65, "tasks": ["35.3", "35.6"] },
    { "id": 66, "tasks": ["35.4", "35.8"] },
    { "id": 67, "tasks": ["35.5", "35.9"] },
    { "id": 68, "tasks": ["35.7"] },
    { "id": 69, "tasks": ["35.10"] }
  ]
}
```
