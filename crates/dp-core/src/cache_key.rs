//! The Result_Cache key and the checks of a cache entry (R7.5).
//!
//! The lookup fields of a run description are encoded as a list of atoms:
//! every field at a fixed position, and the option list after its length.
//! The shell prints that list as a canonical S-expression, whose atoms are
//! length-prefixed, and hashes it; it also stores the list with the entry,
//! so that a lookup compares the stored fields themselves and not only the
//! hash (criterion 5).

use creusot_std::prelude::{Clone, DeepModel, PartialEq, *};

use crate::verdict::{Hash, UnpatchedSolver};

/// One atom of an encoded run description.
#[derive(Clone, Debug, PartialEq, Eq, DeepModel)]
pub enum Atom {
    Bytes(Vec<u8>),
    Num(u64),
}

/// The lookup fields of a run description (R7.5 criterion 1). Checker
/// identities are not here yet: no run description of this build may need
/// certification and be proof-labelled.
#[derive(Clone, Debug, PartialEq, Eq, DeepModel)]
pub struct LookupFields {
    /// The why3find goal path, in canonical form.
    pub goal_path: Vec<u8>,
    pub task_hash: Hash,
    pub driver: Vec<u8>,
    pub pipeline: Vec<u8>,
    pub prover: Vec<u8>,
    /// The pinned prover version.
    pub version: Vec<u8>,
    /// The seed, or `None` for the seed of the prover configuration.
    pub seed: Option<u64>,
    /// The reference time limit.
    pub micros: u64,
    pub import_mark: bool,
    pub options: Vec<Vec<u8>>,
}

/// Number of atoms before the option list.
pub const FIXED: usize = 14;

/// The atoms of `f`, position by position.
#[logic(open)]
pub fn encodes(e: Seq<Atom>, f: LookupFields) -> bool {
    pearlite! {
        e.len() == 14 + f.options@.len()
        && (match e[0] { Atom::Bytes(b) => b@ == f.goal_path@, _ => false })
        && e[1] == Atom::Num(f.task_hash.w0)
        && e[2] == Atom::Num(f.task_hash.w1)
        && e[3] == Atom::Num(f.task_hash.w2)
        && e[4] == Atom::Num(f.task_hash.w3)
        && (match e[5] { Atom::Bytes(b) => b@ == f.driver@, _ => false })
        && (match e[6] { Atom::Bytes(b) => b@ == f.pipeline@, _ => false })
        && (match e[7] { Atom::Bytes(b) => b@ == f.prover@, _ => false })
        && (match e[8] { Atom::Bytes(b) => b@ == f.version@, _ => false })
        && (match f.seed {
            Some(seed) => e[9] == Atom::Num(1u64) && e[10] == Atom::Num(seed),
            None => e[9] == Atom::Num(0u64) && e[10] == Atom::Num(0u64),
        })
        && e[11] == Atom::Num(f.micros)
        && e[12] == Atom::Num(if f.import_mark { 1u64 } else { 0u64 })
        && (match e[13] { Atom::Num(n) => n@ == f.options@.len(), _ => false })
        && (forall<i> 0 <= i && i < f.options@.len() ==>
            match e[14 + i] { Atom::Bytes(b) => b@ == f.options[i]@, _ => false })
    }
}

/// Two run descriptions with the same contents, field by field.
#[logic(open)]
pub fn same_fields(a: LookupFields, b: LookupFields) -> bool {
    pearlite! {
        a.goal_path@ == b.goal_path@
        && a.task_hash == b.task_hash
        && a.driver@ == b.driver@
        && a.pipeline@ == b.pipeline@
        && a.prover@ == b.prover@
        && a.version@ == b.version@
        && a.seed == b.seed
        && a.micros == b.micros
        && a.import_mark == b.import_mark
        && a.options@.len() == b.options@.len()
        && (forall<i> 0 <= i && i < a.options@.len() ==> a.options[i]@ == b.options[i]@)
    }
}

/// Two atom lists with the same contents.
#[logic(open)]
pub fn same_atoms(x: Seq<Atom>, y: Seq<Atom>) -> bool {
    pearlite! {
        x.len() == y.len()
        && (forall<i> 0 <= i && i < x.len() ==> match (x[i], y[i]) {
            (Atom::Bytes(p), Atom::Bytes(q)) => p@ == q@,
            (Atom::Num(m), Atom::Num(n)) => m == n,
            _ => false,
        })
    }
}

/// The encoding is injective (R7.5 criterion 2, P7.5.1): run descriptions
/// with equal encodings are equal in every lookup field. In particular the
/// import mark is part of the key (P7.5.3).
#[logic(open)]
#[requires(encodes(ea, a))]
#[requires(encodes(eb, b))]
#[requires(same_atoms(ea, eb))]
#[ensures(same_fields(a, b))]
pub fn encoding_injective(ea: Seq<Atom>, a: LookupFields, eb: Seq<Atom>, b: LookupFields) {}

/// A copy of `v` with the same contents.
#[ensures(result@ == v@)]
fn copy(v: &Vec<u8>) -> Vec<u8> {
    let r = v.clone();
    proof_assert! { r@.ext_eq(v@) };
    r
}

/// Encodes the lookup fields of a run description.
#[requires(f.options@.len() < 1_000_000)]
#[ensures(encodes(result@, *f))]
pub fn encode_key(f: &LookupFields) -> Vec<Atom> {
    let mut e = Vec::new();
    e.push(Atom::Bytes(copy(&f.goal_path)));
    e.push(Atom::Num(f.task_hash.w0));
    e.push(Atom::Num(f.task_hash.w1));
    e.push(Atom::Num(f.task_hash.w2));
    e.push(Atom::Num(f.task_hash.w3));
    e.push(Atom::Bytes(copy(&f.driver)));
    e.push(Atom::Bytes(copy(&f.pipeline)));
    e.push(Atom::Bytes(copy(&f.prover)));
    e.push(Atom::Bytes(copy(&f.version)));
    match f.seed {
        Some(seed) => {
            e.push(Atom::Num(1));
            e.push(Atom::Num(seed));
        }
        None => {
            e.push(Atom::Num(0));
            e.push(Atom::Num(0));
        }
    }
    e.push(Atom::Num(f.micros));
    e.push(Atom::Num(if f.import_mark { 1 } else { 0 }));
    e.push(Atom::Num(f.options.len() as u64));
    let _head = snapshot! { e };
    let mut i = 0;
    #[invariant(i@ <= f.options@.len())]
    #[invariant(e@.len() == 14 + i@)]
    #[invariant(forall<j> 0 <= j && j < 14 ==> e[j] == _head[j])]
    #[invariant(forall<j> 0 <= j && j < i@ ==>
        match e[14 + j] { Atom::Bytes(b) => b@ == f.options[j]@, _ => false })]
    while i < f.options.len() {
        e.push(Atom::Bytes(copy(&f.options[i])));
        i += 1;
    }
    e
}

#[ensures(result == (a@ == b@))]
fn same_bytes(a: &Vec<u8>, b: &Vec<u8>) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    #[invariant(i@ <= a@.len())]
    #[invariant(forall<j> 0 <= j && j < i@ ==> a[j] == b[j])]
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    proof_assert! { a@.ext_eq(b@) };
    true
}

/// The stored lookup fields of an entry are those of the lookup (R7.5
/// criterion 5). The comparison is of the fields themselves, so two run
/// descriptions with the same Cache_Key under a weak hash never share an
/// entry (P7.5.5).
#[ensures(result == same_atoms(stored@, wanted@))]
pub fn lookup_ok(stored: &Vec<Atom>, wanted: &Vec<Atom>) -> bool {
    if stored.len() != wanted.len() {
        return false;
    }
    let mut i = 0;
    #[invariant(i@ <= stored@.len())]
    #[invariant(forall<j> 0 <= j && j < i@ ==> match (stored[j], wanted[j]) {
        (Atom::Bytes(p), Atom::Bytes(q)) => p@ == q@,
        (Atom::Num(m), Atom::Num(n)) => m == n,
        _ => false,
    })]
    while i < stored.len() {
        let same = match (&stored[i], &wanted[i]) {
            (Atom::Bytes(p), Atom::Bytes(q)) => same_bytes(p, q),
            (Atom::Num(m), Atom::Num(n)) => *m == *n,
            _ => false,
        };
        if !same {
            return false;
        }
        i += 1;
    }
    true
}

/// The validation fields of an entry, with what the current Session knows.
pub struct ValidationFacts {
    /// Prover and pinned version of the entry, as identifiers of the shell.
    pub prover: u64,
    pub version: u64,
    /// The solver binary hash stored with the entry.
    pub binary_hash: Hash,
    /// The Calibration value stored with the entry, in millionths.
    pub velocity: u64,
    /// The local time limit stored with the entry.
    pub local_micros: u64,
    /// The reference time limit of the run description.
    pub micros: u64,
    /// Number of imports that the stored Result lists.
    pub imports: u64,
    /// The import-set hash stored with the entry.
    pub imports_hash: Hash,
    /// The hash of the stored import list, recomputed.
    pub recomputed_imports_hash: Hash,
    /// The entry carries the mark local.
    pub local: bool,
}

/// Why an entry is treated as a miss (R7.5 criteria 10 and 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum Validation {
    Valid,
    /// The stored solver binary hash is not listed in the current Lock_File
    /// for the entry's prover and version.
    BinaryMismatch,
    /// The local time limit is not the Calibration scaling of the stored
    /// Calibration value and the reference time limit.
    LimitMismatch,
    /// The import-set hash is not the hash of the stored import list.
    ImportsMismatch,
}

/// The Lock_File lists `binary_hash` for the prover and version, as a solver
/// of any kind.
#[logic(open)]
pub fn listed(pins: Seq<UnpatchedSolver>, v: ValidationFacts) -> bool {
    pearlite! {
        exists<i> 0 <= i && i < pins.len()
            && pins[i].prover == v.prover
            && pins[i].version == v.version
            && pins[i].binary_hash == v.binary_hash
    }
}

/// Checks the validation fields of an entry before it is labelled (R7.5
/// criterion 9, P7.5.8). `pins` are the solver entries of the current
/// Lock_File, of every kind.
#[ensures((result == Validation::Valid) == (
    listed(pins@, *v)
    && v.local_micros@ == crate::calibration::scaled(v.micros@, v.velocity@)
    && v.imports_hash == v.recomputed_imports_hash))]
pub fn validate_entry(v: &ValidationFacts, pins: &Vec<UnpatchedSolver>) -> Validation {
    let expected_local_micros = crate::calibration::scale(v.micros, v.velocity);
    let mut found = false;
    let mut i = 0;
    #[invariant(i@ <= pins@.len())]
    #[invariant(found == exists<j> 0 <= j && j < i@
        && pins[j].prover == v.prover
        && pins[j].version == v.version
        && pins[j].binary_hash == v.binary_hash)]
    while i < pins.len() {
        let p = pins[i];
        if p.prover == v.prover && p.version == v.version && crate::verdict::same_hash(p.binary_hash, v.binary_hash) {
            found = true;
        }
        i += 1;
    }
    if !found {
        Validation::BinaryMismatch
    } else if v.local_micros != expected_local_micros {
        Validation::LimitMismatch
    } else if !crate::verdict::same_hash(v.imports_hash, v.recomputed_imports_hash) {
        Validation::ImportsMismatch
    } else {
        Validation::Valid
    }
}
