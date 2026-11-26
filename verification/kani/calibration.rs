//! Kani harnesses for `calibration::scale` (R1.4, P1.4.3). They are a
//! module of dp-core under `cfg(kani)`; `ci/jobs/kani.sh` runs them.
//!
//! Creusot proves the contract of `scale` over the mathematical integers,
//! for every input. These harnesses check the compiled arithmetic within
//! recorded bounds: there it neither overflows nor divides by zero, and
//! its result is the rounded-down product. The property tests of
//! `prop_calibration.rs` cover the rest of `u64` by sampling.

use crate::calibration::scale;

/// The recorded bounds: reference limits up to 256 microseconds, and
/// velocities up to 2^21 millionths, which is about twice the speed of the
/// reference machine. Kani's solver decides the 64-bit multiplications and
/// divisions of `scale` in seconds within them, and not in minutes with
/// 2^24 and 2^22.
const MICROS_BOUND: u64 = 1 << 8;
const VELOCITY_BOUND: u64 = 1 << 21;

/// Within the bounds, `scale` is `micros * velocity / 10^6`, rounded down.
#[kani::proof]
fn scale_is_the_rounded_down_product() {
    let micros: u64 = kani::any();
    let velocity: u64 = kani::any();
    kani::assume(micros <= MICROS_BOUND && velocity <= VELOCITY_BOUND);
    let exact = u128::from(micros) * u128::from(velocity) / 1_000_000;
    assert!(u128::from(scale(micros, velocity)) == exact);
}

/// The reference machine keeps a limit, and a machine of velocity zero
/// gives the limit zero, within the bound.
#[kani::proof]
fn the_reference_velocity_is_the_identity() {
    let micros: u64 = kani::any();
    kani::assume(micros <= MICROS_BOUND);
    assert!(scale(micros, 1_000_000) == micros);
    assert!(scale(micros, 0) == 0);
}
