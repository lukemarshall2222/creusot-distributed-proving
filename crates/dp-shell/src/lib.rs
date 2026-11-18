//! Shared `Env` abstraction and adapters (transport, clock, process runner,
//! file store, trace writer) for the I/O shells and for `tests/sim/`, and the
//! client of the why3find bridge (design: "How the shells drive dp-core").
#[cfg(feature = "cluster")]
pub mod env;
pub mod why3find;
pub mod solver;
#[cfg(feature = "cluster")]
pub mod net;
pub mod store;
pub mod trace;
