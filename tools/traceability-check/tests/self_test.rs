// Feature: creusot-distributed-proving, Property 37: Every requirement is mechanically verified and the tests catch bugs
// P-ID: P7.11.6, P7.11.8, P7.11.9, P7.11.10, P7.11.11

//! The self-test of the Traceability_Checker on mutated copies of this
//! repository's `requirements.md`, `docs/traceability.toml` and workflows.
//!
//! The copies are held in memory: the files as they are pass the check,
//! and each change below is made to a fresh copy, which must then fail
//! with the problem that the change makes, or, for the changes that the
//! scope status excuses, must still pass.

use std::{collections::BTreeSet, path::Path};

use traceability_check::{check_all, lines_of, Inputs, Row, Table};

fn repository() -> Inputs {
    Inputs::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).expect("the repository is readable")
}

fn table(inputs: &Inputs) -> Table {
    toml::from_str(&inputs.table).expect("the table parses")
}

/// The check of a copy with one change.
fn changed(change: impl FnOnce(&mut Inputs)) -> Vec<String> {
    let mut copy = repository();
    change(&mut copy);
    check_all(&copy).unwrap_or_else(|unreadable| vec![unreadable])
}

#[track_caller]
fn fails(problems: &[String], expected: &str) {
    assert!(problems.iter().any(|p| p.contains(expected)), "{expected:?} is not among {problems:#?}");
}

/// Replaces the one place of a text; the self-test is about that place.
#[track_caller]
fn replace_once(text: &mut String, from: &str, to: &str) {
    assert_eq!(text.matches(from).count(), 1, "{from:?} is there once");
    *text = text.replace(from, to);
}

/// Changes the table of a copy and writes it back.
fn edit_table(inputs: &mut Inputs, change: impl FnOnce(&mut Table)) {
    let mut t = table(inputs);
    change(&mut t);
    inputs.table = traceability_check::print(&t);
    inputs.rendered = traceability_check::render(&t, &inputs.tracked);
}

fn exists(inputs: &Inputs, artifact: &str) -> bool {
    traceability_check::exists(artifact, &inputs.tracked)
}

/// The rendered table says which artifacts exist, so a commit that adds
/// or removes one renders it again; the copies do the same, so that only
/// the change itself is judged.
fn render_again(inputs: &mut Inputs) {
    inputs.rendered = traceability_check::render(&table(inputs), &inputs.tracked);
}

/// Removes an artifact from the tracked files: the file, or every file of
/// the directory.
fn remove(inputs: &mut Inputs, artifact: &str) {
    inputs.tracked.retain(|f| if artifact.ends_with('/') { !f.starts_with(artifact) } else { f != artifact });
    render_again(inputs);
}

/// Adds an artifact to the tracked files: the file, or a file in the
/// directory.
fn add(inputs: &mut Inputs, artifact: &str) {
    inputs.tracked.insert(if artifact.ends_with('/') { format!("{artifact}added") } else { artifact.to_string() });
    render_again(inputs);
}

fn in_scope(inputs: &Inputs) -> Vec<Row> {
    table(inputs).row.into_iter().filter(|r| r.scope == "in scope").collect()
}

fn deferred(inputs: &Inputs) -> Vec<Row> {
    table(inputs).row.into_iter().filter(|r| r.scope == "deferred").collect()
}

/// The section of a requirement in `requirements.md`, from its heading to
/// the next one.
fn section(requirements: &str, req: &str) -> (usize, usize) {
    let start = requirements
        .match_indices("### Requirement ")
        .map(|(at, _)| at)
        .find(|at| requirements[*at..].lines().next().is_some_and(|l| l.split_once(": ").is_some_and(|(_, r)| r.split_whitespace().next() == Some(req))))
        .unwrap_or_else(|| panic!("{req} has a section"));
    let end = requirements[start + 1..].find("\n### Requirement ").map(|at| start + 1 + at).unwrap_or(requirements.len());
    (start, end)
}

#[test]
fn the_repository_passes_as_it_is() {
    let inputs = repository();
    assert_eq!(check_all(&inputs).unwrap(), Vec::<String>::new());
    // The self-test has something to change: rows of both kinds.
    assert!(in_scope(&inputs).len() >= 10 && deferred(&inputs).len() >= 10);
}

// P7.11.6: removing an artifact path. Every artifact of every row in
// scope, one at a time.
#[test]
fn removing_any_artifact_of_a_row_in_scope_fails() {
    let inputs = repository();
    let mut removed = 0;
    for row in in_scope(&inputs) {
        for artifact in &row.artifacts {
            let problems = changed(|copy| remove(copy, artifact));
            fails(&problems, &format!("{}: in scope, and its artifact {artifact} does not exist", row.req));
            removed += 1;
        }
    }
    assert!(removed >= 20, "{removed} artifacts");
}

// P7.11.11: a missing artifact of a deferred row alone is no failure.
#[test]
fn removing_an_artifact_of_a_deferred_row_does_not_fail() {
    let inputs = repository();
    let shared: BTreeSet<String> = in_scope(&inputs).into_iter().flat_map(|r| r.artifacts).collect();
    let mut removed = 0;
    for row in deferred(&inputs) {
        // An artifact that no row in scope also names, and nothing in it.
        let own = |a: &&String| {
            exists(&inputs, a) && !shared.iter().any(|s| s.starts_with(a.as_str()) || a.starts_with(s.as_str()))
        };
        for artifact in row.artifacts.iter().filter(own) {
            // Scripts of jobs and the tool itself are held by other checks.
            if artifact.starts_with("ci/") || artifact.starts_with("docs/traceability") {
                continue;
            }
            let problems = changed(|copy| remove(copy, artifact));
            assert_eq!(problems, Vec::<String>::new(), "{}: {artifact}", row.req);
            removed += 1;
        }
    }
    assert!(removed >= 5, "{removed} artifacts");
}

/// The row has no problem; what was added may have completed other rows
/// that name the same artifacts, and nothing else is wrong.
#[track_caller]
fn still_deferred(problems: &[String], req: &str) {
    assert!(!problems.iter().any(|p| p.starts_with(&format!("{req}: "))), "{problems:#?}");
    assert!(problems.iter().all(|p| p.contains(": deferred, but every artifact")), "{problems:#?}");
}

// P7.11.11: adding the last missing artifact of a deferred row without
// switching the row fails; adding all but one does not; and a directory
// counts only when it holds a tracked file.
#[test]
fn completing_a_deferred_row_without_switching_it_fails() {
    let inputs = repository();
    let mut completed = 0;
    let mut directories = 0;
    for row in deferred(&inputs) {
        let missing: Vec<&String> = row.artifacts.iter().filter(|a| !exists(&inputs, a)).collect();
        assert!(!missing.is_empty(), "{}: a deferred row lacks an artifact on a commit that passes", row.req);
        let problems = changed(|copy| missing.iter().for_each(|a| add(copy, a)));
        fails(&problems, &format!("{}: deferred, but every artifact", row.req));
        completed += 1;
        // All but one, the deepest path, which no other brings with it:
        // the row may stay deferred, and nothing else is wrong.
        let last = *missing.iter().max_by_key(|a| a.len()).unwrap();
        let problems = changed(|copy| missing.iter().filter(|a| **a != last).for_each(|a| add(copy, a)));
        still_deferred(&problems, &row.req);
        // A file with the name of a missing directory is not the
        // directory (one that no other missing artifact is in).
        let alone = |d: &&&String| d.ends_with('/') && !missing.iter().any(|a| a != *d && a.starts_with(d.as_str()));
        if let Some(directory) = missing.iter().find(alone) {
            let problems = changed(|copy| {
                missing.iter().filter(|a| a != &directory).for_each(|a| add(copy, a));
                copy.tracked.insert(directory.trim_end_matches('/').to_string());
                render_again(copy);
            });
            still_deferred(&problems, &row.req);
            directories += 1;
        }
        // Switched with the artifact, the row is held to what a row in
        // scope is held to, and no longer to this.
        let problems = changed(|copy| {
            missing.iter().for_each(|a| add(copy, a));
            edit_table(copy, |t| t.row.iter_mut().find(|r| r.req == row.req).unwrap().scope = "in scope".into());
        });
        assert!(!problems.iter().any(|p| p.starts_with(&format!("{}: deferred", row.req))), "{problems:?}");
    }
    assert!(completed >= 10 && directories >= 1, "{completed} rows, {directories} directories");
}

// P7.11.6 and P7.11.9: renaming a CI job, in the workflow and in a
// Verification line; removing `ci/lean-partition` from the workflow.
#[test]
fn renaming_or_removing_a_job_fails() {
    let rename = |copy: &mut Inputs| {
        let workflow = copy.workflows.iter_mut().find(|w| w.contains("- name: ci/sim\n")).expect("a workflow runs ci/sim");
        replace_once(workflow, "- name: ci/sim\n", "- name: ci/simulation\n");
    };
    let problems = changed(rename);
    fails(&problems, "ci/sim: in the CI job list, and no workflow defines it");
    fails(&problems, "ci/simulation: defined by a workflow, and not in the CI job list");
    fails(&problems, "in scope, and the workflow runs no job ci/sim");
    fails(&problems, "ci/simulation: no script ci/jobs/simulation.sh");
    fails(&problems, "ci/jobs/sim.sh: a script of no job");

    let problems = changed(|copy| {
        let workflow = copy.workflows.iter_mut().find(|w| w.contains("- name: ci/lean-partition\n")).unwrap();
        // The step with the lines that belong to it.
        let start = workflow.find("      - name: ci/lean-partition\n").unwrap();
        let rest = &workflow[start + 1..];
        let end = start + 1 + rest.find("\n      - ").map(|at| at + 1).unwrap_or(rest.len());
        workflow.replace_range(start..end, "");
    });
    fails(&problems, "ci/lean-partition: in the CI job list, and no workflow defines it");

    // A job of a Verification line that is renamed there.
    let inputs = repository();
    let row = in_scope(&inputs).into_iter().find(|r| r.jobs.iter().any(|j| j == "ci/sim")).expect("a row names ci/sim");
    let problems = changed(|copy| {
        let (start, end) = section(&copy.requirements, &row.req);
        let renamed = copy.requirements[start..end].replace("`ci/sim`", "`ci/simulated`");
        copy.requirements.replace_range(start..end, &renamed);
    });
    fails(&problems, &format!("{}: jobs differ from requirements.md", row.req));

    // A scheduled job is in the list too.
    let problems = changed(|copy| {
        let workflow = copy.workflows.iter_mut().find(|w| w.contains("- name: sched/sim-nightly\n")).unwrap();
        replace_once(workflow, "- name: sched/sim-nightly\n", "- name: nightly simulation\n");
    });
    fails(&problems, "sched/sim-nightly: in the CI job list, and no workflow defines it");
}

// P7.11.6 and P7.11.8: removing the dependency of `ci/traceability` on one
// `ci/` job: each job that its job needs, one at a time, and all of them.
#[test]
fn removing_a_dependency_of_the_last_job_fails() {
    let inputs = repository();
    let groups: Vec<_> = inputs.workflows.iter().flat_map(|w| traceability_check::structure(w)).collect();
    let last = groups.iter().find(|g| g.steps.iter().any(|s| s.job == "ci/traceability")).expect("a job runs ci/traceability");
    assert!(!last.needs.is_empty());
    let line = format!("    needs: [{}]\n", last.needs.join(", "));
    for dropped in &last.needs {
        let kept: Vec<&str> = last.needs.iter().filter(|n| n != &dropped).map(String::as_str).collect();
        let problems = changed(|copy| {
            let workflow = copy.workflows.iter_mut().find(|w| w.contains(&line)).expect("the needs line is as printed");
            replace_once(workflow, &line, &format!("    needs: [{}]\n", kept.join(", ")));
        });
        let group = groups.iter().find(|g| &g.name == dropped).unwrap();
        for step in group.steps.iter().filter(|s| s.job.starts_with("ci/")) {
            fails(&problems, &format!("ci/traceability does not depend on {}", step.job));
        }
        assert!(group.steps.iter().any(|s| s.job.starts_with("ci/")), "{dropped} runs a ci/ job");
    }
    let problems = changed(|copy| {
        let workflow = copy.workflows.iter_mut().find(|w| w.contains(&line)).unwrap();
        replace_once(workflow, &line, "");
    });
    let others = groups.iter().filter(|g| g.name != last.name).flat_map(|g| &g.steps).filter(|s| s.job.starts_with("ci/")).count();
    assert_eq!(problems.iter().filter(|p| p.starts_with("ci/traceability does not depend on")).count(), others);
}

// P7.11.9: every scheduled job declares input paths.
#[test]
fn a_scheduled_job_without_declared_inputs_fails() {
    let inputs = repository();
    let scheduled: Vec<String> = inputs
        .workflows
        .iter()
        .flat_map(|w| traceability_check::structure(w))
        .flat_map(|g| g.steps)
        .filter(|s| s.job.starts_with("sched/"))
        .map(|s| s.job)
        .collect();
    assert!(scheduled.len() >= 6, "{scheduled:?}");
    for job in &scheduled {
        let problems = changed(|copy| {
            let name = format!("      - name: {job}\n");
            let workflow = copy.workflows.iter_mut().find(|w| w.contains(&name)).unwrap();
            let at = workflow.find(&name).unwrap();
            let comment = workflow[..at].rfind("      # inputs:").expect("the job declares inputs");
            workflow.replace_range(comment..at, "");
        });
        assert_eq!(problems, [format!("{job}: a scheduled job that declares no input paths")]);
    }
}

// P7.11.6: adding a requirement without a row.
#[test]
fn a_requirement_without_a_row_fails() {
    let problems = changed(|copy| {
        let at = copy.requirements.find("## Initial Traceability Table").expect("the requirements have the table");
        copy.requirements.insert_str(
            at,
            "### Requirement 99: R9.9 One more\n\n#### Properties\n\n- **P9.9.1** One.\n\n#### Verification\n\n- A4 — a test — `tests/more.rs` — `ci/test`.\n\n",
        );
    });
    fails(&problems, "R9.9: a requirement without a row");
    // And a row without a requirement.
    let problems = changed(|copy| {
        edit_table(copy, |t| {
            let mut row = t.row[0].clone();
            row.req = "R9.9".into();
            t.row.push(row);
        })
    });
    fails(&problems, "R9.9: a row without a requirement");
    // A row that is removed.
    let problems = changed(|copy| edit_table(copy, |t| drop(t.row.remove(0))));
    fails(&problems, ": a requirement without a row");
}

// P7.11.6: a Verification line without a CI job, for every row in scope;
// the same line under a deferred row is no failure.
#[test]
fn a_verification_line_without_a_job_fails_for_a_row_in_scope() {
    let line = "- A4 — one more test — `tools/traceability-check/src/lib.rs`.\n";
    let add_line = |copy: &mut Inputs, req: &str| {
        let (start, end) = section(&copy.requirements, req);
        let verification = start + copy.requirements[start..end].find("#### Verification\n\n").expect("a Verification subsection")
            + "#### Verification\n\n".len();
        copy.requirements.insert_str(verification, line);
        // The table follows the requirements; only the line is at fault.
        let requirements = copy.requirements.clone();
        edit_table(copy, |t| *t = traceability_check::regenerate(t, &requirements));
    };
    let inputs = repository();
    for row in in_scope(&inputs) {
        let problems = changed(|copy| add_line(copy, &row.req));
        assert_eq!(problems.len(), 1, "{problems:#?}");
        fails(&problems, &format!("{}: in scope, and a Verification line lacks a CI job", row.req));
    }
    let row = deferred(&inputs).into_iter().next().unwrap();
    assert_eq!(changed(|copy| add_line(copy, &row.req)), Vec::<String>::new());
}

// P7.11.6: deleting the only A1 or A3 line of a Soundness-critical
// requirement: for every such row in scope, all of its A1 and A3 lines.
#[test]
fn a_soundness_critical_row_without_a1_or_a3_fails() {
    let inputs = repository();
    let lines = lines_of(&inputs.requirements);
    let mut rows = 0;
    for row in in_scope(&inputs).into_iter().filter(|r| r.class == "Soundness-critical") {
        let proofs: Vec<&str> = lines[&row.req]
            .iter()
            .filter(|l| l.levels.iter().any(|level| level == "A1" || level == "A3"))
            .map(|l| l.text.as_str())
            .collect();
        assert!(!proofs.is_empty(), "{}", row.req);
        let problems = changed(|copy| {
            for line in &proofs {
                replace_once(&mut copy.requirements, &format!("{line}\n"), "");
            }
            let requirements = copy.requirements.clone();
            edit_table(copy, |t| *t = traceability_check::regenerate(t, &requirements));
        });
        // A line that also was the A4 line takes that with it; nothing
        // else is at fault.
        fails(&problems, &format!("{}: Soundness-critical and in scope without an A1 or A3 artifact", row.req));
        assert!(problems.iter().all(|p| p.starts_with(&format!("{}: Soundness-critical and in scope without", row.req))), "{problems:#?}");
        rows += 1;
    }
    assert!(rows >= 3, "{rows} Soundness-critical rows in scope");
}

// P7.11.6 and P7.11.10: removing a row's scope status, or giving it one
// that is none of the three.
#[test]
fn a_row_without_a_scope_status_fails() {
    let inputs = repository();
    let first = table(&inputs).row[0].clone();
    let problems = changed(|copy| edit_table(copy, |t| t.row[0].scope = String::new()));
    fails(&problems, &format!("{}: unknown scope status \"\"", first.req));
    let problems = changed(|copy| edit_table(copy, |t| t.row[0].scope = "later".into()));
    fails(&problems, &format!("{}: unknown scope status \"later\"", first.req));
    // The line itself is removed from the file: the table is not read.
    let problems = changed(|copy| {
        let at = copy.table.find("scope = ").expect("a row has a scope status");
        let end = at + copy.table[at..].find('\n').unwrap() + 1;
        copy.table.replace_range(at..end, "");
    });
    assert_eq!(problems.len(), 1);
    fails(&problems, "docs/traceability.toml: ");
    fails(&problems, "scope");
}

// The table and the requirements drift apart, and the rendered table
// with them.
#[test]
fn a_table_that_differs_from_the_requirements_fails() {
    let inputs = repository();
    let row = in_scope(&inputs).into_iter().find(|r| r.properties.len() > 1 && !r.functions.is_empty()).expect("a row with properties and functions");
    let of = |problem: &str| format!("{}: {problem}", row.req);
    fails(&changed(|copy| edit_table(copy, |t| t.row.iter_mut().find(|r| r.req == row.req).unwrap().properties.truncate(1))), &of("properties differ"));
    fails(&changed(|copy| edit_table(copy, |t| t.row.iter_mut().find(|r| r.req == row.req).unwrap().functions.clear())), &of("functions differ"));
    fails(&changed(|copy| edit_table(copy, |t| t.row.iter_mut().find(|r| r.req == row.req).unwrap().artifacts.truncate(0))), &of("artifacts differ"));
    fails(&changed(|copy| edit_table(copy, |t| t.row.iter_mut().find(|r| r.req == row.req).unwrap().jobs.push("ci/more".into()))), &of("jobs differ"));
    fails(&changed(|copy| edit_table(copy, |t| t.row.iter_mut().find(|r| r.req == row.req).unwrap().class = "Process".into())), &of("class"));
    fails(&changed(|copy| edit_table(copy, |t| t.row.push(t.row[0].clone()))), ": two rows");
    // The requirements gain a Property that the table does not have.
    let problems = changed(|copy| {
        let (start, end) = section(&copy.requirements, &row.req);
        let at = start + copy.requirements[start..end].find("#### Verification").unwrap();
        copy.requirements.insert_str(at, "- **P99.99.99** One more.\n\n");
    });
    fails(&problems, &of("properties differ"));
    // The rendered table is not the table's.
    assert_eq!(changed(|copy| copy.rendered.push('\n')), ["docs/traceability.md is not current (run traceability-check --write)"]);
    // A script without a job, and a job without its script.
    assert_eq!(changed(|copy| {
        copy.tracked.insert("ci/jobs/extra.sh".into());
    }), ["ci/jobs/extra.sh: a script of no job"]);
    // An artifact is added or removed and the rendered table is not made again.
    let artifact = row.artifacts[0].clone();
    fails(
        &changed(|copy| copy.tracked.retain(|f| !f.starts_with(artifact.as_str()))),
        "docs/traceability.md is not current",
    );
    fails(&changed(|copy| {
        copy.tracked.remove("ci/jobs/sim.sh");
    }), "ci/sim: no script ci/jobs/sim.sh");
}

// R7.10 criterion 7, on copies of this repository's format registry: each
// artifact of each entry removed in turn; each format that is owed taken
// out of the registry; and a requirement switched to in scope that brings
// formats with it.
#[test]
fn the_format_registry_names_what_exists_for_every_format_that_is_owed() {
    use traceability_check::formats::{Registry, EXCHANGED};
    let inputs = repository();
    let registry: Registry = toml::from_str(&inputs.formats).unwrap();
    assert!(registry.format.len() >= 4 && !registry.pending.is_empty());
    for entry in &registry.format {
        for path in [&entry.grammar, &entry.codec, &entry.roundtrip, &entry.fuzz] {
            let problems = changed(|copy| {
                copy.tracked.retain(|f| if path.ends_with('/') { !f.starts_with(path.as_str()) } else { f != path });
                render_again(copy);
            });
            fails(&problems, &format!("format {}: its ", entry.name));
            fails(&problems, &format!("{path} does not exist"));
        }
    }
    // A format that is owed is neither registered nor pending.
    let scope: BTreeSet<String> = in_scope(&inputs).into_iter().map(|r| r.req).collect();
    let mut owed = 0;
    for (format, cited) in EXCHANGED.iter().filter(|(_, cited)| cited.iter().any(|r| scope.contains(*r))) {
        let header = format!("name = \"{format}\"\n");
        let problems = changed(|copy| {
            // The record or entry, from its table header to the next one.
            let at = copy.formats.find(&header).unwrap_or_else(|| panic!("{format} is in the registry"));
            let start = copy.formats[..at].rfind("[[").unwrap();
            let end = copy.formats[at..].find("\n[[").map(|e| at + e + 1).unwrap_or(copy.formats.len());
            copy.formats.replace_range(start..end, "");
        });
        assert_eq!(problems.len(), 1, "{format}: {problems:#?}");
        fails(&problems, &format!("format {format}: no entry in docs/formats/index.toml"));
        assert!(cited.iter().any(|r| problems[0].contains(r)));
        owed += 1;
    }
    assert!(owed >= 8, "{owed} formats are owed");
    // A format that no requirement in scope is cited for is not owed.
    let free = EXCHANGED.iter().find(|(_, cited)| !cited.iter().any(|r| scope.contains(*r))).unwrap();
    assert!(!inputs.formats.contains(&format!("name = \"{}\"", free.0)), "{} is not owed and not registered", free.0);
}

// R7.11 criteria 1 and 7: the trust items and the level, artifact and job
// triples of a row are those of the requirements.
#[test]
fn trust_items_and_triples_that_differ_from_the_requirements_fail() {
    let inputs = repository();
    let row = in_scope(&inputs).into_iter().find(|r| r.trust.len() > 1 && r.verification.len() > 1).expect("a row with both");
    let edit = |change: &dyn Fn(&mut Row)| {
        changed(|copy| edit_table(copy, |t| change(t.row.iter_mut().find(|r| r.req == row.req).unwrap())))
    };
    assert_eq!(edit(&|r| drop(r.trust.pop())), [format!("{}: trust items differ from requirements.md (missing [{:?}], not there [], or another order)", row.req, row.trust.last().unwrap())]);
    fails(&edit(&|r| r.trust.push("TB-999".into())), &format!("{}: trust items differ", row.req));
    let triples = format!("{}: the levels, artifacts and jobs of its Verification lines differ from requirements.md", row.req);
    assert_eq!(edit(&|r| r.verification[0].levels = vec!["A5".into()]), [triples.clone()]);
    assert_eq!(edit(&|r| drop(r.verification.remove(0))), [triples.clone()]);
    assert_eq!(edit(&|r| r.verification[0].jobs.clear()), [triples.clone()]);
    // An artifact that moves from one line to another: the flat list of the
    // row is the same, the triples are not.
    let problems = edit(&|r| {
        let moved = r.verification[0].artifacts.pop();
        r.verification[1].artifacts.extend(moved);
    });
    if row.verification[0].artifacts.is_empty() {
        assert_eq!(problems, Vec::<String>::new());
    } else {
        assert_eq!(problems, [triples]);
    }
    // The requirements cite one more item.
    let problems = changed(|copy| {
        let (start, end) = section(&copy.requirements, &row.req);
        let at = start + copy.requirements[start..end].find("#### Trust\n\n").expect("a Trust subsection") + "#### Trust\n\n".len();
        copy.requirements.insert_str(at, "- TB-999 (one more).\n");
    });
    fails(&problems, &format!("{}: trust items differ from requirements.md (missing [\"TB-999\"]", row.req));
}
