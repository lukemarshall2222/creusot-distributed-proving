//! Pure decision core (design: "The dp-core decision core", R7.3).
//!
//! Every decision function of R7.3 criterion 1 lives here as a deterministic
//! function or as a step of one of four state machines (Front_End, Coordinator,
//! Worker, Lemma_Pool). This crate takes every external input as a function
//! argument and uses no file, network, clock, randomness, thread, process or
//! global mutable state interface (R7.3 criterion 2). The I/O shells compute
//! facts and carry out the effects that this crate returns.

pub mod cache_key;
pub mod calibration;
pub mod config;
pub mod coord;
pub mod frontend;
pub mod labels;
pub mod machine;
pub mod trace;
pub mod verdict;
pub mod worker;
