//! The `dispatch` format: what why3find in dispatch mode and its parent
//! send each other over a pipe.
//!
//! A message that decodes, in either direction, encodes to the value that
//! it was decoded from and decodes again to itself: no two byte strings
//! are one message.

use dp_formats::{
    dispatch::{DriverAnswer, FromWhy3find, PathStep, ProverInfo, ProverResult, StrategyStep, ToWhy3find},
    sexp,
};

pub fn run(data: &[u8]) {
    let Ok(value) = sexp::parse(data, crate::LIMIT) else { return };
    assert_eq!(sexp::print(&value), data, "the parser accepted a print that is not canonical");
    if let Ok(message) = FromWhy3find::decode(&value) {
        assert_eq!(message.encode(), value, "two encodings of one message from why3find");
        assert_eq!(FromWhy3find::decode(&message.encode()).as_ref(), Ok(&message));
    }
    if let Ok(message) = ToWhy3find::decode(&value) {
        assert_eq!(message.encode(), value, "two encodings of one message to why3find");
        assert_eq!(ToWhy3find::decode(&message.encode()).as_ref(), Ok(&message));
    }
}

pub fn seeds() -> Vec<Vec<u8>> {
    let s = |text: &str| text.to_string();
    let from = [
        FromWhy3find::Hello { version: 1 },
        FromWhy3find::Goal {
            gid: s("g1"),
            file: s("a.mlw"),
            theory: s("T"),
            name: s("G"),
            path: vec![PathStep { tactic: s("split_vc"), index: 2 }],
            task: b"(task)".to_vec(),
        },
        FromWhy3find::CacheGet { rid: s("r1"), gid: s("g1"), prover: s("z3@4"), micros: 1_000_000 },
        FromWhy3find::Prove { rid: s("r1"), gid: s("g1"), step: StrategyStep::ParallelLong, prover: s("z3@4"), micros: 5 },
        FromWhy3find::Cancel { rid: s("r1") },
        FromWhy3find::CacheSet { rid: s("r1"), gid: s("g1"), prover: s("z3@4"), result: ProverResult::Valid { micros: 7 } },
        FromWhy3find::Done,
        FromWhy3find::Printed { qid: s("q1"), tasks: 1, bytes: b"(assert false)\n".to_vec() },
        FromWhy3find::Task { qid: s("q2"), task: vec![0, 255] },
        FromWhy3find::Provers {
            qid: s("q3"),
            provers: vec![ProverInfo { prover: s("z3@4"), digest: s("d"), driver: s("z3"), command: s("z3 %f"), extension: s(".smt2") }],
        },
        FromWhy3find::Calibration { qid: s("q4"), millionths: 1_250_000 },
        FromWhy3find::AnswerRead { qid: s("q5"), answer: DriverAnswer::HighFailure },
        FromWhy3find::Error { qid: s("q6"), message: s("no such goal") },
    ];
    let to = [
        ToWhy3find::Cache { rid: s("r1"), result: ProverResult::NoResult },
        ToWhy3find::Answer { rid: s("r1"), result: ProverResult::Timeout { micros: 9 } },
        ToWhy3find::Stored { rid: s("r1"), store: true },
        ToWhy3find::Print { qid: s("q1"), gid: s("g1"), prover: s("cvc5@1"), transformations: vec![s("eliminate_match")] },
        ToWhy3find::TaskPrint { qid: s("q2"), gid: s("g1") },
        ToWhy3find::Provers { qid: s("q3") },
        ToWhy3find::Calibration { qid: s("q4"), prover: s("z3@4") },
        ToWhy3find::ReadAnswer { qid: s("q5"), prover: s("z3@4"), status: 0, output: b"unsat\n".to_vec() },
    ];
    from.iter().map(|m| sexp::print(&m.encode())).chain(to.iter().map(|m| sexp::print(&m.encode()))).collect()
}
