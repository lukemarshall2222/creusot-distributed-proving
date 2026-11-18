//! Replay_Tool (design C17, R7.6): re-checks the proof-labelled Verdicts of
//! the proof files, on this machine, without the Cluster, the Result_Cache
//! or a network.
//!
//! It runs `why3find prove -r` in dispatch mode. why3find walks the proof
//! files as it does for its own replay and makes one prover call for every
//! entry that the legacy members say is proved. For each call the tool
//! reads the `dp` member of that entry and checks it against what it sees
//! itself:
//!
//! 1. the entry decodes canonically and its label is proved-standalone;
//! 2. the file's `dp-lock` is the hash of the Lock_File in use;
//! 3. the recorded prover is the one of the call, the recorded binary hash
//!    is an Unpatched_Solver of the Lock_File, and the local executable has
//!    that hash;
//! 4. the Task_Hash of its own print of the Goal is the recorded one;
//! 5. the solver, started here with the recorded seed and the recorded
//!    reference limit scaled by the local Calibration and by the replay
//!    factor, proves the Goal, as read by the pinned driver.
//!
//! A Verdict is confirmed only if all of this holds. Trust in a proof file
//! therefore needs nothing that the Session stored besides the file: not
//! the Result_Cache, not a Worker's signature.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use dp_core::{calibration, verdict::{self, Label}};
use dp_formats::{
    dispatch::{DriverAnswer, FromWhy3find, ProverInfo, ProverResult, ToWhy3find},
    hash::{self, Hash},
    lockfile::{current_platform, Kind, LockFile},
    proofjson::{proof_file, DpEntry},
};
use dp_shell::{
    solver,
    why3find::{Bridge, BridgeError, Poll},
};
use serde_json::Value;

/// The replay time-limit factor. A working value, not a proposal for
/// `<TBD-20>`; it is at least 1, so replay never gives the solver less time
/// than the locally scaled reference limit.
pub const LIMIT_FACTOR: u64 = 2;

const MESSAGE_LIMIT: usize = 1 << 30;
const ENTRY_LIMIT: usize = 1 << 20;

pub struct Config {
    /// The patched why3find.
    pub why3find: PathBuf,
    pub lock: LockFile,
    pub lock_hash: Hash,
    /// The project directory.
    pub dir: PathBuf,
    /// The files to replay, and any other argument of `why3find prove`.
    pub args: Vec<OsString>,
}

/// What replay found for one Goal entry of a proof file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A proof-labelled Verdict that every check confirms.
    Confirmed { prover: String, micros: u64 },
    /// A proof-labelled Verdict, or a legacy proof, that a check does not
    /// confirm.
    NotConfirmed(String),
    /// A proof-labelled Verdict whose re-run answers Invalid.
    Contradicted { prover: String },
    /// An entry without a proof: nothing to confirm.
    NotProved { label: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub file: String,
    pub theory: String,
    pub goal: String,
    /// The tactic steps from the root Goal.
    pub path: String,
    pub outcome: Outcome,
}

#[derive(Debug, Default)]
pub struct Report {
    /// One entry per Goal entry of the replayed proof files.
    pub entries: Vec<Entry>,
    /// Calibrations used: prover and local velocity in millionths.
    pub calibrations: BTreeMap<String, u64>,
    /// Proof files whose `dp-lock` is not the hash of the Lock_File in use.
    pub lock_mismatches: Vec<String>,
}

impl Report {
    pub fn count(&self, pick: impl Fn(&Outcome) -> bool) -> usize {
        self.entries.iter().filter(|e| pick(&e.outcome)).count()
    }

    /// Zero only when every proof-labelled Verdict is confirmed; 3, the
    /// reserved status, when one is contradicted (R7.6 criterion 9).
    pub fn exit_status(&self) -> i32 {
        if self.count(|o| matches!(o, Outcome::Contradicted { .. })) > 0 {
            3
        } else if self.count(|o| matches!(o, Outcome::NotConfirmed(_))) > 0 {
            1
        } else {
            0
        }
    }
}

#[derive(Debug)]
pub enum ReplayError {
    Bridge(BridgeError),
    Io(std::io::Error),
    Pin(String),
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReplayError::Bridge(e) => write!(f, "{e}"),
            ReplayError::Io(e) => write!(f, "{e}"),
            ReplayError::Pin(m) => write!(f, "pinned dependency check failed: {m}"),
        }
    }
}

impl From<BridgeError> for ReplayError {
    fn from(e: BridgeError) -> Self {
        ReplayError::Bridge(e)
    }
}

impl From<std::io::Error> for ReplayError {
    fn from(e: std::io::Error) -> Self {
        ReplayError::Io(e)
    }
}

/// A leaf of the proof tree of a root Goal.
struct Leaf {
    /// The legacy members say that a prover proved it.
    legacy_prover: Option<String>,
    /// The value of the member `dp`, if it is a string.
    member: Option<String>,
    outcome: Option<Outcome>,
}

type LeafKey = (String, String, Vec<(String, u32)>);

struct ProofFile {
    lock_ok: bool,
    leaves: BTreeMap<LeafKey, Leaf>,
}

fn leaves(theory: &str, name: &str, steps: &mut Vec<(String, u32)>, node: &Value, out: &mut BTreeMap<LeafKey, Leaf>) {
    let tactic = node.get("tactic").and_then(Value::as_str);
    if let (Some(tactic), Some(Value::Array(children))) = (tactic, node.get("children")) {
        for (i, child) in children.iter().enumerate() {
            steps.push((tactic.to_string(), i as u32));
            leaves(theory, name, steps, child, out);
            steps.pop();
        }
        return;
    }
    out.insert(
        (theory.to_string(), name.to_string(), steps.clone()),
        Leaf {
            legacy_prover: node.get("prover").and_then(Value::as_str).map(str::to_string),
            member: node.get("dp").and_then(Value::as_str).map(str::to_string),
            outcome: None,
        },
    );
}

fn load(path: &Path, lock_hash: Hash) -> ProofFile {
    let mut file = ProofFile { lock_ok: false, leaves: BTreeMap::new() };
    let Ok(text) = fs::read_to_string(path) else { return file };
    let Ok(json) = serde_json::from_str::<Value>(&text) else { return file };
    file.lock_ok = json.get("dp-lock").and_then(Value::as_str) == Some(&format!("sha256:{}", lock_hash.to_hex()));
    if let Some(Value::Object(theories)) = json.get("proofs") {
        for (theory, goals) in theories {
            if let Value::Object(goals) = goals {
                for (name, node) in goals {
                    leaves(theory, name, &mut Vec::new(), node, &mut file.leaves);
                }
            }
        }
    }
    file
}

/// The label of a `dp` member, as dp-core reads label names: anything that
/// is not exactly a proof label is not a proof.
fn label_of(entry: &DpEntry) -> Label {
    verdict::decode_label(entry.label.as_bytes(), b"proved-standalone", b"certified")
}

struct Prover {
    info: ProverInfo,
    exe: PathBuf,
    binary_hash: Hash,
    /// The pinned version of the Unpatched_Solver entry with that hash.
    unpatched_version: Option<String>,
}

struct Goal {
    file: String,
    key: LeafKey,
}

struct Replay {
    bridge: Bridge,
    config: Config,
    platform: String,
    tmp: PathBuf,
    provers: Option<BTreeMap<String, Prover>>,
    goals: BTreeMap<String, Goal>,
    files: BTreeMap<String, ProofFile>,
    report: Report,
    runs: u64,
}

impl Replay {
    fn provers(&mut self) -> Result<&BTreeMap<String, Prover>, ReplayError> {
        if self.provers.is_none() {
            let mut provers = BTreeMap::new();
            for info in self.bridge.provers()? {
                let Some(program) = solver::words(&info.command).into_iter().next() else { continue };
                let Ok(exe) = solver::find_executable(&program) else { continue };
                let binary_hash = hash::sha256(&fs::read(&exe)?);
                let name = info.prover.split_once('@').map_or(info.prover.as_str(), |(n, _)| n);
                let unpatched_version = self
                    .config
                    .lock
                    .pins(Kind::UnpatchedSolver, &self.platform)
                    .find(|(tool, hash)| tool.prover.as_deref() == Some(name) && *hash == binary_hash)
                    .map(|(tool, _)| tool.version.clone());
                provers.insert(info.prover.clone(), Prover { info, exe, binary_hash, unpatched_version });
            }
            self.provers = Some(provers);
        }
        Ok(self.provers.as_ref().expect("just set"))
    }

    /// Replays one prover call of why3find: the checks of the module
    /// documentation, in order.
    fn check(&mut self, gid: &str, call_prover: &str) -> Result<Outcome, ReplayError> {
        let no = |reason: &str| Ok(Outcome::NotConfirmed(reason.to_string()));
        let Some(goal) = self.goals.get(gid) else { return no("the Goal was not announced") };
        let (file, key) = (goal.file.clone(), goal.key.clone());
        let proof = &self.files[&file];
        let Some(leaf) = proof.leaves.get(&key) else { return no("the proof file has no entry for the Goal") };
        let Some(member) = leaf.member.clone() else { return no("the entry has no dp member") };
        if !proof.lock_ok {
            return no("the proof file was written under another Lock_File");
        }
        let entry = match DpEntry::from_member(&member, ENTRY_LIMIT) {
            Ok(entry) => entry,
            Err(e) => return no(&e.to_string()),
        };
        match label_of(&entry) {
            Label::ProvedStandalone => {}
            // Certificates are replayed with the Certificate_Store.
            Label::Certified => return no("certified Verdicts are not replayed yet"),
            Label::NonProof(_) => {
                return no(&format!("the entry is proved for a legacy reader, and its label is {}", entry.label))
            }
        }
        let [replay] = entry.replay.as_slice() else { return no("the entry does not have one set of replay fields") };
        // The prover and its binary.
        let Some(prover) = self.provers()?.get(call_prover) else { return no("the prover is not configured here") };
        let name = prover.info.prover.split_once('@').map_or(prover.info.prover.as_str(), |(n, _)| n).to_string();
        let (exe, binary_hash, info) = (prover.exe.clone(), prover.binary_hash, prover.info.clone());
        if replay.prover != name {
            return no(&format!("the entry records {}, the legacy members name {name}", replay.prover));
        }
        let Some(version) = prover.unpatched_version.clone() else {
            return no("the local solver binary is not an Unpatched_Solver of the Lock_File");
        };
        if replay.binary_hash != binary_hash || replay.version != version {
            return no("the recorded solver binary is not the local pinned one");
        }
        // The Goal, as this machine prints it.
        let printed = self.bridge.print(gid, call_prover, &[])?;
        if printed.tasks != 1 {
            return no("the Goal does not print as one task");
        }
        let task_hash = hash::task_hash(&printed.bytes, info.digest.as_bytes(), b"");
        if replay.driver != info.digest.as_bytes() || replay.task_hash != task_hash {
            return no("the recorded Task_Hash is not that of the Goal as printed here");
        }
        // The run.
        let velocity = match self.report.calibrations.get(call_prover) {
            Some(v) => *v,
            None => {
                let v = self.bridge.calibration(call_prover)?.max(1);
                self.report.calibrations.insert(call_prover.to_string(), v);
                v
            }
        };
        let limit = calibration::scale(replay.reference_micros, velocity).saturating_mul(LIMIT_FACTOR).max(1);
        let command = match replay.seed {
            Some(seed) => solver::with_seed(&info.command, seed),
            None => info.command.clone(),
        };
        self.runs += 1;
        let mut run = solver::start(
            &exe,
            &command,
            &printed.bytes,
            &info.extension,
            limit,
            &self.tmp,
            &format!("replay{}", self.runs),
        )?;
        let done = loop {
            if let Some(done) = run.poll()? {
                break done;
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        if done.timed_out {
            return no("the solver reached the replay time limit");
        }
        let Some(status) = done.status else { return no("the solver was ended by a signal") };
        match self.bridge.read_answer(call_prover, status, &done.output)? {
            DriverAnswer::Valid => Ok(Outcome::Confirmed {
                prover: name,
                micros: (done.elapsed.as_micros() * 1_000_000 / u128::from(velocity)) as u64,
            }),
            DriverAnswer::Invalid => Ok(Outcome::Contradicted { prover: name }),
            other => no(&format!("the solver answers {}", other.name())),
        }
    }

    fn handle(&mut self, msg: FromWhy3find) -> Result<(), ReplayError> {
        match msg {
            FromWhy3find::Goal { gid, file, theory, name, path, .. } => {
                if !self.files.contains_key(&file) {
                    let path = proof_file(&self.config.dir, &file);
                    let loaded = load(&path, self.config.lock_hash);
                    if !loaded.lock_ok {
                        self.report.lock_mismatches.push(path.display().to_string());
                    }
                    self.files.insert(file.clone(), loaded);
                }
                let steps = path.iter().map(|p| (p.tactic.clone(), p.index)).collect();
                self.goals.insert(gid, Goal { file, key: (theory, name, steps) });
                Ok(())
            }
            FromWhy3find::Prove { rid, gid, prover, micros, .. } => {
                let outcome = self.check(&gid, &prover)?;
                let result = match &outcome {
                    Outcome::Confirmed { micros, .. } => ProverResult::Valid { micros: *micros },
                    Outcome::NotConfirmed(reason) if reason.contains("time limit") => ProverResult::Timeout { micros },
                    _ => ProverResult::Unknown { micros: 0 },
                };
                if let Some(goal) = self.goals.get(&gid) {
                    if let Some(leaf) = self.files.get_mut(&goal.file).and_then(|f| f.leaves.get_mut(&goal.key)) {
                        // A second call for an entry cannot improve on a
                        // confirmation, and the first failure stays.
                        leaf.outcome.get_or_insert(outcome);
                    }
                }
                Ok(self.bridge.send(&ToWhy3find::Answer { rid, result })?)
            }
            _ => Ok(()),
        }
    }

    fn run(&mut self) -> Result<(), ReplayError> {
        loop {
            match self.bridge.poll(Duration::from_millis(50))? {
                Poll::Message(msg) => self.handle(msg)?,
                Poll::Idle => {}
                Poll::Closed => return Ok(()),
            }
        }
    }

    /// One entry per Goal entry of the proof files, whether why3find made
    /// a call for it or not.
    fn finish(mut self) -> Report {
        for (file, proof) in std::mem::take(&mut self.files) {
            for ((theory, goal, steps), leaf) in proof.leaves {
                let entry = leaf.member.as_deref().map(|m| DpEntry::from_member(m, ENTRY_LIMIT));
                let outcome = match (leaf.outcome, entry) {
                    (Some(outcome), _) => outcome,
                    // No prover call was made for the entry.
                    (None, Some(Ok(entry))) => match label_of(&entry) {
                        Label::NonProof(_) if leaf.legacy_prover.is_none() => {
                            Outcome::NotProved { label: entry.label }
                        }
                        Label::NonProof(_) => Outcome::NotConfirmed(format!(
                            "the entry is proved for a legacy reader, and its label is {}",
                            entry.label
                        )),
                        _ => Outcome::NotConfirmed("why3find did not replay the entry".to_string()),
                    },
                    (None, Some(Err(e))) => Outcome::NotConfirmed(e.to_string()),
                    (None, None) if leaf.legacy_prover.is_some() => {
                        Outcome::NotConfirmed("the entry has no dp member".to_string())
                    }
                    (None, None) => Outcome::NotProved { label: "unresolved".to_string() },
                };
                let path: Vec<_> = steps.iter().map(|(t, i)| format!("{t}.{i}")).collect();
                self.report.entries.push(Entry { file: file.clone(), theory, goal, path: path.join("/"), outcome });
            }
        }
        self.report
    }
}

/// Replays the proof files of the given arguments.
pub fn replay(config: Config) -> Result<Report, ReplayError> {
    let platform = current_platform();
    let why3find = fs::canonicalize(&config.why3find)
        .map_err(|e| ReplayError::Pin(format!("{}: {e}", config.why3find.display())))?;
    let actual = hash::sha256(&fs::read(&why3find)?);
    if !config.lock.pins(Kind::Why3find, &platform).any(|(_, h)| h == actual) {
        return Err(ReplayError::Pin(format!("why3find ({}) is not the one of the Lock_File", why3find.display())));
    }
    // why3find's own cache has no say: every lookup is a miss, and nothing
    // is stored.
    let responder = |msg: &FromWhy3find| match msg {
        FromWhy3find::CacheGet { rid, .. } => {
            Some(ToWhy3find::Cache { rid: rid.clone(), result: ProverResult::NoResult })
        }
        FromWhy3find::CacheSet { rid, .. } => Some(ToWhy3find::Stored { rid: rid.clone(), store: false }),
        _ => None,
    };
    let mut args: Vec<OsString> = vec!["-r".into()];
    args.extend(config.args.iter().cloned());
    let bridge = Bridge::start(&why3find, &args, &config.dir, MESSAGE_LIMIT, Box::new(responder))?;
    let tmp = std::env::temp_dir().join(format!("dp-replay-{}", std::process::id()));
    fs::create_dir_all(&tmp)?;
    let mut replay = Replay {
        bridge,
        config,
        platform,
        tmp: tmp.clone(),
        provers: None,
        goals: BTreeMap::new(),
        files: BTreeMap::new(),
        report: Report::default(),
        runs: 0,
    };
    let result = replay.run();
    let _ = fs::remove_dir_all(&tmp);
    result?;
    Ok(replay.finish())
}
