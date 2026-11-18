//! The Coordinator's shell (design C5): it turns what arrives into inputs of
//! the dp-core Coordinator machine and carries out the effects. It decides
//! nothing, and it never looks inside a Result.

use std::{collections::BTreeMap, io, time::Duration};

use dp_core::{
    coord::{self, Config, Effect, Failure, Input, JobId, Offer, State},
    machine::Machine,
};
use dp_formats::cluster::{Job, Message};
use dp_shell::{
    env::{Env, NetEvent, Peer},
    trace::TraceWriter,
};

pub struct Shell<E: Env> {
    pub env: E,
    state: Option<State>,
    provers: BTreeMap<String, u64>,
    /// Job identifiers by Front_End connection and the Job's name.
    ids: BTreeMap<(Peer, String), JobId>,
    /// The Jobs that the machine holds.
    jobs: BTreeMap<JobId, (Peer, Job)>,
    next_job: JobId,
    last_tick: u64,
    tick_period: u64,
    trace: TraceWriter,
}

impl<E: Env> Shell<E> {
    pub fn new(env: E, config: Config, trace: TraceWriter) -> Shell<E> {
        Shell {
            state: Some(coord::Coordinator::init(&config)),
            env,
            provers: BTreeMap::new(),
            ids: BTreeMap::new(),
            jobs: BTreeMap::new(),
            next_job: 0,
            last_tick: 0,
            // Often enough to see a silence or a due heartbeat in time.
            tick_period: (config.beat_period.min(config.silence_limit) / 4).max(1),
            trace,
        }
    }

    pub fn state(&self) -> &State {
        self.state.as_ref().expect("the state is present between steps")
    }

    fn prover(&mut self, name: &str) -> u64 {
        let next = self.provers.len() as u64;
        *self.provers.entry(name.to_string()).or_insert(next)
    }

    /// The Job that the Worker `peer` runs, if it has the name `job_id`.
    fn running(&self, peer: Peer, job_id: &str) -> Option<JobId> {
        let worker = self.state().workers.iter().find(|w| w.peer == peer)?;
        let job = worker.busy?;
        (self.jobs.get(&job.id)?.1.job_id == job_id).then_some(job.id)
    }

    fn step(&mut self, input: Input, message: Option<&Message>) -> io::Result<()> {
        let state = self.state.take().expect("the state is present between steps");
        let described = self.trace.enabled().then(|| format!("{input:?}"));
        let out = coord::step(state, input);
        // The record of a step is written before its effects.
        if let Some(described) = described {
            self.trace.record_step(out.action.name(), self.env.now(), &described, &format!("{:?}", out.effects))?;
        }
        self.state = Some(out.state);
        for effect in out.effects {
            match effect {
                Effect::Assign { worker, job, local_micros, velocity } => {
                    if let Some((_, job)) = self.jobs.get(&job) {
                        let msg = Message::Assign { job: job.clone(), local_micros, velocity };
                        self.env.send(worker, &msg);
                    }
                }
                Effect::CancelAt { worker, job } => {
                    if let Some((_, job)) = self.jobs.get(&job) {
                        self.env.send(worker, &Message::Cancel { job_id: job.job_id.clone() });
                    }
                }
                // The Result that just arrived, unchanged.
                Effect::Forward { front_end, .. } => {
                    if let Some(message) = message {
                        self.env.send(front_end, message);
                    }
                }
                Effect::Fail { front_end, job, reason } => {
                    if let Some((_, job)) = self.jobs.get(&job) {
                        let reason = match reason {
                            Failure::NoEligibleWorker => "no eligible Worker",
                            Failure::WorkerFailed => "the Worker did not run it",
                            Failure::WorkersLost => "the Workers that ran it were lost",
                        };
                        let msg = Message::Failed { job_id: job.job_id.clone(), reason: reason.into() };
                        self.env.send(front_end, &msg);
                    }
                }
                Effect::Beat { peer } => self.env.send(peer, &Message::Heartbeat),
                Effect::Drop { peer } => self.env.close(peer),
                Effect::Forget { front_end, job } => {
                    if let Some((_, job)) = self.jobs.remove(&job) {
                        self.ids.remove(&(front_end, job.job_id));
                    }
                }
            }
        }
        Ok(())
    }

    fn handle(&mut self, event: NetEvent) -> io::Result<()> {
        let now = self.env.now();
        match event {
            NetEvent::Connected(_) => Ok(()),
            NetEvent::Disconnected(peer) => self.step(Input::Disconnected { now, peer }, None),
            NetEvent::Message(peer, message) => {
                let input = match &message {
                    Message::FrontEndHello { .. } => Input::FrontEndHello { now, peer },
                    Message::WorkerHello { offers, .. } => Input::WorkerHello {
                        now,
                        peer,
                        offers: offers
                            .iter()
                            .map(|(prover, velocity)| Offer { prover: self.prover(prover), velocity: *velocity })
                            .collect(),
                    },
                    Message::Submit(job) => {
                        let key = (peer, job.job_id.clone());
                        let id = match self.ids.get(&key) {
                            Some(id) => *id,
                            None => {
                                self.next_job += 1;
                                self.ids.insert(key, self.next_job);
                                self.jobs.insert(self.next_job, (peer, job.clone()));
                                self.next_job
                            }
                        };
                        Input::Submit { now, peer, job: id, prover: self.prover(&job.prover), micros: job.micros }
                    }
                    Message::Cancel { job_id } => match self.ids.get(&(peer, job_id.clone())) {
                        Some(id) => Input::Cancel { now, peer, job: *id },
                        None => Input::Heartbeat { now, peer },
                    },
                    Message::Result(signed) => match self.running(peer, &signed.fields.job_id) {
                        Some(job) => Input::Result { now, peer, job },
                        None => Input::Heartbeat { now, peer },
                    },
                    Message::Failed { job_id, .. } => match self.running(peer, job_id) {
                        Some(job) => Input::Failed { now, peer, job },
                        None => Input::Heartbeat { now, peer },
                    },
                    Message::Idle => Input::WorkerIdle { now, peer },
                    // Anything else only shows that the peer is alive.
                    Message::Heartbeat | Message::Assign { .. } => Input::Heartbeat { now, peer },
                };
                // A Front_End that left before its submission was read
                // leaves a Job that the machine does not hold.
                let submitted = match &input {
                    Input::Submit { job, .. } => Some(*job),
                    _ => None,
                };
                self.step(input, Some(&message))?;
                if let Some(job) = submitted {
                    let held = self.state().queue.iter().any(|j| j.id == job)
                        || self.state().workers.iter().any(|w| w.busy.is_some_and(|j| j.id == job));
                    if !held {
                        if let Some((front_end, job)) = self.jobs.remove(&job) {
                            self.ids.remove(&(front_end, job.job_id));
                        }
                    }
                }
                Ok(())
            }
        }
    }

    /// One turn: at most one event, and a tick when one is due.
    pub fn turn(&mut self, wait: Duration) -> io::Result<()> {
        if let Some(event) = self.env.poll(wait) {
            self.handle(event)?;
        }
        let now = self.env.now();
        if now - self.last_tick >= self.tick_period {
            self.last_tick = now;
            self.step(Input::Tick { now }, None)?;
        }
        Ok(())
    }

    /// Number of Jobs whose bytes the shell holds.
    pub fn held(&self) -> usize {
        self.jobs.len()
    }

    pub fn finish(&mut self) -> io::Result<()> {
        self.trace.finish()
    }
}
