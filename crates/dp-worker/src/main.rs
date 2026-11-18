//! `dp-worker`: runs Solver_Configurations on Jobs and signs its Results
//! (design C6, D-16). Untrusted, except as the TCB_Document lists for
//! proved-standalone Results (NG-7).

use std::{collections::BTreeMap, fs, path::PathBuf, process::ExitCode, time::Duration};

use dp_formats::{
    cluster::{from_hex, to_hex, Job, Message, ResultFields, SignedResult},
    hash,
    lockfile::{current_platform, Kind, LockFile},
};
use dp_shell::{
    net::{Connection, Event},
    solver::{self, Run},
};
use ed25519_dalek::SigningKey;

const USAGE: &str = "usage:
  dp-worker keygen KEY_FILE
      writes a new secret key and prints the verification key
  dp-worker run --coordinator HOST:PORT --id ID --key KEY_FILE --why3-conf FILE --lock FILE";

fn keygen(path: &str) -> Result<(), String> {
    let mut secret = [0u8; 32];
    let mut random = fs::File::open("/dev/urandom").map_err(|e| e.to_string())?;
    std::io::Read::read_exact(&mut random, &mut secret).map_err(|e| e.to_string())?;
    let key = SigningKey::from_bytes(&secret);
    fs::write(path, to_hex(&secret) + "\n").map_err(|e| format!("{path}: {e}"))?;
    println!("{}", to_hex(key.verifying_key().as_bytes()));
    Ok(())
}

struct Worker {
    id: String,
    key: SigningKey,
    commands: BTreeMap<String, String>,
    lock: LockFile,
    platform: String,
    tmp: PathBuf,
    connection: Connection,
    running: Option<(Job, Run, hash::Hash)>,
    seq: u64,
}

impl Worker {
    /// Starts the solver of `job`, or says why this Worker does not run it.
    fn start(&mut self, job: Job) -> Result<(), String> {
        let command = self.commands.get(&job.prover).ok_or("prover not configured")?.clone();
        let program = solver::words(&command).into_iter().next().ok_or("empty prover command")?;
        let exe = solver::find_executable(&program).map_err(|e| format!("{program}: {e}"))?;
        // Hash the binary just before starting it and check it against this
        // Worker's Lock_File; the hash goes into the signed Result.
        let binary_hash = hash::sha256(&fs::read(&exe).map_err(|e| e.to_string())?);
        let name = job.prover.split_once('@').map_or(job.prover.as_str(), |(n, _)| n);
        let pinned = self
            .lock
            .pins(Kind::UnpatchedSolver, &self.platform)
            .chain(self.lock.pins(Kind::SharingSolver, &self.platform))
            .any(|(tool, h)| tool.prover.as_deref() == Some(name) && h == binary_hash);
        if !pinned {
            return Err(format!("the binary of {} does not match the Lock_File", job.prover));
        }
        self.seq += 1;
        // The Job chooses nothing of the command line but the seed.
        let command = match job.seed {
            Some(seed) => solver::with_seed(&command, seed),
            None => command,
        };
        let run = solver::start(
            &exe,
            &command,
            &job.goal,
            &job.extension,
            job.micros,
            &self.tmp,
            &format!("job{}", self.seq),
        )
        .map_err(|e| e.to_string())?;
        self.running = Some((job, run, binary_hash));
        Ok(())
    }

    fn run(&mut self) -> Result<(), String> {
        loop {
            match self.connection.poll(Duration::from_millis(5)) {
                Event::Idle => {}
                Event::Closed(reason) => {
                    if let Some((_, run, _)) = self.running.take() {
                        run.cancel();
                    }
                    return Err(reason.unwrap_or_else(|| "the Coordinator closed the connection".into()));
                }
                Event::Message(Message::Assign(job)) => {
                    if self.running.is_some() {
                        self.send(Message::Failed { job_id: job.job_id, reason: "the Worker is busy".into() })?;
                    } else {
                        let job_id = job.job_id.clone();
                        if let Err(reason) = self.start(job) {
                            self.send(Message::Failed { job_id, reason })?;
                        }
                    }
                }
                Event::Message(Message::Cancel { job_id }) => {
                    if self.running.as_ref().is_some_and(|(job, _, _)| job.job_id == job_id) {
                        let (_, run, _) = self.running.take().expect("checked");
                        run.cancel();
                        self.send(Message::Failed { job_id, reason: "cancelled".into() })?;
                    }
                }
                Event::Message(_) => {}
            }
            let finished = match &mut self.running {
                Some((_, run, _)) => run.poll().map_err(|e| e.to_string())?,
                None => None,
            };
            if let Some(finished) = finished {
                let (job, _, binary_hash) = self.running.take().expect("running");
                let fields = ResultFields {
                    task_hash: hash::task_hash(&job.goal, &job.driver, b""),
                    job_id: job.job_id,
                    prover: job.prover,
                    seed: job.seed,
                    binary_hash,
                    worker: self.id.clone(),
                    status: finished.status,
                    timed_out: finished.timed_out,
                    output: finished.output,
                    elapsed_micros: finished.elapsed.as_micros() as u64,
                };
                self.send(Message::Result(SignedResult::sign(fields, &self.key)))?;
            }
        }
    }

    fn send(&mut self, msg: Message) -> Result<(), String> {
        self.connection.send(&msg).map_err(|e| e.to_string())
    }
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
    let tmp = std::env::temp_dir().join(format!("dp-worker-{}", std::process::id()));
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let address = get("--coordinator")?;
    let mut worker = Worker {
        id: get("--id")?,
        key: SigningKey::from_bytes(&secret),
        commands,
        lock,
        platform: current_platform(),
        connection: Connection::connect(&address).map_err(|e| format!("{address}: {e}"))?,
        tmp: tmp.clone(),
        running: None,
        seq: 0,
    };
    let hello = Message::WorkerHello {
        worker: worker.id.clone(),
        provers: worker.commands.keys().cloned().collect(),
    };
    worker.send(hello)?;
    println!("dp-worker: {} connected to {address}", worker.id);
    let result = worker.run();
    let _ = fs::remove_dir_all(tmp);
    result
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
