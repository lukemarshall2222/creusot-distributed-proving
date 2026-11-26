//! `arb_cache_entry`: Result_Cache entries, and entries with one lookup
//! or validation field changed (Property 29).

use dp_formats::{
    cache::{Atom, Entry, Origin},
    cluster::{ResultFields, SignedResult},
    dispatch::DriverAnswer,
    hash::Hash,
};
use ed25519_dalek::SigningKey;
use proptest::prelude::*;

pub fn atom() -> impl Strategy<Value = Atom> {
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..12).prop_map(Atom::Bytes),
        any::<u64>().prop_map(Atom::Num),
    ]
}

pub fn lookup() -> impl Strategy<Value = Vec<Atom>> {
    proptest::collection::vec(atom(), 0..10)
}

/// The mark local, or a Worker's signed Result.
pub fn origin() -> impl Strategy<Value = Origin> {
    prop_oneof![
        Just(Origin::Local),
        (any::<[u8; 32]>(), "[a-z0-9/]{0,8}", any::<Option<u64>>(), proptest::collection::vec(any::<u8>(), 0..20))
            .prop_map(|(key, id, seed, output)| {
                let fields = ResultFields {
                    job_id: id.clone(),
                    task_hash: Hash([1; 32]),
                    prover: id.clone(),
                    seed,
                    binary_hash: Hash([2; 32]),
                    worker: id,
                    status: Some(0),
                    timed_out: false,
                    output,
                    elapsed_micros: 5,
                    velocity: 1_000_000,
                    local_micros: 9,
                    imports_hash: Hash([5; 32]),
                };
                Origin::Worker(SignedResult::sign(fields, &SigningKey::from_bytes(&key)))
            }),
    ]
}

pub fn entry() -> impl Strategy<Value = Entry> {
    (lookup(), any::<[u8; 32]>(), any::<u64>(), any::<u64>(), any::<[u8; 32]>(),
     proptest::sample::select(DriverAnswer::ALL.to_vec()), any::<u64>(), origin())
        .prop_map(|(lookup, binary, velocity, local_micros, imports, answer, micros, origin)| Entry {
            lookup,
            binary_hash: Hash(binary),
            velocity,
            local_micros,
            imports_hash: Hash(imports),
            answer,
            micros,
            origin,
        })
}

/// The number of fields that `change` can change: the validation fields,
/// and one lookup field.
pub const FIELDS: u8 = 5;

/// One lookup or validation field of an entry changed.
pub fn change(e: &Entry, which: u8) -> Entry {
    let mut g = e.clone();
    match which % FIELDS {
        0 => g.binary_hash.0[0] ^= 1,
        1 => g.velocity = g.velocity.wrapping_add(1),
        2 => g.local_micros = g.local_micros.wrapping_add(1),
        3 => g.imports_hash.0[0] ^= 1,
        _ => match g.lookup.first_mut() {
            Some(Atom::Num(n)) => *n = n.wrapping_add(1),
            Some(Atom::Bytes(b)) => b.push(0),
            None => g.lookup.push(Atom::Num(0)),
        },
    }
    g
}
