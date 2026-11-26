// Feature: creusot-distributed-proving, Property 25: Certificates stay inside the supported fragment
// P-ID: P5.2.11, P5.3.1

//! `fragment_ok` over generated command lists: it holds exactly when every
//! command is one of the four proof commands and every step names a
//! pinned rule; one banned command anywhere fails the whole list.

use dp_core::fragment::{command_ok, fragment_ok, Command, CommandKind};
use proptest::prelude::*;

const KINDS: [CommandKind; 5] =
    [CommandKind::Assume, CommandKind::Step, CommandKind::AssumePush, CommandKind::StepPop, CommandKind::Other];

fn command() -> impl Strategy<Value = Command> {
    (prop::sample::select(KINDS.to_vec()), any::<bool>()).prop_map(|(kind, rule_pinned)| Command { kind, rule_pinned })
}

/// A command of the fragment.
fn good() -> impl Strategy<Value = Command> {
    prop_oneof![
        any::<bool>().prop_map(|rule_pinned| Command { kind: CommandKind::Assume, rule_pinned }),
        any::<bool>().prop_map(|rule_pinned| Command { kind: CommandKind::AssumePush, rule_pinned }),
        Just(Command { kind: CommandKind::Step, rule_pinned: true }),
        Just(Command { kind: CommandKind::StepPop, rule_pinned: true }),
    ]
}

/// A command outside it.
fn banned() -> impl Strategy<Value = Command> {
    prop_oneof![
        any::<bool>().prop_map(|rule_pinned| Command { kind: CommandKind::Other, rule_pinned }),
        Just(Command { kind: CommandKind::Step, rule_pinned: false }),
        Just(Command { kind: CommandKind::StepPop, rule_pinned: false }),
    ]
}

/// The allow-list of R5.2, restated without dp-core.
fn allowed(c: &Command) -> bool {
    match c.kind {
        CommandKind::Assume | CommandKind::AssumePush => true,
        CommandKind::Step | CommandKind::StepPop => c.rule_pinned,
        CommandKind::Other => false,
    }
}

proptest! {
    #![proptest_config(dp_testkit::config::cases("prop_fragment", 1000))]

    #[test]
    fn the_check_holds_exactly_when_every_command_is_allowed(commands in proptest::collection::vec(command(), 0..40)) {
        prop_assert_eq!(fragment_ok(&commands), commands.iter().all(allowed));
        for c in &commands {
            prop_assert_eq!(command_ok(*c), allowed(c));
        }
    }

    #[test]
    fn a_list_of_good_commands_passes(commands in proptest::collection::vec(good(), 0..40)) {
        prop_assert!(fragment_ok(&commands));
    }

    // One banned command, wherever it stands, fails the list.
    #[test]
    fn one_banned_command_anywhere_fails_the_list(
        commands in proptest::collection::vec(good(), 0..40),
        bad in banned(),
        at in any::<prop::sample::Index>(),
    ) {
        let mut commands = commands;
        let at = at.index(commands.len() + 1);
        commands.insert(at, bad);
        prop_assert!(!fragment_ok(&commands));
        // Without it the list passes again.
        commands.remove(at);
        prop_assert!(fragment_ok(&commands));
    }
}

#[test]
fn every_kind_is_decided() {
    for kind in KINDS {
        for rule_pinned in [false, true] {
            let c = Command { kind, rule_pinned };
            assert_eq!(command_ok(c), allowed(&c), "{c:?}");
            assert_eq!(fragment_ok(&[c]), allowed(&c), "{c:?}");
        }
    }
    assert!(fragment_ok(&[]));
}
