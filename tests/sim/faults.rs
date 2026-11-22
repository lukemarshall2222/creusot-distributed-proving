// Feature: creusot-distributed-proving, Property 10: The Cluster keeps its promises under the fault model
// P-ID: P1.6.4, P1.6.5, P1.6.10, P1.3.4, P1.5.1
//
// The real Coordinator and Worker shells, and a Front_End around the
// dp-core Front_End machine, on a simulated network. Every test is
// deterministic in its seed.

use dp_formats::cluster::{Message, Operation};
use dp_sim::{operator_key, truth, Call, Cluster, Reply, Setup, Speeds, COORDINATOR, MARGIN, OTHER_PROVER, PROVER, SILENCE, WORKERS};
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
    local_member: bool,
) -> (Vec<Option<Reply>>, usize) {
    let setup = Setup { seeds, speeds: speeds.clone(), local_member, ..Setup::plain() };
    let mut cluster = Cluster::with(seed, faults, 3, 0, script.to_vec(), setup);
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
        local_member in any::<bool>(),
    ) {
        let speeds = Speeds { front_end, workers };
        let faults = Faults { loss, duplicate, min_delay: 50, max_delay };
        let (replies, most) = run(seed, faults, &script, &schedule, forging, seeds, &speeds, local_member);
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
        let again = run(seed, faults, &script, &schedule, forging, seeds, &speeds, local_member);
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
    assert_eq!(conflicted.replay[0].prover, "cvc5");
    assert!(!goal("G1").entry.conflict && goal("G1").entry.invalid.is_empty());
}

// P-ID: P2.2.1, P2.2.8
#[test]
fn a_portfolio_races_provers_and_the_first_proof_wins() {
    // The call's prover never answers on these Goals; the other member of
    // the Portfolio proves the first, and neither proves the second.
    let script = numbered(vec![call(0, "hang", 100_000), call(1_000, "unknown 20000", 100_000)]);
    let setup = Setup {
        portfolio: vec![PROVER.into(), OTHER_PROVER.into()],
        other_prints: [(0, b"unsat 10000".to_vec()), (1, b"unknown 30000".to_vec())].into(),
        ..Setup::plain()
    };
    let mut cluster = Cluster::with(11, Faults::none(), 2, 0, script.clone(), setup);
    cluster.run_until(horizon(&script));
    // The first call is proved long before its own prover's limit.
    let (reply, at) = cluster.answer(0).unwrap();
    assert_eq!(reply, Reply::Valid);
    assert!(at < 30_000, "answered at {at}");
    assert_eq!(cluster.answer(1).unwrap().0, Reply::Unknown);
    assert_eq!(cluster.local_runs(), 0);
    // The record names the prover that proved the Goal, not the call's.
    let labels = cluster.front_end.labels();
    let goal = |name: &str| labels["sim.mlw"].iter().find(|g| g.name == name).unwrap();
    assert_eq!(goal("G0").entry.label, "proved-standalone");
    assert_eq!(goal("G0").entry.replay[0].prover, "cvc5");
    assert_eq!(goal("G1").entry.label, "unknown");
    // The loser of the won race was cancelled: the Workers are idle again.
    assert!(cluster.workers.values().all(|w| w.state().running.is_none()));
}

// P-ID: P7.2.7, P2.2.19
#[test]
fn a_conflict_inside_a_race_is_recorded() {
    // One member answers Invalid before the other proves the Goal.
    let script = numbered(vec![call(0, "sat 5000", 100_000)]);
    let setup = Setup {
        portfolio: vec![PROVER.into(), OTHER_PROVER.into()],
        other_prints: [(0, b"unsat 40000".to_vec())].into(),
        ..Setup::plain()
    };
    let mut cluster = Cluster::with(12, Faults::none(), 2, 0, script.clone(), setup);
    cluster.run_until(horizon(&script));
    assert_eq!(cluster.answer(0).unwrap().0, Reply::Valid);
    let labels = cluster.front_end.labels();
    let entry = &labels["sim.mlw"][0].entry;
    assert!(entry.conflict);
    assert_eq!(entry.invalid[0].prover, "stub");
    assert_eq!(entry.replay[0].prover, "cvc5");
}

// P-ID: P1.2.1 (the option that moves the first pass)
#[test]
fn the_first_pass_runs_on_the_cluster_when_configured() {
    let script = mixed();
    let setup = Setup { first_pass_on_cluster: true, ..Setup::plain() };
    let mut cluster = Cluster::with(13, Faults::none(), 2, 0, script.clone(), setup);
    cluster.run_until(horizon(&script));
    for (rid, call) in script.iter().enumerate() {
        assert_eq!(cluster.answer(rid).unwrap().0, truth(call), "call {rid}");
    }
    assert_eq!(cluster.local_runs(), 0);
    // Without the option, calls of the first pass stay on this machine.
    let mut cluster = Cluster::with(13, Faults::none(), 2, 0, script.clone(), Setup::plain());
    cluster.front_end.bridge.first_pass();
    cluster.run_until(horizon(&script));
    assert_eq!(cluster.local_runs(), script.len());
}

/// Calls that keep two Workers busy for a while: one every 20 ms, each
/// 30 ms long.
fn steady(n: u64) -> Vec<Call> {
    numbered((0..n).map(|i| call(i * 20_000, "unsat 30000", 200_000)).collect())
}

// P-ID: P1.7.3
#[test]
fn a_drained_worker_ends_its_job_and_gets_no_other() {
    let script = steady(10);
    let mut cluster = Cluster::new(20, Faults::none(), 2, 0, script.clone(), 1);
    cluster.run_until(45_000);
    assert!(cluster.operate(1, Operation::Drain, WORKERS, &operator_key()));
    let drained_at = cluster.now();
    cluster.run_until(horizon(&script));
    // Every call is proved, on the Cluster, and nothing was lost.
    for rid in 0..script.len() {
        assert_eq!(cluster.answer(rid).unwrap().0, Reply::Valid, "call {rid}");
    }
    assert_eq!(cluster.local_runs(), 0);
    let world = cluster.world.borrow();
    assert_eq!(world.started.len(), script.len(), "no Job ran twice");
    // After the command the drained Worker started nothing.
    assert!(world.started.iter().all(|(node, at, _)| *node != WORKERS || *at <= drained_at), "{:?}", world.started);
    drop(world);
    // The report says so.
    let Some(Message::Status { workers, queued, front_ends, .. }) = cluster.status() else { panic!("no status") };
    assert_eq!((queued, front_ends), (0, 1));
    let state: Vec<_> = workers.iter().map(|w| (w.worker.as_str(), w.draining, w.job.is_some())).collect();
    assert_eq!(state, vec![("w10", true, false), ("w11", false, false)]);
}

// P-ID: P1.7.5
#[test]
fn only_the_operator_commands_and_each_command_once() {
    let script = steady(4);
    let mut cluster = Cluster::new(21, Faults::none(), 2, 0, script.clone(), 1);
    cluster.run_until(10_000);
    // Another key: refused, and nothing changes.
    let stranger = ed25519_dalek::SigningKey::from_bytes(&[9; 32]);
    assert!(!cluster.operate(1, Operation::Remove, WORKERS, &stranger));
    assert_eq!(cluster.coordinator.as_ref().unwrap().state().workers.len(), 2);
    // The operator's command is taken once; the same serial number again,
    // or a smaller one, is not: a recorded command cannot be replayed.
    assert!(cluster.operate(5, Operation::Drain, WORKERS, &operator_key()));
    assert!(!cluster.operate(5, Operation::Remove, WORKERS, &operator_key()));
    assert!(!cluster.operate(4, Operation::Remove, WORKERS, &operator_key()));
    assert_eq!(cluster.coordinator.as_ref().unwrap().state().workers.len(), 2);
    assert!(cluster.operate(6, Operation::Remove, WORKERS + 1, &operator_key()));
    cluster.run_until(horizon(&script));
    for rid in 0..script.len() {
        assert_eq!(cluster.answer(rid).unwrap().0, Reply::Valid, "call {rid}");
    }
}

// P-ID: P1.7.6
#[test]
fn a_revoked_worker_is_removed_and_its_results_count_for_nothing() {
    let script = steady(8);
    let mut cluster = Cluster::new(22, Faults::none(), 2, 0, script.clone(), 1);
    cluster.run_until(45_000);
    assert!(cluster.operate(1, Operation::Revoke, WORKERS, &operator_key()));
    let revoked_at = cluster.now();
    // The Front_End has the revocation, with the operator's signature.
    assert!(cluster.front_end.revoked().contains("w10"));
    cluster.run_until(horizon(&script));
    for rid in 0..script.len() {
        assert_eq!(cluster.answer(rid).unwrap().0, Reply::Valid, "call {rid}");
    }
    // The revoked Worker keeps trying to join and is turned away: it is
    // not among the Workers, and it ran nothing after the revocation.
    let Some(Message::Status { workers, .. }) = cluster.status() else { panic!("no status") };
    assert_eq!(workers.iter().map(|w| w.worker.as_str()).collect::<Vec<_>>(), vec!["w11"]);
    assert!(cluster.workers[&WORKERS].connections > 1);
    let world = cluster.world.borrow();
    assert!(world.started.iter().all(|(node, at, _)| *node != WORKERS || *at <= revoked_at));
    drop(world);
    // No proof in the record names it for a Job that began after that; and
    // the Job it was running went to the other Worker.
    let labels = cluster.front_end.labels();
    let by_revoked = labels["sim.mlw"]
        .iter()
        .filter(|g| g.entry.replay.iter().any(|r| r.place == dp_formats::proofjson::Place::Worker("w10".into())))
        .count();
    assert!(by_revoked <= 2, "{by_revoked} proofs by the revoked Worker");
}

/// The Front_End does not rely on the Coordinator to enforce a revocation:
/// here the Coordinator goes on giving Jobs to the Worker, and the
/// Front_End alone refuses its Results. A revocation that the operator did
/// not sign changes nothing.
// P-ID: P1.7.6, P1.3.9
#[test]
fn the_front_end_enforces_a_revocation_by_itself() {
    use dp_formats::cluster::SignedCommand;
    use dp_shell::env::Env;
    use dp_testkit::sim::SimEnv;
    for (signer, proved) in [(operator_key(), false), (ed25519_dalek::SigningKey::from_bytes(&[9; 32]), true)] {
        let script = steady(4);
        let mut cluster = Cluster::new(23, Faults::none(), 1, 0, script.clone(), 1);
        cluster.run_until(5_000);
        // The revocation arrives on the Front_End's connection, as if the
        // Coordinator passed it on without acting on it.
        let front_end = cluster.coordinator.as_ref().unwrap().state().front_ends[0].peer;
        let command = SignedCommand::sign(1, Operation::Revoke, "w10", &signer);
        SimEnv::new(&cluster.world, COORDINATOR).send(front_end, &Message::Revocation(command));
        cluster.run_until(horizon(&script));
        assert_eq!(cluster.front_end.revoked().contains("w10"), !proved);
        for rid in 0..script.len() {
            let reply = cluster.answer(rid).unwrap().0;
            assert_eq!(reply == Reply::Valid, proved, "call {rid}: {reply:?}");
        }
        // The Worker did run the Jobs: its Results were refused, not missing.
        assert!(cluster.runners().contains(&WORKERS));
    }
}

/// A Session in which no solver is an Unpatched_Solver, so that every
/// reported proof needs a certificate. The proof producer prints each Goal
/// as the call's prover does.
fn certifying(script: &[Call]) -> Setup {
    Setup {
        certify: true,
        other_prints: script.iter().map(|c| (c.goal, c.text.clone())).collect(),
        ..Setup::plain()
    }
}

fn label_of(cluster: &Cluster, goal: &str) -> String {
    cluster.front_end.labels()["sim.mlw"].iter().find(|g| g.name == goal).unwrap().entry.label.clone()
}

// P-ID: P5.2.1, P5.3.1
#[test]
fn a_worker_produces_the_certificate_and_the_front_end_checks_it() {
    let script = mixed();
    let mut cluster = Cluster::with(30, Faults::none(), 2, 0, script.clone(), certifying(&script));
    cluster.run_until(horizon(&script));
    for (rid, call) in script.iter().enumerate() {
        assert_eq!(cluster.answer(rid).unwrap().0, truth(call), "call {rid}");
    }
    // Proved Goals are certified, with a store entry; nothing is
    // proved-standalone, because no solver is an Unpatched_Solver.
    assert_eq!(label_of(&cluster, "G0"), "certified");
    assert_eq!(label_of(&cluster, "G4"), "certified");
    assert_eq!(label_of(&cluster, "G1"), "unknown");
    let labels = cluster.front_end.labels();
    let certified = labels["sim.mlw"].iter().find(|g| g.name == "G0").unwrap();
    let content = certified.entry.certificate.expect("a certified entry names its certificate");
    assert!(cluster.dir.join(".dp-certs").join(content.to_hex()).join("proof").is_file());
    // The Front_End's machine ran only the checker, once per certificate:
    // the proofs were produced by Workers.
    let world = cluster.world.borrow();
    let local: Vec<_> = world.started.iter().filter(|(node, _, _)| *node == dp_sim::FRONT_END).collect();
    assert_eq!(local.len(), 2, "{local:?}");
    assert!(local.iter().all(|(_, _, input)| input.starts_with(b"(step proof-of ")));
}

// P-ID: P5.3.1, P1.3.4
#[test]
fn a_forged_certificate_certifies_nothing() {
    // The Coordinator rewrites every Result, the answers to proof Jobs
    // included: what arrives is not a proof that the checker accepts.
    let script = mixed();
    let mut cluster = Cluster::with(31, Faults::none(), 2, 0, script.clone(), certifying(&script));
    cluster.world.borrow_mut().forging.insert(COORDINATOR);
    cluster.run_until(horizon(&script));
    for rid in 0..script.len() {
        assert!(cluster.answer(rid).is_some_and(|(reply, _)| reply != Reply::Valid), "call {rid}");
    }
    assert!(cluster.front_end.labels()["sim.mlw"].iter().all(|g| !g.proof));
}

// P-ID: P5.2.3
#[test]
fn without_a_cluster_the_certificate_is_produced_at_home() {
    let script = numbered(vec![call(0, "unsat 30000", 100_000)]);
    let mut cluster = Cluster::with(32, Faults::none(), 1, 0, script.clone(), certifying(&script));
    // The Coordinator is gone before the proof Job can be answered.
    cluster.run_until(45_000);
    cluster.crash(COORDINATOR, true);
    cluster.run_until(horizon(&script));
    assert_eq!(cluster.answer(0).unwrap().0, Reply::Valid);
    assert_eq!(label_of(&cluster, "G0"), "certified");
    // The producer and the checker both ran on the Front_End's machine.
    let world = cluster.world.borrow();
    let local: Vec<_> = world.started.iter().filter(|(node, _, _)| *node == dp_sim::FRONT_END).collect();
    assert!(local.len() >= 2, "{local:?}");
}

// P-ID: P2.2.1
#[test]
fn the_local_member_of_a_race_wins_against_slow_workers() {
    // Workers four times slower than the Front_End's machine.
    let script = numbered(vec![call(0, "unsat 20000", 200_000), call(1_000, "unknown 20000", 200_000)]);
    let setup = Setup {
        local_member: true,
        speeds: Speeds { front_end: 1_000_000, workers: vec![4_000_000] },
        ..Setup::plain()
    };
    let mut cluster = Cluster::with(40, Faults::none(), 2, 0, script.clone(), setup);
    cluster.run_until(horizon(&script));
    // The proof came from this machine, before a Worker could finish.
    let (reply, at) = cluster.answer(0).unwrap();
    assert_eq!(reply, Reply::Valid);
    assert!(at < 40_000, "answered at {at}");
    let labels = cluster.front_end.labels();
    let proved = labels["sim.mlw"].iter().find(|g| g.name == "G0").unwrap();
    assert_eq!(proved.entry.replay[0].place, dp_formats::proofjson::Place::Local);
    assert_eq!(cluster.answer(1).unwrap().0, Reply::Unknown);
    // The Workers' Jobs of the won race were cancelled.
    assert!(cluster.workers.values().all(|w| w.state().running.is_none()));
}

// P-ID: P1.7.6, P1.6.14
#[test]
fn a_restarted_coordinator_remembers_revocations_and_serial_numbers() {
    let mut cluster = Cluster::new(50, Faults::none(), 2, 0, vec![], 1);
    cluster.run_until(10_000);
    assert_eq!(cluster.front_end.incarnation(), Some(1));
    assert!(cluster.operate(3, Operation::Revoke, WORKERS, &operator_key()));
    cluster.crash(COORDINATOR, true);
    cluster.run_until(10_000 + SILENCE);
    cluster.start_coordinator();
    cluster.run_until(10_000 + 4 * SILENCE);
    // It is another start of the Coordinator, and it says so.
    let Some(Message::Status { incarnation, workers, .. }) = cluster.status() else { panic!("no status") };
    assert_eq!(incarnation, 2);
    // The revoked Worker is still turned away; the other one joined again.
    assert_eq!(workers.iter().map(|w| w.worker.as_str()).collect::<Vec<_>>(), vec!["w11"]);
    // A command that the first start took, or an older one, is not taken
    // again by the second.
    assert!(!cluster.operate(3, Operation::Remove, WORKERS + 1, &operator_key()));
    assert!(!cluster.operate(2, Operation::Remove, WORKERS + 1, &operator_key()));
    assert!(cluster.operate(4, Operation::Drain, WORKERS + 1, &operator_key()));
    // The Front_End's project remembers the revocation too.
    let stored = dp_frontend::revocations::load(&cluster.dir, &operator_key().verifying_key());
    assert!(stored.contains("w10"), "{stored:?}");
}
