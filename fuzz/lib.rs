//! Fuzz harnesses (R7.10 criterion 6; design: Testing Strategy).
//!
//! One module per target, in the directory of its name. A harness is a
//! function `run(data)` that feeds the bytes to a parser of untrusted
//! input and asserts what must hold of whatever the parser accepts; it
//! must not panic on any input. `seeds()` gives well-formed inputs to
//! start from.
//!
//! - `smoke.rs` (`ci/fuzz-smoke`) runs every harness on its seeds, on
//!   mutations of them and on random bytes, with fixed seeds.
//! - `<target>/fuzz_target.rs` is the entry for libFuzzer; the package in
//!   `libfuzzer/` builds them (`sched/fuzz-nightly`).

#[path = "cache_entry/harness.rs"]
pub mod cache_entry;
#[path = "cert_manifest/harness.rs"]
pub mod cert_manifest;
#[path = "checker_output/harness.rs"]
pub mod checker_output;
#[path = "config_parser/harness.rs"]
pub mod config_parser;
#[path = "proofjson/harness.rs"]
pub mod proofjson;
#[path = "result_message/harness.rs"]
pub mod result_message;

/// Size limit that the harnesses give the parsers.
pub const LIMIT: usize = 1 << 16;

/// A harness and its seeds, by the name of its directory.
pub type Target = (&'static str, fn(&[u8]), fn() -> Vec<Vec<u8>>);

pub const TARGETS: [Target; 6] = [
    ("result_message", result_message::run, result_message::seeds),
    ("proofjson", proofjson::run, proofjson::seeds),
    ("cache_entry", cache_entry::run, cache_entry::seeds),
    ("cert_manifest", cert_manifest::run, cert_manifest::seeds),
    ("config_parser", config_parser::run, config_parser::seeds),
    ("checker_output", checker_output::run, checker_output::seeds),
];
