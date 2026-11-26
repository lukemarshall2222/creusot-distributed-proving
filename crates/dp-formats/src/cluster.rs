//! Messages between the Front_End, the Coordinator and the Workers, signed
//! Results and the operator-signed Worker registry (design: Data Models).
//!
//! This is the message set of the first distributed slice: one Job per
//! prover call, Unpatched_Solvers only, no Lemma traffic.
//! A Result carries the solver's output and exit status, not an answer: the
//! Front_End reads the answer with its own pinned driver.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use crate::{
    hash::Hash,
    sexp::{self, Sexp},
};

const SIG_RESULT: &str = "dp/sig/result/v1";
const SIG_REGISTRY: &str = "dp/sig/registry/v1";
const SIG_OPERATOR: &str = "dp/sig/operator/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not a cluster message: {}", self.0)
    }
}

impl std::error::Error for DecodeError {}

fn a(s: &str) -> Sexp {
    Sexp::atom(s)
}

fn n(v: u64) -> Sexp {
    Sexp::atom(v.to_string())
}

fn err<T>(what: &str) -> Result<T, DecodeError> {
    Err(DecodeError(what.to_string()))
}

fn text(s: &Sexp) -> Result<String, DecodeError> {
    s.as_str().map(str::to_string).ok_or_else(|| DecodeError("expected a text atom".into()))
}

fn bytes(s: &Sexp) -> Result<Vec<u8>, DecodeError> {
    s.as_atom().map(<[u8]>::to_vec).ok_or_else(|| DecodeError("expected an atom".into()))
}

fn number(s: &Sexp) -> Result<u64, DecodeError> {
    let t = s.as_str().ok_or_else(|| DecodeError("expected a number".into()))?;
    let canonical =
        !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) && (t == "0" || !t.starts_with('0'));
    match t.parse() {
        Ok(v) if canonical => Ok(v),
        _ => err("expected a canonical decimal number"),
    }
}

fn hash(s: &Sexp) -> Result<Hash, DecodeError> {
    let b = s.as_atom().ok_or_else(|| DecodeError("expected a hash".into()))?;
    <[u8; 32]>::try_from(b).map(Hash).or_else(|_| err("a hash has 32 bytes"))
}

fn items(s: &Sexp) -> Result<&[Sexp], DecodeError> {
    s.as_list().ok_or_else(|| DecodeError("expected a list".into()))
}

/// What a Job asks a Worker for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    /// The prover's answer on the Printed_Goal.
    Prove,
    /// A certificate: the prover proves the Printed_Goal again and prints
    /// its proof, which is the Result's output (R5.2). The proof is not
    /// trusted; the Front_End has the pinned checker check it.
    Certify,
}

/// One Job: one run of one prover on one Printed_Goal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub kind: JobKind,
    /// Names the Session and, within it, the Job.
    pub job_id: String,
    /// The prover, as the prover configuration names it (`name@version`).
    pub prover: String,
    /// Identifies the Driver that printed the goal.
    pub driver: Vec<u8>,
    /// Extension of the prover's input file, with its dot.
    pub extension: String,
    /// The reference time limit in microseconds.
    pub micros: u64,
    /// The seed of the Solver_Configuration, or `None` for the seed of the
    /// Worker's prover configuration. A number is all a Job can add to the
    /// command line that a Worker runs.
    pub seed: Option<u64>,
    /// The Printed_Goal.
    pub goal: Vec<u8>,
}

fn optional(v: Option<u64>) -> Sexp {
    match v {
        Some(v) => Sexp::list([n(v)]),
        None => Sexp::list([]),
    }
}

fn optional_number(s: &Sexp) -> Result<Option<u64>, DecodeError> {
    match items(s)? {
        [] => Ok(None),
        [v] => Ok(Some(number(v)?)),
        _ => err("optional number"),
    }
}

impl Job {
    fn encode(&self) -> Vec<Sexp> {
        vec![
            a(match self.kind {
                JobKind::Prove => "prove",
                JobKind::Certify => "certify",
            }),
            a(&self.job_id),
            a(&self.prover),
            Sexp::atom(&self.driver),
            a(&self.extension),
            n(self.micros),
            optional(self.seed),
            Sexp::atom(&self.goal),
        ]
    }

    fn decode(fields: &[Sexp]) -> Result<Job, DecodeError> {
        match fields {
            [kind, job_id, prover, driver, extension, micros, seed, goal] => Ok(Job {
                kind: match kind.as_str() {
                    Some("prove") => JobKind::Prove,
                    Some("certify") => JobKind::Certify,
                    _ => return err("job kind"),
                },
                job_id: text(job_id)?,
                prover: text(prover)?,
                driver: bytes(driver)?,
                extension: text(extension)?,
                micros: number(micros)?,
                seed: optional_number(seed)?,
                goal: bytes(goal)?,
            }),
            _ => err("job"),
        }
    }
}

/// The fields of a Result that its Worker signs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultFields {
    pub job_id: String,
    /// Task_Hash of the Printed_Goal that the solver ran on.
    pub task_hash: Hash,
    pub prover: String,
    /// The seed that the solver ran with, as the Job named it.
    pub seed: Option<u64>,
    /// Hash of the solver binary that ran.
    pub binary_hash: Hash,
    pub worker: String,
    /// The solver's exit status, when it exited by itself.
    pub status: Option<u32>,
    /// The run was ended at its time limit.
    pub timed_out: bool,
    /// The start of the solver's output.
    pub output: Vec<u8>,
    pub elapsed_micros: u64,
    /// The Worker's Calibration of the prover, in millionths, and the local
    /// time limit that the run had (R1.4 criterion 2).
    pub velocity: u64,
    pub local_micros: u64,
    /// The hash of the set of Lemmas and Instantiation_Hints that the run
    /// imported, the hash of the empty set when there were none (R1.3
    /// criterion 1). A Result lists no imports yet, so any other hash is
    /// one that the Front_End cannot recompute, and it discards the Result
    /// (criterion 7).
    pub imports_hash: Hash,
}

impl ResultFields {
    fn encode(&self) -> Sexp {
        Sexp::list([
            a(&self.job_id),
            Sexp::atom(self.task_hash.0),
            a(&self.prover),
            optional(self.seed),
            Sexp::atom(self.binary_hash.0),
            a(&self.worker),
            match self.status {
                Some(s) => Sexp::list([n(u64::from(s))]),
                None => Sexp::list([]),
            },
            a(if self.timed_out { "timed-out" } else { "exited" }),
            Sexp::atom(&self.output),
            n(self.elapsed_micros),
            n(self.velocity),
            n(self.local_micros),
            Sexp::atom(self.imports_hash.0),
        ])
    }

    fn decode(s: &Sexp) -> Result<ResultFields, DecodeError> {
        match items(s)? {
            [job_id, task_hash, prover, seed, binary_hash, worker, status, end, output, elapsed, velocity, local, imports] => {
                Ok(ResultFields {
                    job_id: text(job_id)?,
                    task_hash: hash(task_hash)?,
                    prover: text(prover)?,
                    seed: optional_number(seed)?,
                    binary_hash: hash(binary_hash)?,
                    worker: text(worker)?,
                    status: match items(status)? {
                        [] => None,
                        [s] => Some(u32::try_from(number(s)?).or_else(|_| err("status"))?),
                        _ => return err("status"),
                    },
                    timed_out: match end.as_str() {
                        Some("timed-out") => true,
                        Some("exited") => false,
                        _ => return err("end"),
                    },
                    output: bytes(output)?,
                    elapsed_micros: number(elapsed)?,
                    velocity: number(velocity)?,
                    local_micros: number(local)?,
                    imports_hash: hash(imports)?,
                })
            }
            _ => err("result fields"),
        }
    }

    /// The bytes that the tag signs: a domain tag followed by every field.
    fn signed_bytes(&self) -> Vec<u8> {
        sexp::print(&Sexp::list([a(SIG_RESULT), self.encode()]))
    }
}

/// A Result with its authentication tag (R1.3 criterion 2, R7.1 criterion 7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedResult {
    pub fields: ResultFields,
    pub tag: [u8; 64],
}

impl SignedResult {
    pub fn sign(fields: ResultFields, key: &SigningKey) -> SignedResult {
        let tag = key.sign(&fields.signed_bytes()).to_bytes();
        SignedResult { fields, tag }
    }

    /// The tag verifies for these exact fields under `key`.
    pub fn verifies(&self, key: &VerifyingKey) -> bool {
        key.verify_strict(&self.fields.signed_bytes(), &Signature::from_bytes(&self.tag)).is_ok()
    }

    /// The Result exactly as it travels: its fields and its tag.
    pub fn encode(&self) -> Vec<Sexp> {
        vec![self.fields.encode(), Sexp::atom(self.tag)]
    }

    pub fn decode(fields: &[Sexp]) -> Result<SignedResult, DecodeError> {
        match fields {
            [f, tag] => Ok(SignedResult {
                fields: ResultFields::decode(f)?,
                tag: <[u8; 64]>::try_from(bytes(tag)?).or_else(|_| err("a tag has 64 bytes"))?,
            }),
            _ => err("signed result"),
        }
    }
}

/// What the operator tells the Coordinator about a Worker (R1.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    /// The Worker gets no new Job; the one it runs ends normally.
    Drain,
    /// The Worker is disconnected; its Job goes to another Worker.
    Remove,
    /// As Remove, and its Results are no longer accepted by anyone
    /// (R1.7 criterion 6): Front_Ends are told.
    Revoke,
}

impl Operation {
    pub fn name(self) -> &'static str {
        match self {
            Operation::Drain => "drain",
            Operation::Remove => "remove",
            Operation::Revoke => "revoke",
        }
    }

    pub fn parse(name: &str) -> Option<Operation> {
        [Operation::Drain, Operation::Remove, Operation::Revoke].into_iter().find(|o| o.name() == name)
    }
}

/// An operator command with the operator's signature. The serial number
/// orders the commands: the Coordinator takes a command only if its number
/// is larger than every one it has taken, so a recorded command cannot be
/// sent again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedCommand {
    pub serial: u64,
    pub operation: Operation,
    pub worker: String,
    pub tag: [u8; 64],
}

impl SignedCommand {
    fn body(serial: u64, operation: Operation, worker: &str) -> Sexp {
        Sexp::list([n(serial), a(operation.name()), a(worker)])
    }

    fn signed_bytes(&self) -> Vec<u8> {
        sexp::print(&Sexp::list([a(SIG_OPERATOR), SignedCommand::body(self.serial, self.operation, &self.worker)]))
    }

    pub fn sign(serial: u64, operation: Operation, worker: &str, operator: &SigningKey) -> SignedCommand {
        let mut command = SignedCommand { serial, operation, worker: worker.to_string(), tag: [0; 64] };
        command.tag = operator.sign(&command.signed_bytes()).to_bytes();
        command
    }

    /// The command carries the operator's signature.
    pub fn verifies(&self, operator: &VerifyingKey) -> bool {
        operator.verify_strict(&self.signed_bytes(), &Signature::from_bytes(&self.tag)).is_ok()
    }

    pub fn encode(&self) -> Vec<Sexp> {
        vec![SignedCommand::body(self.serial, self.operation, &self.worker), Sexp::atom(self.tag)]
    }

    pub fn decode(fields: &[Sexp]) -> Result<SignedCommand, DecodeError> {
        let [body, tag] = fields else { return err("signed command") };
        let [serial, operation, worker] = items(body)? else { return err("command") };
        Ok(SignedCommand {
            serial: number(serial)?,
            operation: operation.as_str().and_then(Operation::parse).ok_or_else(|| DecodeError("operation".into()))?,
            worker: text(worker)?,
            tag: <[u8; 64]>::try_from(bytes(tag)?).or_else(|_| err("a tag has 64 bytes"))?,
        })
    }
}

/// One Worker in a status report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerStatus {
    pub worker: String,
    /// The name of the Job that it runs.
    pub job: Option<String>,
    pub draining: bool,
    pub provers: Vec<String>,
}

/// A message of the cluster protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// First message of a Front_End.
    FrontEndHello { session: String },
    /// First message of a Worker: its identity and its offer, the provers
    /// with their Calibrations in millionths (R1.1 criterion 3).
    WorkerHello { worker: String, offers: Vec<(String, u64)> },
    /// Front_End to Coordinator.
    Submit(Job),
    /// Coordinator to Worker: the Job, the local time limit and the
    /// Calibration that the limit was scaled with.
    Assign { job: Job, local_micros: u64, velocity: u64 },
    /// Any direction: the sender is alive (R1.6).
    Heartbeat,
    /// Worker to Coordinator: alive, and running no Job.
    Idle,
    /// Front_End to Coordinator, and Coordinator to Worker.
    Cancel { job_id: String },
    /// Worker to Coordinator, and Coordinator to Front_End, unchanged.
    Result(SignedResult),
    /// The Job ended without a Result.
    Failed { job_id: String, reason: String },
    /// Anyone to Coordinator: what is the state of the Cluster?
    StatusRequest,
    /// The answer: the connected Workers, the number of queued Jobs and of
    /// connected Front_Ends (R1.7 criterion 9). It is a report of an
    /// untrusted component; nothing is decided from it.
    Status { incarnation: u64, workers: Vec<WorkerStatus>, queued: u64, front_ends: u64 },
    /// Coordinator to a peer that joined: which start of the Coordinator
    /// this is. The number grows with every start, so a peer can tell a
    /// Coordinator that came back from the one it knew.
    Welcome { incarnation: u64 },
    /// Operator to Coordinator.
    Operator(SignedCommand),
    /// Coordinator to operator: whether the command was taken.
    OperatorResult { serial: u64, taken: bool },
    /// Coordinator to Front_End: a revocation, as the operator signed it.
    /// The Front_End checks the signature itself.
    Revocation(SignedCommand),
}

impl Message {
    pub fn encode(&self) -> Sexp {
        let (tag, fields) = match self {
            Message::FrontEndHello { session } => ("frontend-hello", vec![a(session)]),
            Message::WorkerHello { worker, offers } => (
                "worker-hello",
                vec![a(worker), Sexp::list(offers.iter().map(|(p, v)| Sexp::list([a(p), n(*v)])))],
            ),
            Message::Submit(job) => ("submit", job.encode()),
            Message::Assign { job, local_micros, velocity } => {
                ("assign", vec![Sexp::List(job.encode()), n(*local_micros), n(*velocity)])
            }
            Message::Heartbeat => ("heartbeat", vec![]),
            Message::Idle => ("idle", vec![]),
            Message::Cancel { job_id } => ("cancel", vec![a(job_id)]),
            Message::Result(r) => ("result", r.encode()),
            Message::Failed { job_id, reason } => ("failed", vec![a(job_id), a(reason)]),
            Message::StatusRequest => ("status-request", vec![]),
            Message::Welcome { incarnation } => ("welcome", vec![n(*incarnation)]),
            Message::Status { incarnation, workers, queued, front_ends } => (
                "status",
                vec![
                    n(*incarnation),
                    Sexp::list(workers.iter().map(|w| {
                        Sexp::list([
                            a(&w.worker),
                            match &w.job {
                                Some(job) => Sexp::list([a(job)]),
                                None => Sexp::list([]),
                            },
                            a(if w.draining { "draining" } else { "serving" }),
                            Sexp::list(w.provers.iter().map(|p| a(p))),
                        ])
                    })),
                    n(*queued),
                    n(*front_ends),
                ],
            ),
            Message::Operator(command) => ("operator", command.encode()),
            Message::OperatorResult { serial, taken } => {
                ("operator-result", vec![n(*serial), a(if *taken { "taken" } else { "refused" })])
            }
            Message::Revocation(command) => ("revocation", command.encode()),
        };
        Sexp::List(std::iter::once(a(tag)).chain(fields).collect())
    }

    /// Reads a message. One that is not the canonical print of what it
    /// reads as is discarded (R1.3 criterion 8, R7.10 criterion 8).
    pub fn decode(s: &Sexp) -> Result<Message, DecodeError> {
        let message = Message::decode_value(s)?;
        let format = match message {
            Message::Result(_) => crate::canonical_check::RESULT_MESSAGE,
            _ => crate::canonical_check::PROTOCOL_MESSAGE,
        };
        crate::canonical_check::check(format, &sexp::print(s), &sexp::print(&message.encode()))
            .map_err(|e| DecodeError(e.to_string()))?;
        Ok(message)
    }

    fn decode_value(s: &Sexp) -> Result<Message, DecodeError> {
        let l = items(s)?;
        let Some(tag) = l.first().and_then(Sexp::as_str) else {
            return err("missing tag");
        };
        match (tag, &l[1..]) {
            ("frontend-hello", [session]) => Ok(Message::FrontEndHello { session: text(session)? }),
            ("worker-hello", [worker, offers]) => Ok(Message::WorkerHello {
                worker: text(worker)?,
                offers: items(offers)?
                    .iter()
                    .map(|o| match items(o)? {
                        [prover, velocity] => Ok((text(prover)?, number(velocity)?)),
                        _ => err("offer"),
                    })
                    .collect::<Result<_, _>>()?,
            }),
            ("submit", fields) => Ok(Message::Submit(Job::decode(fields)?)),
            ("assign", [job, local, velocity]) => Ok(Message::Assign {
                job: Job::decode(items(job)?)?,
                local_micros: number(local)?,
                velocity: number(velocity)?,
            }),
            ("heartbeat", []) => Ok(Message::Heartbeat),
            ("idle", []) => Ok(Message::Idle),
            ("cancel", [job_id]) => Ok(Message::Cancel { job_id: text(job_id)? }),
            ("result", fields) => Ok(Message::Result(SignedResult::decode(fields)?)),
            ("failed", [job_id, reason]) => {
                Ok(Message::Failed { job_id: text(job_id)?, reason: text(reason)? })
            }
            ("status-request", []) => Ok(Message::StatusRequest),
            ("welcome", [incarnation]) => Ok(Message::Welcome { incarnation: number(incarnation)? }),
            ("status", [incarnation, workers, queued, front_ends]) => Ok(Message::Status {
                incarnation: number(incarnation)?,
                workers: items(workers)?
                    .iter()
                    .map(|w| match items(w)? {
                        [worker, job, state, provers] => Ok(WorkerStatus {
                            worker: text(worker)?,
                            job: match items(job)? {
                                [] => None,
                                [job] => Some(text(job)?),
                                _ => return err("job"),
                            },
                            draining: match state.as_str() {
                                Some("draining") => true,
                                Some("serving") => false,
                                _ => return err("worker state"),
                            },
                            provers: items(provers)?.iter().map(text).collect::<Result<_, _>>()?,
                        }),
                        _ => err("worker status"),
                    })
                    .collect::<Result<_, _>>()?,
                queued: number(queued)?,
                front_ends: number(front_ends)?,
            }),
            ("operator", fields) => Ok(Message::Operator(SignedCommand::decode(fields)?)),
            ("operator-result", [serial, taken]) => Ok(Message::OperatorResult {
                serial: number(serial)?,
                taken: match taken.as_str() {
                    Some("taken") => true,
                    Some("refused") => false,
                    _ => return err("operator result"),
                },
            }),
            ("revocation", fields) => Ok(Message::Revocation(SignedCommand::decode(fields)?)),
            _ => err(tag),
        }
    }
}

/// The Worker registry: the registered Worker identities with their
/// verification keys (R1.3 criterion 9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registry {
    pub version: u64,
    pub workers: Vec<(String, [u8; 32])>,
}

impl Registry {
    fn encode(&self) -> Sexp {
        Sexp::list([
            n(self.version),
            Sexp::list(self.workers.iter().map(|(id, key)| Sexp::list([a(id), Sexp::atom(key)]))),
        ])
    }

    fn signed_bytes(&self) -> Vec<u8> {
        sexp::print(&Sexp::list([a(SIG_REGISTRY), self.encode()]))
    }

    /// The registry file: the registry and the operator's signature.
    pub fn sign(&self, operator: &SigningKey) -> Vec<u8> {
        let tag = operator.sign(&self.signed_bytes()).to_bytes();
        sexp::print(&Sexp::list([a("signed-registry"), self.encode(), Sexp::atom(tag)]))
    }

    /// Reads a registry file and accepts it only with a valid signature of
    /// the operator signing key.
    pub fn verify(file: &[u8], operator: &VerifyingKey, limit: usize) -> Result<Registry, DecodeError> {
        let s = sexp::parse(file, limit).map_err(|e| DecodeError(e.to_string()))?;
        let [tag, registry, signature] = items(&s)? else {
            return err("signed registry");
        };
        if tag.as_str() != Some("signed-registry") {
            return err("signed registry");
        }
        let [version, workers] = items(registry)? else {
            return err("registry");
        };
        let registry = Registry {
            version: number(version)?,
            workers: items(workers)?
                .iter()
                .map(|w| match items(w)? {
                    [id, key] => Ok((
                        text(id)?,
                        <[u8; 32]>::try_from(bytes(key)?).or_else(|_| err("a key has 32 bytes"))?,
                    )),
                    _ => err("registry entry"),
                })
                .collect::<Result<_, _>>()?,
        };
        let signature = <[u8; 64]>::try_from(bytes(signature)?).or_else(|_| err("signature"))?;
        operator
            .verify_strict(&registry.signed_bytes(), &Signature::from_bytes(&signature))
            .map_err(|_| DecodeError("the registry is not signed by the operator key".into()))?;
        Ok(registry)
    }

    /// The verification key of a registered Worker.
    pub fn key(&self, worker: &str) -> Option<VerifyingKey> {
        let (_, key) = self.workers.iter().find(|(id, _)| id == worker)?;
        VerifyingKey::from_bytes(key).ok()
    }
}

/// Hexadecimal text of key and hash files.
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    if text.len() != 2 * N || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}
