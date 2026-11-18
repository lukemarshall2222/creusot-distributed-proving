// Feature: creusot-distributed-proving, Property 1: Proof labels come only from the two routes
// Feature: creusot-distributed-proving, Property 4: Discards are no-ops and handlers are idempotent
// P-ID: P7.1.1, P7.1.3, P1.5.1, P1.5.6, P1.3.2, P1.3.4, P1.3.6, P7.3.1

//! Property tests of the Front_End machine over generated event sequences.
//! The machine is pure, so a whole Session is a fold over its inputs.

use std::collections::{BTreeMap, BTreeSet};

use dp_core::{
    frontend::{step, Config, Effect, Evidence, FrontEnd, Input, Pass, Phase, Reply, State},
    labels::ProverAnswer,
    machine::Machine,
    trace::TlaAction,
    verdict::{Hash, Label, ResultFields, UnpatchedSolver},
};
use proptest::prelude::*;

fn h(n: u64) -> Hash {
    Hash { w0: n, w1: 0, w2: 0, w3: 0 }
}

/// The pinned solver: prover 1, version 1, binary hash h(1).
fn config(local_jobs: u64, seeds: u64) -> Config {
    Config {
        local_jobs,
        seeds,
        job_margin: 10,
        silence_limit: 0,
        beat_period: u64::MAX,
        certify_all: false,
        unpatched: vec![UnpatchedSolver { prover: 1, version: 1, binary_hash: h(1) }],
        empty_imports_hash: h(0),
    }
}

/// Evidence that mostly satisfies R7.1 criterion 2, with each condition
/// broken some of the time.
fn evidence() -> impl Strategy<Value = Evidence> {
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

/// R7.1 criterion 2, restated independently of dp-core, for the context that
/// `config` and the evidence give.
fn standalone(e: &Evidence) -> bool {
    e.fields.answer == ProverAnswer::Valid
        && e.fields.task_hash == e.local_task_hash
        && e.task_count == 1
        && !e.fields.partition
        && e.fields.imports == 0
        && e.fields.imports_hash == h(0)
        && e.fields.prover == 1
        && e.fields.version == 1
        && e.fields.binary_hash == h(1)
        && !e.binary_mismatch
}

fn input() -> impl Strategy<Value = Input> {
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
              prop_oneof![3 => Just(5u64), 3 => Just(10u64), 1 => Just(20u64)])
            .prop_map(|(now, rid, seed, prover, evidence, signature_valid, registered, revoked, velocity, local_micros)| Input::RemoteResult {
                now, rid, seed, prover, evidence, signature_valid, registered, revoked, velocity, local_micros,
            }),
        2 => (now.clone(), rid.clone(), seed).prop_map(|(now, rid, seed)| Input::JobNotRun { now, rid, seed }),
        2 => (now.clone(), rid).prop_map(|(now, rid)| Input::CancelCall { now, rid }),
        1 => now.clone().prop_map(|now| Input::ClusterLost { now }),
        2 => now.clone().prop_map(|now| Input::Tick { now }),
        1 => now.prop_map(|now| Input::Why3findDone { now }),
    ]
}

/// Starts most Sessions, so that most sequences do something.
fn session() -> impl Strategy<Value = Vec<Input>> {
    (any::<bool>(), proptest::collection::vec(input(), 0..80)).prop_map(|(cluster, mut inputs)| {
        // Every Goal is announced first, so that prover calls are accepted and
        // the sequences reach the states that matter.
        let mut all = vec![Input::SessionStart { now: 0, cluster }];
        all.extend((0..3).map(|goal| Input::GoalAnnounced { now: 0, goal }));
        all.append(&mut inputs);
        all
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn a_session_keeps_its_promises(inputs in session(), local_jobs in 0u64..3, seeds in 0u64..4) {
        let cfg = config(local_jobs, seeds);
        let mut state: State = FrontEnd::init(&cfg);
        let mut answered: BTreeSet<u64> = BTreeSet::new();
        let mut open: BTreeSet<u64> = BTreeSet::new();          // calls made and not yet answered
        let mut local: BTreeSet<u64> = BTreeSet::new();         // calls with a local solver
        let mut goal_of: BTreeMap<u64, u64> = BTreeMap::new();

        for input in inputs {
            let before = state.clone();
            // Determinism (P7.3.1).
            let again = step(state.clone(), input);
            let out = step(state, input);
            prop_assert_eq!(&out.state, &again.state);
            prop_assert_eq!(&out.effects, &again.effects);
            prop_assert_eq!(out.action, again.action);

            // A discard is a no-op (P1.5.6); a finished Session discards everything.
            if out.action == TlaAction::Discard {
                prop_assert_eq!(&out.state, &before);
                prop_assert!(out.effects.is_empty());
            }
            if before.phase == Phase::Finished {
                prop_assert_eq!(out.action, TlaAction::Discard);
            }

            // A Result that is not authentic changes nothing: its Job stays
            // open for an authentic one (R1.3 criterion 4, P1.3.2).
            if let Input::RemoteResult { signature_valid, registered, revoked, .. } = input {
                if !(signature_valid && registered && !revoked) {
                    prop_assert_eq!(out.action, TlaAction::Discard);
                }
            }

            if let Input::ProverCall { rid, goal, .. } = input {
                if out.action != TlaAction::Discard {
                    open.insert(rid);
                    answered.remove(&rid); // the identifier is in use again
                    goal_of.insert(rid, goal);
                }
            }

            let mut proved_now = Vec::new();
            for effect in &out.effects {
                match *effect {
                    Effect::Answer { rid, reply } => {
                        // Each call is answered once, and only if it is open.
                        prop_assert!(open.remove(&rid), "answer for a call that is not open: {rid}");
                        prop_assert!(answered.insert(rid));
                        local.remove(&rid);
                        if reply == Reply::Valid {
                            proved_now.push(rid);
                        }
                    }
                    Effect::StartLocal { rid } => {
                        prop_assert!(open.contains(&rid));
                        prop_assert!(local.insert(rid));
                    }
                    // Only a Session on the Cluster talks to the Coordinator.
                    Effect::Beat | Effect::LeaveCluster => prop_assert!(before.cluster),
                    Effect::CancelLocal { rid } => {
                        prop_assert!(local.contains(&rid));
                    }
                    Effect::Submit { .. } => {
                        // Jobs go to the Cluster only while the Session uses it.
                        prop_assert!(before.cluster);
                    }
                    Effect::Print { rid } => prop_assert!(open.contains(&rid)),
                    Effect::CancelJob { .. } => {}
                }
            }
            // Never more local solvers than configured.
            prop_assert!(local.len() as u64 <= local_jobs.max(0));

            // A Valid reply comes only from a run that meets every condition
            // of R7.1 criterion 2 and, from the Cluster, from an authentic
            // Result for an open Job of that call and its prover (P7.1.1,
            // P7.1.3, P1.3.2, P1.3.4, P1.3.6).
            for rid in proved_now {
                match input {
                    Input::LocalRun { rid: r, evidence, .. } => {
                        prop_assert_eq!(r, rid);
                        prop_assert!(standalone(&evidence));
                    }
                    Input::RemoteResult { rid: r, seed, prover, evidence, signature_valid, registered, revoked, velocity, local_micros, .. } => {
                        prop_assert_eq!(r, rid);
                        prop_assert!(standalone(&evidence));
                        prop_assert!(signature_valid && registered && !revoked);
                        let call = before.calls.iter().find(|c| c.rid == rid).unwrap();
                        prop_assert!(call.jobs.contains(&seed));
                        prop_assert_eq!(call.prover, prover);
                        // the limit that the Worker ran with is the one that
                        // its Calibration gives (P1.4.6)
                        prop_assert_eq!(u128::from(local_micros), u128::from(call.micros) * u128::from(velocity) / 1_000_000);
                    }
                    other => prop_assert!(false, "Valid reply on {other:?}"),
                }
                // The Goal of a proved call has a proof-labelled Verdict.
                let goal = goal_of[&rid];
                prop_assert!(out.state.verdicts.entries.iter().any(|v| v.goal == goal && v.label == Label::ProvedStandalone));
            }

            // Verdicts and Goals only grow, and a Verdict never changes
            // (P1.5.1); at most one Verdict per Goal.
            for v in &before.verdicts.entries {
                prop_assert!(out.state.verdicts.entries.contains(v));
            }
            for g in &before.goals {
                prop_assert!(out.state.goals.contains(g));
            }
            let goals: BTreeSet<u64> = out.state.verdicts.entries.iter().map(|v| v.goal).collect();
            prop_assert_eq!(goals.len(), out.state.verdicts.entries.len());
            // The machine's view of open calls is the one the effects imply.
            let calls: BTreeSet<u64> = out.state.calls.iter().map(|c| c.rid).collect();
            prop_assert_eq!(&calls, &open);

            state = out.state;
        }
    }

    /// Without a failing condition, a run is proved: the fail-safe rule does
    /// not swallow real proofs (P7.1.5).
    #[test]
    fn a_standalone_local_run_is_proved(e in evidence(), goal in 0u64..4, rid in 0u64..6) {
        let mut state = FrontEnd::init(&config(1, 1));
        for input in [
            Input::SessionStart { now: 0, cluster: false },
            Input::GoalAnnounced { now: 0, goal },
            Input::ProverCall { now: 0, rid, goal, prover: 1, pass: Pass::Fast, micros: 5 },
            Input::PrintFacts { now: 0, rid, task_count: 1 },
        ] {
            state = step(state, input).state;
        }
        let out = step(state, Input::LocalRun { now: 1, rid, evidence: e });
        let valid = out.effects.contains(&Effect::Answer { rid, reply: Reply::Valid });
        prop_assert_eq!(valid, standalone(&e));
    }
}

fn run(state: State, inputs: &[Input]) -> (State, Vec<Effect>) {
    let mut state = state;
    let mut effects = Vec::new();
    for input in inputs {
        let out = step(state, *input);
        state = out.state;
        effects.extend(out.effects);
    }
    (state, effects)
}

fn good() -> Evidence {
    Evidence {
        fields: ResultFields {
            answer: ProverAnswer::Valid,
            task_hash: h(7),
            partition: false,
            imports: 0,
            imports_hash: h(0),
            prover: 1,
            version: 1,
            binary_hash: h(1),
        },
        local_task_hash: h(7),
        task_count: 1,
        binary_mismatch: false,
    }
}

/// A call of prover 1 on the Cluster, with a race of three seeds.
fn racing() -> State {
    let (state, effects) = run(
        FrontEnd::init(&config(1, 3)),
        &[
            Input::SessionStart { now: 0, cluster: true },
            Input::GoalAnnounced { now: 0, goal: 5 },
            Input::ProverCall { now: 0, rid: 9, goal: 5, prover: 1, pass: Pass::Later, micros: 4 },
            Input::PrintFacts { now: 0, rid: 9, task_count: 1 },
        ],
    );
    assert_eq!(
        effects,
        vec![
            Effect::Print { rid: 9 },
            Effect::Submit { rid: 9, seed: 0 },
            Effect::Submit { rid: 9, seed: 1 },
            Effect::Submit { rid: 9, seed: 2 },
        ]
    );
    state
}

fn result(seed: u64, evidence: Evidence, authentic: bool) -> Input {
    Input::RemoteResult {
        now: 1, rid: 9, seed, prover: 1, evidence, signature_valid: authentic, registered: true, revoked: false,
        velocity: 1_000_000, local_micros: 4,
    }
}

fn unknown() -> Evidence {
    let mut e = good();
    e.fields.answer = ProverAnswer::Unknown;
    e
}

#[test]
fn a_race_is_won_by_its_first_proof_and_cancels_the_rest() {
    let (state, effects) = run(racing(), &[result(1, unknown(), true), result(2, good(), true)]);
    assert_eq!(
        effects,
        vec![Effect::CancelJob { rid: 9, seed: 0 }, Effect::Answer { rid: 9, reply: Reply::Valid }]
    );
    assert!(state.calls.is_empty());
    assert_eq!(state.verdicts.entries.len(), 1);
    // A Result of the ended race changes nothing.
    let late = step(state.clone(), result(0, good(), true));
    assert_eq!(late.action, TlaAction::Discard);
    assert_eq!(late.state, state);
}

#[test]
fn a_result_that_is_not_authentic_leaves_its_job_open() {
    let before = racing();
    for forged in [
        result(0, good(), false),
        Input::RemoteResult { now: 1, rid: 9, seed: 0, prover: 1, evidence: good(), signature_valid: true, registered: false, revoked: false, velocity: 1_000_000, local_micros: 4 },
        Input::RemoteResult { now: 1, rid: 9, seed: 0, prover: 1, evidence: good(), signature_valid: true, registered: true, revoked: true, velocity: 1_000_000, local_micros: 4 },
    ] {
        let out = step(before.clone(), forged);
        assert_eq!(out.action, TlaAction::Discard);
        assert_eq!(out.state, before);
    }
    // The authentic Result for the same Job is still taken.
    let (_, effects) = run(before, &[result(0, good(), false), result(0, good(), true)]);
    assert!(effects.contains(&Effect::Answer { rid: 9, reply: Reply::Valid }));
}

#[test]
fn an_authentic_result_for_another_prover_or_task_proves_nothing() {
    // Signed and registered, but for a prover that the call did not submit.
    let wrong_prover = Input::RemoteResult {
        now: 1, rid: 9, seed: 0, prover: 2, evidence: good(), signature_valid: true, registered: true, revoked: false,
        velocity: 1_000_000, local_micros: 4,
    };
    // Signed and registered, but run with a limit that the Worker's
    // Calibration does not give for the call's reference limit.
    let wrong_limit = Input::RemoteResult {
        now: 1, rid: 9, seed: 0, prover: 1, evidence: good(), signature_valid: true, registered: true, revoked: false,
        velocity: 2_000_000, local_micros: 4,
    };
    // Signed and registered, but run on another Printed_Goal.
    let mut other_task = good();
    other_task.fields.task_hash = h(8);
    for input in [wrong_prover, wrong_limit, result(0, other_task, true)] {
        let (state, effects) = run(racing(), &[input, result(1, unknown(), true), result(2, unknown(), true)]);
        assert_eq!(effects, vec![Effect::Answer { rid: 9, reply: Reply::Unknown }]);
        assert!(state.verdicts.entries.is_empty());
    }
}

#[test]
fn a_lost_race_gets_its_most_informative_outcome() {
    let mut timeout = good();
    timeout.fields.answer = ProverAnswer::Timeout;
    let mut failed = good();
    failed.fields.answer = ProverAnswer::HighFailure;
    let (state, effects) =
        run(racing(), &[result(0, failed, true), result(1, timeout, true), result(2, failed, true)]);
    // timeout is more informative than failed (R7.2)
    assert_eq!(effects, vec![Effect::Answer { rid: 9, reply: Reply::Timeout }]);
    assert!(state.calls.is_empty() && state.verdicts.entries.is_empty());
}

#[test]
fn a_call_that_the_cluster_did_not_run_runs_locally() {
    let not_run = |seed| Input::JobNotRun { now: 1, rid: 9, seed };
    let (state, effects) = run(racing(), &[not_run(0), not_run(1), not_run(2)]);
    assert_eq!(effects, vec![Effect::StartLocal { rid: 9 }]);
    let (_, effects) = run(state, &[Input::LocalRun { now: 2, rid: 9, evidence: good() }]);
    assert_eq!(effects, vec![Effect::Answer { rid: 9, reply: Reply::Valid }]);
}

#[test]
fn a_race_ends_at_its_deadline_and_the_fallback_moves_calls_home() {
    // deadline = 0 + 2 * 4 + margin 10
    let (state, effects) = run(racing(), &[Input::Tick { now: 17 }]);
    assert!(effects.is_empty() && state.calls.len() == 1);
    let (state, effects) = run(state, &[Input::Tick { now: 18 }]);
    assert_eq!(
        effects,
        vec![
            Effect::CancelJob { rid: 9, seed: 0 },
            Effect::CancelJob { rid: 9, seed: 1 },
            Effect::CancelJob { rid: 9, seed: 2 },
            Effect::Answer { rid: 9, reply: Reply::Timeout },
        ]
    );
    assert!(state.calls.is_empty());

    let (state, effects) = run(racing(), &[Input::ClusterLost { now: 1 }]);
    assert_eq!(effects, vec![Effect::StartLocal { rid: 9 }]);
    assert!(!state.cluster);
    // Results of the lost Cluster no longer count.
    assert_eq!(step(state, result(0, good(), true)).action, TlaAction::Discard);
}

#[test]
fn local_solvers_are_limited_and_queued_calls_start_in_order() {
    let call = |rid| {
        [
            Input::ProverCall { now: 0, rid, goal: 5, prover: 1, pass: Pass::Fast, micros: 4 },
            Input::PrintFacts { now: 0, rid, task_count: 1 },
        ]
    };
    let mut inputs = vec![Input::SessionStart { now: 0, cluster: false }, Input::GoalAnnounced { now: 0, goal: 5 }];
    for rid in [1, 2, 3] {
        inputs.extend(call(rid));
    }
    let (state, effects) = run(FrontEnd::init(&config(1, 1)), &inputs);
    assert_eq!(effects.iter().filter(|e| matches!(e, Effect::StartLocal { .. })).count(), 1);
    assert!(effects.contains(&Effect::StartLocal { rid: 1 }));
    // Cancelling the running call frees the slot for the oldest queued one.
    let (state, effects) = run(state, &[Input::CancelCall { now: 1, rid: 1 }]);
    assert_eq!(
        effects,
        vec![
            Effect::CancelLocal { rid: 1 },
            Effect::StartLocal { rid: 2 },
            Effect::Answer { rid: 1, reply: Reply::NoResult },
        ]
    );
    let (_, effects) = run(state, &[Input::LocalRun { now: 2, rid: 2, evidence: unknown() }]);
    assert_eq!(effects, vec![Effect::StartLocal { rid: 3 }, Effect::Answer { rid: 2, reply: Reply::Unknown }]);
}

// P-ID: P1.6.4
#[test]
fn a_silent_coordinator_is_a_lost_cluster() {
    let mut cfg = config(1, 1);
    cfg.silence_limit = 10;
    cfg.beat_period = 4;
    let mut state = FrontEnd::init(&cfg);
    let mut all = Vec::new();
    for input in [
        Input::SessionStart { now: 0, cluster: true },
        Input::GoalAnnounced { now: 0, goal: 5 },
        Input::ProverCall { now: 0, rid: 9, goal: 5, prover: 1, pass: Pass::Later, micros: 100 },
        Input::PrintFacts { now: 0, rid: 9, task_count: 1 },
        // heartbeats go out while the Coordinator is heard from
        Input::Tick { now: 4 },
        Input::ClusterHeard { now: 8 },
        Input::Tick { now: 18 },
        // silence: the call moves home, the connection is closed
        Input::Tick { now: 19 },
        // and nothing more is sent to the Cluster
        Input::Tick { now: 40 },
    ] {
        let out = step(state, input);
        state = out.state;
        all.push(out.effects);
    }
    assert_eq!(all[3], vec![Effect::Submit { rid: 9, seed: 0 }]);
    assert_eq!(all[4], vec![Effect::Beat]);
    assert_eq!(all[6], vec![Effect::Beat]);
    assert_eq!(all[7], vec![Effect::LeaveCluster, Effect::StartLocal { rid: 9 }]);
    assert_eq!(all[8], vec![]);
    assert!(!state.cluster);
}
