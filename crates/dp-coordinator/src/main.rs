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
  dp-coordinator --listen HOST:PORT [--silence-seconds S] [--beat-seconds S] [--retries N] [--trace FILE] [--state FILE]
      [--with-parent yes]   stop when the process that started it is gone
  dp-coordinator --listen HOST:PORT --operator-key VERIFICATION_KEY ...
      takes operator commands that are signed with that key
  dp-coordinator status HOST:PORT
      prints the connected Workers and the queue
  dp-coordinator operate KEY_FILE HOST:PORT SERIAL drain|remove|revoke WORKER
      sends a signed operator command; SERIAL must grow with every command
  dp-coordinator operator-keygen KEY_FILE
      writes a new operator signing key and prints the verification key
  dp-coordinator sign-registry KEY_FILE VERSION WORKER=VERIFICATION_KEY...
      prints a Worker registry signed with the operator signing key";

/// The operator's offline tooling (design decision D-9). It is here for the
/// first distributed slice; it needs no running Coordinator.
fn operator(args: &[String]) -> Result<(), String> {
    use dp_formats::cluster::{from_hex, to_hex, Message, Operation, Registry, SignedCommand};
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
        [cmd, address] if cmd == "status" => {
            match request(address, &Message::StatusRequest)? {
                Message::Status { incarnation, workers, queued, front_ends } => {
                    println!(
                        "incarnation {incarnation}: {front_ends} Front_Ends, {queued} queued Jobs, {} Workers",
                        workers.len()
                    );
                    for w in workers {
                        println!(
                            "worker {} {} {} provers: {}",
                            w.worker,
                            if w.draining { "draining" } else { "serving" },
                            w.job.map_or("idle".to_string(), |j| format!("runs {j}")),
                            w.provers.join(" ")
                        );
                    }
                    Ok(())
                }
                other => Err(format!("unexpected answer {other:?}")),
            }
        }
        [cmd, path, address, serial, operation, worker] if cmd == "operate" => {
            let secret = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let secret = from_hex::<32>(&secret).ok_or("the key file is not 64 hex digits")?;
            let serial = serial.parse().map_err(|_| "SERIAL is not a number")?;
            let operation = Operation::parse(operation).ok_or("OPERATION is drain, remove or revoke")?;
            let command = SignedCommand::sign(serial, operation, worker, &SigningKey::from_bytes(&secret));
            match request(address, &Message::Operator(command))? {
                Message::OperatorResult { taken: true, .. } => {
                    println!("taken: {} {worker}", operation.name());
                    Ok(())
                }
                Message::OperatorResult { taken: false, .. } => Err(
                    "refused: the command is not signed by the Coordinator's operator key, or its serial number is not larger than the last one taken".into(),
                ),
                other => Err(format!("unexpected answer {other:?}")),
            }
        }
        _ => Err(USAGE.into()),
    }
}

/// Sends one message to a Coordinator and waits for its answer.
fn request(address: &str, message: &dp_formats::cluster::Message) -> Result<dp_formats::cluster::Message, String> {
    use dp_formats::cluster::Message;
    use dp_shell::env::{Env, NetEvent};
    let mut env = OsEnv::new("dp-coordinator-client").map_err(|e| e.to_string())?;
    let peer = env.connect(address).map_err(|e| format!("{address}: {e}"))?;
    env.send(peer, message);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match env.poll(Duration::from_millis(100)) {
            Some(NetEvent::Message(_, Message::Heartbeat | Message::Welcome { .. })) | None => {}
            Some(NetEvent::Message(_, answer)) => return Ok(answer),
            Some(NetEvent::Disconnected(_)) => return Err("the Coordinator closed the connection".into()),
            Some(NetEvent::Connected(_)) => {}
        }
    }
    Err("no answer from the Coordinator".into())
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
        serial: 0,
    };
    let Some(address) = options.get("--listen") else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let started = OsEnv::new("dp-coordinator").and_then(|mut env| {
        let bound = env.listen(address)?;
        let trace = match options.get("--trace") {
            Some(path) => TraceWriter::to_file(&mut env, "coordinator", path.into(), Retention::Complete)?,
            // On by default: the most recent steps, in memory (D-11).
            None => TraceWriter::in_memory("coordinator", 1000),
        };
        // The operator's verification key: without it no command is taken.
        let operator = match options.get("--operator-key") {
            Some(hex) => Some(
                dp_formats::cluster::from_hex::<32>(hex)
                    .and_then(|k| ed25519_dalek::VerifyingKey::from_bytes(&k).ok())
                    .ok_or_else(|| std::io::Error::other("--operator-key is not a verification key (64 hex digits)"))?,
            ),
            None => None,
        };
        // With a state file the revocations, the last serial number and
        // the incarnation outlive a restart.
        let state_file = options.get("--state").map(std::path::PathBuf::from);
        Ok((Shell::with_state(env, config, trace, operator, state_file)?, bound))
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
