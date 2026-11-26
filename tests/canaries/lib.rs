//! The canary harness (R7.5); see `main.rs`, `registry.toml` and
//! `modes.toml`.

/// The canary functions at the level of Creusot specifications. They are
/// no part of this crate for Creusot: `cargo creusot` in this workspace
/// proves dp-core (`ci/creusot`), and these functions are false by design.
/// The harness gives them to Creusot as a crate of their own.
#[cfg(not(creusot))]
#[path = "creusot/lib.rs"]
pub mod creusot_canaries;

// Creusot refuses a crate that depends on `creusot-std` without using it.
#[cfg(creusot)]
#[allow(unused_imports)]
use creusot_std::prelude::*;
