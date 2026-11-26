//! `arb_fault_schedule`: the faults of a simulated run (Properties 8, 9
//! and 11): crashes with and without notice, restarts, partitions and
//! their healing, at times of the run, and a network that loses,
//! duplicates and delays messages; unequal delays reorder them.
//!
//! Nodes are numbers of the schedule, not of a Cluster: the test that
//! applies a schedule says which machine a number stands for.

use proptest::prelude::*;

use crate::sim::Faults;

/// One fault of a schedule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The node stops; with `noticed`, its peers see their connections
    /// close.
    Crash { node: u32, noticed: bool },
    Restart { node: u32 },
    /// Nothing reaches the node and nothing leaves it.
    Isolate { node: u32 },
    Heal { node: u32 },
}

impl Fault {
    pub fn node(&self) -> u32 {
        match self {
            Fault::Crash { node, .. } | Fault::Restart { node } | Fault::Isolate { node } | Fault::Heal { node } => *node,
        }
    }
}

/// Up to `most` faults on the nodes `0..nodes` at times below `until`, in
/// the order of their times.
pub fn schedule_of(nodes: u32, until: u64, most: usize) -> impl Strategy<Value = Vec<(u64, Fault)>> {
    let fault = (0..nodes, 0u8..4, any::<bool>()).prop_map(|(node, kind, noticed)| match kind {
        0 => Fault::Crash { node, noticed },
        1 => Fault::Restart { node },
        2 => Fault::Isolate { node },
        _ => Fault::Heal { node },
    });
    proptest::collection::vec((0..until, fault), 0..=most).prop_map(|mut faults| {
        faults.sort_by_key(|(at, _)| *at);
        faults
    })
}

/// The schedule of the simulation tests: four nodes (one stands for the
/// Coordinator, three for Workers), up to seven faults in the first 400
/// milliseconds.
pub fn schedule() -> impl Strategy<Value = Vec<(u64, Fault)>> {
    schedule_of(4, 400_000, 7)
}

/// A schedule that leaves every node running and connected in the end:
/// after its faults, each node that is down is restarted and each one that
/// is cut off is healed, at the time `until`. Liveness properties need
/// such a schedule (the fairness switch of the design's generator table).
pub fn fair(nodes: u32, until: u64, most: usize) -> impl Strategy<Value = Vec<(u64, Fault)>> {
    schedule_of(nodes, until, most).prop_map(move |mut faults| {
        for node in 0..nodes {
            let (mut down, mut cut) = (false, false);
            for (_, fault) in faults.iter().filter(|(_, f)| f.node() == node) {
                match fault {
                    Fault::Crash { .. } => down = true,
                    Fault::Restart { .. } => down = false,
                    Fault::Isolate { .. } => cut = true,
                    Fault::Heal { .. } => cut = false,
                }
            }
            if down {
                faults.push((until, Fault::Restart { node }));
            }
            if cut {
                faults.push((until, Fault::Heal { node }));
            }
        }
        faults
    })
}

/// A network: half of the time without loss, half of the time without
/// duplicates, with delays between 50 microseconds and at most 30
/// milliseconds.
pub fn network() -> impl Strategy<Value = Faults> {
    (prop_oneof![Just(0u32), 0u32..200_000], prop_oneof![Just(0u32), 0u32..200_000], 100u64..30_000)
        .prop_map(|(loss, duplicate, max_delay)| Faults { loss, duplicate, min_delay: 50, max_delay })
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest! {
        #![proptest_config(crate::config::cases("fault_schedule", 200))]

        // A fair schedule is in the order of its times before its repairs,
        // and leaves no node down or cut off.
        #[test]
        fn a_fair_schedule_ends_with_every_node_up_and_connected(faults in fair(4, 400_000, 7)) {
            for node in 0..4 {
                let (mut down, mut cut) = (false, false);
                for (_, fault) in faults.iter().filter(|(_, f)| f.node() == node) {
                    match fault {
                        Fault::Crash { .. } => down = true,
                        Fault::Restart { .. } => down = false,
                        Fault::Isolate { .. } => cut = true,
                        Fault::Heal { .. } => cut = false,
                    }
                }
                prop_assert!(!down && !cut, "node {}: {:?}", node, faults);
            }
            prop_assert!(faults.windows(2).all(|w| w[0].0 <= w[1].0));
        }
    }
}
