// Feature: creusot-distributed-proving, Property 29: Cache hits reuse only what nothing could have changed
// P-ID: P7.5.1, P7.5.2, P7.10.1 (Result_Cache entry)

use dp_formats::cache::{cache_key, Atom, Entry, Origin};
use dp_formats::cluster::{ResultFields, SignedResult};
use dp_formats::dispatch::DriverAnswer;
use dp_formats::hash::Hash;
use ed25519_dalek::SigningKey;
use proptest::prelude::*;

fn atom() -> impl Strategy<Value = Atom> {
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..12).prop_map(Atom::Bytes),
        any::<u64>().prop_map(Atom::Num),
    ]
}

fn lookup() -> impl Strategy<Value = Vec<Atom>> {
    proptest::collection::vec(atom(), 0..10)
}

fn origin() -> impl Strategy<Value = Origin> {
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

fn entry() -> impl Strategy<Value = Entry> {
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

proptest! {
    #[test]
    fn entries_round_trip(e in entry()) {
        prop_assert_eq!(Entry::decode(&e.encode(), 1 << 20), Ok(e));
    }

    /// Different lookup fields give different keys: an atom's kind, length
    /// and position are all part of the encoding (P7.5.1).
    #[test]
    fn different_lookup_fields_give_different_keys(a in lookup(), b in lookup()) {
        prop_assert_eq!(a == b, cache_key(&a) == cache_key(&b));
    }

    /// Bytes moved between neighbouring atoms, or an atom split in two,
    /// change the key.
    #[test]
    fn atom_boundaries_are_part_of_the_key(x in proptest::collection::vec(any::<u8>(), 1..12), at in any::<prop::sample::Index>()) {
        let i = at.index(x.len());
        let whole = vec![Atom::Bytes(x.clone())];
        let split = vec![Atom::Bytes(x[..i].to_vec()), Atom::Bytes(x[i..].to_vec())];
        prop_assert_ne!(cache_key(&whole), cache_key(&split));
        prop_assert_ne!(cache_key(&[Atom::Num(7)]), cache_key(&[Atom::Bytes(b"7".to_vec())]));
    }

    /// One changed byte of a stored entry is a parse failure or a different
    /// entry, never the same entry.
    #[test]
    fn a_changed_byte_never_gives_the_same_entry(e in entry(), at in any::<prop::sample::Index>()) {
        let mut bytes = e.encode();
        let i = at.index(bytes.len());
        bytes[i] ^= 1;
        if let Ok(other) = Entry::decode(&bytes, 1 << 20) {
            prop_assert_ne!(other, e);
        }
    }
}
