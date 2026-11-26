// Feature: creusot-distributed-proving, Property 1: Proof labels come only from the two routes
// P-ID: P7.1.1, P7.1.5, P5.3.1 (the Rust decision functions are the Lean reference definitions)

//! The decision functions of dp-core against the executable Lean
//! references of `proofs/lean/DP/Trace.lean` (R7.1 criterion 5), on every
//! input: `label` (8 answers, 9 facts), `label_from_checker_output` (7
//! facts), `accept_remote` (8 facts) and `classify`.
//!
//! `verdict_sound` is proved about the Lean definitions. This test is what
//! ties the Rust functions that run to them.
//!
//! Needs `DP_LEAN_REF`, the path of the `dp-lean-ref` executable
//! (`ci/jobs/lean.sh` builds it). Without it the test passes with a
//! notice; with `DP_REQUIRE_LEAN` it fails.

use std::{
    io::Write,
    process::{Command, Stdio},
};

use dp_core::{
    labels::{classify, Outcome, ProverAnswer},
    verdict::{
        accept_remote, label, label_from_checker_output, Acceptance, CheckFacts, Context, Hash, Label, RemoteFacts,
        ResultFields, UnpatchedSolver,
    },
};

const ANSWERS: [ProverAnswer; 8] = [
    ProverAnswer::Valid,
    ProverAnswer::Invalid,
    ProverAnswer::Timeout,
    ProverAnswer::OutOfMemory,
    ProverAnswer::StepLimitExceeded,
    ProverAnswer::Unknown,
    ProverAnswer::Failure,
    ProverAnswer::HighFailure,
];

fn h(n: u64) -> Hash {
    Hash { w0: n, w1: 0, w2: 0, w3: 0 }
}

fn outcome_name(o: Outcome) -> &'static str {
    match o {
        Outcome::StandaloneInvalid => "standalone-invalid",
        Outcome::CheckedIncomplete => "checked-incomplete",
        Outcome::Unchecked => "unchecked",
        Outcome::DependentInvalid => "dependent-invalid",
        Outcome::Timeout => "timeout",
        Outcome::Unknown => "unknown",
        Outcome::Failed => "failed",
        Outcome::Unresolved => "unresolved",
    }
}

fn label_name(l: Label) -> &'static str {
    match l {
        Label::ProvedStandalone => "proved-standalone",
        Label::Certified => "certified",
        Label::NonProof(o) => outcome_name(o),
    }
}

fn bits(n: u32, count: u32) -> String {
    (0..count).map(|i| if n & (1 << i) != 0 { "1" } else { "0" }).collect::<Vec<_>>().join(" ")
}

/// A Result and its context for the nine facts of the Lean `ResultFacts`,
/// in its order: taskHashMatches, oneTask, partition, importMark,
/// importsEmpty, importsHashEmpty, unpatched, binaryMismatch, certifyAll.
fn result(answer: ProverAnswer, n: u32) -> (ResultFields, Context) {
    let bit = |i: u32| n & (1 << i) != 0;
    let fields = ResultFields {
        answer,
        task_hash: h(if bit(0) { 7 } else { 8 }),
        partition: bit(2),
        imports: u64::from(!bit(4)),
        imports_hash: h(if bit(5) { 0 } else { 5 }),
        prover: 1,
        version: 1,
        binary_hash: h(if bit(6) { 1 } else { 9 }),
    };
    let ctx = Context {
        certify_all: bit(8),
        import_mark: bit(3),
        local_task_hash: h(7),
        task_count: if bit(1) { 1 } else { 2 },
        empty_imports_hash: h(0),
        local_binary_mismatch: bit(7),
        unpatched: vec![UnpatchedSolver { prover: 1, version: 1, binary_hash: h(1) }],
    };
    (fields, ctx)
}

#[test]
fn the_rust_functions_are_the_lean_references() {
    let Some(reference) = std::env::var_os("DP_LEAN_REF") else {
        assert!(std::env::var_os("DP_REQUIRE_LEAN").is_none(), "DP_LEAN_REF is not set");
        eprintln!("DP_LEAN_REF is not set: the comparison with the Lean references is skipped");
        return;
    };
    // Every request, with what the Rust function answers.
    let mut requests: Vec<(String, String)> = Vec::new();
    for (a, answer) in ANSWERS.iter().enumerate() {
        for n in 0..(1 << 9) {
            let (fields, ctx) = result(*answer, n);
            requests.push((format!("label {a} {}", bits(n, 9)), label_name(label(fields, &ctx)).to_string()));
        }
        for dependent in [false, true] {
            let rust = classify(*answer, dependent).map_or("proof", outcome_name);
            requests.push((format!("classify {a} {}", u8::from(dependent)), rust.to_string()));
        }
    }
    for n in 0..(1 << 7) {
        let bit = |i: u32| n & (1 << i) != 0;
        let facts = CheckFacts {
            produced: bit(0),
            checker_pinned: bit(1),
            signature_pinned: bit(2),
            bound_to_local_print: bit(3),
            accepted: bit(4),
            complete: bit(5),
            stored: bit(6),
        };
        requests.push((format!("checker {}", bits(n, 7)), label_name(label_from_checker_output(facts)).to_string()));
    }
    for n in 0..(1 << 8) {
        let bit = |i: u32| n & (1 << i) != 0;
        let facts = RemoteFacts {
            signature_valid: bit(0),
            registered: bit(1),
            revoked: bit(2),
            submitted: bit(3),
            limits_consistent: bit(4),
            imports_consistent: bit(5),
        };
        // Facts 7 and 8: the Task_Hash matches, and the print is one task.
        let (fields, ctx) = result(ProverAnswer::Valid, u32::from(bit(6)) | u32::from(bit(7)) << 1 | 0b0_0111_0000);
        let rust = if accept_remote(fields, &ctx, facts) == Acceptance::Accepted { "accept" } else { "discard" };
        requests.push((format!("accept {}", bits(n, 8)), rust.to_string()));
    }
    assert_eq!(requests.len(), 8 * 512 + 16 + 128 + 256);

    let mut child = Command::new(&reference).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("dp-lean-ref starts");
    let input: String = requests.iter().map(|(request, _)| format!("{request}\n")).collect();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(out.status.success());
    let answers: Vec<&str> = std::str::from_utf8(&out.stdout).unwrap().lines().collect();
    assert_eq!(answers.len(), requests.len(), "one answer per request");
    let mut proofs = 0;
    for ((request, rust), lean) in requests.iter().zip(&answers) {
        assert_ne!(*lean, "error", "the reference does not read {request:?}");
        assert_eq!(rust, lean, "{request}: Rust {rust}, Lean {lean}");
        proofs += usize::from(*lean == "proved-standalone" || *lean == "certified");
    }
    // Exactly one input of each route gives a proof label.
    assert_eq!(proofs, 2);
}
