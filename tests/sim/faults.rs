// Feature: creusot-distributed-proving, Property 10: The Cluster keeps its promises under the fault model
// P-ID: P1.6.4, P1.6.5, P1.6.10, P1.3.4, P1.5.1
//
// The real Coordinator and Worker shells, and a Front_End around the
// dp-core Front_End machine, on a simulated network. Every test is
// deterministic in its seed.

use dp_sim::{truth, Call, Cluster, Reply, Speeds, COORDINATOR, MARGIN, OTHER_PROVER, PROVER, SILENCE, WORKERS};
use dp_testkit::sim::Faults;
use proptest::prelude::*;

/// A call of the first prover; `numbered` gives each call its own Goal.
fn call(at: u64, text: &str, micros: u64) -> Call {
    Call { at, goal: 0, prover: PROVER.into(), text: text.as_bytes().to_vec(), micros }
}

fn numbered(mut calls: Vec<Call>) -> Vec<Call> {
    for (i, call) in calls.iter_mut().enumerate() {
        call.goal = i as u64;
    }
    calls
}

/// When every call must have its answer: the race deadline of the last
/// call, a local run after a fallback, and the time to notice a silence.
fn horizon(script: &[Call]) -> u64 {
    // Limits scale with the speed of a machine, at most 4 here.
    script.iter().map(|c| c.at + 4 * (3 * c.micros + MARGIN)).max().unwrap_or(0) + 3 * SILENCE
}

fn mixed() -> Vec<Call> {
    numbered(vec![
        call(0, "unsat 30000", 100_000),
        call(1_000, "unknown 20000", 100_000),
        call(2_000, "unsat 400000", 100_000), // too slow: a timeout
        call(3_000, "sat 10000", 100_000),
        call(50_000, "unsat 5000", 100_000),
        call(60_000, "hang", 50_000),
    ])
}

#[test]
fn without_faults_every_call_gets_the_true_answer_from_the_cluster() {
    let script = mixed();
    let mut cluster = Cluster::new(1, Faults::none(), 2, 0, script.clone(), 1);
    cluster.run_until(horizon(&script));
    for (rid, call) in script.iter().enumerate() {
        assert_eq!(cluster.answer(rid).unwrap().0, truth(call), "call {rid}");
    }
    assert_eq!(cluster.local_runs(), 0, "nothing ran on the Front_End's machine");
    let world = cluster.world.borrow();
    assert!(world.most_runs.values().all(|n| *n <= 1), "{:?}", world.most_runs);
    assert_eq!(world.started.len(), script.len(), "each Job ran once");
    drop(world);
    assert_eq!(cluster.coordinator.as_ref().unwrap().held(), 0, "the Coordinator holds no finished Job");
}

#[test]
fn the_job_of_a_crashed_worker_goes_to_another_worker() {
    let script = vec![call(0, "unsat 200000", 400_000)];
    let mut cluster = Cluster::new(2, Faults::none(), 2, 0, script.clone(), 1);
    cluster.run_until(50_000);
    assert_eq!(cluster.runners(), vec![WORKERS]);
    cluster.crash(WORKERS, true);
    cluster.run_until(horizon(&script));
    let (reply, at) = cluster.answer(0).unwrap();
    assert_eq!(reply, Reply::Valid);
    // well before the race deadline: the Job was assigned again at once
    assert!(at < 50_000 + 200_000 + 10_000, "answered at {at}");
    assert_eq!(cluster.local_runs(), 0);
    assert_eq!(cluster.runners(), vec![WORKERS, WORKERS + 1]);
}

#[test]
fn the_job_of_a_silent_worker_goes_to_another_worker() {
    let script = vec![call(0, "unsat 200000", 400_000)];
    let mut cluster = Cluster::new(3, Faults::none(), 2, 0, script.clone(), 1);
    cluster.run_until(50_000);
    // Cut off, not closed: only the missing heartbeats show it.
    cluster.world.borrow_mut().isolate(WORKERS);
    cluster.run_until(horizon(&script));
    let (reply, at) = cluster.answer(0).unwrap();
    assert_eq!(reply, Reply::Valid);
    assert!(at > 50_000 + SILENCE && at < 50_000 + 2 * SILENCE + 200_000, "answered at {at}");
    assert_eq!(cluster.local_runs(), 0);
    // The Worker that was cut off stopped its orphaned Job by itself.
    assert_eq!(cluster.workers[&WORKERS].state().running, None);
}

#[test]
fn retries_end_and_the_call_runs_at_home() {
    // One Worker at a time, and each one dies with the Job.
    let script = vec![call(0, "unsat 300000", 400_000)];
    let mut cluster = Cluster::new(4, Faults::none(), 3, 0, script.clone(), 1);
    for (i, at) in [50_000u64, 100_000, 150_000].into_iter().enumerate() {
        cluster.run_until(at);
        cluster.crash(WORKERS + i as u32, true);
    }
    cluster.run_until(horizon(&script));
    assert_eq!(cluster.answer(0).unwrap().0, Reply::Valid);
    // No Worker is left: the Coordinator says so, and the call runs locally.
    assert_eq!(cluster.local_runs(), 1);
}

#[test]
fn a_silent_coordinator_sends_the_session_home() {
    let script = mixed();
    let mut cluster = Cluster::new(5, Faults::none(), 2, 0, script.clone(), 1);
    cluster.run_until(10_000);
    cluster.crash(COORDINATOR, false);
    cluster.run_until(horizon(&script));
    for (rid, call) in script.iter().enumerate() {
        assert_eq!(cluster.answer(rid).unwrap().0, truth(call), "call {rid}");
    }
    assert!(!cluster.front_end.state().cluster);
    assert!(cluster.local_runs() > 0);
    // The Workers stopped what they ran for a Coordinator that is gone.
    assert!(cluster.workers.values().all(|w| w.state().running.is_none()));
}

#[test]
fn workers_join_a_restarted_coordinator_again() {
    let mut cluster = Cluster::new(6, Faults::none(), 2, 0, vec![], 1);
    cluster.run_until(10_000);
    cluster.crash(COORDINATOR, false);
    cluster.run_until(10_000 + 2 * SILENCE);
    cluster.start_coordinator();
    cluster.run_until(10_000 + 4 * SILENCE);
    assert!(cluster.workers.values().all(|w| w.connections == 2 && w.state().connected));
    assert_eq!(cluster.coordinator.as_ref().unwrap().state().workers.len(), 2);
}

#[test]
fn a_forging_coordinator_proves_nothing() {
    // Every Result that the Coordinator forwards claims a proof.
    let script = mixed();
    let mut cluster = Cluster::new(7, Faults::none(), 2, 0, script.clone(), 1);
    cluster.world.borrow_mut().forging.insert(COORDINATOR);
    cluster.run_until(horizon(&script));
    for (rid, call) in script.iter().enumerate() {
        let reply = cluster.answer(rid).unwrap().0;
        // Runs that printed `unsat` anyway are forwarded unchanged.
        assert!(reply != Reply::Valid || truth(call) == Reply::Valid, "call {rid}: {reply:?}");
    }
}

#[test]
fn unregistered_workers_prove_nothing() {
    let script = mixed();
    let mut cluster = Cluster::new(8, Faults::none(), 2, 2, script.clone(), 1);
    cluster.run_until(horizon(&script));
    for rid in 0..script.len() {
        assert!(cluster.answer(rid).is_some_and(|(reply, _)| reply != Reply::Valid), "call {rid}");
    }
}

/// One fault of a schedule.
#[derive(Clone, Debug)]
enum Fault {
    Crash { node: u32, noticed: bool },
    Restart { node: u32 },
    Isolate { node: u32 },
    Heal { node: u32 },
}

fn schedule() -> impl Strategy<Value = Vec<(u64, Fault)>> {
    // Node 0 stands for the Coordinator, 1 to 3 for the Workers.
    let fault = (0u32..4, 0u8..4, any::<bool>()).prop_map(|(node, kind, noticed)| match kind {
        0 => Fault::Crash { node, noticed },
        1 => Fault::Restart { node },
        2 => Fault::Isolate { node },
        _ => Fault::Heal { node },
    });
    proptest::collection::vec((0u64..400_000, fault), 0..8).prop_map(|mut faults| {
        faults.sort_by_key(|(at, _)| *at);
        faults
    })
}

fn calls() -> impl Strategy<Value = Vec<Call>> {
    let goal = prop_oneof![
        3 => (1_000u64..80_000).prop_map(|t| format!("unsat {t}")),
        2 => (1_000u64..80_000).prop_map(|t| format!("unknown {t}")),
        1 => (1_000u64..80_000).prop_map(|t| format!("sat {t}")),
        1 => Just("hang".to_string()),
    ];
    proptest::collection::vec((0u64..300_000, goal, 20_000u64..100_000), 1..10).prop_map(|mut calls| {
        calls.sort_by_key(|(at, _, _)| *at);
        numbered(calls.into_iter().map(|(at, goal, micros)| call(at, &goal, micros)).collect())
    })
}

/// Runs a Cluster of three Workers under a fault schedule; returns the
/// replies and the largest number of runs that a Worker had at once.
fn run(
    seed: u64,
    faults: Faults,
    script: &[Call],
    schedule: &[(u64, Fault)],
    forging: bool,
    seeds: u64,
    speeds: &Speeds,
) -> (Vec<Option<Reply>>, usize) {
    let mut cluster = Cluster::with_speeds(seed, faults, 3, 0, script.to_vec(), seeds, speeds.clone());
    if forging {
        cluster.world.borrow_mut().forging.insert(COORDINATOR);
    }
    let node = |n: u32| if n == 0 { COORDINATOR } else { WORKERS + n - 1 };
    for (at, fault) in schedule {
        cluster.run_until(*at);
        match fault {
            Fault::Crash { node: n, noticed } => cluster.crash(node(*n), *noticed),
            Fault::Restart { node: 0 } => {
                if cluster.coordinator.is_none() {
                    cluster.start_coordinator();
                }
            }
            Fault::Restart { node: n } => {
                if !cluster.workers.contains_key(&node(*n)) {
                    cluster.start_worker(node(*n));
                }
            }
            Fault::Isolate { node: n } => cluster.world.borrow_mut().isolate(node(*n)),
            Fault::Heal { node: n } => cluster.world.borrow_mut().heal(node(*n)),
        }
    }
    cluster.run_until(horizon(script).max(400_000) + 3 * SILENCE);
    let replies = (0..script.len()).map(|rid| cluster.answer(rid).map(|a| a.0)).collect();
    let most = cluster.world.borrow().most_runs.iter().filter(|(n, _)| **n >= WORKERS).map(|(_, m)| *m).max().unwrap_or(0);
    (replies, most)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    // Crashes, restarts, partitions, loss, duplicates and delays, with an
    // honest or a forging Coordinator: every call is answered, once; no
    // answer says more than an honest run of the call; a Worker runs one
    // Job at a time; and the same seed gives the same run.
    #[test]
    fn a_cluster_under_faults_keeps_its_promises(
        seed in any::<u64>(),
        loss in prop_oneof![Just(0u32), 0u32..200_000],
        duplicate in prop_oneof![Just(0u32), 0u32..200_000],
        max_delay in 100u64..30_000,
        script in calls(),
        schedule in schedule(),
        forging in prop::bool::weighted(0.2),
        seeds in 1u64..3,
        // Machines as fast as the reference machine or slower.
        front_end in 1_000_000u64..3_000_000,
        workers in proptest::collection::vec(1_000_000u64..4_000_000, 3),
    ) {
        let speeds = Speeds { front_end, workers };
        let faults = Faults { loss, duplicate, min_delay: 50, max_delay };
        let (replies, most) = run(seed, faults, &script, &schedule, forging, seeds, &speeds);
        for (rid, (reply, call)) in replies.iter().zip(&script).enumerate() {
            let Some(reply) = reply else {
                return Err(TestCaseError::fail(format!("call {rid} was not answered")));
            };
            let honest = truth(call);
            // A race that the faults ended is a timeout; nothing else may
            // differ from the honest answer, and a proof never does.
            prop_assert!(*reply == honest || *reply == Reply::Timeout, "call {}: {:?}, honest {:?}", rid, reply, honest);
        }
        prop_assert!(most <= 1, "a Worker had {} runs at once", most);
        let again = run(seed, faults, &script, &schedule, forging, seeds, &speeds);
        prop_assert_eq!(again.0, replies);
    }
}

// P-ID: P1.4.1, P1.4.6
#[test]
fn machines_of_different_speeds_give_the_same_answers() {
    // A Worker three and a half times slower than the reference machine,
    // one twice as slow, and a Front_End that is slower too. Limits scale
    // with each machine's Calibration, so the answers are those of the
    // reference machine, and the Front_End accepts the scaled limits.
    let script = mixed();
    let speeds = Speeds { front_end: 1_500_000, workers: vec![3_500_000, 2_000_000] };
    let mut cluster = Cluster::with_speeds(9, Faults::none(), 2, 0, script.clone(), 1, speeds);
    cluster.run_until(horizon(&script));
    for (rid, call) in script.iter().enumerate() {
        assert_eq!(cluster.answer(rid).unwrap().0, truth(call), "call {rid}");
    }
    assert_eq!(cluster.local_runs(), 0);
    // The replay fields of a proof carry the Worker's Calibration and the
    // limit that it gives.
    let labels = cluster.front_end.labels();
    let proved: Vec<_> = labels["sim.mlw"].iter().filter(|g| g.proof).collect();
    assert_eq!(proved.len(), 2);
    for goal in proved {
        let replay = &goal.entry.replay[0];
        let velocity = replay.velocity.unwrap();
        assert!(velocity == 3_500_000 || velocity == 2_000_000, "{replay:?}");
        assert_eq!(
            u128::from(replay.local_micros.unwrap()),
            u128::from(replay.reference_micros) * u128::from(velocity) / 1_000_000
        );
    }
}

// P-ID: P7.2.7
#[test]
fn a_proof_and_an_invalid_answer_for_one_goal_are_a_conflict() {
    // One prover answers Invalid on the Goal, another proves it.
    let script = vec![
        Call { at: 0, goal: 0, prover: PROVER.into(), text: b"sat 10000".to_vec(), micros: 100_000 },
        Call { at: 50_000, goal: 0, prover: OTHER_PROVER.into(), text: b"unsat 10000".to_vec(), micros: 100_000 },
        // A Goal without an Invalid answer has no conflict.
        Call { at: 60_000, goal: 1, prover: PROVER.into(), text: b"unsat 10000".to_vec(), micros: 100_000 },
    ];
    let mut cluster = Cluster::new(10, Faults::none(), 2, 0, script.clone(), 1);
    cluster.run_until(horizon(&script));
    assert_eq!(cluster.answer(0).unwrap().0, Reply::Unknown);
    assert_eq!(cluster.answer(1).unwrap().0, Reply::Valid);
    let labels = cluster.front_end.labels();
    let goal = |name: &str| labels["sim.mlw"].iter().find(|g| g.name == name).unwrap();
    let conflicted = &goal("G0").entry;
    assert_eq!(conflicted.label, "proved-standalone");
    assert!(conflicted.conflict);
    assert_eq!(conflicted.invalid.len(), 1);
    assert_eq!(conflicted.invalid[0].prover, "stub");
    assert_eq!(conflicted.replay[0].prover, "other");
    assert!(!goal("G1").entry.conflict && goal("G1").entry.invalid.is_empty());
}
