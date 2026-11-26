// Feature: creusot-distributed-proving, Property 30: proof.json and the Creusot_CLI report labels faithfully
// P-ID: P31.2 (the backend option of Creusot_CLI runs the Front_End and returns its exit status)

//! `cargo creusot --backend dp-frontend` on a small Creusot crate: the
//! whole path from Rust source to labelled proof files (tasks 10.1, 10.2).
//!
//! Needs the development shell (patched Creusot on PATH, `DP_WHY3FIND`,
//! `DP_LOCK`) and `target/debug/dp-frontend` (`cargo build --workspace`).
//! Without them the tests pass with a notice. The fixture crate is built
//! in `target/cli-fixture`, so that `creusot-std` is compiled once.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use dp_formats::proofjson::DpEntry;

/// The manifest of a fixture crate, but for its name. Cargo takes a crate
/// of the same name for built already when the target directory holds one,
/// and Creusot then has nothing to prove, so every project has its own.
const MANIFEST: &str = r#"[package]
version = "0.0.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
creusot-std = "0.15.0"

[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(creusot)'] }
"#;

const CONFIG: &str = r#"{ "fast": 0.2, "time": 1.0, "depth": 4, "packages": [ "creusot" ], "provers": [ "alt-ergo", "z3" ], "tactics": [ "compute_specified", "split_vc" ], "drivers": [], "warnoff": [ "unused_variable", "axiom_abstract" ] }"#;

/// A function whose contract holds.
const ADD: &str = r#"use creusot_std::prelude::*;

#[requires(a@ + b@ <= u32::MAX@)]
#[ensures(result@ == a@ + b@)]
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}
"#;

/// A function whose postcondition is false for every argument.
const WRONG: &str = r#"
#[ensures(result@ > a@)]
pub fn wrong(a: u32) -> u32 {
    a
}
"#;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn frontend() -> Option<PathBuf> {
    let frontend = repo().join("target/debug/dp-frontend");
    let shell = ["DP_WHY3FIND", "DP_LOCK", "DP_CREUSOT_HOME"].iter().all(|v| std::env::var_os(v).is_some());
    if shell && frontend.exists() {
        return Some(frontend);
    }
    // In CI a skipped test would be a check that checked nothing.
    assert!(std::env::var_os("DP_REQUIRE_TOOLS").is_none(), "development shell or target/debug/dp-frontend missing");
    eprintln!("development shell or target/debug/dp-frontend missing: cli_labels skipped");
    None
}

fn crate_name(name: &str) -> String {
    format!("dp_cli_{}_{name}", std::process::id())
}

fn project(name: &str, source: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dp-cli-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("Cargo.toml"), MANIFEST.replacen("[package]\n", &format!("[package]\nname = \"{}\"\n", crate_name(name)), 1)).unwrap();
    fs::write(dir.join("why3find.json"), CONFIG).unwrap();
    fs::write(dir.join("src/lib.rs"), source).unwrap();
    dir
}

/// `cargo creusot` in `dir`, through `backend` when one is given.
fn creusot(dir: &Path, backend: Option<&Path>) -> Output {
    let mut command = Command::new("cargo");
    command.arg("creusot");
    if let Some(backend) = backend {
        command.arg("--backend").arg(backend);
    }
    command.current_dir(dir).env("CARGO_TARGET_DIR", repo().join("target/cli-fixture")).output().unwrap()
}

/// The label of the one Goal of a function's proof file, and whether the
/// legacy part of the entry names a prover.
fn entry(dir: &Path, name: &str, function: &str) -> (String, bool) {
    let path = dir.join(format!("why3find.proofs/verif.{}_rlib.{function}.json", crate_name(name)));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let file: serde_json::Value = serde_json::from_str(&text).unwrap();
    let goal = &file["proofs"]["Coma"][format!("vc_{function}")];
    let dp = DpEntry::from_member(goal["dp"].as_str().unwrap_or_else(|| panic!("no dp member: {text}")), 1 << 20).unwrap();
    assert!(file["dp-lock"].as_str().is_some_and(|l| l.starts_with("sha256:")), "{text}");
    (dp.label, goal.get("prover").is_some())
}

#[test]
fn the_backend_option_labels_every_goal_and_returns_the_exit_status() {
    let Some(frontend) = frontend() else { return };
    let dir = project("mixed", &format!("{ADD}{WRONG}"));
    let out = creusot(&dir, Some(&frontend));
    let stderr = String::from_utf8_lossy(&out.stderr);
    // The Front_End's own status, not a status that Creusot_CLI chose.
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    // why3find's report and the Front_End's summary both reach the user.
    assert!(stderr.contains("Goal Coma.vc_wrong: ✘"), "{stderr}");
    assert!(stderr.contains("dp-frontend: 1 proved-standalone Verdicts"), "{stderr}");
    assert!(stderr.contains("dp-frontend: Goals by label: 1 proved-standalone, 1 "), "{stderr}");
    assert_eq!(entry(&dir, "mixed", "add"), ("proved-standalone".to_string(), true));
    let (label, legacy_proof) = entry(&dir, "mixed", "wrong");
    assert!(label != "proved-standalone" && label != "certified", "{label}");
    assert!(!legacy_proof);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_proved_crate_succeeds_and_unmodified_why3find_accepts_its_proof_files() {
    let Some(frontend) = frontend() else { return };
    let dir = project("proved", ADD);
    let out = creusot(&dir, Some(&frontend));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert!(stderr.contains("dp-frontend: Goals by label: 1 proved-standalone"), "{stderr}");
    assert_eq!(entry(&dir, "proved", "add"), ("proved-standalone".to_string(), true));
    // Without the option Creusot_CLI runs unmodified why3find, which reads
    // the proof file that the Front_End wrote (R7.12): nothing is left to
    // prove, and no prover result of its own cache is involved.
    let _ = fs::remove_dir_all(dir.join(".why3find"));
    fs::write(dir.join("src/lib.rs"), format!("{ADD}\n")).unwrap();
    let out = creusot(&dir, None);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    // The proof file still records the proof, for the legacy reader.
    let path = dir.join(format!("why3find.proofs/verif.{}_rlib.add.json", crate_name("proved")));
    let file: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert!(file["proofs"]["Coma"]["vc_add"].get("prover").is_some(), "{file}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_pin_mismatch_stops_creusot_cli_before_any_proof_is_written() {
    let Some(frontend) = frontend() else { return };
    let dir = project("pin", ADD);
    // A Lock_File in which every solver hash differs from the binaries.
    let lock = fs::read_to_string(std::env::var_os("DP_LOCK").unwrap()).unwrap();
    let tampered: String = lock
        .lines()
        .map(|l| if l.starts_with("hash = \"sha256:") { format!("hash = \"sha256:{}\"", "0".repeat(64)) } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let lock_path = dir.join("lock.toml");
    fs::write(&lock_path, tampered).unwrap();
    let out = Command::new("cargo")
        .arg("creusot")
        .arg("--backend")
        .arg(&frontend)
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", repo().join("target/cli-fixture"))
        .env("DP_LOCK", &lock_path)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("pinned dependency check failed"), "{stderr}");
    assert!(!dir.join("why3find.proofs").exists(), "a proof file was written");
    fs::remove_dir_all(dir).unwrap();
}
