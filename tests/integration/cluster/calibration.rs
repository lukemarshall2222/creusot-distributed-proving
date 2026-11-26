//! Workers that calibrate their provers run with scaled time limits (R1.4).

use super::*;

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
