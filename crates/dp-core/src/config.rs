//! Validation of the project configuration (R1.1 criterion 6, R1.6
//! criterion 7; design: `validate_config`).
//!
//! The shell reads `dp.toml` and the environment, computes the facts that
//! need parsing (whether the cluster address is well formed), and passes
//! the values here. A Session starts no solver unless this function accepts
//! its configuration.

#[allow(unused_imports)] // `Int` is used by logic functions only
use creusot_std::logic::Int;
use creusot_std::prelude::{Clone, DeepModel, PartialEq, *};

/// The largest number of Jobs that race on one prover call. A working
/// bound, not a proposal for an open parameter.
pub const MAX_SEEDS: u64 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct Settings {
    /// The cluster option is set.
    pub cluster: bool,
    /// Its value is a well-formed address (the shell's address parser).
    pub address_ok: bool,
    /// The registry and the operator's verification key are given.
    pub registry_given: bool,
    pub local_jobs: u64,
    pub seeds: u64,
    /// Number of members of the Portfolio; zero: no Portfolio.
    pub portfolio: u64,
    /// Every member names a Solver_Configuration of the file, and no name
    /// is there twice (R2.1 criteria 3 to 5).
    pub portfolio_names_ok: bool,
    /// The first pass goes to the Cluster.
    pub fast_remote: bool,
    /// Zero: the Coordinator's silence is not watched.
    pub silence_limit: u64,
    pub beat_period: u64,
    /// Certification is on.
    pub certify: bool,
    /// The checker and its signature files are named.
    pub checker_given: bool,
    pub certify_all: bool,
    /// The bounded tracing retention setting is chosen, with this many
    /// records.
    pub trace_bounded: bool,
    pub trace_keep: u64,
}

/// What is wrong with a configuration; each names one option.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub enum ConfigError {
    /// The cluster address is not `HOST:PORT`.
    ClusterAddress,
    /// The cluster option needs the registry and the operator key.
    ClusterRegistry,
    /// At least one local solver process.
    LocalJobs,
    /// Between 1 and `MAX_SEEDS` Jobs per prover call.
    Seeds,
    /// A race of several seeds needs the Cluster.
    SeedsWithoutCluster,
    /// A Portfolio member is unknown or named twice.
    PortfolioNames,
    /// A Portfolio is a race, and a race needs the Cluster; and it is not
    /// combined with the seeds option.
    PortfolioUse,
    /// The first pass can go to the Cluster only if there is one.
    FirstPassWithoutCluster,
    /// The heartbeat period is positive, and two heartbeats fit into the
    /// silence limit, so one lost heartbeat is not a lost Cluster.
    HeartbeatMargin,
    /// Certification needs the checker and its signature files.
    Certification,
    /// Certifying every proof needs certification to be on.
    CertifyAll,
    /// The bounded retention setting keeps at least one record.
    TraceRetention,
}

#[logic(open)]
pub fn valid(s: Settings) -> bool {
    pearlite! {
        (s.cluster ==> s.address_ok && s.registry_given)
        && s.local_jobs@ >= 1
        && 1 <= s.seeds@ && s.seeds@ <= MAX_SEEDS@
        && (s.seeds@ > 1 ==> s.cluster)
        && (s.portfolio@ > 0 ==> s.portfolio_names_ok && s.portfolio@ <= MAX_SEEDS@
              && s.cluster && s.seeds@ == 1)
        && (s.fast_remote ==> s.cluster)
        && (s.cluster && s.silence_limit@ > 0 ==>
              s.beat_period@ >= 1 && 2 * s.beat_period@ <= s.silence_limit@)
        && (s.certify ==> s.checker_given)
        && (s.certify_all ==> s.certify)
        && (s.trace_bounded ==> s.trace_keep@ >= 1)
    }
}

/// Accepts a configuration or names the first option that is wrong.
#[ensures((result == None) == valid(*s))]
pub fn validate_config(s: &Settings) -> Option<ConfigError> {
    if s.cluster && !s.address_ok {
        return Some(ConfigError::ClusterAddress);
    }
    if s.cluster && !s.registry_given {
        return Some(ConfigError::ClusterRegistry);
    }
    if s.local_jobs < 1 {
        return Some(ConfigError::LocalJobs);
    }
    if s.seeds < 1 || s.seeds > MAX_SEEDS {
        return Some(ConfigError::Seeds);
    }
    if s.seeds > 1 && !s.cluster {
        return Some(ConfigError::SeedsWithoutCluster);
    }
    if s.portfolio > 0 && !s.portfolio_names_ok {
        return Some(ConfigError::PortfolioNames);
    }
    if s.portfolio > 0 && (s.portfolio > MAX_SEEDS || !s.cluster || s.seeds != 1) {
        return Some(ConfigError::PortfolioUse);
    }
    if s.fast_remote && !s.cluster {
        return Some(ConfigError::FirstPassWithoutCluster);
    }
    if s.cluster && s.silence_limit > 0 && (s.beat_period < 1 || s.beat_period > s.silence_limit / 2) {
        return Some(ConfigError::HeartbeatMargin);
    }
    if s.certify && !s.checker_given {
        return Some(ConfigError::Certification);
    }
    if s.certify_all && !s.certify {
        return Some(ConfigError::CertifyAll);
    }
    if s.trace_bounded && s.trace_keep < 1 {
        return Some(ConfigError::TraceRetention);
    }
    None
}
