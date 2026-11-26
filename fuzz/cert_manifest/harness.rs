//! The manifest of a Certificate_Store entry (R5.4): the store is not
//! trusted.
//!
//! The bytes are stored as the manifest of an entry, under their own
//! hash, next to a goal and a proof. Loading must not panic, and an entry
//! that loads is the one that its manifest describes: its content hash is
//! the name it was loaded by, and its files are those of the store.

use std::{fs, path::PathBuf};

use dp_cert::Certificate;
use dp_formats::hash::{self, Hash};

const GOAL: &[u8] = b"(assert false)\n";
const PROOF: &[u8] = b"(step @p1 :rule false)\n";

fn store() -> PathBuf {
    std::env::temp_dir().join(format!("dp-fuzz-certs-{}-{:?}", std::process::id(), std::thread::current().id()))
}

pub fn run(data: &[u8]) {
    let store = store();
    let content = hash::sha256(data);
    let dir = store.join(content.to_hex());
    fs::create_dir_all(&dir).expect("a temporary directory");
    for (name, bytes) in [("goal", GOAL), ("proof", PROOF), ("manifest", data)] {
        fs::write(dir.join(name), bytes).expect("a temporary file");
    }
    if let Ok(certificate) = Certificate::load(&store, content) {
        assert_eq!(certificate.content_hash(), content, "an entry under a name that is not its hash");
        assert_eq!((certificate.goal.as_slice(), certificate.proof.as_slice()), (GOAL, PROOF));
    }
    // Under another name, nothing loads.
    let mut other = content;
    other.0[0] ^= 1;
    assert!(Certificate::load(&store, other).is_err());
    let _ = fs::remove_dir_all(&dir);
}

pub fn seeds() -> Vec<Vec<u8>> {
    // The manifest of a real entry: stored, then read back.
    let store = store().join("seed");
    let certificate = Certificate {
        goal: GOAL.to_vec(),
        extension: ".smt2".into(),
        proof: PROOF.to_vec(),
        task_hash: Hash([1; 32]),
        driver: b"driver".to_vec(),
        prover: "cvc5@1.4.1".into(),
        checker_hash: Hash([2; 32]),
        signature_hash: Hash([3; 32]),
    };
    let content = certificate.store(&store, &mut dp_shell::store::write_dir_atomic).expect("a temporary store");
    let manifest = fs::read(store.join(content.to_hex()).join("manifest")).expect("the manifest");
    let _ = fs::remove_dir_all(&store);
    vec![manifest]
}
