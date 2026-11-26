//! `patch-check [REPOSITORY ROOT]`: exit status 0 when the patches and
//! their Lock_File entries are in order, 1 with one line per problem when
//! they are not, 2 when the check cannot run.

use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let root = std::env::args_os().nth(1).map_or_else(|| PathBuf::from("."), PathBuf::from);
    match patch_check::run(&root) {
        Ok(problems) if problems.is_empty() => {
            println!("ci/patches: the patches have their headers, their sizes and their Lock_File entries");
            ExitCode::SUCCESS
        }
        Ok(problems) => {
            for p in &problems {
                eprintln!("ci/patches: {p}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("ci/patches: {e}");
            ExitCode::from(2)
        }
    }
}
