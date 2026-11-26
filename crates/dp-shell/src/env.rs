//! The `Env` abstraction (design: "How the shells drive dp-core").
//!
//! A shell of the Coordinator or of a Worker touches the outside world only
//! through an `Env`: the network, the clock, the solver processes and the
//! files that it keeps. The
//! production adapter is [`OsEnv`]; the simulator of `dp-testkit` is
//! another, with a virtual clock, a network that loses, delays and
//! duplicates messages, and scripted solvers. The same shell code runs on
//! both.

use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread,
    time::{Duration, Instant},
};

use dp_formats::{
    cluster::Message,
    hash::{self, Hash},
    sexp,
};

use crate::{
    net::{Connection, Event},
    solver::{self, Run},
    store::{Files, OsFiles},
};

/// Identifies a connection of this process.
pub type Peer = u64;

#[derive(Debug)]
pub enum NetEvent {
    /// A peer connected to the address that this process listens on.
    Connected(Peer),
    Message(Peer, Message),
    /// The connection is gone, or the peer sent bytes that are no message.
    Disconnected(Peer),
}

/// One solver run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSpec {
    /// The command line of the prover configuration.
    pub command: String,
    /// The Printed_Goal.
    pub input: Vec<u8>,
    /// Extension of the prover's input file, with its dot.
    pub extension: String,
    /// The local time limit in microseconds.
    pub micros: u64,
    /// A second input: the reference of a certificate check, with the
    /// extension of its file, which replaces `%r` in the command line.
    pub reference: Option<(Vec<u8>, String)>,
    /// How much of the output is kept; zero: the default of solver runs.
    /// Only standard output is kept then, as for a proof.
    pub output_limit: usize,
}

impl RunSpec {
    /// A solver run on one input.
    pub fn solver(command: String, input: Vec<u8>, extension: String, micros: u64) -> RunSpec {
        RunSpec { command, input, extension, micros, reference: None, output_limit: 0 }
    }
}

/// How a solver run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunEnd {
    pub status: Option<u32>,
    pub timed_out: bool,
    pub output: Vec<u8>,
    pub elapsed_micros: u64,
}

/// The file store of the component is the supertrait `Files`.
pub trait Env: Files {
    /// The clock, in microseconds.
    fn now(&self) -> u64;
    /// The next thing that arrived, waiting for at most `wait`.
    fn poll(&mut self, wait: Duration) -> Option<NetEvent>;
    /// Sends a message. A failure shows up as a disconnection of the peer.
    fn send(&mut self, peer: Peer, msg: &Message);
    fn close(&mut self, peer: Peer);
    fn connect(&mut self, address: &str) -> io::Result<Peer>;
    /// Finds the executable of a solver and hashes it.
    fn solver_binary(&mut self, program: &str) -> io::Result<(PathBuf, Hash)>;
    /// Starts a solver in its own process group.
    fn start(&mut self, run: u64, exe: &Path, spec: &RunSpec) -> io::Result<()>;
    /// Ends the process tree of a run; nothing is reported for it.
    fn kill(&mut self, run: u64);
    /// The runs that ended since the last call.
    fn finished(&mut self) -> io::Result<Vec<(u64, RunEnd)>>;
}

/// The production adapter: TCP, the OS clock and OS processes.
pub struct OsEnv {
    start: Instant,
    tx: Sender<(NetEvent, Option<TcpStream>)>,
    rx: Receiver<(NetEvent, Option<TcpStream>)>,
    writers: BTreeMap<Peer, TcpStream>,
    next_peer: std::sync::Arc<std::sync::atomic::AtomicU64>,
    runs: BTreeMap<u64, Run>,
    tmp: PathBuf,
    /// Hashes of solver binaries by path, with the stamp of the file when
    /// it was hashed.
    hashed: BTreeMap<PathBuf, (FileStamp, Hash)>,
}

/// What identifies the contents of a file between two looks at it without
/// reading it again: device, inode, size and the modification and status
/// change times (TB-17).
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp(u64, u64, u64, i64, i64, i64, i64);

fn file_stamp(path: &Path) -> io::Result<FileStamp> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::metadata(path)?;
    Ok(FileStamp(m.dev(), m.ino(), m.size(), m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec()))
}

/// Turns what arrives on one connection into events.
fn read_loop(peer: Peer, mut connection: Connection, tx: Sender<(NetEvent, Option<TcpStream>)>) {
    thread::spawn(move || loop {
        let event = match connection.poll(Duration::from_secs(3600)) {
            Event::Message(msg) => NetEvent::Message(peer, msg),
            Event::Idle => continue,
            Event::Closed(_) => {
                let _ = tx.send((NetEvent::Disconnected(peer), None));
                return;
            }
        };
        if tx.send((event, None)).is_err() {
            return;
        }
    });
}

impl OsEnv {
    pub fn new(name: &str) -> io::Result<OsEnv> {
        let (tx, rx) = mpsc::channel();
        let tmp = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        fs::create_dir_all(&tmp)?;
        Ok(OsEnv {
            start: Instant::now(),
            tx,
            rx,
            writers: BTreeMap::new(),
            next_peer: Default::default(),
            runs: BTreeMap::new(),
            tmp,
            hashed: BTreeMap::new(),
        })
    }

    fn fresh_peer(next: &std::sync::atomic::AtomicU64) -> Peer {
        next.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
    }

    /// Accepts connections on `address`; returns the bound address.
    pub fn listen(&mut self, address: &str) -> io::Result<std::net::SocketAddr> {
        let listener = TcpListener::bind(address)?;
        let bound = listener.local_addr()?;
        let tx = self.tx.clone();
        let next = self.next_peer.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let peer = OsEnv::fresh_peer(&next);
                let Ok(writer) = stream.try_clone() else { continue };
                let Ok(connection) = Connection::new(stream) else { continue };
                if tx.send((NetEvent::Connected(peer), Some(writer))).is_err() {
                    return;
                }
                read_loop(peer, connection, tx.clone());
            }
        });
        Ok(bound)
    }
}

impl Drop for OsEnv {
    fn drop(&mut self) {
        for (_, run) in std::mem::take(&mut self.runs) {
            run.cancel();
        }
        let _ = fs::remove_dir_all(&self.tmp);
    }
}

impl Env for OsEnv {
    fn now(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }

    fn poll(&mut self, wait: Duration) -> Option<NetEvent> {
        let (event, writer) = match self.rx.recv_timeout(wait) {
            Ok(received) => received,
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return None,
        };
        match (&event, writer) {
            (NetEvent::Connected(peer), Some(writer)) => {
                self.writers.insert(*peer, writer);
            }
            (NetEvent::Disconnected(peer), _) => {
                self.writers.remove(peer);
            }
            _ => {}
        }
        Some(event)
    }

    fn send(&mut self, peer: Peer, msg: &Message) {
        if let Some(writer) = self.writers.get_mut(&peer) {
            let _ = writer.write_all(&sexp::print(&msg.encode()));
        }
    }

    fn close(&mut self, peer: Peer) {
        if let Some(writer) = self.writers.remove(&peer) {
            let _ = writer.shutdown(std::net::Shutdown::Both);
        }
    }

    fn connect(&mut self, address: &str) -> io::Result<Peer> {
        let stream = TcpStream::connect(address)?;
        let writer = stream.try_clone()?;
        let connection = Connection::new(stream)?;
        let peer = OsEnv::fresh_peer(&self.next_peer);
        self.writers.insert(peer, writer);
        read_loop(peer, connection, self.tx.clone());
        Ok(peer)
    }

    fn solver_binary(&mut self, program: &str) -> io::Result<(PathBuf, Hash)> {
        let exe = solver::find_executable(program)?;
        // The binary is read and hashed again only if the file's stamp is
        // not the one seen when it was last hashed.
        let stamp = file_stamp(&exe)?;
        if let Some((seen, hash)) = self.hashed.get(&exe) {
            if *seen == stamp {
                return Ok((exe, *hash));
            }
        }
        let hash = hash::sha256(&fs::read(&exe)?);
        self.hashed.insert(exe.clone(), (stamp, hash));
        Ok((exe, hash))
    }

    fn start(&mut self, run: u64, exe: &Path, spec: &RunSpec) -> io::Result<()> {
        let started = solver::start_with(
            exe,
            &spec.command,
            &spec.input,
            &spec.extension,
            spec.micros.max(1),
            &self.tmp,
            &format!("run{run}"),
            &solver::Extra {
                reference: spec.reference.as_ref().map(|(bytes, extension)| (bytes.as_slice(), extension.as_str())),
                output_limit: spec.output_limit,
                stdout_only: spec.output_limit != 0,
            },
        )?;
        self.runs.insert(run, started);
        Ok(())
    }

    fn kill(&mut self, run: u64) {
        if let Some(run) = self.runs.remove(&run) {
            run.cancel();
        }
    }

    fn finished(&mut self) -> io::Result<Vec<(u64, RunEnd)>> {
        let mut ended = Vec::new();
        let ids: Vec<u64> = self.runs.keys().copied().collect();
        for id in ids {
            if let Some(done) = self.runs.get_mut(&id).expect("listed").poll()? {
                self.runs.remove(&id);
                ended.push((
                    id,
                    RunEnd {
                        status: done.status,
                        timed_out: done.timed_out,
                        output: done.output,
                        elapsed_micros: done.elapsed.as_micros() as u64,
                    },
                ));
            }
        }
        Ok(ended)
    }
}

impl Files for OsEnv {
    fn read_file(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        OsFiles.read_file(path)
    }

    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        OsFiles.write_file(path, bytes)
    }

    fn write_dir(&mut self, dir: &Path, files: &[(&str, &[u8])]) -> io::Result<()> {
        OsFiles.write_dir(dir, files)
    }

    fn append_file(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        OsFiles.append_file(path, bytes)
    }
}
