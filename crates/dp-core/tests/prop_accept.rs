// Feature: creusot-distributed-proving, Property 2: Only bound, authentic, submitted Results are accepted
// P-ID: P1.3.1, P1.3.2, P1.3.4, P1.3.5, P1.3.6, P1.3.7, P1.3.10, P1.4.4

//! Tests of `accept_result`, `accept_remote` and `decide`: a Result is
//! accepted only when every fact holds, each fact alone decides, and what
//! is not accepted proves nothing.

use dp_core::{
    frontend::{decide, Decision, Evidence},
    labels::ProverAnswer,
    verdict::{
        accept_remote, accept_result, Acceptance, Context, Hash, Label, RemoteFacts, ResultFields, UnpatchedSolver,
    },
};
use proptest::prelude::*;

fn h(n: u64) -> Hash {
    Hash { w0: n, w1: n.rotate_left(7), w2: !n, w3: 3 }
}

fn fields(task: u64) -> ResultFields {
    ResultFields {
        answer: ProverAnswer::Valid,
        task_hash: h(task),
        partition: false,
        imports: 0,
        imports_hash: h(0),
        prover: 1,
        version: 1,
        binary_hash: h(1),
    }
}

fn context(local_task: u64, task_count: u64) -> Context {
    Context {
        certify_all: false,
        import_mark: false,
        local_task_hash: h(local_task),
        task_count,
        empty_imports_hash: h(0),
        local_binary_mismatch: false,
        unpatched: vec![UnpatchedSolver { prover: 1, version: 1, binary_hash: h(1) }],
    }
}

fn facts(bits: u32) -> RemoteFacts {
    let bit = |i: u32| bits & (1 << i) != 0;
    RemoteFacts {
        signature_valid: bit(0),
        registered: bit(1),
        revoked: !bit(2),
        submitted: bit(3),
        limits_consistent: bit(4),
        imports_consistent: bit(5),
    }
}

const ALL: u32 = (1 << 6) - 1;

#[test]
fn a_remote_result_is_accepted_only_with_every_fact() {
    for bits in 0..=ALL {
        for (task, count) in [(7, 1), (8, 1), (7, 0), (7, 2)] {
            let bound = task == 7 && count == 1;
            let got = accept_remote(fields(task), &context(7, count), facts(bits));
            assert_eq!(got == Acceptance::Accepted, bits == ALL && bound, "{bits:06b} task {task} count {count}");
        }
    }
}

#[test]
fn each_missing_fact_has_its_reason() {
    let reason = |missing: u32| accept_remote(fields(7), &context(7, 1), facts(ALL & !(1 << missing)));
    assert_eq!(reason(0), Acceptance::BadSignature);
    assert_eq!(reason(1), Acceptance::UnknownWorker);
    assert_eq!(reason(2), Acceptance::UnknownWorker); // revoked
    assert_eq!(reason(3), Acceptance::NotSubmitted);
    assert_eq!(reason(4), Acceptance::LimitMismatch);
    assert_eq!(reason(5), Acceptance::WrongImportsHash);
    assert_eq!(accept_remote(fields(8), &context(7, 1), facts(ALL)), Acceptance::WrongTaskHash);
    assert_eq!(accept_remote(fields(7), &context(7, 2), facts(ALL)), Acceptance::NotOneTask);
    assert_eq!(accept_result(fields(8), &context(7, 1)), Acceptance::WrongTaskHash);
    assert_eq!(accept_result(fields(7), &context(7, 0)), Acceptance::NotOneTask);
    assert_eq!(accept_result(fields(7), &context(7, 1)), Acceptance::Accepted);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 500, failure_persistence: None, ..ProptestConfig::default() })]

    /// A Task_Hash that differs from the local one in any word is not
    /// accepted (P1.3.1).
    #[test]
    fn any_difference_of_the_task_hash_is_a_discard(word in 0usize..4, change in 1u64..) {
        let mut r = fields(7);
        match word {
            0 => r.task_hash.w0 ^= change,
            1 => r.task_hash.w1 ^= change,
            2 => r.task_hash.w2 ^= change,
            _ => r.task_hash.w3 ^= change,
        }
        prop_assert_eq!(accept_result(r, &context(7, 1)), Acceptance::WrongTaskHash);
        prop_assert_eq!(accept_remote(r, &context(7, 1), facts(ALL)), Acceptance::WrongTaskHash);
    }

    /// `decide` gives a proof only for an accepted Result that is
    /// standalone; a discard carries the reason of the acceptance, and a
    /// Result that is not accepted is never labelled (P1.3.4, P7.1.1).
    #[test]
    fn nothing_that_is_discarded_proves(
        bits in 0u32..=ALL,
        remote in any::<bool>(),
        task in prop_oneof![3 => Just(7u64), 1 => Just(8u64)],
        count in prop_oneof![4 => Just(1u64), 1 => Just(0u64), 1 => Just(2u64)],
        valid in prop::bool::weighted(0.8),
        pinned in prop::bool::weighted(0.8),
    ) {
        let mut r = fields(task);
        r.answer = if valid { ProverAnswer::Valid } else { ProverAnswer::Unknown };
        if !pinned {
            r.binary_hash = h(9);
        }
        let ctx = context(7, count);
        let evidence = Evidence { fields: r, local_task_hash: ctx.local_task_hash, task_count: count, binary_mismatch: false };
        let remote_facts = remote.then(|| facts(bits));
        let acceptance = match remote_facts {
            Some(f) => accept_remote(r, &ctx, f),
            None => accept_result(r, &ctx),
        };
        match decide(evidence, &ctx, remote_facts) {
            Decision::Discarded(reason) => {
                prop_assert_ne!(acceptance, Acceptance::Accepted);
                prop_assert_eq!(reason, acceptance);
            }
            Decision::Proved => {
                prop_assert_eq!(acceptance, Acceptance::Accepted);
                prop_assert!(valid && pinned && task == 7 && count == 1);
                prop_assert!(!remote || bits == ALL);
                prop_assert_eq!(dp_core::verdict::label(r, &ctx), Label::ProvedStandalone);
            }
            Decision::Lost(_) => {
                prop_assert_eq!(acceptance, Acceptance::Accepted);
                prop_assert!(!(valid && pinned));
            }
        }
    }
}
