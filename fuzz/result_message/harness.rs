//! The messages of the cluster protocol, the Result message among them
//! (R1.3 criterion 8): everything that arrives from the Coordinator.
//!
//! - The S-expression parser is strict: what it accepts is exactly the
//!   canonical print of the value.
//! - A message that decodes encodes to the value that it was decoded
//!   from, and decodes again to itself: no two byte strings are the same
//!   message.
//! - A Result that the fuzzer made does not verify under a Worker's key.

use dp_formats::{
    cluster::{Job, JobKind, Message, Operation, ResultFields, SignedCommand, SignedResult, WorkerStatus},
    hash::Hash,
    sexp,
};
use ed25519_dalek::SigningKey;

fn key() -> SigningKey {
    SigningKey::from_bytes(&[5; 32])
}

pub fn run(data: &[u8]) {
    let Ok(value) = sexp::parse(data, crate::LIMIT) else { return };
    assert_eq!(sexp::print(&value), data, "the parser accepted a print that is not canonical");
    let Ok(message) = Message::decode(&value) else { return };
    let encoded = message.encode();
    assert_eq!(encoded, value, "two encodings of one message");
    assert_eq!(Message::decode(&encoded).as_ref(), Ok(&message));
    if let Message::Result(signed) = &message {
        // The seeds are signed with this key; any change of a signed field
        // must end that.
        let seed = seeds().iter().any(|s| s == data);
        assert!(seed || !signed.verifies(&key().verifying_key()), "a changed Result verifies");
    }
}

fn result() -> SignedResult {
    let fields = ResultFields {
        job_id: "s1/7/0".into(),
        task_hash: Hash([1; 32]),
        prover: "z3@4.13".into(),
        seed: Some(3),
        binary_hash: Hash([2; 32]),
        worker: "w1".into(),
        status: Some(0),
        timed_out: false,
        output: b"unsat\n".to_vec(),
        elapsed_micros: 1234,
        velocity: 1_000_000,
        local_micros: 5_000_000,
        imports_hash: Hash([3; 32]),
    };
    SignedResult::sign(fields, &key())
}

pub fn seeds() -> Vec<Vec<u8>> {
    let job = Job {
        kind: JobKind::Prove,
        job_id: "s1/7/0".into(),
        prover: "z3@4.13".into(),
        driver: b"driver".to_vec(),
        extension: ".smt2".into(),
        micros: 5_000_000,
        seed: None,
        goal: b"(assert false)\n(check-sat)\n".to_vec(),
    };
    let command = SignedCommand::sign(4, Operation::Revoke, "w1", &key());
    let messages = vec![
        Message::Result(result()),
        Message::FrontEndHello { session: "s1".into() },
        Message::WorkerHello { worker: "w1".into(), offers: vec![("z3@4.13".into(), 1_000_000)] },
        Message::Submit(job.clone()),
        Message::Assign { job: Job { kind: JobKind::Certify, seed: Some(1), ..job }, local_micros: 9, velocity: 2 },
        Message::Heartbeat,
        Message::Idle,
        Message::Cancel { job_id: "s1/7/0".into() },
        Message::Failed { job_id: "s1/7/0".into(), reason: "not run".into() },
        Message::StatusRequest,
        Message::Status {
            incarnation: 2,
            workers: vec![WorkerStatus { worker: "w1".into(), job: Some("j".into()), draining: true, provers: vec!["z3@".into()] }],
            queued: 1,
            front_ends: 1,
        },
        Message::Welcome { incarnation: 2 },
        Message::Operator(command.clone()),
        Message::OperatorResult { serial: 4, taken: true },
        Message::Revocation(command),
    ];
    messages.iter().map(|m| sexp::print(&m.encode())).collect()
}
