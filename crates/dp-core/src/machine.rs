//! The machine interface that every state machine of dp-core implements
//! (design: "Machine interface").

use crate::{labels::Outcome, trace::TlaAction, verdict::Acceptance};
#[allow(unused_imports)] // Creusot's derives need the prelude in scope
use creusot_std::prelude::{Clone, DeepModel, PartialEq, *};

/// A deterministic state machine. The I/O shell computes facts, passes them
/// in as fields of an input, and carries out the effects that a step returns.
pub trait Machine {
    type Config;
    type State: Clone + Eq;
    /// One event: a parsed message with shell-computed facts, a timer tick or
    /// a process exit. Every input carries the shell's clock reading.
    type Input;
    /// One I/O request for the shell.
    type Effect;

    fn init(cfg: &Self::Config) -> Self::State;
    fn step(s: Self::State, i: Self::Input) -> Step<Self::State, Self::Effect>;
}

/// The result of one step.
pub struct Step<S, E> {
    pub state: S,
    pub effects: Vec<E>,
    /// The one TLA+ action that this step is (R1.5 criterion 10).
    pub action: TlaAction,
    /// Discards, rejections with a reason category, and warnings.
    pub log: Vec<LogEntry>,
}

/// A log entry of a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum LogEntry {
    /// The input did not apply in the current state and changed nothing.
    Discarded,
    /// A run of the call was accepted and labelled with a proof label.
    Proved { call: u64 },
    /// The pinned checker accepted a certificate for the call's Goal.
    Certified { call: u64 },
    /// A run of the call was accepted and labelled with a non-proof outcome.
    Lost { call: u64, outcome: Outcome },
    /// A run of the call was not accepted, for this reason (R1.3).
    Rejected { call: u64, reason: Acceptance },
}
