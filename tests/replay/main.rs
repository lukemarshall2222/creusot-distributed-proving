// Feature: creusot-distributed-proving, Property 31: Offline replay confirms exactly what the evidence supports
// P-ID: P7.6.1, P7.6.2, P7.6.8

//! `dp-replay` on the files of honest Sessions: every proof-labelled
//! Verdict is confirmed, and nothing is changed.

use std::fs;

use dp_replay_tests::*;

#[test]
fn replay_confirms_what_a_session_proved_and_changes_nothing() {
    let Some(e) = env() else { return };
    let dir = project("honest");
    prove(&e, &dir);
    let before = fs::read(proof_path(&dir)).unwrap();
    let (status, report) = replay(&e, &dir, &e.lock);
    assert_eq!(status, Some(0), "{report}");
    assert!(report.contains("confirmed arith.mlw Arith.Easy [] alt-ergo"), "{report}");
    assert!(report.contains("confirmed arith.mlw Arith.Mixed [split_vc.0] alt-ergo"), "{report}");
    assert!(report.contains("not-proved arith.mlw Arith.Mixed [split_vc.1] unknown"), "{report}");
    assert!(report.contains("not-proved arith.mlw Arith.Wrong [] unknown"), "{report}");
    // one entry per Goal entry, and the local Calibration is in the report
    assert!(report.contains("2 confirmed, 0 not confirmed, 0 contradicted, 2 without a proof"), "{report}");
    assert!(report.contains("calibration alt-ergo@"), "{report}");
    assert_eq!(fs::read(proof_path(&dir)).unwrap(), before);
    // Replay uses neither the Result_Cache nor why3find's cache.
    fs::remove_dir_all(dir.join(".dp-cache")).unwrap();
    let _ = fs::remove_dir_all(dir.join(".why3find"));
    assert_eq!(replay(&e, &dir, &e.lock).0, Some(0));
    fs::remove_dir_all(dir).unwrap();
}


// P-ID: P7.6.4 (certified part)
#[test]
fn replay_confirms_certified_verdicts_from_the_store_alone() {
    let Some(e) = env() else { return };
    let Some((dir, lock)) = certified_session(&e, "certified") else { return };
    // Neither cache has a say.
    fs::remove_dir_all(dir.join(".dp-cache")).unwrap();
    let _ = fs::remove_dir_all(dir.join(".why3find"));
    let (status, report) = replay(&e, &dir, &lock);
    assert_eq!(status, Some(0), "{report}");
    assert!(report.contains("confirmed arith.mlw Arith.Easy [] certificate"), "{report}");
    assert!(report.contains("0 not confirmed"), "{report}");
    fs::remove_dir_all(dir).unwrap();
}
