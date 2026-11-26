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
//!   to in scope by the commit that adds the last of them (criterion 12);
//! - a Verification line of a row in scope lacks an assurance level, an
//!   artifact path or a CI job (criterion 6), or the lines of the row lack
//!   the levels that its class needs (criterion 4);
//! - an exchanged format that a requirement in scope is cited for has no
//!   entry in the format registry, or an entry names something that does
//!   not exist (R7.10 criterion 7; `formats.rs`);
//! - the workflows do not define exactly the CI job list, `ci/traceability`
//!   does not come after every other `ci/` job, or a `sched/` job declares
//!   no input paths (criteria 9 and 10).
//!
//! A missing artifact of a deferred row alone is no failure. An artifact
//! exists when it is a tracked file, or a directory that holds one.
//!
//! Not checked yet: the outcomes of the CI jobs on the commit (criterion
//! 3) and with them the pending report (criterion 8), and the mutation
//! targets.
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
    /// The trust items that the Trust subsection cites (TB, AS, NG, F).
    #[serde(default)]
    pub trust: Vec<String>,
    /// The Verification lines: the assurance levels of each with its
    /// artifact paths and CI jobs. The triples of a row (criterion 7) are
    /// the combinations within each line.
    #[serde(default)]
    pub verification: Vec<Triples>,
}

/// One Verification line as the table holds it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Triples {
    pub levels: Vec<String>,
    pub artifacts: Vec<String>,
    pub jobs: Vec<String>,
}

pub mod formats;

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
                    trust: Vec::new(),
                    verification: Vec::new(),
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
                    "Trust" => "trust",
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
                "trust" => {
                    for item in trust_items(line) {
                        if !row.trust.contains(&item) {
                            row.trust.push(item);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let lines = lines_of(requirements);
    for row in &mut rows {
        row.verification = lines
            .get(&row.req)
            .map(|lines| {
                lines
                    .iter()
                    .map(|l| Triples { levels: l.levels.clone(), artifacts: l.artifacts.clone(), jobs: l.jobs.clone() })
                    .collect()
            })
            .unwrap_or_default();
    }
    rows
}

/// The trust items that a line names: `TB-1`, `AS-12`, `NG-3`, `F2`.
pub fn trust_items(line: &str) -> Vec<String> {
    let mut items = Vec::new();
    for word in line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
        let digits = |rest: &str| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit());
        let item = ["TB-", "AS-", "NG-", "F"].iter().any(|prefix| word.strip_prefix(prefix).is_some_and(digits));
        if item && !items.iter().any(|i| i == word) {
            items.push(word.to_string());
        }
    }
    items
}

/// One Verification line of a requirement: its assurance levels, artifact
/// paths and CI jobs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub levels: Vec<String>,
    pub artifacts: Vec<String>,
    pub jobs: Vec<String>,
    pub text: String,
}

/// The Verification lines of every requirement of `requirements.md`.
pub fn lines_of(requirements: &str) -> BTreeMap<String, Vec<Line>> {
    let mut lines: BTreeMap<String, Vec<Line>> = BTreeMap::new();
    let mut req: Option<String> = None;
    let mut verification = false;
    for line in requirements.lines() {
        if let Some(heading) = line.strip_prefix("### Requirement ") {
            req = heading.split_once(": ").and_then(|(_, r)| r.split_whitespace().next()).map(str::to_string);
            verification = false;
        } else if line.starts_with("## ") {
            if req.is_some() && line.trim() != "## Requirements" {
                req = None;
            }
        } else if let Some(name) = line.strip_prefix("#### ") {
            verification = name.trim() == "Verification";
        } else if let (Some(req), true, true) = (&req, verification, line.starts_with("- ")) {
            // The levels are the words A1 to A5 outside the quoted texts.
            let plain: String = line.split('`').step_by(2).collect::<Vec<_>>().join(" ");
            let mut levels: Vec<String> = Vec::new();
            for word in plain.split(|c: char| !c.is_ascii_alphanumeric()) {
                if matches!(word, "A1" | "A2" | "A3" | "A4" | "A5") && !levels.iter().any(|l| l == word) {
                    levels.push(word.to_string());
                }
            }
            let tokens = quoted(line);
            lines.entry(req.clone()).or_default().push(Line {
                levels,
                artifacts: tokens.iter().filter(|t| is_artifact(t)).map(|t| t.to_string()).collect(),
                jobs: tokens.iter().filter(|t| is_job(t)).map(|t| t.to_string()).collect(),
                text: line.to_string(),
            });
        }
    }
    lines
}

/// R7.11 criteria 4 and 6 for one row in scope: every Verification line
/// has a level, an artifact path and a CI job, and the lines together have
/// the levels that the class of the requirement needs.
fn check_lines(row: &Row, lines: &[Line]) -> Vec<String> {
    let mut problems = Vec::new();
    for line in lines {
        let shown: String = line.text.chars().take(60).collect();
        for (what, empty) in [
            ("an assurance level", line.levels.is_empty()),
            ("an artifact path", line.artifacts.is_empty()),
            ("a CI job", line.jobs.is_empty()),
        ] {
            if empty {
                problems.push(format!("{}: in scope, and a Verification line lacks {what}: {shown:?}", row.req));
            }
        }
    }
    let has = |levels: &[&str]| lines.iter().any(|l| l.levels.iter().any(|level| levels.contains(&level.as_str())));
    match row.class.as_str() {
        "Soundness-critical" => {
            if !has(&["A1", "A3"]) {
                problems.push(format!("{}: Soundness-critical and in scope without an A1 or A3 artifact", row.req));
            }
            if !has(&["A4"]) {
                problems.push(format!("{}: Soundness-critical and in scope without an A4 artifact", row.req));
            }
        }
        "Liveness" | "Robustness" | "Performance" => {
            if !has(&["A2", "A4", "A5"]) {
                problems.push(format!("{}: {} and in scope without an A2, A4 or A5 artifact", row.req, row.class));
            }
        }
        _ => {}
    }
    problems
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
    let lines = lines_of(requirements);
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
            ("trust items", &row.trust, &w.trust),
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
        if row.verification != w.verification {
            problems.push(format!(
                "{}: the levels, artifacts and jobs of its Verification lines differ from requirements.md",
                row.req
            ));
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
                problems.extend(check_lines(row, lines.get(&row.req).map(Vec::as_slice).unwrap_or_default()));
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

/// The CI job list of `requirements.md` (R7.11): the jobs that the
/// sentence "The *CI job list* is: ..." names.
pub fn job_list(requirements: &str) -> BTreeSet<String> {
    requirements
        .lines()
        .find(|l| l.contains("The *CI job list* is:"))
        .map(|l| quoted(l).into_iter().filter(|t| is_job(t)).map(str::to_string).collect())
        .unwrap_or_default()
}

/// R7.11 criterion 10: the workflows define every job of the CI job list,
/// and no `ci/` or `sched/` job outside it.
pub fn check_jobs(requirements: &str, jobs: &BTreeSet<String>) -> Vec<String> {
    let list = job_list(requirements);
    let mut problems = Vec::new();
    if list.is_empty() {
        problems.push("requirements.md states no CI job list".to_string());
    }
    for job in list.difference(jobs) {
        problems.push(format!("{job}: in the CI job list, and no workflow defines it"));
    }
    for job in jobs.difference(&list) {
        problems.push(format!("{job}: defined by a workflow, and not in the CI job list"));
    }
    problems
}

/// The CI jobs that the workflow runs: its steps named `ci/<job>`.
pub fn workflow_jobs(workflow: &str) -> BTreeSet<String> {
    structure(workflow).into_iter().flat_map(|group| group.steps).map(|step| step.job).collect()
}

/// One job of a workflow file, with the CI jobs that its steps are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    /// The jobs of the workflow that this one waits for.
    pub needs: Vec<String>,
    pub steps: Vec<Step>,
}

/// A step that is a CI job of the list: `- name: ci/<job>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub job: String,
    /// The declared input paths: the comment `# inputs: a/, b/` above the
    /// step.
    pub inputs: Vec<String>,
}

/// The jobs of a workflow file. The file is read by its lines: a job is a
/// key at two spaces under `jobs:`, its `needs` a list on one line, and a
/// CI job a step whose name starts with `ci/` or `sched/`.
pub fn structure(workflow: &str) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut in_jobs = false;
    let mut inputs: Vec<String> = Vec::new();
    for line in workflow.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() && !line.starts_with('#') {
            in_jobs = line.trim_end() == "jobs:";
            continue;
        }
        if !in_jobs {
            continue;
        }
        let trimmed = line.trim();
        let key = line.strip_prefix("  ").filter(|l| !l.starts_with([' ', '#'])).and_then(|l| l.trim_end().strip_suffix(':'));
        if let Some(name) = key {
            groups.push(Group { name: name.to_string(), needs: Vec::new(), steps: Vec::new() });
        } else if let Some(group) = groups.last_mut() {
            if let Some(list) = trimmed.strip_prefix("needs:") {
                group.needs = list
                    .trim()
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .split(',')
                    .map(|n| n.trim().to_string())
                    .filter(|n| !n.is_empty())
                    .collect();
            } else if let Some(list) = trimmed.strip_prefix("# inputs:") {
                inputs = list.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
            } else if let Some(names) = trimmed.strip_prefix("- name: ") {
                for name in names.split(',').map(str::trim).filter(|n| is_job(n)) {
                    group.steps.push(Step { job: name.to_string(), inputs: inputs.clone() });
                }
                inputs.clear();
            } else if trimmed.starts_with("- ") {
                inputs.clear();
            }
        }
    }
    groups
}

/// R7.11 criteria 9 and 10: `ci/traceability` comes after every other
/// `ci/` job (an earlier step of its own job, or a step of a job that its
/// job needs), and every `sched/` job declares input paths.
pub fn check_order(groups: &[Group]) -> Vec<String> {
    let mut problems = Vec::new();
    let last = groups.iter().find(|g| g.steps.iter().any(|s| s.job == "ci/traceability"));
    for group in groups {
        for (index, step) in group.steps.iter().enumerate() {
            if step.job.starts_with("sched/") && step.inputs.is_empty() {
                problems.push(format!("{}: a scheduled job that declares no input paths", step.job));
            }
            if !step.job.starts_with("ci/") || step.job == "ci/traceability" {
                continue;
            }
            let Some(last) = last else { continue };
            let before = if group.name == last.name {
                last.steps.iter().position(|s| s.job == "ci/traceability").is_some_and(|at| index < at)
            } else {
                last.needs.contains(&group.name)
            };
            if !before {
                problems.push(format!(
                    "ci/traceability does not depend on {}: its job does not need the job {:?}",
                    step.job, group.name
                ));
            }
        }
    }
    problems
}

/// What the check reads of a repository.
#[derive(Clone, Debug)]
pub struct Inputs {
    pub requirements: String,
    /// `docs/traceability.toml`.
    pub table: String,
    /// `docs/traceability.md`.
    pub rendered: String,
    /// The texts of the workflow files.
    pub workflows: Vec<String>,
    /// The files that git tracks, and those that are staged to be.
    pub tracked: BTreeSet<String>,
    /// `docs/formats/index.toml`, the format registry.
    pub formats: String,
}

impl Inputs {
    pub fn load(root: &Path) -> Result<Inputs, String> {
        let workflows = root.join(".github/workflows");
        let mut paths: Vec<_> = fs::read_dir(&workflows)
            .map_err(|e| format!("{}: {e}", workflows.display()))?
            .flatten()
            .map(|entry| entry.path())
            .collect();
        paths.sort();
        Ok(Inputs {
            requirements: read(&root.join(".kiro/specs/creusot-distributed-proving/requirements.md"))?,
            table: read(&root.join("docs/traceability.toml"))?,
            rendered: read(&root.join("docs/traceability.md")).unwrap_or_default(),
            workflows: paths.iter().map(|p| read(p)).collect::<Result<_, _>>()?,
            tracked: tracked_files(root)?,
            formats: read(&root.join("docs/formats/index.toml"))?,
        })
    }
}

/// Every problem of a repository; an error when the table cannot be read.
pub fn check_all(inputs: &Inputs) -> Result<Vec<String>, String> {
    let Inputs { requirements, tracked, .. } = inputs;
    let table: Table = toml::from_str(&inputs.table).map_err(|e| format!("docs/traceability.toml: {e}"))?;
    // The jobs of every workflow: the gating one and the scheduled one.
    let groups: Vec<Group> = inputs.workflows.iter().flat_map(|w| structure(w)).collect();
    let jobs: BTreeSet<String> = groups.iter().flat_map(|g| &g.steps).map(|s| s.job.clone()).collect();
    let mut problems = check(&table, requirements, tracked, &jobs);
    problems.extend(check_jobs(requirements, &jobs));
    problems.extend(check_order(&groups));
    let registry: formats::Registry =
        toml::from_str(&inputs.formats).map_err(|e| format!("docs/formats/index.toml: {e}"))?;
    let in_scope: BTreeSet<&str> = table.row.iter().filter(|r| r.scope == "in scope").map(|r| r.req.as_str()).collect();
    problems.extend(formats::check(&registry, tracked, &in_scope));
    // Every job has its script, and every script is a job (R7.11: one
    // script per job under ci/jobs).
    for job in &jobs {
        let script = format!("ci/jobs/{}.sh", job.rsplit('/').next().unwrap_or(job));
        if !tracked.contains(&script) {
            problems.push(format!("{job}: no script {script}"));
        }
    }
    for script in tracked.iter().filter(|f| f.starts_with("ci/jobs/") && f.ends_with(".sh")) {
        let name = &script["ci/jobs/".len()..script.len() - 3];
        if name != "common" && !jobs.iter().any(|j| j.rsplit('/').next() == Some(name)) {
            problems.push(format!("{script}: a script of no job"));
        }
    }
    if inputs.rendered != render(&table, tracked) {
        problems.push("docs/traceability.md is not current (run traceability-check --write)".to_string());
    }
    Ok(problems)
}

/// Checks the repository at `root`, or with `write` rewrites the derived
/// fields first.
pub fn run(root: &Path, write: bool) -> Result<Vec<String>, String> {
    if write {
        let table_path = root.join("docs/traceability.toml");
        let requirements = read(&root.join(".kiro/specs/creusot-distributed-proving/requirements.md"))?;
        let table: Table = match read(&table_path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("docs/traceability.toml: {e}"))?,
            Err(_) => Table::default(),
        };
        let table = regenerate(&table, &requirements);
        fs::write(&table_path, print(&table)).map_err(|e| e.to_string())?;
        fs::write(root.join("docs/traceability.md"), render(&table, &tracked_files(root)?)).map_err(|e| e.to_string())?;
    }
    check_all(&Inputs::load(root)?)
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
        workflow_jobs("jobs:\n  checks:\n    steps:\n      - name: ci/creusot\n      - name: ci/sim, ci/test\n      - name: build the shell\n")
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
    fn the_workflows_define_exactly_the_job_list() {
        let requirements = "- The *CI job list* is: `ci/test`, `ci/sim` and `ci/traceability`, which run on every pull request; and `sched/eval`, which runs on a schedule.\n";
        let list = job_list(requirements);
        assert_eq!(list.len(), 4);
        let defined = |names: &[&str]| -> BTreeSet<String> { names.iter().map(|n| n.to_string()).collect() };
        assert!(check_jobs(requirements, &defined(&["ci/test", "ci/sim", "ci/traceability", "sched/eval"])).is_empty());
        let problems = check_jobs(requirements, &defined(&["ci/test", "ci/traceability", "sched/eval", "ci/extra"]));
        assert_eq!(
            problems,
            ["ci/sim: in the CI job list, and no workflow defines it", "ci/extra: defined by a workflow, and not in the CI job list"]
        );
        assert!(!check_jobs("no list here", &defined(&[])).is_empty());
    }

    #[test]
    fn a_workflow_is_read_by_its_jobs_needs_and_inputs() {
        let text = "name: x\non:\n  push:\njobs:\n  # a comment\n  checks:\n    steps:\n      - uses: a\n      - name: ci/test\n        run: x\n      - name: ci/sim, ci/diff\n  last:\n    needs: [checks, other]\n    steps:\n      - name: ci/traceability\n  nightly:\n    steps:\n      # inputs: tests/sim/, crates/\n      - name: sched/sim-nightly\n      - name: sched/eval\n";
        let groups = structure(text);
        assert_eq!(groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["checks", "last", "nightly"]);
        assert_eq!(groups[0].steps.iter().map(|s| s.job.as_str()).collect::<Vec<_>>(), ["ci/test", "ci/sim", "ci/diff"]);
        assert_eq!(groups[1].needs, ["checks", "other"]);
        assert_eq!(groups[2].steps[0].inputs, ["tests/sim/", "crates/"]);
        // The inputs are those of the step below the comment only.
        assert_eq!(check_order(&groups), ["sched/eval: a scheduled job that declares no input paths"]);
        // Without the dependency, every job of the other group is named.
        let independent = structure(&text.replace("    needs: [checks, other]\n", ""));
        assert_eq!(check_order(&independent).iter().filter(|p| p.starts_with("ci/traceability does not depend on")).count(), 3);
        // A step after ci/traceability in its own job does not come before it.
        let after = structure("jobs:\n  one:\n    steps:\n      - name: ci/test\n      - name: ci/traceability\n      - name: ci/sim\n");
        assert_eq!(check_order(&after), ["ci/traceability does not depend on ci/sim: its job does not need the job \"one\""]);
    }

    #[test]
    fn the_lines_of_a_row_in_scope_have_levels_artifacts_and_jobs() {
        let all = files(&["crates/core/src/coord.rs", "tests/sim/faults.rs", "tests/escalation.rs"]);
        let every: BTreeSet<String> =
            ["ci/creusot", "ci/sim", "ci/test", "sched/sim-nightly"].iter().map(|j| j.to_string()).collect();
        let run = |requirements: &str| {
            let mut t = regenerate(&Table::default(), requirements);
            t.row.iter_mut().for_each(|r| r.scope = "in scope".into());
            check(&t, requirements, &all, &every)
        };
        assert_eq!(run(REQUIREMENTS), Vec::<String>::new());
        // The only A1 line of the Soundness-critical requirement is gone.
        let problems = run(&REQUIREMENTS.replace("- A1 — contracts on `assign` — `crates/core/src/coord.rs` — `ci/creusot`.\n", ""));
        assert_eq!(problems, ["R1.1: Soundness-critical and in scope without an A1 or A3 artifact"]);
        // Its A4 line is gone.
        let problems = run(&REQUIREMENTS.replace("- A4 — simulation", "- A2 — simulation"));
        assert_eq!(problems, ["R1.1: Soundness-critical and in scope without an A4 artifact"]);
        // A line without a job, without a path, without a level.
        let problems = run(&REQUIREMENTS.replace(" — `ci/test`.", "."));
        assert!(problems.iter().any(|p| p.starts_with("R1.2: in scope, and a Verification line lacks a CI job")), "{problems:?}");
        let problems = run(&REQUIREMENTS.replace("- A4 — a test — `tests/escalation.rs`", "- A4 — a test"));
        assert!(problems.iter().any(|p| p.starts_with("R1.2: in scope, and a Verification line lacks an artifact path")), "{problems:?}");
        let problems = run(&REQUIREMENTS.replace("- A4 — a test", "- a test"));
        assert!(problems.iter().any(|p| p.starts_with("R1.2: in scope, and a Verification line lacks an assurance level")), "{problems:?}");
        assert!(problems.iter().any(|p| p == "R1.2: Performance and in scope without an A2, A4 or A5 artifact"), "{problems:?}");
        // None of this is held against a deferred row.
        let mut t = regenerate(&Table::default(), &REQUIREMENTS.replace(" — `ci/test`.", "."));
        t.row[0].scope = "in scope".into();
        assert_eq!(check(&t, &REQUIREMENTS.replace(" — `ci/test`.", "."), &files(&["crates/core/src/coord.rs", "tests/sim/x.rs"]), &every), Vec::<String>::new());
    }

    #[test]
    fn the_repository_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let problems = run(&root, false).unwrap();
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }
}
