//! The configuration of a Session (design decision D-13): for every option,
//! the environment variable if it is set, then `dp.toml` in the project
//! directory, then the default. The values are checked by dp-core's
//! `validate_config` before anything starts; an error names the option, its
//! source and its value.
//!
//! The defaults of the times and counts are working values, not proposals
//! for the open parameters (`<TBD-3>`, `<TBD-5>`, `<TBD-46>`).

use std::{path::Path, time::Duration};

use dp_core::config::{validate_config, ConfigError, Settings, MAX_SEEDS};
use dp_formats::{
    cluster::{from_hex, Registry},
    dptoml::{cluster_address, FirstPass, ProjectConfig, RetentionOption},
};
use dp_shell::trace::Retention;

use crate::session::{Cluster, Member};

/// The options of a Session that the configuration decides.
pub struct Options {
    pub jobs: usize,
    pub cluster: Option<Cluster>,
    pub job_margin: Duration,
    pub silence_limit: Duration,
    pub beat_period: Duration,
    pub portfolio_seeds: u64,
    pub portfolio: Vec<Member>,
    pub fast_remote: bool,
    pub local_racer: bool,
    /// The checker of certification, when certification is on.
    pub checker: Option<dp_cert::Checker>,
    pub certify_all: bool,
    pub trace: Option<(std::path::PathBuf, Retention)>,
}

/// An option's value with where it came from, for error messages.
struct Value<T> {
    value: T,
    /// `NAME (source) = text`, or the name alone for a default.
    shown: String,
}

fn env_text(name: &str) -> Result<Option<String>, String> {
    match std::env::var(name) {
        Ok(text) => Ok(Some(text)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} is not text")),
    }
}

/// The value of an option: the environment, then the file, then the default.
fn pick<T: Copy + std::fmt::Display>(
    option: &str,
    env: &str,
    parse: impl Fn(&str) -> Option<T>,
    file: Option<T>,
    default: T,
) -> Result<Value<T>, String> {
    if let Some(text) = env_text(env)? {
        let value = parse(&text).ok_or_else(|| format!("{option} ({env}) = {text:?}: not a valid value"))?;
        return Ok(Value { value, shown: format!("{option} ({env}) = {text}") });
    }
    Ok(match file {
        Some(value) => Value { value, shown: format!("{option} (dp.toml) = {value}") },
        None => Value { value: default, shown: format!("{option} (default) = {default}") },
    })
}

fn seconds(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|s| s.is_finite() && (0.0..=86_400.0).contains(s))
}

/// Loads and validates the configuration of the Session in `dir`.
pub fn load(dir: &Path) -> Result<Options, String> {
    let file = match std::fs::read_to_string(dir.join("dp.toml")) {
        Ok(text) => ProjectConfig::parse(&text).map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ProjectConfig::default(),
        Err(e) => return Err(format!("dp.toml: {e}")),
    };
    let text = |option: &str, env: &str, file: &Option<String>| -> Result<Option<Value<String>>, String> {
        Ok(match (env_text(env)?, file) {
            (Some(value), _) => Some(Value { shown: format!("{option} ({env}) = {value:?}"), value }),
            (None, Some(value)) => Some(Value { shown: format!("{option} (dp.toml) = {value:?}"), value: value.clone() }),
            (None, None) => None,
        })
    };
    let address = text("cluster.address", "DP_CLUSTER", &file.cluster.address)?;
    let registry = text("cluster.registry", "DP_REGISTRY", &file.cluster.registry)?;
    let operator_key = text("cluster.operator_key", "DP_OPERATOR_KEY", &file.cluster.operator_key)?;
    let default_jobs = std::thread::available_parallelism().map_or(1, |n| n.get() as u64 / 2).max(1);
    let jobs = pick("local.jobs", "DP_JOBS", |t| t.parse().ok(), file.local.jobs, default_jobs)?;
    let seeds =
        pick("cluster.portfolio_seeds", "DP_PORTFOLIO_SEEDS", |t| t.parse().ok(), file.cluster.portfolio_seeds, 1)?;
    let margin =
        pick("cluster.job_margin_seconds", "DP_JOB_MARGIN_SECONDS", seconds, file.cluster.job_margin_seconds, 30.0)?;
    let silence = pick("cluster.silence_seconds", "DP_SILENCE_SECONDS", seconds, file.cluster.silence_seconds, 10.0)?;
    let beat = pick("cluster.beat_seconds", "DP_BEAT_SECONDS", seconds, file.cluster.beat_seconds, 2.0)?;
    let certify_all = pick(
        "certification.certify_all",
        "DP_CERTIFY_ALL",
        |t| t.parse().ok(),
        file.certification.certify_all,
        false,
    )?;
    let certify =
        pick("certification.enabled", "DP_CERTIFY", |t| t.parse().ok(), file.certification.enabled, false)?;
    // The checker and its signature files come from the environment of the
    // pinned tools, not from the project.
    let ethos = env_text("DP_ETHOS")?.unwrap_or_else(|| "ethos".to_string());
    let signature = env_text("DP_CPC_SIGNATURE")?;
    let checker = match (dp_shell::solver::find_executable(&ethos), &signature) {
        (Ok(ethos), Some(signature)) if std::path::Path::new(signature).join("Cpc.eo").is_file() => {
            Some(dp_cert::Checker { ethos, signature: signature.into() })
        }
        _ => None,
    };
    let trace_file = text("tracing.file", "DP_TRACE", &file.tracing.file)?;
    let retention = match (env_text("DP_TRACE_RETENTION")?, file.tracing.retention) {
        (Some(t), _) if t == "complete" => RetentionOption::Complete,
        (Some(t), _) if t == "bounded" => RetentionOption::Bounded,
        (Some(t), _) => {
            return Err(format!("tracing.retention (DP_TRACE_RETENTION) = {t:?}: not bounded or complete"))
        }
        (None, Some(r)) => r,
        (None, None) => RetentionOption::Bounded,
    };
    let keep = pick("tracing.keep", "DP_TRACE_KEEP", |t| t.parse().ok(), file.tracing.keep, 1000)?;

    // The Portfolio: every name is a Solver_Configuration of the file, and
    // neither list has a name twice (R2.1 criteria 3 to 5).
    let race = file.portfolio.race.clone().unwrap_or_default();
    let mut portfolio = Vec::new();
    let mut names_ok = true;
    for (i, name) in race.iter().enumerate() {
        let defined: Vec<_> = file.solver.iter().filter(|s| &s.name == name).collect();
        match defined.as_slice() {
            [solver] if !race[..i].contains(name) => portfolio.push(Member {
                name: solver.name.clone(),
                prover: solver.prover.clone(),
                seed: solver.seed,
            }),
            _ => names_ok = false,
        }
    }
    if file.portfolio.race.as_ref().is_some_and(Vec::is_empty) {
        return Err("portfolio.race (dp.toml) = []: a Portfolio has at least one member".to_string());
    }
    let first_pass = match (env_text("DP_FIRST_PASS")?, file.portfolio.first_pass) {
        (Some(t), _) if t == "cluster" => (FirstPass::Cluster, "portfolio.first_pass (DP_FIRST_PASS) = cluster"),
        (Some(t), _) if t == "local" => (FirstPass::Local, "portfolio.first_pass (DP_FIRST_PASS) = local"),
        (Some(t), _) => return Err(format!("portfolio.first_pass (DP_FIRST_PASS) = {t:?}: not local or cluster")),
        (None, Some(FirstPass::Cluster)) => (FirstPass::Cluster, "portfolio.first_pass (dp.toml) = cluster"),
        (None, _) => (FirstPass::Local, "portfolio.first_pass = local"),
    };

    let local_member = pick(
        "portfolio.local_member",
        "DP_LOCAL_MEMBER",
        |t| t.parse().ok(),
        file.portfolio.local_member,
        false,
    )?;

    let micros = |s: f64| (s * 1e6) as u64;
    let parsed_address = address.as_ref().and_then(|a| cluster_address(&a.value));
    let settings = Settings {
        cluster: address.is_some(),
        address_ok: parsed_address.is_some(),
        registry_given: registry.is_some() && operator_key.is_some(),
        local_jobs: jobs.value,
        seeds: seeds.value,
        portfolio: race.len() as u64,
        portfolio_names_ok: names_ok,
        fast_remote: first_pass.0 == FirstPass::Cluster,
        silence_limit: micros(silence.value),
        beat_period: micros(beat.value),
        certify: certify.value,
        checker_given: checker.is_some(),
        certify_all: certify_all.value,
        trace_bounded: retention == RetentionOption::Bounded,
        trace_keep: keep.value,
    };
    if let Some(error) = validate_config(&settings) {
        let shown = |v: &Option<Value<String>>| v.as_ref().map_or(String::new(), |v| v.shown.clone());
        return Err(match error {
            ConfigError::ClusterAddress => format!("{}: not HOST:PORT", shown(&address)),
            ConfigError::ClusterRegistry => format!(
                "{}: the cluster option needs cluster.registry (DP_REGISTRY) and cluster.operator_key (DP_OPERATOR_KEY)",
                shown(&address)
            ),
            ConfigError::LocalJobs => format!("{}: at least one local solver process", jobs.shown),
            ConfigError::Seeds => format!("{}: between 1 and {MAX_SEEDS}", seeds.shown),
            ConfigError::SeedsWithoutCluster => {
                format!("{}: a race of several seeds needs the cluster option", seeds.shown)
            }
            ConfigError::PortfolioNames => format!(
                "portfolio.race (dp.toml) = {race:?}: every member is the name of one [[solver]] entry, and no name is there twice"
            ),
            ConfigError::PortfolioUse => format!(
                "portfolio.race (dp.toml) = {race:?}: a Portfolio has at most {MAX_SEEDS} members, needs the cluster option, and is not combined with {}",
                seeds.shown
            ),
            ConfigError::FirstPassWithoutCluster => {
                format!("{}: the first pass can go to the Cluster only with the cluster option", first_pass.1)
            }
            ConfigError::HeartbeatMargin => format!(
                "{} with {}: the heartbeat period must be positive and at most half of the silence limit",
                beat.shown, silence.shown
            ),
            ConfigError::Certification => format!(
                "{}: certification needs the checker Ethos (DP_ETHOS, or `ethos` on PATH) and its signature files (DP_CPC_SIGNATURE)",
                certify.shown
            ),
            ConfigError::CertifyAll => format!("{}: needs certification.enabled", certify_all.shown),
            ConfigError::TraceRetention => format!("{}: the bounded setting keeps at least one record", keep.shown),
        });
    }

    // The registry is read only for a valid configuration, and accepted
    // only with the operator's signature.
    let cluster = match (parsed_address, registry, operator_key) {
        (Some(address), Some(registry), Some(key)) => {
            let verification = from_hex::<32>(&key.value)
                .and_then(|k| ed25519_dalek::VerifyingKey::from_bytes(&k).ok())
                .ok_or_else(|| format!("{}: not a verification key (64 hex digits)", key.shown))?;
            let path = dir.join(&registry.value);
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", registry.shown))?;
            let registry =
                Registry::verify(&bytes, &verification, 1 << 20).map_err(|e| format!("{}: {e}", registry.shown))?;
            Some(Cluster { address, registry, operator: verification })
        }
        _ => None,
    };
    Ok(Options {
        jobs: jobs.value as usize,
        cluster,
        job_margin: Duration::from_secs_f64(margin.value),
        silence_limit: Duration::from_secs_f64(silence.value),
        beat_period: Duration::from_secs_f64(beat.value),
        portfolio_seeds: seeds.value,
        portfolio,
        fast_remote: first_pass.0 == FirstPass::Cluster,
        local_racer: local_member.value,
        checker: if certify.value { checker } else { None },
        certify_all: certify_all.value,
        trace: trace_file.map(|f| {
            let retention = match retention {
                RetentionOption::Complete => Retention::Complete,
                RetentionOption::Bounded => Retention::Bounded(keep.value as usize),
            };
            (dir.join(f.value), retention)
        }),
    })
}
