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
    (any::<bool>(), s(), s(), blob(), s(), any::<u64>(), any::<Option<u64>>(), blob()).prop_map(
        |(certify, job_id, prover, driver, extension, micros, seed, goal)| Job {
            kind: if certify { JobKind::Certify } else { JobKind::Prove },
            job_id,
            prover,
            driver,
            extension,
            micros,
            seed,
            goal,
        },
    )
}

fn fields() -> impl Strategy<Value = ResultFields> {
    (s(), any::<[u8; 32]>(), s(), any::<Option<u64>>(), any::<[u8; 32]>(), s(), any::<Option<u32>>(), any::<bool>(), blob(), any::<u64>())
        .prop_map(|(job_id, task_hash, prover, seed, binary_hash, worker, status, timed_out, output, elapsed_micros)| {
            ResultFields {
                job_id,
                task_hash: Hash(task_hash),
                prover,
                seed,
                binary_hash: Hash(binary_hash),
                worker,
                status,
                timed_out,
                output,
                elapsed_micros,
                // every signed field is covered by `mutate` below
                velocity: 1_000_000,
                local_micros: 7,
            }
        })
}

fn key() -> impl Strategy<Value = SigningKey> {
    any::<[u8; 32]>().prop_map(|b| SigningKey::from_bytes(&b))
}

fn command() -> impl Strategy<Value = SignedCommand> {
    (any::<u64>(), 0usize..3, s(), key()).prop_map(|(serial, op, worker, key)| {
        SignedCommand::sign(serial, [Operation::Drain, Operation::Remove, Operation::Revoke][op], &worker, &key)
    })
}

fn message() -> impl Strategy<Value = Message> {
    prop_oneof![
        s().prop_map(|session| Message::FrontEndHello { session }),
        (s(), proptest::collection::vec((s(), any::<u64>()), 0..4))
            .prop_map(|(worker, offers)| Message::WorkerHello { worker, offers }),
        Just(Message::Heartbeat),
        Just(Message::Idle),
        Just(Message::StatusRequest),
        any::<u64>().prop_map(|incarnation| Message::Welcome { incarnation }),
        (proptest::collection::vec((s(), proptest::option::of(s()), any::<bool>(), proptest::collection::vec(s(), 0..3)), 0..3), any::<u64>(), any::<u64>())
            .prop_map(|(workers, queued, front_ends)| Message::Status {
                incarnation: queued / 3,
                workers: workers
                    .into_iter()
                    .map(|(worker, job, draining, provers)| WorkerStatus { worker, job, draining, provers })
                    .collect(),
                queued,
                front_ends,
            }),
        (command(), any::<bool>()).prop_map(|(c, revocation)| if revocation { Message::Revocation(c) } else { Message::Operator(c) }),
        (any::<u64>(), any::<bool>()).prop_map(|(serial, taken)| Message::OperatorResult { serial, taken }),
        job().prop_map(Message::Submit),
        (job(), any::<u64>(), any::<u64>())
            .prop_map(|(job, local_micros, velocity)| Message::Assign { job, local_micros, velocity }),
        s().prop_map(|job_id| Message::Cancel { job_id }),
        (fields(), key()).prop_map(|(f, k)| Message::Result(SignedResult::sign(f, &k))),
        (s(), s()).prop_map(|(job_id, reason)| Message::Failed { job_id, reason }),
    ]
}

/// One changed field of a Result.
fn mutate(f: &ResultFields, which: u8) -> ResultFields {
    let mut g = f.clone();
    match which % 12 {
        10 => g.velocity += 1,
        11 => g.local_micros += 1,
        9 => g.seed = Some(g.seed.map_or(0, |s| s.wrapping_add(1))),
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

proptest! {
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
