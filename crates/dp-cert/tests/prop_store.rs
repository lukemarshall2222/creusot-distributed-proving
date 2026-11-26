// Feature: creusot-distributed-proving, Property 9: A certified Verdict has a complete entry in the Certificate_Store
// P-ID: P5.4.1, P5.4.2, P5.4.5, P5.4.8

//! Tests of the Certificate_Store (R5.4): an entry is read back by its
//! content hash, the hash names its contents and nothing else, and no
//! change of a stored file goes unnoticed.

use std::{fs, path::PathBuf};

use dp_cert::Certificate;
use dp_formats::hash::Hash;
use proptest::prelude::*;

fn bytes(max: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..max)
}

fn certificate() -> impl Strategy<Value = Certificate> {
    (bytes(40), "[.a-z0-9]{0,6}", bytes(60), any::<[u8; 32]>(), bytes(10), "[a-z0-9@.]{0,10}", any::<[u8; 32]>(), any::<[u8; 32]>())
        .prop_map(|(goal, extension, proof, task, driver, prover, checker, signature)| Certificate {
            goal,
            extension,
            proof,
            task_hash: Hash(task),
            driver,
            prover,
            checker_hash: Hash(checker),
            signature_hash: Hash(signature),
        })
}

/// A store of its own for each case.
fn store() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("dp-prop-store-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

proptest! {
    #![proptest_config(dp_testkit::config::cases("prop_store", 150))]

    #[test]
    fn an_entry_is_read_back_and_storing_again_changes_nothing(c in certificate()) {
        let store = store();
        let content = c.store(&store).unwrap();
        prop_assert_eq!(content, c.content_hash());
        prop_assert_eq!(&Certificate::load(&store, content).unwrap(), &c);
        // Idempotent: the same entry, the same name, the same files.
        let listing = |dir: &PathBuf| {
            let mut names: Vec<_> = fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name()).collect();
            names.sort();
            names
        };
        let before = listing(&store);
        prop_assert_eq!(c.store(&store).unwrap(), content);
        prop_assert_eq!(listing(&store), before);
        fs::remove_dir_all(store).unwrap();
    }

    /// The content hash names the whole entry: two entries that differ in
    /// anything have different names.
    #[test]
    fn entries_that_differ_have_different_names(c in certificate(), which in 0u8..8, byte in any::<u8>()) {
        let mut d = c.clone();
        match which {
            0 => d.goal.push(byte),
            1 => d.extension.push('x'),
            2 => d.proof.push(byte),
            3 => d.task_hash.0[usize::from(byte) % 32] ^= 1,
            4 => d.driver.push(byte),
            5 => d.prover.push('x'),
            6 => d.checker_hash.0[usize::from(byte) % 32] ^= 1,
            _ => d.signature_hash.0[usize::from(byte) % 32] ^= 1,
        }
        prop_assert_ne!(c.content_hash(), d.content_hash());
    }

    /// Any change of a stored file, a missing file, and an entry under
    /// another name are all found when the entry is loaded.
    #[test]
    fn a_damaged_entry_does_not_load(c in certificate(), file in 0usize..3, at in any::<prop::sample::Index>(), fault in 0u8..4) {
        let store = store();
        let content = c.store(&store).unwrap();
        let path = store.join(content.to_hex()).join(["goal", "proof", "manifest"][file]);
        let mut stored = fs::read(&path).unwrap();
        match fault {
            0 if !stored.is_empty() => {
                let i = at.index(stored.len());
                stored[i] ^= 1;
                fs::write(&path, &stored).unwrap();
            }
            1 => {
                stored.push(b'\n');
                fs::write(&path, &stored).unwrap();
            }
            2 => fs::remove_file(&path).unwrap(),
            _ => {
                // The whole entry, under the name of another.
                let mut other = content;
                other.0[31] ^= 1;
                fs::rename(store.join(content.to_hex()), store.join(other.to_hex())).unwrap();
                prop_assert!(Certificate::load(&store, other).is_err());
            }
        }
        prop_assert!(Certificate::load(&store, content).is_err());
        fs::remove_dir_all(store).unwrap();
    }
}

/// What a writer that was stopped leaves behind is no entry: a temporary
/// directory is not loaded, and the next writer still stores the entry.
#[test]
fn an_interrupted_write_leaves_no_entry() {
    let store = store();
    let c = Certificate {
        goal: b"(assert false)".to_vec(),
        extension: ".smt2".into(),
        proof: b"(step @p :rule false)".to_vec(),
        task_hash: Hash([1; 32]),
        driver: b"d".to_vec(),
        prover: "cvc5@".into(),
        checker_hash: Hash([2; 32]),
        signature_hash: Hash([3; 32]),
    };
    let content = c.content_hash();
    // The temporary directory of a writer, with the files and no rename.
    let tmp = store.join(format!(".{}.4242.tmp", content.to_hex()));
    fs::create_dir_all(&tmp).unwrap();
    fs::write(tmp.join("goal"), &c.goal).unwrap();
    fs::write(tmp.join("proof"), &c.proof).unwrap();
    assert!(Certificate::load(&store, content).is_err());
    assert_eq!(c.store(&store).unwrap(), content);
    assert_eq!(Certificate::load(&store, content).unwrap(), c);
    fs::remove_dir_all(store).unwrap();
}
