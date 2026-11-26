//! `SimEnv`: a deterministic simulated environment for the shells (design:
//! Testing Strategy, `tests/sim/`).
//!
//! One `World` holds a virtual clock, the network and the solver runs of
//! every node. Each node's shell gets a [`SimEnv`], which implements the
//! same `Env` trait as the production adapter. The network has the fault
//! model of R1.6: messages are delayed, lost, duplicated and, through the
//! delays, reordered; nodes are partitioned and crash. Solvers are
//! scripted: what a run prints and how long it takes is a function of its
//! input. Everything random comes from one seed, so a run replays exactly.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use dp_formats::{cluster::Message, hash::Hash};
use dp_shell::env::{Env, NetEvent, Peer, RunEnd, RunSpec};

pub type Node = u32;

/// The faults of the network, with rates in parts per million.
#[derive(Clone, Copy, Debug)]
pub struct Faults {
    pub loss: u32,
    pub duplicate: u32,
    pub min_delay: u64,
    pub max_delay: u64,
}

impl Faults {
    /// A network that delivers every message once, after a short delay.
    pub fn none() -> Faults {
        Faults { loss: 0, duplicate: 0, min_delay: 100, max_delay: 100 }
    }
}

/// What a scripted solver does on an input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Behaviour {
    /// Time of the run on a machine of reference speed, in microseconds.
    pub micros: u64,
    pub output: Vec<u8>,
    pub status: u32,
}

/// The scripted solver. An input is `<answer> <micros>`: the run prints the
/// answer after that time. Anything else runs for ever.
pub fn script(input: &[u8]) -> Behaviour {
    let text = String::from_utf8_lossy(input);
    let mut words = text.split_whitespace();
    match (words.next(), words.next().and_then(|w| w.parse().ok())) {
        (Some(answer), Some(micros)) => {
            Behaviour { micros, output: format!("{answer}\n").into_bytes(), status: 0 }
        }
        _ => Behaviour { micros: u64::MAX, output: Vec::new(), status: 0 },
    }
}

/// The certificate that the scripted proof producer writes for an input,
/// and that the scripted checker accepts for that input as its reference.
pub fn scripted_proof(input: &[u8]) -> Vec<u8> {
    let mut proof = b"(step proof-of ".to_vec();
    proof.extend_from_slice(input);
    proof.extend_from_slice(b")\n");
    proof
}

struct Connection {
    /// The two ends: node and the peer number that the node knows it by.
    ends: [(Node, Peer); 2],
    open: bool,
}

struct Run {
    end: u64,
    result: RunEnd,
}

/// The hash of every simulated solver binary.
pub const BINARY_HASH: Hash = Hash([0xb1; 32]);

pub struct World {
    now: u64,
    rng: u64,
    seq: u64,
    pub faults: Faults,
    connections: Vec<Connection>,
    next_peer: Peer,
    listeners: BTreeMap<String, Node>,
    /// Deliveries by time and order of sending.
    deliveries: BTreeMap<(u64, u64), (Node, NetEvent)>,
    runs: BTreeMap<(Node, u64), Run>,
    /// Nodes whose messages are dropped in both directions.
    isolated: BTreeSet<Node>,
    down: BTreeSet<Node>,
    /// Speed of a node: local time over reference time, in millionths.
    velocity: BTreeMap<Node, u64>,
    /// Nodes that change every Result that they send (a forging relay).
    pub forging: BTreeSet<Node>,
    /// Workers that sign, with their own key, a Result whose import-set
    /// hash is not the hash of what the Result lists: registered and
    /// authentic, and still not to be believed (R1.3 criterion 7).
    pub import_liars: BTreeMap<Node, [u8; 32]>,
    /// The largest number of solver runs that a node had at once.
    pub most_runs: BTreeMap<Node, usize>,
    /// Every solver run that was started: node, time and input.
    pub started: Vec<(Node, u64, Vec<u8>)>,
    /// Counts of messages by what happened to them.
    pub sent: u64,
    pub lost: u64,
}

impl World {
    pub fn new(seed: u64, faults: Faults) -> Rc<RefCell<World>> {
        Rc::new(RefCell::new(World {
            now: 0,
            rng: seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1,
            seq: 0,
            faults,
            connections: Vec::new(),
            next_peer: 0,
            listeners: BTreeMap::new(),
            deliveries: BTreeMap::new(),
            runs: BTreeMap::new(),
            isolated: BTreeSet::new(),
            down: BTreeSet::new(),
            velocity: BTreeMap::new(),
            forging: BTreeSet::new(),
            import_liars: BTreeMap::new(),
            most_runs: BTreeMap::new(),
            started: Vec::new(),
            sent: 0,
            lost: 0,
        }))
    }

    /// The next pseudo-random number (xorshift64*).
    pub fn random(&mut self) -> u64 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn chance(&mut self, ppm: u32) -> bool {
        ppm > 0 && self.random() % 1_000_000 < u64::from(ppm)
    }

    fn delay(&mut self) -> u64 {
        let Faults { min_delay, max_delay, .. } = self.faults;
        min_delay + self.random() % (max_delay - min_delay + 1)
    }

    fn deliver(&mut self, node: Node, event: NetEvent, at: u64) {
        self.seq += 1;
        self.deliveries.insert((at, self.seq), (node, event));
    }

    pub fn now(&self) -> u64 {
        self.now
    }

    pub fn advance(&mut self, micros: u64) {
        self.now += micros;
    }

    pub fn listen(&mut self, node: Node, address: &str) {
        self.listeners.insert(address.to_string(), node);
    }

    pub fn set_velocity(&mut self, node: Node, velocity: u64) {
        self.velocity.insert(node, velocity);
    }

    /// Cuts a node off: nothing reaches it and nothing leaves it, and
    /// nobody is told.
    pub fn isolate(&mut self, node: Node) {
        self.isolated.insert(node);
    }

    pub fn heal(&mut self, node: Node) {
        self.isolated.remove(&node);
    }

    /// The node stops: its runs and what was on the way to it are gone.
    /// With `noticed`, its peers see their connections close; without, they
    /// see only silence.
    pub fn crash(&mut self, node: Node, noticed: bool) {
        self.down.insert(node);
        self.runs.retain(|(n, _), _| *n != node);
        self.deliveries.retain(|_, (n, _)| *n != node);
        let now = self.now;
        for i in 0..self.connections.len() {
            let ends = self.connections[i].ends;
            if !self.connections[i].open || !ends.iter().any(|(n, _)| *n == node) {
                continue;
            }
            self.connections[i].open = false;
            if noticed {
                let (other, peer) = if ends[0].0 == node { ends[1] } else { ends[0] };
                let at = now + self.delay();
                self.deliver(other, NetEvent::Disconnected(peer), at);
            }
        }
    }

    pub fn restart(&mut self, node: Node) {
        self.down.remove(&node);
    }

    /// Whether something waits for the node at the current time.
    pub fn pending(&self, node: Node) -> bool {
        self.deliveries.iter().take_while(|((at, _), _)| *at <= self.now).any(|(_, (n, _))| *n == node)
    }

    fn cut(&self, a: Node, b: Node) -> bool {
        [a, b].iter().any(|n| self.isolated.contains(n) || self.down.contains(n))
    }
}

/// The environment of one node.
pub struct SimEnv {
    pub world: Rc<RefCell<World>>,
    pub node: Node,
}

impl SimEnv {
    pub fn new(world: &Rc<RefCell<World>>, node: Node) -> SimEnv {
        SimEnv { world: Rc::clone(world), node }
    }
}

impl Env for SimEnv {
    fn now(&self) -> u64 {
        self.world.borrow().now
    }

    fn poll(&mut self, _wait: Duration) -> Option<NetEvent> {
        let mut w = self.world.borrow_mut();
        let now = w.now;
        let key = w
            .deliveries
            .iter()
            .take_while(|((at, _), _)| *at <= now)
            .find(|(_, (node, _))| *node == self.node)
            .map(|(key, _)| *key)?;
        w.deliveries.remove(&key).map(|(_, event)| event)
    }

    fn send(&mut self, peer: Peer, msg: &Message) {
        let mut w = self.world.borrow_mut();
        let Some((other, other_peer)) = w
            .connections
            .iter()
            .filter(|c| c.open)
            .find_map(|c| match c.ends {
                [(n, p), other] if n == self.node && p == peer => Some(other),
                [other, (n, p)] if n == self.node && p == peer => Some(other),
                _ => None,
            })
        else {
            return;
        };
        w.sent += 1;
        let Faults { loss, duplicate, .. } = w.faults;
        if w.cut(self.node, other) || w.chance(loss) {
            w.lost += 1;
            return;
        }
        let msg = match msg {
            Message::Result(signed) if w.forging.contains(&self.node) => {
                let mut signed = signed.clone();
                signed.fields.output = b"unsat\n".to_vec();
                signed.fields.timed_out = false;
                Message::Result(signed)
            }
            Message::Result(signed) if w.import_liars.contains_key(&self.node) => {
                let mut fields = signed.fields.clone();
                fields.imports_hash.0[0] ^= 1;
                let key = ed25519_dalek::SigningKey::from_bytes(&w.import_liars[&self.node]);
                Message::Result(dp_formats::cluster::SignedResult::sign(fields, &key))
            }
            other => other.clone(),
        };
        let copies = if w.chance(duplicate) { 2 } else { 1 };
        for _ in 0..copies {
            let at = w.now + w.delay();
            w.deliver(other, NetEvent::Message(other_peer, msg.clone()), at);
        }
    }

    fn close(&mut self, peer: Peer) {
        let mut w = self.world.borrow_mut();
        for i in 0..w.connections.len() {
            let ends = w.connections[i].ends;
            let Some(mine) = ends.iter().position(|(n, p)| *n == self.node && *p == peer) else { continue };
            if !w.connections[i].open {
                continue;
            }
            w.connections[i].open = false;
            let (other, other_peer) = ends[1 - mine];
            if !w.cut(self.node, other) {
                let at = w.now + w.delay();
                w.deliver(other, NetEvent::Disconnected(other_peer), at);
            }
        }
    }

    fn connect(&mut self, address: &str) -> io::Result<Peer> {
        let mut w = self.world.borrow_mut();
        let refused = || io::Error::new(io::ErrorKind::ConnectionRefused, "no such listener");
        let listener = *w.listeners.get(address).ok_or_else(refused)?;
        if w.cut(self.node, listener) {
            return Err(refused());
        }
        w.next_peer += 2;
        let (mine, theirs) = (w.next_peer - 1, w.next_peer);
        w.connections.push(Connection { ends: [(self.node, mine), (listener, theirs)], open: true });
        let at = w.now + w.delay();
        w.deliver(listener, NetEvent::Connected(theirs), at);
        Ok(mine)
    }

    fn solver_binary(&mut self, program: &str) -> io::Result<(PathBuf, Hash)> {
        Ok((PathBuf::from(program), BINARY_HASH))
    }

    fn start(&mut self, run: u64, _exe: &Path, spec: &RunSpec) -> io::Result<()> {
        let mut w = self.world.borrow_mut();
        let mut behaviour = script(&spec.input);
        if let Some((reference, _)) = &spec.reference {
            // The scripted checker: it accepts exactly the scripted proof
            // of its reference.
            let correct = spec.input == scripted_proof(reference);
            behaviour = Behaviour {
                micros: 2_000,
                output: if correct { b"correct\n".to_vec() } else { b"incorrect\n".to_vec() },
                status: if correct { 0 } else { 1 },
            };
        } else if spec.command.contains("--dump-proofs") && behaviour.output == b"unsat\n" {
            // The scripted proof producer: the answer, then the proof.
            behaviour.output = [b"unsat\n(\n".as_slice(), &scripted_proof(&spec.input), b")\n"].concat();
        }
        let velocity = w.velocity.get(&self.node).copied().unwrap_or(1_000_000);
        let local = u128::from(behaviour.micros) * u128::from(velocity) / 1_000_000;
        let (elapsed, result) = if local > u128::from(spec.micros) {
            (spec.micros, RunEnd { status: None, timed_out: true, output: Vec::new(), elapsed_micros: spec.micros })
        } else {
            let local = local as u64;
            (
                local,
                RunEnd {
                    status: Some(behaviour.status),
                    timed_out: false,
                    output: behaviour.output,
                    elapsed_micros: local,
                },
            )
        };
        let end = w.now + elapsed;
        w.runs.insert((self.node, run), Run { end, result });
        let now = w.now;
        w.started.push((self.node, now, spec.input.clone()));
        let running = w.runs.keys().filter(|(n, _)| *n == self.node).count();
        let most = w.most_runs.entry(self.node).or_insert(0);
        *most = (*most).max(running);
        Ok(())
    }

    fn kill(&mut self, run: u64) {
        self.world.borrow_mut().runs.remove(&(self.node, run));
    }

    fn finished(&mut self) -> io::Result<Vec<(u64, RunEnd)>> {
        let mut w = self.world.borrow_mut();
        let now = w.now;
        let ended: Vec<u64> =
            w.runs.iter().filter(|((n, _), run)| *n == self.node && run.end <= now).map(|((_, id), _)| *id).collect();
        Ok(ended
            .into_iter()
            .map(|id| (id, w.runs.remove(&(self.node, id)).expect("listed").result))
            .collect())
    }
}
