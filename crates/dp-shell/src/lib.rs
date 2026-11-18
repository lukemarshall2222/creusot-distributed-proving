//! Shared `Env` abstraction and adapters (transport, clock, process runner,
//! file store, trace writer) for the I/O shells and for `tests/sim/`, and the
//! client of the why3find bridge (design: "How the shells drive dp-core").
pub mod why3find;
pub mod solver;
pub mod net;
pub mod store;
pub mod trace;
