// Feature: creusot-distributed-proving, Property 9: A certified Verdict has a complete entry in the Certificate_Store
// P-ID: P5.4.3, P5.4.4, P5.4.6

//! Sessions with certification whose Certificate_Store fails (R5.4): a
//! certificate that cannot be stored certifies nothing, and what a writer
//! that was stopped left behind is not taken for an entry.
//!
//! Needs the development shell and `target/debug/dp-frontend`.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use dp_formats::proofjson::DpEntry;

const ARITH: &str = r#"module Arith
  use int.Int

  goal Easy: forall x y: int. x + y = y + x

  goal Wrong: forall x: int. x * x > x
end
"#;

fn tools() -> Option<(PathBuf, PathBuf, String)> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let frontend = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/dp-frontend");
    match (var("DP_WHY3FIND"), var("DP_LOCK"), var("DP_ETHOS"), var("DP_CPC_SIGNATURE")) {
        (Some(patched), Some(lock), Some(_), Some(_)) if frontend.exists() => {
            Some((frontend, patched, fs::read_to_string(lock).unwrap()))
        }
        _ => {
            assert!(std::env::var_os("DP_REQUIRE_TOOLS").is_none(), "development shell or dp-frontend missing");
            eprintln!("development shell or dp-frontend missing: test skipped");
            None
        }
    }
}

/// A project in which every reported proof needs a certificate.
fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dp-store-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("arith.mlw"), ARITH).unwrap();
    fs::write(
        dir.join("why3find.json"),
        r#"{ "fast": 0.2, "time": 1.0, "depth": 4, "packages": [], "provers": [ "cvc5" ], "tactics": [ "split_vc" ], "drivers": [], "warnoff": [] }"#,
    )
    .unwrap();
    fs::write(dir.join("dp.toml"), "[certification]\nenabled = true\ncertify_all = true\n").unwrap();
    dir
}

fn session(frontend: &Path, patched: &Path, lock: &str, dir: &Path) -> String {
    let lock_path = dir.join("lock.toml");
    fs::write(&lock_path, lock).unwrap();
    let out = Command::new(frontend)
        .args(["prove", "-f", "arith.mlw"])
        .current_dir(dir)
        .env("DP_WHY3FIND", patched)
        .env("DP_LOCK", &lock_path)
        .output()
        .unwrap();
    fs::remove_file(lock_path).unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn easy(dir: &Path) -> DpEntry {
    let text = fs::read_to_string(dir.join("why3find.proofs/arith.json")).unwrap();
    let file: serde_json::Value = serde_json::from_str(&text).unwrap();
    DpEntry::from_member(file["proofs"]["Arith"]["Easy"]["dp"].as_str().unwrap(), 1 << 20).unwrap()
}

#[test]
fn a_certificate_that_cannot_be_stored_certifies_nothing() {
    let Some((frontend, patched, lock)) = tools() else { return };
    let dir = project("unwritable");
    // The store's place is taken by a file: nothing can be stored there.
    fs::write(dir.join(".dp-certs"), b"not a directory").unwrap();
    let stderr = session(&frontend, &patched, &lock, &dir);
    assert!(stderr.contains("0 certified Verdicts"), "{stderr}");
    assert!(stderr.contains("the certificate could not be stored"), "{stderr}");
    let entry = easy(&dir);
    // The checker accepted, but without a stored entry the Verdict cannot
    // be replayed: checked-incomplete, and no certificate is named.
    assert_eq!(entry.label, "checked-incomplete");
    assert!(entry.certificate.is_none());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn what_an_interrupted_write_left_is_not_an_entry() {
    let Some((frontend, patched, lock)) = tools() else { return };
    let dir = project("interrupted");
    let stderr = session(&frontend, &patched, &lock, &dir);
    assert!(stderr.contains("1 certified Verdicts"), "{stderr}");
    let content = easy(&dir).certificate.expect("a certified entry names its certificate");
    let entry = dir.join(".dp-certs").join(content.to_hex());
    let honest = dp_cert::Certificate::load(&dir.join(".dp-certs"), content).unwrap();
    // The entry as a writer that was stopped before the rename would have
    // left it: a temporary directory, and no entry.
    let tmp = dir.join(".dp-certs").join(format!(".{}.1.tmp", content.to_hex()));
    fs::rename(&entry, &tmp).unwrap();
    fs::remove_file(tmp.join("manifest")).unwrap();
    let _ = fs::remove_dir_all(dir.join(".dp-cache"));
    let stderr = session(&frontend, &patched, &lock, &dir);
    // The certificate is produced and stored again; the leftover is not used.
    assert!(stderr.contains("1 certified Verdicts; certificates checked: 1 produced here"), "{stderr}");
    assert_eq!(easy(&dir).certificate, Some(content));
    assert_eq!(dp_cert::Certificate::load(&dir.join(".dp-certs"), content).unwrap(), honest);
    // An entry with a file missing is no entry either.
    fs::remove_file(entry.join("proof")).unwrap();
    let _ = fs::remove_dir_all(dir.join(".dp-cache"));
    let stderr = session(&frontend, &patched, &lock, &dir);
    assert!(stderr.contains("certificates checked: 1 produced here"), "{stderr}");
    fs::remove_dir_all(dir).unwrap();
}
