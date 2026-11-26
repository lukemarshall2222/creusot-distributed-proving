//! The facts of acceptance that the Front_End establishes for every Result
//! before dp-core decides (A3 of R1.3, R7.1; design C3).
//!
//! Nothing here accepts or labels. A Result that arrives from the Cluster,
//! or that a stored entry holds, is untrusted bytes; this module computes
//! what is true of it with the Front_End's own data (the operator-signed
//! registry, the revocations held, the hash of the empty import set), and
//! dp-core's `accept_remote` decides from these facts and from the local
//! print.

use std::collections::BTreeSet;

use dp_core::{
    labels::ProverAnswer,
    verdict::{self, RemoteFacts},
};
use dp_formats::{
    cache::Entry,
    cluster::{Registry, SignedResult},
    dispatch::DriverAnswer,
    hash::{self, Hash},
};

/// A hash as dp-core compares it: four words.
pub fn core_hash(h: Hash) -> verdict::Hash {
    let w = |i: usize| u64::from_be_bytes(h.0[8 * i..8 * i + 8].try_into().expect("8 bytes"));
    verdict::Hash { w0: w(0), w1: w(1), w2: w(2), w3: w(3) }
}

/// The answer that the pinned driver read, as dp-core classifies it.
pub fn core_answer(answer: DriverAnswer) -> ProverAnswer {
    match answer {
        DriverAnswer::Valid => ProverAnswer::Valid,
        DriverAnswer::Invalid => ProverAnswer::Invalid,
        DriverAnswer::Timeout => ProverAnswer::Timeout,
        DriverAnswer::OutOfMemory => ProverAnswer::OutOfMemory,
        DriverAnswer::StepLimitExceeded => ProverAnswer::StepLimitExceeded,
        DriverAnswer::Unknown => ProverAnswer::Unknown,
        DriverAnswer::Failure => ProverAnswer::Failure,
        DriverAnswer::HighFailure => ProverAnswer::HighFailure,
    }
}

pub fn driver_answer(answer: ProverAnswer) -> DriverAnswer {
    match answer {
        ProverAnswer::Valid => DriverAnswer::Valid,
        ProverAnswer::Invalid => DriverAnswer::Invalid,
        ProverAnswer::Timeout => DriverAnswer::Timeout,
        ProverAnswer::OutOfMemory => DriverAnswer::OutOfMemory,
        ProverAnswer::StepLimitExceeded => DriverAnswer::StepLimitExceeded,
        ProverAnswer::Unknown => DriverAnswer::Unknown,
        ProverAnswer::Failure => DriverAnswer::Failure,
        ProverAnswer::HighFailure => DriverAnswer::HighFailure,
    }
}

/// Who signed a Result, in the Front_End's view: the registry that the
/// operator signed, and the revocations that the operator signed (R1.3
/// criterion 9). Without a registry nobody is registered.
fn signer(signed: &SignedResult, registry: Option<&Registry>, revoked: &BTreeSet<String>) -> (bool, bool, bool) {
    let key = registry.and_then(|registry| registry.key(&signed.fields.worker));
    (key.is_some(), revoked.contains(&signed.fields.worker), key.is_some_and(|k| signed.verifies(&k)))
}

/// The import-set hash that a Result carries is the hash of the imports
/// that it lists (R1.3 criterion 7). A Result lists none.
fn imports_consistent(signed: &SignedResult) -> bool {
    signed.fields.imports_hash == hash::imports_hash(&[])
}

/// The facts of a Result that the Coordinator forwarded. Whether it is
/// for a Job as submitted, with the time limit of that Job, dp-core
/// decides from the prover, the seed, the Calibration and the limit that
/// the shell passes with the Result; those two facts are left true here.
pub fn result_facts(signed: &SignedResult, registry: Option<&Registry>, revoked: &BTreeSet<String>) -> RemoteFacts {
    let (registered, revoked, signature_valid) = signer(signed, registry, revoked);
    RemoteFacts {
        registered,
        revoked,
        signature_valid,
        submitted: true,
        limits_consistent: true,
        imports_consistent: imports_consistent(signed),
    }
}

/// The facts of a Worker's Result that a Result_Cache entry holds, for a
/// lookup of the run of `prover` with `seed`. The Result is checked again
/// with the registry and the revocations of this Session (R7.5 criterion
/// 4): the store is not trusted.
pub fn stored_result_facts(
    signed: &SignedResult,
    registry: Option<&Registry>,
    revoked: &BTreeSet<String>,
    prover: &str,
    seed: Option<u64>,
    entry: &Entry,
) -> RemoteFacts {
    let (registered, revoked, signature_valid) = signer(signed, registry, revoked);
    RemoteFacts {
        registered,
        revoked,
        signature_valid,
        // The signed Result is for the run that was looked up, by the
        // binary that the entry's validation fields name.
        submitted: signed.fields.prover == prover
            && signed.fields.seed == seed
            && signed.fields.binary_hash == entry.binary_hash,
        // The entry's own time limits were validated; the signed ones
        // must be the same.
        limits_consistent: signed.fields.velocity == entry.velocity
            && signed.fields.local_micros == entry.local_micros,
        imports_consistent: imports_consistent(signed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dp_core::verdict::{accept_remote, Acceptance, Context, ResultFields as CoreFields};
    use dp_formats::{
        cache::Origin,
        cluster::ResultFields,
    };
    use ed25519_dalek::SigningKey;

    fn key(n: u8) -> SigningKey {
        SigningKey::from_bytes(&[n; 32])
    }

    fn fields() -> ResultFields {
        ResultFields {
            job_id: "s/1/0".into(),
            task_hash: Hash([1; 32]),
            prover: "z3@".into(),
            seed: None,
            binary_hash: Hash([2; 32]),
            worker: "w1".into(),
            status: Some(0),
            timed_out: false,
            output: b"unsat\n".to_vec(),
            elapsed_micros: 10,
            velocity: 1_000_000,
            local_micros: 5,
            imports_hash: hash::imports_hash(&[]),
        }
    }

    fn registry() -> Registry {
        Registry { version: 1, workers: vec![("w1".into(), key(1).verifying_key().to_bytes())] }
    }

    fn all(f: RemoteFacts) -> bool {
        f.registered && !f.revoked && f.signature_valid && f.submitted && f.limits_consistent && f.imports_consistent
    }

    /// dp-core's decision on the facts, with a Result that is bound to the
    /// local print.
    fn accepted(f: RemoteFacts) -> bool {
        let h = core_hash(Hash([1; 32]));
        let r = CoreFields {
            answer: ProverAnswer::Valid,
            task_hash: h,
            partition: false,
            imports: 0,
            imports_hash: h,
            prover: 1,
            version: 1,
            binary_hash: h,
        };
        let ctx = Context {
            certify_all: false,
            import_mark: false,
            local_task_hash: h,
            task_count: 1,
            empty_imports_hash: h,
            local_binary_mismatch: false,
            unpatched: Vec::new(),
        };
        accept_remote(r, &ctx, f) == Acceptance::Accepted
    }

    #[test]
    fn an_authentic_result_of_a_registered_worker_has_every_fact() {
        let signed = SignedResult::sign(fields(), &key(1));
        let facts = result_facts(&signed, Some(&registry()), &BTreeSet::new());
        assert!(all(facts) && accepted(facts));
    }

    #[test]
    fn each_way_in_which_a_result_is_not_to_be_believed_is_a_missing_fact() {
        let none = BTreeSet::new();
        let honest = SignedResult::sign(fields(), &key(1));
        // Signed with a key that the registry does not hold for the Worker.
        let forged = SignedResult::sign(fields(), &key(9));
        let facts = result_facts(&forged, Some(&registry()), &none);
        assert!(facts.registered && !facts.signature_valid && !accepted(facts));
        // A field changed after signing.
        let mut tampered = honest.clone();
        tampered.fields.output = b"sat\n".to_vec();
        assert!(!result_facts(&tampered, Some(&registry()), &none).signature_valid);
        // A Worker that the registry does not list, also when the Result
        // verifies under a key of its own.
        let mut stranger = fields();
        stranger.worker = "w7".into();
        let facts = result_facts(&SignedResult::sign(stranger, &key(7)), Some(&registry()), &none);
        assert!(!facts.registered && !facts.signature_valid && !accepted(facts));
        // No registry: nobody is registered.
        assert!(!result_facts(&honest, None, &none).registered);
        // A revoked Worker, with a valid tag.
        let revoked: BTreeSet<String> = ["w1".to_string()].into();
        let facts = result_facts(&honest, Some(&registry()), &revoked);
        assert!(facts.revoked && facts.signature_valid && !accepted(facts));
        // An import-set hash that is not the hash of the imports listed,
        // under a valid tag.
        let mut lying = fields();
        lying.imports_hash = Hash([7; 32]);
        let facts = result_facts(&SignedResult::sign(lying, &key(1)), Some(&registry()), &none);
        assert!(facts.signature_valid && !facts.imports_consistent && !accepted(facts));
    }

    #[test]
    fn a_stored_result_is_for_the_run_that_was_looked_up() {
        let none = BTreeSet::new();
        let signed = SignedResult::sign(fields(), &key(1));
        let entry = Entry {
            lookup: Vec::new(),
            binary_hash: Hash([2; 32]),
            velocity: 1_000_000,
            local_micros: 5,
            imports_hash: hash::imports_hash(&[]),
            answer: DriverAnswer::Valid,
            micros: 10,
            origin: Origin::Worker(signed.clone()),
        };
        let facts = |prover: &str, seed: Option<u64>, entry: &Entry| {
            stored_result_facts(&signed, Some(&registry()), &none, prover, seed, entry)
        };
        assert!(all(facts("z3@", None, &entry)) && accepted(facts("z3@", None, &entry)));
        // Another prover, another seed, another binary: not this run.
        assert!(!facts("cvc5@", None, &entry).submitted);
        assert!(!facts("z3@", Some(3), &entry).submitted);
        let other_binary = Entry { binary_hash: Hash([3; 32]), ..entry.clone() };
        assert!(!facts("z3@", None, &other_binary).submitted);
        // Time limits that are not the signed ones.
        let other_limit = Entry { local_micros: 6, ..entry.clone() };
        assert!(!facts("z3@", None, &other_limit).limits_consistent && !accepted(facts("z3@", None, &other_limit)));
        let other_velocity = Entry { velocity: 2_000_000, ..entry.clone() };
        assert!(!facts("z3@", None, &other_velocity).limits_consistent);
        // A Worker revoked since the entry was stored.
        let revoked: BTreeSet<String> = ["w1".to_string()].into();
        assert!(stored_result_facts(&signed, Some(&registry()), &revoked, "z3@", None, &entry).revoked);
    }
}
