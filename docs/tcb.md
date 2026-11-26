# Trusted computing base

Generated from `docs/tcb.toml` by `tcb-check --write`; do not edit.

## Tools of the Lock_File

| Tool | Trusted for | Items | Reason |
|---|---|---|---|
| alt-ergo | proved-standalone, when its binary is the pinned Unpatched_Solver | NG-3 | An Unpatched_Solver: its answer Valid on the local print is taken as a proof. Not verified; certify-all mode removes this trust. |
| z3 | proved-standalone, when its binary is the pinned Unpatched_Solver | NG-3 | An Unpatched_Solver, as Alt-Ergo. |
| cvc5 | proved-standalone, when its binary is the pinned Unpatched_Solver; nothing as a proof producer | NG-3 | An Unpatched_Solver, as Alt-Ergo. The certificates that it produces are untrusted: the checker decides. |
| cvc4 | proved-standalone, when its binary is the pinned Unpatched_Solver | NG-3 | An Unpatched_Solver, as Alt-Ergo. |
| ethos | certified | NG-4 | The Certificate_Checker: a certificate that it accepts against the local print gives the label certified. |
| cpc-signature | certified | NG-4 | The rules that Ethos checks CPC proofs against; a wrong rule would let a wrong proof pass. |
| why3find-dispatch | every label | NG-2 | Prints every Goal and counts its tasks (TB-23): every label is bound to its print. The dispatch patch is part of what is trusted. |
| creusot | every label | NG-1, NG-4 | Translates the Rust source to the Goals that are proved, and starts the Front_End through the backend patch. |
| why3 | every label | NG-2 | Generates the verification conditions, applies the transformations and prints every Goal with the pinned drivers. |
| carcara | nothing yet | NG-4 | The checker of Alethe proofs. Pinned for the Carcara route of stage 5; no label rests on it until that route exists. |
| lean | the metatheorems, no label | NG-4 | Checks verdict_sound and the reference definitions that the Rust decision functions are compared with. |
| tlc | the protocol models, no label | NG-4 | Model-checks Coordinator.tla and Cluster.tla and validates traces of the Coordinator's shell. |
| apalache | nothing yet | NG-4 | Pinned for the nightly bounded checks of the protocol models; no specification has an Apalache configuration yet. |
| kani | bounded checks of dp-core, no label | NG-4 | Checks the compiled arithmetic of scale and the comparison of Cache_Key encodings within recorded bounds. |
| cargo-mutants | the mutation gate, no label | NG-4 | Makes the mutants that the tests of the soundness-critical functions must catch. |
| rust-toolchain | every label | NG-4 | Compiles the System: the decision core that Creusot proves is trusted as compiled. |
| rust-libraries | every label | NG-4 | The Rust libraries that the System is built with, as Cargo.lock pins them: hashing, signatures, parsing of configuration files. |

## Items

| Item | Kind | Status | Cited by | Summary |
|---|---|---|---|---|
| F1 | fairness |  | R1.5, R1.6, R2.2 | every sent message is eventually delivered or the sender's timeout fires |
| F2 | fairness |  | R1.5, R2.2 | every connected Worker eventually takes steps |
| F3 | fairness |  | R1.5, R1.6, R2.2 | local clocks advance, so timeouts fire |
| F4 | fairness |  | R1.5, R2.2 | the Coordinator follows the protocol |
| AS-1 | assumption | Given |  | The new repository depends on Creusot, Why3, why3find, Alt-Ergo, Z3, cvc5, Ethos and Carcara as Pinned_Dependencies. |
| AS-2 | assumption | To check | R1.1, R7.9 | Creusot's prove command (`cargo creusot prove`) can get, through a small upstreamable patch under `patches/creusot/`, a backend option with which Creusot_CLI runs `dp-frontend prove` with the arguments it would pass to `why3find prove`, for |
| AS-3 | assumption | To check | R1.1, R1.3, R1.6, R1.7 | The dp message protocol that the System defines for the Front_End, the Coordinator, the Workers and the Lemma_Pool (R7.10) can carry, within the size limits of R7.10 criterion 5, the data this project needs: Solver_Configurations with seeds |
| AS-4 | assumption | Given | R1.1, R1.4 | why3find's proof-server mode needs the OCaml `zmq` package and uses each client's prover Calibration to keep results consistent across machines. |
| AS-5 | assumption | To check | R5.1 | cvc5 can emit CPC and Alethe proofs that cover quantifier instantiation, Skolemization and the theories Creusot goals use (uninterpreted functions, integers, algebraic datatypes, arrays, bit-vectors), without Trust_Steps for those theories. |
| AS-6 | assumption | To check | R3.5, R4.4, R5.2, R5.3 | Ethos and Carcara can check a proof whose leaves are a subset of the input problem's assertions, can check a Composed_Certificate in which imported Lemmas appear first as assumptions and then as derived facts, and check every universal-inst |
| AS-7 | assumption | To check | R3.1 | Solvers can export learned Lemmas and accept imported Lemmas during a run, through solver patches or existing interfaces (for example, Z3 clause callbacks, or the cvc5 changes used by SMT-D). |
| AS-8 | assumption | To check | R4.4 | Solvers can report the quantifier instantiations that contributed to a proof, so that Instantiation_Hints can be exported. |
| AS-9 | assumption | To check | R1.3, R4.1 | Why3 printing is deterministic: the same Goal_Version, Driver and Transformation_Pipeline always produce byte-identical Printed_Goals on every machine. |
| AS-10 | assumption | To check | R3.1 | Z3 and cvc5 can run on Printed_Goals from one common Driver without a large loss of proving power, so that Lemmas can flow between Z3 and cvc5 Jobs. |
| AS-11 | assumption | To check | R3.2, R4.2 | Solvers or a proof-producing re-run can supply a Dependency_Set for an exported Lemma that is smaller than the full set of source Assertions. |
| AS-12 | assumption | To check | R3.4, R4.3, R4.4, R5.2, R5.5, R7.1 | A Lean 4 model of many-sorted first-order logic with the SMT-LIB theories used (as uninterpreted or axiomatized symbols where needed) is a faithful enough model of SMT-LIB semantics for the metatheorems in this document. |
| AS-13 | assumption | To check | R7.3 | Creusot can verify the dp-core crate, and Kani can run bounded checks on the same crate, given the Rust features dp-core uses. |
| AS-14 | assumption | To check | R1.5 | TLA+ trace validation (checking that an implementation trace is a behavior of the specification with TLC) is feasible for the Coordinator, Worker and Front_End event logs. |
| AS-15 | assumption | To check | R6.1 | Goals exported from Creusot's test suite, from CreuSAT and from other selected projects may be redistributed under those projects' licenses. |
| AS-16 | assumption | To check | R6.1 | Creusot's test suite and CreuSAT build and generate goals with the pinned Creusot version. |
| AS-17 | assumption | To check | R6.1, R6.3, R7.12, R7.6 | Solvers give the same answer for the same Printed_Goal, seed and options when the time limit is not reached, so that replay and instability measurements are meaningful. |
| AS-18 | assumption | To check | R7.4 | Invalidity witnesses for Canary_Goals, and the models of Injected_False_Lemmas, can be produced mechanically and confirmed by an evaluator independent of the producing solver (R7.4 criterion 2), in two forms: (a) for a Canary_Goal that has  |
| AS-19 | assumption | To check | R2.3, R7.2 | `proof.json` can be extended with new fields such that unmodified pinned why3find reads every entry with a non-proof label (R7.2) as not proved, and reads an entry with a conflict field (R7.2 criterion 7) or an Invalid field (R7.2 criterion |
| AS-20 | assumption | To check | R1.4, R7.6 | why3find's Calibration scaling of a reference time limit into a local time limit is a deterministic function of the Calibration value and the reference time limit, so that the Front_End can recompute the local time limit exactly (R1.4). |
| AS-21 | assumption | To check | R1.4, R1.5 | A Worker, which starts every solver process itself in its own process group (design decision D-16), can terminate a running solver process together with every process that solver process started, on cancellation (R1.5) and on expiry of the  |
| AS-22 | assumption | To check | R1.2 | why3find's first sequential pass applies the Local_Fast_Pass time budget to each solver process separately, and the budget can be set per invocation (R1.2). |
| AS-23 | assumption | To check | R3.2, R5.2, R5.3 | Ethos and Carcara can check a sub-proof whose assumptions are discharged into an implication (CPC `SCOPE`, Alethe subproofs), so that a Composed_Certificate can show that each imported Lemma follows from the open assumptions that R5.2 crite |
| AS-24 | assumption | To check | R3.2, R4.1 | The Canonicalizer can identify the Negated_Goal_Assertion in a Printed_Goal from the Driver output (for example, because the Driver prints the negated goal as the last assertion in a fixed form), so that a Provenance_Record can name it (R3. |
| AS-25 | assumption | To check | R4.1 | For Sibling_Goals printed with the same Driver and Transformation_Pipeline, Why3 prints the same symbol name for symbols that come from the same Why3 declaration, so that identical declarations (R4.1) are recognized across goals. |
| AS-26 | assumption | To check | R4.1 | The Driver output, or the Why3 task before printing, shows which Hypotheses are axioms of which Why3 declaration, so that the Canonicalizer can attribute axioms to declared symbols deterministically (R4.1, OQ-N45). |
| AS-27 | assumption | To check | R4.1, R4.3 | Every symbol that the pinned Z3 and cvc5 introduce during a run (Skolem symbols, fresh constants, purification and other internal symbols) and print in exported Lemmas, instantiation reports, models or proofs matches one of a finite list of |
| AS-28 | assumption | To check | R4.4 | A Sharing_Solver can map each reported quantifier instantiation back to a Hypothesis of the input Printed_Goal and to one term for each variable of that Hypothesis's top-level binder, despite the solver's preprocessing of quantified formula |
| AS-29 | assumption | To check | R4.2 | The why3find goal path of a Goal identifies the Rust function, and the Rust module, whose verification condition produced the Goal (R4.2 criterion 1, OQ-N13). |
| AS-30 | assumption | To check | R3.2, R3.3, R3.4, R5.3, R7.1, R7.6 | The pinned Ethos and Carcara can check a proof against an SMT-LIB input problem supplied as a separate file, and accept only proofs whose assumptions are assertions of that file after parsing, or can report the assumptions a checked proof u |
| AS-31 | assumption | To check | R7.2 | why3find classifies every Why3 prover answer other than Valid and Invalid as one of failed, unknown and timeout, so that the Verdict_Recorder can reuse that classification (R7.2 criterion 6). |
| AS-32 | assumption | To check | R7.13 | cargo-mutants reports each generated mutant with its function name and a mutation text that does not depend on line numbers, can restrict runs to named functions, and can apply one reported mutant so that another tool (a Creusot proof, a Ka |
| AS-33 | assumption | To check | R1.1 | Why3 or why3find can compute a deterministic hash of a Goal's Why3 task before the Transformation_Pipeline and Driver of a Solver_Configuration are applied (for example, Why3's task checksum), equal on every machine for the same pinned inpu |
| AS-34 | assumption | To check | R5.1, R5.2 | In every proof that the pinned cvc5 produces, and that a Sharing_Solver patch produces for a Job with mid-run imports, each imported Lemma and each instance of an imported Instantiation_Hint appears as an `assume` step whose formula matches |
| AS-35 | assumption | To check | R7.5 | Unmodified pinned why3find reads cached prover results only from its own cache layout inside `.why3find`, so that a location exists under the project directory (for example, a directory next to `.why3find`) that unmodified pinned why3find n |
| AS-36 | assumption | To check | R5.1, R5.2 | The pinned cvc5, run with the proof options in the project configuration (global DAG definitions and trust steps off), prints CPC and Alethe proofs whose commands are within the supported fragment of R5.2 (R5.1 criterion 11). |
| AS-37 | assumption | To check | R1.3, R7.12, R7.9 | why3find can get a small upstreamable dispatch mode (`why3find prove --dispatch`, design decision D-3) in which every prover call and cache access of `why3find prove` goes to a parent process, while why3find keeps generating Goals, running  |
| AS-38 | assumption | To check | R5.3 | The Printed_Goals that certifying Drivers produce contain no `define-fun-rec`, `define-funs-rec`, `push` or `pop`, which Ethos does not accept in a reference file (R5.3 criterion 1, AS-30). |
| AS-39 | assumption | To check | R1.4, R6.2 | why3find's Calibration of a prover applies when the Worker or the Front_End starts that prover's binary directly with Why3's command line (design decision D-16). |
| AS-40 | assumption | To check | R3.3, R4.4 | Sharing_Solver patches can print exported Lemmas and Instantiation_Hint terms without `let` (R3.3 criterion 5, R4.4 criterion 10; design decision D-14). |
| NG-1 | non-goal |  | R5.5, R7.1 | Verifying Creusot's translation from MIR to Coma. |
| NG-2 | non-goal |  | R5.5, R7.1 | Verifying Why3's VC generation, transformations (such as `split_vc`), and Driver printing, including the printing and task-count path of the why3find dispatch patch (TB-23). |
| NG-3 | non-goal |  | R1.2, R7.1, R7.6 | Verifying the SMT solvers. |
| NG-4 | non-goal |  | R5.5, R7.1 | Verifying Ethos, Carcara, the Lean 4 kernel, the TLA+ tools, Kani, Creusot-as-verifier and the Rust compiler. |
| NG-5 | non-goal |  |  | Certificates for proofs by Alt-Ergo or Z3, or any solver without a Checkable_Proof_Format. |
| NG-6 | non-goal |  |  | Sharing quantified Lemmas other than Instantiation_Hints. |
| NG-7 | non-goal |  | R1.1, R1.3, R1.7, R3.1, R7.1 | Byzantine fault tolerance for proved-standalone Results from authenticated Workers. |
| NG-8 | non-goal |  | R1.6, R3.1 | Liveness under a faulty Coordinator. |
| NG-9 | non-goal |  |  | Multi-tenant fair-share scheduling, autoscaling, and a graphical interface. |
| NG-10 | non-goal |  |  | Why3 front ends other than Creusot. |
| NG-11 | non-goal |  | R6.1, R6.2, R6.3 | Performance guarantees on arbitrary hardware. |
| TB-1 | trusted |  | R1.3, R3.4, R4.1, R5.4, R7.1, R7.5 | Collision resistance of the hash function used for Task_Hashes, Goal_Hashes, Lemma and Instantiation_Hint identifiers, import-set hashes, Cache_Keys and Certificate_Store content hashes. |
| TB-2 | trusted |  | R1.3, R3.1, R3.2, R7.1 | Unforgeability of the authentication scheme for Results, Lemmas and Instantiation_Hints (Ed25519 signatures with one key pair per Worker, design decision D-9), and secrecy of Worker credentials (R7.1 criterion 7, OQ-N11). |
| TB-3 | trusted |  | R3.2, R3.3, R3.4, R3.5, R5.2, R5.3, R5.5, R7.1, R7.10, R7.4, R7.6 | Ethos, the CPC Certificate_Checker, at its Lock_File version and binary hash, including the CPC semantics it implements. |
| TB-4 | trusted |  | R3.2, R3.3, R3.4, R3.5, R5.2, R5.3, R5.5, R7.1, R7.10, R7.6 | Carcara, the Alethe Certificate_Checker, at its Lock_File version and binary hash, including the Alethe semantics it implements. |
| TB-5 | trusted |  | R3.4, R7.1 | The Lean 4 kernel, for the A1 metatheory. |
| TB-6 | trusted |  | R7.1, R7.3 | Creusot as a verifier, for the A1 contracts on dp-core and dp-formats. |
| TB-7 | trusted |  | R1.5, R7.3 | Kani, for the A2 bounded checks. |
| TB-8 | trusted |  | R1.5 | TLC and Apalache, for the A2 protocol checks and trace validation. |
| TB-9 | trusted |  | R7.1 | The Rust compiler and toolchain that build the System. |
| TB-10 | trusted |  | R7.1, R7.5 | Integrity of the local Result_Cache directory `.dp-cache/` and of the legacy layout inside `.why3find` for Result_Cache entries with the mark local, as for `.why3find` in why3find today. |
| TB-11 | trusted |  | R3.2, R3.3, R3.4, R5.3, R5.5, R7.1, R7.6 | The checker option lists, the signature files they name, and the checker-output table recorded in the TCB_Document (R5.3 criterion 2, R7.8 criteria 3 and 4). |
| TB-12 | trusted |  | R2.1, R7.1 | The Solver_Configuration option lists passed to Unpatched_Solvers, for proved-standalone (OQ-N37). |
| TB-13 | trusted |  | R1.3, R1.7 | The cluster operator registers only intended Workers and provisions and stores Worker and operator credentials securely (design decision D-9), including the offline operator signing key that signs the versioned Worker registry and Worker re |
| TB-14 | trusted |  | R3.3, R7.10 | The pinned SMT-LIB 2.6 standard: theory symbol tables of the logics in use and the reference term grammar (R3.3, R7.10). |
| TB-15 | trusted |  | R6.4 | The statistics and plotting libraries of the Experiment_Runner, pinned in the Lock_File (R6.4). |
| TB-16 | trusted |  | R7.7 | The bootstrap binaries of the pinning mechanism, which is Nix (R7.7; OQ-N10 resolved by design decision D-8). |
| TB-17 | trusted |  | R7.7 | Integrity of the local file system between a binary hash check and the start of the executable (R7.7). |
| TB-18 | trusted |  | R7.11 | The CI system reports job outcomes, and the commit on which each run ran, correctly (R7.11). |
| TB-19 | trusted |  | R7.13 | cargo-mutants generates representative mutants (R7.13). |
| TB-20 | trusted |  | R6.3 | Hardware isolation during evaluation runs, recorded in the run manifest but not checked mechanically (R6.3). |
| TB-21 | trusted |  | R6.2 | Goals are exchangeable samples of the corpus, as the bootstrap and Wilson intervals assume (R6.2, OQ-N52). |
| TB-22 | trusted |  | R7.1, R7.7 | The Unpatched_Solver marks of the Lock_File, for proved-standalone, for the upstream release artifacts that R7.7 criterion 9 accepts by published checksum (OQ-N10 resolved by design decision D-8; an entry accepted by two rebuilds of the ups |
| TB-23 | trusted |  | R1.3 | The why3find dispatch patch returns exactly the bytes that the pinned Driver prints and the number of tasks that the pinned Transformation_Pipeline yields (R1.3 criteria 3 and 10, design decision D-3). |

## Information order of the non-proof outcomes

standalone-invalid > checked-incomplete > unchecked > dependent-invalid > timeout > unknown > failed > unresolved
