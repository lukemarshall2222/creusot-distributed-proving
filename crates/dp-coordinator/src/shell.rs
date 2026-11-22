//! The Coordinator's shell (design C5): it turns what arrives into inputs of
//! the dp-core Coordinator machine and carries out the effects. It decides
//! nothing, and it never looks inside a Result.

use std::{collections::BTreeMap, io, path::PathBuf, time::Duration};

use dp_core::{
    coord::{self, Config, Effect, Failure, Input, JobId, Offer, State},
    machine::Machine,
};
use dp_formats::cluster::{Job, Message, Operation, SignedCommand, WorkerStatus};
use ed25519_dalek::VerifyingKey;
use dp_formats::sexp::{self, Sexp};
use dp_shell::{
    env::{Env, NetEvent, Peer},
    store,
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
    /// The operator's verification key; without it no command is taken.
    operator: Option<VerifyingKey>,
    /// The names of the connected Workers.
    names: BTreeMap<Peer, String>,
    /// The revocations that were taken, as signed, for Front_Ends that
    /// connect later, and the names that they revoke.
    revocations: Vec<SignedCommand>,
    /// Which start of the Coordinator this is; it grows with every start
    /// when there is a state file.
    incarnation: u64,
    /// Where the revocations, the last serial number and the incarnation
    /// are kept across restarts.
    state_file: Option<PathBuf>,
}

/// What a Coordinator keeps across restarts (R1.7): without it, a restart
/// would let a revoked Worker join again and an old operator command be
/// sent again.
struct Durable {
    incarnation: u64,
    serial: u64,
    revocations: Vec<SignedCommand>,
}

impl Durable {
    fn encode(&self) -> Vec<u8> {
        sexp::print(&Sexp::list([
            Sexp::atom("coordinator-state"),
            Sexp::atom(self.incarnation.to_string()),
            Sexp::atom(self.serial.to_string()),
            Sexp::list(self.revocations.iter().map(|r| Sexp::List(r.encode()))),
        ]))
    }

    fn decode(bytes: &[u8]) -> Option<Durable> {
        let parsed = sexp::parse(bytes, 1 << 24).ok()?;
        let [tag, incarnation, serial, revocations] = parsed.as_list()? else { return None };
        if tag.as_str() != Some("coordinator-state") {
            return None;
        }
        Some(Durable {
            incarnation: incarnation.as_str()?.parse().ok()?,
            serial: serial.as_str()?.parse().ok()?,
            revocations: revocations
                .as_list()?
                .iter()
                .map(|r| SignedCommand::decode(r.as_list()?).ok())
                .collect::<Option<_>>()?,
        })
    }
}

impl<E: Env> Shell<E> {
    /// A Coordinator without a state file: it forgets its revocations when
    /// it stops.
    pub fn new(env: E, config: Config, trace: TraceWriter, operator: Option<VerifyingKey>) -> Shell<E> {
        Shell::with_state(env, config, trace, operator, None).expect("no state file is read")
    }

    /// A Coordinator that keeps its revocations, its last serial number
    /// and its incarnation in `state_file`. A file that cannot be read as
    /// such a state stops the start: starting empty would forget
    /// revocations.
    pub fn with_state(
        env: E,
        mut config: Config,
        trace: TraceWriter,
        operator: Option<VerifyingKey>,
        state_file: Option<PathBuf>,
    ) -> io::Result<Shell<E>> {
        let mut durable = Durable { incarnation: 0, serial: config.serial, revocations: Vec::new() };
        if let Some(path) = &state_file {
            match std::fs::read(path) {
                Ok(bytes) => {
                    durable = Durable::decode(&bytes)
                        .ok_or_else(|| io::Error::other(format!("{}: not a Coordinator state", path.display())))?;
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            // Only revocations that the operator signed count, as when
            // they arrived.
            durable.revocations.retain(|r| {
                r.operation == Operation::Revoke && operator.as_ref().is_some_and(|key| r.verifies(key))
            });
            durable.incarnation += 1;
            store::write_atomic(path, &durable.encode())?;
        }
        config.serial = config.serial.max(durable.serial);
        Ok(Shell {
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
            operator,
            names: BTreeMap::new(),
            revocations: durable.revocations,
            incarnation: durable.incarnation,
            state_file,
        })
    }

    pub fn incarnation(&self) -> u64 {
        self.incarnation
    }

    fn persist(&self) -> io::Result<()> {
        let Some(path) = &self.state_file else { return Ok(()) };
        let durable = Durable {
            incarnation: self.incarnation,
            serial: self.state().serial,
            revocations: self.revocations.clone(),
        };
        store::write_atomic(path, &durable.encode())
    }

    fn revoked(&self, worker: &str) -> bool {
        self.revocations.iter().any(|r| r.worker == worker)
    }

    /// The status report (R1.7 criterion 9), from the machine's state.
    fn status(&self) -> Message {
        let state = self.state();
        let name_of = |id: u64| {
            self.provers.iter().find(|(_, v)| **v == id).map_or_else(|| id.to_string(), |(k, _)| k.clone())
        };
        Message::Status {
            incarnation: self.incarnation,
            workers: state
                .workers
                .iter()
                .map(|w| WorkerStatus {
                    worker: self.names.get(&w.peer).cloned().unwrap_or_default(),
                    job: w.busy.and_then(|j| self.jobs.get(&j.id)).map(|(_, job)| job.job_id.clone()),
                    draining: w.draining,
                    provers: w.offers.iter().map(|o| name_of(o.prover)).collect(),
                })
                .collect(),
            queued: state.queue.len() as u64,
            front_ends: state.front_ends.len() as u64,
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
                Effect::Drop { peer } => {
                    self.names.remove(&peer);
                    self.env.close(peer);
                }
                Effect::Forget { front_end, job } => {
                    if let Some((_, job)) = self.jobs.remove(&job) {
                        self.ids.remove(&(front_end, job.job_id));
                    }
                }
                Effect::OperatorReply { peer, serial, taken } => {
                    // A revocation that was taken is kept, as signed.
                    if let (true, Some(Message::Operator(command))) = (taken, message) {
                        if command.operation == Operation::Revoke {
                            self.revocations.push(command.clone());
                        }
                    }
                    // The command is on disk before the operator hears
                    // that it was taken.
                    if taken {
                        self.persist()?;
                    }
                    self.env.send(peer, &Message::OperatorResult { serial, taken });
                }
                Effect::Announce { front_end } => {
                    if let Some(Message::Operator(command)) = message {
                        self.env.send(front_end, &Message::Revocation(command.clone()));
                    }
                }
                Effect::Report { peer } => {
                    let report = self.status();
                    self.env.send(peer, &report);
                }
            }
        }
        Ok(())
    }

    fn handle(&mut self, event: NetEvent) -> io::Result<()> {
        let now = self.env.now();
        match event {
            NetEvent::Connected(_) => Ok(()),
            NetEvent::Disconnected(peer) => {
                self.names.remove(&peer);
                self.step(Input::Disconnected { now, peer }, None)
            }
            NetEvent::Message(peer, message) => {
                let input = match &message {
                    Message::FrontEndHello { .. } => Input::FrontEndHello { now, peer },
                    Message::WorkerHello { worker, offers } => Input::WorkerHello {
                        now,
                        peer,
                        revoked: {
                            self.names.insert(peer, worker.clone());
                            self.revoked(worker)
                        },
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
                    Message::StatusRequest => Input::Status { now, peer },
                    Message::Operator(command) => Input::Operator {
                        now,
                        peer,
                        authentic: self.operator.as_ref().is_some_and(|key| command.verifies(key)),
                        serial: command.serial,
                        operation: match command.operation {
                            Operation::Drain => coord::Operation::Drain,
                            Operation::Remove => coord::Operation::Remove,
                            Operation::Revoke => coord::Operation::Revoke,
                        },
                        // The connection of the Worker with that name, among
                        // the Workers that the machine holds.
                        worker: self
                            .state()
                            .workers
                            .iter()
                            .map(|w| w.peer)
                            .find(|peer| self.names.get(peer) == Some(&command.worker)),
                    },
                    // Anything else only shows that the peer is alive.
                    _ => Input::Heartbeat { now, peer },
                };
                // A Front_End that left before its submission was read
                // leaves a Job that the machine does not hold.
                let submitted = match &input {
                    Input::Submit { job, .. } => Some(*job),
                    _ => None,
                };
                let joined = matches!(input, Input::FrontEndHello { .. });
                let hello = matches!(input, Input::FrontEndHello { .. } | Input::WorkerHello { .. });
                self.step(input, Some(&message))?;
                // Whoever joined learns which start of the Coordinator this is.
                let state = self.state();
                if hello && (state.front_ends.iter().any(|f| f.peer == peer) || state.workers.iter().any(|w| w.peer == peer)) {
                    self.env.send(peer, &Message::Welcome { incarnation: self.incarnation });
                }
                // A Front_End that joins learns of the revocations so far.
                if joined && self.state().front_ends.iter().any(|f| f.peer == peer) {
                    for revocation in self.revocations.clone() {
                        self.env.send(peer, &Message::Revocation(revocation));
                    }
                }
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
