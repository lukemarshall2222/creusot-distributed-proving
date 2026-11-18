//! `dp-worker`: runs Solver_Configurations on Jobs and signs its Results
//! (design C6, D-16). Untrusted, except as the TCB_Document lists for
//! proved-standalone Results (NG-7).

use std::{collections::BTreeMap, fs, process::ExitCode, time::Duration};

use dp_core::worker;
use dp_formats::{
    cluster::{from_hex, to_hex},
    lockfile::{current_platform, Kind, LockFile},
};
use dp_shell::env::OsEnv;
use dp_worker::shell::{Config, Shell};
use ed25519_dalek::SigningKey;

const USAGE: &str = "usage:
  dp-worker keygen KEY_FILE
      writes a new secret key and prints the verification key
  dp-worker run --coordinator HOST:PORT --id ID --key KEY_FILE --why3-conf FILE --lock FILE
      [--why3find PATH]   runs why3find's Calibration of every prover first
      [--silence-seconds S] [--beat-seconds S] [--reconnect-seconds S]
      [--with-parent yes]   stop when the process that started it is gone";

fn keygen(path: &str) -> Result<(), String> {
    let mut secret = [0u8; 32];
    let mut random = fs::File::open("/dev/urandom").map_err(|e| e.to_string())?;
    std::io::Read::read_exact(&mut random, &mut secret).map_err(|e| e.to_string())?;
    let key = SigningKey::from_bytes(&secret);
    fs::write(path, to_hex(&secret) + "\n").map_err(|e| format!("{path}: {e}"))?;
    println!("{}", to_hex(key.verifying_key().as_bytes()));
    Ok(())
}

/// why3find's Calibration of every prover of this host, through the
/// why3find bridge (design decision D-3): a scratch project with one goal
/// keeps `why3find prove --dispatch` open while the requests are answered.
fn calibrate(why3find: &str, provers: &[String]) -> Result<BTreeMap<String, u64>, String> {
    use dp_formats::dispatch::{FromWhy3find, ProverResult, ToWhy3find};
    use dp_shell::why3find::{Bridge, Poll};
    let dir = std::env::temp_dir().join(format!("dp-worker-calibration-{}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = (|| {
        fs::write(dir.join("c.mlw"), "module C\n  use int.Int\n  goal G: forall x y: int. x + y = y + x\nend\n")
            .map_err(|e| e.to_string())?;
        let names: Vec<String> = provers.iter().map(|p| format!("{p:?}")).collect();
        fs::write(
            dir.join("why3find.json"),
            format!(
                r#"{{ "fast": 0.0, "time": 1.0, "depth": 1, "packages": [], "provers": [ {} ], "tactics": [], "drivers": [], "warnoff": [] }}"#,
                names.join(", ")
            ),
        )
        .map_err(|e| e.to_string())?;
        let responder = |msg: &FromWhy3find| match msg {
            FromWhy3find::CacheGet { rid, .. } => {
                Some(ToWhy3find::Cache { rid: rid.clone(), result: ProverResult::NoResult })
            }
            FromWhy3find::CacheSet { rid, .. } => Some(ToWhy3find::Stored { rid: rid.clone(), store: false }),
            _ => None,
        };
        let mut bridge = Bridge::start(
            std::path::Path::new(why3find),
            &["c.mlw".into()],
            &dir,
            1 << 24,
            Box::new(responder),
        )
        .map_err(|e| e.to_string())?;
        // Wait for the first prover call: from then on requests are served.
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        loop {
            match bridge.poll(Duration::from_millis(100)).map_err(|e| e.to_string())? {
                Poll::Message(FromWhy3find::Prove { .. }) => break,
                Poll::Message(_) => {}
                Poll::Idle if std::time::Instant::now() < deadline => {}
                Poll::Idle | Poll::Closed => return Err("why3find made no prover call".to_string()),
            }
        }
        let mut velocities = BTreeMap::new();
        for info in bridge.provers().map_err(|e| e.to_string())? {
            let velocity = bridge.calibration(&info.prover).map_err(|e| e.to_string())?;
            velocities.insert(info.prover, velocity.max(1));
        }
        // Closing the pipe ends why3find without a proof file.
        drop(bridge);
        Ok(velocities)
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

fn run(args: &[String]) -> Result<(), String> {
    let mut options = BTreeMap::new();
    for pair in args.chunks(2) {
        match pair {
            [flag, value] if flag.starts_with("--") => options.insert(flag.as_str(), value.clone()),
            _ => return Err(USAGE.into()),
        };
    }
    let get = |name: &str| options.get(name).cloned().ok_or_else(|| USAGE.to_string());
    let read = |path: String| fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"));
    let secret = from_hex::<32>(&read(get("--key")?)?).ok_or("the key file is not 64 hex digits")?;
    let commands = dp_worker::provers::parse(&read(get("--why3-conf")?)?);
    let lock = LockFile::parse(&read(get("--lock")?)?).map_err(|e| e.to_string())?;
    let platform = current_platform();
    let pinned = lock
        .pins(Kind::UnpatchedSolver, &platform)
        .chain(lock.pins(Kind::SharingSolver, &platform))
        .filter_map(|(tool, hash)| Some((tool.prover.clone()?, hash)))
        .collect();
    // Without why3find a Worker offers every prover at the reference speed.
    let velocities = match options.get("--why3find") {
        Some(why3find) => {
            // The provers as why3find names them in a project configuration.
            let mut names: Vec<String> =
                commands.keys().map(|p| p.split_once('@').map_or(p.as_str(), |(n, _)| n).to_string()).collect();
            names.dedup();
            calibrate(why3find, &names).map_err(|e| format!("Calibration: {e}"))?
        }
        None => BTreeMap::new(),
    };
    let seconds = |name: &str, default: f64| -> u64 {
        (options.get(name).and_then(|v| v.parse::<f64>().ok()).unwrap_or(default) * 1e6) as u64
    };
    let id = get("--id")?;
    let address = get("--coordinator")?;
    let config = Config {
        id: id.clone(),
        key: SigningKey::from_bytes(&secret),
        address: address.clone(),
        commands,
        velocities,
        pinned,
        // Working values, not proposals for <TBD-5>, <TBD-7> and <TBD-31>.
        machine: worker::Config {
            silence_limit: seconds("--silence-seconds", 10.0),
            beat_period: seconds("--beat-seconds", 2.0),
        },
        reconnect_micros: seconds("--reconnect-seconds", 1.0),
    };
    let offers: Vec<String> = config
        .commands
        .keys()
        .map(|p| format!("{p} x{:.2}", config.velocities.get(p).copied().unwrap_or(1_000_000) as f64 / 1e6))
        .collect();
    let env = OsEnv::new("dp-worker").map_err(|e| e.to_string())?;
    let mut shell = Shell::new(env, config);
    let mut announced = 0;
    // For tests and scripts: a Worker whose parent process is gone stops.
    let with_parent = options.get("--with-parent").is_some_and(|v| v == "yes");
    loop {
        if with_parent && std::os::unix::process::parent_id() == 1 {
            return Ok(());
        }
        shell.turn(Duration::from_millis(5)).map_err(|e| e.to_string())?;
        if shell.connections > announced {
            announced = shell.connections;
            println!("dp-worker: {id} connected to {address}, offering {}", offers.join(", "));
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((cmd, [path])) if cmd == "keygen" => keygen(path),
        Some((cmd, rest)) if cmd == "run" => run(rest),
        _ => Err(USAGE.into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("dp-worker: {message}");
            ExitCode::from(2)
        }
    }
}
