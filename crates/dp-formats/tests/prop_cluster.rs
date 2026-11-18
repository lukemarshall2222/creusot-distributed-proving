// Feature: creusot-distributed-proving, Property 3: Accepted Results are bound to the local print, the submission and an authentic Worker
// P-ID: P7.1.7 (authentication scheme), P7.10.1 (cluster messages)

use dp_formats::cluster::*;
use dp_formats::hash::Hash;
use dp_formats::sexp::{parse, print};
use ed25519_dalek::SigningKey;
use proptest::prelude::*;

fn s() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9@._/ -]{0,12}"
}

fn blob() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..40)
}

fn job() -> impl Strategy<Value = Job> {
    (s(), s(), blob(), s(), any::<u64>(), blob()).prop_map(
        |(job_id, prover, driver, extension, micros, goal)| Job { job_id, prover, driver, extension, micros, goal },
    )
}

fn fields() -> impl Strategy<Value = ResultFields> {
    (s(), any::<[u8; 32]>(), s(), any::<[u8; 32]>(), s(), any::<Option<u32>>(), any::<bool>(), blob(), any::<u64>())
        .prop_map(|(job_id, task_hash, prover, binary_hash, worker, status, timed_out, output, elapsed_micros)| {
            ResultFields {
                job_id,
                task_hash: Hash(task_hash),
                prover,
                binary_hash: Hash(binary_hash),
                worker,
                status,
                timed_out,
                output,
                elapsed_micros,
            }
        })
}

fn key() -> impl Strategy<Value = SigningKey> {
    any::<[u8; 32]>().prop_map(|b| SigningKey::from_bytes(&b))
}

fn message() -> impl Strategy<Value = Message> {
    prop_oneof![
        s().prop_map(|session| Message::FrontEndHello { session }),
        (s(), proptest::collection::vec(s(), 0..4))
            .prop_map(|(worker, provers)| Message::WorkerHello { worker, provers }),
        job().prop_map(Message::Submit),
        job().prop_map(Message::Assign),
        s().prop_map(|job_id| Message::Cancel { job_id }),
        (fields(), key()).prop_map(|(f, k)| Message::Result(SignedResult::sign(f, &k))),
        (s(), s()).prop_map(|(job_id, reason)| Message::Failed { job_id, reason }),
    ]
}

/// One changed field of a Result.
fn mutate(f: &ResultFields, which: u8) -> ResultFields {
    let mut g = f.clone();
    match which % 9 {
        0 => g.job_id.push('x'),
        1 => g.task_hash.0[0] ^= 1,
        2 => g.prover.push('x'),
        3 => g.binary_hash.0[31] ^= 0x80,
        4 => g.worker.push('x'),
        5 => g.status = Some(g.status.map_or(0, |s| s.wrapping_add(1))),
        6 => g.timed_out = !g.timed_out,
        7 => g.output.push(b'u'),
        _ => g.elapsed_micros = g.elapsed_micros.wrapping_add(1),
    }
    g
}

proptest! {
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
