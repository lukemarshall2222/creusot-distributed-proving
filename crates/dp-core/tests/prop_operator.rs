// Feature: creusot-distributed-proving, Property 8: Only the operator commands, and each command once
// P-ID: P1.7.1, P1.7.2, P1.7.4, P1.7.6, P1.7.8

//! Tests of the operator commands of the Coordinator machine (R1.7), over
//! generated sequences of inputs:
//!
//! - a command is taken exactly when it carries the operator's signature
//!   and a serial number larger than every one taken before, and the reply
//!   says which;
//! - a command that is not taken changes nothing;
//! - a drained Worker gets no new Job;
//! - a removed or revoked Worker is gone, its connection is closed, and a
//!   revocation is announced to every Front_End;
//! - a Worker that the operator revoked does not join.

use std::collections::BTreeMap;

use dp_core::{
    coord::{step, Config, Coordinator, Effect, Input, Operation, State},
    machine::Machine,
    trace::TlaAction,
};
use dp_testkit::gen::coordinator_ops::input;
use proptest::prelude::*;

fn config(serial: u64) -> Config {
    Config { silence_limit: 1_000_000, beat_period: 1_000_000, retries: 2, serial }
}

proptest! {
    #![proptest_config(dp_testkit::config::cases("prop_operator", 600))]

    #[test]
    fn only_the_operator_commands_and_each_command_once(
        start in 0u64..4,
        inputs in prop::collection::vec(input(), 1..60),
    ) {
        let mut state: State = Coordinator::init(&config(start));
        prop_assert_eq!(state.serial, start);
        // Workers that were drained and have not left since, each with the
        // Job that it ran when it was drained: that one ends normally, and
        // may be sent to it again if it says that it runs nothing.
        let mut drained: BTreeMap<u64, Option<u64>> = BTreeMap::new();
        for input in inputs {
            let before = state.clone();
            let out = step(state, input.clone());
            state = out.state;
            // The serial number of the last command taken never goes back.
            prop_assert!(state.serial >= before.serial);
            // A drained Worker gets no Job, whatever the input was.
            for effect in &out.effects {
                if let Effect::Assign { worker, job, .. } = effect {
                    if let Some(running) = drained.get(worker) {
                        prop_assert_eq!(*running, Some(*job), "a new Job for the drained Worker {}", worker);
                    }
                }
            }
            // A Worker that is gone is drained no more: it may join again.
            drained.retain(|w, _| state.workers.iter().any(|k| k.peer == *w && k.draining));
            // A drained Worker never has another Job than the one it had.
            for k in state.workers.iter().filter(|k| k.draining) {
                if let (Some(running), Some(job)) = (drained.get(&k.peer), k.busy) {
                    prop_assert_eq!(*running, Some(job.id));
                }
            }
            match input {
                Input::Operator { peer, authentic, serial, operation, worker, .. } => {
                    let taken = authentic && serial > before.serial;
                    let holds = out.effects.contains(&Effect::OperatorReply { peer, serial, taken });
                    prop_assert!(holds);
                    let holds = !out.effects.contains(&Effect::OperatorReply { peer, serial, taken: !taken });
                    prop_assert!(holds);
                    if !taken {
                        // Not the operator's, or an old one: nothing changes.
                        prop_assert_eq!(&state, &before);
                        prop_assert_eq!(out.effects.len(), 1);
                        prop_assert_eq!(out.action, TlaAction::Discard);
                        continue;
                    }
                    prop_assert_eq!(state.serial, serial);
                    let connected = worker.filter(|w| before.workers.iter().any(|k| k.peer == *w));
                    match operation {
                        Operation::Drain => {
                            if let Some(w) = connected {
                                prop_assert!(state.workers.iter().any(|k| k.peer == w && k.draining));
                                let running = before.workers.iter().find(|k| k.peer == w).and_then(|k| k.busy).map(|j| j.id);
                                drained.entry(w).or_insert(running);
                            }
                            // Draining closes no connection.
                            let holds = !out.effects.iter().any(|e| matches!(e, Effect::Drop { .. }));
                            prop_assert!(holds);
                        }
                        Operation::Remove | Operation::Revoke => {
                            if let Some(w) = connected {
                                prop_assert!(!state.workers.iter().any(|k| k.peer == w));
                                let holds = out.effects.contains(&Effect::Drop { peer: w });
                                prop_assert!(holds);
                            }
                        }
                    }
                    // A revocation reaches every Front_End, also when the
                    // Worker is not connected; nothing else is announced.
                    let announced: Vec<u64> = out
                        .effects
                        .iter()
                        .filter_map(|e| match e { Effect::Announce { front_end } => Some(*front_end), _ => None })
                        .collect();
                    let front_ends: Vec<u64> = before.front_ends.iter().map(|f| f.peer).collect();
                    if operation == Operation::Revoke {
                        prop_assert_eq!(announced, front_ends);
                    } else {
                        prop_assert!(announced.is_empty());
                    }
                }
                Input::WorkerHello { peer, revoked: true, .. } => {
                    prop_assert!(!state.workers.iter().any(|k| k.peer == peer), "a revoked Worker joined");
                    if !before.front_ends.iter().any(|f| f.peer == peer) {
                        let holds = out.effects.contains(&Effect::Drop { peer });
                        prop_assert!(holds);
                    }
                }
                other => {
                    // No other input takes a command or announces one.
                    prop_assert_eq!(state.serial, before.serial, "{:?}", other);
                    let holds = !out.effects.iter().any(|e| matches!(e, Effect::Announce { .. } | Effect::OperatorReply { .. }));
                    prop_assert!(holds);
                }
            }
        }
    }
}

/// A Coordinator that starts again keeps the serial number that it was
/// started with: a command of an earlier start is not taken again.
#[test]
fn a_command_taken_before_a_restart_is_not_taken_again() {
    let state: State = Coordinator::init(&config(7));
    let command = |serial| Input::Operator { now: 1, peer: 99, authentic: true, serial, operation: Operation::Revoke, worker: None };
    for serial in [0, 6, 7] {
        let out = step(state.clone(), command(serial));
        assert_eq!(out.effects, vec![Effect::OperatorReply { peer: 99, serial, taken: false }]);
        assert_eq!(out.state, state);
    }
    let out = step(state, command(8));
    assert!(out.effects.contains(&Effect::OperatorReply { peer: 99, serial: 8, taken: true }));
    assert_eq!(out.state.serial, 8);
}
