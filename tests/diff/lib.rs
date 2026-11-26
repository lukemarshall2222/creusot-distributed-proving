//! Differential tests (design: Testing Strategy). The tests are the `[[test]]`
//! targets of this package; this library holds what they share.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

/// A small module: two Goals that provers prove, one that is false.
pub const ARITH: &str = r#"module Arith
  use int.Int

  goal Easy: forall x y: int. x + y = y + x

  goal Split: (forall x: int. x * 0 = 0) /\ (forall a b: int. a < b -> a + 1 <= b)

  goal Wrong: forall x: int. x * x > x
end
"#;

/// The tools of the development shell and the Front_End of this build.
pub struct Tools {
    /// why3find with the dispatch patch.
    pub patched: PathBuf,
    /// The text of the Lock_File.
    pub lock: String,
    /// Creusot's directory, with unmodified why3find in `bin`.
    pub home: PathBuf,
    pub frontend: PathBuf,
}

/// The tools, or `None` with a notice when the development shell or the
/// executables are missing; with `DP_REQUIRE_TOOLS` that is a failure.
pub fn tools() -> Option<Tools> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let frontend = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/dp-frontend");
    match (var("DP_WHY3FIND"), var("DP_LOCK"), var("DP_CREUSOT_HOME")) {
        (Some(patched), Some(lock), Some(home)) if frontend.exists() => {
            Some(Tools { patched, lock: fs::read_to_string(lock).unwrap(), home, frontend })
        }
        _ => {
            // In CI a skipped test would be a check that checked nothing.
            assert!(
                std::env::var_os("DP_REQUIRE_TOOLS").is_none(),
                "development shell or target/debug/dp-frontend missing"
            );
            eprintln!("development shell or target/debug/dp-frontend missing: test skipped");
            None
        }
    }
}

/// A project directory with `ARITH` as `arith.mlw` and the given provers.
pub fn project(name: &str, provers: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dp-diff-{}-{name}", std::process::id()));
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

/// A Session in `dir` with the Lock_File text `lock`.
pub fn session(t: &Tools, dir: &Path, lock: &str) -> Output {
    let lock_path = dir.join("lock.toml");
    fs::write(&lock_path, lock).unwrap();
    let out = Command::new(&t.frontend)
        .args(["prove", "-f", "arith.mlw"])
        .current_dir(dir)
        .env("DP_WHY3FIND", &t.patched)
        .env("DP_LOCK", &lock_path)
        .output()
        .unwrap();
    fs::remove_file(lock_path).unwrap();
    out
}

/// Unmodified pinned why3find in `dir`; returns what it printed.
pub fn unmodified(t: &Tools, dir: &Path, args: &[&str]) -> String {
    let path = format!("{}:{}", t.home.join("bin").display(), std::env::var("PATH").unwrap());
    let out = Command::new(t.home.join("bin/why3find")).args(args).current_dir(dir).env("PATH", path).output().unwrap();
    String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout)
}

pub fn proof_file(dir: &Path) -> String {
    fs::read_to_string(dir.join("why3find.proofs/arith.json")).unwrap()
}

/// The label of every leaf of the proof file, by its path: `Goal`, or
/// `Goal/0`, `Goal/1` for the children of a tactic. With the label, whether
/// the leaf has the legacy member `prover`.
pub fn leaves(proofs: &str) -> BTreeMap<String, (String, bool)> {
    fn walk(path: String, node: &serde_json::Value, out: &mut BTreeMap<String, (String, bool)>) {
        if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
            for (i, child) in children.iter().enumerate() {
                walk(format!("{path}/{i}"), child, out);
            }
            return;
        }
        let label = node
            .get("dp")
            .and_then(|d| d.as_str())
            .and_then(|d| dp_formats::proofjson::DpEntry::from_member(d, 1 << 20).ok())
            .map_or_else(|| "no label".to_string(), |e| e.label);
        out.insert(path, (label, node.get("prover").is_some()));
    }
    let file: serde_json::Value = serde_json::from_str(proofs).unwrap();
    let mut out = BTreeMap::new();
    for (goal, node) in file["proofs"]["Arith"].as_object().into_iter().flatten() {
        walk(goal.clone(), node, &mut out);
    }
    out
}

/// The Lock_File in which Alt-Ergo is pinned, but not as an Unpatched_Solver.
pub fn demoted_alt_ergo(lock: &str) -> String {
    let start = lock.find("name = \"alt-ergo\"").unwrap();
    let kind = start + lock[start..].find("unpatched-solver").unwrap();
    format!("{}sharing-solver{}", &lock[..kind], &lock[kind + "unpatched-solver".len()..])
}

/// The prover results of the legacy cache layout inside `.why3find`: the
/// names of the files under its directories that are named after a prover.
/// A name is the hash of the task that the prover ran on.
pub fn legacy_cache(dir: &Path) -> Vec<String> {
    fn files(dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            if entry.path().is_dir() {
                files(&entry.path(), out);
            } else {
                out.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    let mut out = Vec::new();
    for prover in fs::read_dir(dir.join(".why3find")).into_iter().flatten().flatten() {
        if prover.file_name().to_string_lossy().contains('@') {
            files(&prover.path(), &mut out);
        }
    }
    out.sort();
    out
}
