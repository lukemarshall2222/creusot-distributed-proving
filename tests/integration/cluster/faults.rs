//! Results that are not to be believed: an unregistered Worker, and a Coordinator that changes what it forwards (R1.3, R1.6).

use super::*;

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
