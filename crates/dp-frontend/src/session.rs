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
    /// When set, one JSON line per finished prover call is appended to this
    /// file: raw measurements for the evaluation (R6.3).
    pub record: Option<PathBuf>,
    /// Number of Jobs, each with its own solver seed, that race on one prover
    /// call on the Cluster (R2.2). One means no racing.
    pub portfolio_seeds: u64,
    /// When set, every Printed_Goal that a prover call runs on is written to
    /// this directory, named by its Task_Hash.
    pub export: Option<PathBuf>,
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

/// What identifies the contents of a file between two looks at it without
/// reading it again: device, inode, size and the modification and status
/// change times.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp(u64, u64, u64, i64, i64, i64, i64);

fn file_stamp(path: &Path) -> std::io::Result<FileStamp> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::metadata(path)?;
    Ok(FileStamp(m.dev(), m.ino(), m.size(), m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec()))
}

struct Prover {
    info: ProverInfo,
    exe: PathBuf,
    /// The file's stamp when `pinned` was computed.
    stamp: FileStamp,
    /// Hash of the executable at Session start, equal to its Lock_File entry.
    pinned: Hash,
    /// Interned prover name and pinned version.
    name_id: u64,
    version_id: u64,
    /// why3find's Calibration: local time over reference time, in millionths.
    velocity: Option<u64>,
}

/// A Goal as why3find announced it.
struct Goal {
    id: u64,
    file: String,
    theory: String,
    name: String,
    path: String,
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Clone)]
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
    /// The seed of the Job's Solver_Configuration.
    seed: Option<u64>,
    /// When the Front_End stops waiting for a Result of this Job.
    deadline: Instant,
}

/// The Jobs that race on one prover call of why3find (R2.2): the first
/// proof label wins, and without one the call gets the most informative
/// non-proof outcome once every Job has ended.
struct Race {
    /// Jobs that have not ended.
    open: u64,
    /// Outcome and reply of each Job that ended without a proof label.
    lost: Vec<(Outcome, ProverResult)>,
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
    seed: Option<u64>,
}

/// A finished run whose answer the driver is reading.
struct Awaiting {
    request: Request,
    evidence: Evidence,
    asked: Instant,
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
    /// Races by the identifier of the prover call they answer.
    races: BTreeMap<String, Race>,
    portfolio_seeds: u64,
    /// Finished runs by the identifier of their open `read-answer` request.
    awaiting: BTreeMap<String, Awaiting>,
    verdicts: Verdicts,
    /// (goal, prover) pairs whose Result was labelled proved-standalone;
    /// shared with the bridge's responder for cache writes.
    standalone: Rc<RefCell<BTreeSet<(String, String)>>>,
    goals: BTreeMap<String, Goal>,
    record: Option<fs::File>,
    export: Option<PathBuf>,
    /// Wall-clock time by activity, for the summary line.
    spent: BTreeMap<&'static str, Duration>,
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
            let stamp = file_stamp(&exe)?;
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
                stamp,
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
        let t = Instant::now();
        let v = self.bridge.calibration(prover)?.max(1);
        *self.spent.entry("calibration").or_default() += t.elapsed();
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
            let t = Instant::now();
            let printed = self.bridge.print(gid, prover, &[])?;
            *self.spent.entry("print").or_default() += t.elapsed();
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
        // R7.7 criterion 7: check the solver binary just before starting it.
        // The binary is read and hashed again only if the file's stamp is
        // not the one seen when it was hashed at Session start; with an
        // unchanged stamp its contents are the pinned ones (TB-17).
        let t = Instant::now();
        let binary_hash =
            if file_stamp(&prover.exe)? == prover.stamp { prover.pinned } else { file_hash(&prover.exe)? };
        let hashing = t.elapsed();
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
        *self.spent.entry("binary-hash").or_default() += hashing;
        if let Some(dir) = &self.export {
            let file = dir.join(format!("{}{}", task_hash.to_hex(), self.prover(&request.prover)?.info.extension));
            if !file.exists() {
                fs::write(file, &printed.bytes)?;
            }
        }
        self.local.push(LocalJob { request, run, task_hash, binary_hash, mismatch, velocity });
        Ok(())
    }

    /// Submits the Jobs of one prover call to the Cluster: one Job with the
    /// configured seed and, with a Portfolio, one more for each further seed.
    /// On a lost connection the Session falls back to local proving (R1.6
    /// criteria 4 and 5).
    fn submit(&mut self, request: Request, printed: Printed) -> Result<(), SessionError> {
        let (driver, extension) = {
            let prover = self.prover(&request.prover)?;
            (prover.info.digest.clone().into_bytes(), prover.info.extension.clone())
        };
        let limit = Duration::from_micros(request.micros);
        let seeds: Vec<Option<u64>> =
            std::iter::once(None).chain((1..self.portfolio_seeds).map(Some)).collect();
        self.races.insert(request.rid.clone(), Race { open: 0, lost: Vec::new() });
        for seed in seeds {
            self.next_job += 1;
            let job = Job {
                job_id: format!("{}/{}", self.session_id, self.next_job),
                prover: request.prover.clone(),
                driver: driver.clone(),
                extension: extension.clone(),
                micros: request.micros,
                seed,
                goal: printed.bytes.clone(),
            };
            let job_id = job.job_id.clone();
            let (connection, _) = self.cluster.as_mut().expect("checked by the caller");
            if connection.send(&Message::Submit(job)).is_err() {
                self.fall_back("the connection to the Coordinator failed");
                // If no Job of this call was submitted yet, the fallback did
                // not see it.
                if self.races.remove(&request.rid).is_some() {
                    self.pending.push_front(Request { step: StrategyStep::Fast, ..request });
                }
                return Ok(());
            }
            // Twice the limit and a margin for queueing: after that the Job
            // is treated as lost.
            let deadline = Instant::now() + limit * 2 + self.job_margin;
            self.remote.insert(job_id, RemoteJob { request: request.clone(), seed, deadline });
            self.races.get_mut(&request.rid).expect("inserted").open += 1;
        }
        Ok(())
    }

    /// Stops using the Cluster; the prover calls that it was answering run
    /// on the local machine.
    fn fall_back(&mut self, reason: &str) {
        if self.cluster.take().is_some() {
            eprintln!("dp-frontend: {reason}; falling back to local proving");
        }
        let mut requests: BTreeMap<String, Request> = BTreeMap::new();
        for (_, job) in std::mem::take(&mut self.remote) {
            requests.entry(job.request.rid.clone()).or_insert(job.request);
        }
        for (rid, request) in requests {
            if self.races.remove(&rid).is_some() {
                self.pending.push_back(Request { step: StrategyStep::Fast, ..request });
            }
        }
    }

    /// Ends the Jobs of a race that are still open.
    fn cancel_race(&mut self, rid: &str) {
        let open: Vec<String> =
            self.remote.iter().filter(|(_, j)| j.request.rid == rid).map(|(id, _)| id.clone()).collect();
        for job_id in open {
            self.remote.remove(&job_id);
            if let Some((connection, _)) = self.cluster.as_mut() {
                let _ = connection.send(&Message::Cancel { job_id });
            }
        }
    }

    /// Gives why3find the answer of a prover call, directly for a local run
    /// and through its race for a Job on the Cluster.
    fn deliver(
        &mut self,
        rid: &str,
        raced: bool,
        outcome: Option<Outcome>,
        reply: ProverResult,
    ) -> Result<(), SessionError> {
        if !raced {
            return self.answer(rid, reply);
        }
        // A Job of a race that has already ended changes nothing.
        let Some(race) = self.races.get_mut(rid) else { return Ok(()) };
        race.open = race.open.saturating_sub(1);
        match outcome {
            // A proof label: the race is won (R2.2 criterion 8).
            None => {
                self.races.remove(rid);
                self.cancel_race(rid);
                self.answer(rid, reply)
            }
            Some(outcome) => {
                race.lost.push((outcome, reply));
                self.end_lost_race(rid, None)
            }
        }
    }

    /// Ends a race whose Jobs have all ended without a proof label: the call
    /// gets the most informative non-proof outcome (R2.2 criterion 14, R7.2
    /// criterion 5). If the Cluster ran none of its Jobs, the call runs on
    /// the local machine instead; `unrun` is then the call to run.
    fn end_lost_race(&mut self, rid: &str, unrun: Option<Request>) -> Result<(), SessionError> {
        let Some(race) = self.races.get(rid) else { return Ok(()) };
        if race.open > 0 {
            return Ok(());
        }
        let race = self.races.remove(rid).expect("present");
        if race.lost.is_empty() {
            if let Some(request) = unrun {
                eprintln!("dp-frontend: the Cluster ran no Job for a call of {}; running it locally", request.prover);
                self.pending.push_back(Request { step: StrategyStep::Fast, ..request });
                return Ok(());
            }
        }
        let outcomes: Vec<Outcome> = race.lost.iter().map(|(o, _)| *o).collect();
        let best = dp_core::labels::most_informative(&outcomes);
        let reply = race
            .lost
            .into_iter()
            .find(|(o, _)| *o == best)
            .map_or(ProverResult::Failed, |(_, reply)| reply);
        self.answer(rid, reply)
    }

    /// Takes what a finished run produced. The answer is read by the pinned
    /// Why3 driver, never by the Worker; the driver is asked without waiting,
    /// so other runs keep being started and reaped meanwhile.
    fn conclude(&mut self, request: Request, evidence: Evidence) -> Result<(), SessionError> {
        if evidence.timed_out {
            return self.label(request, evidence, ProverAnswer::Timeout);
        }
        // A run that died from a signal has no exit status; the driver still
        // reads its output, as a failed run.
        let status = evidence.status.unwrap_or(1);
        let qid = self.bridge.ask_answer(&request.prover, status, &evidence.output)?;
        self.awaiting.insert(qid, Awaiting { request, evidence, asked: Instant::now() });
        Ok(())
    }

    /// The driver's answer to an open `read-answer` request has arrived.
    fn answered(&mut self, qid: &str, answer: Option<DriverAnswer>) -> Result<(), SessionError> {
        let Some(Awaiting { request, evidence, asked }) = self.awaiting.remove(qid) else {
            return Err(SessionError::Bridge(BridgeError::Protocol(format!("unexpected reply {qid}"))));
        };
        *self.spent.entry("read-answer").or_default() += asked.elapsed();
        let mut answer = answer.map_or(ProverAnswer::HighFailure, core_answer);
        // Why3 never reads a proof from a run that a signal ended.
        if evidence.status.is_none() && answer == ProverAnswer::Valid {
            answer = ProverAnswer::HighFailure;
        }
        self.label(request, evidence, answer)
    }

    /// Labels a finished run and answers why3find.
    fn label(&mut self, request: Request, evidence: Evidence, answer: ProverAnswer) -> Result<(), SessionError> {
        let Request { rid, gid, prover, micros, step } = request;
        let (name_id, version_id) = {
            let p = self.prover(&prover)?;
            (p.name_id, p.version_id)
        };
        let (printed, local_task_hash) = self.print_local(&gid, &prover)?;
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
        let raced = evidence.remote.is_some();
        if acceptance != Acceptance::Accepted {
            eprintln!("dp-frontend: Result of {prover} for goal {gid} discarded: {acceptance:?}");
            *self.counts.entry("discarded".into()).or_insert(0) += 1;
            return self.deliver(&rid, raced, Some(Outcome::Failed), ProverResult::Failed);
        }
        let label = verdict::label(fields, &ctx);
        let spent = evidence.micros;
        let (name, reply) = match label {
            Label::ProvedStandalone => {
                let goal = self.goals[&gid].id;
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
        if let Some(record) = &mut self.record {
            use std::io::Write;
            let goal = &self.goals[&gid];
            writeln!(
                record,
                "{{\"file\":{},\"theory\":{},\"goal\":{},\"path\":{},\"prover\":{},\"seed\":{},\"step\":{},\"limit_us\":{},\"time_us\":{},\"where\":{},\"label\":{},\"task_hash\":{}}}",
                json_string(&goal.file),
                json_string(&goal.theory),
                json_string(&goal.name),
                json_string(&goal.path),
                json_string(&prover),
                evidence.seed.map_or("null".to_string(), |s| s.to_string()),
                json_string(step.name()),
                micros,
                spent,
                json_string(if evidence.remote.is_some() { "worker" } else { "local" }),
                json_string(name),
                json_string(&local_task_hash.to_hex()),
            )?;
        }
        let outcome = match label {
            Label::ProvedStandalone => None,
            Label::NonProof(outcome) => Some(outcome),
            Label::Certified => Some(Outcome::Unchecked),
        };
        self.deliver(&rid, raced, outcome, reply)
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
            // The Job as submitted: its prover and its seed (R1.3 criterion 6).
            submitted: signed.fields.prover == job.request.prover && signed.fields.seed == job.seed,
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
                seed: job.seed,
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
                            seed: None,
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
            let reply = ProverResult::Timeout { micros: job.request.micros };
            self.deliver(&job.request.rid, true, Some(Outcome::Timeout), reply)?;
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
            FromWhy3find::Goal { gid, file, theory, name, path, .. } => {
                let id = self.goals.len() as u64;
                let path = path.iter().map(|p| format!("{}.{}", p.tactic, p.index)).collect::<Vec<_>>().join("/");
                self.goals.insert(gid, Goal { id, file, theory, name, path });
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
                if self.races.remove(&rid).is_some() {
                    self.cancel_race(&rid);
                    return self.answer(&rid, ProverResult::NoResult);
                }
                Ok(()) // already answered
            }
            FromWhy3find::AnswerRead { qid, answer } => self.answered(&qid, Some(answer)),
            FromWhy3find::Error { qid, .. } if self.awaiting.contains_key(&qid) => {
                self.answered(&qid, None)
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
                    // The Cluster did not run this Job. Its race goes on
                    // with the other Jobs; if none of them ran either, the
                    // call runs here.
                    if let Some(job) = self.remote.remove(&job_id) {
                        eprintln!("dp-frontend: {job_id} was not run on the Cluster ({reason})");
                        let rid = job.request.rid.clone();
                        if let Some(race) = self.races.get_mut(&rid) {
                            race.open = race.open.saturating_sub(1);
                        }
                        self.end_lost_race(&rid, Some(job.request))?;
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
        races: BTreeMap::new(),
        portfolio_seeds: config.portfolio_seeds.max(1),
        awaiting: BTreeMap::new(),
        verdicts: Verdicts { entries: Vec::new() },
        standalone,
        goals: BTreeMap::new(),
        record: match &config.record {
            Some(path) => Some(fs::OpenOptions::new().create(true).append(true).open(path)?),
            None => None,
        },
        export: config.export.clone(),
        spent: BTreeMap::new(),
        counts: BTreeMap::new(),
        remote_proofs: 0,
    };
    let result = session.run();
    for job in session.local.drain(..) {
        job.run.cancel();
    }
    let _ = fs::remove_dir_all(&tmp);
    let Session { bridge, verdicts, counts, remote_proofs, spent, .. } = session;
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
            if std::env::var_os("DP_PROFILE").is_some() {
                let parts: Vec<_> = spent.iter().map(|(k, d)| format!("{k} {:.2}s", d.as_secs_f64())).collect();
                eprintln!("dp-frontend: time in {}", parts.join(", "));
            }
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
