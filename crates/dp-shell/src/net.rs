//! A connection that carries cluster messages as canonical S-expressions.
//!
//! The first distributed slice uses plain TCP; the design names ZeroMQ
//! (design: Transport). Nothing that matters for a label relies on the
//! transport: Results are signed end to end and re-checked locally.

use std::{
    io::{self, Read, Write},
    net::TcpStream,
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::Duration,
};

use dp_formats::{
    cluster::Message,
    sexp::{self, Prefix},
};

/// Size limit of one cluster message. A working value, not a proposal for
/// `<TBD-22>`.
pub const MESSAGE_LIMIT: usize = 1 << 28;

/// What `Connection::poll` found.
#[derive(Debug)]
pub enum Event {
    Message(Message),
    /// Nothing arrived within the timeout.
    Idle,
    /// The peer closed the connection, or sent bytes that are not a message.
    /// Either way nothing more is read from it.
    Closed(Option<String>),
}

/// One end of a connection.
pub struct Connection {
    stream: TcpStream,
    incoming: Receiver<Result<Message, String>>,
    closed: bool,
}

impl Connection {
    pub fn new(stream: TcpStream) -> io::Result<Connection> {
        stream.set_nodelay(true)?;
        let mut reader = stream.try_clone()?;
        let (tx, incoming) = mpsc::channel();
        thread::spawn(move || {
            let mut buf: Vec<u8> = Vec::new();
            let mut chunk = vec![0u8; 1 << 16];
            loop {
                loop {
                    match sexp::parse_prefix(&buf, MESSAGE_LIMIT) {
                        Ok(Prefix::Incomplete) => break,
                        Ok(Prefix::Value(value, used)) => {
                            buf.drain(..used);
                            let msg = Message::decode(&value).map_err(|e| e.to_string());
                            let bad = msg.is_err();
                            if tx.send(msg).is_err() || bad {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e.to_string()));
                            return;
                        }
                    }
                }
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            }
        });
        Ok(Connection { stream, incoming, closed: false })
    }

    pub fn connect(address: &str) -> io::Result<Connection> {
        Connection::new(TcpStream::connect(address)?)
    }

    pub fn send(&mut self, msg: &Message) -> io::Result<()> {
        self.stream.write_all(&sexp::print(&msg.encode()))
    }

    pub fn poll(&mut self, timeout: Duration) -> Event {
        if self.closed {
            return Event::Closed(None);
        }
        match self.incoming.recv_timeout(timeout) {
            Ok(Ok(msg)) => Event::Message(msg),
            Ok(Err(e)) => {
                self.closed = true;
                Event::Closed(Some(e))
            }
            Err(RecvTimeoutError::Timeout) => Event::Idle,
            Err(RecvTimeoutError::Disconnected) => {
                self.closed = true;
                Event::Closed(None)
            }
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}
