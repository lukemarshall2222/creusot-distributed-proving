//! Writes the Coordinator's trace of one simulated run with faults, for
//! trace validation against `verification/tla/Cluster.tla`.
//!
//!     cargo run -p dp-sim --example trace -- OUT_FILE SEED

use dp_formats::cluster::Operation;
use dp_sim::{operator_key, Call, Cluster, COORDINATOR, PROVER, SILENCE, WORKERS};
use dp_testkit::sim::Faults;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(out), Some(seed)) = (args.next(), args.next().and_then(|s| s.parse::<u64>().ok())) else {
        eprintln!("usage: trace OUT_FILE SEED");
        std::process::exit(2);
    };
    // Calls of every kind, more than the Workers can run at once.
    let texts = ["unsat 30000", "unknown 20000", "hang", "unsat 90000", "sat 10000", "unsat 5000", "unknown 60000"];
    let script: Vec<Call> = (0..16u64)
        .map(|i| Call {
            at: if i < 10 { i * 15_000 } else { 560_000 + (i - 10) * 15_000 },
            goal: i,
            prover: PROVER.into(),
            text: texts[((seed + i) % texts.len() as u64) as usize].as_bytes().to_vec(),
            micros: 100_000,
        })
        .collect();
    let faults = Faults { loss: 30_000, duplicate: 30_000, min_delay: 50, max_delay: 5_000 };
    let mut cluster = Cluster::new(seed, faults, 3, 0, script, 1 + seed % 2);
    cluster.trace_coordinator("trace");
    // A Worker is cut off, another crashes, and both come back.
    cluster.run_until(40_000);
    cluster.world.borrow_mut().isolate(WORKERS + (seed % 3) as u32);
    cluster.run_until(40_000 + 2 * SILENCE);
    cluster.world.borrow_mut().heal(WORKERS + (seed % 3) as u32);
    let other = WORKERS + ((seed + 1) % 3) as u32;
    cluster.crash(other, seed % 2 == 0);
    cluster.run_until(40_000 + 4 * SILENCE);
    cluster.start_worker(other);
    // The operator drains one Worker and removes another.
    cluster.run_until(40_000 + 5 * SILENCE);
    cluster.operate(1, Operation::Drain, WORKERS + (seed % 3) as u32, &operator_key());
    cluster.operate(2, if seed % 2 == 0 { Operation::Remove } else { Operation::Revoke }, other, &operator_key());
    cluster.run_until(2_000_000);
    cluster.coordinator.as_mut().expect("the Coordinator runs").finish().expect("the trace is written");
    // The trace is a file of the simulated machine; validation reads a real one.
    let trace = cluster.file(COORDINATOR, "trace").expect("the Coordinator wrote its trace");
    std::fs::write(&out, trace).expect("the trace file can be written");
}
