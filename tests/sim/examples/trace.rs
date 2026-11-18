//! Writes the Coordinator's trace of one simulated run with faults, for
//! trace validation against `verification/tla/Cluster.tla`.
//!
//!     cargo run -p dp-sim --example trace -- OUT_FILE SEED

use dp_sim::{Call, Cluster, PROVER, SILENCE, WORKERS};
use dp_testkit::sim::Faults;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(out), Some(seed)) = (args.next(), args.next().and_then(|s| s.parse::<u64>().ok())) else {
        eprintln!("usage: trace OUT_FILE SEED");
        std::process::exit(2);
    };
    // Calls of every kind, more than the Workers can run at once.
    let texts = ["unsat 30000", "unknown 20000", "hang", "unsat 90000", "sat 10000", "unsat 5000", "unknown 60000"];
    let script: Vec<Call> = (0..10u64)
        .map(|i| Call {
            at: i * 15_000,
            goal: i,
            prover: PROVER.into(),
            text: texts[((seed + i) % texts.len() as u64) as usize].as_bytes().to_vec(),
            micros: 100_000,
        })
        .collect();
    let faults = Faults { loss: 30_000, duplicate: 30_000, min_delay: 50, max_delay: 5_000 };
    let mut cluster = Cluster::new(seed, faults, 3, 0, script, 1 + seed % 2);
    cluster.trace_coordinator(&out);
    // A Worker is cut off, another crashes, and both come back.
    cluster.run_until(40_000);
    cluster.world.borrow_mut().isolate(WORKERS + (seed % 3) as u32);
    cluster.run_until(40_000 + 2 * SILENCE);
    cluster.world.borrow_mut().heal(WORKERS + (seed % 3) as u32);
    let other = WORKERS + ((seed + 1) % 3) as u32;
    cluster.crash(other, seed % 2 == 0);
    cluster.run_until(40_000 + 4 * SILENCE);
    cluster.start_worker(other);
    cluster.run_until(2_000_000);
    cluster.coordinator.as_mut().expect("the Coordinator runs").finish().expect("the trace is written");
}
