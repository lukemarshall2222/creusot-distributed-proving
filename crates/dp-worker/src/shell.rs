//! The Worker's shell (design C6): it turns what arrives into inputs of the
//! dp-core Worker machine and carries out the effects. It runs only
//! commands of its own prover table and signs what its solver printed.

use std::{collections::BTreeMap, io, time::Duration};

use dp_core::{
    calibration,
    machine::Machine,
    worker::{self, Effect, Input, JobId, State},
};
use dp_formats::{
    cluster::{Job, JobKind, Message, ResultFields, SignedResult},
    hash::{self, Hash},
};
use dp_shell::{
    env::{Env, NetEvent, Peer, RunEnd, RunSpec},
    solver,
};
use ed25519_dalek::SigningKey;

pub struct Config {
    pub id: String,
    pub key: SigningKey,
    /// The Coordinator's address.
    pub address: String,
    /// Command lines by prover, from the Worker's own Why3 configuration.
    pub commands: BTreeMap<String, String>,
    /// why3find's Calibration of each prover on this host, in millionths.
    pub velocities: BTreeMap<String, u64>,
    /// The solver binaries that this Worker's Lock_File pins: prover name
    /// and hash.
    pub pinned: Vec<(String, Hash)>,
    pub machine: worker::Config,
    /// How long to wait before connecting again.
    pub reconnect_micros: u64,
}

/// The Job that the machine runs, with what its Result needs.
struct Current {
    id: JobId,
    job: Job,
    binary_hash: Hash,
    velocity: u64,
    local_micros: u64,
}

pub struct Shell<E: Env> {
    pub env: E,
    config: Config,
    state: Option<State>,
    peer: Option<Peer>,
    connect_at: u64,
    /// The last assignment: the machine's identifier and the Job.
    assigned: Option<(JobId, Job)>,
    current: Option<Current>,
    next_job: JobId,
    /// How often a connection was made.
    pub connections: u64,
}

impl<E: Env> Shell<E> {
    pub fn new(env: E, config: Config) -> Shell<E> {
        Shell {
            state: Some(worker::Worker::init(&config.machine)),
            env,
            config,
            peer: None,
            connect_at: 0,
            assigned: None,
            current: None,
            next_job: 0,
            connections: 0,
        }
    }

    pub fn state(&self) -> &State {
        self.state.as_ref().expect("the state is present between steps")
    }

    fn send(&mut self, msg: &Message) {
        if let Some(peer) = self.peer {
            self.env.send(peer, msg);
        }
    }

    /// Starts the solver of the assigned Job, or says why not.
    fn start(&mut self, id: JobId) -> Result<(), String> {
        let (_, job) = self.assigned.clone().filter(|(assigned, _)| *assigned == id).ok_or("no such assignment")?;
        let command = self.config.commands.get(&job.prover).ok_or("prover not configured")?.clone();
        let program = solver::words(&command).into_iter().next().ok_or("empty prover command")?;
        // The binary is hashed just before it starts and checked against
        // this Worker's Lock_File; the hash goes into the signed Result.
        let (exe, binary_hash) = self.env.solver_binary(&program).map_err(|e| format!("{program}: {e}"))?;
        let name = job.prover.split_once('@').map_or(job.prover.as_str(), |(n, _)| n);
        if !self.config.pinned.iter().any(|(prover, hash)| prover == name && *hash == binary_hash) {
            return Err(format!("the binary of {} does not match the Lock_File", job.prover));
        }
        // The Job chooses nothing of the command line but the seed, and
        // whether the prover prints a proof: for a certificate the program
        // of the Worker's own command line runs with the fixed proof
        // options of dp-cert.
        let command = match job.kind {
            JobKind::Prove => command,
            JobKind::Certify => dp_cert::proof_command(&program),
        };
        let command = match job.seed {
            Some(seed) => solver::with_seed(&command, seed),
            None => command,
        };
        // The local limit is this Worker's own: its Calibration applied to
        // the Job's reference limit (R1.4 criterion 1). The Front_End
        // recomputes it from the signed Result.
        let velocity = self.config.velocities.get(&job.prover).copied().unwrap_or(1_000_000);
        let local_micros = calibration::scale(job.micros, velocity);
        let mut spec = RunSpec::solver(command, job.goal.clone(), job.extension.clone(), local_micros);
        if job.kind == JobKind::Certify {
            spec.output_limit = dp_cert::PROOF_LIMIT as usize;
        }
        self.env.start(id, &exe, &spec).map_err(|e| e.to_string())?;
        self.current = Some(Current { id, job, binary_hash, velocity, local_micros });
        Ok(())
    }

    fn step(&mut self, input: Input, ended: Option<RunEnd>) {
        let state = self.state.take().expect("the state is present between steps");
        let out = worker::step(state, input);
        self.state = Some(out.state);
        for effect in out.effects {
            match effect {
                Effect::Start { job } => {
                    if let Err(reason) = self.start(job) {
                        eprintln!("dp-worker: {}: not run: {reason}", self.config.id);
                        let now = self.env.now();
                        self.step(Input::StartFailed { now, job }, None);
                    }
                }
                Effect::Kill { job } => {
                    self.env.kill(job);
                    if self.current.as_ref().is_some_and(|c| c.id == job) {
                        self.current = None;
                    }
                }
                Effect::SendResult { job } => {
                    let (Some(current), Some(ended)) = (self.current.take(), ended.clone()) else { continue };
                    if current.id != job {
                        continue;
                    }
                    let fields = ResultFields {
                        task_hash: hash::task_hash(&current.job.goal, &current.job.driver, b""),
                        job_id: current.job.job_id,
                        prover: current.job.prover,
                        seed: current.job.seed,
                        binary_hash: current.binary_hash,
                        worker: self.config.id.clone(),
                        status: ended.status,
                        timed_out: ended.timed_out,
                        output: ended.output,
                        // In reference units, as why3find reports times.
                        elapsed_micros: (u128::from(ended.elapsed_micros) * 1_000_000
                            / u128::from(current.velocity.max(1))) as u64,
                        velocity: current.velocity,
                        local_micros: current.local_micros,
                    };
                    let msg = Message::Result(SignedResult::sign(fields, &self.config.key));
                    self.send(&msg);
                }
                Effect::SendFailed { job } => {
                    if let Some((_, assigned)) = self.assigned.as_ref().filter(|(id, _)| *id == job) {
                        let msg = Message::Failed { job_id: assigned.job_id.clone(), reason: "not run".into() };
                        self.send(&msg);
                    }
                }
                Effect::Beat { idle } => self.send(if idle { &Message::Idle } else { &Message::Heartbeat }),
                Effect::Reconnect => self.disconnect(),
            }
        }
    }

    fn disconnect(&mut self) {
        if let Some(peer) = self.peer.take() {
            self.env.close(peer);
        }
        self.connect_at = self.env.now() + self.config.reconnect_micros;
    }

    fn handle(&mut self, event: NetEvent) {
        let now = self.env.now();
        match event {
            NetEvent::Connected(_) => {}
            NetEvent::Disconnected(peer) => {
                if self.peer == Some(peer) {
                    self.peer = None;
                    self.connect_at = now + self.config.reconnect_micros;
                    self.step(Input::Disconnected { now }, None);
                }
            }
            NetEvent::Message(peer, message) => {
                if self.peer != Some(peer) {
                    return;
                }
                match message {
                    Message::Assign { job, .. } => {
                        // The same assignment again keeps its identifier.
                        let id = match &self.assigned {
                            Some((id, assigned)) if *assigned == job => *id,
                            _ => {
                                self.next_job += 1;
                                self.assigned = Some((self.next_job, job));
                                self.next_job
                            }
                        };
                        self.step(Input::Assign { now, job: id }, None);
                    }
                    Message::Cancel { job_id } => match &self.assigned {
                        Some((id, assigned)) if assigned.job_id == job_id => {
                            let id = *id;
                            self.step(Input::Cancel { now, job: id }, None);
                        }
                        _ => self.step(Input::Heartbeat { now }, None),
                    },
                    // Anything else only shows that the Coordinator is alive.
                    _ => self.step(Input::Heartbeat { now }, None),
                }
            }
        }
    }

    /// One turn: connect when due, at most one event, finished solvers, and
    /// a tick.
    pub fn turn(&mut self, wait: Duration) -> io::Result<()> {
        let now = self.env.now();
        if self.peer.is_none() && now >= self.connect_at {
            match self.env.connect(&self.config.address) {
                Ok(peer) => {
                    self.peer = Some(peer);
                    self.connections += 1;
                    let offers = self
                        .config
                        .commands
                        .keys()
                        .map(|p| (p.clone(), self.config.velocities.get(p).copied().unwrap_or(1_000_000)))
                        .collect();
                    self.send(&Message::WorkerHello { worker: self.config.id.clone(), offers });
                    self.step(Input::Connected { now }, None);
                }
                Err(_) => self.connect_at = now + self.config.reconnect_micros,
            }
        }
        if let Some(event) = self.env.poll(wait) {
            self.handle(event);
        }
        for (run, ended) in self.env.finished()? {
            let now = self.env.now();
            self.step(Input::SolverExit { now, job: run }, Some(ended));
        }
        let now = self.env.now();
        self.step(Input::Tick { now }, None);
        Ok(())
    }
}
