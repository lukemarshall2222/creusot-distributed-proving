//! A whole Cluster on the simulated network: the real shells of the
//! Coordinator, the Workers and the Front_End (`dp-frontend`'s session),
//! with a scripted why3find and scripted solvers.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    rc::Rc,
    time::Duration,
};

use dp_coordinator::shell::Shell as CoordinatorShell;
use dp_core::{coord, worker};
use dp_formats::{
    cluster::{Message, Operation, Registry, SignedCommand},
    dispatch::{DriverAnswer, FromWhy3find, ProverInfo, ProverResult, StrategyStep, ToWhy3find},
    hash,
    lockfile::{current_platform, LockFile},
};
use dp_frontend::session::{self, Session};
use dp_shell::{
    env::{Env, NetEvent},
    trace::TraceWriter,
    why3find::{BridgeError, Poll, Printed, Why3find},
};
use dp_testkit::sim::{script, Faults, Node, SimEnv, World, BINARY_HASH};
use dp_worker::shell::{Config as WorkerConfig, Shell as WorkerShell};
use ed25519_dalek::SigningKey;

pub const ADDRESS: &str = "coordinator";
/// The provers of the simulated project.
pub const PROVER: &str = "stub@";
/// The second prover is also the proof producer of certification.
pub const OTHER_PROVER: &str = "cvc5@";

/// Times of the simulated Cluster, in microseconds.
pub const BEAT: u64 = 20_000;
pub const SILENCE: u64 = 100_000;
pub const MARGIN: u64 = 50_000;

pub const COORDINATOR: Node = 0;
pub const FRONT_END: Node = 1;
/// The operator's machine.
pub const OPERATOR: Node = 2;
/// Workers are the nodes from this number on.
pub const WORKERS: Node = 10;

/// One scripted prover call of why3find.
#[derive(Clone, Debug)]
pub struct Call {
    /// When why3find makes the call.
    pub at: u64,
    /// The Goal that the call is on.
    pub goal: u64,
    pub prover: String,
    /// The print of the Goal for that prover, which is also the solver's
    /// script.
    pub text: Vec<u8>,
    /// The reference time limit.
    pub micros: u64,
}

/// What why3find was told about a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    Valid,
    Unknown,
    Timeout,
    Failed,
}

/// The answer that the scripted driver reads from a run.
fn driver(status: u32, output: &[u8]) -> DriverAnswer {
    match (status, output) {
        (0, b"unsat\n") => DriverAnswer::Valid,
        (0, b"unknown\n") => DriverAnswer::Unknown,
        (0, b"sat\n") => DriverAnswer::Invalid,
        _ => DriverAnswer::Failure,
    }
}

/// The reply that an honest run of the call gives.
pub fn truth(call: &Call) -> Reply {
    let behaviour = script(&call.text);
    if behaviour.micros > call.micros {
        return Reply::Timeout;
    }
    match driver(behaviour.status, &behaviour.output) {
        DriverAnswer::Valid => Reply::Valid,
        DriverAnswer::Failure | DriverAnswer::HighFailure => Reply::Failed,
        _ => Reply::Unknown,
    }
}

/// why3find in dispatch mode, from a script: it announces Goals, makes the
/// prover calls at their times, prints a Goal as the script says, and reads
/// answers as the scripted driver.
pub struct Scripted {
    world: Rc<RefCell<World>>,
    script: Vec<Call>,
    next: usize,
    announced: BTreeSet<u64>,
    out: VecDeque<FromWhy3find>,
    questions: u64,
    /// The Calibration of every prover on the Front_End's machine.
    velocity: u64,
    /// The reply to each call, by its index, with the time of the reply.
    pub answers: BTreeMap<usize, (Reply, u64)>,
    /// Prints of Goals for provers that the script makes no call with: a
    /// Portfolio member may be such a prover.
    prints: BTreeMap<(u64, String), Vec<u8>>,
    /// The strategy step of every call.
    step: StrategyStep,
}

impl Scripted {
    fn release(&mut self) {
        let now = self.world.borrow().now();
        while self.next < self.script.len() && self.script[self.next].at <= now {
            let call = &self.script[self.next];
            if self.announced.insert(call.goal) {
                self.out.push_back(FromWhy3find::Goal {
                    gid: format!("g{}", call.goal),
                    file: "sim.mlw".into(),
                    theory: "T".into(),
                    name: format!("G{}", call.goal),
                    path: Vec::new(),
                    task: Vec::new(),
                });
            }
            self.out.push_back(FromWhy3find::Prove {
                rid: format!("r{}", self.next),
                gid: format!("g{}", call.goal),
                step: self.step,
                prover: call.prover.clone(),
                micros: call.micros,
            });
            self.next += 1;
        }
    }

    /// Makes every call a call of the first sequential pass.
    pub fn first_pass(&mut self) {
        self.step = StrategyStep::Fast;
    }

    /// Whether a message waits for the Front_End.
    pub fn pending(&mut self) -> bool {
        self.release();
        !self.out.is_empty()
    }
}

impl Why3find for Scripted {
    fn send(&mut self, msg: &ToWhy3find) -> Result<(), BridgeError> {
        if let ToWhy3find::Answer { rid, result } = msg {
            let index: usize = rid[1..].parse().expect("a scripted call");
            let reply = match result {
                ProverResult::Valid { .. } => Reply::Valid,
                ProverResult::Unknown { .. } => Reply::Unknown,
                ProverResult::Timeout { .. } => Reply::Timeout,
                ProverResult::Failed | ProverResult::NoResult => Reply::Failed,
            };
            let now = self.world.borrow().now();
            assert!(self.answers.insert(index, (reply, now)).is_none(), "call {index} was answered twice");
        }
        Ok(())
    }

    fn poll(&mut self, _timeout: Duration) -> Result<Poll, BridgeError> {
        self.release();
        Ok(self.out.pop_front().map_or(Poll::Idle, Poll::Message))
    }

    fn print(&mut self, gid: &str, prover: &str, _transformations: &[String]) -> Result<Printed, BridgeError> {
        let goal: u64 = gid[1..].parse().expect("a scripted Goal");
        if let Some(text) = self.prints.get(&(goal, prover.to_string())) {
            return Ok(Printed { tasks: 1, bytes: text.clone() });
        }
        let call = self
            .script
            .iter()
            .find(|c| c.goal == goal && c.prover == prover)
            .ok_or_else(|| BridgeError::Refused("no such print".into()))?;
        Ok(Printed { tasks: 1, bytes: call.text.clone() })
    }

    fn provers(&mut self) -> Result<Vec<ProverInfo>, BridgeError> {
        Ok([PROVER, OTHER_PROVER]
            .iter()
            .map(|p| ProverInfo {
                prover: p.to_string(),
                digest: format!("driver-of-{p}"),
                driver: "stub".into(),
                command: format!("{} %f", p.trim_end_matches('@')),
                extension: ".smt2".into(),
            })
            .collect())
    }

    fn calibration(&mut self, _prover: &str) -> Result<u64, BridgeError> {
        Ok(self.velocity)
    }

    fn ask_answer(&mut self, _prover: &str, status: u32, output: &[u8]) -> Result<String, BridgeError> {
        self.questions += 1;
        let qid = format!("q{}", self.questions);
        self.out.push_back(FromWhy3find::AnswerRead { qid: qid.clone(), answer: driver(status, output) });
        Ok(qid)
    }
}

/// A Lock_File that pins the simulated solver binaries.
fn lock(certify: bool) -> (LockFile, hash::Hash) {
    let mut text = String::new();
    // With certification the solvers are pinned, but not as
    // Unpatched_Solvers, and the checker and its signature are pinned.
    let kind = if certify { "sharing-solver" } else { "unpatched-solver" };
    if certify {
        for (name, kind) in [("ethos", "checker"), ("cpc-signature", "checker-signature")] {
            text.push_str(&format!(
                "[[tool]]\nname = \"{name}\"\nversion = \"1\"\nkind = \"{kind}\"\n\n\
                 [[tool.platform]]\nplatform = \"{}\"\nhash = \"sha256:{}\"\n\n",
                current_platform(),
                BINARY_HASH.to_hex()
            ));
        }
    }
    for prover in [PROVER, OTHER_PROVER] {
        let name = prover.trim_end_matches('@');
        text.push_str(&format!(
            "[[tool]]\nname = \"{name}\"\nversion = \"1\"\nkind = \"{kind}\"\nprover = \"{name}\"\n\n\
             [[tool.platform]]\nplatform = \"{}\"\nhash = \"sha256:{}\"\nupstream = {{ tag = \"v1\", rebuild_hashes = [] }}\n\n",
            current_platform(),
            BINARY_HASH.to_hex()
        ));
    }
    (LockFile::parse(&text).expect("a well-formed Lock_File"), hash::sha256(text.as_bytes()))
}

/// How fast the machines of a Cluster are: local time over reference time,
/// in millionths.
#[derive(Clone, Debug)]
pub struct Speeds {
    pub front_end: u64,
    /// The speed of each Worker, in order; the last one repeats.
    pub workers: Vec<u64>,
}

impl Speeds {
    pub fn reference() -> Speeds {
        Speeds { front_end: 1_000_000, workers: vec![1_000_000] }
    }
}

/// What a simulated Session is configured with beyond its script.
#[derive(Clone, Debug)]
pub struct Setup {
    /// Jobs of different seeds per call, when there is no Portfolio.
    pub seeds: u64,
    pub speeds: Speeds,
    /// The Portfolio: provers that race on every call on the Cluster.
    pub portfolio: Vec<String>,
    /// The first pass goes to the Cluster, and every scripted call is of
    /// the first pass.
    pub first_pass_on_cluster: bool,
    /// Prints of Goals for the other prover, by Goal.
    pub other_prints: BTreeMap<u64, Vec<u8>>,
    /// Certification is on, and no solver is an Unpatched_Solver: every
    /// reported proof needs a certificate.
    pub certify: bool,
    /// The call's own prover also runs on the Front_End's machine as a
    /// member of every race.
    pub local_member: bool,
}

impl Setup {
    pub fn plain() -> Setup {
        Setup {
            seeds: 1,
            speeds: Speeds::reference(),
            portfolio: Vec::new(),
            first_pass_on_cluster: false,
            other_prints: BTreeMap::new(),
            certify: false,
            local_member: false,
        }
    }
}

/// A Cluster with one Coordinator, some Workers and one Front_End.
pub struct Cluster {
    pub world: Rc<RefCell<World>>,
    pub coordinator: Option<CoordinatorShell<SimEnv>>,
    pub workers: BTreeMap<Node, WorkerShell<SimEnv>>,
    pub front_end: Session<SimEnv, Scripted>,
    keys: BTreeMap<Node, SigningKey>,
    speeds: Speeds,
    /// The project directory of the simulated Session.
    pub dir: std::path::PathBuf,
}

impl Drop for Cluster {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A Coordinator that keeps its state in `state`, if one is given.
fn coordinator_shell(world: &Rc<RefCell<World>>, state: Option<std::path::PathBuf>) -> CoordinatorShell<SimEnv> {
    let config = coord::Config { silence_limit: SILENCE, beat_period: BEAT, retries: 2, serial: 0 };
    CoordinatorShell::with_state(
        SimEnv::new(world, COORDINATOR),
        config,
        TraceWriter::off("coordinator"),
        Some(operator_key().verifying_key()),
        state,
    )
    .expect("the Coordinator's state file is readable")
}

/// The operator's signing key of every simulated Cluster.
pub fn operator_key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}

/// The signing key of a registered Worker, for a test that has it lie.
pub fn worker_key(node: Node) -> [u8; 32] {
    key(node, 1).to_bytes()
}

fn key(node: Node, salt: u8) -> SigningKey {
    let mut secret = [salt; 32];
    secret[..4].copy_from_slice(&node.to_be_bytes());
    SigningKey::from_bytes(&secret)
}

impl Cluster {
    /// `workers` Workers; the first `strangers` of them sign with keys that
    /// the registry does not hold. `seeds` Jobs race on every call.
    pub fn new(seed: u64, faults: Faults, workers: u32, strangers: u32, script: Vec<Call>, seeds: u64) -> Cluster {
        Cluster::with_speeds(seed, faults, workers, strangers, script, seeds, Speeds::reference())
    }

    pub fn with_speeds(
        seed: u64,
        faults: Faults,
        workers: u32,
        strangers: u32,
        script: Vec<Call>,
        seeds: u64,
        speeds: Speeds,
    ) -> Cluster {
        Cluster::with(seed, faults, workers, strangers, script, Setup { seeds, speeds, ..Setup::plain() })
    }

    pub fn with(seed: u64, faults: Faults, workers: u32, strangers: u32, script: Vec<Call>, setup: Setup) -> Cluster {
        let Setup { seeds, speeds, portfolio, first_pass_on_cluster, other_prints, certify, local_member } = setup;
        let world = World::new(seed, faults);
        world.borrow_mut().listen(COORDINATOR, ADDRESS);
        world.borrow_mut().set_velocity(FRONT_END, speeds.front_end);
        let nodes: Vec<Node> = (0..workers).map(|i| WORKERS + i).collect();
        let registry = Registry {
            version: 1,
            workers: nodes.iter().map(|n| (format!("w{n}"), key(*n, 1).verifying_key().to_bytes())).collect(),
        };
        let keys: BTreeMap<Node, SigningKey> = nodes
            .iter()
            .map(|n| (*n, if *n < WORKERS + strangers { key(*n, 2) } else { key(*n, 1) }))
            .collect();
        // What a Session stores (certificates) goes to a directory of its own.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "dp-sim-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let coordinator = Some(coordinator_shell(&world, Some(dir.join("coordinator.state"))));
        let (lock, lock_hash) = lock(certify);
        let config = session::Config {
            why3find: Default::default(),
            lock,
            // No Result_Cache: nothing of a simulated Session is on disk.
            args: vec!["--no-cache".into()],
            dir: dir.clone(),
            jobs: 2,
            cluster: Some(session::Cluster {
                address: ADDRESS.into(),
                registry,
                operator: operator_key().verifying_key(),
            }),
            job_margin: Duration::from_micros(MARGIN),
            silence_limit: Duration::from_micros(SILENCE),
            beat_period: Duration::from_micros(BEAT),
            record: None,
            portfolio_seeds: seeds,
            portfolio: portfolio
                .iter()
                .map(|p| session::Member { name: p.clone(), prover: p.trim_end_matches('@').to_string(), seed: None })
                .collect(),
            fast_remote: first_pass_on_cluster,
            local_racer: local_member,
            certification: certify.then(|| session::Certification {
                ethos: "ethos".into(),
                signature: "signature".into(),
                signature_hash: BINARY_HASH,
                check_micros: None,
                proof_limit: 1 << 20,
            }),
            certify_all: false,
            export: None,
            trace: None,
            lock_hash,
            matrix: None,
        };
        let why3find = Scripted {
            world: Rc::clone(&world),
            script,
            next: 0,
            announced: BTreeSet::new(),
            out: VecDeque::new(),
            questions: 0,
            velocity: speeds.front_end,
            answers: BTreeMap::new(),
            prints: other_prints.into_iter().map(|(goal, text)| ((goal, OTHER_PROVER.to_string()), text)).collect(),
            step: if first_pass_on_cluster { StrategyStep::Fast } else { StrategyStep::Parallel },
        };
        // The Front_End connects once the Coordinator listens.
        let front_end = Session::start(SimEnv::new(&world, FRONT_END), why3find, config, Default::default())
            .expect("a simulated Session starts");
        let mut cluster = Cluster { world, coordinator, workers: BTreeMap::new(), front_end, keys, speeds, dir };
        for node in nodes {
            cluster.start_worker(node);
        }
        cluster
    }

    pub fn start_coordinator(&mut self) {
        self.world.borrow_mut().restart(COORDINATOR);
        self.coordinator = Some(coordinator_shell(&self.world, Some(self.dir.join("coordinator.state"))));
    }

    pub fn start_worker(&mut self, node: Node) {
        self.world.borrow_mut().restart(node);
        let index = ((node - WORKERS) as usize).min(self.speeds.workers.len() - 1);
        let velocity = self.speeds.workers[index];
        self.world.borrow_mut().set_velocity(node, velocity);
        let provers = [PROVER, OTHER_PROVER];
        let config = WorkerConfig {
            id: format!("w{node}"),
            key: self.keys[&node].clone(),
            address: ADDRESS.into(),
            commands: provers.iter().map(|p| (p.to_string(), format!("{} %f", p.trim_end_matches('@')))).collect(),
            // The Worker's Calibration is the speed of its machine.
            velocities: provers.iter().map(|p| (p.to_string(), velocity)).collect(),
            pinned: provers.iter().map(|p| (p.trim_end_matches('@').to_string(), BINARY_HASH)).collect(),
            machine: worker::Config { silence_limit: SILENCE, beat_period: BEAT },
            reconnect_micros: BEAT,
        };
        self.workers.insert(node, WorkerShell::new(SimEnv::new(&self.world, node), config));
    }

    /// Starts the Coordinator again, from an empty state, with a complete
    /// trace of its steps in `path`.
    pub fn trace_coordinator(&mut self, path: &str) {
        let config = coord::Config { silence_limit: SILENCE, beat_period: BEAT, retries: 2, serial: 0 };
        let trace = TraceWriter::to_file("coordinator", path.into(), dp_shell::trace::Retention::Complete)
            .expect("the trace file can be written");
        self.coordinator = Some(CoordinatorShell::new(
            SimEnv::new(&self.world, COORDINATOR),
            config,
            trace,
            Some(operator_key().verifying_key()),
        ));
    }

    /// The node stops. Its shell and everything it knew are gone.
    pub fn crash(&mut self, node: Node, noticed: bool) {
        self.world.borrow_mut().crash(node, noticed);
        if node == COORDINATOR {
            self.coordinator = None;
        } else {
            self.workers.remove(&node);
        }
    }

    /// Lets every node take its turns at the current time.
    fn turns(&mut self) {
        loop {
            if let Some(coordinator) = &mut self.coordinator {
                coordinator.turn(Duration::ZERO).expect("simulated turn");
            }
            for worker in self.workers.values_mut() {
                worker.turn(Duration::ZERO).expect("simulated turn");
            }
            self.front_end.turn(Duration::ZERO).expect("simulated turn");
            let waiting = self.front_end.bridge.pending() || {
                let world = self.world.borrow();
                (self.coordinator.is_some() && world.pending(COORDINATOR))
                    || self.workers.keys().any(|n| world.pending(*n))
                    || world.pending(FRONT_END)
            };
            if !waiting {
                return;
            }
        }
    }

    /// Runs the Cluster until the clock reads `until`.
    pub fn run_until(&mut self, until: u64) {
        const STEP: u64 = 500;
        while self.world.borrow().now() < until {
            self.turns();
            self.world.borrow_mut().advance(STEP);
        }
        self.turns();
    }

    /// Sends an operator command about a Worker, signed with `key`, and
    /// runs the Cluster for a moment. Returns whether the Coordinator took
    /// the command.
    pub fn operate(&mut self, serial: u64, operation: Operation, worker: Node, key: &SigningKey) -> bool {
        let mut env = SimEnv::new(&self.world, OPERATOR);
        let peer = env.connect(ADDRESS).expect("the Coordinator listens");
        env.send(peer, &Message::Operator(SignedCommand::sign(serial, operation, &format!("w{worker}"), key)));
        let until = self.now() + 2_000;
        self.run_until(until);
        let mut taken = false;
        while let Some(event) = env.poll(Duration::ZERO) {
            if let NetEvent::Message(_, Message::OperatorResult { taken: t, .. }) = event {
                taken = t;
            }
        }
        env.close(peer);
        taken
    }

    /// Asks the Coordinator for its status report.
    pub fn status(&mut self) -> Option<Message> {
        let mut env = SimEnv::new(&self.world, OPERATOR);
        let peer = env.connect(ADDRESS).expect("the Coordinator listens");
        env.send(peer, &Message::StatusRequest);
        let until = self.now() + 2_000;
        self.run_until(until);
        let mut report = None;
        while let Some(event) = env.poll(Duration::ZERO) {
            if let NetEvent::Message(_, status @ Message::Status { .. }) = event {
                report = Some(status);
            }
        }
        env.close(peer);
        report
    }

    pub fn now(&self) -> u64 {
        self.world.borrow().now()
    }

    /// The reply to the call with this index, and when it came.
    pub fn answer(&self, call: usize) -> Option<(Reply, u64)> {
        self.front_end.bridge.answers.get(&call).copied()
    }

    /// Number of solver runs on the Front_End's machine.
    pub fn local_runs(&self) -> usize {
        self.world.borrow().started.iter().filter(|(node, _, _)| *node == FRONT_END).count()
    }

    /// The nodes that started a solver, in order.
    pub fn runners(&self) -> Vec<Node> {
        self.world.borrow().started.iter().map(|(node, _, _)| *node).collect()
    }
}
