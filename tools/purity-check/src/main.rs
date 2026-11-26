//! `purity-check [REPOSITORY ROOT]`: exit status 0 when dp-core is pure, 1
//! with one line per problem when it is not, 2 when the check cannot run.

use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let root = std::env::args_os().nth(1).map_or_else(|| PathBuf::from("."), PathBuf::from);
    // What the registry still owes is said on every run.
    if let Ok(registry) = purity_check::registry(&root) {
        for line in purity_check::pending(&registry) {
            eprintln!("ci/purity: {line}");
        }
    }
    match purity_check::check(&root) {
        Ok(problems) if problems.is_empty() => {
            println!("ci/purity: dp-core is pure");
            ExitCode::SUCCESS
        }
        Ok(problems) => {
            for p in &problems {
                eprintln!("ci/purity: {p}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("ci/purity: {e}");
            ExitCode::from(2)
        }
    }
}
