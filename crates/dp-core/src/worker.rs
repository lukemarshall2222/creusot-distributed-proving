//! The Worker machine (design C6): which Job a Worker runs, when it stops
//! it, and what it tells the Coordinator.
//!
//! A Worker runs one Job at a time. The Coordinator is the only assigner, so
//! a new assignment replaces the running Job: a cancellation that was lost
//! on the way does no harm. A Worker that does not hear from the Coordinator
//! stops its Job and connects again (R1.5 criterion 15, R1.6).

use creusot_std::prelude::{Clone, DeepModel, PartialEq, *};

use crate::{
    machine::{LogEntry, Machine, Step},
    trace::TlaAction,
};

pub type Time = u64;
pub type JobId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct Config {
    /// Without a message from the Coordinator for longer than this, the
    /// Worker takes the connection as lost.
    pub silence_limit: Time,
    /// How often the Worker sends a heartbeat.
    pub beat_period: Time,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct State {
    pub config: Config,
    /// Whether the Worker has a connection on which it said hello.
    pub connected: bool,
    /// The Job whose solver runs.
    pub running: Option<JobId>,
    /// When the Coordinator was last heard from.
    pub heard: Time,
    /// When the last heartbeat was sent.
    pub beat: Time,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Input {
    /// The connection is up and the offer was sent.
    Connected { now: Time },
    Assign { now: Time, job: JobId },
    Cancel { now: Time, job: JobId },
    Heartbeat { now: Time },
    /// The solver of the Job ended, by itself or at its time limit.
    SolverExit { now: Time, job: JobId },
    /// The shell could not start the solver of the Job.
    StartFailed { now: Time, job: JobId },
    Disconnected { now: Time },
    Tick { now: Time },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Effect {
    /// Start the solver of the Job.
    Start { job: JobId },
    /// End the process tree of the Job's solver; no Result is sent for it.
    Kill { job: JobId },
    /// Sign and send the Result of the run that just ended.
    SendResult { job: JobId },
    /// Tell the Coordinator that the Job was not run to its end.
    SendFailed { job: JobId },
    /// Send a heartbeat that says whether a Job runs.
    Beat { idle: bool },
    /// Close the connection and connect again.
    Reconnect,
}

/// The effects of a step stop a Job only if it ran before the step, and
/// start a Job only if it runs after it.
#[logic(open)]
pub fn consistent(before: State, after: State, effects: Seq<Effect>) -> bool {
    pearlite! {
        forall<i> 0 <= i && i < effects.len() ==> match effects[i] {
            Effect::Start { job } => after.running == Some(job),
            Effect::Kill { job } => before.running == Some(job),
            Effect::SendResult { job } => before.running == Some(job) && after.running == None,
            _ => true,
        }
    }
}

#[ensures(result.state == state && result.effects == effects)]
fn done(state: State, effects: Vec<Effect>, action: TlaAction) -> Step<State, Effect> {
    Step { state, effects, action, log: Vec::new() }
}

#[ensures(result.effects@.len() == 0 && result.state == state)]
fn discard(state: State) -> Step<State, Effect> {
    let mut log = Vec::new();
    log.push(LogEntry::Discarded);
    Step { state, effects: Vec::new(), action: TlaAction::Discard, log }
}

/// One step of a Worker.
#[ensures(consistent(state, result.state, result.effects@))]
// A Worker that is not connected runs nothing after any step but a
// connection or an assignment.
#[ensures(match input {
    Input::Disconnected { .. } => result.state.running == None && !result.state.connected,
    _ => true,
})]
pub fn step(state: State, input: Input) -> Step<State, Effect> {
    let mut s = state;
    let mut effects = Vec::new();
    match input {
        Input::Connected { now } => {
            s.connected = true;
            s.heard = now;
            s.beat = now;
            done(s, effects, TlaAction::WorkerJoined)
        }
        Input::Assign { now, job } => {
            if !s.connected {
                return discard(s);
            }
            s.heard = now;
            match s.running {
                Some(old) => {
                    // The same assignment again: nothing to do.
                    if old == job {
                        return done(s, effects, TlaAction::Heartbeat);
                    }
                    effects.push(Effect::Kill { job: old });
                }
                None => {}
            }
            s.running = Some(job);
            effects.push(Effect::Start { job });
            done(s, effects, TlaAction::Assign)
        }
        Input::Cancel { now, job } => {
            if !s.connected {
                return discard(s);
            }
            s.heard = now;
            match s.running {
                Some(old) => {
                    if old == job {
                        s.running = None;
                        effects.push(Effect::Kill { job });
                        effects.push(Effect::SendFailed { job });
                    }
                }
                None => {}
            }
            done(s, effects, TlaAction::CancelCall)
        }
        Input::Heartbeat { now } => {
            if !s.connected {
                return discard(s);
            }
            s.heard = now;
            done(s, effects, TlaAction::Heartbeat)
        }
        Input::SolverExit { job, .. } => match s.running {
            Some(old) => {
                if old != job {
                    return discard(s);
                }
                s.running = None;
                effects.push(Effect::SendResult { job });
                done(s, effects, TlaAction::LocalExit)
            }
            None => discard(s),
        },
        Input::StartFailed { job, .. } => match s.running {
            Some(old) => {
                if old != job {
                    return discard(s);
                }
                s.running = None;
                effects.push(Effect::SendFailed { job });
                done(s, effects, TlaAction::JobNotRun)
            }
            None => discard(s),
        },
        Input::Disconnected { .. } => {
            match s.running {
                Some(old) => effects.push(Effect::Kill { job: old }),
                None => {}
            }
            s.running = None;
            s.connected = false;
            done(s, effects, TlaAction::PeerLost)
        }
        Input::Tick { now } => {
            if !s.connected {
                return discard(s);
            }
            if now > s.heard && now - s.heard > s.config.silence_limit {
                // Nobody will take the Result of an orphaned Job.
                match s.running {
                    Some(old) => effects.push(Effect::Kill { job: old }),
                    None => {}
                }
                s.running = None;
                s.connected = false;
                effects.push(Effect::Reconnect);
                return done(s, effects, TlaAction::PeerLost);
            }
            if now >= s.beat && now - s.beat >= s.config.beat_period {
                s.beat = now;
                effects.push(Effect::Beat { idle: s.running.is_none() });
            }
            done(s, effects, TlaAction::Tick)
        }
    }
}

/// The Worker machine.
pub struct Worker;

impl Machine for Worker {
    type Config = Config;
    type State = State;
    type Input = Input;
    type Effect = Effect;

    fn init(cfg: &Config) -> State {
        State { config: *cfg, connected: false, running: None, heard: 0, beat: 0 }
    }

    fn step(s: State, i: Input) -> Step<State, Effect> {
        step(s, i)
    }
}
