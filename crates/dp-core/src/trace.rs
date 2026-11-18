//! Names of the TLA+ actions that steps of the machines are (R1.5
//! criterion 10).
//!
//! `verification/tla/Coordinator.tla` does not exist yet (task 14). The
//! actions below are the Front_End actions of stage 1 and are provisional
//! until that specification names them.

use creusot_std::prelude::{Clone, DeepModel, PartialEq};

/// The action of the TLA+ specification that one step is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum TlaAction {
    SessionStart,
    GoalAnnounced,
    ProverCall,
    PrintFacts,
    CacheHit,
    LocalExit,
    RemoteResult,
    JobNotRun,
    CancelCall,
    ClusterLost,
    Tick,
    FinalWrite,
    // Coordinator and Worker actions.
    FrontEndJoined,
    WorkerJoined,
    Submit,
    Assign,
    Forward,
    Heartbeat,
    PeerLost,
    /// The input changed nothing.
    Discard,
}

impl TlaAction {
    /// Every action, in the order of `verification/tla/actions.toml`.
    pub const ALL: [TlaAction; 20] = [
        TlaAction::SessionStart,
        TlaAction::GoalAnnounced,
        TlaAction::ProverCall,
        TlaAction::PrintFacts,
        TlaAction::CacheHit,
        TlaAction::LocalExit,
        TlaAction::RemoteResult,
        TlaAction::JobNotRun,
        TlaAction::CancelCall,
        TlaAction::ClusterLost,
        TlaAction::Tick,
        TlaAction::FinalWrite,
        TlaAction::FrontEndJoined,
        TlaAction::WorkerJoined,
        TlaAction::Submit,
        TlaAction::Assign,
        TlaAction::Forward,
        TlaAction::Heartbeat,
        TlaAction::PeerLost,
        TlaAction::Discard,
    ];

    /// The name of the action in the TLA+ specification.
    pub fn name(self) -> &'static str {
        match self {
            TlaAction::SessionStart => "SessionStart",
            TlaAction::GoalAnnounced => "GoalAnnounced",
            TlaAction::ProverCall => "ProverCall",
            TlaAction::PrintFacts => "PrintFacts",
            TlaAction::CacheHit => "CacheHit",
            TlaAction::LocalExit => "LocalExit",
            TlaAction::RemoteResult => "RemoteResult",
            TlaAction::JobNotRun => "JobNotRun",
            TlaAction::CancelCall => "CancelCall",
            TlaAction::ClusterLost => "ClusterLost",
            TlaAction::Tick => "Tick",
            TlaAction::FinalWrite => "FinalWrite",
            TlaAction::FrontEndJoined => "FrontEndJoined",
            TlaAction::WorkerJoined => "WorkerJoined",
            TlaAction::Submit => "Submit",
            TlaAction::Assign => "Assign",
            TlaAction::Forward => "Forward",
            TlaAction::Heartbeat => "Heartbeat",
            TlaAction::PeerLost => "PeerLost",
            TlaAction::Discard => "Discard",
        }
    }
}
