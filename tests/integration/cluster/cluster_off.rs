//! Without the cluster option the Front_End connects to nothing (R1.1
//! criterion 6, R7.12).

use super::*;

#[test]
fn without_the_cluster_option_nothing_is_connected_to() {
    let Some(e) = env() else { return };
    let s = setup(&e, "off");
    // A listener where a Coordinator of the other tests would be: it must
    // see no connection, also with a registry and a key in the directory.
    sign_registry(&e, &s, &[("w1", &s.worker_keys[0])]);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let out = Command::new(frontend())
        .args(["prove", "--no-autodetect", "--no-autoload", "-f", "arith.mlw"])
        .current_dir(&s.dir)
        .env("WHY3CONFIG", &e.why3_conf)
        .env("DP_WHY3FIND", &e.why3find)
        .env("DP_LOCK", &e.lock)
        .env_remove("DP_CLUSTER")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("2 proved-standalone Verdicts (0 from Workers"), "{stderr}");
    assert!(listener.accept().is_err(), "something connected");
    fs::remove_dir_all(&s.dir).unwrap();
}
