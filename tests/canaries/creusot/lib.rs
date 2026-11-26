//! Canary functions at the level of Creusot specifications (R7.5): each
//! has a contract that the body does not meet, in one of the four places
//! where a specification can be wrong. `registry.toml` lists them, and
//! `witness` below evaluates each violation at a concrete input.
//!
//! The harness proves this file through `cargo creusot --backend` and
//! fails if a canary gets a proof label; the same file is a module of the
//! harness, so that the witnesses run on the code that was proved.

use creusot_std::prelude::*;

/// Not a canary: a function that meets its contract. A mode that does not
/// prove it has proved nothing.
#[requires(a@ + b@ <= u32::MAX@)]
#[ensures(result@ == a@ + b@)]
pub fn control(a: u32, b: u32) -> u32 {
    a + b
}

/// r01, postcondition: the result is never greater than the argument.
#[ensures(result@ > a@)]
pub fn r01_post(a: u32) -> u32 {
    a
}

#[requires(x@ > 0)]
#[ensures(result == x)]
pub fn positive(x: u32) -> u32 {
    x
}

/// The argument that `r02_pre` passes.
pub const R02_ARGUMENT: u32 = 0;

/// r02, precondition: the callee requires a positive argument.
pub fn r02_pre() -> u32 {
    positive(R02_ARGUMENT)
}

/// r03, loop invariant: `i < n` does not hold when the loop is entered
/// with `n == 0`, nor after the last iteration.
#[ensures(result == n)]
pub fn r03_loop(n: u32) -> u32 {
    let mut i = 0;
    #[invariant(i@ < n@)]
    while i < n {
        i += 1;
    }
    i
}

/// A number that is even.
pub struct Even(pub u32);

impl Invariant for Even {
    #[logic(open)]
    fn invariant(self) -> bool {
        pearlite! { self.0@ % 2 == 0 }
    }
}

/// The number that `r04_type` wraps.
pub const R04_VALUE: u32 = 3;

/// r04, type invariant: the value built is odd.
pub fn r04_type() -> Even {
    Even(R04_VALUE)
}

/// Whether the canary's specification is violated at its recorded input,
/// by running the code (R7.5: finite evaluation). Not given to Creusot.
#[cfg(not(creusot))]
pub fn witness(id: &str) -> Option<bool> {
    Some(match id {
        // The postcondition `result > a` at a = 5.
        "r01" => !(r01_post(5) > 5),
        // The precondition `x > 0` at the argument of the call.
        "r02" => !(R02_ARGUMENT > 0),
        // The invariant `i < n` at loop entry (i = 0) for n = 0.
        "r03" => !(0 < 0u32) && r03_loop(0) == 0,
        // The invariant `self.0 % 2 == 0` of the value built.
        "r04" => r04_type().0 % 2 != 0,
        _ => return None,
    })
}
