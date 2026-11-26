//! Entries of the `proof.json` extension (Properties 30 and 34): every
//! label, replay fields with each optional part present or absent, the
//! conflict mark and Invalid records.

use dp_formats::hash::Hash;
use dp_formats::proofjson::{DpEntry, InvalidEntry, Place, ReplayFields, LABELS};
use proptest::prelude::*;

pub fn place() -> impl Strategy<Value = Place> {
    prop_oneof![Just(Place::Local), Just(Place::Absent), "[a-z0-9]{0,6}".prop_map(Place::Worker)]
}

pub fn replay() -> impl Strategy<Value = ReplayFields> {
    (
        any::<[u8; 32]>(),
        proptest::collection::vec(any::<u8>(), 0..12),
        "[a-z-]{0,8}",
        "[0-9.]{0,6}",
        any::<[u8; 32]>(),
        place(),
        any::<Option<u64>>(),
        any::<Option<u64>>(),
        any::<Option<u64>>(),
        any::<u64>(),
    )
        .prop_map(|(task, driver, prover, version, binary, place, seed, velocity, local, reference)| {
            ReplayFields {
                task_hash: Hash(task),
                driver,
                prover,
                version,
                binary_hash: Hash(binary),
                place,
                seed,
                velocity,
                local_micros: local,
                reference_micros: reference,
            }
        })
}

pub fn entry() -> impl Strategy<Value = DpEntry> {
    let invalid = ("[a-z-]{0,8}", "[0-9.]{0,6}", any::<[u8; 32]>(), place()).prop_map(
        |(prover, version, binary, place)| InvalidEntry { prover, version, binary_hash: Hash(binary), place },
    );
    (
        proptest::sample::select(LABELS.to_vec()),
        proptest::collection::vec(replay(), 0..3),
        any::<[u8; 32]>(),
        any::<bool>(),
        proptest::collection::vec(invalid, 0..3),
        any::<Option<[u8; 32]>>(),
    )
        .prop_map(|(label, replay, imports, conflict, invalid, certificate)| DpEntry {
            label: label.to_string(),
            replay,
            imports_hash: Hash(imports),
            certificate: certificate.map(Hash),
            conflict,
            invalid,
        })
}
