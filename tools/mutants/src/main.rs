//! `mutants-gate --check [ROOT]`: the static check of the target list.
//! `mutants-gate --files [ROOT]`: the files to mutate, one per line.
//! `mutants-gate OUTCOMES.json [ROOT]`: judges a run of cargo-mutants.
//! Exit status 0, 1 with one line per problem, or 2 when it cannot run.

use std::{path::PathBuf, process::ExitCode};

use mutants_gate::{check_targets, judge, logic_of, outcomes, read_targets};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(first), root) = (args.first(), PathBuf::from(args.get(1).map_or(".", String::as_str))) else {
        eprintln!("usage: mutants-gate (--check | --files | OUTCOMES.json) [ROOT]");
        return ExitCode::from(2);
    };
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
            path => {
                let json = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
                let verdict = judge(&outcomes(&json)?, &targets, &exclusions, &logic_of(&root, &targets));
                println!(
                    "sched/mutants-weekly: targets: {} mutants caught, {} survived, {} excluded",
                    verdict.caught, verdict.missed, verdict.excluded
                );
                Ok(verdict.problems)
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
