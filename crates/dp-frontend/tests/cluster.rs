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
    sync::mpsc,
    thread,
    time::Duration,
};

use dp_coordinator::machine::{Input, Peer, State};
use dp_formats::{cluster::Message, sexp};
use dp_shell::net::{Connection, Event};

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

fn start_workers(e: &Env, s: &Setup, port: u16) -> Guard {
    let workers = ["w1", "w2"]
        .iter()
        .map(|w| {
            Command::new(e.bin.join("dp-worker"))
                .args(["run", "--coordinator", &format!("127.0.0.1:{port}"), "--id", w])
                .arg("--key")
                .arg(s.dir.join(format!("{w}.key")))
                .arg("--why3-conf")
                .arg(&e.why3_conf)
                .arg("--lock")
                .arg(&e.lock)
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    Guard(workers)
}

fn prove(e: &Env, s: &Setup, port: u16) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dp-frontend"))
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
        .args(["--listen", &format!("127.0.0.1:{port}")])
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
    assert!(stderr.contains("2 proved-standalone Verdicts (2 from Workers)"), "{stderr}");
    assert!(!stderr.contains("discarded"), "{stderr}");
    let proofs = proofs(&s);
    assert!(proofs.contains(r#""Wrong": null"#) && proofs.contains(r#""Easy": { "prover":"#), "{proofs}");
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
        .args(["--listen", &format!("127.0.0.1:{port}")])
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
    assert!(stderr.contains("0 proved-standalone Verdicts (0 from Workers)"), "{stderr}");
    assert!(!proofs(&s).contains("\"prover\""));
    fs::remove_dir_all(&s.dir).unwrap();
}

/// A Coordinator that runs the real assignment logic but rewrites every
/// Result on its way to the Front_End: the solver output becomes `unsat`,
/// the tag stays. It also delivers each Result twice.
fn tampering_coordinator(listener: TcpListener) {
    let (tx, rx) = mpsc::channel::<(Input, Option<TcpStream>)>();
    thread::spawn(move || {
        let mut next: Peer = 0;
        for stream in listener.incoming().flatten() {
            next += 1;
            let peer = next;
            let tx = tx.clone();
            let writer = stream.try_clone().unwrap();
            let mut connection = Connection::new(stream).unwrap();
            let _ = tx.send((Input::Connected(peer), Some(writer)));
            thread::spawn(move || loop {
                match connection.poll(Duration::from_secs(60)) {
                    Event::Message(msg) => {
                        if tx.send((Input::Message(peer, msg), None)).is_err() {
                            return;
                        }
                    }
                    Event::Idle => {}
                    Event::Closed(_) => {
                        let _ = tx.send((Input::Disconnected(peer), None));
                        return;
                    }
                }
            });
        }
    });
    thread::spawn(move || {
        use std::io::Write;
        let mut state = State::default();
        let mut writers = std::collections::BTreeMap::new();
        for (input, writer) in rx {
            if let (Input::Connected(peer), Some(writer)) = (&input, writer) {
                writers.insert(*peer, writer);
            }
            let (next, effects) = state.step(input);
            state = next;
            for (peer, msg) in effects {
                let msg = match msg {
                    Message::Result(mut signed) => {
                        signed.fields.output = b"unsat\n".to_vec();
                        signed.fields.status = Some(0);
                        signed.fields.timed_out = false;
                        Message::Result(signed)
                    }
                    other => other,
                };
                let twice = matches!(msg, Message::Result(_));
                if let Some(w) = writers.get_mut(&peer) {
                    let bytes = sexp::print(&msg.encode());
                    let _ = w.write_all(&bytes);
                    if twice {
                        let _ = w.write_all(&bytes);
                    }
                }
            }
        }
    });
}

#[test]
fn a_coordinator_that_forges_answers_proves_nothing() {
    let Some(e) = env() else { return };
    let s = setup(&e, "forging");
    sign_registry(&e, &s, &[("w1", &s.worker_keys[0]), ("w2", &s.worker_keys[1])]);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    tampering_coordinator(listener);
    let _workers = start_workers(&e, &s, port);
    thread::sleep(Duration::from_millis(300));
    let out = prove(&e, &s, port);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    // Every forged Result fails its tag; even the true goals stay unproved,
    // and the false goal is not proved.
    assert!(stderr.contains("not signed by a registered Worker"), "{stderr}");
    assert!(stderr.contains("0 proved-standalone Verdicts (0 from Workers)"), "{stderr}");
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
    assert!(stderr.contains("2 proved-standalone Verdicts (0 from Workers)"), "{stderr}");
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
