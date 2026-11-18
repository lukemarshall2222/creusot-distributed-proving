//! One Session of `dp-frontend prove` (design C3, flows (a) and (d)).
//!
//! The shell runs why3find in dispatch mode and answers its prover calls. A
//! prover call of the first sequential pass runs on the local machine
//! (R1.2); any other call goes to the Cluster when one is configured and
//! connected, and runs locally otherwise. Every decision about a Result
//! (acceptance, label, Verdict) is taken by dp-core, from facts that this
//! shell computes locally: the Task_Hash of its own print, the hash of the
//! solver binary or the signature and registry entry of the Worker, and the
//! answer that the pinned Why3 driver reads from the solver's output.
//!
//! Not here yet: the Result_Cache (every cache read is a miss), `dp.toml`,
//! the `dp` members of `proof.json`, event traces, heartbeats, Calibration
//! of Workers, Races, and the loop over the dp-core Front_End machine.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use dp_core::{
    labels::{Outcome, ProverAnswer},
    verdict::{self, Acceptance, Context, Label, RemoteFacts, ResultFields, UnpatchedSolver, Verdicts},
};
use dp_formats::{
    cluster::{Job, Message, Registry, SignedResult},
    dispatch::{DriverAnswer, FromWhy3find, ProverInfo, ProverResult, StrategyStep, ToWhy3find},
    hash::{self, Hash},
    lockfile::{current_platform, Kind, LockFile},
};
use dp_shell::{
    net::{Connection, Event},
    solver::{self, Run},
    why3find::{Bridge, BridgeError, Poll, Printed},
};

/// Size limit of one `dispatch` message. A working value for the local pipe,
/// not a proposal for `<TBD-22>`.
const MESSAGE_LIMIT: usize = 1 << 30;

/// The Cluster of a Session: the Coordinator's address and the Worker
/// registry, already verified against the operator signing key.
pub struct Cluster {
    pub address: String,
    pub registry: Registry,
}

pub struct Config {
    /// The patched why3find.
    pub why3find: PathBuf,
    pub lock: LockFile,
    /// Arguments of `why3find prove`.
    pub args: Vec<OsString>,
    /// The project directory.
    pub dir: PathBuf,
    /// Number of solver processes that run at once on the local machine.
    pub jobs: usize,
    pub cluster: Option<Cluster>,
    /// How long, beyond twice its time limit, the Front_End waits for the
    /// Result of a Job on the Cluster. A placeholder for the per-goal
    /// deadline `<TBD-3>`.
    pub job_margin: Duration,
}

#[derive(Debug)]
pub enum SessionError {
    Bridge(BridgeError),
    /// A local executable does not match the Lock_File (R7.7 criterion 3).
    Pin(String),
    Io(std::io::Error),
    Unsupported(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Bridge(e) => write!(f, "{e}"),
            SessionError::Pin(m) => write!(f, "pinned dependency check failed: {m}"),
            SessionError::Io(e) => write!(f, "{e}"),
            SessionError::Unsupported(m) => write!(f, "unsupported: {m}"),
        }
    }
}

impl From<BridgeError> for SessionError {
    fn from(e: BridgeError) -> Self {
        SessionError::Bridge(e)
    }
}

impl From<std::io::Error> for SessionError {
    fn from(e: std::io::Error) -> Self {
        SessionError::Io(e)
    }
}

fn core_hash(h: Hash) -> verdict::Hash {
    let w = |i: usize| u64::from_be_bytes(h.0[8 * i..8 * i + 8].try_into().expect("8 bytes"));
    verdict::Hash { w0: w(0), w1: w(1), w2: w(2), w3: w(3) }
}

fn file_hash(path: &Path) -> std::io::Result<Hash> {
    Ok(hash::sha256(&fs::read(path)?))
}

fn core_answer(answer: DriverAnswer) -> ProverAnswer {
    match answer {
        DriverAnswer::Valid => ProverAnswer::Valid,
        DriverAnswer::Invalid => ProverAnswer::Invalid,
        DriverAnswer::Timeout => ProverAnswer::Timeout,
        DriverAnswer::OutOfMemory => ProverAnswer::OutOfMemory,
        DriverAnswer::StepLimitExceeded => ProverAnswer::StepLimitExceeded,
        DriverAnswer::Unknown => ProverAnswer::Unknown,
        DriverAnswer::Failure => ProverAnswer::Failure,
        DriverAnswer::HighFailure => ProverAnswer::HighFailure,
    }
}

struct Prover {
    info: ProverInfo,
    exe: PathBuf,
    /// Hash of the executable at Session start, equal to its Lock_File entry.
    pinned: Hash,
    /// Interned prover name and pinned version.
    name_id: u64,
    version_id: u64,
    /// why3find's Calibration: local time over reference time, in millionths.
    velocity: Option<u64>,
}

struct Request {
    rid: String,
    gid: String,
    prover: String,
    step: StrategyStep,
    /// The reference time limit.
    micros: u64,
}

struct LocalJob {
    request: Request,
    run: Run,
    task_hash: Hash,
    binary_hash: Hash,
    mismatch: bool,
    velocity: u64,
}

struct RemoteJob {
    request: Request,
    /// When the Front_End stops waiting for a Result of this Job.
    deadline: Instant,
}

/// What a finished run produced, from the local machine or from a Result.
struct Evidence {
    task_hash: Hash,
    binary_hash: Hash,
    mismatch: bool,
    status: Option<u32>,
    timed_out: bool,
    output: Vec<u8>,
    /// Time in reference microseconds.
    micros: u64,
    remote: Option<RemoteFacts>,
}

struct Session {
    bridge: Bridge,
    lock: LockFile,
    platform: String,
    tmp: PathBuf,
    jobs: usize,
    job_margin: Duration,
    cluster: Option<(Connection, Registry)>,
    session_id: String,
    next_job: u64,
    provers: Option<BTreeMap<String, Prover>>,
    names: BTreeMap<String, u64>,
    prints: BTreeMap<(String, String), (Printed, Hash)>,
    pending: VecDeque<Request>,
    local: Vec<LocalJob>,
    remote: BTreeMap<String, RemoteJob>,
    verdicts: Verdicts,
    /// (goal, prover) pairs whose Result was labelled proved-standalone;
    /// shared with the bridge's responder for cache writes.
    standalone: Rc<RefCell<BTreeSet<(String, String)>>>,
    goals: BTreeMap<String, u64>,
    counts: BTreeMap<String, u64>,
    remote_proofs: u64,
}

impl Session {
    fn intern(&mut self, text: &str) -> u64 {
        let next = self.names.len() as u64;
        *self.names.entry(text.to_string()).or_insert(next)
    }

    /// Asks why3find for the provers and checks each executable against the
    /// Lock_File, before any solver starts (R7.7 criterion 3).
    fn load_provers(&mut self) -> Result<(), SessionError> {
        if self.provers.is_some() {
            return Ok(());
        }
        let mut provers = BTreeMap::new();
        for info in self.bridge.provers()? {
            let program = solver::words(&info.command)
                .into_iter()
                .next()
                .ok_or_else(|| SessionError::Unsupported("empty prover command".into()))?;
            let exe = solver::find_executable(&program)
                .map_err(|e| SessionError::Pin(format!("{}: {program}: {e}", info.prover)))?;
            let actual = file_hash(&exe)?;
            let (name, reported) = info.prover.split_once('@').unwrap_or((&info.prover, ""));
            let entries: Vec<_> = self
                .lock
                .pins(Kind::UnpatchedSolver, &self.platform)
                .chain(self.lock.pins(Kind::SharingSolver, &self.platform))
                .filter(|(tool, _)| tool.prover.as_deref() == Some(name))
                .map(|(tool, hash)| (hash, tool.version.clone()))
                .collect();
            let pinned_version =
                entries.iter().find(|(hash, _)| *hash == actual).map(|(_, v)| v.clone());
            let Some(version) = pinned_version else {
                return Err(SessionError::Pin(format!(
                    "{} ({}): binary hash {:?}, Lock_File {}",
                    info.prover,
                    exe.display(),
                    actual,
                    if entries.is_empty() {
                        "has no entry".to_string()
                    } else {
                        format!("pins {entries:?}")
                    }
                )));
            };
            // The version of the Solver_Configuration is the pinned one. Why3
            // configurations may leave the version empty (Creusot's does);
            // a version that they do state must agree with the Lock_File.
            if !reported.is_empty() && reported != version {
                return Err(SessionError::Pin(format!(
                    "{}: the prover configuration states version {reported}, the Lock_File pins {version} for this binary",
                    info.prover
                )));
            }
            let name = name.to_string();
            let prover = Prover {
                exe,
                pinned: actual,
                name_id: self.intern(&format!("prover:{name}")),
                version_id: self.intern(&format!("version:{version}")),
                velocity: None,
                info,
            };
            provers.insert(prover.info.prover.clone(), prover);
        }
        self.provers = Some(provers);
        Ok(())
    }

    fn prover(&self, name: &str) -> Result<&Prover, SessionError> {
        self.provers.as_ref().and_then(|p| p.get(name)).ok_or_else(|| {
            SessionError::Bridge(BridgeError::Protocol(format!("prover call for the unknown prover {name}")))
        })
    }

    /// why3find's Calibration of a prover on this machine (AS-4).
    fn velocity(&mut self, prover: &str) -> Result<u64, SessionError> {
        if let Some(v) = self.prover(prover)?.velocity {
            return Ok(v);
        }
        let v = self.bridge.calibration(prover)?.max(1);
        self.provers.as_mut().expect("loaded").get_mut(prover).expect("checked").velocity = Some(v);
        Ok(v)
    }

    /// The Unpatched_Solver entries of the Lock_File, as dp-core reads them.
    fn unpatched(&mut self) -> Vec<UnpatchedSolver> {
        let pins: Vec<_> = self
            .lock
            .pins(Kind::UnpatchedSolver, &self.platform)
            .map(|(tool, hash)| (tool.prover.clone().unwrap_or_default(), tool.version.clone(), hash))
            .collect();
        pins.into_iter()
            .map(|(prover, version, hash)| UnpatchedSolver {
                prover: self.intern(&format!("prover:{prover}")),
                version: self.intern(&format!("version:{version}")),
                binary_hash: core_hash(hash),
            })
            .collect()
    }

    fn answer(&mut self, rid: &str, result: ProverResult) -> Result<(), SessionError> {
        Ok(self.bridge.send(&ToWhy3find::Answer { rid: rid.to_string(), result })?)
    }

    /// `print_local` of the Goal for the prover's Driver, with its Task_Hash.
    fn print_local(&mut self, gid: &str, prover: &str) -> Result<(Printed, Hash), SessionError> {
        let key = (gid.to_string(), prover.to_string());
        if !self.prints.contains_key(&key) {
            let printed = self.bridge.print(gid, prover, &[])?;
            // The Driver is identified by why3find's digest of the prover
            // configuration and driver files; no further transformations.
            let digest = self.prover(prover)?.info.digest.clone();
            let task_hash = hash::task_hash(&printed.bytes, digest.as_bytes(), b"");
            self.prints.insert(key.clone(), (printed, task_hash));
        }
        Ok(self.prints[&key].clone())
    }

    fn start(&mut self, request: Request) -> Result<(), SessionError> {
        self.load_provers()?;
        self.prover(&request.prover)?;
        let (printed, task_hash) = self.print_local(&request.gid, &request.prover)?;
        if printed.tasks != 1 {
            // print_local is undefined (R1.3 criterion 10): no Job, no Result.
            eprintln!(
                "dp-frontend: goal {} with {}: the pipeline yields {} tasks; not run",
                request.gid, request.prover, printed.tasks
            );
            return self.answer(&request.rid.clone(), ProverResult::Failed);
        }
        if request.step != StrategyStep::Fast && self.cluster.is_some() {
            return self.submit(request, printed);
        }
        let velocity = self.velocity(&request.prover)?;
        let prover = self.prover(&request.prover)?;
        // R7.7 criterion 7: hash the solver again just before starting it.
        let binary_hash = file_hash(&prover.exe)?;
        let mismatch = binary_hash != prover.pinned;
        let local_micros = (u128::from(request.micros) * u128::from(velocity) / 1_000_000) as u64;
        let run = solver::start(
            &prover.exe,
            &prover.info.command,
            &printed.bytes,
            &prover.info.extension,
            local_micros.max(1),
            &self.tmp,
            &format!("job{}", request.rid),
        )?;
        self.local.push(LocalJob { request, run, task_hash, binary_hash, mismatch, velocity });
        Ok(())
    }

    /// Submits a Job to the Cluster. On a lost connection the Session falls
    /// back to local proving (R1.6 criteria 4 and 5).
    fn submit(&mut self, request: Request, printed: Printed) -> Result<(), SessionError> {
        self.next_job += 1;
        let prover = self.prover(&request.prover)?;
        let job = Job {
            job_id: format!("{}/{}", self.session_id, self.next_job),
            prover: request.prover.clone(),
            driver: prover.info.digest.clone().into_bytes(),
            extension: prover.info.extension.clone(),
            micros: request.micros,
            goal: printed.bytes,
        };
        let job_id = job.job_id.clone();
        let limit = Duration::from_micros(request.micros);
        let (connection, _) = self.cluster.as_mut().expect("checked by the caller");
        if connection.send(&Message::Submit(job)).is_err() {
            self.fall_back("the connection to the Coordinator failed");
            self.pending.push_front(request);
            return Ok(());
        }
        // Twice the limit and a margin for queueing: after that the Job is
        // treated as lost.
        let deadline = Instant::now() + limit * 2 + self.job_margin;
        self.remote.insert(job_id, RemoteJob { request, deadline });
        Ok(())
    }

    /// Stops using the Cluster; its open Jobs run on the local machine.
    fn fall_back(&mut self, reason: &str) {
        if self.cluster.take().is_some() {
            eprintln!("dp-frontend: {reason}; falling back to local proving");
        }
        for (_, job) in std::mem::take(&mut self.remote) {
            self.pending.push_back(Request { step: StrategyStep::Fast, ..job.request });
        }
    }

    /// Labels what a finished run produced and answers why3find.
    fn conclude(&mut self, request: Request, evidence: Evidence) -> Result<(), SessionError> {
        let Request { rid, gid, prover, micros, .. } = request;
        let (name_id, version_id) = {
            let p = self.prover(&prover)?;
            (p.name_id, p.version_id)
        };
        let (printed, local_task_hash) = self.print_local(&gid, &prover)?;
        // The answer is read by the pinned Why3 driver, never by the Worker.
        let answer = if evidence.timed_out {
            ProverAnswer::Timeout
        } else {
            match evidence.status {
                Some(status) => core_answer(self.bridge.read_answer(&prover, status, &evidence.output)?),
                None => ProverAnswer::HighFailure,
            }
        };
        let empty = hash::imports_hash(&[]);
        let fields = ResultFields {
            answer,
            task_hash: core_hash(evidence.task_hash),
            partition: false,
            imports: 0,
            imports_hash: core_hash(empty),
            prover: name_id,
            version: version_id,
            binary_hash: core_hash(evidence.binary_hash),
        };
        let ctx = Context {
            certify_all: false,
            import_mark: false,
            local_task_hash: core_hash(local_task_hash),
            task_count: u64::from(printed.tasks),
            empty_imports_hash: core_hash(empty),
            local_binary_mismatch: evidence.mismatch,
            unpatched: self.unpatched(),
        };
        let acceptance = match evidence.remote {
            Some(facts) => verdict::accept_remote(fields, &ctx, facts),
            None => verdict::accept_result(fields, &ctx),
        };
        if acceptance != Acceptance::Accepted {
            eprintln!("dp-frontend: Result of {prover} for goal {gid} discarded: {acceptance:?}");
            *self.counts.entry("discarded".into()).or_insert(0) += 1;
            return self.answer(&rid, ProverResult::Failed);
        }
        let label = verdict::label(fields, &ctx);
        let spent = evidence.micros;
        let (name, reply) = match label {
            Label::ProvedStandalone => {
                let goal = self.goals[&gid];
                let verdicts = std::mem::replace(&mut self.verdicts, Verdicts { entries: Vec::new() });
                let before = verdicts.entries.len();
                self.verdicts = verdict::record_verdict(verdicts, goal, label);
                self.standalone.borrow_mut().insert((gid.clone(), prover.clone()));
                // Count the Verdict, not every Result that could have given it.
                if evidence.remote.is_some() && self.verdicts.entries.len() > before {
                    self.remote_proofs += 1;
                }
                ("proved-standalone", ProverResult::Valid { micros: spent })
            }
            // Not reachable in this build; never answered as a proof here.
            Label::Certified => ("unchecked", ProverResult::Unknown { micros: spent }),
            Label::NonProof(Outcome::Timeout) => ("timeout", ProverResult::Timeout { micros }),
            Label::NonProof(Outcome::Failed) => {
                eprintln!(
                    "dp-frontend: {prover} gave no answer on goal {gid}: {:?}",
                    String::from_utf8_lossy(&evidence.output).chars().take(300).collect::<String>()
                );
                ("failed", ProverResult::Failed)
            }
            Label::NonProof(Outcome::Unchecked) => {
                eprintln!(
                    "dp-frontend: {prover} reports a proof of goal {gid}, labelled unchecked \
                     (its binary is not an Unpatched_Solver of the Lock_File)"
                );
                ("unchecked", ProverResult::Unknown { micros: spent })
            }
            Label::NonProof(Outcome::StandaloneInvalid | Outcome::DependentInvalid) => {
                ("invalid", ProverResult::Unknown { micros: spent })
            }
            Label::NonProof(_) => ("unknown", ProverResult::Unknown { micros: spent }),
        };
        *self.counts.entry(name.into()).or_insert(0) += 1;
        self.answer(&rid, reply)
    }

    /// Handles a Result that the Coordinator forwarded. Everything in it is
    /// untrusted until dp-core accepts it.
    fn result(&mut self, signed: SignedResult) -> Result<(), SessionError> {
        let Some(job) = self.remote.get(&signed.fields.job_id) else {
            // Not an open Job of this Session: late, replayed or invented.
            eprintln!("dp-frontend: Result for {:?} discarded: no such open Job", signed.fields.job_id);
            *self.counts.entry("discarded".into()).or_insert(0) += 1;
            return Ok(());
        };
        let (_, registry) = self.cluster.as_ref().expect("a Result arrives from the Cluster");
        let key = registry.key(&signed.fields.worker);
        let facts = RemoteFacts {
            registered: key.is_some(),
            revoked: false,
            signature_valid: key.is_some_and(|k| signed.verifies(&k)),
            submitted: signed.fields.prover == job.request.prover,
        };
        if !(facts.registered && facts.signature_valid) {
            // Not authentic: anyone on the path can send this. The Job stays
            // open for an authentic Result (R1.3 criterion 4).
            eprintln!(
                "dp-frontend: Result for {:?} discarded: not signed by a registered Worker (names {:?})",
                signed.fields.job_id, signed.fields.worker
            );
            *self.counts.entry("discarded".into()).or_insert(0) += 1;
            return Ok(());
        }
        // An authentic Result ends its Job, accepted or not (R2.2 criterion 24).
        let job = self.remote.remove(&signed.fields.job_id).expect("checked");
        let f = signed.fields;
        self.conclude(
            job.request,
            Evidence {
                task_hash: f.task_hash,
                binary_hash: f.binary_hash,
                mismatch: false,
                status: f.status,
                timed_out: f.timed_out,
                output: f.output,
                micros: f.elapsed_micros,
                remote: Some(facts),
            },
        )
    }

    fn reap(&mut self) -> Result<(), SessionError> {
        let mut i = 0;
        while i < self.local.len() {
            match self.local[i].run.poll()? {
                None => i += 1,
                Some(finished) => {
                    let job = self.local.swap_remove(i);
                    // Report the time in reference units, as why3find does.
                    let micros = (finished.elapsed.as_micros() * 1_000_000 / u128::from(job.velocity)) as u64;
                    self.conclude(
                        job.request,
                        Evidence {
                            task_hash: job.task_hash,
                            binary_hash: job.binary_hash,
                            mismatch: job.mismatch,
                            status: finished.status,
                            timed_out: finished.timed_out,
                            output: finished.output,
                            micros,
                            remote: None,
                        },
                    )?;
                }
            }
        }
        let now = Instant::now();
        let late: Vec<String> =
            self.remote.iter().filter(|(_, j)| now >= j.deadline).map(|(id, _)| id.clone()).collect();
        for job_id in late {
            let job = self.remote.remove(&job_id).expect("listed");
            eprintln!("dp-frontend: no Result for {job_id} in time");
            if let Some((connection, _)) = self.cluster.as_mut() {
                let _ = connection.send(&Message::Cancel { job_id });
            }
            *self.counts.entry("timeout".into()).or_insert(0) += 1;
            self.answer(&job.request.rid, ProverResult::Timeout { micros: job.request.micros })?;
        }
        // Remote submissions do not count against the local limit.
        let mut deferred = VecDeque::new();
        while let Some(request) = self.pending.pop_front() {
            let goes_remote = request.step != StrategyStep::Fast && self.cluster.is_some();
            if goes_remote || self.local.len() < self.jobs {
                self.start(request)?;
            } else {
                deferred.push_back(request);
            }
        }
        self.pending = deferred;
        Ok(())
    }

    fn handle(&mut self, msg: FromWhy3find) -> Result<(), SessionError> {
        match msg {
            FromWhy3find::Hello { version: 1 } => Ok(()),
            FromWhy3find::Hello { version } => {
                Err(SessionError::Unsupported(format!("dispatch protocol version {version}")))
            }
            FromWhy3find::Goal { gid, .. } => {
                let id = self.goals.len() as u64;
                self.goals.insert(gid, id);
                Ok(())
            }
            FromWhy3find::Prove { rid, gid, step, prover, micros } => {
                if !self.goals.contains_key(&gid) {
                    return Err(SessionError::Bridge(BridgeError::Protocol(format!(
                        "prover call for the unannounced goal {gid}"
                    ))));
                }
                self.pending.push_back(Request { rid, gid, prover, step, micros });
                Ok(())
            }
            FromWhy3find::Cancel { rid } => {
                if let Some(i) = self.pending.iter().position(|r| r.rid == rid) {
                    self.pending.remove(i);
                    return self.answer(&rid, ProverResult::NoResult);
                }
                if let Some(i) = self.local.iter().position(|j| j.request.rid == rid) {
                    self.local.swap_remove(i).run.cancel();
                    return self.answer(&rid, ProverResult::NoResult);
                }
                let job_id = self.remote.iter().find(|(_, j)| j.request.rid == rid).map(|(id, _)| id.clone());
                if let Some(job_id) = job_id {
                    self.remote.remove(&job_id);
                    if let Some((connection, _)) = self.cluster.as_mut() {
                        let _ = connection.send(&Message::Cancel { job_id });
                    }
                    return self.answer(&rid, ProverResult::NoResult);
                }
                Ok(()) // already answered
            }
            FromWhy3find::Done => Ok(()),
            other => Err(SessionError::Bridge(BridgeError::Protocol(format!(
                "unexpected message {other:?}"
            )))),
        }
    }

    fn cluster_events(&mut self) -> Result<(), SessionError> {
        loop {
            let Some((connection, _)) = self.cluster.as_mut() else { return Ok(()) };
            match connection.poll(Duration::ZERO) {
                Event::Idle => return Ok(()),
                Event::Closed(reason) => {
                    self.fall_back(&format!(
                        "the Coordinator is gone{}",
                        reason.map(|r| format!(" ({r})")).unwrap_or_default()
                    ));
                    return Ok(());
                }
                Event::Message(Message::Result(signed)) => self.result(signed)?,
                Event::Message(Message::Failed { job_id, reason }) => {
                    // The Cluster did not run the Job; run it here instead.
                    if let Some(job) = self.remote.remove(&job_id) {
                        eprintln!("dp-frontend: {job_id} failed on the Cluster ({reason}); running it locally");
                        self.pending.push_back(Request { step: StrategyStep::Fast, ..job.request });
                    }
                }
                // Nothing else is part of the protocol towards a Front_End.
                Event::Message(_) => {}
            }
        }
    }

    fn run(&mut self) -> Result<(), SessionError> {
        loop {
            match self.bridge.poll(Duration::from_millis(5))? {
                Poll::Message(msg) => self.handle(msg)?,
                Poll::Idle => {}
                Poll::Closed => return Ok(()),
            }
            self.cluster_events()?;
            self.reap()?;
        }
    }
}

/// Runs one Session and returns the exit status for Creusot_CLI.
pub fn run(config: Config) -> Result<i32, SessionError> {
    let platform = current_platform();
    // R7.7 criterion 3: why3find itself is a local executable of the Session.
    let why3find = fs::canonicalize(&config.why3find)
        .map_err(|e| SessionError::Pin(format!("{}: {e}", config.why3find.display())))?;
    let actual = file_hash(&why3find)?;
    let pinned: Vec<_> = config.lock.pins(Kind::Why3find, &platform).map(|(_, h)| h).collect();
    if !pinned.contains(&actual) {
        return Err(SessionError::Pin(format!(
            "why3find ({}): binary hash {actual:?}, Lock_File pins {pinned:?}",
            why3find.display()
        )));
    }
    let session_id = format!(
        "{:x}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    );
    // R1.1 criterion 5: without a connection, name the address and prove locally.
    let cluster = config.cluster.and_then(|c| {
        let connected = Connection::connect(&c.address).and_then(|mut connection| {
            connection.send(&Message::FrontEndHello { session: session_id.clone() })?;
            Ok(connection)
        });
        match connected {
            Ok(connection) => Some((connection, c.registry)),
            Err(e) => {
                eprintln!(
                    "dp-frontend: no connection to the Coordinator at {}: {e}; proving locally",
                    c.address
                );
                None
            }
        }
    });
    let tmp = std::env::temp_dir().join(format!("dp-frontend-{}", std::process::id()));
    fs::create_dir_all(&tmp)?;
    let standalone = Rc::new(RefCell::new(BTreeSet::new()));
    let proved = Rc::clone(&standalone);
    // why3find blocks on its cache messages, so the bridge answers them itself.
    let responder = move |msg: &FromWhy3find| match msg {
        // No Result_Cache yet: every lookup is a miss.
        FromWhy3find::CacheGet { rid, .. } => {
            Some(ToWhy3find::Cache { rid: rid.clone(), result: ProverResult::NoResult })
        }
        // The legacy cache layout holds proved-standalone Results only
        // (R7.5 criterion 7).
        FromWhy3find::CacheSet { rid, gid, prover, result } => Some(ToWhy3find::Stored {
            rid: rid.clone(),
            store: matches!(result, ProverResult::Valid { .. })
                && proved.borrow().contains(&(gid.clone(), prover.clone())),
        }),
        _ => None,
    };
    let bridge =
        Bridge::start(&why3find, &config.args, &config.dir, MESSAGE_LIMIT, Box::new(responder))?;
    let mut session = Session {
        bridge,
        lock: config.lock,
        platform,
        tmp: tmp.clone(),
        jobs: config.jobs.max(1),
        job_margin: config.job_margin,
        cluster,
        session_id,
        next_job: 0,
        provers: None,
        names: BTreeMap::new(),
        prints: BTreeMap::new(),
        pending: VecDeque::new(),
        local: Vec::new(),
        remote: BTreeMap::new(),
        verdicts: Verdicts { entries: Vec::new() },
        standalone,
        goals: BTreeMap::new(),
        counts: BTreeMap::new(),
        remote_proofs: 0,
    };
    let result = session.run();
    for job in session.local.drain(..) {
        job.run.cancel();
    }
    let _ = fs::remove_dir_all(&tmp);
    let Session { bridge, verdicts, counts, remote_proofs, .. } = session;
    match result {
        Ok(()) => {
            let status = bridge.wait()?;
            let summary: Vec<_> = counts.iter().map(|(label, n)| format!("{n} {label}")).collect();
            eprintln!(
                "dp-frontend: {} proved-standalone Verdicts ({} from Workers); prover results: {}",
                verdicts.entries.len(),
                remote_proofs,
                if summary.is_empty() { "none".to_string() } else { summary.join(", ") }
            );
            Ok(status.code().unwrap_or(1))
        }
        Err(e) => {
            // Dropping the bridge closes the pipe: why3find exits without
            // writing its proof files, so the previous ones stay.
            drop(bridge);
            Err(e)
        }
    }
}
