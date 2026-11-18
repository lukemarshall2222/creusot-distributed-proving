//! One Session of `dp-frontend prove` in local mode (design C3, flow (a)).
//!
//! This is a first, thin shell. It runs why3find in dispatch mode, starts
//! solvers on the local machine, and takes every decision about a Result
//! (acceptance, label, Verdict) from dp-core. Not here yet: the Result_Cache
//! (every cache read is a miss), why3find's Calibration (reference time
//! limits are used as local ones), `dp.toml`, the `dp` members of
//! `proof.json`, event traces, and the loop over the dp-core Front_End
//! machine, which so far tracks only the Session phase and the Goals.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::OsString,
    fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use dp_core::{
    labels::{Outcome, ProverAnswer},
    verdict::{self, Acceptance, Context, Label, ResultFields, UnpatchedSolver, Verdicts},
};
use dp_formats::{
    dispatch::{FromWhy3find, ProverInfo, ProverResult, ToWhy3find},
    hash::{self, Hash},
    lockfile::{current_platform, Kind, LockFile},
};
use dp_shell::why3find::{Bridge, BridgeError, Poll, Printed};

use crate::answer::{self, Family};

/// Size limit of one `dispatch` message. A working value for the local pipe,
/// not a proposal for `<TBD-22>`.
const MESSAGE_LIMIT: usize = 1 << 30;

/// Memory limit passed to provers in megabytes, Why3's default.
const MEMORY_LIMIT_MB: u64 = 1000;

pub struct Config {
    /// The patched why3find.
    pub why3find: PathBuf,
    pub lock: LockFile,
    /// Arguments of `why3find prove`.
    pub args: Vec<OsString>,
    /// The project directory.
    pub dir: PathBuf,
    /// Number of solver processes that run at once.
    pub jobs: usize,
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

/// The file that starting `program` runs: a path as given, or the first
/// match on PATH for a bare name, with symbolic links resolved.
fn find_executable(program: &str) -> std::io::Result<PathBuf> {
    if program.contains('/') {
        return fs::canonicalize(program);
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return fs::canonicalize(candidate);
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::NotFound, "not found on PATH"))
}

/// Splits a Why3 prover command line into words. Single quotes group a word.
fn words(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in command.chars() {
        match c {
            '\'' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            c => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out
}

/// Replaces Why3's placeholders in one word of a prover command line.
fn substitute(word: &str, file: &Path, micros: u64) -> Result<String, SessionError> {
    let mut out = String::new();
    let mut rest = word;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let (value, len) = if after.starts_with(".t") {
            (format!("{:.6}", micros as f64 / 1e6), 2)
        } else {
            match after.chars().next() {
                Some('f') => (file.display().to_string(), 1),
                Some('t') => (micros.div_ceil(1_000_000).max(1).to_string(), 1),
                Some('T') => (micros.div_ceil(1000).max(1).to_string(), 1),
                Some('m') => (MEMORY_LIMIT_MB.to_string(), 1),
                Some('%') => ("%".to_string(), 1),
                _ => {
                    return Err(SessionError::Unsupported(format!(
                        "placeholder in prover command word {word:?}"
                    )));
                }
            }
        };
        out.push_str(&value);
        rest = &after[len..];
    }
    out.push_str(rest);
    Ok(out)
}

struct Prover {
    info: ProverInfo,
    family: Family,
    exe: PathBuf,
    /// Hash of the executable at Session start, equal to its Lock_File entry.
    pinned: Hash,
    /// Interned prover name and version.
    name_id: u64,
    version_id: u64,
}

struct Request {
    rid: String,
    gid: String,
    prover: String,
    micros: u64,
}

struct Job {
    request: Request,
    child: Child,
    started: Instant,
    deadline: Instant,
    output: PathBuf,
    input: PathBuf,
    /// Task_Hash of the bytes given to the solver.
    task_hash: Hash,
    binary_hash: Hash,
    mismatch: bool,
}

struct Session {
    bridge: Bridge,
    lock: LockFile,
    platform: String,
    tmp: PathBuf,
    jobs: usize,
    provers: Option<BTreeMap<String, Prover>>,
    names: BTreeMap<String, u64>,
    prints: BTreeMap<(String, String), (Printed, Hash)>,
    pending: VecDeque<Request>,
    running: Vec<Job>,
    verdicts: Verdicts,
    /// (goal, prover) pairs whose Result was labelled proved-standalone.
    standalone: BTreeSet<(String, String)>,
    goals: BTreeMap<String, u64>,
    counts: BTreeMap<&'static str, u64>,
}

fn kill_group(child: &mut Child) {
    // The solver is the leader of its own process group; this ends the solver
    // and every process it started (R1.5 criterion 17).
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
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
            let exe = words(&info.command)
                .into_iter()
                .next()
                .ok_or_else(|| SessionError::Unsupported("empty prover command".into()))?;
            let exe = find_executable(&exe)
                .map_err(|e| SessionError::Pin(format!("{}: {exe}: {e}", info.prover)))?;
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
                family: Family::of_driver(&info.driver).ok_or_else(|| {
                    SessionError::Unsupported(format!(
                        "prover {} uses the Why3 driver {:?}, whose answers dp-frontend cannot read yet",
                        info.prover, info.driver
                    ))
                })?,
                exe,
                pinned: actual,
                name_id: self.intern(&format!("prover:{name}")),
                version_id: self.intern(&format!("version:{version}")),
                info,
            };
            provers.insert(prover.info.prover.clone(), prover);
        }
        self.provers = Some(provers);
        Ok(())
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

    fn start(&mut self, request: Request) -> Result<(), SessionError> {
        self.load_provers()?;
        let key = (request.gid.clone(), request.prover.clone());
        if !self.prints.contains_key(&key) {
            let printed = self.bridge.print(&request.gid, &request.prover, &[])?;
            let digest = &self.provers.as_ref().expect("loaded")[&request.prover].info.digest;
            // The driver is identified by why3find's digest of the prover
            // configuration and driver files; no further transformations.
            let task_hash = hash::task_hash(&printed.bytes, digest.as_bytes(), b"");
            self.prints.insert(key.clone(), (printed, task_hash));
        }
        let (printed, _) = &self.prints[&key];
        if printed.tasks != 1 {
            // print_local is undefined (R1.3 criterion 10): no Job, no Result.
            eprintln!(
                "dp-frontend: goal {} with {}: the pipeline yields {} tasks; not run",
                request.gid, request.prover, printed.tasks
            );
            return self.answer(&request.rid.clone(), ProverResult::Failed);
        }
        let prover = &self.provers.as_ref().expect("loaded")[&request.prover];
        let input = self.tmp.join(format!("{}.{}", request.rid, prover.family.extension()));
        fs::write(&input, &printed.bytes)?;
        let task_hash = hash::task_hash(&printed.bytes, prover.info.digest.as_bytes(), b"");
        // R7.7 criterion 7: hash the solver again just before starting it.
        let binary_hash = file_hash(&prover.exe)?;
        let mismatch = binary_hash != prover.pinned;
        let mut command = words(&prover.info.command).into_iter();
        command.next();
        let args =
            command.map(|w| substitute(&w, &input, request.micros)).collect::<Result<Vec<_>, _>>()?;
        let output = self.tmp.join(format!("{}.out", request.rid));
        let out = fs::File::create(&output)?;
        let child = Command::new(&prover.exe)
            .args(args)
            .stdin(Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(out)
            .process_group(0)
            .spawn()?;
        let started = Instant::now();
        // Like why3find, give the solver half as much again before killing it.
        let deadline = started + Duration::from_micros(request.micros + request.micros / 2);
        self.running.push(Job { request, child, started, deadline, output, input, task_hash, binary_hash, mismatch });
        Ok(())
    }

    /// Labels the Result of a finished Job and answers why3find.
    fn finish(&mut self, job: Job, answer: ProverAnswer, elapsed: Duration) -> Result<(), SessionError> {
        let _ = fs::remove_file(&job.input);
        let _ = fs::remove_file(&job.output);
        let Request { rid, gid, prover, micros } = job.request;
        let (name_id, version_id) = {
            let p = &self.provers.as_ref().expect("loaded")[&prover];
            (p.name_id, p.version_id)
        };
        let (printed, local_task_hash) = self.prints[&(gid.clone(), prover.clone())].clone();
        let empty = hash::imports_hash(&[]);
        let fields = ResultFields {
            answer,
            task_hash: core_hash(job.task_hash),
            partition: false,
            imports: 0,
            imports_hash: core_hash(empty),
            prover: name_id,
            version: version_id,
            binary_hash: core_hash(job.binary_hash),
        };
        let ctx = Context {
            certify_all: false,
            import_mark: false,
            local_task_hash: core_hash(local_task_hash),
            task_count: u64::from(printed.tasks),
            empty_imports_hash: core_hash(empty),
            local_binary_mismatch: job.mismatch,
            unpatched: self.unpatched(),
        };
        if verdict::accept_result(fields, &ctx) != Acceptance::Accepted {
            eprintln!("dp-frontend: Result of {prover} for goal {gid} discarded: not bound to the local print");
            return self.answer(&rid, ProverResult::Failed);
        }
        let label = verdict::label(fields, &ctx);
        let spent = (elapsed.as_micros() as u64).min(micros);
        let (name, reply) = match label {
            Label::ProvedStandalone => {
                let goal = self.goals[&gid];
                let verdicts = std::mem::replace(&mut self.verdicts, Verdicts { entries: Vec::new() });
                self.verdicts = verdict::record_verdict(verdicts, goal, label);
                self.standalone.insert((gid.clone(), prover.clone()));
                ("proved-standalone", ProverResult::Valid { micros: spent })
            }
            // Not reachable in this build; never answered as a proof here.
            Label::Certified => ("unchecked", ProverResult::Unknown { micros: spent }),
            Label::NonProof(Outcome::Timeout) => ("timeout", ProverResult::Timeout { micros }),
            Label::NonProof(Outcome::Failed) => ("failed", ProverResult::Failed),
            Label::NonProof(Outcome::Unchecked) => {
                eprintln!(
                    "dp-frontend: {prover} reports a proof of goal {gid}, labelled unchecked \
                     (its binary or version is not an Unpatched_Solver of the Lock_File)"
                );
                ("unchecked", ProverResult::Unknown { micros: spent })
            }
            Label::NonProof(Outcome::StandaloneInvalid | Outcome::DependentInvalid) => {
                ("invalid", ProverResult::Unknown { micros: spent })
            }
            Label::NonProof(_) => ("unknown", ProverResult::Unknown { micros: spent }),
        };
        *self.counts.entry(name).or_insert(0) += 1;
        self.answer(&rid, reply)
    }

    fn reap(&mut self) -> Result<(), SessionError> {
        let mut i = 0;
        while i < self.running.len() {
            let job = &mut self.running[i];
            let exited = job.child.try_wait()?.is_some();
            let late = Instant::now() >= job.deadline;
            if !exited && !late {
                i += 1;
                continue;
            }
            let mut job = self.running.swap_remove(i);
            let elapsed = job.started.elapsed();
            let answer = if exited {
                // End whatever the solver left running in its process group.
                kill_group(&mut job.child);
                let output = String::from_utf8_lossy(&fs::read(&job.output)?).into_owned();
                let answer = answer::read(
                    self.provers.as_ref().expect("loaded")[&job.request.prover].family,
                    &output,
                );
                if answer == ProverAnswer::Failure {
                    eprintln!(
                        "dp-frontend: {} gave no answer on goal {} after {:?}: {:?}",
                        job.request.prover,
                        job.request.gid,
                        elapsed,
                        output.chars().take(300).collect::<String>()
                    );
                }
                answer
            } else {
                kill_group(&mut job.child);
                ProverAnswer::Timeout
            };
            self.finish(job, answer, elapsed)?;
        }
        while self.running.len() < self.jobs {
            let Some(request) = self.pending.pop_front() else { break };
            self.start(request)?;
        }
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
            // No Result_Cache yet: every lookup is a miss.
            FromWhy3find::CacheGet { rid, .. } => {
                Ok(self.bridge.send(&ToWhy3find::Cache { rid, result: ProverResult::NoResult })?)
            }
            FromWhy3find::Prove { rid, gid, prover, micros, .. } => {
                if !self.goals.contains_key(&gid) {
                    return Err(SessionError::Bridge(BridgeError::Protocol(format!(
                        "prover call for the unannounced goal {gid}"
                    ))));
                }
                self.pending.push_back(Request { rid, gid, prover, micros });
                Ok(())
            }
            FromWhy3find::Cancel { rid } => {
                if let Some(i) = self.pending.iter().position(|r| r.rid == rid) {
                    self.pending.remove(i);
                    return self.answer(&rid, ProverResult::NoResult);
                }
                if let Some(i) = self.running.iter().position(|j| j.request.rid == rid) {
                    let mut job = self.running.swap_remove(i);
                    kill_group(&mut job.child);
                    let _ = fs::remove_file(&job.input);
                    let _ = fs::remove_file(&job.output);
                    return self.answer(&rid, ProverResult::NoResult);
                }
                Ok(()) // already answered
            }
            // The legacy cache layout holds proved-standalone Results only
            // (R7.5 criterion 7).
            FromWhy3find::CacheSet { rid, gid, prover, result } => {
                let store = matches!(result, ProverResult::Valid { .. })
                    && self.standalone.contains(&(gid, prover));
                Ok(self.bridge.send(&ToWhy3find::Stored { rid, store })?)
            }
            FromWhy3find::Done => Ok(()),
            other => Err(SessionError::Bridge(BridgeError::Protocol(format!(
                "unexpected message {other:?}"
            )))),
        }
    }

    fn run(&mut self) -> Result<(), SessionError> {
        loop {
            match self.bridge.poll(Duration::from_millis(5))? {
                Poll::Message(msg) => self.handle(msg)?,
                Poll::Idle => {}
                Poll::Closed => return Ok(()),
            }
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
    let tmp = std::env::temp_dir().join(format!("dp-frontend-{}", std::process::id()));
    fs::create_dir_all(&tmp)?;
    let bridge = Bridge::start(&why3find, &config.args, &config.dir, MESSAGE_LIMIT)?;
    let mut session = Session {
        bridge,
        lock: config.lock,
        platform,
        tmp: tmp.clone(),
        jobs: config.jobs.max(1),
        provers: None,
        names: BTreeMap::new(),
        prints: BTreeMap::new(),
        pending: VecDeque::new(),
        running: Vec::new(),
        verdicts: Verdicts { entries: Vec::new() },
        standalone: BTreeSet::new(),
        goals: BTreeMap::new(),
        counts: BTreeMap::new(),
    };
    let result = session.run();
    for job in &mut session.running {
        kill_group(&mut job.child);
    }
    let _ = fs::remove_dir_all(&tmp);
    let Session { bridge, verdicts, counts, .. } = session;
    match result {
        Ok(()) => {
            let status = bridge.wait()?;
            let summary: Vec<_> = counts.iter().map(|(label, n)| format!("{n} {label}")).collect();
            eprintln!(
                "dp-frontend: {} proved-standalone Verdicts; prover results: {}",
                verdicts.entries.len(),
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
