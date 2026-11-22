//! The final write of the proof files (R1.5 criterion 14; design C2,
//! "`proof.json` sequence").
//!
//! why3find writes its proof files with the legacy members when its run
//! ends. The Front_End then rewrites each file atomically: every Goal entry
//! gets the member `"dp"` with the label of its Verdict, and the file gets
//! the member `"dp-lock"` with the hash of the Lock_File.
//!
//! The rewrite never makes a Goal proved for a legacy reader, and it takes a
//! proof away when the Session has no proof label for it: an entry with a
//! `"prover"` member stays only if the Verdict of its Goal is a proof.
//! Unmodified why3find reads an entry without `"prover"` and `"tactic"`
//! members as not proved (AS-19), so an unproved Goal, which why3find
//! writes as `null`, becomes an object with the member `"dp"` alone.

use std::{collections::BTreeMap, io, path::Path};

use dp_formats::{hash::Hash, proofjson::DpEntry};
use dp_shell::store;
use serde_json::{Map, Value};

/// The label of one Goal of a proof file.
pub struct GoalLabel {
    pub theory: String,
    /// The name of the root Goal in the theory.
    pub name: String,
    /// The tactic steps from the root Goal: tactic name and child index.
    pub steps: Vec<(String, u32)>,
    pub entry: DpEntry,
    /// Whether the label is a proof label.
    pub proof: bool,
}

/// What a rewrite found.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Report {
    /// Number of Goal entries by label.
    pub labels: BTreeMap<String, u64>,
    /// Entries that why3find wrote as proved without a proof label of this
    /// Session; they were written as not proved.
    pub withdrawn: Vec<String>,
    /// Goals with a proof label and an Invalid answer of another run.
    pub conflicts: Vec<String>,
}

struct Walk<'a> {
    goals: BTreeMap<(&'a str, &'a str, &'a [(String, u32)]), &'a GoalLabel>,
    unresolved: &'a DpEntry,
    report: Report,
}

impl Walk<'_> {
    fn node(&mut self, theory: &str, name: &str, steps: &mut Vec<(String, u32)>, node: &mut Value) {
        if let Value::Object(members) = node {
            let tactic = members.get("tactic").and_then(Value::as_str).map(str::to_string);
            if let (Some(tactic), Some(Value::Array(children))) = (tactic, members.get_mut("children")) {
                for (i, child) in children.iter_mut().enumerate() {
                    steps.push((tactic.clone(), i as u32));
                    self.node(theory, name, steps, child);
                    steps.pop();
                }
                members.remove("dp");
                return;
            }
        }
        // A leaf: a Goal that a prover proved, or that nothing proved.
        let found = self.goals.get(&(theory, name, steps.as_slice())).copied();
        let proved_in_file = node.get("prover").is_some();
        let entry = found.map_or(self.unresolved, |g| &g.entry);
        let proof = found.is_some_and(|g| g.proof);
        *self.report.labels.entry(entry.label.clone()).or_insert(0) += 1;
        if entry.conflict {
            let path: Vec<_> = steps.iter().map(|(t, i)| format!("{t}.{i}")).collect();
            let provers: Vec<_> = entry.invalid.iter().map(|i| i.prover.as_str()).collect();
            self.report.conflicts.push(format!(
                "{theory}.{name} {}: labelled {}, and {} answered Invalid",
                path.join("/"),
                entry.label,
                provers.join(", ")
            ));
        }
        let member = Value::String(entry.to_member());
        match node {
            Value::Object(members) if proved_in_file && proof => {
                // The legacy member names the prover of why3find's call. In
                // a race another prover may have proved the Goal: the
                // member then names that one, so a legacy reader replays
                // the proof with the prover that found it.
                if let Some(winner) = entry.replay.first() {
                    let named = members.get("prover").and_then(Value::as_str).unwrap_or("");
                    if named.split('@').next() != Some(winner.prover.as_str()) {
                        members.insert("prover".to_string(), Value::String(winner.prover.clone()));
                    }
                }
                members.insert("dp".to_string(), member);
            }
            _ => {
                if proved_in_file {
                    let path: Vec<_> = steps.iter().map(|(t, i)| format!("{t}.{i}")).collect();
                    self.report.withdrawn.push(format!("{theory}.{name} {}", path.join("/")));
                }
                let mut members = Map::new();
                members.insert("dp".to_string(), member);
                *node = Value::Object(members);
            }
        }
    }
}

/// Adds the extension members to the text of a proof file.
pub fn extend(
    text: &str,
    lock_hash: Hash,
    goals: &[GoalLabel],
    unresolved: &DpEntry,
) -> Result<(String, Report), serde_json::Error> {
    let mut file: Value = serde_json::from_str(text)?;
    let mut walk = Walk {
        goals: goals.iter().map(|g| ((g.theory.as_str(), g.name.as_str(), g.steps.as_slice()), g)).collect(),
        unresolved,
        report: Report::default(),
    };
    if let Some(Value::Object(theories)) = file.get_mut("proofs") {
        for (theory, goals) in theories.iter_mut() {
            if let Value::Object(goals) = goals {
                for (name, node) in goals.iter_mut() {
                    walk.node(theory, name, &mut Vec::new(), node);
                }
            }
        }
    }
    if let Value::Object(members) = &mut file {
        members.insert("dp-lock".to_string(), Value::String(format!("sha256:{}", lock_hash.to_hex())));
    }
    let mut out = serde_json::to_string_pretty(&file)?;
    out.push('\n');
    Ok((out, walk.report))
}

/// Rewrites one proof file with its extension members, atomically. A file
/// that why3find did not write is left alone.
pub fn rewrite(path: &Path, lock_hash: Hash, goals: &[GoalLabel], unresolved: &DpEntry) -> io::Result<Report> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Report::default()),
        Err(e) => return Err(e),
    };
    let (out, report) = extend(&text, lock_hash, goals, unresolved)
        .map_err(|e| io::Error::other(format!("{}: {e}", path.display())))?;
    store::write_atomic(path, out.as_bytes())?;
    Ok(report)
}

pub use dp_formats::proofjson::proof_file;

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(label: &str) -> DpEntry {
        DpEntry { label: label.into(), replay: vec![], imports_hash: Hash([0; 32]), certificate: None, conflict: false, invalid: vec![] }
    }

    fn goal(name: &str, steps: &[(&str, u32)], label: &str) -> GoalLabel {
        GoalLabel {
            theory: "T".into(),
            name: name.into(),
            steps: steps.iter().map(|(t, i)| (t.to_string(), *i)).collect(),
            entry: entry(label),
            proof: label == "proved-standalone",
        }
    }

    const FILE: &str = r#"{ "profile": [], "proofs": { "T": {
        "A": { "prover": "z3", "time": 0.5 },
        "B": null,
        "C": { "tactic": "split_vc", "children": [ { "prover": "z3", "time": 0.25 }, null ] },
        "D": { "prover": "z3", "time": 0.1 }
    } } }"#;

    fn label_of(node: &Value) -> String {
        DpEntry::from_member(node["dp"].as_str().unwrap(), 1 << 20).unwrap().label
    }

    // P-ID: P7.2.6
    #[test]
    fn labels_are_added_and_no_goal_becomes_proved() {
        let goals = [
            goal("A", &[], "proved-standalone"),
            goal("B", &[], "timeout"),
            goal("C", &[], "unknown"),
            goal("C", &[("split_vc", 0)], "proved-standalone"),
            goal("C", &[("split_vc", 1)], "unknown"),
            // D: why3find says proved, the Session has no proof label.
            goal("D", &[], "unchecked"),
        ];
        let (out, report) = extend(FILE, Hash([7; 32]), &goals, &entry("unresolved")).unwrap();
        let file: Value = serde_json::from_str(&out).unwrap();
        let t = &file["proofs"]["T"];
        assert_eq!(file["dp-lock"], format!("sha256:{}", Hash([7; 32]).to_hex()));
        // legacy members of proved Goals are untouched
        assert_eq!(t["A"]["prover"], "z3");
        assert_eq!(t["A"]["time"], 0.5);
        assert_eq!(label_of(&t["A"]), "proved-standalone");
        assert_eq!(label_of(&t["B"]), "timeout");
        assert!(t["B"].get("prover").is_none());
        assert_eq!(t["C"]["tactic"], "split_vc");
        assert!(t["C"].get("dp").is_none());
        assert_eq!(label_of(&t["C"]["children"][0]), "proved-standalone");
        assert_eq!(label_of(&t["C"]["children"][1]), "unknown");
        // withdrawn: no legacy reader takes D as proved
        assert_eq!(label_of(&t["D"]), "unchecked");
        assert!(t["D"].get("prover").is_none());
        assert_eq!(report.withdrawn, vec!["T.D ".to_string()]);
        assert_eq!(report.labels["proved-standalone"], 2);
        assert_eq!(report.labels["unknown"], 1);
        assert!(report.conflicts.is_empty());
        // Writing again changes nothing.
        let (again, _) = extend(&out, Hash([7; 32]), &goals, &entry("unresolved")).unwrap();
        assert_eq!(again, out);
    }

    // P-ID: P7.2.6
    #[test]
    fn a_goal_without_a_verdict_is_unresolved_and_not_proved() {
        let (out, report) = extend(FILE, Hash([7; 32]), &[], &entry("unresolved")).unwrap();
        assert!(!out.contains("\"prover\""));
        assert_eq!(report.labels["unresolved"], 5);
        assert_eq!(report.withdrawn.len(), 3);
    }

    #[test]
    fn proof_files_are_named_as_why3find_names_them() {
        let dir = Path::new("/p");
        assert_eq!(proof_file(dir, "arith.mlw"), Path::new("/p/why3find.proofs/arith.json"));
        assert_eq!(proof_file(dir, "./verif/a/b.coma"), Path::new("/p/why3find.proofs/verif.a.b.json"));
        assert_eq!(proof_file(dir, "/p/verif/a.coma"), Path::new("/p/why3find.proofs/verif.a.json"));
    }
}

#[cfg(test)]
mod conflict_tests {
    use super::*;
    use dp_formats::proofjson::{InvalidEntry, Place};

    // P-ID: P7.2.7
    #[test]
    fn a_conflict_is_written_and_reported_and_the_proof_stays() {
        let invalid = InvalidEntry {
            prover: "cvc5".into(),
            version: "1.4.1".into(),
            binary_hash: Hash([3; 32]),
            place: Place::Local,
        };
        let goal = GoalLabel {
            theory: "T".into(),
            name: "A".into(),
            steps: vec![],
            entry: DpEntry {
                label: "proved-standalone".into(),
                replay: vec![],
                imports_hash: Hash([0; 32]),
                certificate: None, conflict: true,
                invalid: vec![invalid.clone()],
            },
            proof: true,
        };
        let text = r#"{ "profile": [], "proofs": { "T": { "A": { "prover": "z3", "time": 0.5 } } } }"#;
        let unresolved =
            DpEntry { label: "unresolved".into(), replay: vec![], imports_hash: Hash([0; 32]), certificate: None, conflict: false, invalid: vec![] };
        let (out, report) = extend(text, Hash([7; 32]), &[goal], &unresolved).unwrap();
        assert_eq!(report.conflicts.len(), 1);
        assert!(report.conflicts[0].contains("cvc5 answered Invalid"), "{:?}", report.conflicts);
        let file: Value = serde_json::from_str(&out).unwrap();
        let node = &file["proofs"]["T"]["A"];
        assert_eq!(node["prover"], "z3");
        let entry = DpEntry::from_member(node["dp"].as_str().unwrap(), 1 << 20).unwrap();
        assert!(entry.conflict);
        assert_eq!(entry.invalid, vec![invalid]);
    }
}

#[cfg(test)]
mod winner_tests {
    use super::*;
    use dp_formats::proofjson::{Place, ReplayFields};

    // P-ID: P2.3.5
    #[test]
    fn the_legacy_member_names_the_prover_that_won() {
        let replay = |prover: &str| ReplayFields {
            task_hash: Hash([1; 32]),
            driver: vec![],
            prover: prover.into(),
            version: "1".into(),
            binary_hash: Hash([2; 32]),
            place: Place::Local,
            seed: None,
            velocity: None,
            local_micros: None,
            reference_micros: 1,
        };
        let goal = |name: &str, prover: &str| GoalLabel {
            theory: "T".into(),
            name: name.into(),
            steps: vec![],
            entry: DpEntry {
                label: "proved-standalone".into(),
                replay: vec![replay(prover)],
                imports_hash: Hash([0; 32]),
                certificate: None,
                conflict: false,
                invalid: vec![],
            },
            proof: true,
        };
        let text = r#"{ "profile": [], "proofs": { "T": {
            "A": { "prover": "alt-ergo", "time": 0.5 },
            "B": { "prover": "z3@4.12.4", "time": 0.25 } } } }"#;
        let unresolved = DpEntry {
            label: "unresolved".into(),
            replay: vec![],
            imports_hash: Hash([0; 32]),
            certificate: None,
            conflict: false,
            invalid: vec![],
        };
        let (out, _) = extend(text, Hash([7; 32]), &[goal("A", "z3"), goal("B", "z3")], &unresolved).unwrap();
        let file: Value = serde_json::from_str(&out).unwrap();
        // A was called with alt-ergo and proved by z3; B was proved by its
        // own prover and keeps its member as it was.
        assert_eq!(file["proofs"]["T"]["A"]["prover"], "z3");
        assert_eq!(file["proofs"]["T"]["A"]["time"], 0.5);
        assert_eq!(file["proofs"]["T"]["B"]["prover"], "z3@4.12.4");
    }
}
