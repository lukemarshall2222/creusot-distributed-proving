//! The mutation gate (R7.13, design C20).
//!
//! `cargo mutants` changes the code of the target files one place at a
//! time and runs the tests on each change. A change that no test notices
//! is a surviving mutant. The gate reads cargo-mutants' `outcomes.json`
//! and fails when
//!
//! - a mutant of a target function survived and no exclusion covers it;
//! - a target has no mutant at all in the outcomes (a renamed function, or
//!   a run that did not cover its file);
//! - the share of the targets' mutants that were caught is below the
//!   threshold;
//! - an exclusion matches no surviving mutant, gives no reason or names no
//!   artifact.
//!
//! `check_targets` is the static part, run in `ci/traceability`'s job:
//! the functions that the Soundness-critical rows of the Traceability_Table
//! name are targets.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Targets {
    pub packages: Vec<String>,
    pub threshold: f64,
    pub target: Vec<Target>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub file: String,
    /// Without it: every function of the file that is no logic function.
    pub function: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exclusions {
    #[serde(default)]
    pub exclusion: Vec<Exclusion>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exclusion {
    pub file: String,
    /// The line of the mutated place.
    pub line: u64,
    /// The mutant's description, as after the position in its name.
    pub mutant: String,
    pub reason: String,
    pub artifact: String,
}

/// One mutant of a run, as far as the gate reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Mutant {
    pub file: String,
    pub function: String,
    pub line: u64,
    /// The name without `file:line:column: `.
    pub description: String,
    /// `CaughtMutant`, `MissedMutant`, `Timeout` or `Unviable`.
    pub summary: String,
}

/// The mutants of an `outcomes.json` of cargo-mutants.
pub fn outcomes(json: &str) -> Result<Vec<Mutant>, String> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("outcomes.json: {e}"))?;
    let list = value["outcomes"].as_array().ok_or("outcomes.json: no outcomes")?;
    let mut out = Vec::new();
    for outcome in list {
        // The baseline run of the unchanged code is no mutant.
        let Some(mutant) = outcome["scenario"].get("Mutant") else { continue };
        let text = |v: &serde_json::Value| v.as_str().map(str::to_string);
        let (Some(file), Some(name), Some(summary)) = (text(&mutant["file"]), text(&mutant["name"]), text(&outcome["summary"]))
        else {
            return Err("outcomes.json: a mutant without file, name or summary".to_string());
        };
        let description = name.splitn(4, ": ").last().unwrap_or(&name).to_string();
        let line = mutant["span"]["start"]["line"].as_u64().unwrap_or(0);
        // `Outcome::rank_u8` and `<impl T for U>::f` are named by their last part.
        let function = text(&mutant["function"]["function_name"]).unwrap_or_default();
        let function = function.rsplit("::").next().unwrap_or_default().to_string();
        out.push(Mutant { file, function, line, description, summary });
    }
    Ok(out)
}

/// The names of the logic functions of a Rust source: functions with a
/// `#[logic...]` attribute.
pub fn logic_functions(source: &str) -> BTreeSet<String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = BTreeSet::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("pub fn ").or_else(|| trimmed.strip_prefix("fn ")) else { continue };
        let logic = lines[..i]
            .iter()
            .rev()
            .take_while(|l| {
                let l = l.trim_start();
                l.starts_with('#') || l.starts_with("//") || (!l.is_empty() && !l.contains(['{', '}', ';']))
            })
            .any(|l| l.trim_start().starts_with("#[logic"));
        if logic {
            out.insert(rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect());
        }
    }
    out
}

/// Whether a mutant is one of a target. `logic` maps a file to its logic
/// functions.
fn targeted(m: &Mutant, targets: &Targets, logic: &BTreeMap<String, BTreeSet<String>>) -> bool {
    targets.target.iter().any(|t| {
        t.file == m.file
            && match &t.function {
                Some(f) => f == &m.function,
                None => !logic.get(&m.file).is_some_and(|l| l.contains(&m.function)),
            }
    })
}

/// What a run showed for the targets.
#[derive(Debug, Default, PartialEq)]
pub struct Verdict {
    pub caught: usize,
    pub missed: usize,
    pub excluded: usize,
    pub problems: Vec<String>,
}

/// Judges the mutants of a run.
pub fn judge(
    mutants: &[Mutant],
    targets: &Targets,
    exclusions: &Exclusions,
    logic: &BTreeMap<String, BTreeSet<String>>,
) -> Verdict {
    let mut v = Verdict::default();
    let mut used = vec![false; exclusions.exclusion.len()];
    for (i, e) in exclusions.exclusion.iter().enumerate() {
        if e.reason.trim().is_empty() || e.artifact.trim().is_empty() {
            v.problems.push(format!("exclusion {i} ({}): no reason or no artifact", e.mutant));
        }
    }
    for t in &targets.target {
        let any = mutants.iter().any(|m| m.file == t.file && t.function.as_ref().is_none_or(|f| f == &m.function));
        if !any {
            v.problems.push(format!(
                "target {}{}: the run has no mutant of it",
                t.file,
                t.function.as_ref().map_or(String::new(), |f| format!(" `{f}`"))
            ));
        }
    }
    for m in mutants.iter().filter(|m| targeted(m, targets, logic)) {
        match m.summary.as_str() {
            // A mutant that makes the tests hang was noticed.
            "CaughtMutant" | "Timeout" => v.caught += 1,
            "MissedMutant" => {
                match exclusions.exclusion.iter().position(|e| e.file == m.file && e.line == m.line && e.mutant == m.description) {
                    Some(i) => {
                        used[i] = true;
                        v.excluded += 1;
                    }
                    None => {
                        v.missed += 1;
                        v.problems.push(format!("{}:{}: the tests do not catch: {}", m.file, m.line, m.description));
                    }
                }
            }
            // A mutant that does not compile shows nothing.
            _ => {}
        }
    }
    for (e, used) in exclusions.exclusion.iter().zip(&used) {
        if !used {
            v.problems.push(format!("exclusion `{}` at {}:{}: no surviving mutant matches it", e.mutant, e.file, e.line));
        }
    }
    let judged = v.caught + v.missed;
    if judged > 0 && (v.caught as f64) < targets.threshold * judged as f64 {
        v.problems.push(format!(
            "{} of {judged} mutants of the targets were caught; the threshold is {:.0} %",
            v.caught,
            targets.threshold * 100.0
        ));
    }
    v
}

/// The static check: the files of the targets exist, a named function is
/// in its file, and every function that a Soundness-critical row of the
/// Traceability_Table names, and that the target crates define, is a
/// target.
pub fn check_targets(root: &Path, targets: &Targets) -> Result<Vec<String>, String> {
    let mut problems = Vec::new();
    let mut defined: BTreeMap<String, String> = BTreeMap::new();
    for t in &targets.target {
        let Ok(source) = fs::read_to_string(root.join(&t.file)) else {
            problems.push(format!("target {}: no such file", t.file));
            continue;
        };
        if let Some(f) = &t.function {
            if !source.contains(&format!("fn {f}(")) {
                problems.push(format!("target {} `{f}`: no such function", t.file));
            }
        }
        defined.insert(t.file.clone(), source);
    }
    if !(0.0..=1.0).contains(&targets.threshold) {
        problems.push(format!("threshold {}: not between 0 and 1", targets.threshold));
    }
    #[derive(Deserialize)]
    struct Table {
        row: Vec<Row>,
    }
    #[derive(Deserialize)]
    struct Row {
        req: String,
        class: String,
        functions: Vec<String>,
    }
    let table = fs::read_to_string(root.join("docs/traceability.toml")).map_err(|e| format!("docs/traceability.toml: {e}"))?;
    let table: Table = toml::from_str(&table).map_err(|e| format!("docs/traceability.toml: {e}"))?;
    // Every source file of the target packages: a function of a row that
    // one of them defines must be a target.
    let mut sources: Vec<(String, String)> = Vec::new();
    for package in &targets.packages {
        let dir = format!("crates/{package}/src");
        for entry in fs::read_dir(root.join(&dir)).map_err(|e| format!("{dir}: {e}"))?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Ok(source) = fs::read_to_string(entry.path()) {
                sources.push((format!("{dir}/{name}"), source));
            }
        }
    }
    for row in table.row.iter().filter(|r| r.class == "Soundness-critical") {
        for f in &row.functions {
            for (file, source) in sources.iter().filter(|(_, s)| s.contains(&format!("pub fn {f}("))) {
                let logic = logic_functions(source);
                let is_target = targets.target.iter().any(|t| {
                    &t.file == file && t.function.as_ref().map_or(!logic.contains(f), |named| named == f)
                });
                if !is_target {
                    problems.push(format!("{}: its function `{f}` ({file}) is no target", row.req));
                }
            }
        }
    }
    Ok(problems)
}

pub fn read_targets(root: &Path) -> Result<(Targets, Exclusions), String> {
    let read = |name: &str| {
        fs::read_to_string(root.join("tools/mutants").join(name)).map_err(|e| format!("tools/mutants/{name}: {e}"))
    };
    let targets: Targets = toml::from_str(&read("targets.toml")?).map_err(|e| format!("targets.toml: {e}"))?;
    let exclusions: Exclusions = toml::from_str(&read("exclusions.toml")?).map_err(|e| format!("exclusions.toml: {e}"))?;
    Ok((targets, exclusions))
}

/// The logic functions of each target file.
pub fn logic_of(root: &Path, targets: &Targets) -> BTreeMap<String, BTreeSet<String>> {
    targets
        .target
        .iter()
        .filter_map(|t| fs::read_to_string(root.join(&t.file)).ok().map(|s| (t.file.clone(), logic_functions(&s))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Feature: creusot-distributed-proving, Property 37: Every requirement is mechanically verified and the tests catch bugs
    // P-ID: P7.13.1 to P7.13.4 (the gate fails on a surviving mutant of a target, on a stale exclusion and below the threshold)

    fn targets() -> Targets {
        toml::from_str(
            "packages = [\"core\"]\nthreshold = 1.0\n[[target]]\nfile = \"src/verdict.rs\"\n[[target]]\nfile = \"src/frontend.rs\"\nfunction = \"decide\"\n",
        )
        .unwrap()
    }

    fn logic() -> BTreeMap<String, BTreeSet<String>> {
        [("src/verdict.rs".to_string(), ["standalone".to_string()].into())].into()
    }

    fn run(list: &[(&str, &str, &str, &str)]) -> String {
        let outcomes: Vec<String> = std::iter::once(r#"{"scenario":"Baseline","summary":"Success"}"#.to_string())
            .chain(list.iter().map(|(file, function, description, summary)| {
                format!(
                    r#"{{"scenario":{{"Mutant":{{"name":"{file}:10:5: {description}","file":"{file}","function":{{"function_name":"{function}"}},"span":{{"start":{{"line":10,"column":5}}}}}}}},"summary":"{summary}"}}"#
                )
            }))
            .collect();
        format!(r#"{{"outcomes":[{}]}}"#, outcomes.join(","))
    }

    const GOOD: [(&str, &str, &str, &str); 5] = [
        ("src/verdict.rs", "label", "replace == with != in label", "CaughtMutant"),
        ("src/verdict.rs", "label", "replace label -> Label with Default::default()", "Unviable"),
        ("src/verdict.rs", "standalone", "replace standalone -> bool with true", "MissedMutant"),
        ("src/frontend.rs", "decide", "replace && with || in decide", "Timeout"),
        ("src/frontend.rs", "step", "delete match arm in step", "MissedMutant"),
    ];

    #[test]
    fn caught_mutants_logic_functions_and_other_functions_pass() {
        let mutants = outcomes(&run(&GOOD)).unwrap();
        assert_eq!(mutants.len(), 5);
        assert_eq!(mutants[0].description, "replace == with != in label");
        let v = judge(&mutants, &targets(), &Exclusions::default(), &logic());
        // The missed mutants are of a logic function and of a function
        // that is no target.
        assert_eq!(v, Verdict { caught: 2, missed: 0, excluded: 0, problems: vec![] });
    }

    #[test]
    fn a_surviving_mutant_of_a_target_fails_unless_it_is_excluded() {
        let mut list = GOOD.to_vec();
        list.push(("src/verdict.rs", "Outcome::rank_u8", "replace < with <= in position", "MissedMutant"));
        let mutants = outcomes(&run(&list)).unwrap();
        let v = judge(&mutants, &targets(), &Exclusions::default(), &logic());
        assert_eq!(v.missed, 1);
        assert!(v.problems.iter().any(|p| p.contains("src/verdict.rs:10: the tests do not catch: replace < with <= in position")), "{v:?}");
        assert!(v.problems.iter().any(|p| p.contains("2 of 3 mutants")), "{v:?}");
        // With an exclusion that names its artifact, it passes.
        let exclusion_at = |line: u64, artifact: &str| Exclusions {
            exclusion: vec![Exclusion {
                file: "src/verdict.rs".into(),
                line,
                mutant: "replace < with <= in position".into(),
                reason: "equivalent".into(),
                artifact: artifact.into(),
            }],
        };
        let exclusion = |artifact: &str| exclusion_at(10, artifact);
        let v = judge(&mutants, &targets(), &exclusion("proof X"), &logic());
        assert_eq!((v.missed, v.excluded, v.problems.len()), (0, 1, 0), "{v:?}");
        // The same change at another line is another mutant.
        let v = judge(&mutants, &targets(), &exclusion_at(11, "proof X"), &logic());
        assert_eq!((v.missed, v.excluded), (1, 0), "{v:?}");
        // Without an artifact, or when the mutant is caught, the exclusion fails.
        assert!(!judge(&mutants, &targets(), &exclusion(" "), &logic()).problems.is_empty());
        let caught = outcomes(&run(&GOOD)).unwrap();
        let v = judge(&caught, &targets(), &exclusion("proof X"), &logic());
        assert!(v.problems.iter().any(|p| p.contains("no surviving mutant matches it")), "{v:?}");
    }

    #[test]
    fn a_target_without_mutants_and_a_low_score_fail() {
        let only_verdict = outcomes(&run(&GOOD[..3])).unwrap();
        let v = judge(&only_verdict, &targets(), &Exclusions::default(), &logic());
        assert!(v.problems.iter().any(|p| p.contains("target src/frontend.rs `decide`: the run has no mutant")), "{v:?}");
        let mut lenient = targets();
        lenient.threshold = 0.5;
        let mut list = GOOD.to_vec();
        list.push(("src/verdict.rs", "label", "delete ! in label", "MissedMutant"));
        list.push(("src/verdict.rs", "label", "replace && with || in label", "MissedMutant"));
        let v = judge(&outcomes(&run(&list)).unwrap(), &lenient, &Exclusions::default(), &logic());
        // 2 caught of 4: at the threshold; the survivors still fail.
        assert_eq!((v.caught, v.missed), (2, 2));
        assert!(!v.problems.iter().any(|p| p.contains("threshold")), "{v:?}");
        lenient.threshold = 0.75;
        let v = judge(&outcomes(&run(&list)).unwrap(), &lenient, &Exclusions::default(), &logic());
        assert!(v.problems.iter().any(|p| p.contains("2 of 4 mutants")), "{v:?}");
    }

    #[test]
    fn logic_functions_are_found_by_their_attribute() {
        let source = "#[logic(open)]\npub fn spec(x: Int) -> bool {\n    true\n}\n\n/// Doc.\n#[ensures(result == spec(x@))]\npub fn run(x: u64) -> bool {\n    true\n}\n\nimpl T {\n    #[logic]\n    #[ensures(true)]\n    fn lemma() {}\n}\n";
        assert_eq!(logic_functions(source), ["lemma".to_string(), "spec".to_string()].into());
    }

    #[test]
    fn the_targets_of_the_repository_are_consistent() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let (targets, _) = read_targets(&root).unwrap();
        let problems = check_targets(&root, &targets).unwrap();
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }
}
