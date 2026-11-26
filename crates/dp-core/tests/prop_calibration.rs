// Feature: creusot-distributed-proving, Property 7: Calibration scaling is exact, monotone and never overflows
// P-ID: P1.4.1, P1.4.3

//! Tests of `calibration::scale` (R1.4): the local time limit of a
//! reference limit on a machine of a given velocity.

use dp_core::calibration::scale;
use proptest::prelude::*;

/// The definition, in arithmetic that cannot overflow.
fn reference(micros: u64, velocity: u64) -> u64 {
    let exact = u128::from(micros) * u128::from(velocity) / 1_000_000;
    u64::try_from(exact).unwrap_or(u64::MAX)
}

fn value() -> impl Strategy<Value = u64> {
    prop_oneof![
        3 => 0u64..10_000_000,
        2 => any::<u64>(),
        1 => (0u32..64).prop_map(|s| 1u64 << s),
        1 => (0u32..64, 0u64..3).prop_map(|(s, d)| (1u64 << s).wrapping_sub(d)),
        1 => Just(u64::MAX),
        1 => Just(1_000_000u64),
    ]
}

#[test]
fn the_reference_machine_keeps_every_limit() {
    for micros in [0, 1, 999_999, 1_000_000, 1_000_001, u64::MAX / 2, u64::MAX] {
        assert_eq!(scale(micros, 1_000_000), micros);
        assert_eq!(scale(micros, 0), 0);
    }
    assert_eq!(scale(1_000_000, 2_500_000), 2_500_000);
    assert_eq!(scale(3, 500_000), 1); // rounded down
    assert_eq!(scale(u64::MAX, 2_000_000), u64::MAX); // the largest time
    // At the edges of the two overflow checks: the whole seconds times the
    // velocity just fit, and do not.
    assert_eq!(scale(1_000_000, 1 << 63), 1 << 63);
    assert_eq!(scale(2_000_000, 1 << 63), u64::MAX);
    assert_eq!(scale(1_000_000, u64::MAX), u64::MAX);
    assert_eq!(scale(1_999_999, u64::MAX), u64::MAX);
    assert_eq!(scale(999_999, u64::MAX), reference(999_999, u64::MAX));
    assert_eq!(scale(3_000_000, u64::MAX / 3), u64::MAX / 3 * 3);
    assert_eq!(scale(3_000_001, u64::MAX / 3), reference(3_000_001, u64::MAX / 3));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2000, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn scale_is_the_rounded_down_product_or_the_largest_time(micros in value(), velocity in value()) {
        prop_assert_eq!(scale(micros, velocity), reference(micros, velocity));
    }

    /// P1.4.3: a longer reference limit never gives a shorter local limit,
    /// nor does a slower machine.
    #[test]
    fn scale_is_monotone(a in value(), b in value(), velocity in value()) {
        let (low, high) = (a.min(b), a.max(b));
        prop_assert!(scale(low, velocity) <= scale(high, velocity));
        prop_assert!(scale(velocity, low) <= scale(velocity, high));
    }
}
