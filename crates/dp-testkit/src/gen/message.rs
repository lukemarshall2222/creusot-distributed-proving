//! Messages of the dp protocol (Properties 3 and 34): every kind, with
//! Jobs of both kinds, signed Results, operator commands and the status
//! report.

use dp_formats::cluster::*;
use proptest::prelude::*;

use super::result::{blob, fields, key, text as s};

pub fn job() -> impl Strategy<Value = Job> {
    (any::<bool>(), s(), s(), blob(), s(), any::<u64>(), any::<Option<u64>>(), blob()).prop_map(
        |(certify, job_id, prover, driver, extension, micros, seed, goal)| Job {
            kind: if certify { JobKind::Certify } else { JobKind::Prove },
            job_id,
            prover,
            driver,
            extension,
            micros,
            seed,
            goal,
        },
    )
}

pub fn command() -> impl Strategy<Value = SignedCommand> {
    (any::<u64>(), 0usize..3, s(), key()).prop_map(|(serial, op, worker, key)| {
        SignedCommand::sign(serial, [Operation::Drain, Operation::Remove, Operation::Revoke][op], &worker, &key)
    })
}

pub fn message() -> impl Strategy<Value = Message> {
    prop_oneof![
        s().prop_map(|session| Message::FrontEndHello { session }),
        (s(), proptest::collection::vec((s(), any::<u64>()), 0..4))
            .prop_map(|(worker, offers)| Message::WorkerHello { worker, offers }),
        Just(Message::Heartbeat),
        Just(Message::Idle),
        Just(Message::StatusRequest),
        any::<u64>().prop_map(|incarnation| Message::Welcome { incarnation }),
        (proptest::collection::vec((s(), proptest::option::of(s()), any::<bool>(), proptest::collection::vec(s(), 0..3)), 0..3), any::<u64>(), any::<u64>())
            .prop_map(|(workers, queued, front_ends)| Message::Status {
                incarnation: queued / 3,
                workers: workers
                    .into_iter()
                    .map(|(worker, job, draining, provers)| WorkerStatus { worker, job, draining, provers })
                    .collect(),
                queued,
                front_ends,
            }),
        (command(), any::<bool>()).prop_map(|(c, revocation)| if revocation { Message::Revocation(c) } else { Message::Operator(c) }),
        (any::<u64>(), any::<bool>()).prop_map(|(serial, taken)| Message::OperatorResult { serial, taken }),
        job().prop_map(Message::Submit),
        (job(), any::<u64>(), any::<u64>())
            .prop_map(|(job, local_micros, velocity)| Message::Assign { job, local_micros, velocity }),
        s().prop_map(|job_id| Message::Cancel { job_id }),
        (fields(), key()).prop_map(|(f, k)| Message::Result(SignedResult::sign(f, &k))),
        (s(), s()).prop_map(|(job_id, reason)| Message::Failed { job_id, reason }),
    ]
}
