//! `arb_session_events`: event sequences of the Front_End machine
//! (Properties 1, 6, 7 and 23): Goals, prover calls of both passes, print
//! facts, local runs and remote Results in any order, with evidence that
//! mostly satisfies the acceptance conditions and breaks each of them some
//! of the time, Jobs that were not run, cancelled calls, the loss of the
//! Cluster, ticks and the end of why3find.
//!
//! The context of the evidence: the pinned solver is prover 1, version 1,
//! with the binary hash `h(1)`; the local print of every Goal has the Task
//! hash `h(7)`; the hash of the empty import set is `h(0)`.

use dp_core::{
    frontend::{Evidence, Input, Pass},
    labels::ProverAnswer,
    verdict::{Hash, ResultFields},
};
use proptest::prelude::*;

/// A hash that stands for the number `n`.
pub fn h(n: u64) -> Hash {
    Hash { w0: n, w1: 0, w2: 0, w3: 0 }
}

/// Evidence that mostly satisfies R7.1 criterion 2, with each condition
/// broken some of the time.
pub fn evidence() -> impl Strategy<Value = Evidence> {
    (
        prop_oneof![
            4 => Just(ProverAnswer::Valid),
            1 => Just(ProverAnswer::Invalid),
            1 => Just(ProverAnswer::Timeout),
            1 => Just(ProverAnswer::Unknown),
            1 => Just(ProverAnswer::HighFailure),
        ],
        prop_oneof![6 => Just(7u64), 1 => Just(8u64)], // task hash; the local print is 7
        prop::bool::weighted(0.1),                     // partition
        prop_oneof![8 => Just(0u64), 1 => Just(1u64)], // imports
        prop_oneof![8 => Just(0u64), 1 => Just(5u64)], // imports hash
        prop_oneof![8 => Just(1u64), 1 => Just(2u64)], // prover
        prop_oneof![8 => Just(1u64), 1 => Just(2u64)], // version
        prop_oneof![8 => Just(1u64), 1 => Just(9u64)], // binary hash
        prop_oneof![8 => Just(1u64), 1 => Just(2u64)], // task count
        prop::bool::weighted(0.1),                     // binary mismatch
    )
        .prop_map(|(answer, task, partition, imports, imports_hash, prover, version, binary, task_count, binary_mismatch)| {
            Evidence {
                fields: ResultFields {
                    answer,
                    task_hash: h(task),
                    partition,
                    imports,
                    imports_hash: h(imports_hash),
                    prover,
                    version,
                    binary_hash: h(binary),
                },
                local_task_hash: h(7),
                task_count,
                binary_mismatch,
            }
        })
}

/// One input of the machine. Calls are the numbers 0 to 3, Goals 0 to 2,
/// seeds 0 to 3, and times stay below 60.
pub fn input() -> impl Strategy<Value = Input> {
    let rid = 0u64..4;
    let goal = 0u64..3;
    let seed = 0u64..4;
    let now = 0u64..60;
    prop_oneof![
        1 => (now.clone(), any::<bool>()).prop_map(|(now, cluster)| Input::SessionStart { now, cluster }),
        3 => (now.clone(), goal.clone()).prop_map(|(now, goal)| Input::GoalAnnounced { now, goal }),
        6 => (now.clone(), rid.clone(), goal, 1u64..3, any::<bool>(), prop_oneof![Just(5u64), Just(10u64)]).prop_map(
            |(now, rid, goal, prover, fast, micros)| Input::ProverCall {
                now, rid, goal, prover, pass: if fast { Pass::Fast } else { Pass::Later }, micros,
            }
        ),
        6 => (now.clone(), rid.clone(), prop_oneof![5 => Just(1u64), 1 => Just(0u64), 1 => Just(2u64)])
            .prop_map(|(now, rid, task_count)| Input::PrintFacts { now, rid, task_count }),
        6 => (now.clone(), rid.clone(), evidence()).prop_map(|(now, rid, evidence)| Input::LocalRun { now, rid, evidence }),
        8 => (now.clone(), rid.clone(), seed.clone(), 1u64..3, evidence(),
              prop::bool::weighted(0.85), prop::bool::weighted(0.85), prop::bool::weighted(0.1),
              prop_oneof![4 => Just(1_000_000u64), 1 => Just(2_000_000u64)],
              prop_oneof![3 => Just(5u64), 3 => Just(10u64), 1 => Just(20u64)], prop::bool::weighted(0.9))
            .prop_map(|(now, rid, seed, prover, evidence, signature_valid, registered, revoked, velocity, local_micros, imports_consistent)| Input::RemoteResult {
                now, rid, seed, prover, evidence, signature_valid, registered, revoked, velocity, local_micros, imports_consistent,
            }),
        2 => (now.clone(), rid.clone(), seed).prop_map(|(now, rid, seed)| Input::JobNotRun { now, rid, seed }),
        2 => (now.clone(), rid).prop_map(|(now, rid)| Input::CancelCall { now, rid }),
        1 => now.clone().prop_map(|now| Input::ClusterLost { now }),
        2 => now.clone().prop_map(|now| Input::Tick { now }),
        1 => now.prop_map(|now| Input::Why3findDone { now }),
    ]
}

/// Starts most Sessions, so that most sequences do something.
pub fn session() -> impl Strategy<Value = Vec<Input>> {
    (any::<bool>(), proptest::collection::vec(input(), 0..80)).prop_map(|(cluster, mut inputs)| {
        // Every Goal is announced first, so that prover calls are accepted and
        // the sequences reach the states that matter.
        let mut all = vec![Input::SessionStart { now: 0, cluster }];
        all.extend((0..3).map(|goal| Input::GoalAnnounced { now: 0, goal }));
        all.append(&mut inputs);
        all
    })
}
