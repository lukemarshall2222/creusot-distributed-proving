// Feature: creusot-distributed-proving, Property 3: Accepted Results are bound to the local print, the submission and an authentic Worker
// P-ID: P7.1.7 (authentication scheme), P7.10.1 (cluster messages)

use dp_formats::cluster::*;
use dp_formats::sexp::{parse, print};
use dp_testkit::gen::message::{command, message};
use dp_testkit::gen::result::{fields, key, mutate, text as s};
use proptest::prelude::*;

proptest! {
    #![proptest_config(dp_testkit::config::cases("prop_cluster", 256))]

    #[test]
    fn messages_round_trip(m in message()) {
        prop_assert_eq!(Message::decode(&parse(&print(&m.encode()), 1 << 20).unwrap()), Ok(m));
    }

    #[test]
    fn a_tag_verifies_for_its_worker_and_fields_only(f in fields(), k in key(), other in key(), which in any::<u8>()) {
        let signed = SignedResult::sign(f.clone(), &k);
        prop_assert!(signed.verifies(&k.verifying_key()));
        // another Worker's key does not verify it
        prop_assume!(other.to_bytes() != k.to_bytes());
        prop_assert!(!signed.verifies(&other.verifying_key()));
        // no single changed field keeps the tag valid
        let forged = SignedResult { fields: mutate(&f, which), tag: signed.tag };
        prop_assert!(!forged.verifies(&k.verifying_key()));
        // a Worker cannot sign for another Worker's key
        let impostor = SignedResult::sign(f, &other);
        prop_assert!(!impostor.verifies(&k.verifying_key()));
    }

    #[test]
    fn a_registry_is_accepted_only_with_the_operator_signature(
        version in any::<u64>(),
        workers in proptest::collection::vec((s(), any::<[u8; 32]>()), 0..4),
        operator in key(),
        other in key(),
        at in any::<prop::sample::Index>(),
    ) {
        let registry = Registry { version, workers };
        let file = registry.sign(&operator);
        prop_assert_eq!(Registry::verify(&file, &operator.verifying_key(), 1 << 20), Ok(registry));
        prop_assume!(other.to_bytes() != operator.to_bytes());
        prop_assert!(Registry::verify(&file, &other.verifying_key(), 1 << 20).is_err());
        let mut tampered = file.clone();
        let i = at.index(tampered.len());
        tampered[i] ^= 1;
        prop_assert!(Registry::verify(&tampered, &operator.verifying_key(), 1 << 20).is_err());
    }
}

#[test]
fn hex_round_trips_and_rejects_bad_text() {
    let bytes = [0u8, 1, 0xab, 0xff];
    assert_eq!(to_hex(&bytes), "0001abff");
    assert_eq!(from_hex::<4>("0001abff\n"), Some(bytes));
    assert_eq!(from_hex::<4>("0001abf"), None);
    assert_eq!(from_hex::<4>("0001abfg"), None);
    assert_eq!(from_hex::<3>("0001abff"), None);
}

proptest! {
    #![proptest_config(dp_testkit::config::cases("prop_cluster", 256))]

    // P-ID: P1.7.6
    #[test]
    fn an_operator_command_verifies_only_as_signed(c in command(), other in key(), which in 0u8..3) {
        // `command` signs with its own key; recover it by signing again.
        let mut changed = c.clone();
        match which {
            0 => changed.serial = changed.serial.wrapping_add(1),
            1 => changed.worker.push('x'),
            _ => changed.operation = if changed.operation == Operation::Revoke { Operation::Drain } else { Operation::Revoke },
        }
        let signer = SignedCommand::sign(c.serial, c.operation, &c.worker, &other);
        prop_assert!(signer.verifies(&other.verifying_key()));
        let mut forged = signer.clone();
        forged.serial = changed.serial;
        forged.worker = changed.worker.clone();
        forged.operation = changed.operation;
        prop_assert!(!forged.verifies(&other.verifying_key()));
    }
}
