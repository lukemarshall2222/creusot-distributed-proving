// Feature: creusot-distributed-proving, Property 6: A Result_Cache entry answers only the run that it describes
// P-ID: P7.5.3, P7.5.5, P7.5.8

//! Tests of the Cache_Key encoding and of the validation of entries
//! (`cache_key.rs`): two run descriptions have the same encoding only if
//! they are the same, a lookup accepts only its own encoding, and an entry
//! is valid only with a pinned binary, the scaled time limit and the
//! recomputed import-set hash.

use dp_core::{
    cache_key::{encode_key, lookup_ok, validate_entry, LookupFields, Validation, ValidationFacts},
    calibration::scale,
    verdict::{Hash, UnpatchedSolver},
};
use proptest::prelude::*;

fn h(n: u64) -> Hash {
    Hash { w0: n, w1: 1, w2: 2, w3: 3 }
}

fn bytes() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..6)
}

fn lookup_fields() -> impl Strategy<Value = LookupFields> {
    (
        bytes(),
        0u64..4,
        bytes(),
        bytes(),
        bytes(),
        bytes(),
        prop::option::of(0u64..3),
        0u64..3,
        any::<bool>(),
        prop::collection::vec(bytes(), 0..3),
    )
        .prop_map(|(goal_path, task, driver, pipeline, prover, version, seed, micros, import_mark, options)| {
            LookupFields { goal_path, task_hash: h(task), driver, pipeline, prover, version, seed, micros, import_mark, options }
        })
}

/// One field of a run description changed.
fn change(f: &LookupFields, which: u8) -> LookupFields {
    let mut g = f.clone();
    match which % 15 {
        // The same lengths, other contents: a byte of a field, and a byte
        // of an option.
        12 => match g.goal_path.first_mut() {
            Some(b) => *b ^= 1,
            None => g.goal_path.push(1),
        },
        13 => match g.options.first_mut() {
            Some(option) if !option.is_empty() => option[0] ^= 0x80,
            Some(option) => option.push(7),
            None => g.options.push(vec![7]),
        },
        14 => match g.prover.last_mut() {
            Some(b) => *b = b.wrapping_add(1),
            None => g.prover.push(0),
        },
        0 => g.goal_path.push(0),
        1 => g.task_hash.w0 ^= 1,
        2 => g.driver.push(b'd'),
        3 => g.pipeline.push(b'p'),
        4 => g.prover.push(b'x'),
        5 => g.version.push(b'1'),
        6 => g.seed = Some(g.seed.map_or(0, |s| s + 1)),
        7 => g.micros += 1,
        8 => g.import_mark = !g.import_mark,
        9 => g.options.push(Vec::new()),
        10 => g.task_hash.w3 ^= 1 << 63,
        // A byte moved from one field to its neighbour: the boundaries
        // between fields are part of the encoding.
        _ => match g.driver.pop() {
            Some(b) => g.pipeline.insert(0, b),
            None => g.pipeline.insert(0, 0),
        },
    }
    g
}

/// Byte strings of one length that differ in one place, as fields and as
/// options: the encodings differ, and neither is accepted for the other.
#[test]
fn fields_of_equal_length_and_other_contents_are_told_apart() {
    let base = LookupFields {
        goal_path: b"file/Theory/goal".to_vec(),
        task_hash: h(1),
        driver: b"driver".to_vec(),
        pipeline: Vec::new(),
        prover: b"z3".to_vec(),
        version: b"4.13".to_vec(),
        seed: None,
        micros: 1_000_000,
        import_mark: false,
        options: vec![b"--seed".to_vec(), b"42".to_vec()],
    };
    let key = encode_key(&base);
    assert!(lookup_ok(&key, &key));
    let mut variants = Vec::new();
    for i in 0..base.goal_path.len() {
        let mut g = base.clone();
        g.goal_path[i] ^= 1;
        variants.push(g);
    }
    for (o, option) in base.options.iter().enumerate() {
        for i in 0..option.len() {
            let mut g = base.clone();
            g.options[o][i] ^= 1;
            variants.push(g);
        }
    }
    let mut swapped = base.clone();
    swapped.options.swap(0, 1);
    variants.push(swapped);
    for g in variants {
        let other = encode_key(&g);
        assert_ne!(other, key, "{g:?}");
        assert_eq!(other.len(), key.len());
        assert!(!lookup_ok(&other, &key) && !lookup_ok(&key, &other), "{g:?}");
    }
}

proptest! {
    #![proptest_config(dp_testkit::config::cases("prop_cache_key", 500))]

    #[test]
    fn the_encoding_is_a_function_of_the_fields(f in lookup_fields()) {
        let key = encode_key(&f);
        prop_assert_eq!(&encode_key(&f.clone()), &key);
        prop_assert!(lookup_ok(&key, &key));
    }

    #[test]
    fn a_changed_field_changes_the_encoding(f in lookup_fields(), which in any::<u8>()) {
        let g = change(&f, which);
        prop_assume!(g != f);
        let (a, b) = (encode_key(&f), encode_key(&g));
        prop_assert_ne!(&a, &b, "field {}", which % 15);
        prop_assert!(!lookup_ok(&a, &b) && !lookup_ok(&b, &a));
    }

    #[test]
    fn two_descriptions_with_one_encoding_are_one(f in lookup_fields(), g in lookup_fields()) {
        if encode_key(&f) == encode_key(&g) {
            prop_assert_eq!(f, g);
        }
    }

    /// R7.5 criterion 9: an entry is valid exactly with a pinned binary
    /// for its prover and version, the local limit that its Calibration
    /// gives, and the import-set hash of its import list.
    #[test]
    fn an_entry_is_valid_only_with_every_validation_field(
        pinned in prop::bool::weighted(0.7),
        other_prover in prop::bool::weighted(0.2),
        other_version in prop::bool::weighted(0.2),
        micros in 0u64..5_000_000,
        velocity in prop_oneof![Just(1_000_000u64), 1u64..4_000_000],
        limit_off in prop_oneof![3 => Just(0u64), 1 => 1u64..5],
        imports_off in prop::bool::weighted(0.2),
        local in any::<bool>(),
    ) {
        let facts = ValidationFacts {
            prover: if other_prover { 2 } else { 1 },
            version: if other_version { 2 } else { 1 },
            binary_hash: h(if pinned { 1 } else { 9 }),
            velocity,
            local_micros: scale(micros, velocity) + limit_off,
            micros,
            imports: 0,
            imports_hash: h(if imports_off { 5 } else { 0 }),
            recomputed_imports_hash: h(0),
            local,
        };
        let pins = vec![
            UnpatchedSolver { prover: 3, version: 1, binary_hash: h(1) },
            UnpatchedSolver { prover: 1, version: 1, binary_hash: h(1) },
        ];
        let listed = pinned && !other_prover && !other_version;
        let expected = if !listed {
            Validation::BinaryMismatch
        } else if limit_off != 0 {
            Validation::LimitMismatch
        } else if imports_off {
            Validation::ImportsMismatch
        } else {
            Validation::Valid
        };
        prop_assert_eq!(validate_entry(&facts, &pins), expected);
        // Without pins nothing is valid.
        prop_assert_eq!(validate_entry(&facts, &Vec::new()), Validation::BinaryMismatch);
    }
}
