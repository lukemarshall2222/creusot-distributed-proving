//! The operator's view of the Cluster and its commands (R1.7).

use super::*;

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
