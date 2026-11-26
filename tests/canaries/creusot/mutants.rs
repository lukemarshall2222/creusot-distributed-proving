//! Canary functions that the specification mutator wrote from
//! `originals.rs`. Generated: do not edit; `cargo run -p dp-canaries
//! --example bless` rewrites this file.

use creusot_std::prelude::*;

/// m01: `add`, operator `flip`.
#[requires(a@ + b@ <= u32::MAX@)]
#[ensures(result@ != a@ + b@)]
pub fn m01_add_flip1(a: u32, b: u32) -> u32 {
    a + b
}

/// m02: `add`, operator `shift`.
#[requires(a@ + b@ <= u32::MAX@)]
#[ensures(result@ == a@ + b@ + 1)]
pub fn m02_add_shift1(a: u32, b: u32) -> u32 {
    a + b
}

/// m03: `add`, operator `drop`.
#[ensures(result@ == a@ + b@)]
pub fn m03_add_drop(a: u32, b: u32) -> u32 {
    a + b
}

/// m04: `larger`, operator `flip`.
#[ensures(result@ < a@)]
#[ensures(result@ >= b@)]
pub fn m04_larger_flip1(a: u32, b: u32) -> u32 {
    if a >= b {
        a
    } else {
        b
    }
}

/// m05: `larger`, operator `flip`.
#[ensures(result@ >= a@)]
#[ensures(result@ < b@)]
pub fn m05_larger_flip2(a: u32, b: u32) -> u32 {
    if a >= b {
        a
    } else {
        b
    }
}

/// m06: `minus`, operator `flip`.
#[ensures(result@ > a@)]
pub fn m06_minus_flip1(a: u32, b: u32) -> u32 {
    if a >= b {
        a - b
    } else {
        0
    }
}

/// m07: `next`, operator `flip`.
#[requires(a@ < u32::MAX@)]
#[ensures(result@ != a@ + 1)]
pub fn m07_next_flip1(a: u32) -> u32 {
    a + 1
}

/// m08: `next`, operator `shift`.
#[requires(a@ < u32::MAX@)]
#[ensures(result@ == a@ + 1 + 1)]
pub fn m08_next_shift1(a: u32) -> u32 {
    a + 1
}

/// m09: `next`, operator `drop`.
#[ensures(result@ == a@ + 1)]
pub fn m09_next_drop(a: u32) -> u32 {
    a + 1
}

/// Identifier, function and operator of every mutant.
#[cfg(not(creusot))]
pub const MUTANTS: &[(&str, &str, &str)] = &[
    ("m01", "m01_add_flip1", "flip"),
    ("m02", "m02_add_shift1", "shift"),
    ("m03", "m03_add_drop", "drop"),
    ("m04", "m04_larger_flip1", "flip"),
    ("m05", "m05_larger_flip2", "flip"),
    ("m06", "m06_minus_flip1", "flip"),
    ("m07", "m07_next_flip1", "flip"),
    ("m08", "m08_next_shift1", "shift"),
    ("m09", "m09_next_drop", "drop"),
];

/// Whether the mutant's contract is violated at its recorded arguments,
/// by running its body.
#[cfg(not(creusot))]
#[allow(clippy::all, unused_parens)]
pub fn witness(id: &str) -> Option<bool> {
    Some(match id {
        "m01" => { let a: u32 = 3; let b: u32 = 4; let result = m01_add_flip1(a, b); !((result as i128) != (a as i128) + (b as i128)) },
        "m02" => { let a: u32 = 3; let b: u32 = 4; let result = m02_add_shift1(a, b); !((result as i128) == (a as i128) + (b as i128) + 1) },
        "m04" => { let a: u32 = 3; let b: u32 = 9; let result = m04_larger_flip1(a, b); !((result as i128) < (a as i128)) },
        "m05" => { let a: u32 = 3; let b: u32 = 9; let result = m05_larger_flip2(a, b); !((result as i128) < (b as i128)) },
        "m06" => { let a: u32 = 7; let b: u32 = 2; let result = m06_minus_flip1(a, b); !((result as i128) > (a as i128)) },
        "m07" => { let a: u32 = 5; let result = m07_next_flip1(a); !((result as i128) != (a as i128) + 1) },
        "m08" => { let a: u32 = 5; let result = m08_next_shift1(a); !((result as i128) == (a as i128) + 1 + 1) },
        _ => return None,
    })
}

/// Calls a mutant of the operator `drop` at arguments that violate the
/// precondition that it lost. The call does not return: the body overflows.
#[cfg(not(creusot))]
pub fn call_bad(id: &str) {
    match id {
        "m03" => {
            m03_add_drop(u32::MAX, 1);
        }
        "m09" => {
            m09_next_drop(u32::MAX);
        }
        _ => {}
    }
}
