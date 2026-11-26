//! Kani harnesses for the Cache_Key (R7.5, P7.5.1, P7.5.5). They are a
//! module of dp-core under `cfg(kani)`; `ci/jobs/kani.sh` runs them.
//!
//! Creusot proves that the encoding of a run description is injective.
//! These harnesses check the compiled comparison of encodings, within
//! recorded bounds: `lookup_ok` accepts a stored encoding exactly when it
//! is the wanted one, atom by atom and byte by byte.

use crate::cache_key::{lookup_ok, Atom};

/// The bounds: lists of up to three atoms, byte strings of up to two bytes.
const ATOMS: usize = 3;
const BYTES: usize = 2;

fn any_atom() -> Atom {
    if kani::any() {
        Atom::Num(kani::any())
    } else {
        let length: usize = kani::any();
        kani::assume(length <= BYTES);
        let mut bytes = Vec::new();
        for _ in 0..length {
            bytes.push(kani::any());
        }
        Atom::Bytes(bytes)
    }
}

fn any_atoms() -> Vec<Atom> {
    let length: usize = kani::any();
    kani::assume(length <= ATOMS);
    let mut atoms = Vec::new();
    for _ in 0..length {
        atoms.push(any_atom());
    }
    atoms
}

/// The executable definition of equality that `lookup_ok` must agree with.
fn same(a: &[Atom], b: &[Atom]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        let equal = match (&a[i], &b[i]) {
            (Atom::Num(m), Atom::Num(n)) => m == n,
            (Atom::Bytes(x), Atom::Bytes(y)) => x == y,
            _ => false,
        };
        if !equal {
            return false;
        }
        i += 1;
    }
    true
}

/// A lookup accepts a stored encoding exactly when it is the wanted one.
#[kani::proof]
#[kani::unwind(5)]
fn lookup_ok_is_equality_of_encodings() {
    let stored = any_atoms();
    let wanted = any_atoms();
    assert!(lookup_ok(&stored, &wanted) == same(&stored, &wanted));
}

/// An encoding is accepted for itself.
#[kani::proof]
#[kani::unwind(5)]
fn an_encoding_is_accepted_for_itself() {
    let stored = any_atoms();
    assert!(lookup_ok(&stored, &stored));
}
