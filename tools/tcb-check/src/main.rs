//! `tcb-check [--write] [REPOSITORY ROOT]`: exit status 0 when the
//! TCB_Document agrees with the requirements, the Lock_File and dp-core,
//! 1 with one line per problem when it does not, 2 when it cannot run.

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
    match tcb_check::run(&root, write) {
        Ok(problems) if problems.is_empty() => {
            println!("ci/tcb-check: docs/tcb.toml agrees with the requirements, the Lock_File and dp-core");
            ExitCode::SUCCESS
        }
        Ok(problems) => {
            for p in &problems {
                eprintln!("ci/tcb-check: {p}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("ci/tcb-check: {e}");
            ExitCode::from(2)
        }
    }
}
