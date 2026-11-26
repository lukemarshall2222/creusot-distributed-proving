//! A Cluster that is not there: the Session proves on this machine (R1.6 criteria 4 and 5).

use super::*;

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
