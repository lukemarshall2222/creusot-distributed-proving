// Feature: creusot-distributed-proving, Property 31: Offline replay confirms exactly what the evidence supports
// P-ID: P7.6.5, P7.6.6, P5.4.8

//! `dp-replay` on files that were changed after a Session wrote them, or
//! that no Session wrote: nothing that the evidence does not support is
//! confirmed.

use std::{fs, path::PathBuf};

use dp_formats::{cache::Entry, dispatch::DriverAnswer, proofjson::Place};
use dp_replay_tests::*;
use serde_json::Value;

#[test]
fn a_proof_copied_to_another_goal_is_not_confirmed() {
    let Some(e) = env() else { return };
    let dir = project("copied");
    prove(&e, &dir);
    // Wrong gets the whole entry of Easy: legacy members and label.
    edit(&dir, |file| {
        let goals = &mut file["proofs"]["Arith"];
        goals["Wrong"] = goals["Easy"].clone();
    });
    let (status, report) = replay(&e, &dir, &e.lock);
    assert_eq!(status, Some(1), "{report}");
    assert!(report.contains("not-confirmed arith.mlw Arith.Wrong []: the recorded Task_Hash"), "{report}");
    assert!(report.contains("confirmed arith.mlw Arith.Easy"), "{report}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_legacy_proof_without_a_proof_label_is_not_confirmed() {
    let Some(e) = env() else { return };
    let dir = project("unlabelled");
    prove(&e, &dir);
    // Easy keeps its legacy members and loses its label; Wrong gets legacy
    // members and keeps its non-proof label.
    edit(&dir, |file| {
        let goals = file["proofs"]["Arith"].as_object_mut().unwrap();
        let prover = goals["Easy"]["prover"].clone();
        goals["Easy"].as_object_mut().unwrap().remove("dp");
        let wrong = goals["Wrong"].as_object_mut().unwrap();
        wrong.insert("prover".into(), prover);
        wrong.insert("time".into(), 0.01.into());
    });
    let (status, report) = replay(&e, &dir, &e.lock);
    assert_eq!(status, Some(1), "{report}");
    assert!(report.contains("not-confirmed arith.mlw Arith.Easy []: the entry has no dp member"), "{report}");
    assert!(
        report.contains(
            "not-confirmed arith.mlw Arith.Wrong []: the entry is proved for a legacy reader, and its label is unknown"
        ),
        "{report}"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_damaged_label_is_not_confirmed() {
    let Some(e) = env() else { return };
    let dir = project("damaged");
    prove(&e, &dir);
    edit(&dir, |file| {
        let node = &mut file["proofs"]["Arith"]["Easy"];
        // not the canonical text of an entry
        let text = node["dp"].as_str().unwrap().to_string();
        node["dp"] = format!("{text} ").into();
    });
    let (status, report) = replay(&e, &dir, &e.lock);
    assert_eq!(status, Some(1), "{report}");
    assert!(report.contains("not-confirmed arith.mlw Arith.Easy []: not a dp member"), "{report}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_proof_file_of_another_lock_file_is_not_confirmed() {
    let Some(e) = env() else { return };
    let dir = project("lock");
    prove(&e, &dir);
    // The same pins in another file: its hash is not the recorded one.
    let other = format!("{}\n# another Lock_File\n", e.lock);
    let (status, report) = replay(&e, &dir, &other);
    assert_eq!(status, Some(1), "{report}");
    assert!(report.contains("lock-mismatch"), "{report}");
    assert!(report.contains("0 confirmed, 2 not confirmed"), "{report}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_recorded_binary_that_is_not_the_local_one_is_not_confirmed() {
    let Some(e) = env() else { return };
    let dir = project("binary");
    prove(&e, &dir);
    edit(&dir, |file| {
        let node = &mut file["proofs"]["Arith"]["Easy"];
        let mut entry = member(node);
        entry.replay[0].binary_hash.0[0] ^= 1;
        node["dp"] = entry.to_member().into();
    });
    let (status, report) = replay(&e, &dir, &e.lock);
    assert_eq!(status, Some(1), "{report}");
    assert!(
        report.contains("not-confirmed arith.mlw Arith.Easy []: the recorded solver binary is not the local pinned one"),
        "{report}"
    );
    fs::remove_dir_all(dir).unwrap();
}

/// The Result_Cache is not authenticated: a local entry is believed. A
/// forged entry gives a Session a proof label for a false Goal; replay does
/// not use the cache, runs the solver, and does not confirm it.
#[test]
fn a_proof_from_a_forged_cache_entry_is_not_confirmed() {
    let Some(e) = env() else { return };
    let dir = project("forged-cache");
    prove(&e, &dir);
    // Every stored answer becomes Valid.
    for file in fs::read_dir(dir.join(".dp-cache")).unwrap().flatten() {
        let mut entry = Entry::decode(&fs::read(file.path()).unwrap(), 1 << 24).unwrap();
        entry.answer = DriverAnswer::Valid;
        fs::write(file.path(), entry.encode()).unwrap();
    }
    prove(&e, &dir);
    // The Session took the forged entries: the false Goal has a proof label.
    let file: Value = serde_json::from_str(&fs::read_to_string(proof_path(&dir)).unwrap()).unwrap();
    let wrong = &file["proofs"]["Arith"]["Wrong"];
    assert_eq!(member(wrong).label, "proved-standalone", "{file}");
    assert_eq!(member(wrong).replay[0].place, Place::Local);
    // Replay does not.
    let (status, report) = replay(&e, &dir, &e.lock);
    assert_eq!(status, Some(1), "{report}");
    assert!(report.contains("not-confirmed arith.mlw Arith.Wrong []: the solver"), "{report}");
    assert!(report.contains("confirmed arith.mlw Arith.Easy"), "{report}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_file_that_unmodified_why3find_wrote_has_nothing_confirmed() {
    let Some(e) = env() else { return };
    let dir = project("legacy");
    prove(&e, &dir);
    // The legacy members alone, as unmodified why3find writes them.
    edit(&dir, |file| {
        file.as_object_mut().unwrap().remove("dp-lock");
        let goals = file["proofs"]["Arith"].as_object_mut().unwrap();
        goals["Easy"].as_object_mut().unwrap().remove("dp");
        goals.insert("Wrong".into(), Value::Null);
    });
    let (status, report) = replay(&e, &dir, &e.lock);
    assert_eq!(status, Some(1), "{report}");
    assert!(report.contains("not-confirmed arith.mlw Arith.Easy"), "{report}");
    assert!(report.contains("not-proved arith.mlw Arith.Wrong [] unresolved"), "{report}");
    fs::remove_dir_all(dir).unwrap();
}

// P-ID: P7.6.4, P5.4.3
#[test]
fn a_changed_or_missing_certificate_is_not_confirmed() {
    let Some(e) = env() else { return };
    let Some((dir, lock)) = certified_session(&e, "tampered-certificate") else { return };
    // The entries of the store; its index by Task_Hash is not one.
    let entries: Vec<PathBuf> = fs::read_dir(dir.join(".dp-certs"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("proof").is_file())
        .collect();
    assert!(!entries.is_empty());
    // A proof that is not the stored one: found by its hash.
    let saved: Vec<Vec<u8>> = entries.iter().map(|d| fs::read(d.join("proof")).unwrap()).collect();
    for dir in &entries {
        fs::write(dir.join("proof"), b"(step @p1 :rule refl :args (x))\n").unwrap();
    }
    let (status, report) = replay(&e, &dir, &lock);
    assert_eq!(status, Some(1), "{report}");
    assert!(report.contains("the certificate entry is damaged"), "{report}");
    // The certificate of one Goal under the reference of another: every
    // file is as stored, and the checker does not accept it for that Goal.
    for (dir, proof) in entries.iter().zip(&saved) {
        fs::write(dir.join("proof"), proof).unwrap();
    }
    assert_eq!(replay(&e, &dir, &lock).0, Some(0));
    edit(&dir, |file| {
        let goals = file["proofs"]["Arith"].as_object_mut().unwrap();
        let easy = goals["Easy"].clone();
        goals.insert("Wrong".into(), easy);
    });
    let (status, report) = replay(&e, &dir, &lock);
    assert_eq!(status, Some(1), "{report}");
    assert!(
        report.contains("not-confirmed arith.mlw Arith.Wrong []: the certificate is not for the Goal as printed here"),
        "{report}"
    );
    // Without the store nothing certified is confirmed.
    fs::remove_dir_all(dir.join(".dp-certs")).unwrap();
    let (status, report) = replay(&e, &dir, &lock);
    assert_eq!(status, Some(1), "{report}");
    assert!(!report.contains("\nconfirmed "), "{report}");
    fs::remove_dir_all(dir).unwrap();
}
