//! End-to-end tests of the Result_Cache (R7.5), with real provers.
//!
//! They need the development shell, like `local_session.rs`.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use dp_formats::{cache::Entry, hash::Hash, proofjson::DpEntry};

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
            assert!(std::env::var_os("DP_REQUIRE_TOOLS").is_none(), "DP_WHY3FIND or DP_LOCK is not set");
            eprintln!("DP_WHY3FIND or DP_LOCK is not set: cache test skipped");
            None
        }
    }
}

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dp-cache-{}-{name}", std::process::id()));
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

/// The summary line: (proved-standalone Verdicts, Result_Cache hits).
fn summary(out: &Output) -> (u64, u64) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let line = stderr
        .lines()
        .find(|l| l.contains("proved-standalone Verdicts"))
        .unwrap_or_else(|| panic!("no summary line in {stderr}"));
    let number_before = |marker: &str| -> u64 {
        let end = line.find(marker).unwrap();
        line[..end].trim_end().rsplit(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap()
    };
    (number_before("proved-standalone Verdicts"), number_before("Result_Cache hits"))
}

fn proofs(dir: &Path) -> String {
    fs::read_to_string(dir.join("why3find.proofs/arith.json")).unwrap()
}

/// The proof file without the solver times, which differ between runs.
fn shape(proofs: &str) -> String {
    proofs.lines().filter(|l| !l.contains("\"time\"")).collect::<Vec<_>>().join("\n")
}

/// The label of a root Goal in the proof file.
fn label(proofs: &str, goal: &str) -> String {
    let at = proofs.find(&format!("\"{goal}\": {{")).unwrap_or_else(|| panic!("{goal} in {proofs}"));
    let rest = &proofs[at..];
    let start = rest.find("\"dp\": \"").unwrap() + 7;
    let member = &rest[start..start + rest[start..].find('"').unwrap()];
    DpEntry::from_member(member, 1 << 20).unwrap().label
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> =
        fs::read_dir(dir.join(".dp-cache")).unwrap().flatten().map(|e| e.path()).collect();
    files.sort();
    files
}

fn rewrite(dir: &Path, change: impl Fn(&mut Entry)) {
    for file in entries(dir) {
        let mut entry = Entry::decode(&fs::read(&file).unwrap(), 1 << 24).unwrap();
        change(&mut entry);
        fs::write(&file, entry.encode()).unwrap();
    }
}

/// A first Session fills the cache; returns the project with its results.
fn cold(name: &str, why3find: &Path, lock: &str) -> (PathBuf, String) {
    let dir = project(name);
    let out = prove(&dir, why3find, lock);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_eq!(summary(&out), (2, 0), "{stderr}");
    assert!(!entries(&dir).is_empty());
    // Entries are written whole: no temporary file is left behind.
    assert!(entries(&dir).iter().all(|f| f.file_name().unwrap().len() == 64));
    let proofs = proofs(&dir);
    (dir, proofs)
}

// P-ID: P7.5.1
#[test]
fn a_second_session_takes_its_results_from_the_cache() {
    let Some((why3find, lock)) = env() else { return };
    let (dir, first) = cold("warm", &why3find, &lock);
    let stored = entries(&dir).len();
    let out = prove(&dir, &why3find, &lock);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    let (proved, hits) = summary(&out);
    assert_eq!(proved, 2, "{stderr}");
    assert!(hits >= 2, "{stderr}");
    // Same Verdicts, and the false goal is still not proved.
    assert_eq!(shape(&proofs(&dir)), shape(&first));
    assert_eq!(label(&first, "Easy"), "proved-standalone");
    assert_eq!(label(&first, "Wrong"), "unknown");
    // The replay fields of a hit are those of the stored run.
    let member = |p: &str| p.lines().find(|l| l.contains("\"dp\"")).unwrap().to_string();
    assert_eq!(member(&proofs(&dir)), member(&first));
    // Nothing ran that was not already stored.
    assert_eq!(entries(&dir).len(), stored);
    fs::remove_dir_all(dir).unwrap();
}

// P-ID: P7.5.2
#[test]
fn an_entry_under_another_key_is_a_miss() {
    let Some((why3find, lock)) = env() else { return };
    let (dir, first) = cold("moved", &why3find, &lock);
    // Every file gets the entry of the next one: each entry is well formed
    // and valid, but its lookup fields are not those of its key. A proof of
    // one goal must not become a proof of another.
    let files = entries(&dir);
    assert!(files.len() >= 2);
    let contents: Vec<_> = files.iter().map(|f| fs::read(f).unwrap()).collect();
    for (i, file) in files.iter().enumerate() {
        fs::write(file, &contents[(i + 1) % files.len()]).unwrap();
    }
    let out = prove(&dir, &why3find, &lock);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_eq!(summary(&out), (2, 0), "{stderr}");
    assert!(stderr.contains("its lookup fields are not those of the lookup"), "{stderr}");
    assert_eq!(shape(&proofs(&dir)), shape(&first));
    fs::remove_dir_all(dir).unwrap();
}

// P-ID: P7.5.2
#[test]
fn bytes_that_are_not_an_entry_are_a_miss() {
    let Some((why3find, lock)) = env() else { return };
    let (dir, first) = cold("garbage", &why3find, &lock);
    for (i, file) in entries(&dir).iter().enumerate() {
        let bytes = fs::read(file).unwrap();
        // Truncated, or a trailing byte: neither is the canonical form.
        let bad = if i % 2 == 0 { bytes[..bytes.len() / 2].to_vec() } else { [bytes, vec![b' ']].concat() };
        fs::write(file, bad).unwrap();
    }
    let out = prove(&dir, &why3find, &lock);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_eq!(summary(&out), (2, 0), "{stderr}");
    assert_eq!(shape(&proofs(&dir)), shape(&first));
    // The misses were replaced by the results of this Session's runs.
    let out = prove(&dir, &why3find, &lock);
    assert!(summary(&out).1 >= 2);
    fs::remove_dir_all(dir).unwrap();
}

// P-ID: P7.5.3
#[test]
fn an_entry_of_another_binary_is_a_miss() {
    let Some((why3find, lock)) = env() else { return };
    let (dir, first) = cold("binary", &why3find, &lock);
    rewrite(&dir, |e| {
        let mut h = e.binary_hash.0;
        h[0] ^= 1;
        e.binary_hash = Hash(h);
    });
    let out = prove(&dir, &why3find, &lock);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_eq!(summary(&out), (2, 0), "{stderr}");
    assert!(stderr.contains("binary mismatch"), "{stderr}");
    assert_eq!(shape(&proofs(&dir)), shape(&first));
    fs::remove_dir_all(dir).unwrap();
}

// P-ID: P7.5.3
#[test]
fn an_entry_with_another_local_limit_is_a_miss() {
    let Some((why3find, lock)) = env() else { return };
    let (dir, first) = cold("limit", &why3find, &lock);
    rewrite(&dir, |e| e.local_micros += 1);
    let out = prove(&dir, &why3find, &lock);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_eq!(summary(&out), (2, 0), "{stderr}");
    assert!(stderr.contains("local time limit"), "{stderr}");
    assert_eq!(shape(&proofs(&dir)), shape(&first));
    fs::remove_dir_all(dir).unwrap();
}

// P-ID: P7.5.4
#[test]
fn a_hit_is_labelled_by_this_session_not_by_the_one_that_stored_it() {
    let Some((why3find, lock)) = env() else { return };
    let (dir, _) = cold("relabel", &why3find, &lock);
    // In the second Session Alt-Ergo is a Sharing_Solver: its binary is
    // still pinned, so the entries are valid, but its proofs are no longer
    // proved-standalone.
    let start = lock.find("name = \"alt-ergo\"").unwrap();
    let kind = start + lock[start..].find("unpatched-solver").unwrap();
    let demoted = format!("{}sharing-solver{}", &lock[..kind], &lock[kind + "unpatched-solver".len()..]);
    let out = prove(&dir, &why3find, &demoted);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    let (proved, hits) = summary(&out);
    assert_eq!(proved, 0, "{stderr}");
    assert!(hits >= 2, "{stderr}");
    assert!(stderr.contains("labelled unchecked"), "{stderr}");
    let proofs = proofs(&dir);
    assert!(!proofs.contains("\"prover\""));
    assert_eq!(label(&proofs, "Easy"), "unchecked");
    fs::remove_dir_all(dir).unwrap();
}
