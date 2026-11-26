//! The static checks of the Lock_File (R7.7 criteria 6 and 9; design C20).
//!
//! Completeness (`check`): every tool of `expected.toml` has an entry of
//! its kind with a version and a pinned hash, no entry is for a tool that
//! is not listed, a tool listed as pending has no entry, and the entry of
//! the Rust libraries is the hash of the workspace's `Cargo.lock`.
//!
//! Evidence (`evidence`): for every Unpatched_Solver and every checker,
//! on every platform that its entry lists, the pinned hash is the
//! checksum that the upstream project publishes, or the hash of each of
//! two builds of the recorded upstream tag; and every entry lists every
//! platform. What is missing is reported: `ci/repro-build`, which does
//! not gate yet, shows it.

use std::{fs, path::Path};

use dp_formats::{
    hash,
    lockfile::{Kind, LockFile},
};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    pub platforms: Vec<String>,
    pub tool: Vec<ExpectedTool>,
    #[serde(default)]
    pub pending: Vec<Pending>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedTool {
    pub name: String,
    /// The kind, as the Lock_File spells it.
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub name: String,
    pub reason: String,
}

/// The kind of an entry, as the Lock_File spells it.
pub fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::UnpatchedSolver => "unpatched-solver",
        Kind::SharingSolver => "sharing-solver",
        Kind::Checker => "checker",
        Kind::CheckerSignature => "checker-signature",
        Kind::AletheRuleList => "alethe-rule-list",
        Kind::Why3 => "why3",
        Kind::Why3find => "why3find",
        Kind::Creusot => "creusot",
        Kind::Lean => "lean",
        Kind::Tla => "tla",
        Kind::Kani => "kani",
        Kind::CargoMutants => "cargo-mutants",
        Kind::Toolchain => "toolchain",
        Kind::Library => "library",
    }
}

/// The packages that Creusot's development shell takes from its own build
/// and not from the registry: inside the shell cargo writes their entries
/// of `Cargo.lock` without `source` and `checksum`.
pub const SHELL_PROVIDED: [&str; 3] = ["creusot-std", "creusot-std-proc", "pearlite-syn"];

/// `Cargo.lock` as it is hashed for the Lock_File: without the `source`
/// and `checksum` lines of the packages that the development shell
/// provides, so that the hash is the same inside the shell and outside.
/// `deps/gen-lock.sh` computes the same text.
pub fn normalized_cargo_lock(cargo_lock: &str) -> String {
    let mut out = String::new();
    let mut provided = false;
    for line in cargo_lock.lines() {
        if line == "[[package]]" {
            provided = false;
        }
        if let Some(name) = line.strip_prefix("name = \"").and_then(|n| n.strip_suffix('"')) {
            provided = SHELL_PROVIDED.contains(&name);
        }
        if provided && (line.starts_with("source = ") || line.starts_with("checksum = ")) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// R7.7 criterion 6. `cargo_lock` is the text of the workspace's
/// `Cargo.lock`.
pub fn check(lock: &LockFile, expected: &Expected, cargo_lock: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for e in &expected.tool {
        match lock.tool.iter().filter(|t| t.name == e.name).collect::<Vec<_>>().as_slice() {
            [] => problems.push(format!("{}: a tool of R7.7 criterion 1 without a Lock_File entry", e.name)),
            [tool] => {
                if kind_name(tool.kind) != e.kind {
                    problems.push(format!("{}: kind {}, expected {}", e.name, kind_name(tool.kind), e.kind));
                }
                if tool.version.trim().is_empty() {
                    problems.push(format!("{}: no version", e.name));
                }
                if tool.platform.is_empty() {
                    problems.push(format!("{}: no content hash on any platform", e.name));
                }
            }
            _ => problems.push(format!("{}: more than one entry", e.name)),
        }
    }
    for p in &expected.pending {
        if p.reason.trim().is_empty() {
            problems.push(format!("{}: pending without a reason", p.name));
        }
        if lock.tool.iter().any(|t| t.name == p.name) || expected.tool.iter().any(|e| e.name == p.name) {
            problems.push(format!("{}: listed as pending, and pinned", p.name));
        }
    }
    for tool in &lock.tool {
        if !expected.tool.iter().any(|e| e.name == tool.name) {
            problems.push(format!("{}: a Lock_File entry of a tool that expected.toml does not list", tool.name));
        }
    }
    // The Rust libraries are pinned by the workspace's lock file.
    let cargo = format!("sha256:{}", hash::sha256(normalized_cargo_lock(cargo_lock).as_bytes()).to_hex());
    for tool in lock.tool.iter().filter(|t| t.name == "rust-libraries") {
        for platform in &tool.platform {
            if platform.hash != cargo {
                problems.push(format!(
                    "rust-libraries ({}): pins {}, and Cargo.lock has {cargo}; run deps/gen-lock.sh",
                    platform.platform, platform.hash
                ));
            }
        }
    }
    problems
}

/// R7.7 criterion 9, and the platforms: what the Lock_File still lacks.
pub fn evidence(lock: &LockFile, expected: &Expected) -> Vec<String> {
    let mut missing = Vec::new();
    for tool in &lock.tool {
        for platform in &expected.platforms {
            if !tool.platform.iter().any(|p| &p.platform == platform) {
                missing.push(format!("{}: no entry for the platform {platform}", tool.name));
            }
        }
        if !matches!(tool.kind, Kind::UnpatchedSolver | Kind::Checker) {
            continue;
        }
        for p in &tool.platform {
            let upstream = p.upstream.as_ref();
            let published = upstream.and_then(|u| u.published_checksum.as_ref()).is_some_and(|c| c == &p.hash);
            let rebuilt = upstream.is_some_and(|u| u.rebuild_hashes.len() >= 2 && u.rebuild_hashes.iter().all(|h| h == &p.hash));
            if !published && !rebuilt {
                missing.push(format!(
                    "{} ({}): neither the upstream checksum nor two rebuilds give the pinned hash",
                    tool.name, p.platform
                ));
            }
        }
    }
    missing
}

pub struct Read {
    pub lock: LockFile,
    pub expected: Expected,
    pub cargo_lock: String,
}

pub fn read(root: &Path) -> Result<Read, String> {
    let text = |path: &str| fs::read_to_string(root.join(path)).map_err(|e| format!("{path}: {e}"));
    Ok(Read {
        lock: LockFile::parse(&text("deps/lock.toml")?).map_err(|e| e.to_string())?,
        expected: toml::from_str(&text("tools/lock-check/expected.toml")?).map_err(|e| format!("expected.toml: {e}"))?,
        cargo_lock: text("Cargo.lock")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Feature: creusot-distributed-proving, Property 36: Builds are reproducible and the trust story is consistent
    // P-ID: P7.7.6, P7.7.9 (a Lock_File that lacks a tool, a hash or its evidence is found)

    const CARGO: &str = "version = 4\n";

    fn entry(name: &str, kind: &str, hash: &str, upstream: &str) -> String {
        let prover = if kind.ends_with("solver") { "prover = \"x\"\n" } else { "" };
        format!(
            "[[tool]]\nname = \"{name}\"\nversion = \"1\"\nkind = \"{kind}\"\n{prover}\n[[tool.platform]]\nplatform = \"x86_64-linux\"\nhash = \"sha256:{hash}\"\n{upstream}\n"
        )
    }

    fn expected() -> Expected {
        toml::from_str(
            "platforms = [\"x86_64-linux\"]\ntool = [{ name = \"z3\", kind = \"unpatched-solver\" }, { name = \"rust-libraries\", kind = \"library\" }]\n[[pending]]\nname = \"alethe-rule-list\"\nreason = \"later\"\n",
        )
        .unwrap()
    }

    fn good() -> String {
        let h = "ab".repeat(32);
        let cargo = hash::sha256(normalized_cargo_lock(CARGO).as_bytes()).to_hex();
        format!(
            "{}{}",
            entry("z3", "unpatched-solver", &h, &format!("upstream = {{ tag = \"z3-1\", rebuild_hashes = [\"sha256:{h}\", \"sha256:{h}\"] }}")),
            entry("rust-libraries", "library", &cargo, "")
        )
    }

    fn parse(text: &str) -> LockFile {
        LockFile::parse(text).unwrap()
    }

    #[test]
    fn a_complete_lock_file_with_its_evidence_passes() {
        let lock = parse(&good());
        assert_eq!(check(&lock, &expected(), CARGO), Vec::<String>::new());
        assert_eq!(evidence(&lock, &expected()), Vec::<String>::new());
    }

    #[test]
    fn every_seeded_fault_of_completeness_is_found() {
        let found = |text: &str, expected: &Expected, cargo: &str, wanted: &str| {
            let problems = check(&parse(text), expected, cargo);
            assert!(problems.iter().any(|p| p.contains(wanted)), "{wanted:?} not in {problems:?}");
        };
        let good = good();
        let h = "ab".repeat(32);
        // A tool of the list without an entry.
        let without_z3: String = good.split("[[tool]]").filter(|e| !e.contains("\"z3\"")).collect::<Vec<_>>().join("[[tool]]");
        found(&without_z3, &expected(), CARGO, "z3: a tool of R7.7 criterion 1 without a Lock_File entry");
        // An entry of another kind, twice, without a hash, and of an unlisted tool.
        found(&good.replace("kind = \"unpatched-solver\"", "kind = \"sharing-solver\""), &expected(), CARGO, "z3: kind sharing-solver");
        found(&format!("{good}{}", entry("z3", "unpatched-solver", &h, "")), &expected(), CARGO, "z3: more than one entry");
        let no_platform = "[[tool]]\nname = \"z3\"\nversion = \"1\"\nkind = \"unpatched-solver\"\nprover = \"x\"\n";
        found(&format!("{no_platform}{}", entry("rust-libraries", "library", &h, "")), &expected(), CARGO, "z3: no content hash");
        found(&format!("{good}{}", entry("yices", "unpatched-solver", &h, "")), &expected(), CARGO, "yices: a Lock_File entry of a tool");
        // A pending tool that is pinned, and one without a reason.
        found(&format!("{good}{}", entry("alethe-rule-list", "alethe-rule-list", &h, "")), &expected(), CARGO, "listed as pending, and pinned");
        let mut silent = expected();
        silent.pending[0].reason.clear();
        found(&good, &silent, CARGO, "pending without a reason");
        // Cargo.lock changed since the Lock_File was generated.
        found(&good, &expected(), "version = 5\n", "rust-libraries (x86_64-linux): pins");
        // An entry without a version does not even parse.
        assert!(LockFile::parse(&good.replacen("version = \"1\"", "version = \"\"", 1)).is_err());
    }

    #[test]
    fn the_hash_of_the_libraries_is_the_same_inside_the_development_shell() {
        let outside = "version = 4\n\n[[package]]\nname = \"creusot-std\"\nversion = \"0.15.0\"\nsource = \"registry+x\"\nchecksum = \"abc\"\ndependencies = [\n \"pearlite-syn\",\n]\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"registry+x\"\nchecksum = \"def\"\n";
        let inside = outside.replace("version = \"0.15.0\"\nsource = \"registry+x\"\nchecksum = \"abc\"\n", "version = \"0.15.0\"\n");
        assert_ne!(outside, inside);
        assert_eq!(normalized_cargo_lock(outside), normalized_cargo_lock(&inside));
        // Any other package keeps its source and its checksum: another
        // checksum is another hash.
        assert!(normalized_cargo_lock(outside).contains("checksum = \"def\""));
        assert_ne!(normalized_cargo_lock(outside), normalized_cargo_lock(&outside.replace("def", "deg")));
    }

    #[test]
    fn missing_evidence_and_platforms_are_reported() {
        let h = "ab".repeat(32);
        let other = "cd".repeat(32);
        let cargo = entry("rust-libraries", "library", &hash::sha256(normalized_cargo_lock(CARGO).as_bytes()).to_hex(), "");
        let with = |upstream: &str| parse(&format!("{}{cargo}", entry("z3", "unpatched-solver", &h, upstream)));
        let report = |upstream: &str| evidence(&with(upstream), &expected());
        // No evidence, one rebuild, and two rebuilds of which one differs.
        assert_eq!(report("").len(), 1);
        assert_eq!(report(&format!("upstream = {{ tag = \"t\", rebuild_hashes = [\"sha256:{h}\"] }}")).len(), 1);
        assert_eq!(report(&format!("upstream = {{ tag = \"t\", rebuild_hashes = [\"sha256:{h}\", \"sha256:{other}\"] }}")).len(), 1);
        // A published checksum that is not the pinned hash, and one that is.
        assert_eq!(report(&format!("upstream = {{ published_checksum = \"sha256:{other}\" }}")).len(), 1);
        assert!(report(&format!("upstream = {{ published_checksum = \"sha256:{h}\" }}")).is_empty());
        // A platform that an entry does not list.
        let mut wide = expected();
        wide.platforms.push("aarch64-darwin".into());
        let missing = evidence(&parse(&good()), &wide);
        assert_eq!(missing.len(), 2, "{missing:?}");
        assert!(missing.iter().all(|m| m.contains("no entry for the platform aarch64-darwin")));
    }
}
