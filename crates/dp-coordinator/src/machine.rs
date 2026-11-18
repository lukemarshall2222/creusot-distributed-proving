//! The Coordinator's decisions: queueing, assignment, cancellation and the
//! handling of lost Workers and Front_Ends, as a pure state machine.
//!
//! This is the logic of the first distributed slice. The design places it in
//! dp-core (`coord.rs`), verified and model-checked; it lives here until it
//! is written in the subset that Creusot verifies (task 15). The Coordinator
//! is untrusted either way: it affects liveness, never a label.

use std::collections::{BTreeMap, VecDeque};

use dp_formats::cluster::{Job, Message, SignedResult};

/// Identifies a connection.
pub type Peer = u64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Connected(Peer),
    Message(Peer, Message),
    Disconnected(Peer),
}

/// A message to send to a peer.
pub type Effect = (Peer, Message);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Worker {
    provers: Vec<String>,
    /// The Job it runs, with the Front_End that submitted it.
    busy: Option<(String, Peer)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    front_ends: BTreeMap<Peer, String>,
    workers: BTreeMap<Peer, Worker>,
    /// Jobs waiting for a Worker, oldest first.
    queue: VecDeque<(Peer, Job)>,
}

impl State {
    /// Assigns queued Jobs to idle Workers that offer their prover. A Job
    /// goes to the eligible Worker with the smallest peer number, so the
    /// choice does not depend on map iteration order of anything else.
    fn assign(&mut self, effects: &mut Vec<Effect>) {
        let mut waiting = VecDeque::new();
        while let Some((front_end, job)) = self.queue.pop_front() {
            let offered = self.workers.values().any(|w| w.provers.contains(&job.prover));
            let idle = self
                .workers
                .iter_mut()
                .find(|(_, w)| w.busy.is_none() && w.provers.contains(&job.prover));
            match idle {
                Some((peer, worker)) => {
                    worker.busy = Some((job.job_id.clone(), front_end));
                    effects.push((*peer, Message::Assign(job)));
                }
                // No connected Worker offers this prover (R1.1 criterion 7).
                None if !offered => effects.push((
                    front_end,
                    Message::Failed { job_id: job.job_id, reason: "no eligible Worker".into() },
                )),
                None => waiting.push_back((front_end, job)),
            }
        }
        self.queue = waiting;
    }

    pub fn step(mut self, input: Input) -> (State, Vec<Effect>) {
        let mut effects = Vec::new();
        match input {
            Input::Connected(_) => {}
            Input::Message(peer, Message::FrontEndHello { session }) => {
                self.front_ends.insert(peer, session);
            }
            Input::Message(peer, Message::WorkerHello { provers, .. }) => {
                self.workers.insert(peer, Worker { provers, busy: None });
            }
            Input::Message(peer, Message::Submit(job)) if self.front_ends.contains_key(&peer) => {
                self.queue.push_back((peer, job));
            }
            Input::Message(peer, Message::Cancel { job_id }) if self.front_ends.contains_key(&peer) => {
                self.queue.retain(|(fe, job)| !(*fe == peer && job.job_id == job_id));
                for (worker_peer, worker) in &self.workers {
                    if worker.busy.as_ref() == Some(&(job_id.clone(), peer)) {
                        effects.push((*worker_peer, Message::Cancel { job_id: job_id.clone() }));
                    }
                }
            }
            // A Result is forwarded unchanged to the Front_End whose Job the
            // Worker runs. The Coordinator does not look inside it.
            Input::Message(peer, Message::Result(result)) => {
                if let Some(front_end) = self.finish(peer, &result.fields.job_id) {
                    effects.push((front_end, Message::Result(SignedResult { ..result })));
                }
            }
            Input::Message(peer, Message::Failed { job_id, reason }) => {
                if let Some(front_end) = self.finish(peer, &job_id) {
                    effects.push((front_end, Message::Failed { job_id, reason }));
                }
            }
            // Anything else is not part of the protocol for that peer.
            Input::Message(_, _) => {}
            Input::Disconnected(peer) => {
                if let Some(worker) = self.workers.remove(&peer) {
                    if let Some((job_id, front_end)) = worker.busy {
                        effects.push((
                            front_end,
                            Message::Failed { job_id, reason: "the Worker was lost".into() },
                        ));
                    }
                }
                if self.front_ends.remove(&peer).is_some() {
                    self.queue.retain(|(fe, _)| *fe != peer);
                    for (worker_peer, worker) in &mut self.workers {
                        if let Some((job_id, fe)) = &worker.busy {
                            if *fe == peer {
                                effects.push((*worker_peer, Message::Cancel { job_id: job_id.clone() }));
                            }
                        }
                    }
                }
            }
        }
        self.assign(&mut effects);
        (self, effects)
    }

    /// Marks the Worker `peer` idle if it runs `job_id`; returns the Front_End
    /// of that Job.
    fn finish(&mut self, peer: Peer, job_id: &str) -> Option<Peer> {
        let worker = self.workers.get_mut(&peer)?;
        match &worker.busy {
            Some((running, front_end)) if running == job_id => {
                let front_end = *front_end;
                worker.busy = None;
                self.front_ends.contains_key(&front_end).then_some(front_end)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, prover: &str) -> Job {
        Job { job_id: id.into(), prover: prover.into(), driver: vec![], extension: ".smt2".into(), micros: 1, seed: None, goal: vec![] }
    }

    fn run(inputs: Vec<Input>) -> (State, Vec<Effect>) {
        let mut state = State::default();
        let mut all = Vec::new();
        for input in inputs {
            let (next, effects) = state.step(input);
            state = next;
            all.extend(effects);
        }
        (state, all)
    }

    fn hello_fe(p: Peer) -> Input {
        Input::Message(p, Message::FrontEndHello { session: "s".into() })
    }

    fn hello_w(p: Peer, provers: &[&str]) -> Input {
        Input::Message(p, Message::WorkerHello { worker: format!("w{p}"), provers: provers.iter().map(|s| s.to_string()).collect() })
    }

    #[test]
    fn a_job_goes_to_one_eligible_idle_worker() {
        let (_, effects) = run(vec![hello_fe(1), hello_w(2, &["z3@"]), hello_w(3, &["cvc5@"]), Input::Message(1, Message::Submit(job("j1", "cvc5@")))]);
        assert_eq!(effects, vec![(3, Message::Assign(job("j1", "cvc5@")))]);
    }

    #[test]
    fn jobs_queue_while_workers_are_busy_and_run_in_order() {
        let (state, effects) = run(vec![
            hello_fe(1),
            hello_w(2, &["z3@"]),
            Input::Message(1, Message::Submit(job("j1", "z3@"))),
            Input::Message(1, Message::Submit(job("j2", "z3@"))),
            Input::Message(1, Message::Submit(job("j3", "z3@"))),
        ]);
        assert_eq!(effects, vec![(2, Message::Assign(job("j1", "z3@")))]);
        let (_, effects) = state.step(Input::Message(2, Message::Failed { job_id: "j1".into(), reason: "x".into() }));
        assert_eq!(
            effects,
            vec![
                (1, Message::Failed { job_id: "j1".into(), reason: "x".into() }),
                (2, Message::Assign(job("j2", "z3@"))),
            ]
        );
    }

    #[test]
    fn a_prover_that_no_worker_offers_fails_at_once() {
        let (_, effects) = run(vec![hello_fe(1), hello_w(2, &["z3@"]), Input::Message(1, Message::Submit(job("j1", "cvc5@")))]);
        assert_eq!(effects, vec![(1, Message::Failed { job_id: "j1".into(), reason: "no eligible Worker".into() })]);
    }

    #[test]
    fn a_lost_worker_fails_its_job_and_a_lost_front_end_cancels_its_jobs() {
        let (state, _) = run(vec![hello_fe(1), hello_w(2, &["z3@"]), Input::Message(1, Message::Submit(job("j1", "z3@")))]);
        let (_, effects) = state.clone().step(Input::Disconnected(2));
        assert_eq!(effects, vec![(1, Message::Failed { job_id: "j1".into(), reason: "the Worker was lost".into() })]);
        let (_, effects) = state.step(Input::Disconnected(1));
        assert_eq!(effects, vec![(2, Message::Cancel { job_id: "j1".into() })]);
    }

    #[test]
    fn cancel_reaches_the_worker_and_submissions_from_non_front_ends_are_ignored() {
        let (_, effects) = run(vec![
            hello_fe(1),
            hello_w(2, &["z3@"]),
            Input::Message(2, Message::Submit(job("evil", "z3@"))),
            Input::Message(1, Message::Submit(job("j1", "z3@"))),
            Input::Message(1, Message::Cancel { job_id: "j1".into() }),
        ]);
        assert_eq!(effects, vec![(2, Message::Assign(job("j1", "z3@"))), (2, Message::Cancel { job_id: "j1".into() })]);
    }

    #[test]
    fn a_result_is_forwarded_only_from_the_worker_that_runs_the_job() {
        use dp_formats::{cluster::ResultFields, hash::Hash};
        let result = |job_id: &str| {
            Message::Result(SignedResult {
                fields: ResultFields {
                    job_id: job_id.into(),
                    task_hash: Hash([0; 32]),
                    prover: "z3@".into(),
                    seed: None,
                    binary_hash: Hash([0; 32]),
                    worker: "w".into(),
                    status: Some(0),
                    timed_out: false,
                    output: vec![],
                    elapsed_micros: 1,
                },
                tag: [0; 64],
            })
        };
        let (state, _) = run(vec![hello_fe(1), hello_w(2, &["z3@"]), hello_w(3, &["z3@"]), Input::Message(1, Message::Submit(job("j1", "z3@")))]);
        // worker 3 does not run j1; worker 2 does
        assert_eq!(state.clone().step(Input::Message(3, result("j1"))).1, vec![]);
        assert_eq!(state.clone().step(Input::Message(2, result("other"))).1, vec![]);
        assert_eq!(state.step(Input::Message(2, result("j1"))).1, vec![(1, result("j1"))]);
    }
}
