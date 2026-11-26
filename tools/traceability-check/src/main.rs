//! `traceability-check [--write] [REPOSITORY ROOT]`: exit status 0 when
//! the Traceability_Table agrees with the requirements and with what
//! exists, 1 with one line per problem when it does not, 2 when it cannot
//! run.

use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let mut write = false;
    let mut root = PathBuf::from(".");
    for arg in std::env::args_os().skip(1) {
        if arg == "--write" {
            write = true;
        } else {
            root = PathBuf::from(arg);
        }
    }
    match traceability_check::run(&root, write) {
        Ok(problems) if problems.is_empty() => {
            println!("ci/traceability: docs/traceability.toml agrees with the requirements and with what exists");
            ExitCode::SUCCESS
        }
        Ok(problems) => {
            for p in &problems {
                eprintln!("ci/traceability: {p}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("ci/traceability: {e}");
            ExitCode::from(2)
        }
    }
}
