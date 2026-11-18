// Feature: creusot-distributed-proving, Property 31: Offline replay confirms exactly what the evidence supports
// P-ID: P7.6.1, P7.6.2, P7.6.5, P7.6.6, P7.6.8 (proved-standalone part)
//
// End-to-end tests of `dp-replay` on proof files that `dp-frontend` wrote,
// unchanged and tampered with. They need the development shell and the
// workspace executables, like the cluster tests.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use dp_formats::{
    cache::Entry,
    dispatch::DriverAnswer,
    proofjson::{DpEntry, Place},
};
use serde_json::Value;

const ARITH: &str = r#"module Arith
  use int.Int

  goal Easy: forall x y: int. x + y = y + x

  goal Mixed: (forall x: int. x + 0 = x) /\ (forall x: int. x * x > x)

  goal Wrong: forall x: int. x * x > x
end
"#;

struct Env {
    why3find: PathBuf,
    lock: String,
    frontend: PathBuf,
}

fn env() -> Option<Env> {
    let bin = std::env::current_exe().ok()?.ancestors().find(|p| p.ends_with("debug"))?.to_path_buf();
    let frontend = bin.join("dp-frontend");
    match (std::env::var_os("DP_WHY3FIND"), std::env::var_os("DP_LOCK")) {
        (Some(w), Some(l)) if frontend.exists() => {
            Some(Env { why3find: w.into(), lock: fs::read_to_string(l).unwrap(), frontend })
        }
        _ => {
            assert!(std::env::var_os("DP_REQUIRE_TOOLS").is_none(), "development shell or dp-frontend missing");
            eprintln!("development shell or dp-frontend missing: replay test skipped");
            None
        }
    }
}

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dp-replay-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("arith.mlw"), ARITH).unwrap();
    fs::write(
        dir.join("why3find.json"),
        r#"{ "fast": 0.2, "time": 1.0, "depth": 4, "packages": [], "provers": [ "alt-ergo" ], "tactics": [ "split_vc" ], "drivers": [], "warnoff": [] }"#,
    )
    .unwrap();
    dir
}

fn tool(e: &Env, program: &Path, dir: &Path, lock: &str, args: &[&str]) -> Output {
    let lock_path = dir.join("lock.toml");
    fs::write(&lock_path, lock).unwrap();
    let out = Command::new(program)
        .args(args)
        .current_dir(dir)
        .env("DP_WHY3FIND", &e.why3find)
        .env("DP_LOCK", &lock_path)
        .output()
        .unwrap();
    fs::remove_file(lock_path).unwrap();
    out
}

/// A Session: proves, and writes the proof file with its `dp` members.
fn prove(e: &Env, dir: &Path) {
    let out = tool(e, &e.frontend, dir, &e.lock, &["prove", "-f", "arith.mlw"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("proved-standalone Verdicts"), "{out:?}");
}

/// Replay: the exit status and the report.
fn replay(e: &Env, dir: &Path, lock: &str) -> (Option<i32>, String) {
    let out = tool(e, Path::new(env!("CARGO_BIN_EXE_dp-replay")), dir, lock, &["arith.mlw"]);
    (out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned())
}

fn proof_path(dir: &Path) -> PathBuf {
    dir.join("why3find.proofs/arith.json")
}

fn edit(dir: &Path, change: impl FnOnce(&mut Value)) {
    let mut file: Value = serde_json::from_str(&fs::read_to_string(proof_path(dir)).unwrap()).unwrap();
    change(&mut file);
    fs::write(proof_path(dir), serde_json::to_string_pretty(&file).unwrap()).unwrap();
}

fn member(node: &Value) -> DpEntry {
    DpEntry::from_member(node["dp"].as_str().unwrap(), 1 << 20).unwrap()
}

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

fn certified_session(e: &Env, name: &str) -> Option<(PathBuf, String)> {
    if std::env::var_os("DP_ETHOS").is_none() || std::env::var_os("DP_CPC_SIGNATURE").is_none() {
        assert!(std::env::var_os("DP_REQUIRE_TOOLS").is_none(), "DP_ETHOS or DP_CPC_SIGNATURE is not set");
        return None;
    }
    // Alt-Ergo is not an Unpatched_Solver here, so its proofs need
    // certificates; cvc5 is in the project as the proof producer.
    let start = e.lock.find("name = \"alt-ergo\"").unwrap();
    let kind = start + e.lock[start..].find("unpatched-solver").unwrap();
    let lock = format!("{}sharing-solver{}", &e.lock[..kind], &e.lock[kind + "unpatched-solver".len()..]);
    let dir = project(name);
    fs::write(
        dir.join("why3find.json"),
        r#"{ "fast": 0.2, "time": 1.0, "depth": 4, "packages": [], "provers": [ "alt-ergo", "cvc5" ], "tactics": [ "split_vc" ], "drivers": [], "warnoff": [] }"#,
    )
    .unwrap();
    fs::write(dir.join("dp.toml"), "[certification]\nenabled = true\n").unwrap();
    let out = tool(e, &e.frontend, &dir, &lock, &["prove", "-f", "arith.mlw"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("certified Verdicts"), "{out:?}");
    Some((dir, lock))
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

// P-ID: P7.6.4, P5.4.3
#[test]
fn a_changed_or_missing_certificate_is_not_confirmed() {
    let Some(e) = env() else { return };
    let Some((dir, lock)) = certified_session(&e, "tampered-certificate") else { return };
    let entries: Vec<PathBuf> = fs::read_dir(dir.join(".dp-certs")).unwrap().flatten().map(|e| e.path()).collect();
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
