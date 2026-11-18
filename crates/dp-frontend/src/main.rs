//! `dp-frontend prove ARGS`: proves like `why3find prove ARGS`, with every
//! prover call decided by the Front_End (design C3, D-4).

use std::{path::PathBuf, process::ExitCode};

use dp_formats::lockfile::LockFile;
use dp_formats::cluster::{from_hex, Registry};
use dp_frontend::session::{self, Cluster, Config};
use dp_shell::trace::Retention;

fn fail(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("dp-frontend: {message}");
    ExitCode::from(2)
}

/// The cluster option (R1.1): `DP_CLUSTER` names the Coordinator, and then
/// `DP_REGISTRY` (the operator-signed Worker registry) and `DP_OPERATOR_KEY`
/// (the operator's verification key, in hexadecimal) are required. A
/// malformed value stops the Session before any solver starts.
fn cluster_from_env() -> Result<Option<Cluster>, String> {
    let Some(address) = std::env::var_os("DP_CLUSTER") else { return Ok(None) };
    let address = address.into_string().map_err(|_| "DP_CLUSTER is not text")?;
    let address = address.strip_prefix("tcp://").unwrap_or(&address).to_string();
    let port = address.rsplit_once(':').and_then(|(host, port)| {
        (!host.is_empty()).then(|| port.parse::<u16>().ok().filter(|p| *p != 0)).flatten()
    });
    if port.is_none() {
        return Err(format!("DP_CLUSTER: {address:?} is not HOST:PORT"));
    }
    let key = std::env::var("DP_OPERATOR_KEY").map_err(|_| "DP_CLUSTER needs DP_OPERATOR_KEY")?;
    let key = from_hex::<32>(&key)
        .and_then(|k| ed25519_dalek::VerifyingKey::from_bytes(&k).ok())
        .ok_or("DP_OPERATOR_KEY is not a verification key (64 hex digits)")?;
    let path = std::env::var_os("DP_REGISTRY").ok_or("DP_CLUSTER needs DP_REGISTRY")?;
    let file = std::fs::read(&path).map_err(|e| format!("{}: {e}", PathBuf::from(&path).display()))?;
    let registry = Registry::verify(&file, &key, 1 << 20).map_err(|e| format!("DP_REGISTRY: {e}"))?;
    Ok(Some(Cluster { address, registry }))
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
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get() / 2).max(1);
    let cluster = match cluster_from_env() {
        Ok(cluster) => cluster,
        Err(e) => return fail(e),
    };
    // Seconds; a working default, not a proposal for <TBD-3>.
    let job_margin = std::env::var("DP_JOB_MARGIN_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map_or(std::time::Duration::from_secs(30), std::time::Duration::from_secs_f64);
    let config = Config {
        why3find: why3find.into(),
        lock,
        args: args.collect(),
        dir,
        jobs,
        cluster,
        job_margin,
        record: std::env::var_os("DP_RECORD").map(PathBuf::from),
        // Portfolios come from `dp.toml` (R2.1), which does not exist yet.
        portfolio_seeds: std::env::var("DP_PORTFOLIO_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(1),
        export: std::env::var_os("DP_EXPORT_DIR").map(PathBuf::from),
        // `DP_TRACE` names the trace file. The complete retention setting is
        // `DP_TRACE_RETENTION=complete`; otherwise the most recent records
        // are kept (1000 is a working value, not a proposal for <TBD-46>).
        lock_hash,
        trace: std::env::var_os("DP_TRACE").map(|path| {
            let complete = std::env::var("DP_TRACE_RETENTION").is_ok_and(|v| v == "complete");
            (PathBuf::from(path), if complete { Retention::Complete } else { Retention::Bounded(1000) })
        }),
    };
    match session::run(config) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(e) => fail(e),
    }
}
