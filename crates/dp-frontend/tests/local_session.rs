//! End-to-end tests of `dp-frontend prove` in local mode, with real provers.
//!
//! They need the development shell: `DP_WHY3FIND` (patched why3find),
//! `DP_LOCK` (the Lock_File) and the pinned provers on PATH. Without
//! `DP_WHY3FIND` and `DP_LOCK` the tests pass with a notice.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const ARITH: &str = r#"module Arith
  use int.Int

  goal Easy: forall x y: int. x + y = y + x

  goal Split: (forall x: int. x * 0 = 0) /\ (forall a b: int. a < b -> a + 1 <= b)

  goal Wrong: forall x: int. x * x > x
end
"#;

fn env() -> Option<(PathBuf, String)> {
    match (std::env::var_os("DP_WHY3FIND"), std::env::var_os("DP_LOCK")) {
        (Some(w), Some(l)) => Some((w.into(), fs::read_to_string(l).unwrap())),
        _ => {
            eprintln!("DP_WHY3FIND or DP_LOCK is not set: local session test skipped");
            None
        }
    }
}

fn project(name: &str, provers: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dp-session-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("arith.mlw"), ARITH).unwrap();
    fs::write(
        dir.join("why3find.json"),
        format!(
            r#"{{ "fast": 0.2, "time": 1.0, "depth": 4, "packages": [], "provers": [ {provers} ], "tactics": [ "split_vc" ], "drivers": [], "warnoff": [] }}"#
        ),
    )
    .unwrap();
    dir
}

fn prove(dir: &Path, why3find: &Path, lock: &str) -> Output {
    let lock_path = dir.join("lock.toml");
    fs::write(&lock_path, lock).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_dp-frontend"))
        .args(["prove", "-f", "arith.mlw"])
        .current_dir(dir)
        .env("DP_WHY3FIND", why3find)
        .env("DP_LOCK", &lock_path)
        .output()
        .unwrap();
    fs::remove_file(lock_path).unwrap();
    out
}

fn proofs(dir: &Path) -> Option<String> {
    fs::read_to_string(dir.join("why3find.proofs/arith.json")).ok()
}

/// Files of why3find's legacy cache that hold prover results.
fn legacy_cache(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(provers) = fs::read_dir(dir.join(".why3find")) else { return out };
    for prover in provers.flatten() {
        let name = prover.file_name().to_string_lossy().into_owned();
        if name.contains('@') {
            out.push(name);
        }
    }
    out.sort();
    out
}

#[test]
fn proves_what_the_provers_prove_and_nothing_else() {
    let Some((why3find, lock)) = env() else { return };
    let dir = project("ok", r#""alt-ergo", "z3""#);
    let out = prove(&dir, &why3find, &lock);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("Goal Arith.Wrong"), "{stderr}");
    assert!(stderr.contains("Theory arith.Arith: ✘ (2/3)"), "{stderr}");
    assert!(stderr.contains("2 proved-standalone Verdicts"), "{stderr}");
    let proofs = proofs(&dir).unwrap();
    assert!(proofs.contains(r#""Wrong": null"#), "{proofs}");
    assert!(proofs.contains(r#""Easy": { "prover":"#), "{proofs}");
    // nothing but messages on the protocol channel
    assert!(out.stdout.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_solver_that_differs_from_the_lock_file_stops_the_session() {
    let Some((why3find, lock)) = env() else { return };
    // Change the pinned hash of z3: its binary no longer matches.
    let start = lock.find("name = \"z3\"").unwrap();
    let at = start + lock[start..].find("sha256:").unwrap() + 7;
    let flipped = if &lock[at..at + 1] == "0" { "1" } else { "0" };
    let tampered = format!("{}{}{}", &lock[..at], flipped, &lock[at + 1..]);
    let dir = project("pin", r#""alt-ergo", "z3""#);
    let out = prove(&dir, &why3find, &tampered);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("pinned dependency check failed: z3@"), "{stderr}");
    // no solver ran, no proof file, no cached result (R7.7 criterion 3)
    assert_eq!(proofs(&dir), None);
    assert!(legacy_cache(&dir).is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_solver_that_is_not_an_unpatched_solver_proves_nothing() {
    let Some((why3find, lock)) = env() else { return };
    // Alt-Ergo's entry becomes a Sharing_Solver: its binary is pinned, but its
    // proofs are Lemma_Dependent_Results, which this build labels unchecked.
    let start = lock.find("name = \"alt-ergo\"").unwrap();
    let kind = start + lock[start..].find("unpatched-solver").unwrap();
    let demoted =
        format!("{}sharing-solver{}", &lock[..kind], &lock[kind + "unpatched-solver".len()..]);
    let dir = project("demoted", r#""alt-ergo""#);
    let out = prove(&dir, &why3find, &demoted);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("labelled unchecked"), "{stderr}");
    assert!(stderr.contains("0 proved-standalone Verdicts"), "{stderr}");
    let proofs = proofs(&dir).unwrap();
    assert!(!proofs.contains("\"prover\""), "{proofs}");
    // why3find's legacy cache may hold the results of its own Calibration
    // runs here, which are not runs on Goals; no Goal result was stored, or
    // the proof file above would name a prover on replay.
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_why3find_that_differs_from_the_lock_file_is_not_started() {
    let Some((_, lock)) = env() else { return };
    let dir = project("why3find", r#""alt-ergo""#);
    // Any other executable stands in for a substituted why3find.
    let out = prove(&dir, Path::new("/bin/sh"), &lock);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("pinned dependency check failed: why3find"), "{stderr}");
    assert_eq!(proofs(&dir), None);
    fs::remove_dir_all(dir).unwrap();
}
