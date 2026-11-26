//! Cluster options that are not valid stop the Session before it starts (R1.1 criterion 6).

use super::*;

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
