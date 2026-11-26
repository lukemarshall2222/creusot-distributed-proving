//! An adversarial Coordinator (R1.3, P1.3.3; design: Testing Strategy).
//!
//! It stands where the Coordinator stands, between the Front_End and the
//! Workers, and does what a Coordinator that is not to be trusted can do
//! with the messages of the grammar: it assigns a Job honestly, or with
//! the Printed_Goal of another Job, or with another time limit; it answers
//! a Job itself with a Result that it signs with a key of its own, under
//! the name of a registered Worker or of a Worker that it invented; it
//! forwards a Worker's Result unchanged, twice, with a changed answer or
//! under another Job; it replays old Results; it reports Jobs as failed;
//! and it sends revocations that the operator did not sign. It keeps
//! sending heartbeats, so the Front_End has no reason to leave it.
//!
//! What it cannot do is sign as a registered Worker or as the operator.

use std::{collections::BTreeMap, time::Duration};

use dp_core::calibration;
use dp_formats::{
    cluster::{Job, Message, Operation, ResultFields, SignedCommand, SignedResult},
    hash,
};
use dp_shell::env::{Env, NetEvent, Peer};
use dp_testkit::sim::{SimEnv, BINARY_HASH};
use ed25519_dalek::SigningKey;

use crate::{BEAT, WORKERS};

pub struct Adversary {
    env: SimEnv,
    rng: u64,
    front_end: Option<Peer>,
    /// The Workers that said hello: peer, name, offers.
    workers: Vec<(Peer, String, Vec<(String, u64)>)>,
    /// Every Job that the Front_End submitted.
    jobs: Vec<Job>,
    /// Every Result that a Worker sent.
    results: Vec<SignedResult>,
    last_beat: u64,
    /// How often it did each thing.
    pub did: BTreeMap<&'static str, u64>,
}

impl Adversary {
    pub fn new(env: SimEnv, seed: u64) -> Adversary {
        Adversary {
            env,
            rng: seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1,
            front_end: None,
            workers: Vec::new(),
            jobs: Vec::new(),
            results: Vec::new(),
            last_beat: 0,
            did: BTreeMap::new(),
        }
    }

    fn random(&mut self, below: u64) -> u64 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d) % below.max(1)
    }

    fn count(&mut self, what: &'static str) {
        *self.did.entry(what).or_insert(0) += 1;
    }

    fn to_front_end(&mut self, msg: &Message) {
        if let Some(peer) = self.front_end {
            self.env.send(peer, msg);
        }
    }

    /// A key that is not the key of any registered Worker, nor the
    /// operator's.
    fn own_key() -> SigningKey {
        SigningKey::from_bytes(&[0xad; 32])
    }

    /// A Result that claims a proof of the Job, for exactly the Task_Hash
    /// that the Front_End expects, under the name `worker`.
    fn invented(job: &Job, worker: &str) -> SignedResult {
        let fields = ResultFields {
            job_id: job.job_id.clone(),
            task_hash: hash::task_hash(&job.goal, &job.driver, b""),
            prover: job.prover.clone(),
            seed: job.seed,
            binary_hash: BINARY_HASH,
            worker: worker.to_string(),
            status: Some(0),
            timed_out: false,
            output: b"unsat\n".to_vec(),
            elapsed_micros: 1_000,
            velocity: 1_000_000,
            local_micros: job.micros,
            imports_hash: hash::imports_hash(&[]),
        };
        SignedResult::sign(fields, &Adversary::own_key())
    }

    fn assign(&mut self, job: Job, limit_factor: u64) {
        if self.workers.is_empty() {
            return;
        }
        let pick = self.random(self.workers.len() as u64) as usize;
        let (peer, _, offers) = &self.workers[pick];
        let velocity = offers.iter().find(|(p, _)| *p == job.prover).map_or(1_000_000, |(_, v)| *v);
        let local_micros = calibration::scale(job.micros, velocity) * limit_factor;
        let peer = *peer;
        self.env.send(peer, &Message::Assign { job, local_micros, velocity });
    }

    fn submitted(&mut self, job: Job) {
        self.jobs.push(job.clone());
        match self.random(10) {
            0..=3 => {
                self.count("assigned honestly");
                self.assign(job, 1);
            }
            4 => {
                // The Job's identifier with the Printed_Goal of another Job.
                let other = self.random(self.jobs.len() as u64) as usize;
                let goal = self.jobs[other].goal.clone();
                self.count("assigned with another goal");
                self.assign(Job { goal, ..job }, 1);
            }
            5 => {
                self.count("assigned with another time limit");
                self.assign(job, 3);
            }
            6 => {
                // The name of a registered Worker, the adversary's key.
                self.count("answered as a registered Worker");
                let forged = Adversary::invented(&job, &format!("w{WORKERS}"));
                self.to_front_end(&Message::Result(forged));
            }
            7 => {
                // A Worker that it registered with itself only.
                self.count("answered as an invented Worker");
                let forged = Adversary::invented(&job, "w666");
                self.to_front_end(&Message::Result(forged));
            }
            8 => {
                self.count("reported as failed");
                self.to_front_end(&Message::Failed { job_id: job.job_id, reason: "no".into() });
            }
            _ => self.count("withheld"),
        }
    }

    fn answered(&mut self, signed: SignedResult) {
        self.results.push(signed.clone());
        match self.random(8) {
            0..=2 => {
                self.count("forwarded");
                self.to_front_end(&Message::Result(signed));
            }
            3 => {
                self.count("forwarded twice");
                self.to_front_end(&Message::Result(signed.clone()));
                self.to_front_end(&Message::Result(signed));
            }
            4 => {
                // Under the tag of the Worker, the answer that suits.
                self.count("forwarded with a changed answer");
                let mut changed = signed;
                changed.fields.output = b"unsat\n".to_vec();
                changed.fields.status = Some(0);
                changed.fields.timed_out = false;
                self.to_front_end(&Message::Result(changed));
            }
            5 => {
                // An authentic Result, under another Job.
                self.count("forwarded under another Job");
                let other = self.random(self.jobs.len() as u64) as usize;
                let mut moved = signed;
                moved.fields.job_id = self.jobs[other].job_id.clone();
                moved.fields.task_hash = hash::task_hash(&self.jobs[other].goal, &self.jobs[other].driver, b"");
                self.to_front_end(&Message::Result(moved));
            }
            6 => {
                // Unchanged, but for the Job of another call: the tag is
                // the Worker's, the Job is not the one it ran.
                self.count("replayed");
                let old = self.random(self.results.len() as u64) as usize;
                let old = self.results[old].clone();
                self.to_front_end(&Message::Result(old));
                self.to_front_end(&Message::Result(signed));
            }
            _ => self.count("dropped"),
        }
    }

    pub fn turn(&mut self) {
        while let Some(event) = self.env.poll(Duration::ZERO) {
            match event {
                NetEvent::Connected(_) => {}
                NetEvent::Disconnected(peer) => {
                    self.workers.retain(|(p, _, _)| *p != peer);
                    if self.front_end == Some(peer) {
                        self.front_end = None;
                    }
                }
                NetEvent::Message(peer, msg) => match msg {
                    Message::FrontEndHello { .. } => {
                        self.front_end = Some(peer);
                        let incarnation = self.random(1 << 20);
                        self.env.send(peer, &Message::Welcome { incarnation });
                    }
                    Message::WorkerHello { worker, offers } => {
                        self.workers.push((peer, worker, offers));
                        self.env.send(peer, &Message::Welcome { incarnation: 1 });
                    }
                    Message::Submit(job) => self.submitted(job),
                    Message::Result(signed) => self.answered(signed),
                    _ => {}
                },
            }
        }
        let now = self.env.now();
        if now >= self.last_beat + BEAT / 2 {
            self.last_beat = now;
            let peers: Vec<Peer> = self.workers.iter().map(|(p, _, _)| *p).chain(self.front_end).collect();
            for peer in peers {
                self.env.send(peer, &Message::Heartbeat);
            }
            // Now and then: a revocation of an honest Worker that the
            // operator did not sign, and what else the grammar has.
            match self.random(12) {
                0 => {
                    self.count("sent a revocation of its own");
                    let worker = format!("w{}", WORKERS + self.random(2) as u32);
                    let serial = self.random(100);
                    let command = SignedCommand::sign(serial, Operation::Revoke, &worker, &Adversary::own_key());
                    self.to_front_end(&Message::Revocation(command));
                }
                1 => {
                    self.count("sent a status report");
                    self.to_front_end(&Message::Status { incarnation: 7, workers: Vec::new(), queued: 0, front_ends: 9 });
                }
                2 => {
                    let incarnation = self.random(1 << 20);
                    self.to_front_end(&Message::Welcome { incarnation });
                }
                3 => self.to_front_end(&Message::OperatorResult { serial: 1, taken: true }),
                _ => {}
            }
        }
    }
}
