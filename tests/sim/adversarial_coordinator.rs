// Feature: creusot-distributed-proving, Property 2: Only bound, authentic, submitted Results are accepted
// P-ID: P1.3.3, P1.3.9, P7.1.4 (an adversarial Coordinator gets no proof label for what is not proved)

//! The real shells of the Front_End and of the Workers with an adversarial
//! Coordinator between them (`adversary.rs`), on the simulated network.
//!
//! Whatever the adversary does, a call is answered Valid only if the
//! solver's honest answer on the call's own Printed_Goal is a proof, and
//! no revocation that the operator did not sign is taken.

use std::collections::BTreeMap;

use dp_sim::{truth, Call, Cluster, Reply, MARGIN, PROVER, SILENCE};
use proptest::prelude::*;

fn call(at: u64, goal: u64, text: &str, micros: u64) -> Call {
    Call { at, goal, prover: PROVER.into(), text: text.as_bytes().to_vec(), micros }
}

/// Calls of which about half are provable, so that a Result moved from
/// one Job to another, or a Job run on the goal of another, would often
/// give a proof where there is none.
fn script(extra: &[(u64, bool)]) -> Vec<Call> {
    let mut calls = vec![
        call(0, 0, "unsat 30000", 100_000),
        call(1_000, 1, "unknown 20000", 100_000),
        call(2_000, 2, "sat 10000", 100_000),
        call(3_000, 3, "unsat 5000", 100_000),
        call(4_000, 4, "unknown 8000", 100_000),
        call(40_000, 5, "unsat 12000", 100_000),
        call(41_000, 6, "hang", 50_000),
        call(42_000, 7, "unknown 3000", 100_000),
    ];
    for (i, (at, provable)) in extra.iter().enumerate() {
        let text = if *provable { format!("unsat {}", 4_000 + 1_000 * i) } else { format!("unknown {}", 4_000 + 1_000 * i) };
        calls.push(call(60_000 + at, 8 + i as u64, &text, 100_000));
    }
    calls
}

fn horizon(script: &[Call]) -> u64 {
    script.iter().map(|c| c.at + 4 * (3 * c.micros + MARGIN)).max().unwrap_or(0) + 3 * SILENCE
}

/// Runs a Session against the adversary and checks every reply. Returns
/// the number of calls answered Valid and what the adversary did.
fn run(seed: u64, script: &[Call]) -> (usize, BTreeMap<&'static str, u64>) {
    let mut cluster = Cluster::adversarial(seed, 2, script.to_vec());
    cluster.run_until(horizon(script));
    let mut proved = 0;
    for (rid, call) in script.iter().enumerate() {
        if let Some((reply, _)) = cluster.answer(rid) {
            assert!(
                reply != Reply::Valid || truth(call) == Reply::Valid,
                "seed {seed}: call {rid} ({:?}) was answered Valid",
                String::from_utf8_lossy(&call.text)
            );
            proved += usize::from(reply == Reply::Valid);
        }
    }
    // P1.3.9: no revocation without the operator's signature.
    assert!(cluster.front_end.revoked().is_empty(), "seed {seed}: {:?}", cluster.front_end.revoked());
    let did = cluster.adversary.as_ref().unwrap().did.clone();
    (proved, did)
}

/// The adversary does every one of its things, and honest assignments
/// still prove: the property below is not about a Cluster that does
/// nothing.
#[test]
fn the_adversary_tries_everything_and_proves_only_what_is_provable() {
    let script = script(&[(0, true), (500, false), (900, true), (1_300, false)]);
    let mut total: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut proved = 0;
    for seed in 0..40 {
        let (p, did) = run(seed, &script);
        proved += p;
        for (what, n) in did {
            *total.entry(what).or_insert(0) += n;
        }
    }
    for what in [
        "assigned honestly",
        "assigned with another goal",
        "assigned with another time limit",
        "answered as a registered Worker",
        "answered as an invented Worker",
        "reported as failed",
        "withheld",
        "forwarded",
        "forwarded twice",
        "forwarded with a changed answer",
        "forwarded under another Job",
        "replayed",
        "dropped",
        "sent a revocation of its own",
    ] {
        assert!(total.get(what).copied().unwrap_or(0) > 0, "the adversary never {what}: {total:?}");
    }
    assert!(proved > 0, "no call was proved in any Session: {total:?}");
}

/// `sched/sim-nightly` runs more schedules: DP_SIM_CASES.
fn cases() -> u32 {
    std::env::var("DP_SIM_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(60)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), failure_persistence: None, ..ProptestConfig::default() })]

    /// P1.3.3: for any schedule of the adversary and any calls, the calls
    /// answered Valid are among those whose own Printed_Goal is provable.
    #[test]
    fn no_schedule_of_the_adversary_proves_what_is_not_provable(
        seed in any::<u64>(),
        extra in prop::collection::vec((0u64..20_000, any::<bool>()), 0..8),
    ) {
        run(seed, &script(&extra));
    }
}
