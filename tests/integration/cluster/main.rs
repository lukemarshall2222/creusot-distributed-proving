//! End-to-end tests of the first distributed slice: a Coordinator, Workers
//! and `dp-frontend prove` on one machine, with real provers.
//!
//! They need the development shell (`DP_WHY3FIND`, `DP_LOCK`,
//! `DP_CREUSOT_HOME`, the pinned provers on PATH) and the workspace
//! executables (`cargo build --workspace`). Without them the tests pass with
//! a notice.

use std::{
    fs,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::Duration,
};

use dp_coordinator::shell::Shell;
use dp_formats::cluster::Message;
use dp_shell::{
    env::{Env as EnvTrait, NetEvent, OsEnv, Peer},
    trace::TraceWriter,
};

const ARITH: &str = r#"module Arith
  use int.Int

  goal Easy: forall x y: int. x + y = y + x

  goal Split: (forall x: int. x * 0 = 0) /\ (forall a b: int. a < b -> a + 1 <= b)

  goal Wrong: forall x: int. x * x > x
end
"#;

/// The Front_End of this build.
fn frontend() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/dp-frontend")
}

struct Env {
    why3find: PathBuf,
    lock: PathBuf,
    why3_conf: PathBuf,
    bin: PathBuf,
}

fn env() -> Option<Env> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug");
    match (var("DP_WHY3FIND"), var("DP_LOCK"), var("DP_CREUSOT_HOME")) {
        (Some(why3find), Some(lock), Some(home))
            if bin.join("dp-coordinator").exists() && bin.join("dp-worker").exists() && frontend().exists() =>
        {
            Some(Env { why3find, lock, why3_conf: home.join("creusot_why3.conf"), bin })
        }
        _ => {
            // In CI a skipped test would be a check that checked nothing.
            assert!(
                std::env::var_os("DP_REQUIRE_TOOLS").is_none(),
                "development shell or workspace executables missing"
            );
            eprintln!("development shell or workspace executables missing: cluster test skipped");
            None
        }
    }
}

/// Processes that are killed when the test ends.
struct Guard(Vec<Child>);

impl Drop for Guard {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn wait_for_port(port: u16) {
    for _ in 0..200 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("nothing listens on port {port}");
}

fn run(program: &Path, args: &[&str]) -> String {
    let out = Command::new(program).args(args).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

struct Setup {
    dir: PathBuf,
    operator_key: String,
    /// Verification keys of the Workers `w1` and `w2`.
    worker_keys: Vec<String>,
}

/// A project whose prover calls all go to the Cluster: with no fast pass
/// configured, why3find starts with the reference-time parallel pass.
fn setup(e: &Env, name: &str) -> Setup {
    let dir = std::env::temp_dir().join(format!("dp-cluster-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("arith.mlw"), ARITH).unwrap();
    fs::write(
        dir.join("why3find.json"),
        r#"{ "fast": 0.0, "time": 1.0, "depth": 4, "packages": [], "provers": [ "alt-ergo", "z3" ], "tactics": [ "split_vc" ], "drivers": [], "warnoff": [] }"#,
    )
    .unwrap();
    let key = |file: &str| dir.join(file).display().to_string();
    let operator_key = run(&e.bin.join("dp-coordinator"), &["operator-keygen", &key("operator.key")]);
    let worker_keys = ["w1", "w2"]
        .iter()
        .map(|w| run(&e.bin.join("dp-worker"), &["keygen", &key(&format!("{w}.key"))]))
        .collect();
    Setup { dir, operator_key, worker_keys }
}

fn sign_registry(e: &Env, s: &Setup, workers: &[(&str, &str)]) {
    let entries: Vec<String> = workers.iter().map(|(id, key)| format!("{id}={key}")).collect();
    let mut args = vec!["sign-registry".to_string(), s.dir.join("operator.key").display().to_string(), "1".to_string()];
    args.extend(entries);
    let out = Command::new(e.bin.join("dp-coordinator")).args(&args).output().unwrap();
    assert!(out.status.success());
    fs::write(s.dir.join("registry"), out.stdout).unwrap();
}

/// Starts the Workers `w1` and `w2` and waits until both have connected.
fn start_workers(e: &Env, s: &Setup, port: u16) -> Guard {
    start_workers_with(e, s, port, false).0
}

/// With `calibrate`, each Worker first runs why3find's Calibration. Returns
/// the Workers and the lines in which they state their offers.
fn start_workers_with(e: &Env, s: &Setup, port: u16, calibrate: bool) -> (Guard, Vec<String>) {
    use std::io::{BufRead, BufReader};
    let mut workers = Vec::new();
    let mut lines = Vec::new();
    for w in ["w1", "w2"] {
        let mut command = Command::new(e.bin.join("dp-worker"));
        if calibrate {
            command.arg("run").arg("--why3find").arg(&e.why3find).env("WHY3CONFIG", &e.why3_conf);
        } else {
            command.arg("run");
        }
        let mut child = command
            .args(["--coordinator", &format!("127.0.0.1:{port}"), "--id", w, "--with-parent", "yes"])
            .arg("--key")
            .arg(s.dir.join(format!("{w}.key")))
            .arg("--why3-conf")
            .arg(&e.why3_conf)
            .arg("--lock")
            .arg(&e.lock)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        assert!(line.contains("connected"), "{line:?}");
        lines.push(line);
        workers.push(child);
    }
    // The Coordinator reads the Workers' first messages a moment later.
    thread::sleep(Duration::from_millis(500));
    (Guard(workers), lines)
}

fn prove(e: &Env, s: &Setup, port: u16) -> Output {
    prove_with_seeds(e, s, port, 1)
}

fn prove_with_seeds(e: &Env, s: &Setup, port: u16, seeds: u32) -> Output {
    Command::new(frontend())
        .env("DP_PORTFOLIO_SEEDS", seeds.to_string())
        .args(["prove", "--no-autodetect", "--no-autoload", "-f", "arith.mlw"])
        .current_dir(&s.dir)
        .env("WHY3CONFIG", &e.why3_conf)
        .env("DP_WHY3FIND", &e.why3find)
        .env("DP_LOCK", &e.lock)
        .env("DP_CLUSTER", format!("tcp://127.0.0.1:{port}"))
        .env("DP_REGISTRY", s.dir.join("registry"))
        .env("DP_OPERATOR_KEY", &s.operator_key)
        .env("DP_JOB_MARGIN_SECONDS", "1")
        .output()
        .unwrap()
}

/// The summary line says that the Session has `verdicts` proof-labelled
/// Verdicts, that every proof came from a Worker's Result, and that none
/// came from the Result_Cache. Two provers may both prove a Goal before
/// the cancellation of the slower one arrives, so there can be more proofs
/// from Workers than Verdicts.
fn all_from_workers(stderr: &str, verdicts: u64) -> bool {
    let Some(line) = stderr.lines().find(|l| l.contains("proved-standalone Verdicts (")) else { return false };
    let number = |before: &str| -> Option<u64> {
        let end = line.find(before)?;
        line[..end].trim_end().rsplit(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
    };
    let proved_runs: Option<u64> = line
        .split("prover results: ")
        .nth(1)
        .and_then(|results| results.split(", ").find(|p| p.ends_with("proved-standalone")))
        .and_then(|p| p.split(' ').next())
        .and_then(|n| n.parse().ok());
    number(" proved-standalone Verdicts (") == Some(verdicts)
        && number(" Result_Cache hits") == Some(0)
        && number(" from Workers") >= Some(verdicts)
        && number(" from Workers") == proved_runs
}

fn proofs(s: &Setup) -> String {
    fs::read_to_string(s.dir.join("why3find.proofs/arith.json")).unwrap_or_default()
}

#[test]
fn workers_prove_what_the_provers_prove() {
    let Some(e) = env() else { return };
    let s = setup(&e, "honest");
    sign_registry(&e, &s, &[("w1", &s.worker_keys[0]), ("w2", &s.worker_keys[1])]);
    let port = free_port();
    let coordinator = Command::new(e.bin.join("dp-coordinator"))
        .args(["--listen", &format!("127.0.0.1:{port}"), "--with-parent", "yes"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _guard = Guard(vec![coordinator]);
    wait_for_port(port);
    let _workers = start_workers(&e, &s, port);
    thread::sleep(Duration::from_millis(300));
    let out = prove(&e, &s, port);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("Theory arith.Arith: ✘ (2/3)"), "{stderr}");
    assert!(stderr.contains("Goal Arith.Wrong"), "{stderr}");
    // every proof came from a Worker's signed Result
    assert!(all_from_workers(&stderr, 2), "{stderr}");
    assert!(!stderr.contains("discarded"), "{stderr}");
    let proofs = proofs(&s);
    // Easy and Split are proved, Wrong is not.
    assert_eq!(proofs.matches("\"prover\"").count(), 2, "{proofs}");
    assert_eq!(proofs.matches("\"dp\"").count(), 3, "{proofs}");
    fs::remove_dir_all(&s.dir).unwrap();
}

/// R5.2: on a Cluster the certificate is produced by a Worker and checked
/// by the Front_End; no solver is an Unpatched_Solver for this Front_End,
/// so every proof needs one.
#[test]
fn workers_produce_certificates_that_the_front_end_checks() {
    use dp_formats::proofjson::DpEntry;
    let Some(e) = env() else { return };
    if std::env::var_os("DP_ETHOS").is_none() || std::env::var_os("DP_CPC_SIGNATURE").is_none() {
        assert!(std::env::var_os("DP_REQUIRE_TOOLS").is_none(), "DP_ETHOS or DP_CPC_SIGNATURE is not set");
        return;
    }
    let s = setup(&e, "certifying");
    fs::write(
        s.dir.join("why3find.json"),
        r#"{ "fast": 0.0, "time": 1.0, "depth": 4, "packages": [], "provers": [ "alt-ergo", "cvc5" ], "tactics": [ "split_vc" ], "drivers": [], "warnoff": [] }"#,
    )
    .unwrap();
    fs::write(s.dir.join("dp.toml"), "[certification]\nenabled = true\n").unwrap();
    // The Front_End's Lock_File has the solvers pinned, but none as an
    // Unpatched_Solver. The Workers keep theirs.
    let lock = fs::read_to_string(&e.lock).unwrap().replace("unpatched-solver", "sharing-solver");
    let lock_path = s.dir.join("front-end-lock.toml");
    fs::write(&lock_path, lock).unwrap();
    sign_registry(&e, &s, &[("w1", &s.worker_keys[0]), ("w2", &s.worker_keys[1])]);
    let port = free_port();
    let coordinator = Command::new(e.bin.join("dp-coordinator"))
        .args(["--listen", &format!("127.0.0.1:{port}"), "--with-parent", "yes"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _guard = Guard(vec![coordinator]);
    wait_for_port(port);
    let _workers = start_workers(&e, &s, port);
    let out = Command::new(frontend())
        .args(["prove", "--no-autodetect", "--no-autoload", "-f", "arith.mlw"])
        .current_dir(&s.dir)
        .env("WHY3CONFIG", &e.why3_conf)
        .env("DP_WHY3FIND", &e.why3find)
        .env("DP_LOCK", &lock_path)
        .env("DP_CLUSTER", format!("tcp://127.0.0.1:{port}"))
        .env("DP_REGISTRY", s.dir.join("registry"))
        .env("DP_OPERATOR_KEY", &s.operator_key)
        .env("DP_JOB_MARGIN_SECONDS", "5")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("0 proved-standalone Verdicts"), "{stderr}");
    assert!(stderr.contains("2 certified Verdicts"), "{stderr}");
    assert!(stderr.contains("from Workers") && !stderr.contains("produced here"), "{stderr}");
    for line in proofs(&s).lines().filter(|l| l.contains("\"dp\"")) {
        let entry = DpEntry::from_member(line.split('"').nth(3).unwrap(), 1 << 20).unwrap();
        assert!(entry.label == "certified" || entry.label == "unknown", "{}", entry.label);
        assert_eq!(entry.certificate.is_some(), entry.label == "certified");
    }
    fs::remove_dir_all(&s.dir).unwrap();
}

mod calibration;
mod cluster_off;
mod config;
mod faults;
mod fallback;
mod operator;
mod race;
