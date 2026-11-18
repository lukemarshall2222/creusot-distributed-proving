//! `dp-frontend prove ARGS`: proves like `why3find prove ARGS`, with every
//! prover call decided by the Front_End (design C3, D-4).

use std::{path::PathBuf, process::ExitCode};

use dp_formats::lockfile::LockFile;
use dp_frontend::session::{self, Config};

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
    let lock = match std::fs::read_to_string(&lock_path) {
        Ok(text) => match LockFile::parse(&text) {
            Ok(lock) => lock,
            Err(e) => return fail(e),
        },
        Err(e) => return fail(format!("{}: {e}", PathBuf::from(lock_path).display())),
    };
    let dir = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(e) => return fail(e),
    };
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get() / 2).max(1);
    let config = Config { why3find: why3find.into(), lock, args: args.collect(), dir, jobs };
    match session::run(config) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(e) => fail(e),
    }
}
