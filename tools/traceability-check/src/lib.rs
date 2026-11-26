//! `ci/traceability` (R7.11, design C20): the Traceability_Table
//! `docs/traceability.toml`.
//!
//! The table has one row per requirement: its class, its Property IDs, its
//! decision functions, and the artifacts and CI jobs that its Verification
//! lines name. Each row has a scope status, written by hand: `deferred`,
//! `in scope` or `dropped`. The check fails when
//!
//! - a requirement has no row, or a row has no requirement;
//! - a derived field of a row is not what `requirements.md` says;
//! - a row in scope names an artifact that does not exist, or a CI job
//!   that the workflow does not run;
//! - a deferred row names only artifacts that exist: it must be switched
//!   to in scope by the commit that adds the last of them (criterion 12).
//!
//! A missing artifact of a deferred row alone is no failure. An artifact
//! exists when it is a tracked file, or a directory that holds one.
//!
//! Not checked yet: the outcomes of the CI jobs on the commit (criterion
//! 3), the format registry and the mutation targets.
//!
//! `traceability-check --write` rewrites the derived fields and keeps the
//! scope statuses; a new row starts deferred.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Table {
    #[serde(default)]
    pub row: Vec<Row>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub req: String,
    pub class: String,
    /// `deferred`, `in scope` or `dropped`. Written by hand.
    pub scope: String,
    pub properties: Vec<String>,
    pub functions: Vec<String>,
    pub artifacts: Vec<String>,
    pub jobs: Vec<String>,
}

pub const SCOPES: [&str; 3] = ["deferred", "in scope", "dropped"];

/// The texts between backticks of a line.
fn quoted(line: &str) -> Vec<&str> {
    line.split('`').skip(1).step_by(2).collect()
}

fn is_job(token: &str) -> bool {
    (token.starts_with("ci/") || token.starts_with("sched/")) && !token.contains(' ') && !token.ends_with(".sh")
}

/// Whether a quoted text of a Verification line is the path of an artifact.
fn is_artifact(token: &str) -> bool {
    const EXTENSIONS: [&str; 12] = ["rs", "tla", "lean", "toml", "py", "md", "mlw", "cfg", "sh", "json", "yml", "abnf"];
    // The script of a job is no artifact of a requirement.
    !is_job(token)
        && !token.starts_with("ci/")
        && token.contains('/')
        && !token.contains([' ', '(', '<', '*', '='])
        && (token.ends_with('/') || token.rsplit_once('.').is_some_and(|(_, e)| EXTENSIONS.contains(&e)))
}

/// The rows that `requirements.md` gives: one per requirement section,
/// with the class and the functions of the Initial Traceability Table.
pub fn rows_of(requirements: &str) -> Vec<Row> {
    // `| R1.3 | Class | Scope | Properties | Functions | Artifacts | CI jobs |`
    let mut table: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    let mut in_table = false;
    for line in requirements.lines() {
        if line.starts_with("## ") {
            in_table = line.trim() == "## Initial Traceability Table";
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if in_table && cells.len() >= 8 && cells[1].starts_with('R') && cells[1][1..].starts_with(|c: char| c.is_ascii_digit()) {
            table.insert(cells[1].to_string(), (cells[2].to_string(), quoted(cells[5]).iter().map(|f| f.to_string()).collect()));
        }
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut section = "";
    for line in requirements.lines() {
        if let Some(heading) = line.strip_prefix("### Requirement ") {
            // `### Requirement 3: R1.3 Result binding and authentication`
            if let Some(req) = heading.split_once(": ").and_then(|(_, r)| r.split_whitespace().next()) {
                let (class, functions) = table.get(req).cloned().unwrap_or_default();
                rows.push(Row {
                    req: req.to_string(),
                    class,
                    scope: "deferred".to_string(),
                    properties: Vec::new(),
                    functions,
                    artifacts: Vec::new(),
                    jobs: Vec::new(),
                });
            }
            section = "";
        } else if line.starts_with("## ") {
            // Past the requirements: nothing below belongs to the last one.
            if !rows.is_empty() && line.trim() != "## Requirements" {
                section = "outside";
            }
        } else if let Some(name) = line.strip_prefix("#### ") {
            if section != "outside" {
                section = match name.trim() {
                    "Properties" => "properties",
                    "Verification" => "verification",
                    _ => "",
                };
            }
        } else if let Some(row) = rows.last_mut() {
            match section {
                // `- **P1.3.1** ...`
                "properties" => {
                    if let Some(id) = line.strip_prefix("- **P").and_then(|r| r.split_once("**")).map(|(id, _)| id) {
                        let id: String = format!("P{id}").chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '.').collect();
                        if !row.properties.contains(&id) {
                            row.properties.push(id);
                        }
                    }
                }
                "verification" if line.starts_with("- ") => {
                    for token in quoted(line) {
                        let list = if is_job(token) {
                            &mut row.jobs
                        } else if is_artifact(token) {
                            &mut row.artifacts
                        } else {
                            continue;
                        };
                        if !list.iter().any(|t| t == token) {
                            list.push(token.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    rows
}

/// Whether an artifact path exists on the commit: a tracked file, or a
/// directory that holds a tracked file.
pub fn exists(artifact: &str, tracked: &BTreeSet<String>) -> bool {
    match artifact.strip_suffix('/') {
        Some(dir) => {
            let prefix = format!("{dir}/");
            tracked.range(prefix.clone()..).next().is_some_and(|f| f.starts_with(&prefix))
        }
        None => tracked.contains(artifact),
    }
}

/// Every disagreement of the table with the requirements, the tracked
/// files and the CI jobs of the workflow.
pub fn check(table: &Table, requirements: &str, tracked: &BTreeSet<String>, jobs: &BTreeSet<String>) -> Vec<String> {
    let mut problems = Vec::new();
    let wanted = rows_of(requirements);
    let mut seen = BTreeSet::new();
    for row in &table.row {
        if !seen.insert(row.req.as_str()) {
            problems.push(format!("{}: two rows", row.req));
        }
        let Some(w) = wanted.iter().find(|w| w.req == row.req) else {
            problems.push(format!("{}: a row without a requirement", row.req));
            continue;
        };
        if !SCOPES.contains(&row.scope.as_str()) {
            problems.push(format!("{}: unknown scope status {:?}", row.req, row.scope));
        }
        if row.class != w.class || row.class.is_empty() {
            problems.push(format!("{}: class {:?}, the Initial Traceability Table has {:?}", row.req, row.class, w.class));
        }
        for (what, ours, theirs) in [
            ("properties", &row.properties, &w.properties),
            ("functions", &row.functions, &w.functions),
            ("artifacts", &row.artifacts, &w.artifacts),
            ("jobs", &row.jobs, &w.jobs),
        ] {
            if ours != theirs {
                let missing: Vec<_> = theirs.iter().filter(|t| !ours.contains(t)).collect();
                let extra: Vec<_> = ours.iter().filter(|t| !theirs.contains(t)).collect();
                problems.push(format!(
                    "{}: {what} differ from requirements.md (missing {missing:?}, not there {extra:?}, or another order)",
                    row.req
                ));
            }
        }
        let missing: Vec<&String> = row.artifacts.iter().filter(|a| !exists(a, tracked)).collect();
        match row.scope.as_str() {
            "in scope" => {
                if row.artifacts.is_empty() || row.properties.is_empty() {
                    problems.push(format!("{}: in scope without artifacts or without Properties", row.req));
                }
                for a in &missing {
                    problems.push(format!("{}: in scope, and its artifact {a} does not exist", row.req));
                }
                for j in row.jobs.iter().filter(|j| !jobs.contains(*j)) {
                    problems.push(format!("{}: in scope, and the workflow runs no job {j}", row.req));
                }
            }
            "deferred" if missing.is_empty() && !row.artifacts.is_empty() => problems.push(format!(
                "{}: deferred, but every artifact that its Verification lines name exists: switch the row to in scope",
                row.req
            )),
            _ => {}
        }
    }
    for w in &wanted {
        if !seen.contains(w.req.as_str()) {
            problems.push(format!("{}: a requirement without a row", w.req));
        }
    }
    problems
}

/// The table with its derived fields as the requirements give them.
pub fn regenerate(table: &Table, requirements: &str) -> Table {
    let mut rows = rows_of(requirements);
    for row in &mut rows {
        if let Some(old) = table.row.iter().find(|old| old.req == row.req) {
            row.scope = old.scope.clone();
        }
    }
    Table { row: rows }
}

const HEADER: &str = "# The Traceability_Table (R7.11). Every field but `scope` is derived from\n\
# requirements.md by `cargo run -p traceability-check -- --write`; `scope`\n\
# is written by hand, and switched from deferred to in scope by the commit\n\
# that adds the last artifact of the row. `ci/traceability` checks both.\n\n";

pub fn print(table: &Table) -> String {
    format!("{HEADER}{}", toml::to_string_pretty(table).expect("the table is plain data"))
}

/// `docs/traceability.md`: per row, what exists of what it names.
pub fn render(table: &Table, tracked: &BTreeSet<String>) -> String {
    let mut out = String::from(
        "# Traceability\n\nGenerated from `docs/traceability.toml` by `traceability-check --write`; do not edit.\n\n\
         | Requirement | Class | Scope | Properties | Artifacts that exist | Missing artifacts |\n|---|---|---|---|---|---|\n",
    );
    for row in &table.row {
        let missing: Vec<&str> = row.artifacts.iter().filter(|a| !exists(a, tracked)).map(String::as_str).collect();
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} of {} | {} |\n",
            row.req,
            row.class,
            row.scope,
            row.properties.len(),
            row.artifacts.len() - missing.len(),
            row.artifacts.len(),
            missing.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(", ")
        ));
    }
    out
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The files that git tracks, and those that are staged to be.
fn tracked_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let out = Command::new("git").args(["ls-files", "--cached"]).current_dir(root).output().map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git ls-files: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect())
}

/// The CI jobs that the workflow runs: its steps named `ci/<job>`.
pub fn workflow_jobs(workflow: &str) -> BTreeSet<String> {
    workflow
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- name: "))
        .flat_map(|names| names.split(',').map(|n| n.trim().to_string()).collect::<Vec<_>>())
        .filter(|n| is_job(n))
        .collect()
}

/// Checks the repository at `root`, or with `write` rewrites the derived
/// fields first.
pub fn run(root: &Path, write: bool) -> Result<Vec<String>, String> {
    let table_path = root.join("docs/traceability.toml");
    let rendered_path = root.join("docs/traceability.md");
    let requirements = read(&root.join(".kiro/specs/creusot-distributed-proving/requirements.md"))?;
    let tracked = tracked_files(root)?;
    // The jobs of every workflow: the gating one and the scheduled one.
    let mut jobs = BTreeSet::new();
    let workflows = root.join(".github/workflows");
    for entry in fs::read_dir(&workflows).map_err(|e| format!("{}: {e}", workflows.display()))?.flatten() {
        jobs.extend(workflow_jobs(&read(&entry.path())?));
    }
    let mut table: Table = match read(&table_path) {
        Ok(text) => toml::from_str(&text).map_err(|e| format!("docs/traceability.toml: {e}"))?,
        Err(_) if write => Table::default(),
        Err(e) => return Err(e),
    };
    if write {
        table = regenerate(&table, &requirements);
        fs::write(&table_path, print(&table)).map_err(|e| e.to_string())?;
        fs::write(&rendered_path, render(&table, &tracked)).map_err(|e| e.to_string())?;
    }
    let mut problems = check(&table, &requirements, &tracked, &jobs);
    if read(&rendered_path).ok().as_deref() != Some(render(&table, &tracked).as_str()) {
        problems.push("docs/traceability.md is not current (run traceability-check --write)".to_string());
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Feature: creusot-distributed-proving, Property 37: Every requirement is mechanically verified and the tests catch bugs
    // P-ID: P7.11.6, P7.11.11 (seeded changes of the table, the requirements and the files fail the check)

    const REQUIREMENTS: &str = "## Requirements\n\n\
### Requirement 1: R1.1 Distribution\n\n#### Properties\n\n- **P1.1.1** One.\n- **P1.1.2** (Named) Two.\n\n\
#### Verification\n\n- A1 — contracts on `assign` — `crates/core/src/coord.rs` — `ci/creusot`.\n\
- A4 — simulation — `tests/sim/` — `ci/sim`, `sched/sim-nightly`; run by `ci/jobs/test.sh`.\n\n\
#### Trust\n\n- Nothing, see `crates/other/x.rs`.\n\n\
### Requirement 2: R1.2 Escalation\n\n#### Properties\n\n- **P1.2.1** One.\n\n\
#### Verification\n\n- A4 — a test — `tests/escalation.rs` — `ci/test`.\n\n\
## Initial Traceability Table\n\n| Req | Class | Scope | Properties | Functions | Artifacts (level) | CI jobs |\n|---|---|---|---|---|---|---|\n\
| R1.1 | Soundness-critical | deferred, stage 3 | P1.1.1–P1.1.2 | `assign`, `pick` | `coord.rs` (A1) | `ci/creusot` |\n\
| R1.2 | Performance | deferred, stage 8 | P1.2.1 | — | `escalation.rs` (A4) | `ci/test` |\n";

    fn files(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|f| f.to_string()).collect()
    }

    fn jobs() -> BTreeSet<String> {
        workflow_jobs("    steps:\n      - name: ci/creusot\n      - name: ci/sim, ci/test\n      - name: build the shell\n")
    }

    fn table(scopes: [&str; 2]) -> Table {
        let mut t = regenerate(&Table::default(), REQUIREMENTS);
        t.row[0].scope = scopes[0].to_string();
        t.row[1].scope = scopes[1].to_string();
        t
    }

    #[test]
    fn rows_are_read_from_the_requirements() {
        let t = table(["deferred", "deferred"]);
        assert_eq!(t.row.len(), 2);
        let r = &t.row[0];
        assert_eq!((r.req.as_str(), r.class.as_str()), ("R1.1", "Soundness-critical"));
        assert_eq!(r.properties, ["P1.1.1", "P1.1.2"]);
        assert_eq!(r.functions, ["assign", "pick"]);
        // Paths of the Verification lines only; the script of a job and
        // the path in the Trust subsection are no artifacts.
        assert_eq!(r.artifacts, ["crates/core/src/coord.rs", "tests/sim/"]);
        assert_eq!(r.jobs, ["ci/creusot", "ci/sim", "sched/sim-nightly"]);
        assert_eq!(toml::from_str::<Table>(&print(&t)).unwrap(), t);
    }

    #[test]
    fn scope_decides_what_a_missing_artifact_means() {
        let all = files(&["crates/core/src/coord.rs", "tests/sim/faults.rs", "tests/escalation.rs"]);
        let some = files(&["crates/core/src/coord.rs"]);
        let run = |t: &Table, tracked: &BTreeSet<String>| check(t, REQUIREMENTS, tracked, &jobs());
        // A missing artifact of a deferred row alone is no failure.
        assert!(run(&table(["deferred", "deferred"]), &some).is_empty());
        // It is one for a row in scope.
        let problems = run(&table(["in scope", "deferred"]), &some);
        assert!(problems.iter().any(|p| p.contains("R1.1: in scope, and its artifact tests/sim/ does not exist")), "{problems:?}");
        // A deferred row whose artifacts all exist must be switched.
        let problems = run(&table(["deferred", "deferred"]), &all);
        assert!(problems.iter().any(|p| p.starts_with("R1.2: deferred, but every artifact")), "{problems:?}");
        assert!(problems.iter().any(|p| p.starts_with("R1.1: deferred, but every artifact")), "{problems:?}");
        // Switched, R1.2 passes; R1.1 names a job that the workflow lacks.
        let problems = run(&table(["in scope", "in scope"]), &all);
        assert_eq!(problems, ["R1.1: in scope, and the workflow runs no job sched/sim-nightly"]);
        // A directory exists only when it holds a tracked file.
        assert!(!exists("tests/sim/", &files(&["tests/simulator.rs", "tests/sim"])));
        assert!(exists("tests/sim/", &files(&["tests/sim/lib.rs"])));
        // A dropped row is not held to anything.
        assert!(run(&table(["dropped", "deferred"]), &some).is_empty());
    }

    #[test]
    fn every_seeded_change_of_the_table_is_found() {
        let tracked = files(&["crates/core/src/coord.rs"]);
        let found = |change: &dyn Fn(&mut Table), requirements: &str, expected: &str| {
            let mut t = table(["deferred", "deferred"]);
            change(&mut t);
            let problems = check(&t, requirements, &tracked, &jobs());
            assert!(problems.iter().any(|p| p.contains(expected)), "{expected:?} not in {problems:?}");
        };
        found(&|t| t.row.truncate(1), REQUIREMENTS, "R1.2: a requirement without a row");
        found(&|t| t.row[1].req = "R9.9".into(), REQUIREMENTS, "R9.9: a row without a requirement");
        found(&|t| t.row.push(t.row[0].clone()), REQUIREMENTS, "R1.1: two rows");
        found(&|t| t.row[0].scope = "soon".into(), REQUIREMENTS, "R1.1: unknown scope status");
        found(&|t| t.row[0].class = "Functional".into(), REQUIREMENTS, "R1.1: class");
        found(&|t| t.row[0].properties.truncate(1), REQUIREMENTS, "R1.1: properties differ");
        found(&|t| t.row[0].functions.clear(), REQUIREMENTS, "R1.1: functions differ");
        found(&|t| t.row[0].artifacts.truncate(1), REQUIREMENTS, "R1.1: artifacts differ");
        found(&|t| t.row[0].jobs.push("ci/extra".into()), REQUIREMENTS, "R1.1: jobs differ");
        // The requirements change and the table does not.
        let more = REQUIREMENTS.replace("- **P1.2.1** One.", "- **P1.2.1** One.\n- **P1.2.2** Two.");
        found(&|_| {}, &more, "R1.2: properties differ");
        let renamed = REQUIREMENTS.replace("`tests/escalation.rs`", "`tests/escalate.rs`");
        found(&|_| {}, &renamed, "R1.2: artifacts differ");
    }

    #[test]
    fn the_repository_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let problems = run(&root, false).unwrap();
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }
}
