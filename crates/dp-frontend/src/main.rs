//! `dp-frontend prove ARGS`: proves like `why3find prove ARGS`, with every
//! prover call decided by the Front_End (design C3, D-4).
//!
//! Options come from the environment and from `dp.toml` in the project
//! directory (see `config.rs`). An invalid configuration stops the Session
//! before any solver or why3find starts, with exit status 2.

use std::{path::PathBuf, process::ExitCode};

use dp_formats::lockfile::LockFile;
use dp_frontend::{
    config,
    session::{self, Config},
};

fn fail(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("dp-frontend: {message}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("prove")) {
        return fail("usage: dp-frontend prove [ARGUMENTS OF why3find prove]");
    }
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
    let options = match config::load(&dir) {
        Ok(options) => options,
        Err(e) => return fail(format!("configuration: {e}")),
    };
    let config = Config {
        why3find: why3find.into(),
        lock,
        args: args.collect(),
        dir,
        jobs: options.jobs,
        cluster: options.cluster,
        job_margin: options.job_margin,
        silence_limit: options.silence_limit,
        beat_period: options.beat_period,
        // Raw measurements for the evaluation (R6.3); not project options.
        record: std::env::var_os("DP_RECORD").map(PathBuf::from),
        portfolio_seeds: options.portfolio_seeds,
        portfolio: options.portfolio,
        fast_remote: options.fast_remote,
        local_racer: options.local_racer,
        certification: match options.checker {
            // The signature files are hashed once, at the start.
            // The signature files are hashed once, at the start, and the
            // rules that they declare are read then.
            Some(checker) => match checker.signature_hash().and_then(|hash| Ok((hash, checker.rules()?))) {
                Ok((signature_hash, rules)) => Some(session::Certification {
                    ethos: checker.ethos.to_string_lossy().into_owned(),
                    signature: checker.signature,
                    signature_hash,
                    check_micros: options.check_limit.map(|d| d.as_micros() as u64),
                    proof_limit: options.proof_limit,
                    rules: std::sync::Arc::new(rules),
                }),
                Err(e) => return fail(format!("the signature files of the checker: {e}")),
            },
            None => None,
        },
        certify_all: options.certify_all,
        export: std::env::var_os("DP_EXPORT_DIR").map(PathBuf::from),
        trace: options.trace,
        trace_keep: options.trace_keep,
        lock_hash,
        // Corpus measurement: needs DP_RECORD.
        matrix: std::env::var("DP_MATRIX_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|s| *s > 0.0)
            .map(std::time::Duration::from_secs_f64),
    };
    match session::run(config) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(e) => fail(e),
    }
}
