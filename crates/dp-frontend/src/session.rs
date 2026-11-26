//! One Session of `dp-frontend prove`: the I/O shell of the Front_End
//! machine (design C3; "How the shells drive dp-core").
//!
//! The shell decides nothing. It turns what happens into inputs of the
//! dp-core Front_End machine, each carrying facts that it computed locally,
//! and carries out the effects that a step returns:
//!
//! ```text
//! loop:
//!   input := next thing that happened, with its facts
//!   step  := frontend::step(state, input)        # pure, verified
//!   trace(step.action, input)                     # before any effect
//!   state := step.state
//!   for e in step.effects: perform(e)
//! ```
//!
//! The facts are: the Task_Hash and task count of its own print of a Goal,
//! the hash of a solver binary or the signature and registry entry of a
//! Worker, the answer that the pinned Why3 driver reads from a solver's
//! output, and the checks of a Result_Cache entry.
//!
//! Not here yet: `dp.toml`, the `dp` members of `proof.json` beyond the
//! labels file, heartbeats, Calibration of Workers.

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
    cache_key::{self, LookupFields, Validation, ValidationFacts},
    calibration,
    frontend::{self, CallId, Effect, Evidence, Input, Pass, Reply, State},
    labels::{Outcome, ProverAnswer},
    machine::{LogEntry, Machine},
    verdict::{self, Label, RemoteFacts, ResultFields, UnpatchedSolver},
};
use dp_formats::{
    cache::{Entry, Origin},
    cluster::{Job, JobKind, Message, Registry, SignedResult},
    dispatch::{DriverAnswer, FromWhy3find, ProverInfo, ProverResult, StrategyStep, ToWhy3find},
    hash::{self, Hash},
    lockfile::{current_platform, Kind, LockFile},
    proofjson::{DpEntry, InvalidEntry, Place, ReplayFields},
};
use crate::{
    accept::{self, core_answer, core_hash, driver_answer},
    cache::{self as result_cache, formats_atoms},
    proofs::{self, GoalLabel},
};
use dp_shell::{
    env::{Env, NetEvent, OsEnv, Peer, RunSpec},
    solver,
    store,
    trace::{Retention, TraceWriter},
    why3find::{Bridge, BridgeError, Poll, Printed, Why3find},
};

/// Size limit of one `dispatch` message and of one cache entry. Working
/// values, not proposals for `<TBD-22>`.
const MESSAGE_LIMIT: usize = 1 << 30;

/// The Cluster of a Session: the Coordinator's address and the Worker
/// registry, already verified against the operator signing key.
pub struct Cluster {
    pub address: String,
    pub registry: Registry,
    /// The operator's verification key: revocations are accepted only with
    /// its signature (R1.7 criterion 6).
    pub operator: ed25519_dalek::VerifyingKey,
}

/// The checker of certification, as a Session uses it (R5.3).
#[derive(Clone, Debug)]
pub struct Certification {
    /// The checker's program: a path, or a name on PATH.
    pub ethos: String,
    /// The directory of its signature files, and their hash.
    pub signature: PathBuf,
    pub signature_hash: Hash,
    /// The time limit of one checker run in microseconds; without one it
    /// follows from the time limit of the prover call.
    pub check_micros: Option<u64>,
    /// The largest certificate taken, in bytes.
    pub proof_limit: usize,
}

/// A member of the Portfolio: a Solver_Configuration by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    /// The prover, as `why3find.json` names it.
    pub prover: String,
    pub seed: Option<u64>,
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
    /// Jobs of a prover call on the Cluster. A placeholder for the per-goal
    /// deadline `<TBD-3>`.
    pub job_margin: Duration,
    /// Without a message from the Coordinator for longer than this, the
    /// Session falls back to local proving; and how often it sends its own
    /// heartbeat. Working values, not proposals for `<TBD-5>`.
    pub silence_limit: Duration,
    pub beat_period: Duration,
    /// When set, one JSON line per labelled run is appended to this file:
    /// raw measurements for the evaluation (R6.3).
    pub record: Option<PathBuf>,
    /// Number of Jobs, each with its own solver seed, that race on one prover
    /// call on the Cluster (R2.2). One means no racing.
    pub portfolio_seeds: u64,
    /// The Portfolio: the Solver_Configurations that race on every prover
    /// call that goes to the Cluster. Empty: the seeds above race.
    pub portfolio: Vec<Member>,
    /// The first sequential pass goes to the Cluster too.
    pub fast_remote: bool,
    /// The call's own prover also runs here as a member of its race.
    pub local_racer: bool,
    /// Certification is on, with this checker (R5.1).
    pub certification: Option<Certification>,
    /// Every reported proof needs a certificate, also one of an
    /// Unpatched_Solver (R5.5). Only with certification.
    pub certify_all: bool,
    /// When set, every Printed_Goal that a prover call runs on is written to
    /// this directory, named by its Task_Hash.
    pub export: Option<PathBuf>,
    /// Where the event trace goes and how much of it is kept (R1.5 criteria
    /// 18 and 19).
    pub trace: Option<(PathBuf, Retention)>,
    /// The hash of the Lock_File, for the member `dp-lock` of the proof
    /// files (R7.7 criterion 5).
    pub lock_hash: Hash,
    /// Measurement for the corpus (R6.1): when set, every prover of the
    /// project is also run on every Goal that gets a prover call, with this
    /// reference time limit, and the outcome goes to the record file. These
    /// runs never reach the machine and decide nothing.
    pub matrix: Option<Duration>,
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

fn file_hash(path: &Path) -> std::io::Result<Hash> {
    Ok(hash::sha256(&fs::read(path)?))
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

struct Prover {
    info: ProverInfo,
    exe: PathBuf,
    /// The first word of its command line.
    program: String,
    /// Hash of the executable at Session start, equal to its Lock_File entry.
    pinned: Hash,
    /// The pinned version of the Lock_File entry.
    version: String,
    /// Interned prover name and pinned version, as dp-core compares them.
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
    /// The tactic steps from the root Goal.
    steps: Vec<(String, u32)>,
    /// The non-proof outcomes of the runs on the Goal, for its final label.
    outcomes: Vec<Outcome>,
    /// The replay fields of the run that proved the Goal.
    replay: Option<ReplayFields>,
    /// The runs on the Goal that answered Invalid.
    invalid: Vec<InvalidEntry>,
    /// The entry of the Certificate_Store for the label certified.
    certificate: Option<Hash>,
}

/// A prover call of why3find that has not been answered.
struct Call {
    rid: String,
    gid: String,
    prover: String,
    step: StrategyStep,
    /// The reference time limit.
    micros: u64,
}

struct LocalRun {
    task_hash: Hash,
    binary_hash: Hash,
    mismatch: bool,
    velocity: u64,
    local_micros: u64,
}

/// Where a finished run came from, with what a cache entry needs of it.
enum Source {
    Local { binary_hash: Hash, mismatch: bool, velocity: u64, local_micros: u64 },
    Worker { seed: u64, signed: SignedResult, facts: RemoteFacts, prover_id: u64 },
    /// A Result_Cache entry; nothing is stored again.
    Cache { binary_hash: Hash, place: Place, seed: Option<u64>, velocity: Option<u64>, local_micros: Option<u64> },
}

/// A finished run, from the local machine, a Worker or the Result_Cache.
struct Finished {
    call: CallId,
    /// The prover that ran, as why3find names it: the call's prover, or the
    /// prover of the Portfolio member whose Job this is.
    prover: String,
    source: Source,
    task_hash: Hash,
    status: Option<u32>,
    timed_out: bool,
    output: Vec<u8>,
    /// Time of the run in reference microseconds.
    micros: u64,
}

/// A finished run whose answer the driver is reading.
struct Awaiting {
    finished: Finished,
    asked: Instant,
}

/// One Session, over the environment `E` and the why3find `W`.
pub struct Session<E: Env, W: Why3find> {
    pub env: E,
    pub bridge: W,
    lock: LockFile,
    platform: String,
    dir: PathBuf,
    lock_hash: Hash,
    cluster: Option<(Peer, Registry)>,
    operator: Option<ed25519_dalek::VerifyingKey>,
    /// The Worker registry of the cluster option. It is kept when the
    /// Coordinator is not reachable: stored Results of Workers are still
    /// checked against it.
    registry: Option<Registry>,
    /// Which start of the Coordinator the Session is connected to.
    incarnation: Option<u64>,
    /// Workers that the operator revoked: none of their Results is
    /// accepted from now on, from the Cluster or from the Result_Cache.
    revoked: BTreeSet<String>,
    session_id: String,
    /// The state of the Front_End machine; `None` only inside a step.
    state: Option<State>,
    /// Inputs waiting for the machine, oldest first.
    inputs: VecDeque<(Input, Option<Finished>)>,
    trace: TraceWriter,
    provers: Option<BTreeMap<String, Prover>>,
    names: BTreeMap<String, u64>,
    goals: BTreeMap<String, Goal>,
    call_ids: BTreeMap<String, CallId>,
    calls: BTreeMap<CallId, Call>,
    prints: BTreeMap<(String, String), (Printed, Hash)>,
    local: BTreeMap<CallId, LocalRun>,
    /// Open Jobs on the Cluster, by Job identifier.
    jobs: BTreeMap<String, (CallId, u64)>,
    /// Finished runs by the identifier of their open `read-answer` request.
    awaiting: BTreeMap<String, Awaiting>,
    /// (goal, prover) pairs whose Result was labelled proved-standalone;
    /// shared with the bridge's responder for cache writes.
    standalone: Rc<RefCell<BTreeSet<(String, String)>>>,
    use_cache: bool,
    record: Option<fs::File>,
    export: Option<PathBuf>,
    /// Wall-clock time by activity, for the summary line.
    spent: BTreeMap<&'static str, Duration>,
    counts: BTreeMap<String, u64>,
    cache_hits: u64,
    remote_proofs: u64,
    /// When the machine last got a tick, and how often it gets one.
    last_tick: u64,
    tick_period: u64,
    started: bool,
    matrix: Matrix,
    portfolio: Vec<Member>,
    certification: Option<Certification>,
    /// The certifications that are under way.
    certs: Vec<Cert>,
    cert_runs: u64,
    /// Certificates by where they came from: a Worker, this machine, or
    /// the Certificate_Store of an earlier Session.
    cert_sources: BTreeMap<&'static str, u64>,
    job_margin: u64,
    /// Store entries of certificates whose check the machine has not
    /// labelled yet, by call.
    stored: BTreeMap<CallId, Hash>,
}

/// Identifiers of the runs of certification start here.
const CERT_BASE: u64 = 1 << 41;

/// Where a certification is.
enum CertStage {
    /// The proof producer runs: on a Worker (`job`, until `deadline`) or on
    /// this machine (`run`).
    Producing { run: Option<u64>, job: Option<String>, deadline: u64 },
    /// The checker runs on the certificate.
    Checking { run: u64, proof: Vec<u8> },
}

/// One certification of the Goal of a call.
struct Cert {
    call: CallId,
    gid: String,
    /// The proof producer, as why3find names it, and its executable.
    prover: String,
    exe: PathBuf,
    program: String,
    /// The Front_End's print of the Goal for that prover: the reference.
    goal: Vec<u8>,
    extension: String,
    task_hash: Hash,
    driver: Vec<u8>,
    /// The reference time limit of the call.
    micros: u64,
    stage: CertStage,
    facts: verdict::CheckFacts,
}

/// Identifiers of measurement runs start here; calls are numbered from 0.
const MEASURE_BASE: u64 = 1 << 40;

/// One measurement run.
struct Measure {
    gid: String,
    prover: String,
    task_hash: Hash,
    velocity: u64,
}

/// The measurement runs of the corpus mode.
#[derive(Default)]
struct Matrix {
    /// The reference limit in microseconds; `None`: no measurement.
    limit: Option<u64>,
    jobs: usize,
    measured: BTreeSet<String>,
    queue: VecDeque<(String, String)>,
    running: BTreeMap<u64, Measure>,
    reading: BTreeMap<String, (Measure, u64)>,
    next: u64,
    /// Number of measurement runs of each Goal that have not ended.
    open: BTreeMap<String, usize>,
    /// Answers to why3find that wait for the measurement of their Goal:
    /// why3find must stay until the driver has read every run.
    held: Vec<(String, ToWhy3find)>,
}

impl<E: Env, W: Why3find> Session<E, W> {
    fn now(&self) -> u64 {
        self.env.now()
    }

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
            let (exe, actual) = self
                .env
                .solver_binary(&program)
                .map_err(|e| SessionError::Pin(format!("{}: {program}: {e}", info.prover)))?;
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
                program,
                pinned: actual,
                name_id: self.intern(&format!("prover:{name}")),
                version_id: self.intern(&format!("version:{version}")),
                version,
                velocity: None,
                info,
            };
            provers.insert(prover.info.prover.clone(), prover);
        }
        for member in &self.portfolio {
            if !provers.keys().any(|p: &String| p.split_once('@').map_or(p.as_str(), |(n, _)| n) == member.prover) {
                return Err(SessionError::Unsupported(format!(
                    "portfolio member {}: {} is not a prover of the project",
                    member.name, member.prover
                )));
            }
        }
        self.provers = Some(provers);
        Ok(())
    }

    /// What Job number `k` of a call runs: the prover, as why3find names
    /// it, and the seed. Without a Portfolio the Jobs are seeds of the
    /// call's prover, and number 0 is the seed of the prover configuration.
    fn member(&self, call_prover: &str, k: u64) -> (String, Option<u64>) {
        match self.portfolio.get(k as usize) {
            Some(member) => {
                let prover = self
                    .provers
                    .as_ref()
                    .and_then(|ps| {
                        ps.keys().find(|p| p.split_once('@').map_or(p.as_str(), |(n, _)| n) == member.prover)
                    })
                    .cloned()
                    .unwrap_or_else(|| member.prover.clone());
                (prover, member.seed)
            }
            None => (call_prover.to_string(), (k != 0).then_some(k)),
        }
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

    /// The solver entries of the Lock_File of the given kinds, as dp-core
    /// reads them.
    fn pins(&mut self, kinds: &[Kind]) -> Vec<UnpatchedSolver> {
        let mut pins = Vec::new();
        for kind in kinds {
            for (tool, hash) in self.lock.pins(*kind, &self.platform) {
                pins.push((tool.prover.clone().unwrap_or_default(), tool.version.clone(), hash));
            }
        }
        pins.into_iter()
            .map(|(prover, version, hash)| UnpatchedSolver {
                prover: self.intern(&format!("prover:{prover}")),
                version: self.intern(&format!("version:{version}")),
                binary_hash: core_hash(hash),
            })
            .collect()
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

    /// The facts of a run for dp-core: the fields of its Result and what the
    /// Session knows locally about its call.
    fn evidence(
        &mut self,
        call: CallId,
        answer: ProverAnswer,
        task_hash: Hash,
        result_prover: &str,
        binary_hash: Hash,
        mismatch: bool,
    ) -> Result<Evidence, SessionError> {
        let (gid, prover) = {
            let c = &self.calls[&call];
            (c.gid.clone(), c.prover.clone())
        };
        // The local print for the prover that ran, when the project has it:
        // a Portfolio member may be another prover than the call's.
        let printer = if self.provers.as_ref().is_some_and(|p| p.contains_key(result_prover)) {
            result_prover.to_string()
        } else {
            prover
        };
        let (printed, local_task_hash) = self.print_local(&gid, &printer)?;
        // The prover and version of the Result's Solver_Configuration: the
        // pinned ones of the prover that the Result names, if the project
        // has it.
        let ids = self.provers.as_ref().and_then(|p| p.get(result_prover)).map(|p| (p.name_id, p.version_id));
        let (name_id, version_id) = match ids {
            Some(ids) => ids,
            None => (self.intern(&format!("unknown-prover:{result_prover}")), u64::MAX),
        };
        let empty = hash::imports_hash(&[]);
        Ok(Evidence {
            fields: ResultFields {
                answer,
                task_hash: core_hash(task_hash),
                partition: false,
                imports: 0,
                imports_hash: core_hash(empty),
                prover: name_id,
                version: version_id,
                binary_hash: core_hash(binary_hash),
            },
            local_task_hash: core_hash(local_task_hash),
            task_count: u64::from(printed.tasks),
            binary_mismatch: mismatch,
        })
    }

    // ---- The Result_Cache (R7.5) -------------------------------------------

    /// The lookup fields of the run description of a run on the Goal of a
    /// call (R7.5 criterion 1): the call's own prover, or the prover and
    /// seed of a Portfolio member.
    fn lookup_fields(&mut self, call: CallId, prover: &str, seed: Option<u64>) -> Result<LookupFields, SessionError> {
        let (gid, micros) = {
            let c = &self.calls[&call];
            (c.gid.clone(), c.micros)
        };
        let prover = prover.to_string();
        let (_, task_hash) = self.print_local(&gid, &prover)?;
        let goal = &self.goals[&gid];
        let goal_path = dp_formats::sexp::print(&dp_formats::sexp::Sexp::list([
            dp_formats::sexp::Sexp::atom(&goal.file),
            dp_formats::sexp::Sexp::atom(&goal.theory),
            dp_formats::sexp::Sexp::atom(&goal.name),
            dp_formats::sexp::Sexp::atom(&goal.path),
        ]));
        let p = self.prover(&prover)?;
        let name = p.info.prover.split_once('@').map_or(p.info.prover.as_str(), |(n, _)| n);
        Ok(LookupFields {
            goal_path,
            task_hash: core_hash(task_hash),
            driver: p.info.digest.clone().into_bytes(),
            pipeline: Vec::new(),
            prover: name.as_bytes().to_vec(),
            version: p.version.clone().into_bytes(),
            seed,
            micros,
            import_mark: false,
            // The option list is the command line of the prover configuration.
            options: vec![p.info.command.clone().into_bytes()],
        })
    }

    /// Looks the run description of a call up in the Result_Cache. A hit is
    /// an entry whose stored lookup fields are those of the lookup and whose
    /// validation fields pass; it is returned as a cache-hit input, which
    /// dp-core still has to accept and label. Anything else is a miss.
    fn cache_lookup(&mut self, call: CallId) -> Result<Option<(Input, Finished)>, SessionError> {
        if !self.use_cache {
            return Ok(None);
        }
        let call_prover = self.calls[&call].prover.clone();
        if self.portfolio.is_empty() {
            return Ok(self.cache_entry(call, &call_prover, None)?.map(|(input, finished, _)| (input, finished)));
        }
        // With a Portfolio, a call is a race of its members. A stored proof
        // of any member answers it. Stored non-proofs answer it only if
        // every member has one: otherwise the members without one may
        // still prove the Goal, and the race runs.
        let mut members: Vec<(String, Option<u64>)> = Vec::new();
        for k in 0..self.portfolio.len() as u64 {
            let member = self.member(&call_prover, k);
            if !members.contains(&member) {
                members.push(member);
            }
        }
        let mut hits = Vec::new();
        for (prover, seed) in &members {
            if let Some(hit) = self.cache_entry(call, prover, *seed)? {
                if hit.2 == DriverAnswer::Valid {
                    return Ok(Some((hit.0, hit.1)));
                }
                hits.push(hit);
            }
        }
        if hits.len() == members.len() {
            return Ok(hits.into_iter().next().map(|(input, finished, _)| (input, finished)));
        }
        Ok(None)
    }

    /// The Result_Cache entry of one run description, as a cache-hit input,
    /// with the answer that it stores.
    fn cache_entry(
        &mut self,
        call: CallId,
        prover: &str,
        seed: Option<u64>,
    ) -> Result<Option<(Input, Finished, DriverAnswer)>, SessionError> {
        let prover = prover.to_string();
        let wanted = cache_key::encode_key(&self.lookup_fields(call, &prover, seed)?);
        let path = result_cache::file(&self.dir, &formats_atoms(&wanted));
        let miss = |reason: &str| {
            eprintln!("dp-frontend: Result_Cache entry {} is a miss: {reason}", path.display());
        };
        let entry = match result_cache::read(&self.dir, &wanted) {
            result_cache::Read::Absent => return Ok(None),
            result_cache::Read::Miss(_, reason) => {
                miss(&reason);
                return Ok(None);
            }
            result_cache::Read::Entry(entry) => entry,
        };
        let (gid, micros) = {
            let c = &self.calls[&call];
            (c.gid.clone(), c.micros)
        };
        let (name_id, version_id) = {
            let p = self.prover(&prover)?;
            (p.name_id, p.version_id)
        };
        let empty = hash::imports_hash(&[]);
        let facts = ValidationFacts {
            prover: name_id,
            version: version_id,
            binary_hash: core_hash(entry.binary_hash),
            velocity: entry.velocity,
            local_micros: entry.local_micros,
            micros,
            imports: 0,
            imports_hash: core_hash(entry.imports_hash),
            recomputed_imports_hash: core_hash(empty),
            local: entry.origin == Origin::Local,
        };
        let pins = self.pins(&[Kind::UnpatchedSolver, Kind::SharingSolver]);
        match cache_key::validate_entry(&facts, &pins) {
            Validation::Valid => {}
            Validation::BinaryMismatch => {
                miss("binary mismatch");
                return Ok(None);
            }
            Validation::LimitMismatch => {
                miss("local time limit");
                return Ok(None);
            }
            Validation::ImportsMismatch => {
                miss("import-set hash");
                return Ok(None);
            }
        }
        let (_, local_task_hash) = self.print_local(&gid, &prover)?;
        // A stored Worker Result is checked again with the registry of this
        // Session (criterion 4).
        let (task_hash, result_prover, remote) = match &entry.origin {
            Origin::Local => (local_task_hash, prover.clone(), None),
            Origin::Worker(signed) => {
                let facts =
                    accept::stored_result_facts(signed, self.registry.as_ref(), &self.revoked, &prover, seed, &entry);
                (signed.fields.task_hash, signed.fields.prover.clone(), Some(facts))
            }
        };
        let evidence =
            self.evidence(call, core_answer(entry.answer), task_hash, &result_prover, entry.binary_hash, false)?;
        let finished = Finished {
            call,
            prover: prover.clone(),
            // Where the stored run took place, for the replay fields.
            source: match &entry.origin {
                Origin::Local => Source::Cache {
                    binary_hash: entry.binary_hash,
                    place: Place::Local,
                    seed: None,
                    velocity: Some(entry.velocity),
                    local_micros: Some(entry.local_micros),
                },
                Origin::Worker(signed) => Source::Cache {
                    binary_hash: entry.binary_hash,
                    place: Place::Worker(signed.fields.worker.clone()),
                    seed: signed.fields.seed,
                    velocity: Some(signed.fields.velocity),
                    local_micros: Some(signed.fields.local_micros),
                },
            },
            task_hash,
            status: None,
            timed_out: false,
            output: Vec::new(),
            micros: entry.micros,
        };
        Ok(Some((Input::CacheHit { now: self.now(), rid: call, evidence, remote }, finished, entry.answer)))
    }

    /// Stores the Result of a labelled run (R7.5 criterion 3).
    fn cache_store(&mut self, finished: &Finished, answer: ProverAnswer) -> Result<(), SessionError> {
        // A failed run says nothing that a later Session could reuse.
        if !self.use_cache || matches!(answer, ProverAnswer::Failure | ProverAnswer::HighFailure) {
            return Ok(());
        }
        // The run description is that of the run: the prover that ran,
        // and the seed of its Solver_Configuration.
        let seed = match &finished.source {
            Source::Worker { signed, .. } => signed.fields.seed,
            _ => None,
        };
        let (binary_hash, velocity, local_micros, origin) = match &finished.source {
            Source::Cache { .. } => return Ok(()),
            Source::Local { binary_hash, mismatch, velocity, local_micros } => {
                if *mismatch {
                    return Ok(());
                }
                (*binary_hash, *velocity, *local_micros, Origin::Local)
            }
            // The Calibration and the local limit that the Worker signed.
            Source::Worker { signed, .. } => (
                signed.fields.binary_hash,
                signed.fields.velocity,
                signed.fields.local_micros,
                Origin::Worker(signed.clone()),
            ),
        };
        if !self.calls.contains_key(&finished.call) {
            return Ok(());
        }
        let lookup =
            formats_atoms(&cache_key::encode_key(&self.lookup_fields(finished.call, &finished.prover, seed)?));
        let entry = Entry {
            lookup,
            binary_hash,
            velocity,
            local_micros,
            imports_hash: hash::imports_hash(&[]),
            answer: driver_answer(answer),
            micros: finished.micros,
            origin,
        };
        result_cache::write(&self.dir, &entry)?;
        Ok(())
    }

    // ---- Effects ------------------------------------------------------------

    fn start_local(&mut self, call: CallId) -> Result<(), SessionError> {
        let (gid, prover_name, micros) = {
            let c = &self.calls[&call];
            (c.gid.clone(), c.prover.clone(), c.micros)
        };
        let (printed, task_hash) = self.print_local(&gid, &prover_name)?;
        let velocity = self.velocity(&prover_name)?;
        let prover = self.prover(&prover_name)?;
        let (exe, program, pinned) = (prover.exe.clone(), prover.program.clone(), prover.pinned);
        let spec = RunSpec::solver(
            prover.info.command.clone(),
            printed.bytes.clone(),
            prover.info.extension.clone(),
            calibration::scale(micros, velocity).max(1),
        );
        // R7.7 criterion 7: the solver binary is checked just before it
        // starts (the environment reads it again only if the file changed).
        let (_, binary_hash) = self.env.solver_binary(&program)?;
        let mismatch = binary_hash != pinned;
        let local_micros = calibration::scale(micros, velocity);
        self.env.start(call, &exe, &spec)?;
        let prover = self.prover(&prover_name)?;
        if let Some(dir) = &self.export {
            let file = dir.join(format!("{}{}", task_hash.to_hex(), prover.info.extension));
            if !file.exists() {
                fs::write(file, &printed.bytes)?;
            }
        }
        self.local.insert(call, LocalRun { task_hash, binary_hash, mismatch, velocity, local_micros });
        Ok(())
    }

    fn submit(&mut self, call: CallId, seed: u64) -> Result<(), SessionError> {
        let (gid, prover_name, micros) = {
            let c = &self.calls[&call];
            (c.gid.clone(), c.prover.clone(), c.micros)
        };
        // `seed` is the number of the Job in its race.
        let (prover_name, solver_seed) = self.member(&prover_name, seed);
        let (printed, _) = self.print_local(&gid, &prover_name)?;
        if printed.tasks != 1 {
            // This member's driver does not print the Goal as one task.
            let now = self.now();
            self.inputs.push_back((Input::JobNotRun { now, rid: call, seed }, None));
            return Ok(());
        }
        let prover = self.prover(&prover_name)?;
        let job = Job {
            kind: JobKind::Prove,
            job_id: format!("{}/{call}/{seed}", self.session_id),
            prover: prover_name.clone(),
            driver: prover.info.digest.clone().into_bytes(),
            extension: prover.info.extension.clone(),
            micros,
            seed: solver_seed,
            goal: printed.bytes,
        };
        let job_id = job.job_id.clone();
        // A failed send shows up as a closed connection.
        if let Some((peer, _)) = &self.cluster {
            self.env.send(*peer, &Message::Submit(job));
            self.jobs.insert(job_id, (call, seed));
        }
        Ok(())
    }

    fn cluster_lost(&mut self, reason: &str) {
        if let Some((peer, _)) = self.cluster.take() {
            self.env.close(peer);
            eprintln!("dp-frontend: {reason}; falling back to local proving");
            self.jobs.clear();
            let now = self.now();
            self.inputs.push_back((Input::ClusterLost { now }, None));
        }
    }

    fn perform(&mut self, effect: Effect, finished: &Option<Finished>) -> Result<(), SessionError> {
        match effect {
            Effect::Print { rid } => {
                self.load_provers()?;
                let (gid, prover) = {
                    let c = &self.calls[&rid];
                    (c.gid.clone(), c.prover.clone())
                };
                self.prover(&prover)?;
                let (printed, _) = self.print_local(&gid, &prover)?;
                if printed.tasks == 1 {
                    if let Some((input, finished)) = self.cache_lookup(rid)? {
                        self.inputs.push_front((input, Some(finished)));
                        return Ok(());
                    }
                } else {
                    eprintln!(
                        "dp-frontend: goal {gid} with {prover}: the pipeline yields {} tasks; not run",
                        printed.tasks
                    );
                }
                let now = self.now();
                self.inputs.push_front((
                    Input::PrintFacts { now, rid, task_count: u64::from(printed.tasks) },
                    None,
                ));
                Ok(())
            }
            Effect::StartLocal { rid } => self.start_local(rid),
            Effect::Submit { rid, seed } => self.submit(rid, seed),
            Effect::CancelLocal { rid } => {
                if self.local.remove(&rid).is_some() {
                    self.env.kill(rid);
                }
                Ok(())
            }
            Effect::CancelJob { rid, seed } => {
                let job_id = format!("{}/{rid}/{seed}", self.session_id);
                if self.jobs.remove(&job_id).is_some() {
                    if let Some((peer, _)) = &self.cluster {
                        self.env.send(*peer, &Message::Cancel { job_id });
                    }
                }
                Ok(())
            }
            Effect::Certify { rid } => self.cert_begin(rid),
            Effect::Beat => {
                if let Some((peer, _)) = &self.cluster {
                    self.env.send(*peer, &Message::Heartbeat);
                }
                Ok(())
            }
            // The machine has already moved the calls home.
            Effect::LeaveCluster => {
                if let Some((peer, _)) = self.cluster.take() {
                    self.env.close(peer);
                    eprintln!("dp-frontend: the Coordinator is silent; falling back to local proving");
                    self.jobs.clear();
                }
                Ok(())
            }
            Effect::Answer { rid, reply } => {
                // A call that is answered while its Goal is being certified
                // was cancelled.
                self.cert_abort(rid);
                let Some(call) = self.calls.remove(&rid) else { return Ok(()) };
                self.call_ids.remove(&call.rid);
                // The time of the run that settled the call, if this step
                // concluded one.
                let spent = finished.as_ref().filter(|f| f.call == rid).map_or(0, |f| f.micros);
                let result = match reply {
                    Reply::Valid => ProverResult::Valid { micros: spent },
                    Reply::Unknown => ProverResult::Unknown { micros: spent },
                    Reply::Timeout => ProverResult::Timeout { micros: call.micros },
                    Reply::Failed => ProverResult::Failed,
                    Reply::NoResult => ProverResult::NoResult,
                };
                let answer = ToWhy3find::Answer { rid: call.rid, result };
                if self.matrix.open.get(&call.gid).is_some_and(|n| *n > 0) {
                    self.matrix.held.push((call.gid, answer));
                    return Ok(());
                }
                Ok(self.bridge.send(&answer)?)
            }
        }
    }

    // ---- Certification (R5) -------------------------------------------------
    //
    // A certificate is produced by cvc5, on a Worker when there is a Cluster
    // and otherwise here, and then checked here by the pinned checker with
    // the Front_End's own print of the Goal as the reference. Both runs go
    // on beside the solver runs. Nothing of what a Worker sends is trusted:
    // a wrong or forged proof fails the check. dp-core gives the label from
    // the facts that this code establishes.

    fn cert_finish(&mut self, cert: Cert) {
        let now = self.now();
        self.inputs.push_back((Input::Certificate { now, rid: cert.call, facts: cert.facts }, None));
    }

    fn cert_index(&self, task_hash: Hash) -> PathBuf {
        self.dir.join(".dp-certs").join("by-task").join(task_hash.to_hex())
    }

    /// Starts the certification of the Goal of a call.
    fn cert_begin(&mut self, call: CallId) -> Result<(), SessionError> {
        let facts = verdict::CheckFacts {
            produced: false,
            checker_pinned: false,
            signature_pinned: false,
            bound_to_local_print: false,
            accepted: false,
            complete: false,
            stored: false,
        };
        let now = self.now();
        let refuse = |session: &mut Self| {
            session.inputs.push_back((Input::Certificate { now, rid: call, facts }, None));
            Ok(())
        };
        let Some((gid, micros)) = self.calls.get(&call).map(|c| (c.gid.clone(), c.micros)) else {
            return refuse(self);
        };
        // The proof producer is the project's cvc5; it is not trusted.
        self.load_provers()?;
        let producer = self.provers.as_ref().expect("loaded").values().find(|p| {
            p.info.prover.split_once('@').map_or(p.info.prover.as_str(), |(n, _)| n) == "cvc5"
        });
        let Some(producer) = producer else {
            eprintln!("dp-frontend: goal {gid}: no certificate, cvc5 is not a prover of the project");
            return refuse(self);
        };
        let (prover, exe, program, extension, driver) = (
            producer.info.prover.clone(),
            producer.exe.clone(),
            producer.program.clone(),
            producer.info.extension.clone(),
            producer.info.digest.clone().into_bytes(),
        );
        // The certifying print: the producer's Driver after the pipeline of
        // certification. It is this Session's own print, as every reference.
        let t = Instant::now();
        let printed = match self.bridge.print(&gid, &prover, &dp_cert::pipeline()) {
            Ok(printed) => printed,
            // Why3 cannot apply the pipeline to this Goal: no certificate,
            // and the Session goes on.
            Err(BridgeError::Refused(reason)) => {
                eprintln!("dp-frontend: goal {gid}: no certificate, the certifying print failed: {reason}");
                return refuse(self);
            }
            Err(e) => return Err(e.into()),
        };
        *self.spent.entry("print").or_default() += t.elapsed();
        let task_hash = hash::task_hash(&printed.bytes, &driver, &dp_cert::pipeline_hash());
        if printed.tasks != 1 {
            return refuse(self);
        }
        let mut cert = Cert {
            call,
            gid,
            prover,
            exe,
            program,
            goal: printed.bytes,
            extension,
            task_hash,
            driver,
            micros: micros.max(1_000_000),
            stage: CertStage::Producing { run: None, job: None, deadline: 0 },
            // The reference of the check is this print, and nothing else.
            facts: verdict::CheckFacts { bound_to_local_print: true, ..facts },
        };
        // A certificate that an earlier Session stored for this very print
        // saves the production, not the check: the store is not trusted.
        let stored = fs::read_to_string(self.cert_index(task_hash))
            .ok()
            .and_then(|hex| dp_formats::cluster::from_hex::<32>(&hex))
            .and_then(|content| dp_cert::Certificate::load(&self.dir.join(".dp-certs"), Hash(content)).ok())
            .filter(|stored| stored.goal == cert.goal && stored.prover == cert.prover);
        match stored {
            Some(stored) => {
                *self.cert_sources.entry("from the store").or_insert(0) += 1;
                self.cert_check(cert, stored.proof)
            }
            None => {
                self.cert_produce(&mut cert, self.cluster.is_some())?;
                self.certs.push(cert);
                Ok(())
            }
        }
    }

    /// Starts the proof producer: on a Worker, or on this machine.
    fn cert_produce(&mut self, cert: &mut Cert, on_cluster: bool) -> Result<(), SessionError> {
        if let (true, Some((peer, _))) = (on_cluster, &self.cluster) {
            let job = Job {
                kind: JobKind::Certify,
                job_id: format!("{}/cert/{}", self.session_id, cert.call),
                prover: cert.prover.clone(),
                driver: cert.driver.clone(),
                extension: cert.extension.clone(),
                micros: cert.micros,
                seed: None,
                goal: cert.goal.clone(),
            };
            let job_id = job.job_id.clone();
            self.env.send(*peer, &Message::Submit(job));
            let deadline = self.now().saturating_add(2 * cert.micros).saturating_add(self.job_margin);
            cert.stage = CertStage::Producing { run: None, job: Some(job_id), deadline };
            return Ok(());
        }
        self.cert_runs += 1;
        let run = CERT_BASE + self.cert_runs;
        let velocity = self.velocity(&cert.prover)?;
        let mut spec = RunSpec::solver(
            dp_cert::proof_command(&cert.program),
            cert.goal.clone(),
            cert.extension.clone(),
            calibration::scale(cert.micros, velocity).max(1),
        );
        spec.output_limit = self.certification.as_ref().map_or(dp_cert::PROOF_LIMIT as usize, |c| c.proof_limit);
        self.env.start(run, &cert.exe, &spec)?;
        cert.stage = CertStage::Producing { run: Some(run), job: None, deadline: 0 };
        Ok(())
    }

    /// The proof producer answered: its proof goes to the checker.
    fn cert_produced(&mut self, cert: Cert, output: Option<Vec<u8>>) -> Result<(), SessionError> {
        // A proof at the size limit may be cut short; it is not taken.
        let limit = self.certification.as_ref().map_or(dp_cert::PROOF_LIMIT as usize, |c| c.proof_limit);
        match output.as_deref().filter(|o| o.len() < limit).and_then(dp_cert::extract_proof) {
            Some(proof) => self.cert_check(cert, proof),
            None => {
                let said = String::from_utf8_lossy(output.as_deref().unwrap_or_default());
                let said: String = said.trim().chars().take(200).collect();
                eprintln!(
                    "dp-frontend: goal {}: {} produced no certificate within the limits: {said:?}",
                    cert.gid, cert.prover
                );
                self.cert_finish(cert);
                Ok(())
            }
        }
    }

    /// Starts the pinned checker on a certificate.
    fn cert_check(&mut self, mut cert: Cert, proof: Vec<u8>) -> Result<(), SessionError> {
        let Some(certification) = self.certification.clone() else {
            self.cert_finish(cert);
            return Ok(());
        };
        cert.facts.produced = true;
        cert.facts.complete = !dp_cert::has_trusted_step(&proof);
        // The checker binary is hashed as it is about to run.
        let Ok((exe, checker_hash)) = self.env.solver_binary(&certification.ethos) else {
            self.cert_finish(cert);
            return Ok(());
        };
        let pinned = |kind: Kind, hash: Hash| self.lock.pins(kind, &self.platform).any(|(_, pin)| pin == hash);
        cert.facts.checker_pinned = pinned(Kind::Checker, checker_hash);
        cert.facts.signature_pinned = pinned(Kind::CheckerSignature, certification.signature_hash);
        self.cert_runs += 1;
        let run = CERT_BASE + self.cert_runs;
        let spec = RunSpec {
            command: dp_cert::check_command(&certification.ethos, &certification.signature),
            input: proof.clone(),
            extension: ".cpc".to_string(),
            // Checking a proof is slower than finding it for some rules.
            micros: certification
                .check_micros
                .unwrap_or_else(|| cert.micros.saturating_mul(10).saturating_add(10_000_000)),
            reference: Some((cert.goal.clone(), cert.extension.clone())),
            output_limit: 1 << 16,
        };
        self.env.start(run, &exe, &spec)?;
        cert.stage = CertStage::Checking { run, proof };
        self.stored.insert(cert.call, checker_hash);
        self.certs.push(cert);
        Ok(())
    }

    /// A run of certification ended.
    fn cert_run_ended(&mut self, run: u64, done: dp_shell::env::RunEnd) -> Result<(), SessionError> {
        let Some(at) = self.certs.iter().position(|c| match &c.stage {
            CertStage::Producing { run: r, .. } => *r == Some(run),
            CertStage::Checking { run: r, .. } => *r == run,
        }) else {
            return Ok(());
        };
        let mut cert = self.certs.remove(at);
        match std::mem::replace(&mut cert.stage, CertStage::Producing { run: None, job: None, deadline: 0 }) {
            CertStage::Producing { .. } => {
                *self.cert_sources.entry("produced here").or_insert(0) += 1;
                self.cert_produced(cert, (!done.timed_out).then_some(done.output))
            }
            CertStage::Checking { proof, .. } => {
                let checker_hash = self.stored.remove(&cert.call);
                cert.facts.accepted = !done.timed_out && dp_cert::accepted(done.status, &done.output);
                if !cert.facts.accepted {
                    // Why the checker did not accept, in its own words.
                    let said = String::from_utf8_lossy(&done.output);
                    let said: String = said.trim().chars().take(300).collect();
                    eprintln!(
                        "dp-frontend: goal {}: the checker did not accept the certificate of {}: {}",
                        cert.gid,
                        cert.prover,
                        if done.timed_out { "its time limit ended".to_string() } else { format!("{said:?}") }
                    );
                }
                if let (true, Some(checker_hash), Some(certification)) =
                    (cert.facts.accepted, checker_hash, self.certification.clone())
                {
                    let certificate = dp_cert::Certificate {
                        goal: cert.goal.clone(),
                        extension: cert.extension.clone(),
                        proof,
                        task_hash: cert.task_hash,
                        driver: cert.driver.clone(),
                        prover: cert.prover.clone(),
                        checker_hash,
                        signature_hash: certification.signature_hash,
                    };
                    match certificate.store(&self.dir.join(".dp-certs")) {
                        Ok(content) => {
                            cert.facts.stored = true;
                            self.stored.insert(cert.call, content);
                            // For later Sessions: the certificate of this print.
                            let _ = store::write_atomic(&self.cert_index(cert.task_hash), content.to_hex().as_bytes());
                        }
                        Err(e) => {
                            eprintln!("dp-frontend: goal {}: the certificate could not be stored: {e}", cert.gid)
                        }
                    }
                }
                self.cert_finish(cert);
                Ok(())
            }
        }
    }

    /// The Cluster answered a proof Job, or will not: `output` is what the
    /// producer printed, if the Job ran. Without it the producer runs here.
    fn cert_job_ended(&mut self, job_id: &str, output: Option<Vec<u8>>) -> Result<bool, SessionError> {
        let Some(at) = self.certs.iter().position(
            |c| matches!(&c.stage, CertStage::Producing { job: Some(j), .. } if j == job_id),
        ) else {
            return Ok(false);
        };
        let mut cert = self.certs.remove(at);
        match output {
            Some(output) => {
                *self.cert_sources.entry("from Workers").or_insert(0) += 1;
                self.cert_produced(cert, Some(output))?
            }
            None => {
                self.cert_produce(&mut cert, false)?;
                self.certs.push(cert);
            }
        }
        Ok(true)
    }

    /// Proof Jobs that the Cluster will not answer in time, or at all, are
    /// run on this machine instead.
    fn cert_fallback(&mut self, all: bool) -> Result<(), SessionError> {
        let now = self.now();
        let late: Vec<String> = self
            .certs
            .iter()
            .filter_map(|c| match &c.stage {
                CertStage::Producing { job: Some(job), deadline, .. } if all || now >= *deadline => Some(job.clone()),
                _ => None,
            })
            .collect();
        for job_id in late {
            if let Some((peer, _)) = &self.cluster {
                self.env.send(*peer, &Message::Cancel { job_id: job_id.clone() });
            }
            self.cert_job_ended(&job_id, None)?;
        }
        Ok(())
    }

    /// The call of a certification was answered without it: stop its runs.
    fn cert_abort(&mut self, call: CallId) {
        let Some(at) = self.certs.iter().position(|c| c.call == call) else { return };
        let cert = self.certs.remove(at);
        match cert.stage {
            CertStage::Producing { run, job, .. } => {
                if let Some(run) = run {
                    self.env.kill(run);
                }
                if let (Some(job_id), Some((peer, _))) = (job, &self.cluster) {
                    self.env.send(*peer, &Message::Cancel { job_id });
                }
            }
            CertStage::Checking { run, .. } => self.env.kill(run),
        }
        self.stored.remove(&call);
    }

    // ---- Measurement runs (corpus mode) --------------------------------------

    /// Queues one run of every prover on a Goal that gets its first call.
    fn measure(&mut self, gid: &str) -> Result<(), SessionError> {
        if self.matrix.limit.is_none() || !self.matrix.measured.insert(gid.to_string()) {
            return Ok(());
        }
        self.load_provers()?;
        let provers: Vec<String> = self.provers.as_ref().expect("loaded").keys().cloned().collect();
        // Calibrate before any run starts: a Calibration blocks the loop
        // that reaps runs, which would count as time of those runs.
        for prover in &provers {
            self.velocity(prover)?;
        }
        self.matrix.open.insert(gid.to_string(), provers.len());
        for prover in provers {
            self.matrix.queue.push_back((gid.to_string(), prover));
        }
        Ok(())
    }

    fn measure_start(&mut self) -> Result<(), SessionError> {
        let Some(limit) = self.matrix.limit else { return Ok(()) };
        while self.matrix.running.len() < self.matrix.jobs {
            let Some((gid, prover_name)) = self.matrix.queue.pop_front() else { return Ok(()) };
            let (printed, task_hash) = self.print_local(&gid, &prover_name)?;
            let velocity = self.velocity(&prover_name)?;
            let measure = Measure { gid, prover: prover_name.clone(), task_hash, velocity };
            if printed.tasks != 1 {
                self.measure_done(measure, "not-one-task", 0)?;
                continue;
            }
            let prover = self.prover(&prover_name)?;
            let exe = prover.exe.clone();
            let spec = RunSpec::solver(
                prover.info.command.clone(),
                printed.bytes,
                prover.info.extension.clone(),
                calibration::scale(limit, velocity).max(1),
            );
            self.matrix.next += 1;
            let id = MEASURE_BASE + self.matrix.next;
            self.env.start(id, &exe, &spec)?;
            self.matrix.running.insert(id, measure);
        }
        Ok(())
    }

    /// A measurement run has its outcome: one line of the record file.
    fn measure_done(&mut self, measure: Measure, outcome: &str, micros: u64) -> Result<(), SessionError> {
        if let Some(record) = &mut self.record {
            use std::io::Write;
            let goal = &self.goals[&measure.gid];
            writeln!(
                record,
                "{{\"file\":{},\"theory\":{},\"goal\":{},\"path\":{},\"prover\":{},\"seed\":null,\"step\":\"matrix\",\"limit_us\":{},\"time_us\":{},\"where\":\"matrix\",\"label\":{},\"task_hash\":{}}}",
                json_string(&goal.file),
                json_string(&goal.theory),
                json_string(&goal.name),
                json_string(&goal.path),
                json_string(&measure.prover),
                self.matrix.limit.unwrap_or(0),
                micros,
                json_string(outcome),
                json_string(&measure.task_hash.to_hex()),
            )?;
        }
        let open = self.matrix.open.entry(measure.gid.clone()).or_insert(1);
        *open -= 1;
        if *open == 0 {
            let held = std::mem::take(&mut self.matrix.held);
            for (gid, answer) in held {
                if gid == measure.gid {
                    self.bridge.send(&answer)?;
                } else {
                    self.matrix.held.push((gid, answer));
                }
            }
        }
        Ok(())
    }

    /// A measurement run ended: the driver reads its answer.
    fn measure_ended(&mut self, id: u64, done: dp_shell::env::RunEnd) -> Result<(), SessionError> {
        let Some(measure) = self.matrix.running.remove(&id) else { return Ok(()) };
        let micros = (u128::from(done.elapsed_micros) * 1_000_000 / u128::from(measure.velocity.max(1))) as u64;
        if done.timed_out {
            return self.measure_done(measure, "timeout", micros);
        }
        let qid = self.bridge.ask_answer(&measure.prover, done.status.unwrap_or(1), &done.output)?;
        self.matrix.reading.insert(qid, (measure, micros));
        Ok(())
    }

    /// One step of the machine: trace, new state, then the effects.
    fn step(&mut self, input: Input, finished: Option<Finished>) -> Result<(), SessionError> {
        let state = self.state.take().expect("the state is present between steps");
        let out = frontend::step(state, input);
        let now = self.now();
        if self.trace.enabled() {
            self.trace.record(out.action.name(), now, &format!("{input:?}"))?;
        }
        self.state = Some(out.state);
        for entry in &out.log {
            self.logged(*entry, &input, &finished)?;
        }
        // An entry that dp-core did not accept is a miss: the call goes on
        // as if the Result_Cache had nothing (R7.5 criterion 6).
        if let (Input::CacheHit { rid, .. }, dp_core::trace::TlaAction::Discard) = (input, out.action) {
            eprintln!("dp-frontend: a Result_Cache entry was not accepted in this Session; it is a miss");
            self.inputs.push_front((Input::PrintFacts { now, rid, task_count: 1 }, None));
        }
        for effect in out.effects {
            self.perform(effect, &finished)?;
        }
        Ok(())
    }

    /// Acts on what the machine decided about a run: counts, the record
    /// file, the legacy-cache set and the Result_Cache.
    fn logged(&mut self, entry: LogEntry, input: &Input, finished: &Option<Finished>) -> Result<(), SessionError> {
        let (call, name) = match entry {
            LogEntry::Discarded => return Ok(()),
            LogEntry::Proved { call } => (call, "proved-standalone".to_string()),
            LogEntry::Certified { call } => {
                *self.counts.entry("certified".to_string()).or_insert(0) += 1;
                let content = self.stored.remove(&call);
                if let Some(gid) = self.calls.get(&call).map(|c| c.gid.clone()) {
                    if let Some(goal) = self.goals.get_mut(&gid) {
                        goal.certificate = content;
                    }
                }
                return Ok(());
            }
            LogEntry::Lost { call, outcome } => (call, outcome_name(outcome).to_string()),
            LogEntry::Rejected { call, reason } => {
                if let Some(c) = self.calls.get(&call) {
                    eprintln!("dp-frontend: Result of {} for goal {} discarded: {reason:?}", c.prover, c.gid);
                }
                (call, "discarded".to_string())
            }
        };
        *self.counts.entry(name.clone()).or_insert(0) += 1;
        // The outcome of a certificate that gave no proof label is an
        // outcome of its Goal: checked-incomplete when the checker
        // accepted and the certificate has a trusted step or is not stored
        // (R7.2 criterion 5). It comes with no run.
        if let (LogEntry::Lost { outcome, .. }, Input::Certificate { .. }) = (&entry, input) {
            if let Some(gid) = self.calls.get(&call).map(|c| c.gid.clone()) {
                if let Some(goal) = self.goals.get_mut(&gid) {
                    goal.outcomes.push(*outcome);
                }
            }
        }
        let Some(finished) = finished else { return Ok(()) };
        let Some(c) = self.calls.get(&call) else { return Ok(()) };
        let (gid, prover, step, micros) = (c.gid.clone(), c.prover.clone(), c.step, c.micros);
        let answer = match input {
            Input::LocalRun { evidence, .. }
            | Input::RemoteResult { evidence, .. }
            | Input::CacheHit { evidence, .. } => evidence.fields.answer,
            _ => return Ok(()),
        };
        let (place, seed) = match &finished.source {
            Source::Local { .. } => ("local", None),
            Source::Worker { seed, .. } => ("worker", Some(*seed)),
            Source::Cache { .. } => ("cache", None),
        };
        match entry {
            LogEntry::Proved { .. } => {
                self.standalone.borrow_mut().insert((gid.clone(), prover.clone()));
                match finished.source {
                    Source::Worker { .. } => self.remote_proofs += 1,
                    Source::Cache { .. } => self.cache_hits += 1,
                    Source::Local { .. } => {}
                }
                let (binary_hash, place, velocity, local_micros) = match &finished.source {
                    Source::Local { binary_hash, velocity, local_micros, .. } => {
                        (*binary_hash, Place::Local, Some(*velocity), Some(*local_micros))
                    }
                    Source::Worker { signed, .. } => (
                        signed.fields.binary_hash,
                        Place::Worker(signed.fields.worker.clone()),
                        Some(signed.fields.velocity),
                        Some(signed.fields.local_micros),
                    ),
                    Source::Cache { binary_hash, place, velocity, local_micros, .. } => {
                        (*binary_hash, place.clone(), *velocity, *local_micros)
                    }
                };
                let seed = match &finished.source {
                    Source::Cache { seed, .. } => *seed,
                    Source::Worker { signed, .. } => signed.fields.seed,
                    Source::Local { .. } => None,
                };
                let p = self.prover(&finished.prover)?;
                let replay = ReplayFields {
                    task_hash: finished.task_hash,
                    driver: p.info.digest.clone().into_bytes(),
                    prover: p.info.prover.split_once('@').map_or(p.info.prover.as_str(), |(n, _)| n).to_string(),
                    version: p.version.clone(),
                    binary_hash,
                    place,
                    seed,
                    velocity,
                    local_micros,
                    reference_micros: micros,
                };
                if let Some(goal) = self.goals.get_mut(&gid) {
                    goal.replay.get_or_insert(replay);
                }
            }
            LogEntry::Lost { outcome, .. } => {
                if matches!(finished.source, Source::Cache { .. }) {
                    self.cache_hits += 1;
                }
                let invalid = if matches!(outcome, Outcome::StandaloneInvalid | Outcome::DependentInvalid) {
                    let p = self.prover(&finished.prover)?;
                    Some(InvalidEntry {
                        prover: p.info.prover.split_once('@').map_or(p.info.prover.as_str(), |(n, _)| n).to_string(),
                        version: p.version.clone(),
                        binary_hash: match &finished.source {
                            Source::Local { binary_hash, .. } | Source::Cache { binary_hash, .. } => *binary_hash,
                            Source::Worker { signed, .. } => signed.fields.binary_hash,
                        },
                        place: match &finished.source {
                            Source::Local { .. } => Place::Local,
                            Source::Worker { signed, .. } => Place::Worker(signed.fields.worker.clone()),
                            Source::Cache { place, .. } => place.clone(),
                        },
                    })
                } else {
                    None
                };
                if let Some(goal) = self.goals.get_mut(&gid) {
                    goal.outcomes.push(outcome);
                    goal.invalid.extend(invalid);
                }
                if outcome == Outcome::Unchecked {
                    eprintln!(
                        "dp-frontend: {prover} reports a proof of goal {gid}, labelled unchecked \
                         (its binary is not an Unpatched_Solver of the Lock_File)"
                    );
                }
                if outcome == Outcome::Failed {
                    eprintln!(
                        "dp-frontend: {prover} gave no answer on goal {gid}: {:?}",
                        String::from_utf8_lossy(&finished.output).chars().take(300).collect::<String>()
                    );
                }
            }
            _ => {}
        }
        if !matches!(entry, LogEntry::Rejected { .. }) {
            self.cache_store(finished, answer)?;
        }
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
                seed.map_or("null".to_string(), |s| s.to_string()),
                json_string(step.name()),
                micros,
                finished.micros,
                json_string(place),
                json_string(&name),
                json_string(&finished.task_hash.to_hex()),
            )?;
        }
        Ok(())
    }

    // ---- Events -------------------------------------------------------------

    /// A run ended: the answer is read by the pinned Why3 driver, never by
    /// the Worker. The driver is asked without waiting, so other runs keep
    /// being started and reaped meanwhile.
    fn finished(&mut self, finished: Finished) -> Result<(), SessionError> {
        if finished.timed_out {
            return self.answered(finished, ProverAnswer::Timeout);
        }
        if !self.calls.contains_key(&finished.call) {
            return Ok(());
        }
        // A run that died from a signal has no exit status; the driver still
        // reads its output, as a failed run. The driver is that of the
        // prover that ran.
        let status = finished.status.unwrap_or(1);
        let qid = self.bridge.ask_answer(&finished.prover, status, &finished.output)?;
        self.awaiting.insert(qid, Awaiting { finished, asked: Instant::now() });
        Ok(())
    }

    /// The answer of a finished run is known: it becomes an input.
    fn answered(&mut self, finished: Finished, mut answer: ProverAnswer) -> Result<(), SessionError> {
        // Why3 never reads a proof from a run that a signal ended.
        if finished.status.is_none() && answer == ProverAnswer::Valid {
            answer = ProverAnswer::HighFailure;
        }
        let Some(call) = self.calls.get(&finished.call) else { return Ok(()) };
        let call_prover = call.prover.clone();
        let now = self.now();
        let input = match &finished.source {
            Source::Local { binary_hash, mismatch, .. } => {
                let evidence = self.evidence(
                    finished.call,
                    answer,
                    finished.task_hash,
                    &call_prover,
                    *binary_hash,
                    *mismatch,
                )?;
                Input::LocalRun { now, rid: finished.call, evidence }
            }
            Source::Worker { seed, signed, facts, prover_id } => {
                let evidence = self.evidence(
                    finished.call,
                    answer,
                    finished.task_hash,
                    &signed.fields.prover,
                    signed.fields.binary_hash,
                    false,
                )?;
                Input::RemoteResult {
                    now,
                    rid: finished.call,
                    seed: *seed,
                    prover: *prover_id,
                    evidence,
                    signature_valid: facts.signature_valid,
                    registered: facts.registered,
                    revoked: facts.revoked,
                    velocity: signed.fields.velocity,
                    local_micros: signed.fields.local_micros,
                    imports_consistent: facts.imports_consistent,
                }
            }
            Source::Cache { .. } => return Ok(()),
        };
        self.inputs.push_back((input, Some(finished)));
        Ok(())
    }

    /// A Result that the Coordinator forwarded. Everything in it is
    /// untrusted; the shell only computes facts about it.
    fn result(&mut self, signed: SignedResult) -> Result<(), SessionError> {
        let Some(&(call, seed)) = self.jobs.get(&signed.fields.job_id) else {
            // Not an open Job of this Session: late, replayed or invented.
            eprintln!("dp-frontend: Result for {:?} discarded: no such open Job", signed.fields.job_id);
            *self.counts.entry("discarded".into()).or_insert(0) += 1;
            return Ok(());
        };
        let (_, registry) = self.cluster.as_ref().expect("a Result arrives from the Cluster");
        let facts = accept::result_facts(&signed, Some(registry), &self.revoked);
        let authentic = facts.registered && facts.signature_valid && !facts.revoked;
        if !authentic {
            eprintln!(
                "dp-frontend: Result for {:?} discarded: {} (names {:?})",
                signed.fields.job_id,
                if facts.revoked { "its Worker is revoked" } else { "not signed by a registered Worker" },
                signed.fields.worker
            );
            *self.counts.entry("discarded".into()).or_insert(0) += 1;
        } else {
            // An authentic Result ends its Job.
            self.jobs.remove(&signed.fields.job_id);
        }
        // The Job as submitted: its prover and its seed (R1.3 criterion 6).
        // A Result that names another prover or seed is for no Job of this
        // call; for dp-core, a Result for the Job's own Solver_Configuration
        // carries the call's prover.
        let call_prover = self.calls.get(&call).map(|c| c.prover.clone()).unwrap_or_default();
        let (member_prover, member_seed) = self.member(&call_prover, seed);
        let prover_id = if signed.fields.prover == member_prover && signed.fields.seed == member_seed {
            self.intern(&format!("call-prover:{call_prover}"))
        } else {
            u64::MAX
        };
        let f = signed.fields.clone();
        let finished = Finished {
            call,
            prover: f.prover.clone(),
            source: Source::Worker { seed, signed, facts, prover_id },
            task_hash: f.task_hash,
            status: f.status,
            timed_out: f.timed_out,
            output: f.output,
            micros: f.elapsed_micros,
        };
        if authentic {
            self.finished(finished)
        } else {
            // dp-core discards it whatever the answer; the driver is not asked.
            self.answered(finished, ProverAnswer::HighFailure)
        }
    }

    fn reap(&mut self) -> Result<(), SessionError> {
        for (call, done) in self.env.finished()? {
            if call >= CERT_BASE {
                self.cert_run_ended(call, done)?;
                continue;
            }
            if call >= MEASURE_BASE {
                self.measure_ended(call, done)?;
                continue;
            }
            let Some(local) = self.local.remove(&call) else { continue };
            // Report the time in reference units, as why3find does.
            let micros = (u128::from(done.elapsed_micros) * 1_000_000 / u128::from(local.velocity.max(1))) as u64;
            let Some(prover) = self.calls.get(&call).map(|c| c.prover.clone()) else { continue };
            self.finished(Finished {
                call,
                prover,
                source: Source::Local {
                    binary_hash: local.binary_hash,
                    mismatch: local.mismatch,
                    velocity: local.velocity,
                    local_micros: local.local_micros,
                },
                task_hash: local.task_hash,
                status: done.status,
                timed_out: done.timed_out,
                output: done.output,
                micros,
            })?;
        }
        Ok(())
    }

    fn handle(&mut self, msg: FromWhy3find) -> Result<(), SessionError> {
        let now = self.now();
        match msg {
            FromWhy3find::Hello { version: 1 } => Ok(()),
            FromWhy3find::Hello { version } => {
                Err(SessionError::Unsupported(format!("dispatch protocol version {version}")))
            }
            FromWhy3find::Goal { gid, file, theory, name, path, .. } => {
                let id = self.goals.len() as u64;
                let steps: Vec<_> = path.iter().map(|p| (p.tactic.clone(), p.index)).collect();
                let path = path.iter().map(|p| format!("{}.{}", p.tactic, p.index)).collect::<Vec<_>>().join("/");
                self.goals.insert(
                    gid,
                    Goal {
                        id,
                        file,
                        theory,
                        name,
                        path,
                        steps,
                        outcomes: Vec::new(),
                        replay: None,
                        invalid: Vec::new(),
                        certificate: None,
                    },
                );
                self.inputs.push_back((Input::GoalAnnounced { now, goal: id }, None));
                Ok(())
            }
            FromWhy3find::Prove { rid, gid, step, prover, micros } => {
                let Some(goal) = self.goals.get(&gid).map(|g| g.id) else {
                    return Err(SessionError::Bridge(BridgeError::Protocol(format!(
                        "prover call for the unannounced goal {gid}"
                    ))));
                };
                let call = self.intern(&format!("call:{rid}:{now}:{}", self.calls.len()));
                let prover_id = self.intern(&format!("call-prover:{prover}"));
                self.call_ids.insert(rid.clone(), call);
                self.measure(&gid)?;
                self.calls.insert(call, Call { rid, gid, prover, step, micros });
                let pass = if step == StrategyStep::Fast { Pass::Fast } else { Pass::Later };
                self.inputs.push_back((
                    Input::ProverCall { now, rid: call, goal, prover: prover_id, pass, micros },
                    None,
                ));
                Ok(())
            }
            FromWhy3find::Cancel { rid } => {
                if let Some(&call) = self.call_ids.get(&rid) {
                    self.inputs.push_back((Input::CancelCall { now, rid: call }, None));
                }
                Ok(())
            }
            FromWhy3find::AnswerRead { qid, answer } => self.driver_answered(&qid, Some(answer)),
            FromWhy3find::Error { qid, .. }
                if self.awaiting.contains_key(&qid) || self.matrix.reading.contains_key(&qid) =>
            {
                self.driver_answered(&qid, None)
            }
            FromWhy3find::Done => Ok(()),
            other => Err(SessionError::Bridge(BridgeError::Protocol(format!(
                "unexpected message {other:?}"
            )))),
        }
    }

    fn driver_answered(&mut self, qid: &str, answer: Option<DriverAnswer>) -> Result<(), SessionError> {
        if let Some((measure, micros)) = self.matrix.reading.remove(qid) {
            return self.measure_done(measure, answer.map_or("failure", DriverAnswer::name), micros);
        }
        let Some(Awaiting { finished, asked }) = self.awaiting.remove(qid) else {
            return Err(SessionError::Bridge(BridgeError::Protocol(format!("unexpected reply {qid}"))));
        };
        *self.spent.entry("read-answer").or_default() += asked.elapsed();
        self.answered(finished, answer.map_or(ProverAnswer::HighFailure, core_answer))
    }

    fn cluster_events(&mut self) -> Result<(), SessionError> {
        loop {
            let Some(event) = self.env.poll(Duration::ZERO) else { return Ok(()) };
            // What arrives after the Session left the Cluster is dropped.
            let Some(peer) = self.cluster.as_ref().map(|(peer, _)| *peer) else { continue };
            match event {
                NetEvent::Disconnected(p) if p == peer => self.cluster_lost("the Coordinator is gone"),
                NetEvent::Message(p, message) if p == peer => {
                    let now = self.now();
                    self.inputs.push_back((Input::ClusterHeard { now }, None));
                    match message {
                        // The answer to a proof Job is a certificate to
                        // check, whoever sent it.
                        Message::Result(signed) if self.cert_job_ended(&signed.fields.job_id, None::<Vec<u8>>.or(Some(signed.fields.output.clone())))? => {}
                        Message::Result(signed) => self.result(signed)?,
                        Message::Failed { job_id, .. } if self.cert_job_ended(&job_id, None)? => {}
                        Message::Failed { job_id, reason } => {
                            if let Some((call, seed)) = self.jobs.remove(&job_id) {
                                eprintln!("dp-frontend: {job_id} was not run on the Cluster ({reason})");
                                self.inputs.push_back((Input::JobNotRun { now, rid: call, seed }, None));
                            }
                        }
                        Message::Welcome { incarnation } => self.incarnation = Some(incarnation),
                        // A revocation counts only with the operator's
                        // signature; the Coordinator merely passes it on.
                        Message::Revocation(command) => {
                            let signed = self.operator.as_ref().is_some_and(|key| command.verifies(key));
                            if signed && command.operation == dp_formats::cluster::Operation::Revoke {
                                if self.revoked.insert(command.worker.clone()) {
                                    eprintln!("dp-frontend: the operator revoked Worker {:?}", command.worker);
                                }
                                // Later Sessions of the project know it too.
                                if let Err(e) = crate::revocations::add(&self.dir, &command) {
                                    eprintln!("dp-frontend: the revocation could not be stored: {e}");
                                }
                            } else {
                                eprintln!("dp-frontend: a revocation without the operator's signature was ignored");
                            }
                        }
                        // Nothing else is part of the protocol towards a Front_End.
                        _ => {}
                    }
                }
                // Not from the Coordinator of this Session.
                _ => {}
            }
        }
    }

    fn drain(&mut self) -> Result<(), SessionError> {
        while let Some((input, finished)) = self.inputs.pop_front() {
            self.step(input, finished)?;
        }
        Ok(())
    }

    /// One turn of the Session: at most one message of why3find, what
    /// arrived from the Cluster, the solvers that ended, and a tick when
    /// one is due. Returns whether the Session has ended.
    pub fn turn(&mut self, wait: Duration) -> Result<bool, SessionError> {
        if !self.started {
            self.started = true;
            let now = self.now();
            let cluster = self.cluster.is_some();
            self.inputs.push_back((Input::SessionStart { now, cluster }, None));
        }
        self.drain()?;
        match self.bridge.poll(wait)? {
            Poll::Message(msg) => self.handle(msg)?,
            Poll::Idle => {}
            Poll::Closed => {
                let now = self.now();
                self.inputs.push_back((Input::Why3findDone { now }, None));
                self.drain()?;
                return Ok(true);
            }
        }
        self.cluster_events()?;
        // Proof Jobs that are late, or whose Cluster is gone, run here.
        let gone = self.cluster.is_none();
        self.cert_fallback(gone)?;
        self.reap()?;
        self.measure_start()?;
        // Deadlines of races, heartbeats and the Coordinator's silence are
        // checked regularly, while there is a Cluster.
        let now = self.now();
        if self.cluster.is_some() && now - self.last_tick >= self.tick_period {
            self.last_tick = now;
            self.inputs.push_back((Input::Tick { now }, None));
        }
        self.drain()?;
        Ok(false)
    }

    pub fn state(&self) -> &State {
        self.state.as_ref().expect("the state is present between steps")
    }

    /// Which start of the Coordinator this Session is connected to.
    pub fn incarnation(&self) -> Option<u64> {
        self.incarnation
    }

    /// The Workers that the operator revoked, as far as this Session knows.
    pub fn revoked(&self) -> &BTreeSet<String> {
        &self.revoked
    }

    /// The label of every Goal that was announced, by source file, as the
    /// final write records them.
    pub fn labels(&self) -> BTreeMap<String, Vec<GoalLabel>> {
        goal_labels(self.state(), &self.goals)
    }

    /// Starts a Session: connects to the Cluster if one is configured
    /// (R1.1 criterion 5: without a connection, the address is named and
    /// the Session proves locally).
    pub fn start(
        mut env: E,
        bridge: W,
        config: Config,
        standalone: Rc<RefCell<BTreeSet<(String, String)>>>,
    ) -> Result<Session<E, W>, SessionError> {
        let certify = config.certification.is_some();
        let session_id = format!(
            "{:x}-{:x}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
        );
        let operator = config.cluster.as_ref().map(|c| c.operator);
        let registry = config.cluster.as_ref().map(|c| c.registry.clone());
        // Revocations that earlier Sessions of this project saw.
        let revoked = match &operator {
            Some(key) => crate::revocations::load(&config.dir, key),
            None => BTreeSet::new(),
        };
        let cluster = config.cluster.and_then(|c| match env.connect(&c.address) {
            Ok(peer) => {
                env.send(peer, &Message::FrontEndHello { session: session_id.clone() });
                Some((peer, c.registry))
            }
            Err(e) => {
                eprintln!(
                    "dp-frontend: no connection to the Coordinator at {}: {e}; proving locally",
                    c.address
                );
                None
            }
        });
        let trace = match &config.trace {
            Some((path, retention)) => TraceWriter::to_file("front-end", path.clone(), *retention)?,
            None => TraceWriter::off("front-end"),
        };
        let silence = config.silence_limit.as_micros() as u64;
        let beat = config.beat_period.as_micros() as u64;
        let now = env.now();
        let mut session = Session {
            env,
            bridge,
            platform: current_platform(),
            dir: config.dir.clone(),
            lock_hash: config.lock_hash,
            cluster,
            operator,
            registry,
            incarnation: None,
            revoked,
            session_id,
            state: None,
            inputs: VecDeque::new(),
            trace,
            provers: None,
            names: BTreeMap::new(),
            goals: BTreeMap::new(),
            call_ids: BTreeMap::new(),
            calls: BTreeMap::new(),
            prints: BTreeMap::new(),
            local: BTreeMap::new(),
            jobs: BTreeMap::new(),
            awaiting: BTreeMap::new(),
            standalone,
            use_cache: !config.args.iter().any(|a| a == "--no-cache"),
            record: match &config.record {
                Some(path) => Some(fs::OpenOptions::new().create(true).append(true).open(path)?),
                None => None,
            },
            export: config.export.clone(),
            spent: BTreeMap::new(),
            counts: BTreeMap::new(),
            cache_hits: 0,
            remote_proofs: 0,
            last_tick: now,
            // Often enough to see a silence or a due heartbeat in time.
            tick_period: if silence == 0 { 200_000 } else { (beat.min(silence) / 4).clamp(1, 200_000) },
            started: false,
            portfolio: config.portfolio.clone(),
            certification: config.certification.clone(),
            certs: Vec::new(),
            cert_runs: 0,
            cert_sources: BTreeMap::new(),
            job_margin: config.job_margin.as_micros() as u64,
            stored: BTreeMap::new(),
            matrix: Matrix {
                limit: config.matrix.map(|d| d.as_micros() as u64),
                jobs: config.jobs.max(1),
                ..Matrix::default()
            },
            lock: config.lock,
        };
        let unpatched = session.pins(&[Kind::UnpatchedSolver]);
        session.state = Some(frontend::FrontEnd::init(&frontend::Config {
            local_jobs: config.jobs.max(1) as u64,
            seeds: if config.portfolio.is_empty() {
                config.portfolio_seeds.max(1)
            } else {
                config.portfolio.len() as u64
            },
            fast_remote: config.fast_remote,
            local_racer: config.local_racer,
            job_margin: config.job_margin.as_micros() as u64,
            silence_limit: silence,
            beat_period: beat,
            certify,
            certify_all: config.certify_all && certify,
            unpatched,
            empty_imports_hash: core_hash(hash::imports_hash(&[])),
        }));
        Ok(session)
    }
}

fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::StandaloneInvalid | Outcome::DependentInvalid => "invalid",
        Outcome::CheckedIncomplete => "checked-incomplete",
        Outcome::Unchecked => "unchecked",
        Outcome::Timeout => "timeout",
        Outcome::Unknown => "unknown",
        Outcome::Failed => "failed",
        Outcome::Unresolved => "unresolved",
    }
}

/// The label of every announced Goal, by source file.
fn goal_labels(state: &State, goals: &BTreeMap<String, Goal>) -> BTreeMap<String, Vec<GoalLabel>> {
    let imports_hash = hash::imports_hash(&[]);
    let mut files: BTreeMap<String, Vec<GoalLabel>> = BTreeMap::new();
    for goal in goals.values() {
        let recorded = frontend::verdict_of(&state.verdicts, goal.id).map(|v| v.label);
        let label = verdict::final_label(recorded, &goal.outcomes);
        let (name, proof) = match label {
            Label::ProvedStandalone => ("proved-standalone", true),
            Label::Certified => ("certified", true),
            Label::NonProof(outcome) => (outcome_name(outcome), false),
        };
        files.entry(goal.file.clone()).or_default().push(GoalLabel {
            theory: goal.theory.clone(),
            name: goal.name.clone(),
            steps: goal.steps.clone(),
            entry: DpEntry {
                label: name.to_string(),
                replay: if proof { goal.replay.clone().into_iter().collect() } else { Vec::new() },
                imports_hash,
                certificate: if label == Label::Certified { goal.certificate } else { None },
                conflict: verdict::conflict(label, goal.invalid.len() as u64),
                invalid: goal.invalid.clone(),
            },
            proof,
        });
    }
    files
}

/// The final write (R1.5 criterion 14): every proof file that why3find
/// wrote gets the label of each of its Goals and the Lock_File hash.
/// Returns the number of Goals by label and the number of conflicts.
fn final_write(
    dir: &Path,
    lock_hash: Hash,
    files: BTreeMap<String, Vec<GoalLabel>>,
) -> Result<(BTreeMap<String, u64>, u64), SessionError> {
    let unresolved = DpEntry {
        label: "unresolved".to_string(),
        replay: Vec::new(),
        imports_hash: hash::imports_hash(&[]),
        certificate: None, conflict: false,
        invalid: Vec::new(),
    };
    let mut labels = BTreeMap::new();
    let mut conflicts = 0;
    for (file, goals) in files {
        let path = proofs::proof_file(dir, &file);
        let report = proofs::rewrite(&path, lock_hash, &goals, &unresolved)?;
        for (label, n) in report.labels {
            *labels.entry(label).or_insert(0) += n;
        }
        for conflict in &report.conflicts {
            conflicts += 1;
            eprintln!("dp-frontend: conflict: {}: {conflict}", path.display());
        }
        for goal in report.withdrawn {
            eprintln!(
                "dp-frontend: {}: {goal} was written as proved without a proof label of this Session; it is written as not proved",
                path.display()
            );
        }
    }
    Ok((labels, conflicts))
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
    let standalone = Rc::new(RefCell::new(BTreeSet::new()));
    let proved = Rc::clone(&standalone);
    // why3find blocks on its cache messages, so the bridge answers them
    // itself. The Result_Cache proper is consulted when a prover call
    // arrives, after the Goal is printed; why3find's own lookup is a miss.
    let responder = move |msg: &FromWhy3find| match msg {
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
    // Two Sessions that start in one project directory at the same time
    // would both have why3find create its cache directory, and the second
    // fails on it (found 2026-11-25). The directory exists before either
    // why3find starts.
    fs::create_dir_all(config.dir.join(".why3find"))?;
    let bridge = Bridge::start(&why3find, &config.args, &config.dir, MESSAGE_LIMIT, Box::new(responder))?;
    let env = OsEnv::new("dp-frontend")?;
    let mut session = Session::start(env, bridge, config, standalone)?;
    let result = loop {
        match session.turn(Duration::from_millis(5)) {
            Ok(false) => {}
            Ok(true) => break Ok(()),
            Err(e) => break Err(e),
        }
    };
    for call in std::mem::take(&mut session.local).into_keys() {
        session.env.kill(call);
    }
    session.trace.finish()?;
    let files = goal_labels(session.state(), &session.goals);
    let Session { bridge, state, counts, cache_hits, remote_proofs, spent, dir, lock_hash, cert_sources, .. } = session;
    match result {
        Ok(()) => {
            let status = bridge.wait()?;
            let state = state.expect("the state is present between steps");
            // why3find has written its proof files; now the final write.
            let (labels, conflicts) = final_write(&dir, lock_hash, files)?;
            let verdicts =
                state.verdicts.entries.iter().filter(|v| v.label == Label::ProvedStandalone).count();
            let certified = state.verdicts.entries.iter().filter(|v| v.label == Label::Certified).count();
            if certified > 0 || !cert_sources.is_empty() {
                let sources: Vec<_> = cert_sources.iter().map(|(from, n)| format!("{n} {from}")).collect();
                eprintln!("dp-frontend: {certified} certified Verdicts; certificates checked: {}", sources.join(", "));
            }
            let summary: Vec<_> = counts.iter().map(|(label, n)| format!("{n} {label}")).collect();
            eprintln!(
                "dp-frontend: {verdicts} proved-standalone Verdicts ({remote_proofs} from Workers, {cache_hits} Result_Cache hits); prover results: {}",
                if summary.is_empty() { "none".to_string() } else { summary.join(", ") }
            );
            // The report of labels (R7.2 criterion 6): Goals of the proof
            // files by label.
            let by_label: Vec<_> = labels.iter().map(|(label, n)| format!("{n} {label}")).collect();
            eprintln!(
                "dp-frontend: Goals by label: {}",
                if by_label.is_empty() { "none".to_string() } else { by_label.join(", ") }
            );
            if std::env::var_os("DP_PROFILE").is_some() {
                let parts: Vec<_> = spent.iter().map(|(k, d)| format!("{k} {:.2}s", d.as_secs_f64())).collect();
                eprintln!("dp-frontend: time in {}", parts.join(", "));
            }
            // A proof and an Invalid answer for one Goal: the Session does
            // not end as a success (R7.2 criterion 8).
            if conflicts > 0 {
                eprintln!("dp-frontend: {conflicts} Goals have a proof label and an Invalid answer");
                return Ok(status.code().filter(|c| *c != 0).unwrap_or(1));
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
