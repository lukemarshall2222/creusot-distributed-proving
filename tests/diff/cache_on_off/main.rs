// Feature: creusot-distributed-proving, Property 33: Turning a feature on or off never flips a proof against a counter-model
// P-ID: P7.5.5 (an entry that differs in one lookup or validation field is not used)

//! A warm Result_Cache whose entries each differ from the true ones in one
//! lookup field or one validation field (R7.5 criteria 3 to 5): no entry
//! answers a prover call, and the Session gives the labels of a Session
//! without a cache.
//!
//! Needs the development shell and `target/debug/dp-frontend`.

use std::{fs, path::Path};

use dp_diff::{leaves, project, proof_file, session, tools};
use dp_formats::cache::{Atom, Entry};

/// The Result_Cache hits of the summary line of a Session.
fn hits(stderr: &str) -> u64 {
    let line = stderr.lines().find(|l| l.contains("Result_Cache hits")).unwrap_or_else(|| panic!("{stderr}"));
    let before = &line[..line.find("Result_Cache hits").unwrap()];
    before.trim_end().rsplit(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap()
}

fn entries(dir: &Path) -> Vec<(std::path::PathBuf, Entry)> {
    let mut out = Vec::new();
    for file in fs::read_dir(dir.join(".dp-cache")).unwrap().flatten().filter(|f| f.path().is_file()) {
        if let Ok(entry) = Entry::decode(&fs::read(file.path()).unwrap(), 1 << 24) {
            out.push((file.path(), entry));
        }
    }
    assert!(!out.is_empty(), "the Session stored nothing");
    out
}

fn change(atom: &mut Atom) {
    match atom {
        Atom::Num(n) => *n = n.wrapping_add(1),
        Atom::Bytes(b) if b.is_empty() => b.push(b'x'),
        Atom::Bytes(b) => b[0] ^= 1,
    }
}

#[test]
fn an_entry_that_differs_in_one_field_answers_nothing() {
    let Some(t) = tools() else { return };
    // Without a cache: the labels to compare with.
    let cold = project("cache-cold", r#""alt-ergo""#);
    let out = session(&t, &cold, &t.lock);
    assert_eq!(hits(&String::from_utf8_lossy(&out.stderr)), 0);
    let expected = leaves(&proof_file(&cold));
    assert!(expected.values().any(|(label, _)| label == "proved-standalone"), "{expected:?}");
    let stored = entries(&cold);
    // The entries as stored do answer: the changes below are what stops them.
    let out = session(&t, &cold, &t.lock);
    assert!(hits(&String::from_utf8_lossy(&out.stderr)) > 0);
    assert_eq!(leaves(&proof_file(&cold)), expected);

    let lookup_fields = stored.iter().map(|(_, e)| e.lookup.len()).min().unwrap();
    assert!(lookup_fields >= 4, "an entry with {lookup_fields} lookup fields");
    let mut changes: Vec<(String, Box<dyn Fn(&mut Entry)>)> = Vec::new();
    for i in 0..lookup_fields {
        changes.push((format!("lookup field {i}"), Box::new(move |e| change(&mut e.lookup[i]))));
    }
    changes.push(("solver binary hash".into(), Box::new(|e| e.binary_hash.0[0] ^= 1)));
    changes.push(("Calibration value".into(), Box::new(|e| e.velocity += 1)));
    changes.push(("local time limit".into(), Box::new(|e| e.local_micros += 1)));
    changes.push(("import-set hash".into(), Box::new(|e| e.imports_hash.0[0] ^= 1)));

    for (k, (what, apply)) in changes.iter().enumerate() {
        let dir = project(&format!("cache-warm-{k}"), r#""alt-ergo""#);
        fs::create_dir_all(dir.join(".dp-cache")).unwrap();
        for (path, entry) in &stored {
            let mut entry = entry.clone();
            apply(&mut entry);
            // Under the same file name: the key that a lookup computes.
            fs::write(dir.join(".dp-cache").join(path.file_name().unwrap()), entry.encode()).unwrap();
        }
        let out = session(&t, &dir, &t.lock);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(hits(&stderr), 0, "{what}: an entry answered: {stderr}");
        assert_eq!(leaves(&proof_file(&dir)), expected, "{what}");
        fs::remove_dir_all(dir).unwrap();
    }
    fs::remove_dir_all(cold).unwrap();
}
