//! `mutants-gate --check [ROOT]`: the static check of the target list.
//! `mutants-gate --files [ROOT]`: the files to mutate, one per line.
//! `mutants-gate --crates [ROOT]`: the crates with a score threshold, each
//! with the packages whose tests judge it.
//! `mutants-gate OUTCOMES.json...`: judges a run of cargo-mutants, from
//! the repository root: the outcomes of the targets and of each crate.
//! Exit status 0, 1 with one line per problem, or 2 when it cannot run.

use std::{path::PathBuf, process::ExitCode};

use mutants_gate::{check_targets, judge, judge_crates, logic_of, outcomes, read_targets};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(first) = args.first() else {
        eprintln!("usage: mutants-gate (--check | --files | --crates) [ROOT] | mutants-gate OUTCOMES.json...");
        return ExitCode::from(2);
    };
    // The outcomes of a run: one file for the targets and one per crate.
    let judging = first.ends_with(".json");
    let root = PathBuf::from(if judging { "." } else { args.get(1).map_or(".", String::as_str) });
    let run = || -> Result<Vec<String>, String> {
        let (targets, exclusions) = read_targets(&root)?;
        match first.as_str() {
            "--check" => check_targets(&root, &targets),
            "--files" => {
                let mut files: Vec<&str> = targets.target.iter().map(|t| t.file.as_str()).collect();
                files.dedup();
                println!("{}", files.join("\n"));
                Ok(Vec::new())
            }
            // Per crate with a score threshold: its name and the packages
            // whose tests judge it.
            "--crates" => {
                for rule in &targets.crates {
                    println!("{} {}", rule.name, rule.tests.join(","));
                }
                Ok(Vec::new())
            }
            _ => {
                let mut mutants = Vec::new();
                for path in &args {
                    let json = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
                    mutants.extend(outcomes(&json)?);
                }
                let verdict = judge(&mutants, &targets, &exclusions, &logic_of(&root, &targets));
                println!(
                    "sched/mutants-weekly: targets: {} mutants caught, {} survived, {} excluded",
                    verdict.caught, verdict.missed, verdict.excluded
                );
                let (scores, crate_problems) = judge_crates(&mutants, &targets);
                for score in &scores {
                    println!(
                        "sched/mutants-weekly: crate {}: {} of {} mutants caught ({:.1} %), {} did not compile",
                        score.name,
                        score.caught,
                        score.caught + score.missed,
                        score.share() * 100.0,
                        score.unviable
                    );
                }
                Ok(verdict.problems.into_iter().chain(crate_problems).collect())
            }
        }
    };
    match run() {
        Ok(problems) if problems.is_empty() => ExitCode::SUCCESS,
        Ok(problems) => {
            for p in &problems {
                eprintln!("mutants-gate: {p}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("mutants-gate: {e}");
            ExitCode::from(2)
        }
    }
}
