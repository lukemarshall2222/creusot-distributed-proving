//! Client of the why3find bridge: runs why3find in dispatch mode as a child
//! process and exchanges `dispatch` messages with it over a pipe (design
//! decisions D-2 and D-3). There is no FFI.

use std::{
    collections::VecDeque,
    ffi::OsString,
    io::{self, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::Duration,
};

use dp_formats::{
    dispatch::{DriverAnswer, FromWhy3find, ProverInfo, ToWhy3find},
    sexp::{self, Prefix},
};

/// A failure of the bridge. Every one of them ends the Session.
#[derive(Debug)]
pub enum BridgeError {
    Io(io::Error),
    /// why3find sent bytes that are not a message of the `dispatch` format.
    Protocol(String),
    /// why3find closed the pipe before the expected message.
    Closed,
    /// why3find answered a request with an error.
    Refused(String),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::Io(e) => write!(f, "why3find bridge: {e}"),
            BridgeError::Protocol(m) => write!(f, "why3find bridge: protocol error: {m}"),
            BridgeError::Closed => write!(f, "why3find bridge: why3find closed the pipe"),
            BridgeError::Refused(m) => write!(f, "why3find bridge: request refused: {m}"),
        }
    }
}

impl std::error::Error for BridgeError {}

impl From<io::Error> for BridgeError {
    fn from(e: io::Error) -> Self {
        BridgeError::Io(e)
    }
}

/// What `Bridge::poll` found.
#[derive(Debug)]
pub enum Poll {
    Message(FromWhy3find),
    /// No message arrived within the timeout.
    Idle,
    /// why3find closed its end of the pipe; it has exited or is exiting.
    Closed,
}

/// The result of a print request: the number of tasks that the
/// Transformation_Pipeline yields and, when it is one, the Printed_Goal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Printed {
    pub tasks: u32,
    pub bytes: Vec<u8>,
}

/// Answers the messages on which why3find blocks: `cache-get` and
/// `cache-set`. why3find waits for these answers before it does anything
/// else, including answering a request, so they are answered as soon as they
/// are read, also while a request is open.
pub type Responder = Box<dyn FnMut(&FromWhy3find) -> Option<ToWhy3find>>;

/// A running why3find in dispatch mode.
pub struct Bridge {
    responder: Responder,
    child: Child,
    stdin: Option<ChildStdin>,
    incoming: Receiver<Result<FromWhy3find, BridgeError>>,
    /// Messages read while waiting for the reply to a request.
    deferred: VecDeque<FromWhy3find>,
    next_qid: u64,
    closed: bool,
}

impl Bridge {
    /// Starts `why3find prove --dispatch ARGS` in `dir`.
    ///
    /// why3find's regular output goes to its standard error, which the child
    /// inherits. `limit` is the size limit of one message.
    pub fn start(
        why3find: &Path,
        args: &[OsString],
        dir: &Path,
        limit: usize,
        responder: Responder,
    ) -> Result<Bridge, BridgeError> {
        let mut child = Command::new(why3find)
            .arg("prove")
            .arg("--dispatch")
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take().expect("piped stdout");
        let (tx, incoming) = mpsc::channel();
        // The reader thread only parses and forwards; every decision is made
        // by the thread that owns the Bridge.
        thread::spawn(move || {
            let mut buf: Vec<u8> = Vec::new();
            let mut chunk = vec![0u8; 1 << 16];
            loop {
                loop {
                    match sexp::parse_prefix(&buf, limit) {
                        Ok(Prefix::Incomplete) => break,
                        Ok(Prefix::Value(value, used)) => {
                            buf.drain(..used);
                            let msg = FromWhy3find::decode(&value)
                                .map_err(|e| BridgeError::Protocol(e.to_string()));
                            let failed = msg.is_err();
                            if tx.send(msg).is_err() || failed {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(BridgeError::Protocol(e.to_string())));
                            return;
                        }
                    }
                }
                match stdout.read(&mut chunk) {
                    Ok(0) => {
                        if !buf.is_empty() {
                            let _ = tx.send(Err(BridgeError::Protocol(
                                "pipe closed inside a message".into(),
                            )));
                        }
                        return;
                    }
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => {
                        let _ = tx.send(Err(BridgeError::Io(e)));
                        return;
                    }
                }
            }
        });
        Ok(Bridge { responder, child, stdin, incoming, deferred: VecDeque::new(), next_qid: 0, closed: false })
    }

    /// Sends one message.
    pub fn send(&mut self, msg: &ToWhy3find) -> Result<(), BridgeError> {
        let stdin = self.stdin.as_mut().ok_or(BridgeError::Closed)?;
        stdin.write_all(&sexp::print(&msg.encode()))?;
        stdin.flush()?;
        Ok(())
    }

    /// The next message, waiting at most `timeout` for it.
    pub fn poll(&mut self, timeout: Duration) -> Result<Poll, BridgeError> {
        if let Some(msg) = self.deferred.pop_front() {
            return Ok(Poll::Message(msg));
        }
        self.receive(timeout)
    }

    fn receive(&mut self, timeout: Duration) -> Result<Poll, BridgeError> {
        if self.closed {
            return Ok(Poll::Closed);
        }
        match self.incoming.recv_timeout(timeout) {
            Ok(Ok(msg)) => match (self.responder)(&msg) {
                Some(reply) => {
                    self.send(&reply)?;
                    Ok(Poll::Idle)
                }
                None => Ok(Poll::Message(msg)),
            },
            Ok(Err(e)) => Err(e),
            Err(RecvTimeoutError::Timeout) => Ok(Poll::Idle),
            Err(RecvTimeoutError::Disconnected) => {
                self.closed = true;
                Ok(Poll::Closed)
            }
        }
    }

    fn request(
        &mut self,
        make: impl FnOnce(String) -> ToWhy3find,
    ) -> Result<FromWhy3find, BridgeError> {
        self.next_qid += 1;
        let qid = format!("q{}", self.next_qid);
        self.send(&make(qid.clone()))?;
        loop {
            match self.receive(Duration::from_secs(3600))? {
                Poll::Closed => return Err(BridgeError::Closed),
                Poll::Idle => {}
                Poll::Message(msg) => {
                    let reply_to = match &msg {
                        FromWhy3find::Printed { qid, .. }
                        | FromWhy3find::Task { qid, .. }
                        | FromWhy3find::Provers { qid, .. }
                        | FromWhy3find::Calibration { qid, .. }
                        | FromWhy3find::AnswerRead { qid, .. }
                        | FromWhy3find::Error { qid, .. } => Some(qid),
                        _ => None,
                    };
                    if reply_to == Some(&qid) {
                        return match msg {
                            FromWhy3find::Error { message, .. } => {
                                Err(BridgeError::Refused(message))
                            }
                            msg => Ok(msg),
                        };
                    }
                    // why3find keeps working while it answers; keep its other
                    // messages in order for `poll`.
                    self.deferred.push_back(msg);
                }
            }
        }
    }

    /// Prints the Goal `gid` with the driver of `prover` after the given
    /// transformations (R1.3 criteria 3 and 10).
    pub fn print(
        &mut self,
        gid: &str,
        prover: &str,
        transformations: &[String],
    ) -> Result<Printed, BridgeError> {
        let reply = self.request(|qid| ToWhy3find::Print {
            qid,
            gid: gid.to_string(),
            prover: prover.to_string(),
            transformations: transformations.to_vec(),
        })?;
        match reply {
            FromWhy3find::Printed { tasks, bytes, .. } => Ok(Printed { tasks, bytes }),
            other => Err(BridgeError::Protocol(format!("unexpected reply {other:?}"))),
        }
    }

    /// The print of the untransformed task of the Goal `gid`.
    pub fn task_print(&mut self, gid: &str) -> Result<Vec<u8>, BridgeError> {
        match self.request(|qid| ToWhy3find::TaskPrint { qid, gid: gid.to_string() })? {
            FromWhy3find::Task { task, .. } => Ok(task),
            other => Err(BridgeError::Protocol(format!("unexpected reply {other:?}"))),
        }
    }

    /// The provers of the project with their drivers and command lines.
    pub fn provers(&mut self) -> Result<Vec<ProverInfo>, BridgeError> {
        match self.request(|qid| ToWhy3find::Provers { qid })? {
            FromWhy3find::Provers { provers, .. } => Ok(provers),
            other => Err(BridgeError::Protocol(format!("unexpected reply {other:?}"))),
        }
    }

    /// why3find's Calibration of `prover`: local time over reference time,
    /// in millionths.
    pub fn calibration(&mut self, prover: &str) -> Result<u64, BridgeError> {
        match self.request(|qid| ToWhy3find::Calibration { qid, prover: prover.to_string() })? {
            FromWhy3find::Calibration { millionths, .. } => Ok(millionths),
            other => Err(BridgeError::Protocol(format!("unexpected reply {other:?}"))),
        }
    }

    /// The answer that the pinned driver of `prover` reads from the output
    /// and exit status of a prover run. Why3 itself reads it.
    pub fn read_answer(
        &mut self,
        prover: &str,
        status: u32,
        output: &[u8],
    ) -> Result<DriverAnswer, BridgeError> {
        let reply = self.request(|qid| ToWhy3find::ReadAnswer {
            qid,
            prover: prover.to_string(),
            status,
            output: output.to_vec(),
        })?;
        match reply {
            FromWhy3find::AnswerRead { answer, .. } => Ok(answer),
            other => Err(BridgeError::Protocol(format!("unexpected reply {other:?}"))),
        }
    }

    /// Closes the pipe to why3find. In dispatch mode why3find treats this as
    /// fatal and exits without writing its proof files.
    pub fn close(&mut self) {
        self.stdin = None;
    }

    /// Waits for why3find to exit.
    pub fn wait(mut self) -> Result<ExitStatus, BridgeError> {
        self.stdin = None;
        Ok(self.child.wait()?)
    }
}
