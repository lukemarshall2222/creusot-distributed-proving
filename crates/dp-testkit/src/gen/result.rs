//! `arb_result`: Results as Workers send them, signing keys, and
//! single-field mutations (Properties 3 and 4).

use dp_formats::{cluster::ResultFields, hash::Hash};
use ed25519_dalek::SigningKey;
use proptest::prelude::*;

/// Short texts of the kinds that name Jobs, provers and Workers.
pub fn text() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9@._/ -]{0,12}"
}

pub fn blob() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..40)
}

pub fn key() -> impl Strategy<Value = SigningKey> {
    any::<[u8; 32]>().prop_map(|bytes| SigningKey::from_bytes(&bytes))
}

/// The fields of a Result. The Calibration, the local limit and the
/// import-set hash are fixed here; `mutate` changes each of them.
pub fn fields() -> impl Strategy<Value = ResultFields> {
    (text(), any::<[u8; 32]>(), text(), any::<Option<u64>>(), any::<[u8; 32]>(), text(), any::<Option<u32>>(), any::<bool>(), blob(), any::<u64>())
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
                velocity: 1_000_000,
                local_micros: 7,
                imports_hash: Hash([3; 32]),
            }
        })
}

/// The number of fields that `mutate` can change: every signed field.
pub const FIELDS: u8 = 13;

/// One field of a Result changed: the field `which % FIELDS`.
pub fn mutate(f: &ResultFields, which: u8) -> ResultFields {
    let mut g = f.clone();
    match which % FIELDS {
        0 => g.job_id.push('x'),
        1 => g.task_hash.0[0] ^= 1,
        2 => g.prover.push('x'),
        3 => g.binary_hash.0[31] ^= 0x80,
        4 => g.worker.push('x'),
        5 => g.status = Some(g.status.map_or(0, |s| s.wrapping_add(1))),
        6 => g.timed_out = !g.timed_out,
        7 => g.output.push(b'u'),
        8 => g.elapsed_micros = g.elapsed_micros.wrapping_add(1),
        9 => g.seed = Some(g.seed.map_or(0, |s| s.wrapping_add(1))),
        10 => g.velocity += 1,
        11 => g.local_micros += 1,
        _ => g.imports_hash.0[7] ^= 2,
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mutation_changes_exactly_one_thing() {
        let f = ResultFields {
            job_id: "j".into(),
            task_hash: Hash([1; 32]),
            prover: "p".into(),
            seed: None,
            binary_hash: Hash([2; 32]),
            worker: "w".into(),
            status: None,
            timed_out: false,
            output: Vec::new(),
            elapsed_micros: 0,
            velocity: 1,
            local_micros: 1,
            imports_hash: Hash([3; 32]),
        };
        let all: Vec<ResultFields> = (0..FIELDS).map(|which| mutate(&f, which)).collect();
        for (i, g) in all.iter().enumerate() {
            assert_ne!(*g, f, "mutation {i} changes nothing");
            assert!(all.iter().skip(i + 1).all(|h| h != g), "mutation {i} is another one again");
        }
        assert_eq!(mutate(&f, FIELDS), all[0]);
    }
}
