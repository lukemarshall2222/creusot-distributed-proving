//! The shared configuration of the property tests (R7.11 criterion 4).
//!
//! Every property test takes its configuration from `cases`: at least 100
//! cases, and more in the nightly jobs, which set `DP_PROPTEST_FACTOR` to
//! multiply the number of every test. A failure is kept as a seed in the
//! committed directory `tests/proptest-regressions/`, in one file per test
//! file, so that it is tried again first on every later run.

use proptest::test_runner::{Config, FileFailurePersistence};

/// The fewest cases that a property test runs.
pub const MIN_CASES: u32 = 100;

/// The factor of the nightly jobs: `DP_PROPTEST_FACTOR`, 1 without it.
pub fn factor() -> u32 {
    std::env::var("DP_PROPTEST_FACTOR").ok().and_then(|v| v.parse().ok()).filter(|f| *f >= 1).unwrap_or(1)
}

/// The number of cases of a test that asks for `base` cases.
pub fn case_count(base: u32, factor: u32) -> u32 {
    base.max(MIN_CASES).saturating_mul(factor.max(1))
}

/// The configuration of a property test of the test file `file` (its
/// name without `.rs`) that asks for `base` cases.
pub fn cases(file: &'static str, base: u32) -> Config {
    // One file per test file, next to the other regression seeds. The
    // path is from the directory of the crate that the test belongs to.
    let regressions: &'static str = Box::leak(
        format!("{}/../../tests/proptest-regressions/{file}.txt", env!("CARGO_MANIFEST_DIR")).into_boxed_str(),
    );
    Config {
        cases: case_count(base, factor()),
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(regressions))),
        ..Config::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // P-ID: P7.11.4
    #[test]
    fn no_test_runs_fewer_than_the_minimum_and_the_factor_multiplies() {
        assert_eq!(case_count(1, 1), MIN_CASES);
        assert_eq!(case_count(100, 1), 100);
        assert_eq!(case_count(600, 1), 600);
        assert_eq!(case_count(600, 50), 30_000);
        assert_eq!(case_count(10, 3), 300);
        assert_eq!(case_count(u32::MAX, 2), u32::MAX);
        // A factor below one does not lower the number.
        assert_eq!(case_count(600, 0), 600);
    }

    #[test]
    fn regression_seeds_go_to_the_committed_directory() {
        let config = cases("prop_example", 10);
        assert!(config.cases >= MIN_CASES);
        assert!(config.failure_persistence.is_some());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/proptest-regressions");
        assert!(dir.is_dir(), "{}", dir.display());
    }
}
