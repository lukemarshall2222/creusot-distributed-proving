// Feature: creusot-distributed-proving, Property 34: Exchanged formats parse strictly and print canonically
// P-ID: P7.10.1 (dispatch format)

use dp_formats::dispatch::*;
use dp_formats::sexp::{parse, print, Sexp};
use proptest::prelude::*;

fn s() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9@._ -]{0,12}"
}

fn result() -> impl Strategy<Value = ProverResult> {
    prop_oneof![
        Just(ProverResult::NoResult),
        Just(ProverResult::Failed),
        any::<u64>().prop_map(|micros| ProverResult::Valid { micros }),
        any::<u64>().prop_map(|micros| ProverResult::Unknown { micros }),
        any::<u64>().prop_map(|micros| ProverResult::Timeout { micros }),
    ]
}

fn step() -> impl Strategy<Value = StrategyStep> {
    prop_oneof![
        Just(StrategyStep::Fast),
        Just(StrategyStep::Parallel),
        Just(StrategyStep::ParallelLong),
        Just(StrategyStep::Replay)
    ]
}

fn blob() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..40)
}

fn from_why3find() -> impl Strategy<Value = FromWhy3find> {
    use FromWhy3find::*;
    let path = proptest::collection::vec(
        (s(), any::<u32>()).prop_map(|(tactic, index)| PathStep { tactic, index }),
        0..4,
    );
    let provers = proptest::collection::vec(
        (s(), s(), s(), s(), s()).prop_map(|(prover, digest, driver, command, extension)| {
            ProverInfo { prover, digest, driver, command, extension }
        }),
        0..4,
    );
    prop_oneof![
        any::<u32>().prop_map(|version| Hello { version }),
        (s(), s(), s(), s(), path, blob())
            .prop_map(|(gid, file, theory, name, path, task)| Goal { gid, file, theory, name, path, task }),
        (s(), s(), s(), any::<u64>())
            .prop_map(|(rid, gid, prover, micros)| CacheGet { rid, gid, prover, micros }),
        (s(), s(), step(), s(), any::<u64>())
            .prop_map(|(rid, gid, step, prover, micros)| Prove { rid, gid, step, prover, micros }),
        s().prop_map(|rid| Cancel { rid }),
        (s(), s(), s(), result())
            .prop_map(|(rid, gid, prover, result)| CacheSet { rid, gid, prover, result }),
        Just(Done),
        (s(), any::<u32>(), blob()).prop_map(|(qid, tasks, bytes)| Printed { qid, tasks, bytes }),
        (s(), blob()).prop_map(|(qid, task)| Task { qid, task }),
        (s(), provers).prop_map(|(qid, provers)| Provers { qid, provers }),
        (s(), any::<u64>()).prop_map(|(qid, millionths)| Calibration { qid, millionths }),
        (s(), proptest::sample::select(DriverAnswer::ALL.to_vec()))
            .prop_map(|(qid, answer)| AnswerRead { qid, answer }),
        (s(), s()).prop_map(|(qid, message)| Error { qid, message }),
    ]
}

fn to_why3find() -> impl Strategy<Value = ToWhy3find> {
    use ToWhy3find::*;
    prop_oneof![
        (s(), result()).prop_map(|(rid, result)| Cache { rid, result }),
        (s(), result()).prop_map(|(rid, result)| Answer { rid, result }),
        (s(), any::<bool>()).prop_map(|(rid, store)| Stored { rid, store }),
        (s(), s(), s(), proptest::collection::vec(s(), 0..3)).prop_map(
            |(qid, gid, prover, transformations)| Print { qid, gid, prover, transformations }
        ),
        (s(), s()).prop_map(|(qid, gid)| TaskPrint { qid, gid }),
        s().prop_map(|qid| Provers { qid }),
        (s(), s()).prop_map(|(qid, prover)| Calibration { qid, prover }),
        (s(), s(), any::<u32>(), blob())
            .prop_map(|(qid, prover, status, output)| ReadAnswer { qid, prover, status, output }),
    ]
}

proptest! {
    #[test]
    fn messages_from_why3find_round_trip(m in from_why3find()) {
        let bytes = print(&m.encode());
        prop_assert_eq!(FromWhy3find::decode(&parse(&bytes, 1 << 20).unwrap()), Ok(m));
    }

    #[test]
    fn messages_to_why3find_round_trip(m in to_why3find()) {
        let bytes = print(&m.encode());
        prop_assert_eq!(ToWhy3find::decode(&parse(&bytes, 1 << 20).unwrap()), Ok(m));
    }
}

#[test]
fn numbers_must_be_canonical() {
    let msg = |t: &str| {
        Sexp::list([Sexp::atom("calibration"), Sexp::atom("q"), Sexp::atom(t)])
    };
    assert!(FromWhy3find::decode(&msg("1000000")).is_ok());
    assert!(FromWhy3find::decode(&msg("0")).is_ok());
    for bad in ["01", "+1", "-1", "", "1.0", "1e3", " 1", "18446744073709551616"] {
        assert!(FromWhy3find::decode(&msg(bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn unknown_tags_and_wrong_arity_are_rejected() {
    let l = |xs: &[&str]| Sexp::list(xs.iter().map(|x| Sexp::atom(*x)));
    assert!(FromWhy3find::decode(&l(&["done"])).is_ok());
    assert!(FromWhy3find::decode(&l(&["done", "x"])).is_err());
    assert!(FromWhy3find::decode(&l(&["bogus"])).is_err());
    assert!(FromWhy3find::decode(&l(&[])).is_err());
    assert!(FromWhy3find::decode(&Sexp::atom("done")).is_err());
    assert!(ToWhy3find::decode(&l(&["stored", "1", "maybe"])).is_err());
}
