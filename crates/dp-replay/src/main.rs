//! `dp-replay [ARGUMENTS OF why3find prove]`: re-checks the proof-labelled
//! Verdicts of the proof files on this machine (design C17, R7.6).
//!
//! Exit status: 0 when every proof-labelled Verdict is confirmed, 1 when
//! one is not, 3 when one is contradicted, 2 for a usage or setup error.

use std::{path::PathBuf, process::ExitCode};

use dp_formats::lockfile::LockFile;
use dp_replay::{replay, Config, Outcome};

fn fail(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("dp-replay: {message}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let Some(why3find) = std::env::var_os("DP_WHY3FIND") else {
        return fail("DP_WHY3FIND must name the why3find built with the dispatch patch");
    };
    let Some(lock_path) = std::env::var_os("DP_LOCK") else {
        return fail("DP_LOCK must name the Lock_File");
    };
    let (lock, lock_hash) = match std::fs::read_to_string(&lock_path) {
        Ok(text) => match LockFile::parse(&text) {
            Ok(lock) => (lock, dp_formats::hash::sha256(text.as_bytes())),
            Err(e) => return fail(e),
        },
        Err(e) => return fail(format!("{}: {e}", PathBuf::from(lock_path).display())),
    };
    let dir = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(e) => return fail(e),
    };
    let config =
        Config { why3find: why3find.into(), lock, lock_hash, dir, args: std::env::args_os().skip(1).collect() };
    let report = match replay(config) {
        Ok(report) => report,
        Err(e) => return fail(e),
    };
    // The comparison of the Lock_File hashes comes first (R7.6 criterion 6).
    println!("Lock_File sha256:{}", lock_hash.to_hex());
    for file in &report.lock_mismatches {
        println!("lock-mismatch {file}");
    }
    for (prover, velocity) in &report.calibrations {
        println!("calibration {prover} {velocity}");
    }
    for e in &report.entries {
        let goal = format!("{} {}.{} [{}]", e.file, e.theory, e.goal, e.path);
        match &e.outcome {
            Outcome::Confirmed { prover, micros } => println!("confirmed {goal} {prover} {micros}us"),
            Outcome::NotConfirmed(reason) => println!("not-confirmed {goal}: {reason}"),
            Outcome::Contradicted { prover } => println!("contradicted {goal}: {prover} answers Invalid"),
            Outcome::NotProved { label } => println!("not-proved {goal} {label}"),
        }
    }
    println!(
        "dp-replay: {} confirmed, {} not confirmed, {} contradicted, {} without a proof",
        report.count(|o| matches!(o, Outcome::Confirmed { .. })),
        report.count(|o| matches!(o, Outcome::NotConfirmed(_))),
        report.count(|o| matches!(o, Outcome::Contradicted { .. })),
        report.count(|o| matches!(o, Outcome::NotProved { .. })),
    );
    ExitCode::from(report.exit_status() as u8)
}
