//! The `dispatch` format: messages between the Front_End and why3find in
//! dispatch mode (design decision D-3, C2).
//!
//! Every message is one canonical S-expression. The grammar is in
//! `src/dispatch.mli` of `patches/why3find/`. Times and Calibration
//! velocities are decimal integers, in microseconds and in millionths.

use crate::sexp::Sexp;

/// A why3find prover result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProverResult {
    NoResult,
    Failed,
    Valid { micros: u64 },
    Unknown { micros: u64 },
    Timeout { micros: u64 },
}

/// The step of why3find's strategy that a prover call belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrategyStep {
    /// The first sequential pass: the Local_Fast_Pass.
    Fast,
    /// The reference-time parallel pass.
    Parallel,
    ParallelLong,
    /// A prover call that replays a recorded proof.
    Replay,
}

/// One tactic step of a goal path: the tactic and the index of the child.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathStep {
    pub tactic: String,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProverInfo {
    /// `name@version`, as why3find names the prover.
    pub prover: String,
    /// why3find's digest of the prover configuration and driver files.
    pub digest: String,
    pub driver: String,
    /// Why3's command line, with `%f`, `%t` and the other placeholders.
    pub command: String,
    /// Extension of the prover's input file, with its dot, from the driver.
    pub extension: String,
}

/// The answer that a Why3 driver reads from the output of a prover run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverAnswer {
    Valid,
    Invalid,
    Timeout,
    OutOfMemory,
    StepLimitExceeded,
    Unknown,
    Failure,
    HighFailure,
}

impl DriverAnswer {
    pub const ALL: [DriverAnswer; 8] = [
        DriverAnswer::Valid,
        DriverAnswer::Invalid,
        DriverAnswer::Timeout,
        DriverAnswer::OutOfMemory,
        DriverAnswer::StepLimitExceeded,
        DriverAnswer::Unknown,
        DriverAnswer::Failure,
        DriverAnswer::HighFailure,
    ];

    pub fn name(self) -> &'static str {
        match self {
            DriverAnswer::Valid => "valid",
            DriverAnswer::Invalid => "invalid",
            DriverAnswer::Timeout => "timeout",
            DriverAnswer::OutOfMemory => "outofmemory",
            DriverAnswer::StepLimitExceeded => "steplimitexceeded",
            DriverAnswer::Unknown => "unknown",
            DriverAnswer::Failure => "failure",
            DriverAnswer::HighFailure => "highfailure",
        }
    }

    pub fn decode(s: &Sexp) -> Result<DriverAnswer, DecodeError> {
        let name = s.as_str();
        DriverAnswer::ALL
            .into_iter()
            .find(|a| Some(a.name()) == name)
            .ok_or_else(|| DecodeError("driver answer".into()))
    }
}

/// A message from why3find.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FromWhy3find {
    Hello { version: u32 },
    Goal { gid: String, file: String, theory: String, name: String, path: Vec<PathStep>, task: Vec<u8> },
    CacheGet { rid: String, gid: String, prover: String, micros: u64 },
    Prove { rid: String, gid: String, step: StrategyStep, prover: String, micros: u64 },
    Cancel { rid: String },
    CacheSet { rid: String, gid: String, prover: String, result: ProverResult },
    Done,
    Printed { qid: String, tasks: u32, bytes: Vec<u8> },
    Task { qid: String, task: Vec<u8> },
    Provers { qid: String, provers: Vec<ProverInfo> },
    Calibration { qid: String, millionths: u64 },
    AnswerRead { qid: String, answer: DriverAnswer },
    Error { qid: String, message: String },
}

/// A message to why3find.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToWhy3find {
    Cache { rid: String, result: ProverResult },
    Answer { rid: String, result: ProverResult },
    Stored { rid: String, store: bool },
    Print { qid: String, gid: String, prover: String, transformations: Vec<String> },
    TaskPrint { qid: String, gid: String },
    Provers { qid: String },
    Calibration { qid: String, prover: String },
    /// Asks for the answer that the driver of `prover` reads from the output
    /// and exit status of a run.
    ReadAnswer { qid: String, prover: String, status: u32, output: Vec<u8> },
}

/// A well-formed S-expression that is not a message of the format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not a dispatch message: {}", self.0)
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

/// A decimal integer in canonical form: no sign and no leading zeros.
fn number(s: &Sexp) -> Result<u64, DecodeError> {
    let t = s.as_str().ok_or_else(|| DecodeError("expected a number".into()))?;
    let canonical = !t.is_empty()
        && t.bytes().all(|b| b.is_ascii_digit())
        && (t == "0" || !t.starts_with('0'));
    match t.parse() {
        Ok(v) if canonical => Ok(v),
        _ => err("expected a canonical decimal number"),
    }
}

fn small(s: &Sexp) -> Result<u32, DecodeError> {
    u32::try_from(number(s)?).or_else(|_| err("number out of range"))
}

fn items(s: &Sexp) -> Result<&[Sexp], DecodeError> {
    s.as_list().ok_or_else(|| DecodeError("expected a list".into()))
}

impl ProverResult {
    pub fn encode(self) -> Sexp {
        match self {
            ProverResult::NoResult => Sexp::list([a("noresult")]),
            ProverResult::Failed => Sexp::list([a("failed")]),
            ProverResult::Valid { micros } => Sexp::list([a("valid"), n(micros)]),
            ProverResult::Unknown { micros } => Sexp::list([a("unknown"), n(micros)]),
            ProverResult::Timeout { micros } => Sexp::list([a("timeout"), n(micros)]),
        }
    }

    pub fn decode(s: &Sexp) -> Result<ProverResult, DecodeError> {
        let l = items(s)?;
        let tag = l.first().and_then(Sexp::as_str);
        match (tag, &l[l.len().min(1)..]) {
            (Some("noresult"), []) => Ok(ProverResult::NoResult),
            (Some("failed"), []) => Ok(ProverResult::Failed),
            (Some("valid"), [t]) => Ok(ProverResult::Valid { micros: number(t)? }),
            (Some("unknown"), [t]) => Ok(ProverResult::Unknown { micros: number(t)? }),
            (Some("timeout"), [t]) => Ok(ProverResult::Timeout { micros: number(t)? }),
            _ => err("result"),
        }
    }
}

impl StrategyStep {
    pub fn name(self) -> &'static str {
        match self {
            StrategyStep::Fast => "fast",
            StrategyStep::Parallel => "parallel",
            StrategyStep::ParallelLong => "parallel-long",
            StrategyStep::Replay => "replay",
        }
    }

    fn decode(s: &Sexp) -> Result<StrategyStep, DecodeError> {
        match s.as_str() {
            Some("fast") => Ok(StrategyStep::Fast),
            Some("parallel") => Ok(StrategyStep::Parallel),
            Some("parallel-long") => Ok(StrategyStep::ParallelLong),
            Some("replay") => Ok(StrategyStep::Replay),
            _ => err("strategy step"),
        }
    }
}

impl FromWhy3find {
    pub fn encode(&self) -> Sexp {
        use FromWhy3find::*;
        match self {
            Hello { version } => Sexp::list([a("hello"), n(u64::from(*version))]),
            Goal { gid, file, theory, name, path, task } => Sexp::list([
                a("goal"),
                a(gid),
                a(file),
                a(theory),
                a(name),
                Sexp::list(
                    path.iter().map(|p| Sexp::list([a(&p.tactic), n(u64::from(p.index))])),
                ),
                Sexp::atom(task),
            ]),
            CacheGet { rid, gid, prover, micros } => {
                Sexp::list([a("cache-get"), a(rid), a(gid), a(prover), n(*micros)])
            }
            Prove { rid, gid, step, prover, micros } => {
                Sexp::list([a("prove"), a(rid), a(gid), a(step.name()), a(prover), n(*micros)])
            }
            Cancel { rid } => Sexp::list([a("cancel"), a(rid)]),
            CacheSet { rid, gid, prover, result } => {
                Sexp::list([a("cache-set"), a(rid), a(gid), a(prover), result.encode()])
            }
            Done => Sexp::list([a("done")]),
            Printed { qid, tasks, bytes } => {
                Sexp::list([a("printed"), a(qid), n(u64::from(*tasks)), Sexp::atom(bytes)])
            }
            Task { qid, task } => Sexp::list([a("task"), a(qid), Sexp::atom(task)]),
            Provers { qid, provers } => Sexp::List(
                [a("provers"), a(qid)]
                    .into_iter()
                    .chain(provers.iter().map(|p| {
                        Sexp::list([
                            a(&p.prover),
                            a(&p.digest),
                            a(&p.driver),
                            a(&p.command),
                            a(&p.extension),
                        ])
                    }))
                    .collect(),
            ),
            Calibration { qid, millionths } => {
                Sexp::list([a("calibration"), a(qid), n(*millionths)])
            }
            AnswerRead { qid, answer } => {
                Sexp::list([a("answer-read"), a(qid), a(answer.name())])
            }
            Error { qid, message } => Sexp::list([a("error"), a(qid), a(message)]),
        }
    }

    pub fn decode(s: &Sexp) -> Result<FromWhy3find, DecodeError> {
        use FromWhy3find::*;
        let l = items(s)?;
        let Some(tag) = l.first().and_then(Sexp::as_str) else {
            return err("missing tag");
        };
        match (tag, &l[1..]) {
            ("hello", [v]) => Ok(Hello { version: small(v)? }),
            ("goal", [gid, file, theory, name, path, task]) => Ok(Goal {
                gid: text(gid)?,
                file: text(file)?,
                theory: text(theory)?,
                name: text(name)?,
                path: items(path)?
                    .iter()
                    .map(|p| match items(p)? {
                        [tactic, index] => {
                            Ok(PathStep { tactic: text(tactic)?, index: small(index)? })
                        }
                        _ => err("path step"),
                    })
                    .collect::<Result<_, _>>()?,
                task: bytes(task)?,
            }),
            ("cache-get", [rid, gid, prover, t]) => Ok(CacheGet {
                rid: text(rid)?,
                gid: text(gid)?,
                prover: text(prover)?,
                micros: number(t)?,
            }),
            ("prove", [rid, gid, step, prover, t]) => Ok(Prove {
                rid: text(rid)?,
                gid: text(gid)?,
                step: StrategyStep::decode(step)?,
                prover: text(prover)?,
                micros: number(t)?,
            }),
            ("cancel", [rid]) => Ok(Cancel { rid: text(rid)? }),
            ("cache-set", [rid, gid, prover, r]) => Ok(CacheSet {
                rid: text(rid)?,
                gid: text(gid)?,
                prover: text(prover)?,
                result: ProverResult::decode(r)?,
            }),
            ("done", []) => Ok(Done),
            ("printed", [qid, tasks, b]) => {
                Ok(Printed { qid: text(qid)?, tasks: small(tasks)?, bytes: bytes(b)? })
            }
            ("task", [qid, task]) => Ok(Task { qid: text(qid)?, task: bytes(task)? }),
            ("provers", [qid, provers @ ..]) => Ok(Provers {
                qid: text(qid)?,
                provers: provers
                    .iter()
                    .map(|p| match items(p)? {
                        [prover, digest, driver, command, extension] => Ok(ProverInfo {
                            prover: text(prover)?,
                            digest: text(digest)?,
                            driver: text(driver)?,
                            command: text(command)?,
                            extension: text(extension)?,
                        }),
                        _ => err("prover entry"),
                    })
                    .collect::<Result<_, _>>()?,
            }),
            ("calibration", [qid, v]) => {
                Ok(Calibration { qid: text(qid)?, millionths: number(v)? })
            }
            ("answer-read", [qid, answer]) => {
                Ok(AnswerRead { qid: text(qid)?, answer: DriverAnswer::decode(answer)? })
            }
            ("error", [qid, message]) => Ok(Error { qid: text(qid)?, message: text(message)? }),
            _ => err(tag),
        }
    }
}

impl ToWhy3find {
    pub fn encode(&self) -> Sexp {
        use ToWhy3find::*;
        match self {
            Cache { rid, result } => Sexp::list([a("cache"), a(rid), result.encode()]),
            Answer { rid, result } => Sexp::list([a("answer"), a(rid), result.encode()]),
            Stored { rid, store } => {
                Sexp::list([a("stored"), a(rid), a(if *store { "store" } else { "skip" })])
            }
            Print { qid, gid, prover, transformations } => Sexp::list([
                a("print"),
                a(qid),
                a(gid),
                a(prover),
                Sexp::list(transformations.iter().map(|t| a(t))),
            ]),
            TaskPrint { qid, gid } => Sexp::list([a("task-print"), a(qid), a(gid)]),
            Provers { qid } => Sexp::list([a("provers"), a(qid)]),
            Calibration { qid, prover } => Sexp::list([a("calibration"), a(qid), a(prover)]),
            ReadAnswer { qid, prover, status, output } => Sexp::list([
                a("read-answer"),
                a(qid),
                a(prover),
                n(u64::from(*status)),
                Sexp::atom(output),
            ]),
        }
    }

    pub fn decode(s: &Sexp) -> Result<ToWhy3find, DecodeError> {
        use ToWhy3find::*;
        let l = items(s)?;
        let Some(tag) = l.first().and_then(Sexp::as_str) else {
            return err("missing tag");
        };
        match (tag, &l[1..]) {
            ("cache", [rid, r]) => Ok(Cache { rid: text(rid)?, result: ProverResult::decode(r)? }),
            ("answer", [rid, r]) => {
                Ok(Answer { rid: text(rid)?, result: ProverResult::decode(r)? })
            }
            ("stored", [rid, s]) => Ok(Stored {
                rid: text(rid)?,
                store: match s.as_str() {
                    Some("store") => true,
                    Some("skip") => false,
                    _ => return err("stored"),
                },
            }),
            ("print", [qid, gid, prover, ts]) => Ok(Print {
                qid: text(qid)?,
                gid: text(gid)?,
                prover: text(prover)?,
                transformations: items(ts)?.iter().map(text).collect::<Result<_, _>>()?,
            }),
            ("task-print", [qid, gid]) => Ok(TaskPrint { qid: text(qid)?, gid: text(gid)? }),
            ("provers", [qid]) => Ok(Provers { qid: text(qid)? }),
            ("calibration", [qid, prover]) => {
                Ok(Calibration { qid: text(qid)?, prover: text(prover)? })
            }
            ("read-answer", [qid, prover, status, output]) => Ok(ReadAnswer {
                qid: text(qid)?,
                prover: text(prover)?,
                status: small(status)?,
                output: bytes(output)?,
            }),
            _ => err(tag),
        }
    }
}
