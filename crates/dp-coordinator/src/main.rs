//! `dp-coordinator --listen ADDRESS`: queues Jobs, assigns them to Workers
//! and forwards Results (design C5, D-1). Untrusted.

use std::{io::Write, process::ExitCode, time::Duration};

use dp_coordinator::shell::Shell;
use dp_core::coord::Config;
use dp_shell::{
    env::OsEnv,
    trace::{Retention, TraceWriter},
};

const USAGE: &str = "usage:
  dp-coordinator --listen HOST:PORT [--silence-seconds S] [--beat-seconds S] [--retries N] [--trace FILE]
      [--with-parent yes]   stop when the process that started it is gone
  dp-coordinator operator-keygen KEY_FILE
      writes a new operator signing key and prints the verification key
  dp-coordinator sign-registry KEY_FILE VERSION WORKER=VERIFICATION_KEY...
      prints a Worker registry signed with the operator signing key";

/// The operator's offline tooling (design decision D-9). It is here for the
/// first distributed slice; it needs no running Coordinator.
fn operator(args: &[String]) -> Result<(), String> {
    use dp_formats::cluster::{from_hex, to_hex, Registry};
    use ed25519_dalek::SigningKey;
    match args {
        [cmd, path] if cmd == "operator-keygen" => {
            let mut secret = [0u8; 32];
            let mut random = std::fs::File::open("/dev/urandom").map_err(|e| e.to_string())?;
            std::io::Read::read_exact(&mut random, &mut secret).map_err(|e| e.to_string())?;
            std::fs::write(path, to_hex(&secret) + "\n").map_err(|e| format!("{path}: {e}"))?;
            println!("{}", to_hex(SigningKey::from_bytes(&secret).verifying_key().as_bytes()));
            Ok(())
        }
        [cmd, path, version, workers @ ..] if cmd == "sign-registry" => {
            let secret = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let secret = from_hex::<32>(&secret).ok_or("the key file is not 64 hex digits")?;
            let registry = Registry {
                version: version.parse().map_err(|_| "VERSION is not a number")?,
                workers: workers
                    .iter()
                    .map(|w| {
                        let (id, key) = w.split_once('=').ok_or(USAGE)?;
                        Ok((id.to_string(), from_hex::<32>(key).ok_or("a verification key is 64 hex digits")?))
                    })
                    .collect::<Result<_, String>>()?,
            };
            std::io::stdout().write_all(&registry.sign(&SigningKey::from_bytes(&secret))).map_err(|e| e.to_string())
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a != "--listen") {
        return match operator(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("dp-coordinator: {message}");
                ExitCode::from(2)
            }
        };
    }
    let mut options = std::collections::BTreeMap::new();
    for pair in args.chunks(2) {
        match pair {
            [flag, value] if flag.starts_with("--") => options.insert(flag.as_str(), value.clone()),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        };
    }
    let seconds = |name: &str, default: f64| -> u64 {
        let value = options.get(name).and_then(|v| v.parse::<f64>().ok()).unwrap_or(default);
        (value * 1e6) as u64
    };
    // Working values, not proposals for <TBD-5>, <TBD-7> and <TBD-31>.
    let config = Config {
        silence_limit: seconds("--silence-seconds", 10.0),
        beat_period: seconds("--beat-seconds", 2.0),
        retries: options.get("--retries").and_then(|v| v.parse().ok()).unwrap_or(2),
    };
    let Some(address) = options.get("--listen") else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let started = OsEnv::new("dp-coordinator").and_then(|mut env| {
        let bound = env.listen(address)?;
        let trace = match options.get("--trace") {
            Some(path) => TraceWriter::to_file("coordinator", path.into(), Retention::Complete)?,
            None => TraceWriter::off("coordinator"),
        };
        Ok((Shell::new(env, config, trace), bound))
    });
    let (mut shell, bound) = match started {
        Ok(started) => started,
        Err(e) => {
            eprintln!("dp-coordinator: {address}: {e}");
            return ExitCode::from(2);
        }
    };
    eprintln!("dp-coordinator: listening on {bound}");
    // For tests and scripts: a Coordinator whose parent process is gone
    // stops, so an aborted test leaves nothing running.
    let with_parent = options.get("--with-parent").is_some_and(|v| v == "yes");
    loop {
        if with_parent && std::os::unix::process::parent_id() == 1 {
            return ExitCode::SUCCESS;
        }
        if let Err(e) = shell.turn(Duration::from_millis(50)) {
            eprintln!("dp-coordinator: {e}");
            return ExitCode::from(1);
        }
    }
}
