//! The supported fragment of a certificate (R5.2 criteria 15 and 16, R5.3
//! criterion 19; design: "Supported fragment").
//!
//! A certificate is a list of proof commands. The checker takes a proof
//! file as a script, and some commands of a script change what the checker
//! accepts: one that declares a rule adds a rule, and a proof that states
//! its own rule proves anything. So a certificate goes to the checker only
//! if every command of it is one of the four proof commands, and every
//! step names a rule of the pinned signature.
//!
//! The shell reads the commands of a proof and looks each rule up in the
//! pinned signature files; this module decides.

use creusot_std::prelude::{Clone, DeepModel, PartialEq, *};

/// What a proof command is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum CommandKind {
    /// `assume`: an assertion of the reference.
    Assume,
    /// `step`: a rule applied to earlier steps.
    Step,
    /// `assume-push`: an assumption that opens a scope.
    AssumePush,
    /// `step-pop`: a rule that closes a scope.
    StepPop,
    /// Any other command, and anything that is no command.
    Other,
}

/// One command of a certificate, as the shell read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct Command {
    pub kind: CommandKind,
    /// The command names a rule, and the pinned signature declares a rule
    /// of that name. Read for the two kinds of step only.
    pub rule_pinned: bool,
}

/// A command of the supported fragment of CPC (R5.2).
#[logic(open)]
pub fn allowed(c: Command) -> bool {
    pearlite! {
        match c.kind {
            CommandKind::Assume => true,
            CommandKind::AssumePush => true,
            CommandKind::Step => c.rule_pinned,
            CommandKind::StepPop => c.rule_pinned,
            CommandKind::Other => false,
        }
    }
}

/// Whether one command is in the supported fragment.
#[ensures(result == allowed(c))]
pub fn command_ok(c: Command) -> bool {
    match c.kind {
        CommandKind::Assume | CommandKind::AssumePush => true,
        CommandKind::Step | CommandKind::StepPop => c.rule_pinned,
        CommandKind::Other => false,
    }
}

/// The supported-fragment check (P5.2.11): every command is allowed. A
/// certificate that fails it is not given to the checker, and its Result
/// stays unchecked with the reason banned command (R5.3 criterion 19).
#[ensures(result == forall<i> 0 <= i && i < commands@.len() ==> allowed(commands[i]))]
pub fn fragment_ok(commands: &[Command]) -> bool {
    let mut i = 0;
    #[invariant(i@ <= commands@.len())]
    #[invariant(forall<j> 0 <= j && j < i@ ==> allowed(commands[j]))]
    while i < commands.len() {
        if !command_ok(commands[i]) {
            return false;
        }
        i += 1;
    }
    true
}
