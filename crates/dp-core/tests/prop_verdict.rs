// Feature: creusot-distributed-proving, Property 1: Proof labels come only from the two routes
// Feature: creusot-distributed-proving, Property 3: The label of a Result is decided by its facts alone
// P-ID: P7.1.1, P7.1.3, P7.1.5, P7.1.6, P5.3.1, P7.2.3, P7.2.4, P1.5.1, P7.2.8

//! Tests of the Verdict_Recorder's decision functions (`verdict.rs`,
//! `labels.rs`). Where the inputs are few, every combination is tried; the
//! contracts that Creusot proves are stated here again as executable
//! checks, so that a change of a function and of its contract together is
//! still caught.

use dp_core::{
    labels::{classify, most_informative, Outcome, ProverAnswer},
    verdict::{
        cache_label, conflict, decode_label, final_label, label, label_from_checker_output, record_verdict,
        CheckFacts, Context, Hash, Label, ResultFields, UnpatchedSolver, Verdicts,
    },
};
use proptest::prelude::*;

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

const OUTCOMES: [Outcome; 8] = [
    Outcome::StandaloneInvalid,
    Outcome::CheckedIncomplete,
    Outcome::Unchecked,
    Outcome::DependentInvalid,
    Outcome::Timeout,
    Outcome::Unknown,
    Outcome::Failed,
    Outcome::Unresolved,
];

fn h(n: u64) -> Hash {
    Hash { w0: n, w1: 0, w2: 0, w3: 0 }
}

/// A Result and its context from nine conditions, each true when it is as
/// R7.1 criterion 2 needs it for proved-standalone.
fn case(answer: ProverAnswer, bits: u32) -> (ResultFields, Context) {
    let bit = |i: u32| bits & (1 << i) != 0;
    let fields = ResultFields {
        answer,
        task_hash: h(if bit(0) { 7 } else { 8 }),
        partition: !bit(1),
        imports: u64::from(!bit(2)),
        imports_hash: h(if bit(3) { 0 } else { 5 }),
        prover: 1,
        version: if bit(4) { 1 } else { 2 },
        binary_hash: h(if bit(5) { 1 } else { 9 }),
    };
    let ctx = Context {
        certify_all: !bit(6),
        import_mark: !bit(7),
        local_task_hash: h(7),
        task_count: 1,
        empty_imports_hash: h(0),
        local_binary_mismatch: !bit(8),
        unpatched: vec![UnpatchedSolver { prover: 1, version: 1, binary_hash: h(1) }],
    };
    (fields, ctx)
}

#[test]
fn proved_standalone_needs_every_condition_and_a_proof() {
    let all = (1 << 9) - 1;
    for answer in ANSWERS {
        for bits in 0..=all {
            let (fields, ctx) = case(answer, bits);
            let got = label(fields, &ctx);
            let standalone = answer == ProverAnswer::Valid && bits == all;
            assert_eq!(got == Label::ProvedStandalone, standalone, "{answer:?} {bits:09b}");
            // This function never gives certified.
            assert_ne!(got, Label::Certified);
            // A reported proof that is not standalone needs certification.
            if answer == ProverAnswer::Valid && !standalone {
                assert_eq!(got, Label::NonProof(Outcome::Unchecked), "{bits:09b}");
            }
            // A stored entry is labelled as the same Result would be now.
            assert_eq!(cache_label(fields, &ctx), got);
        }
    }
    // More than one task, or none: no print to be bound to.
    for task_count in [0, 2] {
        let (fields, mut ctx) = case(ProverAnswer::Valid, all);
        ctx.task_count = task_count;
        assert_eq!(label(fields, &ctx), Label::NonProof(Outcome::Unchecked));
    }
    // A solver that the Lock_File does not pin at all.
    let (fields, mut ctx) = case(ProverAnswer::Valid, all);
    ctx.unpatched.clear();
    assert_eq!(label(fields, &ctx), Label::NonProof(Outcome::Unchecked));
}

#[test]
fn an_answer_that_is_no_proof_gets_its_classification() {
    for answer in ANSWERS {
        for dependent in [false, true] {
            let expected = match answer {
                ProverAnswer::Valid => None,
                ProverAnswer::Invalid if dependent => Some(Outcome::DependentInvalid),
                ProverAnswer::Invalid => Some(Outcome::StandaloneInvalid),
                ProverAnswer::Timeout => Some(Outcome::Timeout),
                ProverAnswer::OutOfMemory | ProverAnswer::StepLimitExceeded | ProverAnswer::Unknown => {
                    Some(Outcome::Unknown)
                }
                ProverAnswer::Failure | ProverAnswer::HighFailure => Some(Outcome::Failed),
            };
            assert_eq!(classify(answer, dependent), expected);
        }
    }
    // Through `label`: an Invalid answer of an Unpatched_Solver is
    // standalone, any other Invalid answer is dependent.
    let all = (1 << 9) - 1;
    let (fields, ctx) = case(ProverAnswer::Invalid, all);
    assert_eq!(label(fields, &ctx), Label::NonProof(Outcome::StandaloneInvalid));
    let (fields, ctx) = case(ProverAnswer::Invalid, all & !(1 << 5));
    assert_eq!(label(fields, &ctx), Label::NonProof(Outcome::DependentInvalid));
}

#[test]
fn certified_needs_every_fact_of_the_check() {
    for bits in 0u32..(1 << 7) {
        let bit = |i: u32| bits & (1 << i) != 0;
        let facts = CheckFacts {
            produced: bit(0),
            checker_pinned: bit(1),
            signature_pinned: bit(2),
            bound_to_local_print: bit(3),
            accepted: bit(4),
            complete: bit(5),
            stored: bit(6),
        };
        let got = label_from_checker_output(facts);
        assert_eq!(got == Label::Certified, bits == (1 << 7) - 1, "{bits:07b}");
        assert_ne!(got, Label::ProvedStandalone);
        let checked = bit(0) && bit(1) && bit(2) && bit(3) && bit(4);
        if got != Label::Certified {
            let expected = if checked { Outcome::CheckedIncomplete } else { Outcome::Unchecked };
            assert_eq!(got, Label::NonProof(expected), "{bits:07b}");
        }
    }
}

#[test]
fn the_information_order_is_total_and_as_documented() {
    for (i, a) in OUTCOMES.iter().enumerate() {
        assert_eq!(a.rank_u8() as usize, i, "{a:?}");
    }
}

#[test]
fn label_names_other_than_the_two_proof_labels_are_no_proof() {
    let (proved, certified) = (b"proved-standalone".as_slice(), b"certified".as_slice());
    assert_eq!(decode_label(proved, proved, certified), Label::ProvedStandalone);
    assert_eq!(decode_label(certified, proved, certified), Label::Certified);
    for name in [&b""[..], b"proved", b"proved-standalone ", b"Certified", b"certifiedx", b"proved-with-lemmas", b"unknown"] {
        assert_eq!(decode_label(name, proved, certified), Label::NonProof(Outcome::Unresolved), "{name:?}");
    }
}

#[test]
fn a_conflict_is_a_proof_label_with_an_invalid_answer() {
    for invalid in [0, 1, 5] {
        assert_eq!(conflict(Label::ProvedStandalone, invalid), invalid > 0);
        assert_eq!(conflict(Label::Certified, invalid), invalid > 0);
        for o in OUTCOMES {
            assert!(!conflict(Label::NonProof(o), invalid));
        }
    }
}

fn outcome() -> impl Strategy<Value = Outcome> {
    (0usize..8).prop_map(|i| OUTCOMES[i])
}

fn any_label() -> impl Strategy<Value = Label> {
    prop_oneof![Just(Label::ProvedStandalone), Just(Label::Certified), outcome().prop_map(Label::NonProof)]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, failure_persistence: None, ..ProptestConfig::default() })]

    /// R7.2 criterion 5: the most informative outcome is the one of the
    /// smallest rank, and unresolved when there is none.
    #[test]
    fn the_most_informative_outcome_is_the_minimum(outcomes in prop::collection::vec(outcome(), 0..12)) {
        let best = most_informative(&outcomes);
        match outcomes.iter().map(|o| o.rank_u8()).min() {
            None => prop_assert_eq!(best, Outcome::Unresolved),
            Some(least) => {
                prop_assert_eq!(best.rank_u8(), least);
                prop_assert!(outcomes.contains(&best));
            }
        }
        // A recorded Verdict wins; without one the label is no proof.
        prop_assert_eq!(final_label(Some(Label::Certified), &outcomes), Label::Certified);
        prop_assert_eq!(final_label(None, &outcomes), Label::NonProof(best));
    }

    /// R1.5 criterion 1: one Verdict per Goal, and the first one stays.
    #[test]
    fn a_verdict_is_final(records in prop::collection::vec((0u64..6, any_label()), 0..30)) {
        let mut verdicts = Verdicts { entries: Vec::new() };
        let mut first: std::collections::BTreeMap<u64, Label> = Default::default();
        for (goal, label) in records {
            verdicts = record_verdict(verdicts, goal, label);
            first.entry(goal).or_insert(label);
            // increasing by Goal: at most one Verdict per Goal
            prop_assert!(verdicts.entries.windows(2).all(|w| w[0].goal < w[1].goal));
            let recorded: std::collections::BTreeMap<u64, Label> =
                verdicts.entries.iter().map(|v| (v.goal, v.label)).collect();
            prop_assert_eq!(&recorded, &first);
        }
    }
}
