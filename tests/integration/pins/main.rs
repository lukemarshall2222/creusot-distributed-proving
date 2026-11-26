// Feature: creusot-distributed-proving, Property 5: Invalid configurations and pin mismatches fail closed
// P-ID: P7.7.3, P7.7.5, P7.7.7

//! Sessions whose tools are not those of the Lock_File (R7.7): no solver
//! starts, the tool and both hashes are named, the exit status is 2, and
//! no proof file and no cached result is written.
//!
//! Needs the development shell and `target/debug/dp-frontend`.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const ARITH: &str = "module Arith\n  use int.Int\n\n  goal Easy: forall x y: int. x + y = y + x\nend\n";

fn tools() -> Option<(PathBuf, PathBuf, String)> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let frontend = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/dp-frontend");
    match (var("DP_WHY3FIND"), var("DP_LOCK")) {
        (Some(patched), Some(lock)) if frontend.exists() => Some((frontend, patched, fs::read_to_string(lock).unwrap())),
        _ => {
            assert!(std::env::var_os("DP_REQUIRE_TOOLS").is_none(), "development shell or dp-frontend missing");
            eprintln!("development shell or dp-frontend missing: test skipped");
            None
        }
    }
}

fn project(name: &str, provers: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dp-pins-{}-{name}", std::process::id()));
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

fn session(frontend: &Path, patched: &Path, lock: &str, dir: &Path) -> Output {
    // Outside the project: the Session must leave the project as it was.
    let lock_path = dir.with_extension("lock.toml");
    fs::write(&lock_path, lock).unwrap();
    let out = Command::new(frontend)
        .args(["prove", "-f", "arith.mlw"])
        .current_dir(dir)
        .env("DP_WHY3FIND", patched)
        .env("DP_LOCK", &lock_path)
        .output()
        .unwrap();
    fs::remove_file(lock_path).unwrap();
    out
}

/// The Lock_File with the pinned hash of one tool changed in one digit.
fn with_other_hash(lock: &str, tool: &str) -> String {
    let start = lock.find(&format!("name = \"{tool}\"")).unwrap_or_else(|| panic!("no tool {tool}"));
    let at = start + lock[start..].find("sha256:").unwrap() + 7;
    let other = if &lock[at..at + 1] == "0" { "1" } else { "0" };
    format!("{}{other}{}", &lock[..at], &lock[at + 1..])
}

/// Every file of the project directory with its contents.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            if entry.path().is_dir() {
                walk(&entry.path(), out);
            } else {
                out.push((entry.path(), fs::read(entry.path()).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

/// Stopped with status 2 and the message, with nothing proved or stored.
fn stopped(out: &Output, dir: &Path, expected: &[&str]) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    for part in expected {
        assert!(stderr.contains(part), "no {part:?} in {stderr}");
    }
    assert!(!stderr.contains("proved-standalone Verdicts"), "{stderr}");
    assert!(!dir.join("why3find.proofs").exists(), "a proof file was written");
    assert!(!dir.join(".dp-cache").exists(), "a result was cached");
}

#[test]
fn a_solver_that_is_not_the_pinned_one_stops_the_session() {
    let Some((frontend, patched, lock)) = tools() else { return };
    for prover in ["alt-ergo", "z3"] {
        let dir = project(&format!("solver-{prover}"), &format!("\"{prover}\""));
        let out = session(&frontend, &patched, &with_other_hash(&lock, prover), &dir);
        // The tool, the hash of its binary and the hash that is pinned.
        stopped(&out, &dir, &["pinned dependency check failed", &format!("{prover}@"), "binary hash", "Lock_File pins"]);
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn a_bridge_that_is_not_the_pinned_one_is_not_started() {
    let Some((frontend, patched, lock)) = tools() else { return };
    let dir = project("bridge", "\"alt-ergo\"");
    let before = snapshot(&dir);
    let out = session(&frontend, &patched, &with_other_hash(&lock, "why3find-dispatch"), &dir);
    stopped(&out, &dir, &["pinned dependency check failed", "binary hash", "Lock_File pins"]);
    // It was never started: the project directory is byte for byte as it was.
    assert_eq!(snapshot(&dir), before);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_lock_file_that_cannot_be_read_stops_the_session() {
    let Some((frontend, patched, lock)) = tools() else { return };
    let dir = project("unreadable", "\"alt-ergo\"");
    let before = snapshot(&dir);
    // Not the format, a hash that is no hash, a patched build marked as an
    // Unpatched_Solver, and no entry for this platform.
    let malformed_hash = lock.replacen("sha256:", "sha256:xyz", 1);
    let patched_solver = lock.replacen("kind = \"unpatched-solver\"", "kind = \"unpatched-solver\"\npatches = [\"p.patch\"]", 1);
    let other_platform = lock.replace("platform = \"", "platform = \"other-");
    for (what, text) in [("not toml", "[[tool".to_string()), ("hash", malformed_hash), ("patched", patched_solver), ("platform", other_platform)] {
        let out = session(&frontend, &patched, &text, &dir);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{what}: {stderr}");
        assert_eq!(snapshot(&dir), before, "{what}: the project directory changed");
    }
    fs::remove_dir_all(dir).unwrap();
}
