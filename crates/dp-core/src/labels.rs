//! Classification of prover answers and the most informative non-proof
//! outcome (R7.2 criteria 5 and 6).

#[allow(unused_imports)] // `Int` is used by logic functions only
use creusot_std::logic::Int;
use creusot_std::prelude::{Clone, DeepModel, PartialEq, *};

/// A prover answer, as Why3 reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum ProverAnswer {
    Valid,
    Invalid,
    Timeout,
    OutOfMemory,
    StepLimitExceeded,
    Unknown,
    Failure,
    HighFailure,
}

/// A non-proof outcome. The variants are in the information order of R7.2,
/// most informative first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Outcome {
    /// Invalid, from a Result that is not a Lemma_Dependent_Result.
    StandaloneInvalid,
    CheckedIncomplete,
    Unchecked,
    /// Invalid, from a Lemma_Dependent_Result.
    DependentInvalid,
    Timeout,
    Unknown,
    Failed,
    Unresolved,
}

impl Outcome {
    /// Position in the information order; smaller is more informative.
    #[logic(open)]
    pub fn rank(self) -> Int {
        match self {
            Outcome::StandaloneInvalid => 0,
            Outcome::CheckedIncomplete => 1,
            Outcome::Unchecked => 2,
            Outcome::DependentInvalid => 3,
            Outcome::Timeout => 4,
            Outcome::Unknown => 5,
            Outcome::Failed => 6,
            Outcome::Unresolved => 7,
        }
    }

    /// `rank` as a program value.
    #[ensures(result@ == self.rank())]
    pub fn rank_u8(self) -> u8 {
        match self {
            Outcome::StandaloneInvalid => 0,
            Outcome::CheckedIncomplete => 1,
            Outcome::Unchecked => 2,
            Outcome::DependentInvalid => 3,
            Outcome::Timeout => 4,
            Outcome::Unknown => 5,
            Outcome::Failed => 6,
            Outcome::Unresolved => 7,
        }
    }
}

/// The information order is total and strict: two outcomes of equal rank are
/// equal. With the contract of `most_informative`, this makes its result a
/// function of the multiset of its input, so it does not depend on the order
/// of the outcomes (P7.2.4).
#[logic(open)]
#[ensures(a.rank() == b.rank() ==> a == b)]
pub fn rank_injective(a: Outcome, b: Outcome) {}

/// Classifies a prover answer (R7.2 criterion 6).
///
/// Valid reports a proof, which `label` decides, so it has no outcome here.
/// Invalid stays Invalid, standalone or dependent. Every other answer follows
/// why3find's classification (AS-31).
#[ensures(answer == ProverAnswer::Valid ==> result == None)]
#[ensures(answer == ProverAnswer::Invalid && !lemma_dependent
    ==> result == Some(Outcome::StandaloneInvalid))]
#[ensures(answer == ProverAnswer::Invalid && lemma_dependent
    ==> result == Some(Outcome::DependentInvalid))]
#[ensures(answer == ProverAnswer::Timeout ==> result == Some(Outcome::Timeout))]
#[ensures(answer == ProverAnswer::OutOfMemory
    || answer == ProverAnswer::StepLimitExceeded
    || answer == ProverAnswer::Unknown
    ==> result == Some(Outcome::Unknown))]
#[ensures(answer == ProverAnswer::Failure || answer == ProverAnswer::HighFailure
    ==> result == Some(Outcome::Failed))]
pub fn classify(answer: ProverAnswer, lemma_dependent: bool) -> Option<Outcome> {
    match answer {
        ProverAnswer::Valid => None,
        ProverAnswer::Invalid => Some(if lemma_dependent {
            Outcome::DependentInvalid
        } else {
            Outcome::StandaloneInvalid
        }),
        ProverAnswer::Timeout => Some(Outcome::Timeout),
        ProverAnswer::OutOfMemory | ProverAnswer::StepLimitExceeded | ProverAnswer::Unknown => {
            Some(Outcome::Unknown)
        }
        ProverAnswer::Failure | ProverAnswer::HighFailure => Some(Outcome::Failed),
    }
}

/// The most informative non-proof outcome (R7.2 criterion 5, P7.2.4): the
/// minimum of `outcomes` in the information order, and unresolved when there
/// is none.
#[ensures(forall<i> 0 <= i && i < outcomes@.len() ==> result.rank() <= outcomes[i].rank())]
#[ensures(result == Outcome::Unresolved
    || exists<i> 0 <= i && i < outcomes@.len() && outcomes[i] == result)]
pub fn most_informative(outcomes: &[Outcome]) -> Outcome {
    let mut best = Outcome::Unresolved;
    let mut i = 0;
    #[invariant(i@ <= outcomes@.len())]
    #[invariant(forall<j> 0 <= j && j < i@ ==> best.rank() <= outcomes[j].rank())]
    #[invariant(best == Outcome::Unresolved
        || exists<j> 0 <= j && j < i@ && outcomes[j] == best)]
    while i < outcomes.len() {
        let o = outcomes[i];
        if o.rank_u8() < best.rank_u8() {
            best = o;
        }
        i += 1;
    }
    best
}
