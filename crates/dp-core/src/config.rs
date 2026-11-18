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
    /// Zero: the Coordinator's silence is not watched.
    pub silence_limit: u64,
    pub beat_period: u64,
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
    /// The heartbeat period is positive, and two heartbeats fit into the
    /// silence limit, so one lost heartbeat is not a lost Cluster.
    HeartbeatMargin,
    /// Certification is not built: the option cannot be on.
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
        && (s.cluster && s.silence_limit@ > 0 ==>
              s.beat_period@ >= 1 && 2 * s.beat_period@ <= s.silence_limit@)
        && !s.certify_all
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
    if s.cluster && s.silence_limit > 0 && (s.beat_period < 1 || s.beat_period > s.silence_limit / 2) {
        return Some(ConfigError::HeartbeatMargin);
    }
    if s.certify_all {
        return Some(ConfigError::CertifyAll);
    }
    if s.trace_bounded && s.trace_keep < 1 {
        return Some(ConfigError::TraceRetention);
    }
    None
}
