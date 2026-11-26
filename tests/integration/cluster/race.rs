//! Races on the Cluster: several seeds of one prover, and a Portfolio of provers (R2.2).

use super::*;

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
    // No Result is discarded for what it says. One that was on its way
    // when its race was decided arrives for a Job that is no longer open,
    // which a busy machine makes likely (seen 2026-11-26 at a load of 12);
    // that is the only discard that an honest Cluster gives.
    for line in stderr.lines().filter(|l| l.contains("discarded:")) {
        assert!(line.ends_with("discarded: no such open Job"), "{stderr}");
    }
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
