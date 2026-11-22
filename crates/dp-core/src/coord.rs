//! The Coordinator machine (design C5): queueing, assignment with
//! Calibration-scaled limits, cancellation, heartbeats, and the handling of
//! lost Workers and Front_Ends.
//!
//! The Coordinator is untrusted: it affects when and where a Job runs,
//! never a label. A Result passes through it unread; the shell keeps the
//! bytes and this machine says where they go.
//!
//! Identifiers are numbers that the shell assigns. A peer is a connection.
//! A Job identifier names one Job of one Front_End connection. Times are
//! microseconds on the shell's clock.

use creusot_std::prelude::{Clone, DeepModel, PartialEq, *};

use crate::{
    calibration::scale,
    machine::{LogEntry, Machine, Step},
    trace::TlaAction,
};

pub type Time = u64;
pub type Peer = u64;
pub type JobId = u64;
pub type ProverId = u64;

/// One prover that a Worker offers, with its Calibration on that Worker:
/// local time over reference time, in millionths (R1.1 criterion 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct Offer {
    pub prover: ProverId,
    pub velocity: u64,
}

/// A Job that the Coordinator holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct Job {
    pub id: JobId,
    pub front_end: Peer,
    pub prover: ProverId,
    /// The reference time limit.
    pub micros: u64,
    /// How many Workers that ran it were lost.
    pub lost: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, DeepModel)]
pub struct Worker {
    pub peer: Peer,
    pub offers: Vec<Offer>,
    /// The Job that it runs.
    pub busy: Option<Job>,
    /// When the Coordinator last heard from it.
    pub heard: Time,
    /// The operator drains it: it gets no new Job (R1.7).
    pub draining: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct FrontEnd {
    pub peer: Peer,
    pub heard: Time,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct Config {
    /// A peer that is silent for longer than this is lost (R1.6).
    pub silence_limit: Time,
    /// How often the Coordinator sends a heartbeat to every peer.
    pub beat_period: Time,
    /// How many times a Job is given to another Worker after the Worker
    /// that ran it was lost, before the Job fails.
    pub retries: u64,
    /// The largest serial number of an operator command that an earlier
    /// start of this Coordinator took: none of those is taken again.
    pub serial: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, DeepModel)]
pub struct State {
    pub config: Config,
    pub front_ends: Vec<FrontEnd>,
    /// In the order in which they joined.
    pub workers: Vec<Worker>,
    /// Jobs waiting for a Worker, oldest first.
    pub queue: Vec<Job>,
    /// When the last heartbeats were sent.
    pub beat: Time,
    /// The largest serial number of an operator command that was taken.
    pub serial: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, DeepModel)]
pub enum Input {
    FrontEndHello { now: Time, peer: Peer },
    /// `revoked`: the operator revoked the Worker that the offer names.
    WorkerHello { now: Time, peer: Peer, offers: Vec<Offer>, revoked: bool },
    /// A command with the operator's name on it. `authentic`: it carries
    /// the operator's signature. `worker`: the connection of the Worker
    /// that it names, if that Worker is connected.
    Operator { now: Time, peer: Peer, authentic: bool, serial: u64, operation: Operation, worker: Option<Peer> },
    /// Someone asks for the state of the Cluster.
    Status { now: Time, peer: Peer },
    Submit { now: Time, peer: Peer, job: JobId, prover: ProverId, micros: u64 },
    Cancel { now: Time, peer: Peer, job: JobId },
    /// A Worker's Result for a Job. The shell holds the message.
    Result { now: Time, peer: Peer, job: JobId },
    /// A Worker did not run a Job to its end.
    Failed { now: Time, peer: Peer, job: JobId },
    Heartbeat { now: Time, peer: Peer },
    /// A Worker says that it runs nothing.
    WorkerIdle { now: Time, peer: Peer },
    Disconnected { now: Time, peer: Peer },
    Tick { now: Time },
}

/// What the operator tells the Coordinator about a Worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Operation {
    Drain,
    Remove,
    Revoke,
}

/// Why a Job ended without a Result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Failure {
    /// No connected Worker offers the Job's prover (R1.1 criterion 7).
    NoEligibleWorker,
    /// The Worker reported that it did not run the Job.
    WorkerFailed,
    /// Every Worker that ran the Job was lost, up to the retry limit.
    WorkersLost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Effect {
    /// Send the Job to the Worker with this local time limit.
    Assign { worker: Peer, job: JobId, local_micros: u64, velocity: u64 },
    /// Tell the Worker to stop the Job.
    CancelAt { worker: Peer, job: JobId },
    /// Send the Result that just arrived to the Front_End, unchanged.
    Forward { front_end: Peer, job: JobId },
    /// Tell the Front_End that the Job ended without a Result.
    Fail { front_end: Peer, job: JobId, reason: Failure },
    /// Send a heartbeat.
    Beat { peer: Peer },
    /// Close the connection of a peer that is taken as lost.
    Drop { peer: Peer },
    /// The Job is gone from the Coordinator: the shell forgets its bytes.
    Forget { front_end: Peer, job: JobId },
    /// Tell the sender of an operator command whether it was taken.
    OperatorReply { peer: Peer, serial: u64, taken: bool },
    /// Send the revocation that was just taken to a Front_End, as signed.
    Announce { front_end: Peer },
    /// Send a status report.
    Report { peer: Peer },
}

#[ensures(match result {
    Some(k) => k@ < workers@.len() && workers[k@].peer == peer,
    None => true,
})]
fn find_worker(workers: &Vec<Worker>, peer: Peer) -> Option<usize> {
    let mut k = 0;
    #[invariant(k@ <= workers@.len())]
    while k < workers.len() {
        if workers[k].peer == peer {
            return Some(k);
        }
        k += 1;
    }
    None
}

#[ensures(match result {
    Some(k) => k@ < front_ends@.len() && front_ends[k@].peer == peer,
    None => true,
})]
fn find_front_end(front_ends: &Vec<FrontEnd>, peer: Peer) -> Option<usize> {
    let mut k = 0;
    #[invariant(k@ <= front_ends@.len())]
    while k < front_ends.len() {
        if front_ends[k].peer == peer {
            return Some(k);
        }
        k += 1;
    }
    None
}

#[ensures(match result {
    Some(k) => k@ < queue@.len() && queue[k@].front_end == front_end && queue[k@].id == job,
    None => true,
})]
fn find_queued(queue: &Vec<Job>, front_end: Peer, job: JobId) -> Option<usize> {
    let mut k = 0;
    #[invariant(k@ <= queue@.len())]
    while k < queue.len() {
        if queue[k].front_end == front_end && queue[k].id == job {
            return Some(k);
        }
        k += 1;
    }
    None
}

/// The Worker offers the prover with this Calibration.
#[logic(open)]
pub fn offers(worker: Worker, prover: ProverId, velocity: u64) -> bool {
    pearlite! { exists<i> 0 <= i && i < worker.offers@.len()
        && worker.offers[i].prover == prover && worker.offers[i].velocity == velocity }
}

/// The Calibration with which the Worker offers the prover.
#[ensures(match result {
    Some(v) => offers(*worker, prover, v),
    None => true,
})]
fn offer(worker: &Worker, prover: ProverId) -> Option<u64> {
    let mut k = 0;
    #[invariant(k@ <= worker.offers@.len())]
    while k < worker.offers.len() {
        if worker.offers[k].prover == prover {
            return Some(worker.offers[k].velocity);
        }
        k += 1;
    }
    None
}

/// What the Workers can do for a Job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Pick {
    /// This Worker is idle and offers the prover with this Calibration.
    Idle { index: usize, velocity: u64 },
    /// Some Worker offers the prover, and every such Worker is busy.
    Busy,
    /// No Worker offers the prover.
    Nobody,
}

/// The eligible Worker for a prover: the first one, in joining order, that
/// is idle and offers it (R1.7 criterion 2).
#[ensures(match result {
    Pick::Idle { index, velocity } => index@ < workers@.len()
        && workers[index@].busy == None
        && !workers[index@].draining
        && offers(workers[index@], prover, velocity),
    _ => true,
})]
pub fn pick(workers: &Vec<Worker>, prover: ProverId) -> Pick {
    let mut offered = false;
    let mut k = 0;
    #[invariant(k@ <= workers@.len())]
    while k < workers.len() {
        // A Worker that is being drained offers nothing.
        if !workers[k].draining {
            match offer(&workers[k], prover) {
                Some(velocity) => {
                    if workers[k].busy.is_none() {
                        return Pick::Idle { index: k, velocity };
                    }
                    offered = true;
                }
                None => {}
            }
        }
        k += 1;
    }
    if offered {
        Pick::Busy
    } else {
        Pick::Nobody
    }
}

/// The local time limit of an assignment (R1.4 criterion 1).
#[ensures(result@ == crate::calibration::scaled(job.micros@, velocity@))]
pub fn local_limit(job: Job, velocity: u64) -> u64 {
    scale(job.micros, velocity)
}

/// Gives queued Jobs to idle Workers, oldest Job first. A Job whose prover
/// no Worker offers fails at once.
#[ensures(true)]
fn assign(s: &mut State, effects: &mut Vec<Effect>) {
    let mut i = 0;
    #[invariant(i@ <= s.queue@.len())]
    while i < s.queue.len() {
        let job = s.queue[i];
        match pick(&s.workers, job.prover) {
            Pick::Idle { index, velocity } => {
                s.queue.remove(i);
                s.workers[index].busy = Some(job);
                effects.push(Effect::Assign {
                    worker: s.workers[index].peer,
                    job: job.id,
                    local_micros: local_limit(job, velocity),
                    velocity,
                });
            }
            Pick::Nobody => {
                s.queue.remove(i);
                effects.push(Effect::Fail {
                    front_end: job.front_end,
                    job: job.id,
                    reason: Failure::NoEligibleWorker,
                });
                effects.push(Effect::Forget { front_end: job.front_end, job: job.id });
            }
            Pick::Busy => i += 1,
        }
    }
}

/// The Worker at `k` is lost. Its Job goes back to the head of the queue
/// for another Worker, until the retry limit (R1.6).
#[requires(k@ < s.workers@.len())]
#[ensures((^s).workers@.len() == s.workers@.len() - 1)]
#[ensures((^s).front_ends == s.front_ends && (^s).config == s.config)]
fn lose_worker(s: &mut State, k: usize, effects: &mut Vec<Effect>) {
    let worker = s.workers.remove(k);
    match worker.busy {
        Some(job) => {
            if job.lost < s.config.retries {
                let again = Job { lost: job.lost + 1, ..job };
                s.queue.insert(0, again);
            } else {
                effects.push(Effect::Fail {
                    front_end: job.front_end,
                    job: job.id,
                    reason: Failure::WorkersLost,
                });
                effects.push(Effect::Forget { front_end: job.front_end, job: job.id });
            }
        }
        None => {}
    }
}

/// The Front_End at `k` is lost: its queued Jobs go, and its running Jobs
/// are cancelled (R1.5 criterion 15).
#[requires(k@ < s.front_ends@.len())]
#[ensures((^s).front_ends@.len() == s.front_ends@.len() - 1)]
#[ensures((^s).config == s.config)]
fn lose_front_end(s: &mut State, k: usize, effects: &mut Vec<Effect>) {
    let peer = s.front_ends.remove(k).peer;
    let mut i = 0;
    #[invariant(i@ <= s.queue@.len())]
    while i < s.queue.len() {
        if s.queue[i].front_end == peer {
            let job = s.queue.remove(i);
            effects.push(Effect::Forget { front_end: peer, job: job.id });
        } else {
            i += 1;
        }
    }
    let mut w = 0;
    #[invariant(w@ <= s.workers@.len())]
    while w < s.workers.len() {
        match s.workers[w].busy {
            Some(job) => {
                if job.front_end == peer {
                    effects.push(Effect::CancelAt { worker: s.workers[w].peer, job: job.id });
                    effects.push(Effect::Forget { front_end: peer, job: job.id });
                    s.workers[w].busy = None;
                }
            }
            None => {}
        }
        w += 1;
    }
}

/// Whether a peer last heard at `heard` is silent for too long at `now`.
fn silent(config: Config, heard: Time, now: Time) -> bool {
    now > heard && now - heard > config.silence_limit
}

/// The Worker `peer` runs `job`: marks it idle and returns the Job.
#[ensures(match result {
    Some(j) => j.id == job,
    None => true,
})]
fn finish(s: &mut State, peer: Peer, job: JobId) -> Option<Job> {
    match find_worker(&s.workers, peer) {
        Some(k) => match s.workers[k].busy {
            Some(running) => {
                if running.id == job {
                    s.workers[k].busy = None;
                    Some(running)
                } else {
                    None
                }
            }
            None => None,
        },
        None => None,
    }
}

/// Records that `peer` was heard from at `now`.
#[ensures(true)]
fn heard(s: &mut State, peer: Peer, now: Time) {
    match find_worker(&s.workers, peer) {
        Some(k) => s.workers[k].heard = now,
        None => {}
    }
    match find_front_end(&s.front_ends, peer) {
        Some(k) => s.front_ends[k].heard = now,
        None => {}
    }
}

fn done(state: State, effects: Vec<Effect>, action: TlaAction) -> Step<State, Effect> {
    Step { state, effects, action, log: Vec::new() }
}

#[ensures(result.effects@.len() == 0 && result.state == state)]
fn discard(state: State) -> Step<State, Effect> {
    let mut log = Vec::new();
    log.push(LogEntry::Discarded);
    Step { state, effects: Vec::new(), action: TlaAction::Discard, log }
}

/// One step of the Coordinator.
#[ensures(true)]
pub fn step(state: State, input: Input) -> Step<State, Effect> {
    let mut s = state;
    let mut effects = Vec::new();
    let action = match input {
        Input::FrontEndHello { now, peer } => {
            if find_front_end(&s.front_ends, peer).is_some() || find_worker(&s.workers, peer).is_some() {
                return discard(s);
            }
            s.front_ends.push(FrontEnd { peer, heard: now });
            TlaAction::FrontEndJoined
        }
        Input::WorkerHello { now, peer, offers, revoked } => {
            if find_front_end(&s.front_ends, peer).is_some() {
                return discard(s);
            }
            // A revoked Worker does not join (R1.7 criterion 6).
            if revoked {
                effects.push(Effect::Drop { peer });
                match find_worker(&s.workers, peer) {
                    Some(k) => lose_worker(&mut s, k, &mut effects),
                    None => {}
                }
                assign(&mut s, &mut effects);
                return done(s, effects, TlaAction::PeerLost);
            }
            match find_worker(&s.workers, peer) {
                // A new offer of a Worker that is connected replaces its old one.
                Some(k) => {
                    s.workers[k].offers = offers;
                    s.workers[k].heard = now;
                }
                None => s.workers.push(Worker { peer, offers, busy: None, heard: now, draining: false }),
            }
            TlaAction::WorkerJoined
        }
        Input::Submit { now, peer, job, prover, micros } => {
            // Only a Front_End submits, and a Job is held once.
            if find_front_end(&s.front_ends, peer).is_none()
                || find_queued(&s.queue, peer, job).is_some()
                || running(&s.workers, peer, job)
            {
                return discard(s);
            }
            heard(&mut s, peer, now);
            s.queue.push(Job { id: job, front_end: peer, prover, micros, lost: 0 });
            TlaAction::Submit
        }
        Input::Cancel { now, peer, job } => {
            if find_front_end(&s.front_ends, peer).is_none() {
                return discard(s);
            }
            heard(&mut s, peer, now);
            match find_queued(&s.queue, peer, job) {
                Some(k) => {
                    s.queue.remove(k);
                    effects.push(Effect::Forget { front_end: peer, job });
                }
                None => {}
            }
            let mut w = 0;
            #[invariant(w@ <= s.workers@.len())]
            while w < s.workers.len() {
                match s.workers[w].busy {
                    Some(running) => {
                        if running.front_end == peer && running.id == job {
                            effects.push(Effect::CancelAt { worker: s.workers[w].peer, job });
                            effects.push(Effect::Forget { front_end: peer, job });
                            s.workers[w].busy = None;
                        }
                    }
                    None => {}
                }
                w += 1;
            }
            TlaAction::CancelCall
        }
        // A Result is forwarded only from the Worker that runs the Job, to
        // the Front_End that submitted it.
        Input::Result { now, peer, job } => {
            heard(&mut s, peer, now);
            match finish(&mut s, peer, job) {
                Some(running) => {
                    effects.push(Effect::Forward { front_end: running.front_end, job });
                    effects.push(Effect::Forget { front_end: running.front_end, job });
                }
                None => return discard(s),
            }
            TlaAction::Forward
        }
        Input::Failed { now, peer, job } => {
            heard(&mut s, peer, now);
            match finish(&mut s, peer, job) {
                Some(running) => {
                    effects.push(Effect::Fail {
                        front_end: running.front_end,
                        job,
                        reason: Failure::WorkerFailed,
                    });
                    effects.push(Effect::Forget { front_end: running.front_end, job });
                }
                None => return discard(s),
            }
            TlaAction::JobNotRun
        }
        Input::Heartbeat { now, peer } => {
            heard(&mut s, peer, now);
            TlaAction::Heartbeat
        }
        Input::Status { peer, .. } => {
            effects.push(Effect::Report { peer });
            return done(s, effects, TlaAction::Heartbeat);
        }
        // Only a command that the operator signed, and that is newer than
        // every command taken so far, changes anything (R1.7 criterion 5).
        Input::Operator { peer, authentic, serial, operation, worker, .. } => {
            if !authentic || serial <= s.serial {
                effects.push(Effect::OperatorReply { peer, serial, taken: false });
                return done(s, effects, TlaAction::Discard);
            }
            s.serial = serial;
            effects.push(Effect::OperatorReply { peer, serial, taken: true });
            match operation {
                Operation::Drain => match worker {
                    Some(w) => match find_worker(&s.workers, w) {
                        Some(k) => s.workers[k].draining = true,
                        None => {}
                    },
                    None => {}
                },
                Operation::Remove | Operation::Revoke => match worker {
                    Some(w) => match find_worker(&s.workers, w) {
                        Some(k) => {
                            effects.push(Effect::Drop { peer: w });
                            lose_worker(&mut s, k, &mut effects);
                        }
                        None => {}
                    },
                    None => {}
                },
            }
            match operation {
                // Every Front_End learns of a revocation, whether the
                // Worker is connected or not.
                Operation::Revoke => {
                    let mut f = 0;
                    #[invariant(f@ <= s.front_ends@.len())]
                    while f < s.front_ends.len() {
                        effects.push(Effect::Announce { front_end: s.front_ends[f].peer });
                        f += 1;
                    }
                }
                _ => {}
            }
            TlaAction::Operator
        }
        // The Worker runs nothing, and the Coordinator has a Job for it:
        // the assignment or the Job's end was lost on the way. The Job is
        // assigned again; a Worker that runs it already ignores that.
        Input::WorkerIdle { now, peer } => {
            heard(&mut s, peer, now);
            match find_worker(&s.workers, peer) {
                Some(k) => match s.workers[k].busy {
                    Some(job) => match offer(&s.workers[k], job.prover) {
                        Some(velocity) => effects.push(Effect::Assign {
                            worker: peer,
                            job: job.id,
                            local_micros: local_limit(job, velocity),
                            velocity,
                        }),
                        None => {}
                    },
                    None => {}
                },
                None => {}
            }
            TlaAction::Heartbeat
        }
        Input::Disconnected { peer, .. } => {
            match find_worker(&s.workers, peer) {
                Some(k) => lose_worker(&mut s, k, &mut effects),
                None => {}
            }
            match find_front_end(&s.front_ends, peer) {
                Some(k) => lose_front_end(&mut s, k, &mut effects),
                None => {}
            }
            TlaAction::PeerLost
        }
        Input::Tick { now } => {
            // Silent peers are lost, and their connections are closed, so a
            // peer that is only slow finds out and joins again.
            let mut w = 0;
            #[invariant(w@ <= s.workers@.len())]
            while w < s.workers.len() {
                if silent(s.config, s.workers[w].heard, now) {
                    effects.push(Effect::Drop { peer: s.workers[w].peer });
                    lose_worker(&mut s, w, &mut effects);
                } else {
                    w += 1;
                }
            }
            let mut f = 0;
            #[invariant(f@ <= s.front_ends@.len())]
            while f < s.front_ends.len() {
                if silent(s.config, s.front_ends[f].heard, now) {
                    effects.push(Effect::Drop { peer: s.front_ends[f].peer });
                    lose_front_end(&mut s, f, &mut effects);
                } else {
                    f += 1;
                }
            }
            if now >= s.beat && now - s.beat >= s.config.beat_period {
                s.beat = now;
                let mut w = 0;
                #[invariant(w@ <= s.workers@.len())]
                while w < s.workers.len() {
                    effects.push(Effect::Beat { peer: s.workers[w].peer });
                    w += 1;
                }
                let mut f = 0;
                #[invariant(f@ <= s.front_ends@.len())]
                while f < s.front_ends.len() {
                    effects.push(Effect::Beat { peer: s.front_ends[f].peer });
                    f += 1;
                }
            }
            TlaAction::Tick
        }
    };
    assign(&mut s, &mut effects);
    done(s, effects, action)
}

/// Some Worker runs the Job.
#[ensures(true)]
fn running(workers: &Vec<Worker>, front_end: Peer, job: JobId) -> bool {
    let mut w = 0;
    #[invariant(w@ <= workers@.len())]
    while w < workers.len() {
        match workers[w].busy {
            Some(running) => {
                if running.front_end == front_end && running.id == job {
                    return true;
                }
            }
            None => {}
        }
        w += 1;
    }
    false
}

/// The Coordinator machine.
pub struct Coordinator;

impl Machine for Coordinator {
    type Config = Config;
    type State = State;
    type Input = Input;
    type Effect = Effect;

    fn init(cfg: &Config) -> State {
        State {
            config: *cfg,
            front_ends: Vec::new(),
            workers: Vec::new(),
            queue: Vec::new(),
            beat: 0,
            serial: cfg.serial,
        }
    }

    fn step(s: State, i: Input) -> Step<State, Effect> {
        step(s, i)
    }
}
