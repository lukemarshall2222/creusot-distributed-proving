//! What the replay tests share: a project, a Session on it, a replay of
//! it, and edits of its proof file. The tests are the `[[test]]` targets
//! of this package: `main.rs` replays what honest Sessions wrote, and
//! `tamper/main.rs` what was changed afterwards.
//!
//! They need the development shell and the workspace executables
//! (`cargo build --workspace`).

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use dp_formats::proofjson::DpEntry;
use serde_json::Value;

pub const ARITH: &str = r#"module Arith
  use int.Int

  goal Easy: forall x y: int. x + y = y + x

  goal Mixed: (forall x: int. x + 0 = x) /\ (forall x: int. x * x > x)

  goal Wrong: forall x: int. x * x > x
end
"#;

pub struct Env {
    pub why3find: PathBuf,
    pub lock: String,
    pub frontend: PathBuf,
    pub replay: PathBuf,
}

pub fn env() -> Option<Env> {
    let bin = std::env::current_exe().ok()?.ancestors().find(|p| p.ends_with("debug"))?.to_path_buf();
    let frontend = bin.join("dp-frontend");
    match (std::env::var_os("DP_WHY3FIND"), std::env::var_os("DP_LOCK")) {
        (Some(w), Some(l)) if frontend.exists() && bin.join("dp-replay").exists() => {
            Some(Env { why3find: w.into(), lock: fs::read_to_string(l).unwrap(), replay: bin.join("dp-replay"), frontend })
        }
        _ => {
            assert!(std::env::var_os("DP_REQUIRE_TOOLS").is_none(), "development shell or dp-frontend missing");
            eprintln!("development shell or dp-frontend missing: replay test skipped");
            None
        }
    }
}

pub fn project(name: &str) -> PathBuf {
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

pub fn tool(e: &Env, program: &Path, dir: &Path, lock: &str, args: &[&str]) -> Output {
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
pub fn prove(e: &Env, dir: &Path) {
    let out = tool(e, &e.frontend, dir, &e.lock, &["prove", "-f", "arith.mlw"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("proved-standalone Verdicts"), "{out:?}");
}

/// Replay: the exit status and the report.
pub fn replay(e: &Env, dir: &Path, lock: &str) -> (Option<i32>, String) {
    let out = tool(e, &e.replay, dir, lock, &["arith.mlw"]);
    (out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn proof_path(dir: &Path) -> PathBuf {
    dir.join("why3find.proofs/arith.json")
}

pub fn edit(dir: &Path, change: impl FnOnce(&mut Value)) {
    let mut file: Value = serde_json::from_str(&fs::read_to_string(proof_path(dir)).unwrap()).unwrap();
    change(&mut file);
    fs::write(proof_path(dir), serde_json::to_string_pretty(&file).unwrap()).unwrap();
}

pub fn member(node: &Value) -> DpEntry {
    DpEntry::from_member(node["dp"].as_str().unwrap(), 1 << 20).unwrap()
}


pub fn certified_session(e: &Env, name: &str) -> Option<(PathBuf, String)> {
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
