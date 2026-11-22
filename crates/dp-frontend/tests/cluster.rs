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

struct Env {
    why3find: PathBuf,
    lock: PathBuf,
    why3_conf: PathBuf,
    bin: PathBuf,
}

fn env() -> Option<Env> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let bin = Path::new(env!("CARGO_BIN_EXE_dp-frontend")).parent().unwrap().to_path_buf();
    match (var("DP_WHY3FIND"), var("DP_LOCK"), var("DP_CREUSOT_HOME")) {
        (Some(why3find), Some(lock), Some(home))
            if bin.join("dp-coordinator").exists() && bin.join("dp-worker").exists() =>
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
    Command::new(env!("CARGO_BIN_EXE_dp-frontend"))
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

#[test]
fn a_race_of_seeds_proves_the_same_goals() {
    let Some(e) = env() else { return };
    let s = setup(&e, "race");
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
    // Three Jobs with different seeds race on every prover call.
    let out = prove_with_seeds(&e, &s, port, 3);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("Theory arith.Arith: ✘ (2/3)"), "{stderr}");
    assert!(all_from_workers(&stderr, 2), "{stderr}");
    let proofs = proofs(&s);
    // Easy and Split are proved, Wrong is not.
    assert_eq!(proofs.matches("\"prover\"").count(), 2, "{proofs}");
    assert_eq!(proofs.matches("\"dp\"").count(), 3, "{proofs}");
    fs::remove_dir_all(&s.dir).unwrap();
}

#[test]
fn results_of_an_unregistered_worker_prove_nothing() {
    let Some(e) = env() else { return };
    let s = setup(&e, "unregistered");
    // The registry lists the Workers' names with other keys: the Workers that
    // connect are not the registered ones.
    let other = run(&e.bin.join("dp-worker"), &["keygen", &s.dir.join("other.key").display().to_string()]);
    sign_registry(&e, &s, &[("w1", &other), ("w2", &other)]);
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
    assert!(stderr.contains("not signed by a registered Worker"), "{stderr}");
    assert!(stderr.contains("0 proved-standalone Verdicts (0 from Workers, 0 Result_Cache hits)"), "{stderr}");
    assert!(!proofs(&s).contains("\"prover\""));
    fs::remove_dir_all(&s.dir).unwrap();
}

/// A Coordinator that runs the real assignment logic but rewrites every
/// Result on its way to the Front_End: the solver output becomes `unsat`,
/// the tag stays. It also delivers each Result twice.
fn tampering_coordinator() -> u16 {
    /// The real environment, except that what it sends is tampered with.
    struct Tampering(OsEnv);
    impl EnvTrait for Tampering {
        fn now(&self) -> u64 {
            self.0.now()
        }
        fn poll(&mut self, wait: Duration) -> Option<NetEvent> {
            self.0.poll(wait)
        }
        fn send(&mut self, peer: Peer, msg: &Message) {
            match msg {
                Message::Result(signed) => {
                    let mut signed = signed.clone();
                    signed.fields.output = b"unsat\n".to_vec();
                    signed.fields.status = Some(0);
                    signed.fields.timed_out = false;
                    let forged = Message::Result(signed);
                    self.0.send(peer, &forged);
                    self.0.send(peer, &forged);
                }
                other => self.0.send(peer, other),
            }
        }
        fn close(&mut self, peer: Peer) {
            self.0.close(peer)
        }
        fn connect(&mut self, address: &str) -> std::io::Result<Peer> {
            self.0.connect(address)
        }
        fn solver_binary(&mut self, program: &str) -> std::io::Result<(PathBuf, dp_formats::hash::Hash)> {
            self.0.solver_binary(program)
        }
        fn start(&mut self, run: u64, exe: &Path, spec: &dp_shell::env::RunSpec) -> std::io::Result<()> {
            self.0.start(run, exe, spec)
        }
        fn kill(&mut self, run: u64) {
            self.0.kill(run)
        }
        fn finished(&mut self) -> std::io::Result<Vec<(u64, dp_shell::env::RunEnd)>> {
            self.0.finished()
        }
    }
    let mut env = OsEnv::new("dp-tampering-coordinator").unwrap();
    let port = env.listen("127.0.0.1:0").unwrap().port();
    let config = dp_core::coord::Config { silence_limit: 10_000_000, beat_period: 2_000_000, retries: 2, serial: 0 };
    let mut shell = Shell::new(Tampering(env), config, TraceWriter::off("coordinator"), None);
    thread::spawn(move || loop {
        shell.turn(Duration::from_millis(20)).unwrap();
    });
    port
}

#[test]
fn a_coordinator_that_forges_answers_proves_nothing() {
    let Some(e) = env() else { return };
    let s = setup(&e, "forging");
    sign_registry(&e, &s, &[("w1", &s.worker_keys[0]), ("w2", &s.worker_keys[1])]);
    let port = tampering_coordinator();
    let _workers = start_workers(&e, &s, port);
    thread::sleep(Duration::from_millis(300));
    let out = prove_with_seeds(&e, &s, port, 2);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    // Every forged Result fails its tag; even the true goals stay unproved,
    // and the false goal is not proved.
    assert!(stderr.contains("not signed by a registered Worker"), "{stderr}");
    assert!(stderr.contains("0 proved-standalone Verdicts (0 from Workers, 0 Result_Cache hits)"), "{stderr}");
    assert!(!proofs(&s).contains("\"prover\""));
    fs::remove_dir_all(&s.dir).unwrap();
}

#[test]
fn without_a_coordinator_the_session_proves_locally() {
    let Some(e) = env() else { return };
    let s = setup(&e, "absent");
    sign_registry(&e, &s, &[("w1", &s.worker_keys[0])]);
    let out = prove(&e, &s, free_port());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("no connection to the Coordinator at 127.0.0.1:"), "{stderr}");
    assert!(stderr.contains("2 proved-standalone Verdicts (0 from Workers, 0 Result_Cache hits)"), "{stderr}");
    fs::remove_dir_all(&s.dir).unwrap();
}

#[test]
fn a_registry_that_the_operator_did_not_sign_stops_the_session() {
    let Some(e) = env() else { return };
    let s = setup(&e, "registry");
    sign_registry(&e, &s, &[("w1", &s.worker_keys[0])]);
    // Another key signs a registry that names the same Workers.
    let other = setup(&e, "registry-other");
    sign_registry(&e, &other, &[("w1", &s.worker_keys[0])]);
    fs::copy(other.dir.join("registry"), s.dir.join("registry")).unwrap();
    let out = prove(&e, &s, free_port());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("not signed by the operator key"), "{stderr}");
    assert!(proofs(&s).is_empty());
    fs::remove_dir_all(&s.dir).unwrap();
    fs::remove_dir_all(&other.dir).unwrap();
}

/// R1.4: a calibrated Worker runs with the limit that its own Calibration
/// gives, and signs both; the Front_End recomputes the limit.
#[test]
fn calibrated_workers_run_with_scaled_limits() {
    use dp_formats::proofjson::{DpEntry, Place};
    let Some(e) = env() else { return };
    let s = setup(&e, "calibrated");
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
    let (_workers, offers) = start_workers_with(&e, &s, port, true);
    // Both provers were calibrated: the offer states a speed for each.
    for line in &offers {
        assert!(line.contains("alt-ergo@") && line.contains("z3@") && line.contains(" x"), "{line}");
    }
    let out = prove(&e, &s, port);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(all_from_workers(&stderr, 2), "{stderr}");
    assert!(!stderr.contains("discarded"), "{stderr}");
    // The replay fields of each proof carry the Worker's Calibration and a
    // local limit that is the reference limit scaled by it.
    let proofs = proofs(&s);
    let mut seen = 0;
    for line in proofs.lines().filter(|l| l.contains("\"dp\"")) {
        let member = line.split('"').nth(3).unwrap();
        let entry = DpEntry::from_member(member, 1 << 20).unwrap();
        for replay in &entry.replay {
            assert!(matches!(replay.place, Place::Worker(_)), "{replay:?}");
            let velocity = replay.velocity.unwrap();
            let expected = u128::from(replay.reference_micros) * u128::from(velocity) / 1_000_000;
            assert_eq!(u128::from(replay.local_micros.unwrap()), expected, "{replay:?}");
            seen += 1;
        }
    }
    assert_eq!(seen, 2, "{proofs}");
    fs::remove_dir_all(&s.dir).unwrap();
}

/// R2.2: a Portfolio of two provers races on every prover call, the first
/// pass included; the proof file records the prover that won, and replay
/// confirms it with that prover.
#[test]
fn a_portfolio_of_provers_races_and_its_winners_replay() {
    use dp_formats::proofjson::{DpEntry, Place};
    let Some(e) = env() else { return };
    let s = setup(&e, "portfolio");
    // A first pass exists in this project, and it goes to the Cluster.
    fs::write(
        s.dir.join("why3find.json"),
        r#"{ "fast": 0.5, "time": 1.0, "depth": 4, "packages": [], "provers": [ "alt-ergo", "z3" ], "tactics": [ "split_vc" ], "drivers": [], "warnoff": [] }"#,
    )
    .unwrap();
    fs::write(
        s.dir.join("dp.toml"),
        "[[solver]]\nname = \"ae\"\nprover = \"alt-ergo\"\n\n[[solver]]\nname = \"z3\"\nprover = \"z3\"\n\n\
         [portfolio]\nrace = [\"ae\", \"z3\"]\nfirst_pass = \"cluster\"\n",
    )
    .unwrap();
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
    let out = prove(&e, &s, port);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("Theory arith.Arith: ✘ (2/3)"), "{stderr}");
    // Both proofs came from Workers. A later call on a Goal whose race was
    // lost by every member is answered from the Result_Cache at once: the
    // same members would only lose it again.
    assert!(stderr.contains("2 proved-standalone Verdicts (2 from Workers, "), "{stderr}");
    assert!(!stderr.contains("discarded"), "{stderr}");
    // Every proof names a member of the Portfolio and the Worker that ran it.
    let proofs = proofs(&s);
    let mut winners = Vec::new();
    for line in proofs.lines().filter(|l| l.contains("\"dp\"")) {
        let entry = DpEntry::from_member(line.split('"').nth(3).unwrap(), 1 << 20).unwrap();
        for replay in entry.replay {
            assert!(replay.prover == "alt-ergo" || replay.prover == "z3", "{replay:?}");
            assert!(matches!(replay.place, Place::Worker(_)));
            winners.push(replay.prover);
        }
    }
    assert_eq!(winners.len(), 2, "{proofs}");
    // Replay reruns each recorded prover, whichever prover the legacy
    // members name.
    let replay = Command::new(e.bin.join("dp-replay"))
        .args(["--no-autodetect", "--no-autoload", "arith.mlw"])
        .current_dir(&s.dir)
        .env("WHY3CONFIG", &e.why3_conf)
        .env("DP_WHY3FIND", &e.why3find)
        .env("DP_LOCK", &e.lock)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&replay.stdout);
    assert_eq!(replay.status.code(), Some(0), "{report}");
    assert!(report.contains("2 confirmed, 0 not confirmed"), "{report}");
    // So does unmodified why3find: the legacy members name the winners.
    let home = PathBuf::from(std::env::var_os("DP_CREUSOT_HOME").unwrap());
    let legacy = Command::new(home.join("bin/why3find"))
        .args(["prove", "--no-autodetect", "--no-autoload", "-r", "arith.mlw"])
        .current_dir(&s.dir)
        .env("WHY3CONFIG", &e.why3_conf)
        .env("PATH", format!("{}:{}", home.join("bin").display(), std::env::var("PATH").unwrap()))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&legacy.stderr).into_owned() + &String::from_utf8_lossy(&legacy.stdout);
    assert!(text.contains("Theory arith.Arith: ✘ (2/3)"), "{text}");
    for (line, winner) in proofs.lines().filter(|l| l.contains("\"prover\"")).zip(&winners) {
        assert!(line.contains(&format!("\"{winner}\"")), "{line} should name {winner}");
    }
    // A second Session takes the winners' Results from the Result_Cache:
    // nothing runs on the Cluster.
    let out = prove(&e, &s, port);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("2 proved-standalone Verdicts (0 from Workers, "), "{stderr}");
    assert!(!stderr.contains(" 0 Result_Cache hits"), "{stderr}");
    fs::remove_dir_all(&s.dir).unwrap();
}

/// R1.7: the operator sees the Cluster, and a Worker that it revokes is
/// gone: it gets no Job, and no proof comes from it.
#[test]
fn the_operator_sees_the_cluster_and_revokes_a_worker() {
    use dp_formats::proofjson::{DpEntry, Place};
    let Some(e) = env() else { return };
    let s = setup(&e, "operator");
    sign_registry(&e, &s, &[("w1", &s.worker_keys[0]), ("w2", &s.worker_keys[1])]);
    let port = free_port();
    let address = format!("127.0.0.1:{port}");
    let coordinator = Command::new(e.bin.join("dp-coordinator"))
        .args(["--listen", &address, "--with-parent", "yes", "--operator-key", &s.operator_key])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _guard = Guard(vec![coordinator]);
    wait_for_port(port);
    let _workers = start_workers(&e, &s, port);
    let tool = |args: &[&str]| Command::new(e.bin.join("dp-coordinator")).args(args).output().unwrap();
    let status = || String::from_utf8_lossy(&tool(&["status", &address]).stdout).into_owned();
    let report = status();
    assert!(report.contains("0 queued Jobs, 2 Workers"), "{report}");
    assert!(report.contains("worker w1 serving idle") && report.contains("worker w2 serving idle"), "{report}");
    assert!(report.contains("alt-ergo@"), "{report}");
    let operator_key = s.dir.join("operator.key").display().to_string();
    // A command that another key signed is refused.
    let other_key = s.dir.join("w1.key").display().to_string();
    let refused = tool(&["operate", &other_key, &address, "1", "revoke", "w1"]);
    assert_eq!(refused.status.code(), Some(2), "{refused:?}");
    assert!(status().contains("2 Workers"));
    // The operator's is taken, once.
    let taken = tool(&["operate", &operator_key, &address, "1", "revoke", "w1"]);
    assert!(taken.status.success(), "{taken:?}");
    assert_eq!(tool(&["operate", &operator_key, &address, "1", "revoke", "w2"]).status.code(), Some(2));
    let report = status();
    assert!(report.contains("1 Workers") && !report.contains("worker w1"), "{report}");
    // A Session started now learns of the revocation and proves with the
    // other Worker.
    let out = prove(&e, &s, port);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("the operator revoked Worker \"w1\""), "{stderr}");
    assert!(all_from_workers(&stderr, 2), "{stderr}");
    for line in proofs(&s).lines().filter(|l| l.contains("\"dp\"")) {
        let entry = DpEntry::from_member(line.split('"').nth(3).unwrap(), 1 << 20).unwrap();
        for replay in entry.replay {
            assert_eq!(replay.place, Place::Worker("w2".into()));
        }
    }
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
    let out = Command::new(env!("CARGO_BIN_EXE_dp-frontend"))
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
