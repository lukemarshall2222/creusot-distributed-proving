//! Skeleton of the Front_End machine: the Scheduler and the Verdict_Recorder
//! (design C4). It tracks the Session phase and the announced Goals; the
//! decision functions of later tasks (escalation, verdict, cache) attach to
//! its steps.

use creusot_std::prelude::{Clone, DeepModel, PartialEq, *};

use crate::{
    machine::{LogEntry, Machine, Step},
    trace::TlaAction,
};

/// A clock reading of the shell, in milliseconds.
pub type Time = u64;

/// The identifier that why3find gives a Goal when it announces it.
pub type GoalId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Phase {
    Idle,
    Running,
    Finished,
}

/// State of the Front_End machine. `goals` is strictly increasing (the type
/// invariant below), so it has one order whatever the order of the
/// announcements.
#[derive(Clone, Debug, PartialEq, Eq, DeepModel)]
pub struct State {
    pub phase: Phase,
    pub goals: Vec<GoalId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Input {
    SessionStart { now: Time },
    GoalAnnounced { now: Time, goal: GoalId },
    PrintFacts { now: Time, goal: GoalId },
    CacheFacts { now: Time, goal: GoalId },
    ProverCall { now: Time, goal: GoalId },
    LocalExit { now: Time, goal: GoalId },
    FinalWrite { now: Time },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Effect {
    PrintRequest { goal: GoalId },
    CacheLookup { goal: GoalId },
    LocalStart { goal: GoalId },
    ReplyToWhy3find { goal: GoalId },
    WriteProofJson,
}

/// `s` is strictly increasing.
#[logic(open)]
pub fn increasing(s: Seq<GoalId>) -> bool {
    pearlite! { forall<i, j> 0 <= i && i < j && j < s.len() ==> s[i]@ < s[j]@ }
}

impl Invariant for State {
    #[logic(open)]
    fn invariant(self) -> bool {
        pearlite! { increasing(self.goals@) }
    }
}

/// `g` occurs in `s`.
#[logic(open)]
pub fn member(s: Seq<GoalId>, g: GoalId) -> bool {
    pearlite! { exists<i> 0 <= i && i < s.len() && s[i] == g }
}

/// Position of `goal` in `goals`: `Ok` when it is there, and otherwise `Err`
/// with the position that keeps `goals` increasing.
#[requires(increasing(goals@))]
#[ensures(match result {
    Ok(k) => k@ < goals@.len() && goals[k@] == goal,
    Err(k) => k@ <= goals@.len() && !member(goals@, goal)
        && (forall<i> 0 <= i && i < k@ ==> goals[i]@ < goal@)
        && (forall<i> k@ <= i && i < goals@.len() ==> goal@ < goals[i]@),
})]
pub fn position(goals: &Vec<GoalId>, goal: GoalId) -> Result<usize, usize> {
    let mut k = 0;
    #[invariant(k@ <= goals@.len())]
    #[invariant(forall<i> 0 <= i && i < k@ ==> goals[i]@ < goal@)]
    while k < goals.len() {
        if goals[k] == goal {
            return Ok(k);
        }
        if goals[k] > goal {
            return Err(k);
        }
        k += 1;
    }
    Err(k)
}

/// Inserts `goal` at position `k`, which `position` found.
#[requires(increasing(goals@))]
#[requires(k@ <= goals@.len())]
#[requires(forall<i> 0 <= i && i < k@ ==> goals[i]@ < goal@)]
#[requires(forall<i> k@ <= i && i < goals@.len() ==> goal@ < goals[i]@)]
#[ensures(increasing(result@))]
#[ensures(member(result@, goal))]
#[ensures(forall<g> member(goals@, g) ==> member(result@, g))]
fn insert_goal(goals: Vec<GoalId>, k: usize, goal: GoalId) -> Vec<GoalId> {
    let _old = snapshot! { goals };
    let mut goals = goals;
    goals.insert(k, goal);
    proof_assert! { goals[k@] == goal };
    proof_assert! { forall<i> 0 <= i && i < k@ ==> goals[i] == _old[i] };
    proof_assert! { forall<i> k@ <= i && i < _old@.len() ==> goals[i + 1] == _old[i] };
    goals
}

#[ensures(result.state == state && result.effects@.len() == 0
    && result.action == TlaAction::Discard)]
fn discard(state: State) -> Step<State, Effect> {
    let mut log = Vec::new();
    log.push(LogEntry::Discarded);
    Step { state, effects: Vec::new(), action: TlaAction::Discard, log }
}

#[ensures(result.state == state)]
fn one(state: State, effect: Effect, action: TlaAction) -> Step<State, Effect> {
    let mut effects = Vec::new();
    effects.push(effect);
    Step { state, effects, action, log: Vec::new() }
}

/// One step of the Front_End machine.
///
/// The announced Goals stay strictly increasing (the type invariant of
/// `State`), no step removes a Goal, and a finished Session accepts nothing
/// more.
#[ensures(forall<g> member(s.goals@, g) ==> member(result.state.goals@, g))]
#[ensures(s.phase == Phase::Finished
    ==> result.state == s && result.effects@.len() == 0
        && result.action == TlaAction::Discard)]
pub fn step(s: State, i: Input) -> Step<State, Effect> {
    match (s.phase, i) {
        (Phase::Idle, Input::SessionStart { .. }) => Step {
            state: State { phase: Phase::Running, goals: s.goals },
            effects: Vec::new(),
            action: TlaAction::SessionStart,
            log: Vec::new(),
        },
        (Phase::Running, Input::GoalAnnounced { goal, .. }) => match position(&s.goals, goal) {
            Ok(_) => discard(s),
            Err(k) => {
                let goals = insert_goal(s.goals, k, goal);
                one(
                    State { phase: Phase::Running, goals },
                    Effect::PrintRequest { goal },
                    TlaAction::GoalAnnounced,
                )
            }
        },
        (Phase::Running, Input::PrintFacts { goal, .. }) => match position(&s.goals, goal) {
            Ok(_) => one(s, Effect::CacheLookup { goal }, TlaAction::PrintFacts),
            Err(_) => discard(s),
        },
        (Phase::Running, Input::CacheFacts { goal, .. }) => match position(&s.goals, goal) {
            Ok(_) => Step {
                state: s,
                effects: Vec::new(),
                action: TlaAction::CacheFacts,
                log: Vec::new(),
            },
            Err(_) => discard(s),
        },
        (Phase::Running, Input::ProverCall { goal, .. }) => match position(&s.goals, goal) {
            Ok(_) => one(s, Effect::LocalStart { goal }, TlaAction::ProverCall),
            Err(_) => discard(s),
        },
        (Phase::Running, Input::LocalExit { goal, .. }) => match position(&s.goals, goal) {
            Ok(_) => one(s, Effect::ReplyToWhy3find { goal }, TlaAction::LocalExit),
            Err(_) => discard(s),
        },
        (Phase::Running, Input::FinalWrite { .. }) => one(
            State { phase: Phase::Finished, goals: s.goals },
            Effect::WriteProofJson,
            TlaAction::FinalWrite,
        ),
        _ => discard(s),
    }
}

/// The Front_End machine.
pub struct FrontEnd;

impl Machine for FrontEnd {
    type Config = ();
    type State = State;
    type Input = Input;
    type Effect = Effect;

    fn init(_cfg: &()) -> State {
        State { phase: Phase::Idle, goals: Vec::new() }
    }

    fn step(s: State, i: Input) -> Step<State, Effect> {
        step(s, i)
    }
}
