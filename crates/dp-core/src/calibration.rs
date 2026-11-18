//! Calibration scaling of time limits (R1.4).
//!
//! why3find's Calibration of a prover on a machine is a velocity: local time
//! over reference time. It is carried in millionths.

#[allow(unused_imports)] // `Int` is used by logic functions only
use creusot_std::logic::Int;
use creusot_std::prelude::*;

/// `micros * velocity / 1_000_000`, rounded down, and the largest time if
/// that does not fit.
#[logic(open)]
pub fn scaled(micros: Int, velocity: Int) -> Int {
    pearlite! {
        if micros * velocity / 1_000_000 <= u64::MAX@ {
            micros * velocity / 1_000_000
        } else {
            u64::MAX@
        }
    }
}

/// The local time limit for a reference time limit of `micros` on a machine
/// with the given velocity: `micros * velocity / 1_000_000`, rounded down,
/// and the largest time if that does not fit.
///
/// It is a function of its two arguments only, so the Front_End can
/// recompute the limit that a run should have had (R1.4 criterion 6).
#[ensures(result@ == scaled(micros@, velocity@))]
pub fn scale(micros: u64, velocity: u64) -> u64 {
    let whole = micros / 1_000_000;
    let part = micros % 1_000_000;
    // micros * velocity / 10^6 = whole * velocity + part * velocity / 10^6,
    // and neither `part * velocity` nor the sum may overflow.
    if velocity != 0 && whole > u64::MAX / velocity {
        return u64::MAX;
    }
    let high = whole * velocity;
    let low = if velocity != 0 && part > u64::MAX / velocity {
        // Split the velocity the same way; `part` is below 10^6.
        part * (velocity / 1_000_000) + part * (velocity % 1_000_000) / 1_000_000
    } else {
        part * velocity / 1_000_000
    };
    if high > u64::MAX - low {
        u64::MAX
    } else {
        high + low
    }
}
