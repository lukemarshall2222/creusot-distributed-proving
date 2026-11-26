//! Functions that meet their contracts: the material of the specification
//! mutator (`mutator.rs`), which writes `mutants.rs` from this file. In
//! the harness they are controls: a mode that does not prove them has
//! proved nothing.
//!
//! Above each function, `// sample:` gives arguments that satisfy its
//! precondition, and `// bad:` arguments that violate it. Contracts are
//! one per line and use integer views, constants, arithmetic and
//! comparisons only, so that a mutated contract can also be evaluated.

use creusot_std::prelude::*;

// sample: (3, 4)
// bad: (u32::MAX, 1)
#[requires(a@ + b@ <= u32::MAX@)]
#[ensures(result@ == a@ + b@)]
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}

// sample: (3, 9)
#[ensures(result@ >= a@)]
#[ensures(result@ >= b@)]
pub fn larger(a: u32, b: u32) -> u32 {
    if a >= b {
        a
    } else {
        b
    }
}

// sample: (7, 2)
#[ensures(result@ <= a@)]
pub fn minus(a: u32, b: u32) -> u32 {
    if a >= b {
        a - b
    } else {
        0
    }
}

// sample: (5)
// bad: (u32::MAX)
#[requires(a@ < u32::MAX@)]
#[ensures(result@ == a@ + 1)]
pub fn next(a: u32) -> u32 {
    a + 1
}
