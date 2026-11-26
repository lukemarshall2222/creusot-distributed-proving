//! `arb_coordinator_ops`: inputs of the Coordinator machine (Properties 4
//! and 8): Workers and Front_Ends that join, submissions, Results,
//! disconnections, and operator commands with and without the operator's
//! signature and with fresh and old serial numbers.

use dp_core::coord::{Input, Offer, Operation};
use proptest::prelude::*;

/// The peer that sends operator commands.
pub const OPERATOR: u64 = 99;

/// Front_Ends are the peers 1 and 2, Workers the peers 10 to 13.
pub fn input() -> impl Strategy<Value = Input> {
    let worker = 10u64..14;
    let front_end = 1u64..3;
    let now = Just(5u64);
    prop_oneof![
        2 => (now.clone(), front_end.clone()).prop_map(|(now, peer)| Input::FrontEndHello { now, peer }),
        4 => (now.clone(), worker.clone(), prop::bool::weighted(0.15)).prop_map(|(now, peer, revoked)| {
            Input::WorkerHello { now, peer, offers: vec![Offer { prover: 1, velocity: 1_000_000 }], revoked }
        }),
        6 => (now.clone(), front_end.clone(), 0u64..40).prop_map(|(now, peer, job)| Input::Submit {
            now, peer, job, prover: 1, micros: 1_000,
        }),
        3 => (now.clone(), worker.clone(), 0u64..40).prop_map(|(now, peer, job)| Input::Result { now, peer, job }),
        2 => (now.clone(), worker.clone()).prop_map(|(now, peer)| Input::WorkerIdle { now, peer }),
        1 => (now.clone(), worker.clone()).prop_map(|(now, peer)| Input::Disconnected { now, peer }),
        8 => (
            now,
            prop::bool::weighted(0.8),
            0u64..12,
            prop_oneof![Just(Operation::Drain), Just(Operation::Remove), Just(Operation::Revoke)],
            prop::option::weighted(0.85, worker),
        )
            .prop_map(|(now, authentic, serial, operation, worker)| Input::Operator {
                now, peer: OPERATOR, authentic, serial, operation, worker,
            }),
    ]
}
