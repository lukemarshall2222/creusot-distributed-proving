//! The Front_End machine: the Scheduler and the Verdict_Recorder (design C4).
//!
//! The shell turns what happens (a message from why3find, a finished solver
//! run, a Result from the Cluster, a tick of the clock) into an `Input` whose
//! fields are facts it computed locally, and carries out the `Effect`s that a
//! step returns. Every decision is taken here: where a prover call runs,
//! whether a Result is accepted, which label it gets, when a race ends and
//! what why3find is told.
//!
//! Identifiers are numbers that the shell assigns: this module compares
//! them and nothing else. Times are microseconds on the shell's clock.

use creusot_std::prelude::{Clone, DeepModel, PartialEq, *};

use crate::{
    labels::{most_informative, Outcome},
    machine::{LogEntry, Machine, Step},
    trace::TlaAction,
    verdict::{
        accept_remote, accept_result, label, label_from_checker_output, record_verdict, Acceptance, CheckFacts,
        Context, Hash, Label, RemoteFacts, ResultFields, UnpatchedSolver, Verdict, Verdicts,
    },
};

/// A clock reading of the shell, in microseconds.
pub type Time = u64;

/// Identifies a Goal that why3find announced.
pub type GoalId = u64;

/// Identifies one prover call of why3find.
pub type CallId = u64;

/// Identifies a prover of the project configuration.
pub type ProverId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Phase {
    Idle,
    Running,
    Finished,
}

/// The step of why3find's strategy that a prover call belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Pass {
    /// The first sequential pass: the Local_Fast_Pass, which stays local
    /// (R1.2).
    Fast,
    /// Any later step: it goes to the Cluster when there is one.
    Later,
}

/// Where a prover call is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Place {
    /// The shell is printing the Goal for the call's prover.
    Printing,
    /// Waiting for a free local solver slot.
    Queued,
    /// A local solver is running.
    Local,
    /// Jobs on the Cluster race for it.
    Remote,
    /// A run reported a proof that needs a certificate; the shell is
    /// getting one and having the pinned checker check it.
    Certifying,
}

/// One prover call of why3find that has not been answered.
#[derive(Clone, Debug, PartialEq, Eq, DeepModel)]
pub struct Call {
    pub rid: CallId,
    pub goal: GoalId,
    pub prover: ProverId,
    pub pass: Pass,
    /// The reference time limit.
    pub micros: u64,
    pub place: Place,
    /// Seeds of the Jobs of its race that have not ended; 0 is the seed of
    /// the prover configuration.
    pub jobs: Vec<u64>,
    /// Outcomes of the Jobs of its race that ended without a proof label.
    pub lost: Vec<Outcome>,
    /// When the Front_End stops waiting for its race.
    pub deadline: Time,
}

/// What does not change during a Session.
#[derive(Clone, Debug, PartialEq, Eq, DeepModel)]
pub struct Config {
    /// Number of solver processes that run at once on the local machine.
    pub local_jobs: u64,
    /// Number of Jobs that race on one prover call on the Cluster (R2.2):
    /// the members of the Portfolio, or the seeds of the call's prover. The
    /// shell knows what member each number stands for. At least one.
    pub seeds: u64,
    /// The first sequential pass also goes to the Cluster, as a race, when
    /// there is one. Without this it stays local (R1.2).
    pub fast_remote: bool,
    /// How long, beyond twice its time limit, the Front_End waits for a race.
    pub job_margin: Time,
    /// Certification is on (R5.1): a reported proof that is not
    /// proved-standalone gets a certificate and a run of the pinned checker.
    pub certify: bool,
    /// Without a message from the Coordinator for longer than this, the
    /// Front_End falls back to local proving (R1.6). Zero: no such check.
    pub silence_limit: Time,
    /// How often the Front_End sends a heartbeat to the Coordinator.
    pub beat_period: Time,
    pub certify_all: bool,
    /// The Unpatched_Solver entries of the Lock_File.
    pub unpatched: Vec<UnpatchedSolver>,
    /// `H(canon([]))`, the import-set hash of no imports.
    pub empty_imports_hash: Hash,
}

/// State of the Front_End machine. `goals` is strictly increasing, so it has
/// one order whatever the order of the announcements.
#[derive(Clone, Debug, PartialEq, Eq, DeepModel)]
pub struct State {
    pub config: Config,
    pub phase: Phase,
    /// The Session uses the Cluster (R1.1) and has not fallen back (R1.6).
    pub cluster: bool,
    pub goals: Vec<GoalId>,
    pub calls: Vec<Call>,
    /// Number of calls whose place is `Local`.
    pub local_running: u64,
    pub verdicts: Verdicts,
    /// When the Coordinator was last heard from.
    pub heard: Time,
    /// When the last heartbeat was sent.
    pub beat: Time,
}

/// What a finished run established, with the facts that the shell computed
/// for it in this Session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct Evidence {
    pub fields: ResultFields,
    /// Task_Hash of `print_local` for the call's Goal and prover.
    pub local_task_hash: Hash,
    /// Number of tasks of that print.
    pub task_count: u64,
    /// The local solver binary differed from the Lock_File when it was
    /// started (R7.7 criterion 7); false for a Result from a Worker.
    pub binary_mismatch: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Input {
    /// `cluster` is whether the Session is connected to a Coordinator.
    SessionStart { now: Time, cluster: bool },
    GoalAnnounced { now: Time, goal: GoalId },
    ProverCall { now: Time, rid: CallId, goal: GoalId, prover: ProverId, pass: Pass, micros: u64 },
    /// The shell printed the Goal of the call: the number of tasks.
    PrintFacts { now: Time, rid: CallId, task_count: u64 },
    /// The Result_Cache holds an entry for the run description of the call,
    /// whose stored lookup fields equal those of the lookup and whose
    /// validation fields passed `validate_entry`. `evidence` is the stored
    /// Result with the facts of the current Session; `remote` is set for an
    /// entry that carries a Worker identity, with the facts about its tag
    /// and the registry in the current Session (R7.5 criterion 4).
    CacheHit { now: Time, rid: CallId, evidence: Evidence, remote: Option<RemoteFacts> },
    /// The local solver run of a call ended.
    LocalRun { now: Time, rid: CallId, evidence: Evidence },
    /// A Result arrived from the Cluster for the Job `seed` of a call.
    /// `prover` is the prover that the Result names; the other facts are
    /// about its tag and the registry.
    RemoteResult {
        now: Time,
        rid: CallId,
        seed: u64,
        prover: ProverId,
        evidence: Evidence,
        signature_valid: bool,
        registered: bool,
        revoked: bool,
        /// The Calibration and the local time limit that the Worker signed.
        velocity: u64,
        local_micros: u64,
    },
    /// The Cluster reported that it did not run a Job.
    JobNotRun { now: Time, rid: CallId, seed: u64 },
    /// why3find withdrew a prover call.
    CancelCall { now: Time, rid: CallId },
    /// The connection to the Coordinator is gone.
    ClusterLost { now: Time },
    /// A message arrived from the Coordinator.
    ClusterHeard { now: Time },
    /// The shell tried to certify the Goal of a call.
    Certificate { now: Time, rid: CallId, facts: CheckFacts },
    Tick { now: Time },
    /// why3find finished.
    Why3findDone { now: Time },
}

/// What why3find is told about a prover call. The shell adds the times.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Reply {
    /// Only for a Result labelled with a proof label.
    Valid,
    Unknown,
    Timeout,
    Failed,
    /// The call was withdrawn.
    NoResult,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Effect {
    /// Print the Goal of the call for its prover, then send `PrintFacts`.
    Print { rid: CallId },
    /// Start the call's solver on the local machine.
    StartLocal { rid: CallId },
    /// Submit a Job of the call to the Cluster.
    Submit { rid: CallId, seed: u64 },
    /// End the call's local solver process tree.
    CancelLocal { rid: CallId },
    /// Cancel a Job on the Cluster.
    CancelJob { rid: CallId, seed: u64 },
    /// Get a certificate for the Goal of the call and have the pinned
    /// checker check it, then send `Certificate`.
    Certify { rid: CallId },
    /// Send a heartbeat to the Coordinator.
    Beat,
    /// Close the connection to the Coordinator.
    LeaveCluster,
    /// Answer why3find. Each call is answered once.
    Answer { rid: CallId, reply: Reply },
}

/// `s` is strictly increasing.
#[logic(open)]
pub fn increasing(s: Seq<GoalId>) -> bool {
    pearlite! { forall<i, j> 0 <= i && i < j && j < s.len() ==> s[i]@ < s[j]@ }
}

/// `g` occurs in `s`.
#[logic(open)]
pub fn member(s: Seq<GoalId>, g: GoalId) -> bool {
    pearlite! { exists<i> 0 <= i && i < s.len() && s[i] == g }
}

impl Invariant for State {
    #[logic(open)]
    fn invariant(self) -> bool {
        pearlite! { increasing(self.goals@) }
    }
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

/// Index of the open call `rid`.
#[ensures(match result {
    Some(k) => k@ < calls@.len() && calls[k@].rid == rid,
    None => true,
})]
fn find_call(calls: &Vec<Call>, rid: CallId) -> Option<usize> {
    let mut k = 0;
    #[invariant(k@ <= calls@.len())]
    while k < calls.len() {
        if calls[k].rid == rid {
            return Some(k);
        }
        k += 1;
    }
    None
}

/// Index of `seed` among the open Jobs of a race.
#[ensures(match result {
    Some(k) => k@ < jobs@.len() && jobs[k@] == seed,
    None => true,
})]
fn find_job(jobs: &Vec<u64>, seed: u64) -> Option<usize> {
    let mut k = 0;
    #[invariant(k@ <= jobs@.len())]
    while k < jobs.len() {
        if jobs[k] == seed {
            return Some(k);
        }
        k += 1;
    }
    None
}

/// `a + b`, or the largest time if that does not fit.
fn add(a: u64, b: u64) -> u64 {
    if a > u64::MAX - b {
        u64::MAX
    } else {
        a + b
    }
}

/// What why3find is told for a non-proof outcome (R7.2 criterion 6).
pub fn reply_of(outcome: Outcome) -> Reply {
    match outcome {
        Outcome::Timeout => Reply::Timeout,
        Outcome::Failed => Reply::Failed,
        _ => Reply::Unknown,
    }
}

/// What the Verdict_Recorder makes of a finished run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Decision {
    /// Accepted and labelled proved-standalone.
    Proved,
    /// Accepted and labelled with a non-proof outcome.
    Lost(Outcome),
    /// Not accepted: it is never labelled (R1.3).
    Discarded(Acceptance),
}

/// The context of a run, as `label` reads it.
fn context(config: Config, evidence: Evidence) -> Context {
    Context {
        certify_all: config.certify_all,
        // No Job carries the import mark before the sharing stages.
        import_mark: false,
        local_task_hash: evidence.local_task_hash,
        task_count: evidence.task_count,
        empty_imports_hash: config.empty_imports_hash,
        local_binary_mismatch: evidence.binary_mismatch,
        unpatched: config.unpatched,
    }
}

/// Accepts or discards a finished run and labels it (R1.3, R7.1).
///
/// The run is `Proved` only if it was accepted and every condition of R7.1
/// criterion 2 holds; this is the only source of a `Valid` reply.
#[ensures(result == Decision::Proved ==> crate::verdict::standalone(evidence.fields, *ctx))]
#[ensures(result == Decision::Proved ==> match remote {
    Some(facts) => facts.signature_valid && facts.registered && !facts.revoked && facts.submitted,
    None => true,
})]
pub fn decide(evidence: Evidence, ctx: &Context, remote: Option<RemoteFacts>) -> Decision {
    let acceptance = match remote {
        Some(facts) => accept_remote(evidence.fields, ctx, facts),
        None => accept_result(evidence.fields, ctx),
    };
    match acceptance {
        Acceptance::Accepted => match label(evidence.fields, ctx) {
            Label::ProvedStandalone => Decision::Proved,
            Label::NonProof(outcome) => Decision::Lost(outcome),
            // Not reachable in this build; never a proof here.
            Label::Certified => Decision::Lost(Outcome::Unchecked),
        },
        other => Decision::Discarded(other),
    }
}

/// Fallback to local proving (R1.6 criteria 4 and 5): the calls that the
/// Cluster was answering run here.
#[ensures((^s).goals == s.goals && (^s).verdicts == s.verdicts && (^s).phase == s.phase)]
fn fall_back(s: &mut State, effects: &mut Vec<Effect>) {
    s.cluster = false;
    let _old = snapshot! { *s };
    let mut k = 0;
    #[invariant(k@ <= s.calls@.len())]
    #[invariant(s.goals == _old.goals && s.verdicts == _old.verdicts && s.phase == _old.phase)]
    while k < s.calls.len() {
        if s.calls[k].place == Place::Remote {
            s.calls[k].place = Place::Queued;
            s.calls[k].jobs.clear();
            s.calls[k].lost.clear();
        }
        k += 1;
    }
    start_queued(s, effects);
}

/// Moves queued calls to free local solver slots, oldest first.
#[ensures((^s).goals == s.goals && (^s).verdicts == s.verdicts && (^s).phase == s.phase)]
fn start_queued(s: &mut State, effects: &mut Vec<Effect>) {
    let _old = snapshot! { *s };
    let mut k = 0;
    #[invariant(k@ <= s.calls@.len())]
    #[invariant(s.goals == _old.goals && s.verdicts == _old.verdicts && s.phase == _old.phase)]
    while k < s.calls.len() && s.local_running < s.config.local_jobs {
        if s.calls[k].place == Place::Queued {
            s.calls[k].place = Place::Local;
            s.local_running += 1;
            effects.push(Effect::StartLocal { rid: s.calls[k].rid });
        }
        k += 1;
    }
}

/// A local solver slot became free.
#[ensures((^s).goals == s.goals && (^s).verdicts == s.verdicts && (^s).phase == s.phase)]
fn release_local(s: &mut State, effects: &mut Vec<Effect>) {
    if s.local_running > 0 {
        s.local_running -= 1;
    }
    start_queued(s, effects);
}

/// Cancels the open Jobs of a race.
// A contract, even a trivial one, is what lets the loop invariant below be
// erased in a build without Creusot.
#[ensures(true)]
fn cancel_jobs(call: &Call, effects: &mut Vec<Effect>) {
    let mut k = 0;
    #[invariant(k@ <= call.jobs@.len())]
    while k < call.jobs.len() {
        effects.push(Effect::CancelJob { rid: call.rid, seed: call.jobs[k] });
        k += 1;
    }
}

/// Ends a race that no Job won. With outcomes, why3find gets the most
/// informative one (R2.2 criterion 14, R7.2 criterion 5). If the Cluster ran
/// none of its Jobs, the call runs on the local machine instead.
#[requires(k@ < s.calls@.len())]
#[ensures((^s).goals == s.goals && (^s).verdicts == s.verdicts && (^s).phase == s.phase)]
fn end_lost_race(s: &mut State, k: usize, effects: &mut Vec<Effect>) {
    if s.calls[k].lost.is_empty() {
        s.calls[k].place = Place::Queued;
        start_queued(s, effects);
    } else {
        let call = s.calls.remove(k);
        let best = most_informative(&call.lost);
        answer_or_certify(s, call, best, effects);
    }
}

/// A call ends with a non-proof outcome. If that outcome is a reported
/// proof that lacks a certificate, and certification is on, the call waits
/// for one instead (R5.1 criterion 1); otherwise why3find gets its answer.
#[ensures((^s).goals == s.goals && (^s).verdicts == s.verdicts && (^s).phase == s.phase
    && (^s).local_running == s.local_running)]
#[ensures((^s).calls@.len() >= s.calls@.len())]
fn answer_or_certify(s: &mut State, call: Call, outcome: Outcome, effects: &mut Vec<Effect>) {
    let unchecked = match outcome {
        Outcome::Unchecked => true,
        _ => false,
    };
    if s.config.certify && unchecked {
        let rid = call.rid;
        let mut call = call;
        call.place = Place::Certifying;
        call.jobs.clear();
        s.calls.push(call);
        effects.push(Effect::Certify { rid });
    } else {
        effects.push(Effect::Answer { rid: call.rid, reply: reply_of(outcome) });
    }
}

#[ensures(result.state == state && result.effects@.len() == 0 && result.action == TlaAction::Discard)]
fn discard(state: State) -> Step<State, Effect> {
    let mut log = Vec::new();
    log.push(LogEntry::Discarded);
    Step { state, effects: Vec::new(), action: TlaAction::Discard, log }
}

#[ensures(result.state == state)]
fn done(state: State, effects: Vec<Effect>, action: TlaAction) -> Step<State, Effect> {
    Step { state, effects, action, log: Vec::new() }
}

/// A local run ended, or a Job of a race ended with an authentic Result.
#[requires(k@ < s.calls@.len())]
#[ensures(result.state.goals == s.goals)]
#[ensures(forall<w> crate::verdict::recorded(s.verdicts.entries@, w)
    ==> crate::verdict::recorded(result.state.verdicts.entries@, w))]
fn conclude(
    mut s: State,
    k: usize,
    evidence: Evidence,
    remote: Option<RemoteFacts>,
    action: TlaAction,
) -> Step<State, Effect> {
    let mut effects = Vec::new();
    let rid = s.calls[k].rid;
    let ctx = context(s.config.clone(), evidence);
    let decision = decide(evidence, &ctx, remote);
    let raced = match remote {
        Some(_) => true,
        None => false,
    };
    match decision {
        Decision::Proved => {
            // A proof label: the call is answered, its race is won (R2.2
            // criterion 8), and the Goal has its Verdict (R1.5 criterion 1).
            let call = s.calls.remove(k);
            s.verdicts = record_verdict(s.verdicts, call.goal, Label::ProvedStandalone);
            if raced {
                cancel_jobs(&call, &mut effects);
            } else {
                release_local(&mut s, &mut effects);
            }
            effects.push(Effect::Answer { rid: call.rid, reply: Reply::Valid });
        }
        other => {
            let outcome = match other {
                Decision::Lost(outcome) => outcome,
                // A discarded Result ends its Job as failed (R2.2 criterion 24).
                _ => Outcome::Failed,
            };
            if raced {
                s.calls[k].lost.push(outcome);
                if s.calls[k].jobs.is_empty() {
                    end_lost_race(&mut s, k, &mut effects);
                }
            } else {
                let call = s.calls.remove(k);
                release_local(&mut s, &mut effects);
                answer_or_certify(&mut s, call, outcome, &mut effects);
            }
        }
    }
    let mut log = Vec::new();
    match decision {
        Decision::Proved => log.push(LogEntry::Proved { call: rid }),
        Decision::Lost(outcome) => log.push(LogEntry::Lost { call: rid, outcome }),
        Decision::Discarded(reason) => log.push(LogEntry::Rejected { call: rid, reason }),
    }
    Step { state: s, effects, action, log }
}

/// One step of the Front_End machine.
///
/// - A finished Session accepts nothing more.
/// - No step removes a Goal or changes a recorded Verdict (R1.5 criterion 8).
#[ensures(s.phase == Phase::Finished
    ==> result.state == s && result.effects@.len() == 0
        && result.action == TlaAction::Discard)]
#[ensures(forall<g> member(s.goals@, g) ==> member(result.state.goals@, g))]
#[ensures(forall<w> crate::verdict::recorded(s.verdicts.entries@, w)
    ==> crate::verdict::recorded(result.state.verdicts.entries@, w))]
pub fn step(mut s: State, i: Input) -> Step<State, Effect> {
    match s.phase {
        Phase::Finished => return discard(s),
        Phase::Idle => {
            return match i {
                Input::SessionStart { cluster, now } => {
                    s.phase = Phase::Running;
                    s.cluster = cluster;
                    s.heard = now;
                    s.beat = now;
                    done(s, Vec::new(), TlaAction::SessionStart)
                }
                _ => discard(s),
            };
        }
        Phase::Running => {}
    }
    let mut effects = Vec::new();
    match i {
        Input::SessionStart { .. } => discard(s),
        Input::GoalAnnounced { goal, .. } => match position(&s.goals, goal) {
            Ok(_) => discard(s),
            Err(k) => {
                s.goals = insert_goal(s.goals, k, goal);
                done(s, effects, TlaAction::GoalAnnounced)
            }
        },
        Input::ProverCall { rid, goal, prover, pass, micros, .. } => {
            // A call needs an announced Goal and a fresh identifier.
            let known = match position(&s.goals, goal) {
                Ok(_) => true,
                Err(_) => false,
            };
            let fresh = match find_call(&s.calls, rid) {
                Some(_) => false,
                None => true,
            };
            if !known || !fresh {
                return discard(s);
            }
            s.calls.push(Call {
                rid,
                goal,
                prover,
                pass,
                micros,
                place: Place::Printing,
                jobs: Vec::new(),
                lost: Vec::new(),
                deadline: 0,
            });
            effects.push(Effect::Print { rid });
            done(s, effects, TlaAction::ProverCall)
        }
        Input::PrintFacts { now, rid, task_count } => {
            let k = match find_call(&s.calls, rid) {
                Some(k) => k,
                None => return discard(s),
            };
            if s.calls[k].place != Place::Printing {
                return discard(s);
            }
            if task_count != 1 {
                // print_local is undefined (R1.3 criterion 10): no Job.
                s.calls.remove(k);
                effects.push(Effect::Answer { rid, reply: Reply::Failed });
                return done(s, effects, TlaAction::PrintFacts);
            }
            let remote = match s.calls[k].pass {
                Pass::Later => s.cluster,
                Pass::Fast => s.cluster && s.config.fast_remote,
            };
            if remote {
                // One Job with the configured seed and one for each further
                // seed of the Portfolio (R2.2).
                s.calls[k].place = Place::Remote;
                let limit = s.calls[k].micros;
                s.calls[k].deadline = add(now, add(add(limit, limit), s.config.job_margin));
                let mut seed = 0;
                let _old = snapshot! { s };
                #[invariant(k@ < s.calls@.len())]
                #[invariant(s.goals == _old.goals && s.verdicts == _old.verdicts)]
                while seed < s.config.seeds {
                    s.calls[k].jobs.push(seed);
                    effects.push(Effect::Submit { rid, seed });
                    seed += 1;
                }
                if s.calls[k].jobs.is_empty() {
                    // A Portfolio of no seeds: run locally.
                    s.calls[k].place = Place::Queued;
                    start_queued(&mut s, &mut effects);
                }
            } else {
                s.calls[k].place = Place::Queued;
                start_queued(&mut s, &mut effects);
            }
            done(s, effects, TlaAction::PrintFacts)
        }
        Input::CacheHit { rid, evidence, remote, .. } => {
            let k = match find_call(&s.calls, rid) {
                Some(k) => k,
                None => return discard(s),
            };
            if s.calls[k].place != Place::Printing {
                return discard(s);
            }
            // The entry is labelled as a Result received in this Session,
            // with the facts and the Lock_File of this Session (R7.1
            // criterion 6); a discarded entry is a miss (R7.5 criterion 6).
            let ctx = context(s.config.clone(), evidence);
            match decide(evidence, &ctx, remote) {
                Decision::Proved => {
                    let call = s.calls.remove(k);
                    s.verdicts = record_verdict(s.verdicts, call.goal, Label::ProvedStandalone);
                    effects.push(Effect::Answer { rid, reply: Reply::Valid });
                    let mut log = Vec::new();
                    log.push(LogEntry::Proved { call: rid });
                    Step { state: s, effects, action: TlaAction::CacheHit, log }
                }
                Decision::Lost(outcome) => {
                    s.calls.remove(k);
                    effects.push(Effect::Answer { rid, reply: reply_of(outcome) });
                    let mut log = Vec::new();
                    log.push(LogEntry::Lost { call: rid, outcome });
                    Step { state: s, effects, action: TlaAction::CacheHit, log }
                }
                Decision::Discarded(_) => discard(s),
            }
        }
        Input::LocalRun { rid, evidence, .. } => {
            let k = match find_call(&s.calls, rid) {
                Some(k) => k,
                None => return discard(s),
            };
            if s.calls[k].place != Place::Local {
                return discard(s);
            }
            conclude(s, k, evidence, None, TlaAction::LocalExit)
        }
        Input::RemoteResult {
            rid, seed, prover, evidence, signature_valid, registered, revoked, velocity, local_micros, ..
        } => {
            let k = match find_call(&s.calls, rid) {
                Some(k) => k,
                None => return discard(s),
            };
            if s.calls[k].place != Place::Remote {
                return discard(s);
            }
            // Not authentic: anyone on the path can send this. The Job stays
            // open for an authentic Result (R1.3 criterion 4).
            if !registered || revoked || !signature_valid {
                return discard(s);
            }
            let j = match find_job(&s.calls[k].jobs, seed) {
                Some(j) => j,
                // Not an open Job of this call: late, replayed or invented.
                None => return discard(s),
            };
            // An authentic Result ends its Job, accepted or not (R2.2
            // criterion 24).
            s.calls[k].jobs.remove(j);
            let facts = RemoteFacts {
                signature_valid,
                registered,
                revoked,
                // The Job as submitted: an open Job of the call, for the
                // call's prover (R1.3 criterion 6).
                // and with the local time limit that its reference limit
                // and the Worker's Calibration give (R1.4 criterion 6).
                submitted: prover == s.calls[k].prover
                    && local_micros == crate::calibration::scale(s.calls[k].micros, velocity),
            };
            conclude(s, k, evidence, Some(facts), TlaAction::RemoteResult)
        }
        Input::JobNotRun { rid, seed, .. } => {
            let k = match find_call(&s.calls, rid) {
                Some(k) => k,
                None => return discard(s),
            };
            if s.calls[k].place != Place::Remote {
                return discard(s);
            }
            let j = match find_job(&s.calls[k].jobs, seed) {
                Some(j) => j,
                None => return discard(s),
            };
            s.calls[k].jobs.remove(j);
            if s.calls[k].jobs.is_empty() {
                end_lost_race(&mut s, k, &mut effects);
            }
            done(s, effects, TlaAction::JobNotRun)
        }
        Input::CancelCall { rid, .. } => {
            let k = match find_call(&s.calls, rid) {
                Some(k) => k,
                // Already answered.
                None => return discard(s),
            };
            let call = s.calls.remove(k);
            match call.place {
                Place::Local => {
                    effects.push(Effect::CancelLocal { rid });
                    release_local(&mut s, &mut effects);
                }
                Place::Remote => cancel_jobs(&call, &mut effects),
                _ => {}
            }
            effects.push(Effect::Answer { rid, reply: Reply::NoResult });
            done(s, effects, TlaAction::CancelCall)
        }
        Input::ClusterLost { .. } => {
            if !s.cluster {
                return discard(s);
            }
            fall_back(&mut s, &mut effects);
            done(s, effects, TlaAction::ClusterLost)
        }
        Input::Certificate { rid, facts, .. } => {
            let k = match find_call(&s.calls, rid) {
                Some(k) => k,
                None => return discard(s),
            };
            if s.calls[k].place != Place::Certifying {
                return discard(s);
            }
            let call = s.calls.remove(k);
            let mut log = Vec::new();
            match label_from_checker_output(facts) {
                Label::Certified => {
                    // The second route to a proof label (R7.1 criterion 3).
                    s.verdicts = record_verdict(s.verdicts, call.goal, Label::Certified);
                    effects.push(Effect::Answer { rid, reply: Reply::Valid });
                    log.push(LogEntry::Certified { call: rid });
                }
                Label::NonProof(outcome) => {
                    effects.push(Effect::Answer { rid, reply: reply_of(outcome) });
                    log.push(LogEntry::Lost { call: rid, outcome });
                }
                // Not reachable: a checker run never gives proved-standalone.
                Label::ProvedStandalone => {
                    effects.push(Effect::Answer { rid, reply: Reply::Unknown });
                    log.push(LogEntry::Lost { call: rid, outcome: Outcome::Unchecked });
                }
            }
            Step { state: s, effects, action: TlaAction::Certificate, log }
        }
        Input::ClusterHeard { now } => {
            if !s.cluster {
                return discard(s);
            }
            s.heard = now;
            done(s, effects, TlaAction::Heartbeat)
        }
        Input::Tick { now } => {
            if s.cluster {
                // A silent Coordinator is a lost Cluster (R1.6 criterion 4).
                let limit = s.config.silence_limit;
                if limit > 0 && now > s.heard && now - s.heard > limit {
                    effects.push(Effect::LeaveCluster);
                    fall_back(&mut s, &mut effects);
                    return done(s, effects, TlaAction::ClusterLost);
                }
                if now >= s.beat && now - s.beat >= s.config.beat_period {
                    s.beat = now;
                    effects.push(Effect::Beat);
                }
            }
            // A race whose deadline has passed ends: its open Jobs count as
            // timed out (R1.5 criterion 9).
            let _old = snapshot! { s };
            let mut k = 0;
            #[invariant(k@ <= s.calls@.len())]
            #[invariant(s.goals == _old.goals && s.verdicts == _old.verdicts)]
            while k < s.calls.len() {
                if s.calls[k].place == Place::Remote && now >= s.calls[k].deadline {
                    let mut call = s.calls.remove(k);
                    cancel_jobs(&call, &mut effects);
                    call.lost.push(Outcome::Timeout);
                    let best = most_informative(&call.lost);
                    answer_or_certify(&mut s, call, best, &mut effects);
                } else {
                    k += 1;
                }
            }
            done(s, effects, TlaAction::Tick)
        }
        Input::Why3findDone { .. } => {
            s.phase = Phase::Finished;
            done(s, effects, TlaAction::FinalWrite)
        }
    }
}

/// The Front_End machine.
pub struct FrontEnd;

impl Machine for FrontEnd {
    type Config = Config;
    type State = State;
    type Input = Input;
    type Effect = Effect;

    fn init(cfg: &Config) -> State {
        State {
            config: cfg.clone(),
            phase: Phase::Idle,
            cluster: false,
            goals: Vec::new(),
            calls: Vec::new(),
            local_running: 0,
            verdicts: Verdicts { entries: Vec::new() },
            heard: 0,
            beat: 0,
        }
    }

    fn step(s: State, i: Input) -> Step<State, Effect> {
        step(s, i)
    }
}

/// The Verdict of `goal`, if it has one.
// A contract, even a trivial one, is what lets the loop invariant below be
// erased in a build without Creusot.
#[ensures(true)]
pub fn verdict_of(verdicts: &Verdicts, goal: GoalId) -> Option<Verdict> {
    let mut k = 0;
    #[invariant(k@ <= verdicts.entries@.len())]
    while k < verdicts.entries.len() {
        if verdicts.entries[k].goal == goal {
            return Some(verdicts.entries[k]);
        }
        k += 1;
    }
    None
}
