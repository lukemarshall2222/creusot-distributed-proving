// Feature: creusot-distributed-proving, Property 32: Canaries never reach a proof label
// P-ID: P7.5.1 (no mode labels a Canary_Goal proved-standalone or certified)

//! The canary harness (R7.5; tasks 9.1 to 9.3).
//!
//! A Canary_Goal is false. `registry.toml` lists the canaries, each with a
//! witness of its falsity; `modes.toml` lists the modes of the System. The
//! harness
//!
//! - checks that every canary has the shape that ties it to its witness;
//! - validates every witness again on every run: unmodified pinned
//!   why3find proves the witness Goals, and the witnesses of the Creusot
//!   canaries are executed;
//! - runs every canary in every mode and fails on any proof label, on a
//!   listed mode that did not run, on a mode that is not listed, and on a
//!   mode that ran too few quantified canaries or proved too few witnesses
//!   (a mode in which nothing is proved shows nothing);
//! - has unmodified pinned why3find read the proof files after each
//!   Session: it must not take a canary for proved either.
//!
//! The modes with real tools need the development shell (`DP_WHY3FIND`,
//! `DP_LOCK`, `DP_CREUSOT_HOME`, `DP_ETHOS`, `DP_CPC_SIGNATURE`) and the
//! workspace executables (`cargo build --workspace`). Without them those
//! tests pass with a notice; with `DP_REQUIRE_TOOLS` they fail.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use dp_formats::{
    cache::Entry,
    dispatch::DriverAnswer,
    proofjson::DpEntry,
};
use serde::Deserialize;
use serde_json::Value;

const GOALS: &str = include_str!("goals/canaries.mlw");
const CREUSOT: &str = include_str!("creusot/lib.rs");
const ORIGINALS: &str = include_str!("creusot/originals.rs");
const MUTANTS: &str = include_str!("creusot/mutants.rs");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    canary: Vec<Canary>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Canary {
    id: String,
    kind: String,
    quantified: bool,
    what: String,
    /// The function, for a Creusot canary.
    goal: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Modes {
    mode: Vec<Mode>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mode {
    name: String,
    kind: String,
    what: String,
    min_quantified: usize,
    min_witnesses_proved: usize,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Exclusions {
    #[serde(default)]
    exclusion: Vec<Exclusion>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Exclusion {
    id: String,
    reason: String,
}

fn here() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn registry() -> Registry {
    toml::from_str(&fs::read_to_string(here().join("registry.toml")).unwrap()).expect("registry.toml")
}

fn modes() -> Modes {
    toml::from_str(&fs::read_to_string(here().join("modes.toml")).unwrap()).expect("modes.toml")
}

fn exclusions() -> Exclusions {
    toml::from_str(&fs::read_to_string(here().join("exclusions.toml")).unwrap()).expect("exclusions.toml")
}

/// The lines of a module of the goals file.
fn module(name: &str) -> Vec<&'static str> {
    let start = GOALS.find(&format!("module {name}\n")).unwrap_or_else(|| panic!("no module {name}"));
    let body = &GOALS[start..];
    body[..body.find("\nend\n").expect("module end")].lines().collect()
}

/// R7.5 criterion 1: `creusot/mutants.rs` is what the specification mutator
/// writes from `creusot/originals.rs`, and every mutant has a witness: its
/// contract is violated when its body runs at the recorded arguments.
#[test]
fn the_mutants_are_those_of_the_mutator_and_each_has_a_witness() {
    let generated = dp_canaries::mutator::generate(ORIGINALS);
    assert!(
        MUTANTS == generated,
        "creusot/mutants.rs is not what the mutator writes; run `cargo run -p dp-canaries --example bless`"
    );
    let mutants = dp_canaries::mutants::MUTANTS;
    assert!(mutants.len() >= 8, "{} mutants", mutants.len());
    for operator in ["flip", "shift", "drop"] {
        assert!(mutants.iter().any(|(_, _, o)| *o == operator), "no mutant of the operator {operator}");
    }
    for (id, function, operator) in mutants {
        assert!(MUTANTS.contains(&format!("pub fn {function}(")), "{id}: no function {function}");
        assert!(mutant_witnessed(id, operator), "{id}: the witness does not show a violation");
    }
    // The check of a `drop` witness can tell a call that returns.
    assert!(!fails_in_a_process("no such mutant"));
}

/// Not a test of its own: the process that `fails_in_a_process` starts.
/// It calls a `drop` mutant at its bad arguments and ends with success
/// only if the call returns.
#[test]
fn witness_process() {
    if let Ok(id) = std::env::var("DP_WITNESS_OF") {
        println!("calling {id}");
        dp_canaries::mutants::call_bad(&id);
        std::process::exit(0);
    }
}

/// Whether the call of `call_bad(id)` ends its process without returning.
fn fails_in_a_process(id: &str) -> bool {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["witness_process", "--exact", "--nocapture", "--test-threads=1"])
        .env("DP_WITNESS_OF", id)
        .output()
        .unwrap();
    // The process got as far as the call, and did not come back from it.
    String::from_utf8_lossy(&out.stdout).contains(&format!("calling {id}")) && !out.status.success()
}

/// The witness of a mutant: its contract is false where its body ran, or,
/// for a dropped precondition, its body fails where the precondition was
/// violated.
fn mutant_witnessed(id: &str, operator: &str) -> bool {
    match operator {
        "drop" => fails_in_a_process(id),
        _ => dp_canaries::mutants::witness(id) == Some(true),
    }
}

/// R7.5 criteria 1 to 4: the registry and the goals file agree, and every
/// canary is tied to its witness by its shape. For a canary `id` with the
/// predicate `id` of `Defs`, the Goal is `forall .... id ARGS` (or `id
/// ARGS` when it is not quantified) and the witness is `not (id ARGS)`:
/// the witness contradicts an instance of the canary, whatever `id` says.
#[test]
fn every_canary_is_tied_to_its_witness() {
    let registry = registry();
    let excluded = exclusions();
    let ids: BTreeSet<&str> = registry.canary.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), registry.canary.len(), "a canary identifier is used twice");
    for e in &excluded.exclusion {
        assert!(ids.contains(e.id.as_str()), "exclusions.toml names the unknown canary {}", e.id);
        assert!(e.reason.trim().len() >= 20, "{}: an exclusion states its reason", e.id);
    }
    let is_excluded = |id: &str| excluded.exclusion.iter().any(|e| e.id == id);
    let (defs, canaries, witnesses) = (module("Defs"), module("Canaries"), module("Witnesses"));
    for c in &registry.canary {
        assert!(!c.what.trim().is_empty(), "{}: says nothing about itself", c.id);
        match c.kind.as_str() {
            "why3" => {
                assert!(c.goal.is_none(), "{}: `goal` is for Creusot canaries", c.id);
                let id = &c.id;
                assert_eq!(
                    defs.iter().filter(|l| l.trim_start().starts_with(&format!("predicate {id} "))).count(),
                    1,
                    "{id}: one predicate in Defs"
                );
                let goal = canaries
                    .iter()
                    .find_map(|l| l.trim_start().strip_prefix(&format!("goal Canary_{id}: ")))
                    .unwrap_or_else(|| panic!("{id}: no Goal Canary_{id}"));
                // After the quantifier, nothing but the predicate applied.
                let statement = match goal.strip_prefix("forall ") {
                    Some(rest) => {
                        assert!(c.quantified, "{id}: quantified in the file, not in the registry");
                        rest.split_once(". ").unwrap_or_else(|| panic!("{id}: {goal}")).1
                    }
                    None => {
                        assert!(!c.quantified, "{id}: quantified in the registry, not in the file");
                        goal
                    }
                };
                assert!(
                    statement.starts_with(&format!("{id} ")) && !statement.contains(['/', '\\', '>', '=']),
                    "{id}: the Goal is not its predicate applied: {goal}"
                );
                if is_excluded(id) {
                    continue;
                }
                let witness = witnesses
                    .iter()
                    .find_map(|l| l.trim_start().strip_prefix(&format!("goal Witness_{id}: ")))
                    .unwrap_or_else(|| panic!("{id}: no witness and no exclusion"));
                assert!(
                    witness.starts_with(&format!("not ({id} ")) && witness.ends_with(')'),
                    "{id}: the witness is not the negated predicate at an argument: {witness}"
                );
            }
            "creusot" => {
                let function = c.goal.as_deref().unwrap_or_else(|| panic!("{}: a Creusot canary names its function", c.id));
                assert!(function.starts_with(&format!("{}_", c.id)), "{}: {function}", c.id);
                assert!(CREUSOT.contains(&format!("pub fn {function}(")), "{}: no function {function}", c.id);
                if is_excluded(&c.id) {
                    continue;
                }
                // The witness is executed, on the code that Creusot is given.
                assert_eq!(
                    dp_canaries::creusot_canaries::witness(&c.id),
                    Some(true),
                    "{}: the witness does not show a violation",
                    c.id
                );
            }
            other => panic!("{}: unknown kind {other}", c.id),
        }
    }
    // Nothing in the files that the registry does not know.
    let listed = |prefix: &str, lines: &[&str]| lines.iter().filter(|l| l.trim_start().starts_with(prefix)).count();
    let why3 = registry.canary.iter().filter(|c| c.kind == "why3").count();
    assert_eq!(listed("goal Canary_", &canaries), why3, "Canaries has a Goal that the registry lacks");
    assert_eq!(listed("goal ", &canaries), why3, "Canaries has a Goal that is no canary");
    assert_eq!(listed("goal ", &witnesses), listed("goal Witness_", &witnesses));
    for m in &modes().mode {
        assert!(matches!(m.kind.as_str(), "why3" | "creusot"), "{}: unknown kind", m.name);
        assert!(!m.what.trim().is_empty() && m.min_quantified >= 1 && m.min_witnesses_proved >= 1, "{}", m.name);
    }
}

// ---- the modes with real tools ----

struct Tools {
    why3find: PathBuf,
    lock: String,
    home: PathBuf,
    bin: PathBuf,
    /// Scratch directory of this run.
    scratch: PathBuf,
}

fn tools(test: &str) -> Option<Tools> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let bin = here().join("../../target/debug");
    let executables = ["dp-frontend", "dp-coordinator", "dp-worker", "dp-replay"].iter().all(|p| bin.join(p).exists());
    match (var("DP_WHY3FIND"), var("DP_LOCK"), var("DP_CREUSOT_HOME"), var("DP_ETHOS"), var("DP_CPC_SIGNATURE")) {
        (Some(why3find), Some(lock), Some(home), Some(_), Some(_)) if executables => {
            let scratch = std::env::temp_dir().join(format!("dp-canaries-{}-{test}", std::process::id()));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).unwrap();
            Some(Tools { why3find, lock: fs::read_to_string(lock).unwrap(), home, bin, scratch })
        }
        _ => {
            // In CI a skipped test would be a check that checked nothing.
            assert!(
                std::env::var_os("DP_REQUIRE_TOOLS").is_none(),
                "development shell or workspace executables missing"
            );
            eprintln!("development shell or workspace executables missing: canary modes skipped");
            None
        }
    }
}

const ARGS: [&str; 5] = ["prove", "--no-autodetect", "--no-autoload", "-f", "canaries.mlw"];

/// A project with the goals file. With `fast` zero why3find has no fast
/// pass, so every prover call can go to a Cluster.
fn project(t: &Tools, name: &str, provers: &str, fast: &str) -> PathBuf {
    let dir = t.scratch.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("canaries.mlw"), GOALS).unwrap();
    fs::write(
        dir.join("why3find.json"),
        format!(
            r#"{{ "fast": {fast}, "time": 1.0, "depth": 4, "packages": [], "provers": [ {provers} ], "tactics": [ "split_vc" ], "drivers": [], "warnoff": [] }}"#
        ),
    )
    .unwrap();
    dir
}

/// A Lock_File of this run, by name.
fn lock_file(t: &Tools, name: &str, text: &str) -> PathBuf {
    let path = t.scratch.join(format!("{name}.lock.toml"));
    fs::write(&path, text).unwrap();
    path
}

/// A Session in `dir`.
fn session(t: &Tools, dir: &Path, lock: &Path, env: &[(&str, String)]) -> String {
    let mut command = Command::new(t.bin.join("dp-frontend"));
    command
        .args(ARGS)
        .current_dir(dir)
        .env("WHY3CONFIG", t.home.join("creusot_why3.conf"))
        .env("DP_WHY3FIND", &t.why3find)
        .env("DP_LOCK", lock);
    for (name, value) in env {
        command.env(name, value);
    }
    let out = command.output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    // Canaries are unproved, so the Session ends with 1; 2 is a failure to run.
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    stderr
}

const PROOF_LABELS: [&str; 2] = ["proved-standalone", "certified"];

/// Whether a node of a proof file is a proof for the Front_End's labels:
/// a leaf with a proof label, or a tactic whose children all are.
fn labelled_proved(node: &Value) -> bool {
    if let Some(dp) = node.get("dp").and_then(Value::as_str) {
        return DpEntry::from_member(dp, 1 << 20).is_ok_and(|e| PROOF_LABELS.contains(&e.label.as_str()));
    }
    node.get("children").and_then(Value::as_array).is_some_and(|c| !c.is_empty() && c.iter().all(labelled_proved))
}

/// Whether a node is a complete proof for a legacy reader: a prover, or a
/// tactic whose children all are complete.
fn legacy_proved(node: &Value) -> bool {
    match node {
        Value::Object(o) if o.contains_key("prover") => true,
        Value::Object(o) => {
            o.get("children").and_then(Value::as_array).is_some_and(|c| !c.is_empty() && c.iter().all(legacy_proved))
        }
        _ => false,
    }
}

/// `Module.Goal` to its node, from the proof file of the goals file.
fn goals(dir: &Path) -> BTreeMap<String, Value> {
    let path = dir.join("why3find.proofs/canaries.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let file: Value = serde_json::from_str(&text).unwrap();
    let mut out = BTreeMap::new();
    for (theory, goals) in file["proofs"].as_object().into_iter().flatten() {
        for (goal, node) in goals.as_object().into_iter().flatten() {
            out.insert(format!("{theory}.{goal}"), node.clone());
        }
    }
    out
}

/// What a mode showed.
struct Ran {
    canaries: usize,
    quantified: usize,
    witnesses_proved: usize,
}

struct Harness {
    start: std::time::Instant,
    registry: Registry,
    ran: BTreeMap<String, Ran>,
    failures: Vec<String>,
}

impl Harness {
    /// Judges the proof file of a Session of mode `mode`: no canary has a
    /// proof label, and none is a proof for a legacy reader.
    fn judge(&mut self, mode: &str, dir: &Path) {
        let goals = goals(dir);
        let mut ran = Ran { canaries: 0, quantified: 0, witnesses_proved: 0 };
        for c in self.registry.canary.iter().filter(|c| c.kind == "why3") {
            match goals.get(&format!("Canaries.Canary_{}", c.id)) {
                Some(node) => {
                    ran.canaries += 1;
                    ran.quantified += usize::from(c.quantified);
                    if labelled_proved(node) {
                        self.failures.push(format!("{mode}: canary {} has a proof label: {node}", c.id));
                    }
                    if legacy_proved(node) {
                        self.failures.push(format!("{mode}: canary {} is a legacy proof: {node}", c.id));
                    }
                }
                None => self.failures.push(format!("{mode}: canary {} did not run", c.id)),
            }
            if goals.get(&format!("Witnesses.Witness_{}", c.id)).is_some_and(labelled_proved) {
                ran.witnesses_proved += 1;
            }
        }
        self.record(mode, ran);
    }

    fn record(&mut self, mode: &str, ran: Ran) {
        eprintln!(
            "canaries: mode {mode}: {} canaries ({} quantified), {} witnesses proved; {:.0} s since the start",
            ran.canaries,
            ran.quantified,
            ran.witnesses_proved,
            self.start.elapsed().as_secs_f64()
        );
        if self.ran.insert(mode.to_string(), ran).is_some() {
            self.failures.push(format!("{mode}: ran twice"));
        }
    }

    /// R7.5 criterion 8: unmodified pinned why3find, given the proof files
    /// that a Session left, does not take a canary for proved.
    fn unmodified_why3find(&mut self, t: &Tools, mode: &str, dir: &Path) {
        let copy = t.scratch.join(format!("{mode}-unmodified"));
        fs::create_dir_all(copy.join("why3find.proofs")).unwrap();
        for file in ["canaries.mlw", "why3find.json", "why3find.proofs/canaries.json"] {
            fs::copy(dir.join(file), copy.join(file)).unwrap();
        }
        let (status, said) = stock(t, &copy);
        if status != Some(1) {
            self.failures
                .push(format!("{mode}: unmodified why3find ended with {status:?} on the Session's proof files: {said}"));
        }
        for (name, node) in goals(&copy) {
            if name.starts_with("Canaries.") && legacy_proved(&node) {
                self.failures.push(format!("{mode}: unmodified why3find takes {name} for proved: {node}"));
            }
        }
    }
}

/// Unmodified pinned why3find, as Creusot_CLI runs it.
fn stock(t: &Tools, dir: &Path) -> (Option<i32>, String) {
    let path = format!("{}:{}", t.home.join("bin").display(), std::env::var("PATH").unwrap());
    let out = Command::new(t.home.join("bin/why3find"))
        .args(ARGS)
        .current_dir(dir)
        .env("PATH", path)
        .env("DUNE_DIR_LOCATIONS", format!("why3find:lib:{}", t.home.join("share/why3find").display()))
        .env("WHY3CONFIG", t.home.join("creusot_why3.conf"))
        .output()
        .unwrap();
    let mut said = String::from_utf8_lossy(&out.stderr).into_owned();
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&out.status) {
        said.push_str(&format!("\n(ended by signal {signal})"));
    }
    (out.status.code(), said)
}

/// R7.5 criteria 2 and 3: every witness is validated again on every run,
/// by a tool that is not the System: unmodified pinned why3find proves
/// every witness Goal, and proves no canary.
#[test]
fn unmodified_why3find_proves_every_witness_and_no_canary() {
    let Some(t) = tools("witnesses") else { return };
    let dir = project(&t, "witnesses", r#""alt-ergo", "z3", "cvc5""#, "0.2");
    let (status, stderr) = stock(&t, &dir);
    assert_eq!(status, Some(1), "{stderr}");
    let goals = goals(&dir);
    let excluded = exclusions();
    for c in registry().canary.iter().filter(|c| c.kind == "why3") {
        assert!(!legacy_proved(&goals[&format!("Canaries.Canary_{}", c.id)]), "{}: proved by unmodified why3find", c.id);
        if excluded.exclusion.iter().any(|e| e.id == c.id) {
            continue;
        }
        assert!(
            legacy_proved(&goals[&format!("Witnesses.Witness_{}", c.id)]),
            "{}: its witness is not proved: {stderr}",
            c.id
        );
    }
    fs::remove_dir_all(&t.scratch).unwrap();
}

/// `(from Workers, Result_Cache hits)` of the summary line of a Session.
fn summary(stderr: &str) -> (u64, u64) {
    let line = stderr
        .lines()
        .find(|l| l.contains("proved-standalone Verdicts ("))
        .unwrap_or_else(|| panic!("no summary: {stderr}"));
    let inner = line.split_once('(').unwrap().1;
    let number = |text: &str| text.trim().split(' ').next().unwrap().parse::<u64>().unwrap();
    let (workers, rest) = inner.split_once(',').unwrap();
    (number(workers), number(rest))
}

/// Processes that are ended when the mode ends.
struct Guard(Vec<Child>);

impl Drop for Guard {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn run(program: &Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new(program).args(args).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    out.stdout
}

/// A Coordinator and two Workers on this machine; returns the environment
/// of a Session that uses them.
fn cluster(t: &Tools, lock: &Path) -> (Guard, Vec<(&'static str, String)>) {
    let dir = t.scratch.join("cluster-keys");
    fs::create_dir_all(&dir).unwrap();
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap().trim().to_string();
    let file = |name: &str| dir.join(name).display().to_string();
    let coordinator = t.bin.join("dp-coordinator");
    let operator = text(run(&coordinator, &["operator-keygen", &file("operator.key")]));
    let mut entries = Vec::new();
    for w in ["w1", "w2"] {
        let key = text(run(&t.bin.join("dp-worker"), &["keygen", &file(&format!("{w}.key"))]));
        entries.push(format!("{w}={key}"));
    }
    let mut args = vec!["sign-registry", "operator.key", "1"];
    args.extend(entries.iter().map(String::as_str));
    let registry = Command::new(&coordinator).args(&args).current_dir(&dir).output().unwrap();
    assert!(registry.status.success(), "{}", String::from_utf8_lossy(&registry.stderr));
    fs::write(dir.join("registry"), registry.stdout).unwrap();

    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut guard = Guard(vec![Command::new(&coordinator)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--with-parent", "yes"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()]);
    assert!(
        (0..200).any(|_| {
            thread::sleep(Duration::from_millis(25));
            TcpStream::connect(("127.0.0.1", port)).is_ok()
        }),
        "the Coordinator does not listen"
    );
    for w in ["w1", "w2"] {
        let mut child = Command::new(t.bin.join("dp-worker"))
            .args(["run", "--coordinator", &format!("127.0.0.1:{port}"), "--id", w, "--with-parent", "yes"])
            .arg("--key")
            .arg(dir.join(format!("{w}.key")))
            .arg("--why3-conf")
            .arg(t.home.join("creusot_why3.conf"))
            .arg("--lock")
            .arg(lock)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        assert!(line.contains("connected"), "{line:?}");
        guard.0.push(child);
    }
    // The Coordinator reads the Workers' first messages a moment later.
    thread::sleep(Duration::from_millis(500));
    let env = vec![
        ("DP_CLUSTER", format!("tcp://127.0.0.1:{port}")),
        ("DP_REGISTRY", dir.join("registry").display().to_string()),
        ("DP_OPERATOR_KEY", operator),
        ("DP_JOB_MARGIN_SECONDS", "1".to_string()),
    ];
    (guard, env)
}

/// The Lock_File in which Alt-Ergo is pinned, but not as an Unpatched_Solver.
fn demoted(lock: &str) -> String {
    let start = lock.find("name = \"alt-ergo\"").unwrap();
    let kind = start + lock[start..].find("unpatched-solver").unwrap();
    format!("{}sharing-solver{}", &lock[..kind], &lock[kind + "unpatched-solver".len()..])
}

/// R7.5 criteria 5 to 8 (Property 32): every canary in every mode.
#[test]
fn no_canary_gets_a_proof_label_in_any_mode() {
    let Some(t) = tools("modes") else { return };
    let mut h = Harness { start: std::time::Instant::now(), registry: registry(), ran: BTreeMap::new(), failures: Vec::new() };
    let pinned = lock_file(&t, "pinned", &t.lock);
    let two = r#""alt-ergo", "z3""#;

    // local, and warm-cache on what it stored.
    let dir = project(&t, "local", two, "0.2");
    session(&t, &dir, &pinned, &[]);
    h.judge("local", &dir);
    h.unmodified_why3find(&t, "local", &dir);
    let stderr = session(&t, &dir, &pinned, &[]);
    if summary(&stderr).1 == 0 {
        h.failures.push(format!("warm-cache: no Result_Cache hit: {stderr}"));
    }
    h.judge("warm-cache", &dir);
    h.unmodified_why3find(&t, "warm-cache", &dir);

    // replay: of that Session, and of one whose stored answers were all
    // forged to Valid. The forged cache gives the Session's own labels to
    // canaries (the local cache is trusted for labels); replay runs the
    // solvers and confirms none of them.
    let replay = |dir: &Path| -> String {
        let out = Command::new(t.bin.join("dp-replay"))
            .args(&ARGS[1..])
            .current_dir(dir)
            .env("WHY3CONFIG", t.home.join("creusot_why3.conf"))
            .env("DP_WHY3FIND", &t.why3find)
            .env("DP_LOCK", &pinned)
            .output()
            .unwrap();
        assert!(matches!(out.status.code(), Some(0 | 1)), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let honest = replay(&dir);
    for file in fs::read_dir(dir.join(".dp-cache")).unwrap().flatten().filter(|f| f.path().is_file()) {
        if let Ok(mut entry) = Entry::decode(&fs::read(file.path()).unwrap(), 1 << 24) {
            entry.answer = DriverAnswer::Valid;
            fs::write(file.path(), entry.encode()).unwrap();
        }
    }
    let forged_session = Command::new(t.bin.join("dp-frontend"))
        .args(ARGS)
        .current_dir(&dir)
        .env("WHY3CONFIG", t.home.join("creusot_why3.conf"))
        .env("DP_WHY3FIND", &t.why3find)
        .env("DP_LOCK", &pinned)
        .output()
        .unwrap();
    let forged_labels = goals(&dir).iter().filter(|(n, v)| n.starts_with("Canaries.") && labelled_proved(v)).count();
    if forged_labels == 0 {
        h.failures.push(format!(
            "replay: the forged cache gave no canary a label, so the replay of it shows nothing: {}",
            String::from_utf8_lossy(&forged_session.stderr)
        ));
    }
    let forged = replay(&dir);
    let mut ran = Ran { canaries: 0, quantified: 0, witnesses_proved: usize::MAX };
    for report in [&honest, &forged] {
        let confirmed: Vec<&str> = report.lines().filter(|l| l.starts_with("confirmed ")).collect();
        for line in &confirmed {
            if line.contains("Canaries.") {
                h.failures.push(format!("replay: a canary is confirmed: {line}"));
            }
        }
        ran.witnesses_proved = ran.witnesses_proved.min(confirmed.iter().filter(|l| l.contains("Witnesses.")).count());
    }
    for c in h.registry.canary.iter().filter(|c| c.kind == "why3") {
        // A canary with a forged label must be answered by replay, not skipped.
        if forged.lines().any(|l| l.starts_with("not-confirmed ") && l.contains(&format!("Canaries.Canary_{} ", c.id))) {
            ran.canaries += 1;
            ran.quantified += usize::from(c.quantified);
        }
    }
    h.record("replay", ran);

    // certification and certify-all: cvc5 produces, Ethos checks.
    let demoted_lock = lock_file(&t, "demoted", &demoted(&t.lock));
    let dir = project(&t, "certification", r#""alt-ergo", "cvc5""#, "0.2");
    fs::write(dir.join("dp.toml"), "[certification]\nenabled = true\n").unwrap();
    let stderr = session(&t, &dir, &demoted_lock, &[]);
    if !stderr.lines().any(|l| l.contains("certified Verdicts") && !l.contains(" 0 certified Verdicts")) {
        h.failures.push(format!("certification: nothing was certified: {stderr}"));
    }
    h.judge("certification", &dir);
    h.unmodified_why3find(&t, "certification", &dir);
    let dir = project(&t, "certify-all", r#""alt-ergo", "cvc5""#, "0.2");
    fs::write(dir.join("dp.toml"), "[certification]\nenabled = true\ncertify_all = true\n").unwrap();
    let stderr = session(&t, &dir, &pinned, &[]);
    if !stderr.contains("0 proved-standalone Verdicts") {
        h.failures.push(format!("certify-all: a proof was labelled without a certificate: {stderr}"));
    }
    h.judge("certify-all", &dir);
    h.unmodified_why3find(&t, "certify-all", &dir);

    // cluster and portfolio: Workers answer every prover call.
    {
        let (_guard, env) = cluster(&t, &pinned);
        let dir = project(&t, "cluster", two, "0.0");
        let stderr = session(&t, &dir, &pinned, &env);
        if summary(&stderr).0 == 0 {
            h.failures.push(format!("cluster: no proof came from a Worker: {stderr}"));
        }
        h.judge("cluster", &dir);
        h.unmodified_why3find(&t, "cluster", &dir);
        let dir = project(&t, "portfolio", two, "0.2");
        fs::write(
            dir.join("dp.toml"),
            "[[solver]]\nname = \"ae\"\nprover = \"alt-ergo\"\n\n[[solver]]\nname = \"z3\"\nprover = \"z3\"\n\n\
             [portfolio]\nrace = [\"ae\", \"z3\"]\nfirst_pass = \"cluster\"\n",
        )
        .unwrap();
        let stderr = session(&t, &dir, &pinned, &env);
        if summary(&stderr).0 == 0 {
            h.failures.push(format!("portfolio: no proof came from a Worker: {stderr}"));
        }
        h.judge("portfolio", &dir);
        h.unmodified_why3find(&t, "portfolio", &dir);
    }

    // creusot-cli: the canary functions through Creusot_CLI.
    {
        let dir = t.scratch.join("creusot-cli");
        fs::create_dir_all(dir.join("src")).unwrap();
        // Cargo takes a crate of the same name for built already when the
        // target directory holds one, and Creusot then has nothing to
        // prove; the crate of each run has its own name.
        let crate_name = format!("dp_canary_functions_{}", std::process::id());
        fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{crate_name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n\
                 [workspace]\n\n[dependencies]\ncreusot-std = \"0.15.0\"\n\n\
                 [lints.rust]\nunexpected_cfgs = {{ level = \"warn\", check-cfg = ['cfg(creusot)'] }}\n"
            ),
        )
        .unwrap();
        // The hand-written canaries, the originals and their mutants.
        fs::write(dir.join("src/lib.rs"), format!("{CREUSOT}\npub mod mutants;\npub mod originals;\n")).unwrap();
        fs::write(dir.join("src/originals.rs"), ORIGINALS).unwrap();
        fs::write(dir.join("src/mutants.rs"), MUTANTS).unwrap();
        fs::write(
            dir.join("why3find.json"),
            r#"{ "fast": 0.2, "time": 1.0, "depth": 4, "packages": [ "creusot" ], "provers": [ "alt-ergo", "z3" ], "tactics": [ "compute_specified", "split_vc" ], "drivers": [], "warnoff": [ "unused_variable", "axiom_abstract" ] }"#,
        )
        .unwrap();
        let out = Command::new("cargo")
            .arg("creusot")
            .arg("--backend")
            .arg(t.bin.join("dp-frontend"))
            .current_dir(&dir)
            .env("CARGO_TARGET_DIR", here().join("../../target/cli-fixture"))
            .env("DP_LOCK", &pinned)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_eq!(out.status.code(), Some(1), "{stderr}");
        // One proof file per function, named by its path in the crate
        // (`control`, `mutants.m01_add_flip1`), with the Goal
        // `vc_<function>`.
        let node = |function: &str| -> Option<Value> {
            let path = dir.join(format!("why3find.proofs/verif.{crate_name}_rlib.{function}.json"));
            let file: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
            let name = format!("vc_{}", function.rsplit('.').next().unwrap_or(function));
            file["proofs"].as_object()?.values().find_map(|theory| theory.get(&name)).cloned()
        };
        let mut ran = Ran { canaries: 0, quantified: 0, witnesses_proved: 0 };
        // A mode in which the function that meets its contract is not
        // proved shows nothing.
        let originals: Vec<String> = ORIGINALS
            .lines()
            .filter_map(|l| l.strip_prefix("pub fn "))
            .map(|l| format!("originals.{}", l.split('(').next().unwrap()))
            .collect();
        assert!(originals.len() >= 4, "{originals:?}");
        for control in std::iter::once("control".to_string()).chain(originals) {
            if !node(&control).is_some_and(|n| labelled_proved(&n)) {
                h.failures.push(format!("creusot-cli: the function {control}, which meets its contract, is not proved: {stderr}"));
            }
        }
        // The mutants: every one is a canary with a universal statement
        // (its contract is over all arguments).
        for (id, function, operator) in dp_canaries::mutants::MUTANTS {
            match node(&format!("mutants.{function}")) {
                Some(n) => {
                    ran.canaries += 1;
                    ran.quantified += 1;
                    if labelled_proved(&n) || legacy_proved(&n) {
                        h.failures.push(format!("creusot-cli: mutant {id} ({function}) is taken for proved: {n}"));
                    }
                }
                None => h.failures.push(format!("creusot-cli: mutant {id} ({function}) did not run: {stderr}")),
            }
            if mutant_witnessed(id, operator) {
                ran.witnesses_proved += 1;
            }
        }
        for c in h.registry.canary.iter().filter(|c| c.kind == "creusot") {
            let function = c.goal.as_deref().unwrap();
            match node(function) {
                Some(n) => {
                    ran.canaries += 1;
                    ran.quantified += usize::from(c.quantified);
                    if labelled_proved(&n) || legacy_proved(&n) {
                        h.failures.push(format!("creusot-cli: canary {} is taken for proved: {n}", c.id));
                    }
                }
                None => h.failures.push(format!("creusot-cli: canary {} did not run: {stderr}", c.id)),
            }
            if dp_canaries::creusot_canaries::witness(&c.id) == Some(true) {
                ran.witnesses_proved += 1;
            }
        }
        h.record("creusot-cli", ran);
    }

    // Every listed mode ran, with enough canaries, and no other mode did.
    let listed = modes();
    for m in &listed.mode {
        let mutants = if m.kind == "creusot" { dp_canaries::mutants::MUTANTS.len() } else { 0 };
        let total = h.registry.canary.iter().filter(|c| c.kind == m.kind).count() + mutants;
        match h.ran.get(&m.name) {
            None => h.failures.push(format!("{}: listed in modes.toml, did not run", m.name)),
            Some(ran) => {
                if ran.canaries != total {
                    h.failures.push(format!("{}: {} of {total} canaries ran", m.name, ran.canaries));
                }
                if ran.quantified < m.min_quantified {
                    h.failures.push(format!("{}: {} quantified canaries, {} required", m.name, ran.quantified, m.min_quantified));
                }
                if ran.witnesses_proved < m.min_witnesses_proved {
                    h.failures.push(format!(
                        "{}: {} witnesses proved, {} required",
                        m.name, ran.witnesses_proved, m.min_witnesses_proved
                    ));
                }
            }
        }
    }
    for name in h.ran.keys() {
        if !listed.mode.iter().any(|m| &m.name == name) {
            h.failures.push(format!("{name}: ran, but modes.toml does not list it"));
        }
    }
    assert!(h.failures.is_empty(), "canary failures:\n{}", h.failures.join("\n"));
    fs::remove_dir_all(&t.scratch).unwrap();
}
