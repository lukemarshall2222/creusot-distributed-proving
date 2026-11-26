//! The static checks of the dependency patches (R7.9, R7.7 criterion 8;
//! design C20). `ci/jobs/patches.sh` does what needs the builds: that
//! each patch applies to its pinned upstream version and builds.
//!
//! A patch is a file `patches/<dependency>/<name>.patch`. The check fails
//! when
//!
//! - a patch has no header that states its purpose, the requirements it
//!   serves and its upstream status;
//! - the patches of one dependency change more lines than the limit, so
//!   that a patch stays small enough to be reviewed and sent upstream;
//! - a patch is named by no entry of the Lock_File, or an entry names a
//!   patch that is not there;
//! - an entry with patches is marked as an Unpatched_Solver, or pins no
//!   hash: a patched build is trusted for nothing by default.

use std::{collections::BTreeMap, fs, path::Path};

use dp_formats::lockfile::{Kind, LockFile};

/// The largest number of changed lines of the patches of one dependency.
/// A working value for `<TBD-21>`.
pub const MAX_CHANGED_LINES: usize = 600;

/// The header fields that every patch states, before its `---` line.
pub const HEADER_FIELDS: [&str; 3] = ["Purpose:", "Requirements: R", "Upstream:"];

/// The lines that a patch adds or removes.
pub fn changed_lines(patch: &str) -> usize {
    patch
        .lines()
        .skip_while(|l| *l != "---")
        .filter(|l| (l.starts_with('+') && !l.starts_with("+++")) || (l.starts_with('-') && !l.starts_with("---")))
        .count()
}

/// The header fields that a patch lacks.
pub fn missing_fields(patch: &str) -> Vec<&'static str> {
    let header: Vec<&str> = patch.lines().take_while(|l| *l != "---").collect();
    HEADER_FIELDS.iter().copied().filter(|field| !header.iter().any(|l| l.starts_with(field))).collect()
}

/// Checks the patches (path relative to the repository, text) against the
/// Lock_File. `exists` says whether a path that the Lock_File names is a
/// file.
pub fn check(patches: &[(String, String)], lock: &LockFile) -> Vec<String> {
    let mut problems = Vec::new();
    let mut per_dependency: BTreeMap<&str, usize> = BTreeMap::new();
    for (path, text) in patches {
        for field in missing_fields(text) {
            problems.push(format!("{path}: the header lacks \"{field}\""));
        }
        if !text.lines().any(|l| l == "---") {
            problems.push(format!("{path}: no `---` line between the header and the changes"));
        }
        let dependency = path.split('/').nth(1).unwrap_or(path);
        *per_dependency.entry(dependency).or_insert(0) += changed_lines(text);
        let named = lock.tool.iter().filter(|t| t.patches.iter().any(|p| p == path)).count();
        if named == 0 {
            problems.push(format!("{path}: no Lock_File entry names it"));
        }
    }
    for (dependency, lines) in per_dependency {
        if lines > MAX_CHANGED_LINES {
            problems.push(format!(
                "patches/{dependency}: {lines} changed lines, more than the limit of {MAX_CHANGED_LINES}"
            ));
        }
    }
    for tool in lock.tool.iter().filter(|t| !t.patches.is_empty()) {
        for named in &tool.patches {
            if !patches.iter().any(|(path, _)| path == named) {
                problems.push(format!("Lock_File entry {}: names {named}, which is no patch", tool.name));
            }
        }
        // `LockFile::parse` refuses this already; the rule is stated here
        // for a Lock_File that was built by other means.
        if matches!(tool.kind, Kind::UnpatchedSolver) {
            problems.push(format!("Lock_File entry {}: a patched build marked as an Unpatched_Solver", tool.name));
        }
        if tool.platform.is_empty() {
            problems.push(format!("Lock_File entry {}: a patched build without a pinned hash", tool.name));
        }
    }
    problems
}

/// The patches of the repository at `root`, in the order of their paths.
pub fn patches(root: &Path) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    let dir = root.join("patches");
    for dependency in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
        if !dependency.path().is_dir() {
            continue;
        }
        for file in fs::read_dir(dependency.path()).map_err(|e| e.to_string())?.flatten() {
            let name = file.file_name().to_string_lossy().into_owned();
            if name.ends_with(".patch") {
                let path = format!("patches/{}/{name}", dependency.file_name().to_string_lossy());
                out.push((path, fs::read_to_string(file.path()).map_err(|e| e.to_string())?));
            }
        }
    }
    out.sort();
    Ok(out)
}

pub fn run(root: &Path) -> Result<Vec<String>, String> {
    let lock_path = root.join("deps/lock.toml");
    let text = fs::read_to_string(&lock_path).map_err(|e| format!("{}: {e}", lock_path.display()))?;
    let lock = LockFile::parse(&text).map_err(|e| e.to_string())?;
    let patches = patches(root)?;
    if patches.is_empty() {
        return Err("no patch under patches/".to_string());
    }
    Ok(check(&patches, &lock))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Feature: creusot-distributed-proving, Property 36: Builds are reproducible and the trust story is consistent
    // P-ID: P7.9.1 to P7.9.4 (seeded faults of patches and of their Lock_File entries are found)

    const PATCH: &str = "Subject: Add a mode\n\nPurpose: a mode that the System needs.\nRequirements: R7.2, R7.9\nUpstream: not yet submitted\n---\n src/a.ml | 2 +-\n\n--- a/src/a.ml\n+++ b/src/a.ml\n@@ -1,3 +1,3 @@\n context\n-old\n+new\n context\n";

    fn lock(kind: &str, patches: &str, platform: bool) -> LockFile {
        let platform = if platform {
            format!("\n[[tool.platform]]\nplatform = \"x86_64-linux\"\nhash = \"sha256:{}\"\n", "ab".repeat(32))
        } else {
            String::new()
        };
        let prover = if kind.ends_with("solver") { "prover = \"x\"\n" } else { "" };
        LockFile::parse(&format!("[[tool]]\nname = \"dep\"\nversion = \"1\"\nkind = \"{kind}\"\n{prover}patches = [{patches}]\n{platform}"))
            .unwrap()
    }

    fn one(text: &str) -> Vec<(String, String)> {
        vec![("patches/dep/0001-mode.patch".to_string(), text.to_string())]
    }

    const NAMED: &str = "\"patches/dep/0001-mode.patch\"";

    #[test]
    fn a_patch_with_its_header_and_its_entry_passes() {
        assert_eq!(changed_lines(PATCH), 2);
        assert!(missing_fields(PATCH).is_empty());
        assert_eq!(check(&one(PATCH), &lock("why3find", NAMED, true)), Vec::<String>::new());
    }

    #[test]
    fn every_seeded_fault_is_found() {
        let found = |patches: &[(String, String)], lock: &LockFile, expected: &str| {
            let problems = check(patches, lock);
            assert!(problems.iter().any(|p| p.contains(expected)), "{expected:?} not in {problems:?}");
        };
        let good = lock("why3find", NAMED, true);
        // Header fields, each in turn, and a patch without a header.
        for field in HEADER_FIELDS {
            let name = field.split(':').next().unwrap();
            let without: String = PATCH.lines().filter(|l| !l.starts_with(name)).map(|l| format!("{l}\n")).collect();
            found(&one(&without), &good, &format!("the header lacks \"{field}\""));
        }
        found(&one(&PATCH.replace("Requirements: R7.2, R7.9", "Requirements: none")), &good, "Requirements: R");
        found(&one("+only a change\n"), &good, "no `---` line");
        // More changed lines than the limit, over the patches of one dependency.
        let big = format!("{PATCH}{}", "+line\n".repeat(MAX_CHANGED_LINES / 2));
        let two = vec![("patches/dep/0001-mode.patch".to_string(), big.clone()), ("patches/dep/0002-more.patch".to_string(), big)];
        found(&two, &lock("why3find", &format!("{NAMED}, \"patches/dep/0002-more.patch\""), true), "more than the limit");
        // The Lock_File and the patches name each other.
        found(&one(PATCH), &lock("why3find", "", true), "no Lock_File entry names it");
        found(&one(PATCH), &lock("why3find", &format!("{NAMED}, \"patches/dep/0009-gone.patch\""), true), "which is no patch");
        // A patched build without a pinned hash.
        found(&one(PATCH), &lock("why3find", NAMED, false), "without a pinned hash");
        // A patched build as an Unpatched_Solver does not even parse.
        let text = format!("[[tool]]\nname = \"dep\"\nversion = \"1\"\nkind = \"unpatched-solver\"\nprover = \"x\"\npatches = [{NAMED}]\n");
        assert!(LockFile::parse(&text).is_err());
    }

    #[test]
    fn the_repository_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let problems = run(&root).unwrap();
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }
}
