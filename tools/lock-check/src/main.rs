//! `lock-check [--evidence] [REPOSITORY ROOT]`: the completeness of the
//! Lock_File, and with `--evidence` also its upstream checksums or double
//! rebuilds and its platforms. Exit status 0, 1 with one line per
//! problem, or 2 when the check cannot run.

use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let mut with_evidence = false;
    let mut root = PathBuf::from(".");
    for arg in std::env::args_os().skip(1) {
        if arg == "--evidence" {
            with_evidence = true;
        } else {
            root = PathBuf::from(arg);
        }
    }
    let read = match lock_check::read(&root) {
        Ok(read) => read,
        Err(e) => {
            eprintln!("lock-check: {e}");
            return ExitCode::from(2);
        }
    };
    let mut problems = lock_check::check(&read.lock, &read.expected, &read.cargo_lock);
    if with_evidence {
        problems.extend(lock_check::evidence(&read.lock, &read.expected));
    }
    for p in &read.expected.pending {
        println!("lock-check: pending: {} ({})", p.name, p.reason);
    }
    if problems.is_empty() {
        println!("lock-check: the Lock_File pins {} tools{}", read.lock.tool.len(), if with_evidence { ", each with its evidence" } else { "" });
        return ExitCode::SUCCESS;
    }
    for p in &problems {
        eprintln!("lock-check: {p}");
    }
    ExitCode::from(1)
}
