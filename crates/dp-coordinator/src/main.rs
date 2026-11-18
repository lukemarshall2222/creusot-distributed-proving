//! `dp-coordinator --listen ADDRESS`: queues Jobs, assigns them to Workers
//! and forwards Results (design C5, D-1). Untrusted.

use std::{
    collections::BTreeMap,
    io::Write,
    net::{TcpListener, TcpStream},
    process::ExitCode,
    sync::mpsc,
    thread,
    time::Duration,
};

use dp_coordinator::machine::{Input, Peer, State};
use dp_formats::sexp;
use dp_shell::net::{Connection, Event};

const USAGE: &str = "usage:
  dp-coordinator --listen HOST:PORT
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
    let [_, address] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let listener = match TcpListener::bind(address) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("dp-coordinator: {address}: {e}");
            return ExitCode::from(2);
        }
    };
    eprintln!("dp-coordinator: listening on {}", listener.local_addr().expect("bound"));

    // Every connection has a thread that turns what arrives into inputs; one
    // loop owns the state and all writing ends.
    let (tx, rx) = mpsc::channel::<(Input, Option<TcpStream>)>();
    thread::spawn(move || {
        let mut next: Peer = 0;
        for stream in listener.incoming().flatten() {
            next += 1;
            let peer = next;
            let tx = tx.clone();
            let Ok(writer) = stream.try_clone() else { continue };
            let Ok(mut connection) = Connection::new(stream) else { continue };
            if tx.send((Input::Connected(peer), Some(writer))).is_err() {
                return;
            }
            thread::spawn(move || loop {
                let input = match connection.poll(Duration::from_secs(3600)) {
                    Event::Message(msg) => Input::Message(peer, msg),
                    Event::Idle => continue,
                    Event::Closed(_) => {
                        let _ = tx.send((Input::Disconnected(peer), None));
                        return;
                    }
                };
                if tx.send((input, None)).is_err() {
                    return;
                }
            });
        }
    });

    let mut state = State::default();
    let mut writers: BTreeMap<Peer, TcpStream> = BTreeMap::new();
    for (input, writer) in rx {
        match (&input, writer) {
            (Input::Connected(peer), Some(writer)) => {
                writers.insert(*peer, writer);
            }
            (Input::Disconnected(peer), _) => {
                writers.remove(peer);
            }
            _ => {}
        }
        let (next, effects) = state.step(input);
        state = next;
        for (peer, msg) in effects {
            if let Some(writer) = writers.get_mut(&peer) {
                // A failed write shows up as a disconnection of that peer.
                let _ = writer.write_all(&sexp::print(&msg.encode()));
            }
        }
    }
    ExitCode::SUCCESS
}
