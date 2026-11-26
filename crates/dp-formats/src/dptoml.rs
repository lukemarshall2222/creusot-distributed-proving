//! The project configuration file `dp.toml` (R2.1, R1.1; design decision
//! D-13). Every option is optional; a key that the format does not have is
//! an error that names it, so a misspelt option is never silently ignored.

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    #[serde(default)]
    pub cluster: ClusterOptions,
    #[serde(default)]
    pub local: LocalOptions,
    #[serde(default)]
    pub tracing: TracingOptions,
    #[serde(default)]
    pub certification: CertificationOptions,
    /// The Solver_Configurations that a Portfolio can name (R2.1).
    #[serde(default)]
    pub solver: Vec<SolverOption>,
    #[serde(default)]
    pub portfolio: PortfolioOptions,
}

/// A named Solver_Configuration: a prover of the project and a seed. The
/// command line is the one of the prover configuration of whoever runs it;
/// option lists are not part of the format yet (OQ-N37).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SolverOption {
    pub name: String,
    /// The prover as `why3find.json` names it.
    pub prover: String,
    pub seed: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FirstPass {
    Local,
    Cluster,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PortfolioOptions {
    /// Names of the Solver_Configurations that race on every prover call
    /// that goes to the Cluster, in order.
    pub race: Option<Vec<String>>,
    /// Where the first sequential pass runs: local, the default, or as a
    /// race on the Cluster.
    pub first_pass: Option<FirstPass>,
    /// The call's own prover also runs on this machine as a member of
    /// every race, when a local solver slot is free.
    pub local_member: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClusterOptions {
    /// `HOST:PORT`, with or without `tcp://`.
    pub address: Option<String>,
    /// Path of the operator-signed Worker registry.
    pub registry: Option<String>,
    /// The operator's verification key, 64 hexadecimal digits.
    pub operator_key: Option<String>,
    /// Number of Jobs that race on one prover call.
    pub portfolio_seeds: Option<u64>,
    pub job_margin_seconds: Option<f64>,
    pub silence_seconds: Option<f64>,
    pub beat_seconds: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LocalOptions {
    /// Number of solver processes that run at once on this machine.
    pub jobs: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RetentionOption {
    Bounded,
    Complete,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TracingOptions {
    /// The trace file; without it no trace is written.
    pub file: Option<String>,
    /// The `tracing_retention` option: bounded, the default, or complete.
    pub retention: Option<RetentionOption>,
    /// Number of records that the bounded setting keeps.
    pub keep: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CertificationOptions {
    /// A reported proof that is not proved-standalone gets a certificate
    /// and a run of the pinned checker (R5.1).
    pub enabled: Option<bool>,
    /// Every reported proof does (R5.5).
    pub certify_all: Option<bool>,
    /// The time limit of one run of the checker, in seconds. Without it
    /// the limit follows from the time limit of the prover call.
    pub check_seconds: Option<f64>,
    /// The largest certificate that the Session takes, in megabytes.
    pub proof_megabytes: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigFileError(pub String);

impl std::fmt::Display for ConfigFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "dp.toml: {}", self.0)
    }
}

/// The message of a TOML error with the line that it is about: a value of
/// the wrong type is reported by the parser without the name of its
/// option, and an error has to name the option (R1.1 criterion 6; found by
/// `prop_config.rs` on 2026-11-26).
fn located(text: &str, error: &toml::de::Error) -> String {
    let message = error.message();
    let Some(span) = error.span() else { return message.to_string() };
    let start = span.start.min(text.len());
    let number = text.as_bytes()[..start].iter().filter(|b| **b == b'\n').count() + 1;
    match text.lines().nth(number - 1).map(str::trim) {
        Some(line) if !line.is_empty() => format!("line {number}, `{line}`: {message}"),
        _ => format!("line {number}: {message}"),
    }
}

impl ProjectConfig {
    pub fn parse(text: &str) -> Result<ProjectConfig, ConfigFileError> {
        let config: ProjectConfig = toml::from_str(text).map_err(|e| ConfigFileError(located(text, &e)))?;
        for (name, value) in [
            ("cluster.job_margin_seconds", config.cluster.job_margin_seconds),
            ("cluster.silence_seconds", config.cluster.silence_seconds),
            ("cluster.beat_seconds", config.cluster.beat_seconds),
        ] {
            if value.is_some_and(|v| !v.is_finite() || !(0.0..=86_400.0).contains(&v)) {
                return Err(ConfigFileError(format!("{name} = {}: not a time between 0 and a day", value.unwrap())));
            }
        }
        Ok(config)
    }
}

/// Reads a cluster address: `HOST:PORT` with a non-empty host and a port
/// from 1 to 65535, with or without the prefix `tcp://`.
pub fn cluster_address(text: &str) -> Option<String> {
    let address = text.strip_prefix("tcp://").unwrap_or(text);
    let (host, port) = address.rsplit_once(':')?;
    let well_formed = !host.is_empty()
        && !host.contains(|c: char| c.is_whitespace() || c == '/')
        && port.parse::<u16>().is_ok_and(|p| p != 0)
        && port.bytes().all(|b| b.is_ascii_digit());
    well_formed.then(|| address.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_option_is_optional_and_unknown_keys_are_named() {
        assert_eq!(ProjectConfig::parse("").unwrap(), ProjectConfig::default());
        let config = ProjectConfig::parse(
            "[cluster]\naddress = \"tcp://h:1\"\nportfolio_seeds = 3\nbeat_seconds = 0.5\n\
             [local]\njobs = 4\n[tracing]\nfile = \"t\"\nretention = \"complete\"\n[certification]\ncertify_all = false\n",
        )
        .unwrap();
        assert_eq!(config.cluster.address.as_deref(), Some("tcp://h:1"));
        assert_eq!(config.cluster.portfolio_seeds, Some(3));
        assert_eq!(config.local.jobs, Some(4));
        assert_eq!(config.tracing.retention, Some(RetentionOption::Complete));
        let e = ProjectConfig::parse("[cluster]\nadress = \"h:1\"\n").unwrap_err();
        assert!(e.0.contains("adress"), "{e}");
        let e = ProjectConfig::parse("[clustre]\n").unwrap_err();
        assert!(e.0.contains("clustre"), "{e}");
        assert!(ProjectConfig::parse("[local]\njobs = -1\n").is_err());
        assert!(ProjectConfig::parse("[local]\njobs = \"4\"\n").is_err());
        let e = ProjectConfig::parse("[cluster]\nbeat_seconds = -2\n").unwrap_err();
        assert!(e.0.contains("cluster.beat_seconds"), "{e}");
        assert!(ProjectConfig::parse("[tracing]\nretention = \"some\"\n").is_err());
    }

    #[test]
    fn cluster_addresses() {
        assert_eq!(cluster_address("tcp://host:80").as_deref(), Some("host:80"));
        assert_eq!(cluster_address("10.0.0.1:65535").as_deref(), Some("10.0.0.1:65535"));
        for bad in ["host", "host:", ":80", "host:0", "host:65536", "host:+80", "ho st:80", "http://host:80", "host:8 0"] {
            assert_eq!(cluster_address(bad), None, "{bad}");
        }
    }
}
