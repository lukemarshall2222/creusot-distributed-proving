// Feature: creusot-distributed-proving, Property 5: Invalid configurations and pin mismatches fail closed
// P-ID: P1.1.6, P1.6.7

//! `validate_config` over generated configurations: what it accepts is what
//! the requirements allow, an error names an option that is wrong, and the
//! heartbeat period of an accepted configuration fits twice into the
//! silence limit, so that one lost heartbeat is not a lost Cluster.

use dp_core::config::{validate_config, ConfigError, Settings, MAX_PROOF_MEGABYTES, MAX_SEEDS};
use dp_testkit::gen::config_text::{broken_settings, settings, valid_settings};
use proptest::prelude::*;

/// Whether the option that an error names is wrong in the configuration,
/// restated from the requirements and not from `validate_config`.
fn wrong(s: &Settings, error: ConfigError) -> bool {
    match error {
        ConfigError::ClusterAddress => s.cluster && !s.address_ok,
        ConfigError::ClusterRegistry => s.cluster && !s.registry_given,
        ConfigError::LocalJobs => s.local_jobs == 0,
        ConfigError::Seeds => s.seeds == 0 || s.seeds > MAX_SEEDS,
        ConfigError::SeedsWithoutCluster => s.seeds > 1 && !s.cluster,
        ConfigError::PortfolioNames => s.portfolio > 0 && !s.portfolio_names_ok,
        ConfigError::PortfolioUse => s.portfolio > 0 && (s.portfolio > MAX_SEEDS || !s.cluster || s.seeds != 1),
        ConfigError::FirstPassWithoutCluster => s.fast_remote && !s.cluster,
        ConfigError::HeartbeatMargin => {
            s.cluster && s.silence_limit > 0 && (s.beat_period == 0 || 2 * u128::from(s.beat_period) > u128::from(s.silence_limit))
        }
        ConfigError::Certification => s.certify && !s.checker_given,
        ConfigError::CertifyAll => s.certify_all && !s.certify,
        ConfigError::ProofSize => s.certify && (s.proof_megabytes == 0 || s.proof_megabytes > MAX_PROOF_MEGABYTES),
        ConfigError::CheckLimit => s.certify && s.check_limit_given && s.check_limit == 0,
        ConfigError::TraceRetention => s.trace_bounded && s.trace_keep == 0,
    }
}

const ERRORS: [ConfigError; 14] = [
    ConfigError::ClusterAddress,
    ConfigError::ClusterRegistry,
    ConfigError::LocalJobs,
    ConfigError::Seeds,
    ConfigError::SeedsWithoutCluster,
    ConfigError::PortfolioNames,
    ConfigError::PortfolioUse,
    ConfigError::FirstPassWithoutCluster,
    ConfigError::HeartbeatMargin,
    ConfigError::Certification,
    ConfigError::CertifyAll,
    ConfigError::ProofSize,
    ConfigError::CheckLimit,
    ConfigError::TraceRetention,
];

proptest! {
    #![proptest_config(dp_testkit::config::cases("heartbeat_margin", 2000))]

    #[test]
    fn a_valid_configuration_is_accepted(s in valid_settings()) {
        prop_assert_eq!(validate_config(&s), None);
        prop_assert!(ERRORS.iter().all(|e| !wrong(&s, *e)));
    }

    // Accepted exactly when no option is wrong; and the option that an
    // error names is one that is wrong.
    #[test]
    fn a_configuration_is_refused_exactly_when_an_option_is_wrong(s in settings()) {
        match validate_config(&s) {
            None => prop_assert!(ERRORS.iter().all(|e| !wrong(&s, *e)), "accepted: {:?}", s),
            Some(error) => prop_assert!(wrong(&s, error), "{:?} for {:?}", error, s),
        }
    }

    // P1.6.7: with a Cluster and a silence limit, two heartbeats fit into
    // the limit, for every pair of the two values, the largest included.
    #[test]
    fn two_heartbeats_fit_into_the_silence_limit(
        s in valid_settings(),
        silence in prop_oneof![1u64..1_000, any::<u64>()],
        beat in prop_oneof![0u64..1_000, any::<u64>()],
        cluster in any::<bool>(),
    ) {
        let s = Settings { cluster, seeds: 1, portfolio: 0, fast_remote: false, silence_limit: silence, beat_period: beat, ..s };
        let fits = beat >= 1 && 2 * u128::from(beat) <= u128::from(silence);
        let result = validate_config(&s);
        if cluster {
            prop_assert_eq!(result, if fits { None } else { Some(ConfigError::HeartbeatMargin) });
        } else {
            // Without a Cluster nothing is watched: the two values are free.
            prop_assert_eq!(result, None);
        }
    }

    // The generator breaks things: most of its changed configurations are
    // refused, and every error is one of its kinds.
    #[test]
    fn a_changed_option_is_noticed_unless_it_does_not_apply(s in broken_settings()) {
        let refused = validate_config(&s).is_some();
        prop_assert_eq!(refused, ERRORS.iter().any(|e| wrong(&s, *e)));
    }
}

/// Every error is reached by the generator, so that no branch of
/// `validate_config` goes untested.
#[test]
fn the_generator_reaches_every_error() {
    use proptest::{strategy::ValueTree, test_runner::TestRunner};
    let mut runner = TestRunner::deterministic();
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..5_000 {
        let s = broken_settings().new_tree(&mut runner).unwrap().current();
        if let Some(error) = validate_config(&s) {
            seen.insert(format!("{error:?}"));
        }
    }
    assert_eq!(seen.len(), ERRORS.len(), "{seen:?}");
}
