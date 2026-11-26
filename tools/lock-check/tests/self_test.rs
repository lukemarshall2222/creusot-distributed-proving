// Feature: creusot-distributed-proving, Property 36: Builds are reproducible and the trust story is consistent
// P-ID: P7.7.6, P7.7.9

//! The self-test of the Lock_File check on changed copies of this
//! repository's `deps/lock.toml`, `tools/lock-check/expected.toml` and
//! `Cargo.lock`: the files as they are pass, and each change below, made
//! to a fresh copy, is found.

use std::path::Path;

use dp_formats::lockfile::LockFile;
use lock_check::{check, evidence, Expected};

struct Copy {
    lock: String,
    expected: String,
    cargo_lock: String,
}

fn repository() -> Copy {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = |path: &str| std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
    Copy { lock: text("deps/lock.toml"), expected: text("tools/lock-check/expected.toml"), cargo_lock: text("Cargo.lock") }
}

fn expected(copy: &Copy) -> Expected {
    toml::from_str(&copy.expected).expect("expected.toml parses")
}

/// The problems of a copy; a Lock_File that does not parse is one problem.
fn problems(copy: &Copy) -> Vec<String> {
    match LockFile::parse(&copy.lock) {
        Ok(lock) => check(&lock, &expected(copy), &copy.cargo_lock),
        Err(e) => vec![format!("the Lock_File is refused: {e}")],
    }
}

fn changed(change: impl FnOnce(&mut Copy)) -> Vec<String> {
    let mut copy = repository();
    change(&mut copy);
    problems(&copy)
}

#[track_caller]
fn fails(problems: &[String], expected: &str) {
    assert!(problems.iter().any(|p| p.contains(expected)), "{expected:?} is not among {problems:#?}");
}

/// The entry of a tool in the text of the Lock_File: from its `[[tool]]`
/// line to the next one.
fn entry(lock: &str, name: &str) -> (usize, usize) {
    let at = lock.find(&format!("name = \"{name}\"\n")).unwrap_or_else(|| panic!("{name} has an entry"));
    let start = lock[..at].rfind("[[tool]]").unwrap();
    let end = lock[at..].find("\n[[tool]]").map(|e| at + e + 1).unwrap_or(lock.len());
    (start, end)
}

fn names(copy: &Copy) -> Vec<String> {
    expected(copy).tool.into_iter().map(|t| t.name).collect()
}

#[test]
fn the_repository_passes_as_it_is() {
    let copy = repository();
    assert_eq!(problems(&copy), Vec::<String>::new());
    assert!(names(&copy).len() >= 17);
}

// A missing tool: every tool of the list, one at a time.
#[test]
fn a_tool_without_an_entry_is_found() {
    for name in names(&repository()) {
        let found = changed(|copy| {
            let (start, end) = entry(&copy.lock, &name);
            copy.lock.replace_range(start..end, "");
        });
        assert_eq!(found, [format!("{name}: a tool of R7.7 criterion 1 without a Lock_File entry")]);
    }
}

// A Lock_File entry without a hash: the hash line removed, emptied or
// malformed, and the whole platform removed, for every tool.
#[test]
fn an_entry_without_a_hash_is_found() {
    for name in names(&repository()) {
        let hash_line = |copy: &Copy| {
            let (start, end) = entry(&copy.lock, &name);
            let at = start + copy.lock[start..end].find("hash = \"").expect("the entry has a hash");
            (at, at + copy.lock[at..].find('\n').unwrap() + 1)
        };
        let found = changed(|copy| {
            let (at, end) = hash_line(copy);
            copy.lock.replace_range(at..end, "");
        });
        fails(&found, "the Lock_File is refused");
        let found = changed(|copy| {
            let (at, end) = hash_line(copy);
            copy.lock.replace_range(at..end, "hash = \"\"\n");
        });
        fails(&found, "the Lock_File is refused");
        let found = changed(|copy| {
            let (at, end) = hash_line(copy);
            copy.lock.replace_range(at..end, "hash = \"sha256:abc\"\n");
        });
        fails(&found, "the Lock_File is refused");
        let found = changed(|copy| {
            let (start, end) = entry(&copy.lock, &name);
            let at = start + copy.lock[start..end].find("[[tool.platform]]").unwrap();
            copy.lock.replace_range(at..end, "");
        });
        assert_eq!(found, [format!("{name}: no content hash on any platform")]);
    }
}

// A patched build marked as an Unpatched_Solver: every entry with patches.
#[test]
fn a_patched_build_marked_unpatched_is_refused() {
    let copy = repository();
    let lock = LockFile::parse(&copy.lock).unwrap();
    let patched: Vec<_> = lock.tool.iter().filter(|t| !t.patches.is_empty()).collect();
    assert!(patched.len() >= 2);
    for tool in patched {
        let found = changed(|copy| {
            let (start, end) = entry(&copy.lock, &tool.name);
            let text = copy.lock[start..end].to_string();
            let kind = text.lines().find(|l| l.starts_with("kind = ")).unwrap().to_string();
            copy.lock.replace_range(start..end, &text.replace(&kind, "kind = \"unpatched-solver\"\nprover = \"x\""));
        });
        assert_eq!(found.len(), 1, "{found:#?}");
        fails(&found, "the Lock_File is refused");
    }
}

// Other disagreements of the Lock_File with the list of tools.
#[test]
fn entries_that_the_list_does_not_have_are_found() {
    let copy = repository();
    let (start, end) = entry(&copy.lock, "z3");
    let z3 = copy.lock[start..end].to_string();
    // An entry of a tool that is not listed, an entry twice, another kind.
    fails(&changed(|c| c.lock.push_str(&z3.replace("\"z3\"", "\"yices\""))), "yices: a Lock_File entry of a tool that expected.toml does not list");
    fails(&changed(|c| c.lock.push_str(&z3)), "z3");
    let found = changed(|c| {
        let (start, end) = entry(&c.lock, "z3");
        let text = c.lock[start..end].replace("kind = \"unpatched-solver\"", "kind = \"sharing-solver\"");
        c.lock.replace_range(start..end, &text);
    });
    fails(&found, "z3: kind sharing-solver, expected unpatched-solver");
    // A tool that is listed as pending is pinned after all.
    let pending = expected(&copy).pending;
    assert!(!pending.is_empty());
    for p in &pending {
        let found = changed(|c| {
            let text = z3.replace("\"z3\"", &format!("\"{}\"", p.name));
            c.lock.push_str(&text);
        });
        fails(&found, &format!("{}: listed as pending, and pinned", p.name));
        // And a pending tool says why.
        let found = changed(|c| c.expected = c.expected.replace(&format!("reason = \"{}\"", p.reason), "reason = \"\""));
        fails(&found, &format!("{}: pending without a reason", p.name));
    }
    // A tool that the list gains.
    let found = changed(|c| c.expected = c.expected.replace("tool = [\n", "tool = [\n    { name = \"yices\", kind = \"unpatched-solver\" },\n"));
    assert_eq!(found, ["yices: a tool of R7.7 criterion 1 without a Lock_File entry"]);
}

// The Rust libraries: any change of a pinned package of `Cargo.lock`.
#[test]
fn a_changed_cargo_lock_is_found() {
    let copy = repository();
    let checksum = copy.cargo_lock.lines().find(|l| l.starts_with("checksum = \"")).expect("Cargo.lock has checksums");
    let mut other = checksum.to_string();
    let digit = other.len() - 2;
    other.replace_range(digit..digit + 1, if &checksum[digit..digit + 1] == "0" { "1" } else { "0" });
    let found = changed(|c| c.cargo_lock = c.cargo_lock.replacen(checksum, &other, 1));
    assert_eq!(found.len(), 1);
    fails(&found, "rust-libraries (");
    let found = changed(|c| c.cargo_lock.push_str("\n[[package]]\nname = \"one-more\"\nversion = \"1.0.0\"\n"));
    fails(&found, "rust-libraries (");
}

// The evidence of R7.7 criterion 9: for each Unpatched_Solver and checker,
// the report goes away only with two rebuilds or a published checksum
// that give the pinned hash.
#[test]
fn the_evidence_of_a_pinned_hash_is_two_equal_rebuilds_or_the_published_checksum() {
    let copy = repository();
    let lock = LockFile::parse(&copy.lock).unwrap();
    let before = evidence(&lock, &expected(&copy));
    let mut tried = 0;
    for name in ["alt-ergo", "z3", "cvc5", "cvc4"] {
        let (start, end) = entry(&copy.lock, name);
        let text = &copy.lock[start..end];
        let hash = text.lines().find_map(|l| l.strip_prefix("hash = \"")).unwrap().trim_end_matches('"');
        let line = text.lines().find(|l| l.starts_with("upstream = ")).expect("an Unpatched_Solver records its upstream");
        let lacks = |report: &[String]| report.iter().any(|m| m.starts_with(&format!("{name} (")) && m.contains("neither"));
        assert!(lacks(&before), "{name} has no evidence yet on this platform");
        let with = |upstream: String| {
            let mut c = repository();
            let (start, end) = entry(&c.lock, name);
            let changed = c.lock[start..end].replace(line, &upstream);
            c.lock.replace_range(start..end, &changed);
            evidence(&LockFile::parse(&c.lock).expect("the changed Lock_File parses"), &expected(&c))
        };
        let wrong = format!("sha256:{}", "0".repeat(64));
        assert!(!lacks(&with(format!("upstream = {{ tag = \"t\", rebuild_hashes = [\"{hash}\", \"{hash}\"] }}"))));
        assert!(!lacks(&with(format!("upstream = {{ published_checksum = \"{hash}\" }}"))));
        assert!(lacks(&with(format!("upstream = {{ tag = \"t\", rebuild_hashes = [\"{hash}\"] }}"))));
        assert!(lacks(&with(format!("upstream = {{ tag = \"t\", rebuild_hashes = [\"{hash}\", \"{wrong}\"] }}"))));
        assert!(lacks(&with(format!("upstream = {{ published_checksum = \"{wrong}\" }}"))));
        tried += 1;
    }
    assert_eq!(tried, 4);
    // Every entry is reported for the platforms that it does not list.
    for tool in &lock.tool {
        assert!(before.iter().any(|m| m.starts_with(&format!("{}: no entry for the platform", tool.name))), "{}", tool.name);
    }
}
