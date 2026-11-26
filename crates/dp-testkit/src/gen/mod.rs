//! The generators of the design's generator table (Testing Strategy), one
//! module per generator. A module is filled by the first task of the plan
//! that names it; the modules of generators that no built task needs yet
//! say which task fills them.
//!
//! | Generator | Module | State |
//! |---|---|---|
//! | `arb_result` | `result` | Results with single-field mutations |
//! | `arb_cache_entry` | `cache_entry` | entries; one changed field |
//! | `arb_coordinator_ops` | `coordinator_ops` | inputs of the Coordinator machine |
//! | `arb_checker_tuple` | `checker_tuple` | the facts of a checker run |
//! | `arb_session_events` | `session_events` | task 6.x: still in `prop_frontend.rs` |
//! | `arb_fault_schedule` | `fault_schedule` | task 17.x: still in `tests/sim/faults.rs` |
//! | `arb_config_text` | `config_text` | task 4.8 |
//! | `arb_printed_goal`, `arb_lemma`, `arb_hint`, `arb_provenance`, `arb_proof_skeleton`, `arb_partition_tree`, `arb_raw_eval` | | stages 5 to 8 |

pub mod cache_entry;
pub mod checker_tuple;
pub mod coordinator_ops;
pub mod result;

/// Task 4.8 fills this: Solver_Configuration files and option values.
pub mod config_text {}

/// Still in `tests/sim/faults.rs` (`schedule`); a later task of group 17
/// moves it here.
pub mod fault_schedule {}

/// Still in `crates/dp-core/tests/prop_frontend.rs` (`input`); a task of
/// group 6 moves it here.
pub mod session_events {}
