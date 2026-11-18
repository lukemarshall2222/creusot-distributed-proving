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
    PrintFacts,
    CacheFacts,
    ProverCall,
    LocalExit,
    FinalWrite,
    /// The input changed nothing.
    Discard,
}

impl TlaAction {
    /// Every action, in the order of `verification/tla/actions.toml`.
    pub const ALL: [TlaAction; 8] = [
        TlaAction::SessionStart,
        TlaAction::GoalAnnounced,
        TlaAction::PrintFacts,
        TlaAction::CacheFacts,
        TlaAction::ProverCall,
        TlaAction::LocalExit,
        TlaAction::FinalWrite,
        TlaAction::Discard,
    ];

    /// The name of the action in the TLA+ specification.
    pub fn name(self) -> &'static str {
        match self {
            TlaAction::SessionStart => "SessionStart",
            TlaAction::GoalAnnounced => "GoalAnnounced",
            TlaAction::PrintFacts => "PrintFacts",
            TlaAction::CacheFacts => "CacheFacts",
            TlaAction::ProverCall => "ProverCall",
            TlaAction::LocalExit => "LocalExit",
            TlaAction::FinalWrite => "FinalWrite",
            TlaAction::Discard => "Discard",
        }
    }
}
