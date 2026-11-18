//! The Verdict_Recorder for the proved-standalone route: Result acceptance,
//! label assignment and Verdict recording (R1.3, R1.5, R7.1).
//!
//! Fail-safe rule (design, Error Handling): `label` returns
//! proved-standalone from exactly one branch, whose condition is every
//! condition of R7.1 criterion 2. Every other branch returns a non-proof
//! label. No function of this module returns certified; that route arrives
//! with the certificate stage, and until then every Result that needs
//! certification is labelled unchecked.

use creusot_std::prelude::{Clone, DeepModel, PartialEq, *};

use crate::{
    frontend::GoalId,
    labels::{classify, most_informative, Outcome, ProverAnswer},
};

/// A SHA-256 digest as four words. The shell computes hashes; this module
/// only compares them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct Hash {
    pub w0: u64,
    pub w1: u64,
    pub w2: u64,
    pub w3: u64,
}

/// An Assurance_Label.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Label {
    ProvedStandalone,
    Certified,
    NonProof(Outcome),
}

/// A Lock_File entry of kind Unpatched_Solver for one platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct UnpatchedSolver {
    /// Identifier of the prover name, interned by the shell.
    pub prover: u64,
    /// Identifier of the pinned prover version, interned by the shell.
    pub version: u64,
    pub binary_hash: Hash,
}

/// The fields of a Result that labelling reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct ResultFields {
    pub answer: ProverAnswer,
    /// The Task_Hash that the Result carries.
    pub task_hash: Hash,
    /// The Result is for a Partition, not for the whole Goal_Version.
    pub partition: bool,
    /// Number of imported Lemmas and Instantiation_Hints that it lists.
    pub imports: u64,
    pub imports_hash: Hash,
    /// Prover and version of its Solver_Configuration.
    pub prover: u64,
    pub version: u64,
    /// The solver binary hash that the run reports.
    pub binary_hash: Hash,
}

/// What the Front_End knows locally about the Job of a Result.
pub struct Context {
    pub certify_all: bool,
    /// The Job carried the import mark (R3.1 criterion 5).
    pub import_mark: bool,
    /// Task_Hash of `print_local(G)` for the Result's Driver and
    /// Transformation_Pipeline, recomputed in this Session.
    pub local_task_hash: Hash,
    /// Number of tasks that the Transformation_Pipeline yields for the Goal.
    pub task_count: u64,
    /// `H(canon([]))`, the import-set hash of no imports.
    pub empty_imports_hash: Hash,
    /// The local solver binary differed from the Lock_File when it was
    /// started (R7.7 criterion 7).
    pub local_binary_mismatch: bool,
    /// The Unpatched_Solver entries of the Lock_File.
    pub unpatched: Vec<UnpatchedSolver>,
}

/// The Result is bound to the local print: its Task_Hash is that of
/// `print_local(G)`, which is defined only for one task (R1.3 criteria 3
/// and 10).
#[logic(open)]
pub fn bound(r: ResultFields, ctx: Context) -> bool {
    pearlite! { r.task_hash == ctx.local_task_hash && ctx.task_count@ == 1 }
}

/// The Lock_File lists the Result's solver binary as an Unpatched_Solver for
/// its prover and version (R1.3 criterion 5).
#[logic(open)]
pub fn unpatched(r: ResultFields, ctx: Context) -> bool {
    pearlite! {
        exists<i> 0 <= i && i < ctx.unpatched@.len()
            && ctx.unpatched[i].prover == r.prover
            && ctx.unpatched[i].version == r.version
            && ctx.unpatched[i].binary_hash == r.binary_hash
    }
}

/// The Result is a Lemma_Dependent_Result (Glossary, R1.3 criterion 5).
#[logic(open)]
pub fn lemma_dependent(r: ResultFields, ctx: Context) -> bool {
    pearlite! {
        ctx.import_mark
            || r.imports@ != 0
            || r.imports_hash != ctx.empty_imports_hash
            || !unpatched(r, ctx)
            || ctx.local_binary_mismatch
    }
}

/// Every condition of R7.1 criterion 2 (P7.1.1, case a).
#[logic(open)]
pub fn standalone(r: ResultFields, ctx: Context) -> bool {
    pearlite! {
        !ctx.certify_all
            && r.answer == ProverAnswer::Valid
            && bound(r, ctx)
            && !r.partition
            && !lemma_dependent(r, ctx)
    }
}

/// Outcome of `accept_result`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Acceptance {
    Accepted,
    /// The Task_Hash is not that of the local print (R1.3 criterion 3).
    WrongTaskHash,
    /// The Transformation_Pipeline does not yield exactly one task (R1.3
    /// criterion 10).
    NotOneTask,
    /// The tag does not verify under the named Worker's key (R1.3 criterion 4).
    BadSignature,
    /// The named Worker is not in the operator-signed registry, or it is
    /// revoked (R1.3 criterion 9, R1.7 criterion 6).
    UnknownWorker,
    /// The Front_End did not submit this Job with this prover (R1.3
    /// criterion 6).
    NotSubmitted,
}

/// What the shell established about a Result that arrived from the Cluster.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct RemoteFacts {
    /// The tag verifies under the verification key that the registry holds
    /// for the Worker that the Result names.
    pub signature_valid: bool,
    /// The registry that the Front_End holds lists that Worker.
    pub registered: bool,
    /// The Front_End holds a revocation of that Worker.
    pub revoked: bool,
    /// The Result names an open Job of this Session and the prover that the
    /// Front_End submitted it with.
    pub submitted: bool,
}

/// The Result comes from an authentic, registered Worker and answers a Job
/// that the Front_End submitted (P1.3.2, P1.3.4, P1.3.6).
#[logic(open)]
pub fn authentic(f: RemoteFacts) -> bool {
    pearlite! { f.signature_valid && f.registered && !f.revoked && f.submitted }
}

#[ensures(result == (a == b))]
fn same_hash(a: Hash, b: Hash) -> bool {
    a.w0 == b.w0 && a.w1 == b.w1 && a.w2 == b.w2 && a.w3 == b.w3
}

/// Accepts or discards a Result of a local Job (R1.3 criteria 3 and 10).
/// A discarded Result is never labelled.
#[ensures((result == Acceptance::Accepted) == bound(r, *ctx))]
pub fn accept_result(r: ResultFields, ctx: &Context) -> Acceptance {
    if ctx.task_count != 1 {
        Acceptance::NotOneTask
    } else if !same_hash(r.task_hash, ctx.local_task_hash) {
        Acceptance::WrongTaskHash
    } else {
        Acceptance::Accepted
    }
}

/// Accepts or discards a Result that arrived from the Cluster (R1.3
/// criteria 3, 4, 6, 9 and 10). Everything that arrives is untrusted until
/// this function accepts it; a discarded Result is never labelled.
#[ensures((result == Acceptance::Accepted) == (authentic(facts) && bound(r, *ctx)))]
pub fn accept_remote(r: ResultFields, ctx: &Context, facts: RemoteFacts) -> Acceptance {
    if !facts.registered || facts.revoked {
        Acceptance::UnknownWorker
    } else if !facts.signature_valid {
        Acceptance::BadSignature
    } else if !facts.submitted {
        Acceptance::NotSubmitted
    } else {
        accept_result(r, ctx)
    }
}

#[ensures(result == unpatched(r, *ctx))]
fn is_unpatched(r: ResultFields, ctx: &Context) -> bool {
    let mut i = 0;
    #[invariant(i@ <= ctx.unpatched@.len())]
    #[invariant(forall<j> 0 <= j && j < i@ ==> !(ctx.unpatched[j].prover == r.prover
        && ctx.unpatched[j].version == r.version
        && ctx.unpatched[j].binary_hash == r.binary_hash))]
    while i < ctx.unpatched.len() {
        let e = ctx.unpatched[i];
        if e.prover == r.prover && e.version == r.version && same_hash(e.binary_hash, r.binary_hash)
        {
            return true;
        }
        i += 1;
    }
    false
}

#[ensures(result == lemma_dependent(r, *ctx))]
fn is_lemma_dependent(r: ResultFields, ctx: &Context) -> bool {
    ctx.import_mark
        || r.imports != 0
        || !same_hash(r.imports_hash, ctx.empty_imports_hash)
        || !is_unpatched(r, ctx)
        || ctx.local_binary_mismatch
}

/// The Assurance_Label of a Result (R7.1 criteria 2 and 4, R7.2 criterion 6).
///
/// - P7.1.1 (a) and P7.1.5: proved-standalone exactly under the conditions
///   of R7.1 criterion 2.
/// - P7.1.3: the exclusions follow, because each excluded case falsifies
///   `standalone`.
/// - This build never labels certified.
/// - A Result that does not report a proof gets its classified answer.
#[ensures((result == Label::ProvedStandalone) == standalone(r, *ctx))]
#[ensures(result != Label::Certified)]
#[ensures(r.answer != ProverAnswer::Valid ==> exists<o> result == Label::NonProof(o))]
#[ensures(r.answer == ProverAnswer::Valid && !standalone(r, *ctx)
    ==> result == Label::NonProof(Outcome::Unchecked))]
pub fn label(r: ResultFields, ctx: &Context) -> Label {
    let dependent = is_lemma_dependent(r, ctx);
    match classify(r.answer, dependent) {
        // The answer is not a proof: its classification is the label.
        Some(outcome) => Label::NonProof(outcome),
        // The answer reports a proof.
        None => {
            let accepted = match accept_result(r, ctx) {
                Acceptance::Accepted => true,
                _ => false,
            };
            if accepted && !ctx.certify_all && !r.partition && !dependent {
                Label::ProvedStandalone
            } else {
                // It needs certification, which this build cannot give, or it
                // is not bound to the local print.
                Label::NonProof(Outcome::Unchecked)
            }
        }
    }
}

/// The label of a Result_Cache entry that reports a proof: the label of the
/// same fields as a Result received in the current Session, with the current
/// context (R7.1 criterion 6, P7.1.6).
#[ensures((result == Label::ProvedStandalone) == standalone(entry, *ctx))]
#[ensures(result != Label::Certified)]
pub fn cache_label(entry: ResultFields, ctx: &Context) -> Label {
    label(entry, ctx)
}

/// The Verdict of one Goal_Version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct Verdict {
    pub goal: GoalId,
    pub label: Label,
}

/// The Verdicts of a Session, strictly increasing by Goal, so a Goal has at
/// most one Verdict (R1.5 criterion 1).
#[derive(Clone, Debug, PartialEq, Eq, DeepModel)]
pub struct Verdicts {
    pub entries: Vec<Verdict>,
}

#[logic(open)]
pub fn increasing(s: Seq<Verdict>) -> bool {
    pearlite! { forall<i, j> 0 <= i && i < j && j < s.len() ==> s[i].goal@ < s[j].goal@ }
}

impl Invariant for Verdicts {
    #[logic(open)]
    fn invariant(self) -> bool {
        pearlite! { increasing(self.entries@) }
    }
}

/// `v` is one of the Verdicts `s`.
#[logic(open)]
pub fn recorded(s: Seq<Verdict>, v: Verdict) -> bool {
    pearlite! { exists<i> 0 <= i && i < s.len() && s[i] == v }
}

/// `s` has a Verdict for `goal`.
#[logic(open)]
pub fn has_verdict(s: Seq<Verdict>, goal: GoalId) -> bool {
    pearlite! { exists<i> 0 <= i && i < s.len() && s[i].goal == goal }
}

/// Position of the Verdict of `goal`, or where it would be inserted.
#[requires(increasing(entries@))]
#[ensures(match result {
    Ok(k) => k@ < entries@.len() && entries[k@].goal == goal,
    Err(k) => k@ <= entries@.len() && !has_verdict(entries@, goal)
        && (forall<i> 0 <= i && i < k@ ==> entries[i].goal@ < goal@)
        && (forall<i> k@ <= i && i < entries@.len() ==> goal@ < entries[i].goal@),
})]
fn position(entries: &Vec<Verdict>, goal: GoalId) -> Result<usize, usize> {
    let mut k = 0;
    #[invariant(k@ <= entries@.len())]
    #[invariant(forall<i> 0 <= i && i < k@ ==> entries[i].goal@ < goal@)]
    while k < entries.len() {
        if entries[k].goal == goal {
            return Ok(k);
        }
        if entries[k].goal > goal {
            return Err(k);
        }
        k += 1;
    }
    Err(k)
}

#[requires(increasing(entries@))]
#[requires(k@ <= entries@.len())]
#[requires(forall<i> 0 <= i && i < k@ ==> entries[i].goal@ < v.goal@)]
#[requires(forall<i> k@ <= i && i < entries@.len() ==> v.goal@ < entries[i].goal@)]
#[ensures(increasing(result@))]
#[ensures(recorded(result@, v))]
#[ensures(forall<w> recorded(entries@, w) ==> recorded(result@, w))]
#[ensures(forall<w> recorded(result@, w) ==> w == v || recorded(entries@, w))]
fn insert(entries: Vec<Verdict>, k: usize, v: Verdict) -> Vec<Verdict> {
    let _old = snapshot! { entries };
    let mut entries = entries;
    entries.insert(k, v);
    proof_assert! { entries[k@] == v };
    proof_assert! { forall<i> 0 <= i && i < k@ ==> entries[i] == _old[i] };
    proof_assert! { forall<i> k@ <= i && i < _old@.len() ==> entries[i + 1] == _old[i] };
    proof_assert! { forall<i> k@ < i && i < entries@.len() ==> entries[i] == _old[i - 1] };
    entries
}

/// Records the Verdict of `goal` (R1.5 criteria 1 and 8).
///
/// - Every Verdict recorded before is still recorded, unchanged.
/// - If `goal` already had a Verdict, nothing changes: a Verdict is final.
/// - Otherwise the only new Verdict is `(goal, label)`.
#[ensures(forall<w> recorded(verdicts.entries@, w) ==> recorded(result.entries@, w))]
#[ensures(has_verdict(verdicts.entries@, goal) ==> result == verdicts)]
#[ensures(!has_verdict(verdicts.entries@, goal)
    ==> recorded(result.entries@, Verdict { goal, label }))]
#[ensures(forall<w> recorded(result.entries@, w)
    ==> w == Verdict { goal, label } || recorded(verdicts.entries@, w))]
pub fn record_verdict(verdicts: Verdicts, goal: GoalId, label: Label) -> Verdicts {
    match position(&verdicts.entries, goal) {
        Ok(_) => verdicts,
        Err(k) => Verdicts { entries: insert(verdicts.entries, k, Verdict { goal, label }) },
    }
}

/// The Verdict of a Goal at the end of a Session: its recorded Verdict, and
/// otherwise the most informative non-proof outcome of its Jobs and accepted
/// Results, which is unresolved when there is none (R7.2 criterion 5). A
/// proof label comes only from a recorded Verdict.
#[ensures(match recorded {
    Some(l) => result == l,
    None => exists<o> result == Label::NonProof(o),
})]
pub fn final_label(recorded: Option<Label>, outcomes: &[Outcome]) -> Label {
    match recorded {
        Some(label) => label,
        None => Label::NonProof(most_informative(outcomes)),
    }
}
