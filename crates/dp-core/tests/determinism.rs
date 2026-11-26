// Feature: creusot-distributed-proving, Property 35: The decision core is pure, deterministic and exercised
// P-ID: P7.3.1

//! Determinism of the decision core (R7.3 criteria 1 and 2): every
//! function of the decision-function registry, called on the same inputs,
//! gives the same outputs: twice in one thread, in several threads at
//! once, and in processes that are started one after the other, each with
//! its own hash seeds and its own layout of memory.
//!
//! For each function the test has a way to call it on inputs that are
//! made from a counter, and it compares a digest of the printed outputs
//! of a few hundred calls. The list of functions here must be the list of
//! `tools/purity-check/registry.toml`: a function that is registered and
//! not called here fails the test, so a new decision function comes with
//! its determinism check.

use std::{collections::BTreeMap, process::Command};

use dp_core::{
    cache_key::{self, Atom, LookupFields, ValidationFacts},
    calibration, config, coord,
    fragment::{self, Command as ProofCommand, CommandKind},
    frontend,
    labels::{self, Outcome, ProverAnswer},
    machine::Machine,
    trace::TlaAction,
    verdict::{self, CheckFacts, Context, Hash, Label, RemoteFacts, ResultFields, UnpatchedSolver, Verdict, Verdicts},
    worker,
};
use dp_testkit::gen;
use proptest::{
    strategy::{Strategy, ValueTree},
    test_runner::TestRunner,
};

/// Numbers from a counter: the same in every process.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn flag(&mut self) -> bool {
        self.next() % 2 == 0
    }
    /// Mostly small numbers, so that fields agree often, and sometimes any.
    fn number(&mut self) -> u64 {
        match self.below(8) {
            0 => self.next(),
            1 => u64::MAX - self.below(3),
            _ => self.below(4),
        }
    }
    fn bytes(&mut self) -> Vec<u8> {
        (0..self.below(6)).map(|_| self.below(4) as u8 + b'a').collect()
    }
}

fn hash(r: &mut Rng) -> Hash {
    Hash { w0: r.below(3), w1: 0, w2: r.below(2), w3: 0 }
}

const ANSWERS: [ProverAnswer; 8] = [
    ProverAnswer::Valid,
    ProverAnswer::Invalid,
    ProverAnswer::Timeout,
    ProverAnswer::OutOfMemory,
    ProverAnswer::StepLimitExceeded,
    ProverAnswer::Unknown,
    ProverAnswer::Failure,
    ProverAnswer::HighFailure,
];

const OUTCOMES: [Outcome; 8] = [
    Outcome::StandaloneInvalid,
    Outcome::CheckedIncomplete,
    Outcome::Unchecked,
    Outcome::DependentInvalid,
    Outcome::Timeout,
    Outcome::Unknown,
    Outcome::Failed,
    Outcome::Unresolved,
];

const ACTIONS: [TlaAction; 22] = [
    TlaAction::SessionStart,
    TlaAction::GoalAnnounced,
    TlaAction::ProverCall,
    TlaAction::PrintFacts,
    TlaAction::CacheHit,
    TlaAction::LocalExit,
    TlaAction::RemoteResult,
    TlaAction::JobNotRun,
    TlaAction::CancelCall,
    TlaAction::ClusterLost,
    TlaAction::Tick,
    TlaAction::FinalWrite,
    TlaAction::Certificate,
    TlaAction::FrontEndJoined,
    TlaAction::WorkerJoined,
    TlaAction::Submit,
    TlaAction::Assign,
    TlaAction::Forward,
    TlaAction::Heartbeat,
    TlaAction::PeerLost,
    TlaAction::Operator,
    TlaAction::Discard,
];

fn answer(r: &mut Rng) -> ProverAnswer {
    ANSWERS[r.below(8) as usize]
}

fn outcome(r: &mut Rng) -> Outcome {
    OUTCOMES[r.below(8) as usize]
}

fn label(r: &mut Rng) -> Label {
    match r.below(4) {
        0 => Label::ProvedStandalone,
        1 => Label::Certified,
        _ => Label::NonProof(outcome(r)),
    }
}

fn result(r: &mut Rng) -> ResultFields {
    ResultFields {
        answer: answer(r),
        task_hash: hash(r),
        partition: r.below(5) == 0,
        imports: r.below(5) / 4,
        imports_hash: hash(r),
        prover: r.below(3),
        version: r.below(2),
        binary_hash: hash(r),
    }
}

fn pins(r: &mut Rng) -> Vec<UnpatchedSolver> {
    (0..r.below(4)).map(|_| UnpatchedSolver { prover: r.below(3), version: r.below(2), binary_hash: hash(r) }).collect()
}

fn context(r: &mut Rng) -> Context {
    Context {
        certify_all: r.below(5) == 0,
        import_mark: r.below(5) == 0,
        local_task_hash: hash(r),
        task_count: r.below(3),
        empty_imports_hash: hash(r),
        local_binary_mismatch: r.below(5) == 0,
        unpatched: pins(r),
    }
}

fn remote(r: &mut Rng) -> RemoteFacts {
    let mostly = |r: &mut Rng| r.below(6) != 0;
    RemoteFacts {
        signature_valid: mostly(r),
        registered: mostly(r),
        revoked: !mostly(r),
        submitted: mostly(r),
        limits_consistent: mostly(r),
        imports_consistent: mostly(r),
    }
}

fn check_facts(r: &mut Rng) -> CheckFacts {
    let mostly = |r: &mut Rng| r.below(5) != 0;
    CheckFacts {
        produced: mostly(r),
        checker_pinned: mostly(r),
        signature_pinned: mostly(r),
        bound_to_local_print: mostly(r),
        accepted: mostly(r),
        complete: mostly(r),
        stored: mostly(r),
    }
}

fn verdicts(r: &mut Rng) -> Verdicts {
    let mut goal = 0;
    Verdicts {
        entries: (0..r.below(5))
            .map(|_| {
                goal += 1 + r.below(3);
                Verdict { goal, label: label(r) }
            })
            .collect(),
    }
}

fn lookup(r: &mut Rng) -> LookupFields {
    LookupFields {
        goal_path: r.bytes(),
        task_hash: hash(r),
        driver: r.bytes(),
        pipeline: r.bytes(),
        prover: r.bytes(),
        version: r.bytes(),
        seed: if r.flag() { Some(r.number()) } else { None },
        micros: r.number(),
        import_mark: r.flag(),
        options: (0..r.below(3)).map(|_| r.bytes()).collect(),
    }
}

fn atoms(r: &mut Rng) -> Vec<Atom> {
    (0..r.below(4)).map(|_| if r.flag() { Atom::Bytes(r.bytes()) } else { Atom::Num(r.below(3)) }).collect()
}

fn workers(r: &mut Rng) -> Vec<coord::Worker> {
    (0..r.below(5))
        .map(|peer| coord::Worker {
            peer,
            offers: (0..r.below(3)).map(|_| coord::Offer { prover: r.below(3), velocity: r.number().max(1) }).collect(),
            busy: if r.below(3) == 0 { Some(job(r)) } else { None },
            heard: r.number(),
            draining: r.below(4) == 0,
        })
        .collect()
}

fn job(r: &mut Rng) -> coord::Job {
    coord::Job { id: r.number(), front_end: r.below(3), prover: r.below(3), micros: r.number(), lost: r.below(3) }
}

/// Values of a generator of the test kit, from a fixed seed: the same in
/// every process.
fn generated<S: Strategy>(strategy: S, count: usize) -> Vec<S::Value> {
    let mut runner = TestRunner::deterministic();
    (0..count).map(|_| strategy.new_tree(&mut runner).expect("a value").current()).collect()
}

fn frontend_config() -> frontend::Config {
    frontend::Config {
        local_jobs: 2,
        seeds: 2,
        fast_remote: false,
        certify: false,
        local_racer: false,
        job_margin: 10,
        silence_limit: 0,
        beat_period: u64::MAX,
        certify_all: false,
        unpatched: vec![UnpatchedSolver { prover: 1, version: 1, binary_hash: gen::session_events::h(1) }],
        empty_imports_hash: gen::session_events::h(0),
    }
}

/// The outputs of one function on its inputs, printed.
type Call = fn(&mut Rng) -> String;

/// How many calls of a function go into its digest.
const CALLS: u64 = 300;

/// Every function of the registry, by its path there.
const FUNCTIONS: [(&str, Call); 30] = [
    ("verdict::accept_result", |r| format!("{:?}", verdict::accept_result(result(r), &context(r)))),
    ("verdict::accept_remote", |r| format!("{:?}", verdict::accept_remote(result(r), &context(r), remote(r)))),
    ("verdict::label", |r| format!("{:?}", verdict::label(result(r), &context(r)))),
    ("verdict::cache_label", |r| format!("{:?}", verdict::cache_label(result(r), &context(r)))),
    ("verdict::record_verdict", |r| format!("{:?}", verdict::record_verdict(verdicts(r), r.below(12), label(r)))),
    ("verdict::final_label", |r| {
        let recorded = if r.below(3) == 0 { Some(label(r)) } else { None };
        let outcomes: Vec<Outcome> = (0..r.below(5)).map(|_| outcome(r)).collect();
        format!("{:?}", verdict::final_label(recorded, &outcomes))
    }),
    ("verdict::decode_label", |r| {
        let names: [&[u8]; 6] = [b"proved-standalone", b"certified", b"unknown", b"", b"proved-standalonE", b"certified "];
        let name = names[r.below(6) as usize];
        format!("{:?}", verdict::decode_label(name, b"proved-standalone", b"certified"))
    }),
    ("verdict::conflict", |r| format!("{:?}", verdict::conflict(label(r), r.below(3)))),
    ("verdict::label_from_checker_output", |r| format!("{:?}", verdict::label_from_checker_output(check_facts(r)))),
    ("verdict::same_hash", |r| format!("{:?}", verdict::same_hash(hash(r), hash(r)))),
    ("labels::classify", |r| format!("{:?}", labels::classify(answer(r), r.flag()))),
    ("labels::most_informative", |r| {
        let outcomes: Vec<Outcome> = (0..r.below(6)).map(|_| outcome(r)).collect();
        format!("{:?}", labels::most_informative(&outcomes))
    }),
    ("labels::rank_u8", |r| format!("{:?}", outcome(r).rank_u8())),
    ("cache_key::encode_key", |r| format!("{:?}", cache_key::encode_key(&lookup(r)))),
    ("cache_key::lookup_ok", |r| {
        let stored = atoms(r);
        let wanted = if r.flag() { stored.clone() } else { atoms(r) };
        format!("{:?}", cache_key::lookup_ok(&stored, &wanted))
    }),
    ("cache_key::validate_entry", |r| {
        let micros = r.number();
        let velocity = 1 + r.below(3) * 500_000;
        let facts = ValidationFacts {
            prover: r.below(3),
            version: r.below(2),
            binary_hash: hash(r),
            velocity,
            local_micros: if r.below(4) == 0 { r.number() } else { calibration::scale(micros, velocity) },
            micros,
            imports: r.below(5) / 4,
            imports_hash: hash(r),
            recomputed_imports_hash: hash(r),
            local: r.flag(),
        };
        format!("{:?}", cache_key::validate_entry(&facts, &pins(r)))
    }),
    ("config::validate_config", |r| {
        // The settings of the test kit's generator, chosen by the counter.
        thread_local! {
            static SETTINGS: Vec<config::Settings> = generated(gen::config_text::settings(), 64);
        }
        let which = r.below(64) as usize;
        SETTINGS.with(|settings| format!("{:?}", config::validate_config(&settings[which])))
    }),
    ("calibration::scale", |r| {
        let (micros, velocity) = (r.number().wrapping_mul(r.next() | 1), r.number().wrapping_mul(r.next() | 1));
        format!("{:?}", calibration::scale(micros, velocity))
    }),
    ("coord::pick", |r| format!("{:?}", coord::pick(&workers(r), r.below(3)))),
    ("coord::local_limit", |r| format!("{:?}", coord::local_limit(job(r), r.number()))),
    ("coord::step", |r| {
        // A run of the machine over inputs of the test kit's generator;
        // the state after each step and what the step returned.
        thread_local! {
            static INPUTS: Vec<coord::Input> = generated(gen::coordinator_ops::input(), 512);
        }
        let config = coord::Config { silence_limit: 100, beat_period: 20, retries: 2, serial: 0 };
        let mut state = coord::Coordinator::init(&config);
        let mut out = String::new();
        let start = r.below(480) as usize;
        INPUTS.with(|inputs| {
            for input in &inputs[start..start + 24] {
                let step = coord::step(state.clone(), input.clone());
                out.push_str(&format!("{:?}{:?}{:?}", step.state, step.effects, step.action));
                state = step.state;
            }
        });
        out
    }),
    ("frontend::decide", |r| {
        let evidence = frontend::Evidence {
            fields: result(r),
            local_task_hash: hash(r),
            task_count: r.below(3),
            binary_mismatch: r.below(5) == 0,
        };
        let facts = if r.flag() { Some(remote(r)) } else { None };
        format!("{:?}", frontend::decide(evidence, &context(r), facts))
    }),
    ("frontend::step", |r| {
        thread_local! {
            static SESSIONS: Vec<Vec<frontend::Input>> = generated(gen::session_events::session(), 48);
        }
        let which = r.below(48) as usize;
        let mut state = frontend::FrontEnd::init(&frontend_config());
        let mut out = String::new();
        SESSIONS.with(|sessions| {
            for input in &sessions[which] {
                let step = frontend::step(state.clone(), input.clone());
                out.push_str(&format!("{:?}{:?}{:?}", step.state, step.effects, step.action));
                state = step.state;
            }
        });
        out
    }),
    ("frontend::position", |r| {
        let mut goal = 0;
        let goals: Vec<u64> = (0..r.below(6))
            .map(|_| {
                goal += 1 + r.below(3);
                goal
            })
            .collect();
        format!("{:?}", frontend::position(&goals, r.below(14)))
    }),
    ("frontend::reply_of", |r| format!("{:?}", frontend::reply_of(outcome(r)))),
    ("frontend::verdict_of", |r| format!("{:?}", frontend::verdict_of(&verdicts(r), r.below(12)))),
    ("worker::step", |r| {
        let config = worker::Config { silence_limit: 100, beat_period: 20 };
        let mut state = worker::Worker::init(&config);
        let mut now = 0;
        let mut out = String::new();
        for _ in 0..24 {
            now += r.below(60);
            let job = r.below(3);
            let input = match r.below(8) {
                0 => worker::Input::Connected { now },
                1 => worker::Input::Assign { now, job },
                2 => worker::Input::Cancel { now, job },
                3 => worker::Input::Heartbeat { now },
                4 => worker::Input::SolverExit { now, job },
                5 => worker::Input::StartFailed { now, job },
                6 => worker::Input::Disconnected { now },
                _ => worker::Input::Tick { now },
            };
            let step = worker::step(state, input);
            out.push_str(&format!("{:?}{:?}{:?}", step.state, step.effects, step.action));
            state = step.state;
        }
        out
    }),
    ("trace::name", |r| ACTIONS[r.below(22) as usize].name().to_string()),
    ("fragment::fragment_ok", |r| {
        let commands: Vec<ProofCommand> = (0..r.below(8)).map(|_| proof_command(r)).collect();
        format!("{:?}", fragment::fragment_ok(&commands))
    }),
    ("fragment::command_ok", |r| format!("{:?}", fragment::command_ok(proof_command(r)))),
];

fn proof_command(r: &mut Rng) -> ProofCommand {
    let kinds = [CommandKind::Assume, CommandKind::Step, CommandKind::AssumePush, CommandKind::StepPop, CommandKind::Other];
    ProofCommand { kind: kinds[r.below(5) as usize], rule_pinned: r.below(4) != 0 }
}

/// FNV-1a: a digest that does not depend on the process.
fn fnv(text: &str, mut digest: u64) -> u64 {
    for byte in text.bytes() {
        digest = (digest ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    digest
}

/// The digest of the outputs of every function, and for each the number
/// of different outputs that its calls gave.
fn digests() -> BTreeMap<&'static str, (u64, usize)> {
    FUNCTIONS
        .iter()
        .map(|(name, call)| {
            // The inputs of a function depend on its name and on nothing else.
            let mut rng = Rng(fnv(name, 0xcbf2_9ce4_8422_2325));
            let mut digest = 0xcbf2_9ce4_8422_2325;
            let mut different = std::collections::BTreeSet::new();
            for _ in 0..CALLS {
                let output = call(&mut rng);
                digest = fnv(&output, digest);
                digest = fnv("\n", digest);
                different.insert(fnv(&output, 0));
            }
            (*name, (digest, different.len()))
        })
        .collect()
}

const CHILD: &str = "DP_DETERMINISM_CHILD";

#[test]
fn every_function_gives_equal_outputs_in_every_thread_and_process() {
    let here = digests();
    if std::env::var_os(CHILD).is_some() {
        // A process that the test started: it says what it computed.
        for (name, (digest, _)) in &here {
            println!("digest {name} {digest:016x}");
        }
        return;
    }
    // The calls do something: no function gives one output for all inputs.
    for (name, (_, different)) in &here {
        assert!(*different >= 2, "{name}: every call gave the same output");
    }
    // Again in this thread, and in four threads at once.
    assert_eq!(digests(), here, "a second run in the same thread differs");
    let threads: Vec<_> = (0..4).map(|_| std::thread::spawn(digests)).collect();
    for thread in threads {
        assert_eq!(thread.join().unwrap(), here, "a thread computed other outputs");
    }
    // In other processes: each has its own seeds for the hash maps of the
    // standard library and its own addresses.
    let exe = std::env::current_exe().unwrap();
    for process in 0..4 {
        let out = Command::new(&exe)
            .args(["every_function_gives_equal_outputs_in_every_thread_and_process", "--exact", "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            // Whatever the outputs may wrongly depend on, it differs.
            .env("TZ", ["UTC", "Asia/Tokyo", "America/Lima", "Europe/Paris"][process])
            .env("LANG", ["C", "de_DE.UTF-8", "ja_JP.UTF-8", "C.UTF-8"][process])
            .current_dir(std::env::temp_dir())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let text = String::from_utf8(out.stdout).unwrap();
        let there: BTreeMap<&str, u64> = text
            .lines()
            // The test harness writes its own words in front of the first line.
            .filter_map(|line| line.find("digest ").map(|at| &line[at + 7..]))
            .map(|line| {
                let (name, digest) = line.split_once(' ').unwrap();
                (name, u64::from_str_radix(digest, 16).unwrap())
            })
            .collect();
        assert_eq!(there.len(), FUNCTIONS.len(), "process {process}: {text}");
        for (name, (digest, _)) in &here {
            assert_eq!(there.get(name), Some(digest), "process {process}: {name} gave other outputs");
        }
    }
}

// R7.3 criterion 4, for this test: the functions called here are those of
// the decision-function registry, no more and no fewer.
#[test]
fn the_functions_checked_are_those_of_the_registry() {
    let registry = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/purity-check/registry.toml")).unwrap();
    let registered: std::collections::BTreeSet<&str> = registry
        .lines()
        .filter_map(|line| line.strip_prefix("path = \""))
        .map(|rest| rest.trim_end_matches('"'))
        .collect();
    let checked: std::collections::BTreeSet<&str> = FUNCTIONS.iter().map(|(name, _)| *name).collect();
    assert_eq!(checked.len(), FUNCTIONS.len(), "a function is listed twice");
    let missing: Vec<_> = registered.difference(&checked).collect();
    let unknown: Vec<_> = checked.difference(&registered).collect();
    assert!(missing.is_empty(), "registered, and without a determinism check: {missing:?}");
    assert!(unknown.is_empty(), "checked here, and not in the registry: {unknown:?}");
}

// The check notices what it is for: a function whose output depends on
// the process (here: the address of a value, or a hash map's order) has
// another digest in another process.
#[test]
fn a_function_that_depends_on_the_process_would_be_noticed() {
    use std::collections::HashMap;
    let order = || -> String {
        let map: HashMap<u64, u64> = (0..64).map(|i| (i, i)).collect();
        format!("{:?}", map.keys().collect::<Vec<_>>())
    };
    if std::env::var_os(CHILD).is_some() {
        println!("order {:016x}", fnv(&order(), 0));
        return;
    }
    let exe = std::env::current_exe().unwrap();
    let mut seen = std::collections::BTreeSet::new();
    seen.insert(format!("{:016x}", fnv(&order(), 0)));
    for _ in 0..6 {
        let out = Command::new(&exe)
            .args(["a_function_that_depends_on_the_process_would_be_noticed", "--exact", "--nocapture"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        seen.extend(text.lines().filter_map(|l| l.find("order ").map(|at| l[at + 6..].to_string())));
    }
    assert!(seen.len() >= 2, "seven processes iterate a hash map in one order: the processes do not differ in their hash seeds");
}
