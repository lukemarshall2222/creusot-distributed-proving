//! The format registry `docs/formats/index.toml` (R7.10 criterion 7).
//!
//! An entry names, for one exchanged format, its grammar file, its codec
//! module, its round-trip test, its fuzz target and its size limit. The
//! check fails when an entry names something that does not exist, and when
//! an exchanged format has neither an entry nor a pending record although
//! a requirement that R7.10's list cites for it is in scope (R7.10 itself
//! for a format that cites none); this scope rule is the plan's reading of
//! the criterion (tasks.md, Notes, format registry).
//!
//! A pending record names a format whose entry cannot be written yet, with
//! what is missing and why. It does not fail the check; it is printed on
//! every run, so that what is owed stays in view. A pending record fails
//! when it names nothing that is missing, and when everything that it
//! names as missing has been given a path that exists: then the format is
//! registered.

use std::collections::BTreeSet;

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    #[serde(default)]
    pub format: Vec<Entry>,
    #[serde(default)]
    pub pending: Vec<Pending>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    /// A file under `docs/formats/`.
    pub grammar: String,
    pub codec: String,
    pub roundtrip: String,
    pub fuzz: String,
    /// The largest input in bytes, or the open parameter that decides it.
    pub size_limit: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub name: String,
    /// Which of `grammar`, `codec`, `roundtrip`, `fuzz` do not exist yet.
    pub missing: Vec<String>,
    pub reason: String,
}

pub const PARTS: [&str; 4] = ["grammar", "codec", "roundtrip", "fuzz"];

/// The exchanged formats of R7.10, each with the requirements that the
/// list cites for it. A format that cites none belongs to R7.10.
pub const EXCHANGED: [(&str, &[&str]); 28] = [
    ("canonical-name", &["R4.1"]),
    ("smtlib-script", &["R4.1"]),
    ("lemma-term", &["R3.3"]),
    ("hint-term", &["R4.4"]),
    ("lemma-message", &["R3.2", "R4.4"]),
    ("provenance-record", &["R7.10"]),
    ("result-message", &["R1.3"]),
    ("protocol-messages", &["R1.1", "R1.5", "R1.6", "R1.7", "R3.1", "R3.6"]),
    ("dispatch", &["R7.10"]),
    ("solver-channel", &["R3.1", "R4.4"]),
    ("status-report", &["R1.7"]),
    ("cpc-proof", &["R5.2"]),
    ("alethe-proof", &["R5.2"]),
    ("composed-certificate", &["R5.2"]),
    ("checker-output", &["R5.3", "R5.4"]),
    ("certificate-manifest", &["R5.4"]),
    ("solver-configuration", &["R2.1"]),
    ("worker-registry", &["R1.3"]),
    ("partition-tree", &["R3.5"]),
    ("proofjson-extension", &["R2.3", "R7.2"]),
    ("cache-entry", &["R7.5"]),
    ("lock-file", &["R7.7"]),
    ("tcb-checker-entries", &["R7.8"]),
    ("event-trace", &["R1.5"]),
    ("corpus-metadata", &["R6.1"]),
    ("pre-registration", &["R6.2"]),
    ("run-manifest", &["R6.3"]),
    ("report-provenance", &["R6.4"]),
];

/// The pending records, as lines for the report of a run.
pub fn pending(registry: &Registry) -> Vec<String> {
    registry
        .pending
        .iter()
        .map(|p| format!("pending: format {} lacks {} ({})", p.name, p.missing.join(", "), p.reason))
        .collect()
}

/// Every problem of the registry on a commit with these tracked files and
/// these requirements in scope.
pub fn check(registry: &Registry, tracked: &BTreeSet<String>, in_scope: &BTreeSet<&str>) -> Vec<String> {
    let mut problems = Vec::new();
    let known = |name: &str| EXCHANGED.iter().any(|(format, _)| *format == name);
    let mut seen = BTreeSet::new();
    for entry in &registry.format {
        if !seen.insert(entry.name.as_str()) {
            problems.push(format!("format {}: two entries", entry.name));
        }
        if !known(&entry.name) {
            problems.push(format!("format {}: no exchanged format of R7.10 has this name", entry.name));
        }
        for (part, path) in [
            ("grammar", &entry.grammar),
            ("codec module", &entry.codec),
            ("round-trip test", &entry.roundtrip),
            ("fuzz target", &entry.fuzz),
        ] {
            if !crate::exists(path, tracked) {
                problems.push(format!("format {}: its {part} {path} does not exist", entry.name));
            }
        }
        if !entry.grammar.starts_with("docs/formats/") {
            problems.push(format!("format {}: its grammar {} is not under docs/formats/", entry.name, entry.grammar));
        }
        if entry.size_limit.trim().is_empty() {
            problems.push(format!("format {}: no size limit", entry.name));
        }
    }
    for record in &registry.pending {
        if !seen.insert(record.name.as_str()) {
            problems.push(format!("format {}: pending, and listed twice or registered", record.name));
        }
        if !known(&record.name) {
            problems.push(format!("format {}: no exchanged format of R7.10 has this name", record.name));
        }
        if record.missing.is_empty() || record.reason.trim().is_empty() {
            problems.push(format!("format {}: pending without what is missing or without a reason", record.name));
        }
        for part in record.missing.iter().filter(|part| !PARTS.contains(&part.as_str())) {
            problems.push(format!("format {}: pending for {part:?}, which is no part of an entry", record.name));
        }
    }
    for (format, cited) in EXCHANGED {
        let needed: Vec<&&str> = cited.iter().filter(|req| in_scope.contains(**req)).collect();
        if !needed.is_empty() && !seen.contains(format) {
            problems.push(format!(
                "format {format}: no entry in docs/formats/index.toml, and {} is in scope",
                needed.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    // Feature: creusot-distributed-proving, Property 34: Every exchanged format has one grammar and one strict codec
    // P-ID: P7.10.6 (the registry names what exists, for every format that is owed)

    const GOOD: &str = "[[format]]\nname = \"cache-entry\"\ngrammar = \"docs/formats/cache-entry.abnf\"\n\
codec = \"crates/f/src/cache.rs\"\nroundtrip = \"crates/f/tests/prop_cache.rs\"\nfuzz = \"fuzz/cache_entry/\"\nsize_limit = \"16777216\"\n\n\
[[pending]]\nname = \"lock-file\"\nmissing = [\"grammar\", \"fuzz\"]\nreason = \"not written\"\n";

    fn tracked() -> BTreeSet<String> {
        ["docs/formats/cache-entry.abnf", "crates/f/src/cache.rs", "crates/f/tests/prop_cache.rs", "fuzz/cache_entry/harness.rs"]
            .iter()
            .map(|f| f.to_string())
            .collect()
    }

    fn run(text: &str, scope: &[&str]) -> Vec<String> {
        check(&toml::from_str(text).unwrap(), &tracked(), &scope.iter().copied().collect())
    }

    #[test]
    fn a_registry_that_names_what_exists_passes() {
        assert_eq!(run(GOOD, &["R7.5", "R7.7"]), Vec::<String>::new());
        assert_eq!(run("", &[]), Vec::<String>::new());
        assert_eq!(pending(&toml::from_str(GOOD).unwrap()), ["pending: format lock-file lacks grammar, fuzz (not written)"]);
    }

    #[test]
    fn every_seeded_fault_of_the_registry_is_found() {
        let found = |text: &str, scope: &[&str], expected: &str| {
            let problems = run(text, scope);
            assert!(problems.iter().any(|p| p.contains(expected)), "{expected:?} not in {problems:?}");
        };
        // Each of the four artifacts of an entry, when it does not exist.
        found(&GOOD.replace("cache-entry.abnf", "cache.abnf"), &[], "format cache-entry: its grammar docs/formats/cache.abnf does not exist");
        found(&GOOD.replace("src/cache.rs", "src/entry.rs"), &[], "its codec module crates/f/src/entry.rs does not exist");
        found(&GOOD.replace("prop_cache.rs", "roundtrip.rs"), &[], "its round-trip test crates/f/tests/roundtrip.rs does not exist");
        found(&GOOD.replace("fuzz/cache_entry/", "fuzz/cache/"), &[], "its fuzz target fuzz/cache/ does not exist");
        found(&GOOD.replace("docs/formats/cache-entry.abnf", "crates/f/src/cache.rs"), &[], "is not under docs/formats/");
        found(&GOOD.replace("16777216", " "), &[], "format cache-entry: no size limit");
        // A format that is owed and has no entry and no pending record.
        found(GOOD, &["R1.3"], "format result-message: no entry in docs/formats/index.toml, and R1.3 is in scope");
        found(GOOD, &["R1.3"], "format worker-registry: no entry");
        found("", &["R7.10"], "format dispatch: no entry in docs/formats/index.toml, and R7.10 is in scope");
        // Not owed while the requirements that cite it are not in scope.
        assert!(!run(GOOD, &["R7.5"]).iter().any(|p| p.contains("result-message")));
        // Names and pending records.
        found(&GOOD.replace("name = \"cache-entry\"", "name = \"cache\""), &[], "format cache: no exchanged format of R7.10 has this name");
        found(&GOOD.replace("name = \"lock-file\"", "name = \"cache-entry\""), &[], "format cache-entry: pending, and listed twice or registered");
        found(&GOOD.replace("[\"grammar\", \"fuzz\"]", "[]"), &[], "format lock-file: pending without what is missing");
        found(&GOOD.replace("not written", ""), &[], "format lock-file: pending without what is missing or without a reason");
        found(&GOOD.replace("\"fuzz\"]", "\"tests\"]"), &[], "pending for \"tests\"");
        // A key that the registry does not have.
        assert!(toml::from_str::<Registry>(&GOOD.replace("size_limit", "limit")).is_err());
    }

    #[test]
    fn the_list_of_exchanged_formats_has_no_name_twice() {
        let names: BTreeSet<&str> = EXCHANGED.iter().map(|(name, _)| *name).collect();
        assert_eq!(names.len(), EXCHANGED.len());
    }
}
