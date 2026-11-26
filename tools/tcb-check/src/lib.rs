//! `ci/tcb-check` (R7.8, design C20): the TCB_Document `docs/tcb.toml`.
//!
//! The document lists what the labels of the System rest on: the trusted
//! items (TB-n), the Assumptions (AS-n), the Non-goals (NG-n) and the
//! Fairness assumptions (F1 to F4) of `requirements.md`, each with the
//! requirements whose Trust subsection cites it, and every tool of the
//! Lock_File with what it is trusted for. The check fails when
//!
//! - an item of `requirements.md` is missing from the document, or the
//!   document has an item that the requirements do not define;
//! - an item's summary, status or list of requirements is not what the
//!   requirements say (the list is derived from the Trust subsections);
//! - a Trust subsection cites an identifier that is not defined;
//! - a trusted item is cited by no requirement and does not say that it
//!   holds for every label;
//! - a tool of the Lock_File has no entry, an entry names no tool of the
//!   Lock_File, an entry gives no reason or cites an unknown item, or a
//!   solver or a checker is not tied to the Non-goal that covers it;
//! - the information order of the non-proof outcomes is not the order
//!   that dp-core implements;
//! - `docs/tcb.md` is not the rendering of the document.
//!
//! `tcb-check --write` rewrites the derived parts (items and `tcb.md`) and
//! keeps the parts written by hand (the tools and the reasons).

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use dp_core::labels::Outcome;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tcb {
    /// The non-proof outcomes, most informative first (R7.2).
    pub information_order: Vec<String>,
    #[serde(default)]
    pub tool: Vec<Tool>,
    #[serde(default)]
    pub item: Vec<Item>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: String,
    /// `trusted`, `assumption`, `non-goal` or `fairness`.
    pub kind: String,
    /// For an Assumption: `Given` or `To check`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status: String,
    /// The requirements whose Trust subsection cites the item.
    pub requirements: Vec<String>,
    /// Written by hand: the item holds for every label, whatever cites it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub every_label: bool,
    /// The start of the item's text in `requirements.md`.
    pub summary: String,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    /// The name of the Lock_File entry.
    pub name: String,
    /// What a label needs the tool to be right about.
    pub trusted_for: String,
    pub items: Vec<String>,
    pub reason: String,
}

fn kind_of(id: &str) -> &'static str {
    match &id[..1] {
        "T" => "trusted",
        "A" => "assumption",
        "N" => "non-goal",
        _ => "fairness",
    }
}

/// The identifiers of trust items in a text: `TB-n`, `AS-n`, `NG-n`, and
/// `F1` to `F4`, also written as the range `F1–F4`.
pub fn cited(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let bytes = text.as_bytes();
    let word_start = |i: usize| i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'-');
    for prefix in ["TB-", "AS-", "NG-"] {
        let mut from = 0;
        while let Some(at) = text[from..].find(prefix) {
            let start = from + at;
            let digits: String = text[start + 3..].chars().take_while(char::is_ascii_digit).collect();
            if !digits.is_empty() && word_start(start) {
                out.insert(format!("{prefix}{digits}"));
            }
            from = start + 3;
        }
    }
    if text.contains("F1–F4") {
        out.extend(["F1", "F2", "F3", "F4"].map(String::from));
    }
    for n in 1..=4 {
        let id = format!("F{n}");
        let mut from = 0;
        while let Some(at) = text[from..].find(&id) {
            let start = from + at;
            let after = bytes.get(start + 2).copied().unwrap_or(b' ');
            if word_start(start) && !after.is_ascii_alphanumeric() {
                out.insert(id.clone());
            }
            from = start + 2;
        }
    }
    out
}

fn summary_of(text: &str) -> String {
    let text = text.trim();
    let end = text.find(". ").map_or(text.len(), |i| i + 1);
    let mut cut = end.min(240);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text[..cut].to_string()
}

/// The trust items that `requirements.md` defines, with the requirements
/// whose Trust subsection cites each, and the identifiers that a Trust
/// subsection cites.
pub fn items_of(requirements: &str) -> (Vec<Item>, BTreeMap<String, BTreeSet<String>>) {
    let mut items: Vec<Item> = Vec::new();
    for line in requirements.lines() {
        // `- **TB-1** text`, `- **AS-1 (Given)** text`, `- **NG-1 Title.** text`.
        if let Some(rest) = line.strip_prefix("- **") {
            let Some((bold, text)) = rest.split_once("**") else { continue };
            let id: String = bold.chars().take_while(|c| !c.is_whitespace()).collect();
            if !["TB-", "AS-", "NG-"].iter().any(|p| id.starts_with(p)) || !id[3..].chars().all(|c| c.is_ascii_digit()) {
                // The Fairness assumptions are one glossary entry.
                if bold == "Fairness assumptions F1–F4" {
                    for n in 1..=4 {
                        let id = format!("F{n}");
                        let start = text.find(&format!("{id} ")).map(|i| i + 3);
                        let end = text.find(&format!("; F{} ", n + 1)).unwrap_or_else(|| text.find(". ").map_or(text.len(), |i| i));
                        if let Some(start) = start.filter(|s| *s <= end) {
                            items.push(Item {
                                kind: "fairness".to_string(),
                                status: String::new(),
                                requirements: Vec::new(),
                                every_label: false,
                                summary: summary_of(&text[start..end]),
                                id,
                            });
                        }
                    }
                }
                continue;
            }
            let after = bold[id.len()..].trim();
            let (status, title) = match after.strip_prefix('(').and_then(|a| a.split_once(')')) {
                Some((status, rest)) => (status.to_string(), rest.trim()),
                None => (String::new(), after),
            };
            let text = format!("{title} {}", text.trim());
            items.push(Item {
                kind: kind_of(&id).to_string(),
                status,
                requirements: Vec::new(),
                every_label: false,
                summary: summary_of(&text),
                id,
            });
        }
    }
    // The Trust subsection of each requirement.
    let mut citations: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut requirement: Option<String> = None;
    let mut in_trust = false;
    for line in requirements.lines() {
        if let Some(heading) = line.strip_prefix("### Requirement ") {
            // `### Requirement 3: R1.3 Result binding and authentication`
            requirement = heading.split_once(": ").and_then(|(_, r)| r.split_whitespace().next()).map(str::to_string);
            in_trust = false;
        } else if line.starts_with("## ") {
            requirement = None;
            in_trust = false;
        } else if line.starts_with("#### ") {
            in_trust = line.trim() == "#### Trust";
        } else if let (true, Some(r)) = (in_trust, &requirement) {
            for id in cited(line) {
                citations.entry(id).or_default().insert(r.clone());
            }
        }
    }
    for item in &mut items {
        item.requirements = citations.get(&item.id).map(|r| r.iter().cloned().collect()).unwrap_or_default();
    }
    (items, citations)
}

/// The name of a non-proof outcome in the document. The match has no
/// wildcard: a new outcome of dp-core does not compile until it is named.
fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::StandaloneInvalid => "standalone-invalid",
        Outcome::CheckedIncomplete => "checked-incomplete",
        Outcome::Unchecked => "unchecked",
        Outcome::DependentInvalid => "dependent-invalid",
        Outcome::Timeout => "timeout",
        Outcome::Unknown => "unknown",
        Outcome::Failed => "failed",
        Outcome::Unresolved => "unresolved",
    }
}

/// The information order that dp-core implements.
pub fn implemented_order() -> Vec<String> {
    let mut all = [
        Outcome::Unresolved,
        Outcome::Failed,
        Outcome::Unknown,
        Outcome::Timeout,
        Outcome::DependentInvalid,
        Outcome::Unchecked,
        Outcome::CheckedIncomplete,
        Outcome::StandaloneInvalid,
    ];
    all.sort_by_key(|o| o.rank_u8());
    all.iter().map(|o| outcome_name(*o).to_string()).collect()
}

/// Every disagreement of the document with the requirements, the tools of
/// the Lock_File (name and kind) and dp-core.
pub fn check(tcb: &Tcb, requirements: &str, lock_tools: &[(String, String)]) -> Vec<String> {
    let mut problems = Vec::new();
    let (defined, citations) = items_of(requirements);
    let ids: BTreeSet<&str> = defined.iter().map(|i| i.id.as_str()).collect();
    for id in citations.keys() {
        if !ids.contains(id.as_str()) {
            problems.push(format!("{id}: cited by the Trust subsection of {:?}, defined nowhere", citations[id]));
        }
    }
    let mut seen = BTreeSet::new();
    for item in &tcb.item {
        if !seen.insert(item.id.as_str()) {
            problems.push(format!("{}: listed twice", item.id));
        }
        let Some(wanted) = defined.iter().find(|d| d.id == item.id) else {
            problems.push(format!("{}: in docs/tcb.toml, not defined in requirements.md", item.id));
            continue;
        };
        if item.summary.trim().is_empty() {
            problems.push(format!("{}: no summary", item.id));
        }
        for (what, ours, theirs) in [
            ("kind", &item.kind, &wanted.kind),
            ("status", &item.status, &wanted.status),
            ("summary", &item.summary, &wanted.summary),
        ] {
            if ours != theirs {
                problems.push(format!("{}: {what} {ours:?}, requirements.md has {theirs:?}", item.id));
            }
        }
        if item.requirements != wanted.requirements {
            problems.push(format!(
                "{}: requirements {:?}, the Trust subsections cite it in {:?}",
                item.id, item.requirements, wanted.requirements
            ));
        }
        if item.kind == "trusted" && item.requirements.is_empty() && !item.every_label {
            problems.push(format!("{}: a trusted item that no requirement cites and that is not marked every_label", item.id));
        }
    }
    for d in &defined {
        if !seen.contains(d.id.as_str()) {
            problems.push(format!("{}: defined in requirements.md, missing from docs/tcb.toml", d.id));
        }
    }
    // The tools of the Lock_File.
    for (name, kind) in lock_tools {
        match tcb.tool.iter().filter(|t| &t.name == name).collect::<Vec<_>>().as_slice() {
            [tool] => {
                if tool.reason.trim().is_empty() || tool.trusted_for.trim().is_empty() {
                    problems.push(format!("tool {name}: no reason, or nothing it is trusted for"));
                }
                for id in &tool.items {
                    if !ids.contains(id.as_str()) {
                        problems.push(format!("tool {name}: cites the unknown item {id}"));
                    }
                }
                let needs = match kind.as_str() {
                    "unpatched-solver" | "sharing-solver" => Some("NG-3"),
                    "checker" | "checker-signature" | "creusot" => Some("NG-4"),
                    "why3find" | "why3" => Some("NG-2"),
                    _ => None,
                };
                if let Some(needs) = needs.filter(|n| !tool.items.iter().any(|i| i == n)) {
                    problems.push(format!("tool {name}: a Lock_File entry of kind {kind} is covered by {needs}, which the entry does not cite"));
                }
            }
            [] => problems.push(format!("tool {name}: in the Lock_File, missing from docs/tcb.toml")),
            _ => problems.push(format!("tool {name}: listed twice")),
        }
    }
    for tool in &tcb.tool {
        if !lock_tools.iter().any(|(name, _)| name == &tool.name) {
            problems.push(format!("tool {}: in docs/tcb.toml, not in the Lock_File", tool.name));
        }
    }
    if tcb.information_order != implemented_order() {
        problems.push(format!(
            "information_order {:?}, dp-core implements {:?}",
            tcb.information_order,
            implemented_order()
        ));
    }
    problems
}

/// The document with its derived parts as the requirements give them.
pub fn regenerate(tcb: &Tcb, requirements: &str) -> Tcb {
    let (mut items, _) = items_of(requirements);
    for item in &mut items {
        item.every_label = tcb.item.iter().any(|old| old.id == item.id && old.every_label);
    }
    Tcb { information_order: implemented_order(), tool: tcb.tool.clone(), item: items }
}

const HEADER: &str = "# The TCB_Document (R7.8). The items are derived from requirements.md by\n\
# `cargo run -p tcb-check -- --write`; the tools, their reasons and the\n\
# every_label marks are written by hand. `ci/tcb-check` checks both.\n\n";

pub fn print(tcb: &Tcb) -> String {
    format!("{HEADER}{}", toml::to_string_pretty(tcb).expect("the document is plain data"))
}

/// `docs/tcb.md`: the document for readers.
pub fn render(tcb: &Tcb) -> String {
    let mut out = String::from(
        "# Trusted computing base\n\nGenerated from `docs/tcb.toml` by `tcb-check --write`; do not edit.\n\n\
         ## Tools of the Lock_File\n\n| Tool | Trusted for | Items | Reason |\n|---|---|---|---|\n",
    );
    for t in &tcb.tool {
        out.push_str(&format!("| {} | {} | {} | {} |\n", t.name, t.trusted_for, t.items.join(", "), t.reason));
    }
    out.push_str("\n## Items\n\n| Item | Kind | Status | Cited by | Summary |\n|---|---|---|---|---|\n");
    for i in &tcb.item {
        let cited = if i.every_label && i.requirements.is_empty() { "every label".to_string() } else { i.requirements.join(", ") };
        out.push_str(&format!("| {} | {} | {} | {} | {} |\n", i.id, i.kind, i.status, cited, i.summary.replace('|', "\\|")));
    }
    out.push_str(&format!("\n## Information order of the non-proof outcomes\n\n{}\n", tcb.information_order.join(" > ")));
    out
}

pub struct Paths {
    pub tcb: std::path::PathBuf,
    pub rendered: std::path::PathBuf,
    pub requirements: std::path::PathBuf,
    pub lock: std::path::PathBuf,
}

pub fn paths(root: &Path) -> Paths {
    Paths {
        tcb: root.join("docs/tcb.toml"),
        rendered: root.join("docs/tcb.md"),
        requirements: root.join(".kiro/specs/creusot-distributed-proving/requirements.md"),
        lock: root.join("deps/lock.toml"),
    }
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn lock_tools(text: &str) -> Result<Vec<(String, String)>, String> {
    #[derive(Deserialize)]
    struct Lock {
        #[serde(default)]
        tool: Vec<LockTool>,
    }
    #[derive(Deserialize)]
    struct LockTool {
        name: String,
        kind: String,
    }
    // Parsed by dp-formats first, so that only a valid Lock_File is read.
    dp_formats::lockfile::LockFile::parse(text).map_err(|e| e.to_string())?;
    let lock: Lock = toml::from_str(text).map_err(|e| e.to_string())?;
    Ok(lock.tool.into_iter().map(|t| (t.name, t.kind)).collect())
}

/// Checks the repository at `root`, or with `write` rewrites the derived
/// parts first.
pub fn run(root: &Path, write: bool) -> Result<Vec<String>, String> {
    let p = paths(root);
    let requirements = read(&p.requirements)?;
    let mut tcb: Tcb = toml::from_str(&read(&p.tcb)?).map_err(|e| format!("docs/tcb.toml: {e}"))?;
    if write {
        tcb = regenerate(&tcb, &requirements);
        fs::write(&p.tcb, print(&tcb)).map_err(|e| e.to_string())?;
        fs::write(&p.rendered, render(&tcb)).map_err(|e| e.to_string())?;
    }
    let mut problems = check(&tcb, &requirements, &lock_tools(&read(&p.lock)?)?);
    if read(&p.rendered).ok().as_deref() != Some(render(&tcb).as_str()) {
        problems.push("docs/tcb.md is not the rendering of docs/tcb.toml (run tcb-check --write)".to_string());
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Feature: creusot-distributed-proving, Property 36: Builds are reproducible and the trust story is consistent
    // P-ID: P7.8.1 (seeded disagreements of the TCB_Document are found)

    const REQUIREMENTS: &str = "## Glossary\n\n\
- **Fairness assumptions F1–F4**: under which liveness is claimed: F1 messages arrive; F2 Workers step; F3 solvers end; F4 the Coordinator follows. More.\n\n\
## Assumptions\n\n- **AS-1 (Given)** The tools are pinned. More text.\n- **AS-2 (To check)** The patch is small.\n\n\
## Non-goals\n\n- **NG-3 Verifying the SMT solvers.** Reason: certificates.\n- **NG-4 Verifying the checkers.** Reason: they are the checkers.\n\n\
## Trusted items\n\n- **TB-1** Collision resistance of the hash.\n- **TB-2** Unforgeability of tags.\n\n\
## Requirements\n\n### Requirement 1: R1.1 Distribution\n\n#### Trust\n\n- AS-2 and TB-2 (tags). Liveness under F1–F4.\n\n\
#### Verification\n\n- TB-1 is not cited here, this is no Trust subsection.\n\n\
### Requirement 2: R1.2 Escalation\n\n#### Trust\n\n- Solvers are trusted (NG-3), and TB-2.\n";

    fn document() -> Tcb {
        let mut tcb = regenerate(&Tcb::default(), REQUIREMENTS);
        tcb.item.iter_mut().find(|i| i.id == "TB-1").unwrap().every_label = true;
        tcb.tool = vec![
            Tool { name: "z3".into(), trusted_for: "proved-standalone".into(), items: vec!["NG-3".into()], reason: "an Unpatched_Solver".into() },
            Tool { name: "ethos".into(), trusted_for: "certified".into(), items: vec!["NG-4".into()], reason: "the checker".into() },
        ];
        tcb
    }

    fn tools() -> Vec<(String, String)> {
        vec![("z3".to_string(), "unpatched-solver".to_string()), ("ethos".to_string(), "checker".to_string())]
    }

    #[test]
    fn items_and_their_citations_are_read_from_the_requirements() {
        let tcb = document();
        let ids: Vec<&str> = tcb.item.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["F1", "F2", "F3", "F4", "AS-1", "AS-2", "NG-3", "NG-4", "TB-1", "TB-2"]);
        let item = |id: &str| tcb.item.iter().find(|i| i.id == id).unwrap();
        assert_eq!(item("AS-2").status, "To check");
        assert_eq!(item("AS-1").summary, "The tools are pinned.");
        assert_eq!(item("NG-3").summary, "Verifying the SMT solvers.");
        assert_eq!(item("TB-2").requirements, ["R1.1", "R1.2"]);
        assert_eq!(item("F3").requirements, ["R1.1"]);
        assert_eq!(item("F3").summary, "solvers end");
        // Cited only outside a Trust subsection.
        assert!(item("TB-1").requirements.is_empty());
        assert!(check(&tcb, REQUIREMENTS, &tools()).is_empty(), "{:?}", check(&tcb, REQUIREMENTS, &tools()));
        // Printing and reading give the document back.
        assert_eq!(toml::from_str::<Tcb>(&print(&tcb)).unwrap(), tcb);
    }

    #[test]
    fn every_seeded_disagreement_is_found() {
        let found = |change: &dyn Fn(&mut Tcb), requirements: &str, tools: &[(String, String)], expected: &str| {
            let mut tcb = document();
            change(&mut tcb);
            let problems = check(&tcb, requirements, tools);
            assert!(problems.iter().any(|p| p.contains(expected)), "{expected:?} not in {problems:?}");
        };
        let t = tools();
        found(&|d| d.item.retain(|i| i.id != "AS-2"), REQUIREMENTS, &t, "AS-2: defined in requirements.md, missing");
        found(&|d| d.item[5].id = "AS-9".into(), REQUIREMENTS, &t, "AS-9: in docs/tcb.toml, not defined");
        found(&|d| d.item[5].status = "Given".into(), REQUIREMENTS, &t, "AS-2: status");
        found(&|d| d.item[9].summary.clear(), REQUIREMENTS, &t, "TB-2: no summary");
        found(&|d| d.item[9].requirements.pop().map(|_| ()).unwrap(), REQUIREMENTS, &t, "TB-2: requirements");
        // A trusted item without a requirement and without the mark.
        found(&|d| d.item[8].every_label = false, REQUIREMENTS, &t, "TB-1: a trusted item that no requirement cites");
        // A Trust subsection that cites an identifier defined nowhere.
        let dangling = REQUIREMENTS.replace("Solvers are trusted (NG-3)", "Solvers are trusted (NG-3, TB-77)");
        found(&|_| {}, &dangling, &t, "TB-77: cited by the Trust subsection");
        // Tools.
        found(&|d| d.tool.truncate(1), REQUIREMENTS, &t, "tool ethos: in the Lock_File, missing");
        found(&|d| d.tool[0].name = "yices".into(), REQUIREMENTS, &t, "tool yices: in docs/tcb.toml, not in the Lock_File");
        found(&|d| d.tool[0].reason.clear(), REQUIREMENTS, &t, "tool z3: no reason");
        found(&|d| d.tool[0].items = vec!["NG-4".into()], REQUIREMENTS, &t, "tool z3: a Lock_File entry of kind unpatched-solver is covered by NG-3");
        found(&|d| d.tool[1].items.push("NG-99".into()), REQUIREMENTS, &t, "tool ethos: cites the unknown item NG-99");
        // The information order.
        found(&|d| d.information_order.swap(0, 1), REQUIREMENTS, &t, "information_order");
    }

    #[test]
    fn the_repository_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let problems = run(&root, false).unwrap();
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }
}
