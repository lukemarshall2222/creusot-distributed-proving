//! Domain-separated hashing (design: "Encoding, hashing and authentication",
//! TB-1).
//!
//! `H(tag, x…)` is SHA-256 over the canonical S-expression of a domain tag
//! and the arguments. Domain tags keep a hash of one kind of value from
//! equalling a hash of another kind.

use sha2::{Digest, Sha256};

use crate::sexp::{self, Sexp};

/// A SHA-256 digest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash(pub [u8; 32]);

impl Hash {
    pub fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl std::fmt::Debug for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sha256:{}", self.to_hex())
    }
}

pub const TAG_TASK: &str = "dp/task/v1";
pub const TAG_GOAL: &str = "dp/goal/v1";
pub const TAG_IMPORTS: &str = "dp/imports/v1";
pub const TAG_CACHE: &str = "dp/cache/v1";

/// `H(tag, args…)`.
pub fn h(tag: &str, args: &[Sexp]) -> Hash {
    let mut items = Vec::with_capacity(args.len() + 1);
    items.push(Sexp::atom(tag));
    items.extend_from_slice(args);
    Hash(Sha256::digest(sexp::print(&Sexp::List(items))).into())
}

/// SHA-256 of raw bytes, for content hashes of files such as solver binaries.
pub fn sha256(bytes: &[u8]) -> Hash {
    Hash(Sha256::digest(bytes).into())
}

/// `goal_hash = H("dp/goal/v1", task_print)`.
pub fn goal_hash(task_print: &[u8]) -> Hash {
    h(TAG_GOAL, &[Sexp::atom(task_print)])
}

/// `task_hash = H("dp/task/v1", printed_goal_bytes, driver.hash, pipeline.hash)`.
pub fn task_hash(printed_goal: &[u8], driver_hash: &[u8], pipeline_hash: &[u8]) -> Hash {
    h(TAG_TASK, &[Sexp::atom(printed_goal), Sexp::atom(driver_hash), Sexp::atom(pipeline_hash)])
}

/// `imports_hash = H("dp/imports/v1", imports)`.
pub fn imports_hash(imports: &[Hash]) -> Hash {
    h(TAG_IMPORTS, &[Sexp::list(imports.iter().map(|i| Sexp::atom(i.0)))])
}
