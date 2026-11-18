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
    cache::{self, Entry, Origin},
    cluster::{Job, Message, Registry, SignedResult},
    dispatch::{DriverAnswer, FromWhy3find, ProverInfo, ProverResult, StrategyStep, ToWhy3find},
    hash::{self, Hash},
    lockfile::{current_platform, Kind, LockFile},
    proofjson::{DpEntry, Place, ReplayFields},
};
use crate::proofs::{self, GoalLabel};
use dp_shell::{
    net::{Connection, Event},
    solver::{self, Run},
    store,
    trace::{Retention, TraceWriter},
    why3find::{Bridge, BridgeError, Poll, Printed},
};

/// Size limit of one `dispatch` message and of one cache entry. Working
/// values, not proposals for `<TBD-22>`.
const MESSAGE_LIMIT: usize = 1 << 30;
const ENTRY_LIMIT: usize = 1 << 24;

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
    /// Jobs of a prover call on the Cluster. A placeholder for the per-goal
    /// deadline `<TBD-3>`.
    pub job_margin: Duration,
    /// When set, one JSON line per labelled run is appended to this file:
    /// raw measurements for the evaluation (R6.3).
    pub record: Option<PathBuf>,
    /// Number of Jobs, each with its own solver seed, that race on one prover
    /// call on the Cluster (R2.2). One means no racing.
    pub portfolio_seeds: u64,
    /// When set, every Printed_Goal that a prover call runs on is written to
    /// this directory, named by its Task_Hash.
    pub export: Option<PathBuf>,
    /// Where the event trace goes and how much of it is kept (R1.5 criteria
    /// 18 and 19).
    pub trace: Option<(PathBuf, Retention)>,
    /// The hash of the Lock_File, for the member `dp-lock` of the proof
    /// files (R7.7 criterion 5).
    pub lock_hash: Hash,
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

fn driver_answer(answer: ProverAnswer) -> DriverAnswer {
    match answer {
        ProverAnswer::Valid => DriverAnswer::Valid,
        ProverAnswer::Invalid => DriverAnswer::Invalid,
        ProverAnswer::Timeout => DriverAnswer::Timeout,
        ProverAnswer::OutOfMemory => DriverAnswer::OutOfMemory,
        ProverAnswer::StepLimitExceeded => DriverAnswer::StepLimitExceeded,
        ProverAnswer::Unknown => DriverAnswer::Unknown,
        ProverAnswer::Failure => DriverAnswer::Failure,
        ProverAnswer::HighFailure => DriverAnswer::HighFailure,
    }
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
    run: Run,
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

struct Session {
    bridge: Bridge,
    lock: LockFile,
    platform: String,
    tmp: PathBuf,
    dir: PathBuf,
    start: Instant,
    cluster: Option<(Connection, Registry)>,
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
    last_tick: Instant,
}

impl Session {
    fn now(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
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
                version,
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
        let (printed, local_task_hash) = self.print_local(&gid, &prover)?;
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

    /// The lookup fields of the run description of a call (R7.5 criterion 1).
    fn lookup_fields(&mut self, call: CallId) -> Result<LookupFields, SessionError> {
        let (gid, prover, micros) = {
            let c = &self.calls[&call];
            (c.gid.clone(), c.prover.clone(), c.micros)
        };
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
            seed: None,
            micros,
            import_mark: false,
            // The option list is the command line of the prover configuration.
            options: vec![p.info.command.clone().into_bytes()],
        })
    }

    fn cache_file(&self, atoms: &[cache::Atom]) -> PathBuf {
        self.dir.join(".dp-cache").join(cache::cache_key(atoms).to_hex())
    }

    /// Looks the run description of a call up in the Result_Cache. A hit is
    /// an entry whose stored lookup fields are those of the lookup and whose
    /// validation fields pass; it is returned as a cache-hit input, which
    /// dp-core still has to accept and label. Anything else is a miss.
    fn cache_lookup(&mut self, call: CallId) -> Result<Option<(Input, Finished)>, SessionError> {
        if !self.use_cache {
            return Ok(None);
        }
        let wanted = cache_key::encode_key(&self.lookup_fields(call)?);
        let path = self.cache_file(&formats_atoms(&wanted));
        let Ok(bytes) = fs::read(&path) else { return Ok(None) };
        let miss = |reason: &str| {
            eprintln!("dp-frontend: Result_Cache entry {} is a miss: {reason}", path.display());
        };
        let entry = match Entry::decode(&bytes, ENTRY_LIMIT) {
            Ok(entry) => entry,
            Err(e) => {
                miss(&e.to_string());
                return Ok(None);
            }
        };
        // The stored lookup fields themselves, not only the key (criterion 5).
        if !cache_key::lookup_ok(&core_atoms(&entry.lookup), &wanted) {
            miss("its lookup fields are not those of the lookup");
            return Ok(None);
        }
        let (gid, prover, micros) = {
            let c = &self.calls[&call];
            (c.gid.clone(), c.prover.clone(), c.micros)
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
                let key = self.cluster.as_ref().and_then(|(_, registry)| registry.key(&signed.fields.worker));
                let facts = RemoteFacts {
                    registered: key.is_some(),
                    revoked: false,
                    signature_valid: key.is_some_and(|k| signed.verifies(&k)),
                    submitted: signed.fields.prover == prover
                        && signed.fields.binary_hash == entry.binary_hash,
                };
                (signed.fields.task_hash, signed.fields.prover.clone(), Some(facts))
            }
        };
        let evidence =
            self.evidence(call, core_answer(entry.answer), task_hash, &result_prover, entry.binary_hash, false)?;
        let finished = Finished {
            call,
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
                    velocity: None,
                    local_micros: None,
                },
            },
            task_hash,
            status: None,
            timed_out: false,
            output: Vec::new(),
            micros: entry.micros,
        };
        Ok(Some((Input::CacheHit { now: self.now(), rid: call, evidence, remote }, finished)))
    }

    /// Stores the Result of a labelled run (R7.5 criterion 3).
    fn cache_store(&mut self, finished: &Finished, answer: ProverAnswer) -> Result<(), SessionError> {
        // A failed run says nothing that a later Session could reuse.
        if !self.use_cache || matches!(answer, ProverAnswer::Failure | ProverAnswer::HighFailure) {
            return Ok(());
        }
        let (binary_hash, velocity, local_micros, origin) = match &finished.source {
            Source::Cache { .. } => return Ok(()),
            Source::Local { binary_hash, mismatch, velocity, local_micros } => {
                if *mismatch {
                    return Ok(());
                }
                (*binary_hash, *velocity, *local_micros, Origin::Local)
            }
            // Workers are not calibrated yet: their local limit is the
            // reference limit.
            Source::Worker { signed, .. } => {
                let micros = self.calls.get(&finished.call).map_or(0, |c| c.micros);
                (signed.fields.binary_hash, 1_000_000, micros, Origin::Worker(signed.clone()))
            }
        };
        if !self.calls.contains_key(&finished.call) {
            return Ok(());
        }
        let lookup = formats_atoms(&cache_key::encode_key(&self.lookup_fields(finished.call)?));
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
        store::write_atomic(&self.cache_file(&entry.lookup), &entry.encode())?;
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
        // R7.7 criterion 7: check the solver binary just before starting it.
        // The binary is read and hashed again only if the file's stamp is
        // not the one seen when it was hashed at Session start; with an
        // unchanged stamp its contents are the pinned ones (TB-17).
        let binary_hash =
            if file_stamp(&prover.exe)? == prover.stamp { prover.pinned } else { file_hash(&prover.exe)? };
        let mismatch = binary_hash != prover.pinned;
        let local_micros = calibration::scale(micros, velocity);
        let run = solver::start(
            &prover.exe,
            &prover.info.command,
            &printed.bytes,
            &prover.info.extension,
            local_micros.max(1),
            &self.tmp,
            &format!("job{call}"),
        )?;
        if let Some(dir) = &self.export {
            let file = dir.join(format!("{}{}", task_hash.to_hex(), prover.info.extension));
            if !file.exists() {
                fs::write(file, &printed.bytes)?;
            }
        }
        self.local.insert(call, LocalRun { run, task_hash, binary_hash, mismatch, velocity, local_micros });
        Ok(())
    }

    fn submit(&mut self, call: CallId, seed: u64) -> Result<(), SessionError> {
        let (gid, prover_name, micros) = {
            let c = &self.calls[&call];
            (c.gid.clone(), c.prover.clone(), c.micros)
        };
        let (printed, _) = self.print_local(&gid, &prover_name)?;
        let prover = self.prover(&prover_name)?;
        let job = Job {
            job_id: format!("{}/{call}/{seed}", self.session_id),
            prover: prover_name.clone(),
            driver: prover.info.digest.clone().into_bytes(),
            extension: prover.info.extension.clone(),
            micros,
            // Seed 0 is the seed of the Worker's prover configuration.
            seed: (seed != 0).then_some(seed),
            goal: printed.bytes,
        };
        let job_id = job.job_id.clone();
        let sent = match self.cluster.as_mut() {
            Some((connection, _)) => connection.send(&Message::Submit(job)).is_ok(),
            None => false,
        };
        if sent {
            self.jobs.insert(job_id, (call, seed));
        } else {
            self.cluster_lost("the connection to the Coordinator failed");
        }
        Ok(())
    }

    fn cluster_lost(&mut self, reason: &str) {
        if self.cluster.take().is_some() {
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
                if let Some(local) = self.local.remove(&rid) {
                    local.run.cancel();
                }
                Ok(())
            }
            Effect::CancelJob { rid, seed } => {
                let job_id = format!("{}/{rid}/{seed}", self.session_id);
                if self.jobs.remove(&job_id).is_some() {
                    if let Some((connection, _)) = self.cluster.as_mut() {
                        let _ = connection.send(&Message::Cancel { job_id });
                    }
                }
                Ok(())
            }
            Effect::Answer { rid, reply } => {
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
                Ok(self.bridge.send(&ToWhy3find::Answer { rid: call.rid, result })?)
            }
        }
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
            LogEntry::Lost { call, outcome } => (call, outcome_name(outcome).to_string()),
            LogEntry::Rejected { call, reason } => {
                if let Some(c) = self.calls.get(&call) {
                    eprintln!("dp-frontend: Result of {} for goal {} discarded: {reason:?}", c.prover, c.gid);
                }
                (call, "discarded".to_string())
            }
        };
        *self.counts.entry(name.clone()).or_insert(0) += 1;
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
                    Source::Worker { signed, .. } => {
                        (signed.fields.binary_hash, Place::Worker(signed.fields.worker.clone()), None, None)
                    }
                    Source::Cache { binary_hash, place, velocity, local_micros, .. } => {
                        (*binary_hash, place.clone(), *velocity, *local_micros)
                    }
                };
                let seed = match &finished.source {
                    Source::Cache { seed, .. } => *seed,
                    _ => seed,
                };
                let p = self.prover(&prover)?;
                let replay = ReplayFields {
                    task_hash: finished.task_hash,
                    driver: p.info.digest.clone().into_bytes(),
                    prover: p.info.prover.split_once('@').map_or(p.info.prover.as_str(), |(n, _)| n).to_string(),
                    version: p.version.clone(),
                    binary_hash,
                    place,
                    seed: seed.filter(|s| *s != 0),
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
                if let Some(goal) = self.goals.get_mut(&gid) {
                    goal.outcomes.push(outcome);
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
        let Some(call) = self.calls.get(&finished.call) else { return Ok(()) };
        // A run that died from a signal has no exit status; the driver still
        // reads its output, as a failed run.
        let status = finished.status.unwrap_or(1);
        let qid = self.bridge.ask_answer(&call.prover, status, &finished.output)?;
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
        let key = registry.key(&signed.fields.worker);
        let facts = RemoteFacts {
            registered: key.is_some(),
            revoked: false,
            signature_valid: key.is_some_and(|k| signed.verifies(&k)),
            submitted: true, // decided by dp-core from the prover and the seed
        };
        let authentic = facts.registered && facts.signature_valid;
        if !authentic {
            eprintln!(
                "dp-frontend: Result for {:?} discarded: not signed by a registered Worker (names {:?})",
                signed.fields.job_id, signed.fields.worker
            );
            *self.counts.entry("discarded".into()).or_insert(0) += 1;
        } else {
            // An authentic Result ends its Job.
            self.jobs.remove(&signed.fields.job_id);
        }
        // The Job as submitted: its prover and its seed (R1.3 criterion 6).
        // A Result that names another seed is for no Job of this call.
        let signed_seed = signed.fields.seed.unwrap_or(0);
        let prover_id = if signed_seed == seed {
            self.intern(&format!("call-prover:{}", signed.fields.prover))
        } else {
            u64::MAX
        };
        let f = signed.fields.clone();
        let finished = Finished {
            call,
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
        let ids: Vec<CallId> = self.local.keys().copied().collect();
        for call in ids {
            let done = self.local.get_mut(&call).expect("listed").run.poll()?;
            if let Some(done) = done {
                let local = self.local.remove(&call).expect("listed");
                // Report the time in reference units, as why3find does.
                let micros = (done.elapsed.as_micros() * 1_000_000 / u128::from(local.velocity.max(1))) as u64;
                self.finished(Finished {
                    call,
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
                    Goal { id, file, theory, name, path, steps, outcomes: Vec::new(), replay: None },
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
            FromWhy3find::Error { qid, .. } if self.awaiting.contains_key(&qid) => {
                self.driver_answered(&qid, None)
            }
            FromWhy3find::Done => Ok(()),
            other => Err(SessionError::Bridge(BridgeError::Protocol(format!(
                "unexpected message {other:?}"
            )))),
        }
    }

    fn driver_answered(&mut self, qid: &str, answer: Option<DriverAnswer>) -> Result<(), SessionError> {
        let Some(Awaiting { finished, asked }) = self.awaiting.remove(qid) else {
            return Err(SessionError::Bridge(BridgeError::Protocol(format!("unexpected reply {qid}"))));
        };
        *self.spent.entry("read-answer").or_default() += asked.elapsed();
        self.answered(finished, answer.map_or(ProverAnswer::HighFailure, core_answer))
    }

    fn cluster_events(&mut self) -> Result<(), SessionError> {
        loop {
            let Some((connection, _)) = self.cluster.as_mut() else { return Ok(()) };
            match connection.poll(Duration::ZERO) {
                Event::Idle => return Ok(()),
                Event::Closed(reason) => {
                    self.cluster_lost(&format!(
                        "the Coordinator is gone{}",
                        reason.map(|r| format!(" ({r})")).unwrap_or_default()
                    ));
                    return Ok(());
                }
                Event::Message(Message::Result(signed)) => self.result(signed)?,
                Event::Message(Message::Failed { job_id, reason }) => {
                    if let Some((call, seed)) = self.jobs.remove(&job_id) {
                        eprintln!("dp-frontend: {job_id} was not run on the Cluster ({reason})");
                        let now = self.now();
                        self.inputs.push_back((Input::JobNotRun { now, rid: call, seed }, None));
                    }
                }
                // Nothing else is part of the protocol towards a Front_End.
                Event::Message(_) => {}
            }
        }
    }

    fn drain(&mut self) -> Result<(), SessionError> {
        while let Some((input, finished)) = self.inputs.pop_front() {
            self.step(input, finished)?;
        }
        Ok(())
    }

    fn run(&mut self) -> Result<(), SessionError> {
        let now = self.now();
        let cluster = self.cluster.is_some();
        self.inputs.push_back((Input::SessionStart { now, cluster }, None));
        loop {
            self.drain()?;
            match self.bridge.poll(Duration::from_millis(5))? {
                Poll::Message(msg) => self.handle(msg)?,
                Poll::Idle => {}
                Poll::Closed => {
                    let now = self.now();
                    self.inputs.push_back((Input::Why3findDone { now }, None));
                    return self.drain();
                }
            }
            self.cluster_events()?;
            self.reap()?;
            // Deadlines of races are checked a few times a second, while
            // there are Jobs on the Cluster.
            if !self.jobs.is_empty() && self.last_tick.elapsed() >= Duration::from_millis(200) {
                self.last_tick = Instant::now();
                let now = self.now();
                self.inputs.push_back((Input::Tick { now }, None));
            }
        }
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

/// The final write (R1.5 criterion 14): every proof file that why3find
/// wrote gets the label of each of its Goals and the Lock_File hash.
/// Returns the number of Goals by label.
fn final_write(
    dir: &Path,
    lock_hash: Hash,
    state: &State,
    goals: &BTreeMap<String, Goal>,
) -> Result<BTreeMap<String, u64>, SessionError> {
    let imports_hash = hash::imports_hash(&[]);
    let mut files: BTreeMap<&str, Vec<GoalLabel>> = BTreeMap::new();
    for goal in goals.values() {
        let recorded = frontend::verdict_of(&state.verdicts, goal.id).map(|v| v.label);
        let label = verdict::final_label(recorded, &goal.outcomes);
        let (name, proof) = match label {
            Label::ProvedStandalone => ("proved-standalone", true),
            Label::Certified => ("certified", true),
            Label::NonProof(outcome) => (outcome_name(outcome), false),
        };
        files.entry(&goal.file).or_default().push(GoalLabel {
            theory: goal.theory.clone(),
            name: goal.name.clone(),
            steps: goal.steps.clone(),
            entry: DpEntry {
                label: name.to_string(),
                replay: if proof { goal.replay.clone().into_iter().collect() } else { Vec::new() },
                imports_hash,
            },
            proof,
        });
    }
    let unresolved = DpEntry { label: "unresolved".to_string(), replay: Vec::new(), imports_hash };
    let mut labels = BTreeMap::new();
    for (file, goals) in files {
        let path = proofs::proof_file(dir, file);
        let report = proofs::rewrite(&path, lock_hash, &goals, &unresolved)?;
        for (label, n) in report.labels {
            *labels.entry(label).or_insert(0) += n;
        }
        for goal in report.withdrawn {
            eprintln!(
                "dp-frontend: {}: {goal} was written as proved without a proof label of this Session; it is written as not proved",
                path.display()
            );
        }
    }
    Ok(labels)
}

fn formats_atoms(atoms: &[cache_key::Atom]) -> Vec<cache::Atom> {
    atoms
        .iter()
        .map(|a| match a {
            cache_key::Atom::Bytes(b) => cache::Atom::Bytes(b.clone()),
            cache_key::Atom::Num(n) => cache::Atom::Num(*n),
        })
        .collect()
}

fn core_atoms(atoms: &[cache::Atom]) -> Vec<cache_key::Atom> {
    atoms
        .iter()
        .map(|a| match a {
            cache::Atom::Bytes(b) => cache_key::Atom::Bytes(b.clone()),
            cache::Atom::Num(n) => cache_key::Atom::Num(*n),
        })
        .collect()
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
    let use_cache = !config.args.iter().any(|a| a == "--no-cache");
    let bridge =
        Bridge::start(&why3find, &config.args, &config.dir, MESSAGE_LIMIT, Box::new(responder))?;
    let trace = match &config.trace {
        Some((path, retention)) => TraceWriter::to_file("front-end", path.clone(), *retention)?,
        None => TraceWriter::off("front-end"),
    };
    let mut session = Session {
        bridge,
        platform,
        tmp: tmp.clone(),
        dir: config.dir.clone(),
        start: Instant::now(),
        cluster,
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
        use_cache,
        record: match &config.record {
            Some(path) => Some(fs::OpenOptions::new().create(true).append(true).open(path)?),
            None => None,
        },
        export: config.export.clone(),
        spent: BTreeMap::new(),
        counts: BTreeMap::new(),
        cache_hits: 0,
        remote_proofs: 0,
        last_tick: Instant::now(),
        lock: config.lock,
    };
    let unpatched = session.pins(&[Kind::UnpatchedSolver]);
    session.state = Some(frontend::FrontEnd::init(&frontend::Config {
        local_jobs: config.jobs.max(1) as u64,
        seeds: config.portfolio_seeds.max(1),
        job_margin: config.job_margin.as_micros() as u64,
        certify_all: false,
        unpatched,
        empty_imports_hash: core_hash(hash::imports_hash(&[])),
    }));
    let result = session.run();
    for (_, local) in std::mem::take(&mut session.local) {
        local.run.cancel();
    }
    let _ = fs::remove_dir_all(&tmp);
    session.trace.finish()?;
    let Session { bridge, state, counts, cache_hits, remote_proofs, spent, goals, dir, .. } = session;
    match result {
        Ok(()) => {
            let status = bridge.wait()?;
            let state = state.expect("the state is present between steps");
            // why3find has written its proof files; now the final write.
            let labels = final_write(&dir, config.lock_hash, &state, &goals)?;
            let verdicts =
                state.verdicts.entries.iter().filter(|v| v.label == Label::ProvedStandalone).count();
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
