//! The generators of the design's generator table (Testing Strategy), one
//! module per generator. A module is filled by the first task of the plan
//! that names it.
//!
//! | Generator | Module | State |
//! |---|---|---|
//! | `arb_result` | `result` | Results with single-field mutations |
//! | `arb_cache_entry` | `cache_entry` | entries; one changed field |
//! | `arb_coordinator_ops` | `coordinator_ops` | inputs of the Coordinator machine |
//! | `arb_checker_tuple` | `checker_tuple` | the facts of a checker run |
//! | `arb_session_events` | `session_events` | inputs of the Front_End machine with their evidence |
//! | `arb_fault_schedule` | `fault_schedule` | crashes, restarts, partitions, the network; fair schedules |
//! | `arb_config_text` | `config_text` | settings and `dp.toml` texts, valid and invalid |
//! | `arb_printed_goal`, `arb_lemma`, `arb_hint`, `arb_provenance`, `arb_proof_skeleton`, `arb_partition_tree`, `arb_raw_eval` | | stages 5 to 8 |

pub mod cache_entry;
pub mod checker_tuple;
pub mod config_text;
pub mod coordinator_ops;
pub mod fault_schedule;
pub mod result;
pub mod session_events;
