// Feature: creusot-distributed-proving, Property 36: Builds are reproducible and the trust story is consistent
// P-ID: P7.9.1, P7.9.2, P7.9.3, P7.9.4

//! The self-test of the patch check on changed copies of this
//! repository's patches and Lock_File: as they are they pass, and each
//! change below, made to a fresh copy, is found.

use std::path::Path;

use dp_formats::lockfile::LockFile;
use patch_check::{changed_lines, check, patches, HEADER_FIELDS, MAX_CHANGED_LINES};

struct Copy {
    patches: Vec<(String, String)>,
    lock: String,
}

fn repository() -> Copy {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    Copy { patches: patches(&root).unwrap(), lock: std::fs::read_to_string(root.join("deps/lock.toml")).unwrap() }
}

fn changed(change: impl FnOnce(&mut Copy)) -> Vec<String> {
    let mut copy = repository();
    change(&mut copy);
    match LockFile::parse(&copy.lock) {
        Ok(lock) => check(&copy.patches, &lock),
        Err(e) => vec![format!("the Lock_File is refused: {e}")],
    }
}

#[track_caller]
fn fails(problems: &[String], expected: &str) {
    assert!(problems.iter().any(|p| p.contains(expected)), "{expected:?} is not among {problems:#?}");
}

#[test]
fn the_repository_passes_as_it_is() {
    assert_eq!(changed(|_| {}), Vec::<String>::new());
    assert!(repository().patches.len() >= 2);
}

// A patch without header fields: each field of each patch, removed in
// turn, with the lines that continue it; and the header as a whole.
#[test]
fn a_patch_without_a_header_field_is_found() {
    let count = repository().patches.len();
    for index in 0..count {
        for field in HEADER_FIELDS {
            let mut path = String::new();
            let found = changed(|copy| {
                let (p, text) = &mut copy.patches[index];
                path = p.clone();
                let mut kept = Vec::new();
                let mut dropping = false;
                let mut header = true;
                for line in text.lines() {
                    if line == "---" {
                        header = false;
                    }
                    if header && line.starts_with(field) {
                        dropping = true;
                        continue;
                    }
                    // The lines that continue a field are indented.
                    if header && dropping && line.starts_with(' ') {
                        continue;
                    }
                    dropping = false;
                    kept.push(line);
                }
                *text = kept.join("\n") + "\n";
            });
            assert_eq!(found, [format!("{path}: the header lacks \"{field}\"")]);
        }
        let found = changed(|copy| {
            let text = &mut copy.patches[index].1;
            let at = text.find("\n---\n").expect("the patch has a header") + 1;
            text.replace_range(..at, "");
        });
        assert_eq!(found.len(), HEADER_FIELDS.len(), "{found:#?}");
        // The line between the header and the changes.
        let found = changed(|copy| {
            let text = &mut copy.patches[index].1;
            *text = text.replacen("\n---\n", "\n", 1);
        });
        fails(&found, "no `---` line between the header and the changes");
    }
}

// The patches of a dependency over the limit of changed lines: each
// dependency grown to one line more than the limit, and to the limit.
#[test]
fn patches_over_the_limit_of_changed_lines_are_found() {
    let copy = repository();
    for index in 0..copy.patches.len() {
        let (path, text) = &copy.patches[index];
        let dependency = path.split('/').nth(1).unwrap().to_string();
        let now: usize = copy.patches.iter().filter(|(p, _)| p.split('/').nth(1) == Some(&dependency)).map(|(_, t)| changed_lines(t)).sum();
        assert!(now <= MAX_CHANGED_LINES, "{path}: {} changed lines", changed_lines(text));
        let grow = |by: usize| {
            changed(|copy| {
                let text = &mut copy.patches[index].1;
                for i in 0..by {
                    text.push_str(&format!("+one more line {i}\n"));
                }
            })
        };
        assert_eq!(grow(MAX_CHANGED_LINES - now), Vec::<String>::new());
        assert_eq!(
            grow(MAX_CHANGED_LINES - now + 1),
            [format!("patches/{dependency}: {} changed lines, more than the limit of {MAX_CHANGED_LINES}", MAX_CHANGED_LINES + 1)]
        );
    }
    // A second patch of the same dependency counts with the first.
    let found = changed(|copy| {
        let (path, text) = copy.patches[0].clone();
        copy.patches.push((path.replace("0001", "0002"), text.clone() + &"+x\n".repeat(MAX_CHANGED_LINES)));
    });
    fails(&found, "more than the limit");
    fails(&found, "no Lock_File entry names it");
}

// Patches and Lock_File entries that do not match, and a patched build
// that is trusted as if it were unpatched or pins nothing.
#[test]
fn a_patch_and_its_lock_file_entry_must_match() {
    let copy = repository();
    for index in 0..copy.patches.len() {
        let path = copy.patches[index].0.clone();
        // The entry no longer names the patch.
        let found = changed(|c| c.lock = c.lock.replace(&format!("patches = [\"{path}\"]\n"), ""));
        assert_eq!(found, [format!("{path}: no Lock_File entry names it")]);
        // The patch is gone and the entry still names it.
        let found = changed(|c| drop(c.patches.remove(index)));
        assert_eq!(found.len(), 1);
        fails(&found, &format!("names {path}, which is no patch"));
        // The patch is renamed.
        let found = changed(|c| c.patches[index].0 = path.replace("0001", "0009"));
        assert_eq!(found.len(), 2, "{found:#?}");
        // The patched build is marked as an Unpatched_Solver.
        let found = changed(|c| {
            let at = c.lock.find(&format!("patches = [\"{path}\"]")).unwrap();
            let start = c.lock[..at].rfind("kind = ").unwrap();
            c.lock.replace_range(start..at, "kind = \"unpatched-solver\"\nprover = \"x\"\n");
        });
        assert_eq!(found.len(), 1);
        fails(&found, "the Lock_File is refused");
        // The patched build pins no hash.
        let found = changed(|c| {
            let at = c.lock.find(&format!("patches = [\"{path}\"]")).unwrap();
            let platform = at + c.lock[at..].find("[[tool.platform]]").unwrap();
            let end = c.lock[platform..].find("\n[[tool]]").map(|e| platform + e + 1).unwrap_or(c.lock.len());
            c.lock.replace_range(platform..end, "");
        });
        assert_eq!(found.len(), 1, "{found:#?}");
        fails(&found, "a patched build without a pinned hash");
    }
    // An unpatched solver that gains a patch is refused as well.
    let found = changed(|c| c.lock = c.lock.replace("prover = \"z3\"\n", "prover = \"z3\"\npatches = [\"patches/z3/0001-x.patch\"]\n"));
    assert_eq!(found.len(), 1);
    fails(&found, "the Lock_File is refused");
}
