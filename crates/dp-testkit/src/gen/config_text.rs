//! `arb_config_text`: project configurations, valid and invalid (Property
//! 5): the values that `validate_config` of dp-core decides on, and texts
//! of `dp.toml`.

use dp_core::config::{Settings, MAX_PROOF_MEGABYTES, MAX_SEEDS};
use dp_formats::dptoml::{
    CertificationOptions, ClusterOptions, FirstPass, LocalOptions, PortfolioOptions, ProjectConfig, RetentionOption,
    SolverOption, TracingOptions,
};
use proptest::prelude::*;

/// A configuration that is valid, whatever else it sets: a Cluster with a
/// race of several seeds or a Portfolio, or none; certification on or off.
pub fn valid_settings() -> impl Strategy<Value = Settings> {
    (
        any::<bool>(),
        1u64..9,
        prop_oneof![3 => Just(1u64), 1 => 2u64..=MAX_SEEDS],
        prop_oneof![3 => Just(0u64), 1 => 1u64..=MAX_SEEDS],
        any::<bool>(),
        prop_oneof![1 => Just((0u64, 0u64)), 4 => (1u64..1_000_000).prop_flat_map(|beat| (2 * beat..4_000_000, Just(beat)))],
        (any::<bool>(), any::<bool>(), 1u64..=MAX_PROOF_MEGABYTES, any::<bool>(), 1u64..1_000_000),
        (any::<bool>(), 1u64..100_000),
    )
        .prop_map(
            |(cluster, local_jobs, seeds, portfolio, fast_remote, (silence_limit, beat_period), certification, trace)| {
                let (certify, certify_all, proof_megabytes, check_limit_given, check_limit) = certification;
                // A Portfolio is not combined with the seeds option.
                let portfolio = if cluster && seeds == 1 { portfolio } else { 0 };
                Settings {
                    cluster,
                    address_ok: true,
                    registry_given: true,
                    local_jobs,
                    seeds: if cluster { seeds } else { 1 },
                    portfolio,
                    portfolio_names_ok: true,
                    fast_remote: fast_remote && cluster,
                    silence_limit,
                    beat_period,
                    certify,
                    checker_given: true,
                    certify_all: certify_all && certify,
                    proof_megabytes,
                    check_limit_given,
                    check_limit,
                    trace_bounded: trace.0,
                    trace_keep: trace.1,
                }
            },
        )
}

/// The ways in which one option of a valid configuration is made wrong.
pub const BREAKS: usize = 16;

/// A valid configuration with one of its options changed; the change may
/// leave it valid (a broken address is no fault without the cluster
/// option). `validate_config` is to agree with the requirements on both.
pub fn broken_settings() -> impl Strategy<Value = Settings> {
    (valid_settings(), 0..BREAKS, any::<u64>()).prop_map(|(mut s, kind, n)| {
        match kind {
            0 => s.address_ok = false,
            1 => s.registry_given = false,
            2 => s.local_jobs = 0,
            3 => s.seeds = 0,
            4 => s.seeds = MAX_SEEDS + 1 + n % 100,
            5 => (s.cluster, s.seeds) = (false, 2 + n % 8),
            6 => (s.portfolio, s.portfolio_names_ok) = (1 + n % 4, false),
            7 => s.portfolio = MAX_SEEDS + 1 + n % 100,
            8 => (s.portfolio, s.seeds) = (2, 2),
            9 => (s.cluster, s.fast_remote) = (false, true),
            // One heartbeat does not fit twice into the silence limit.
            10 => (s.silence_limit, s.beat_period) = (1 + n % 1_000, (1 + n % 1_000) / 2 + 1),
            11 => (s.silence_limit, s.beat_period) = (1 + n % 1_000, 0),
            12 => s.checker_given = false,
            13 => (s.certify, s.certify_all) = (false, true),
            14 => s.proof_megabytes = if n % 2 == 0 { 0 } else { MAX_PROOF_MEGABYTES + 1 + n % 100 },
            _ => {
                if n % 2 == 0 {
                    (s.check_limit_given, s.check_limit) = (true, 0)
                } else {
                    (s.trace_bounded, s.trace_keep) = (true, 0)
                }
            }
        }
        s
    })
}

/// Valid configurations and configurations with one option changed.
pub fn settings() -> impl Strategy<Value = Settings> {
    prop_oneof![1 => valid_settings(), 3 => broken_settings()]
}

fn name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9-]{0,8}"
}

/// A time that `dp.toml` takes: between zero and a day, with at most
/// three decimals, so that its text reads back as the same number.
fn seconds() -> impl Strategy<Value = f64> {
    (0u64..86_400_000).prop_map(|millis| millis as f64 / 1000.0)
}

/// The contents of a `dp.toml` file that the format accepts. Every option
/// is there or absent.
pub fn project_config() -> impl Strategy<Value = ProjectConfig> {
    let opt_bool = || prop::option::of(any::<bool>());
    let cluster = (
        prop::option::of((name(), 1u16..=u16::MAX).prop_map(|(host, port)| format!("tcp://{host}:{port}"))),
        prop::option::of(name()),
        prop::option::of("[0-9a-f]{64}"),
        prop::option::of(0u64..100),
        prop::option::of(seconds()),
        prop::option::of(seconds()),
        prop::option::of(seconds()),
    )
        .prop_map(|(address, registry, operator_key, portfolio_seeds, job_margin_seconds, silence_seconds, beat_seconds)| {
            ClusterOptions {
                address,
                registry,
                operator_key,
                portfolio_seeds,
                job_margin_seconds,
                silence_seconds,
                beat_seconds,
            }
        });
    let tracing = (
        prop::option::of(name()),
        prop::option::of(prop_oneof![Just(RetentionOption::Bounded), Just(RetentionOption::Complete)]),
        prop::option::of(0u64..100_000),
    )
        .prop_map(|(file, retention, keep)| TracingOptions { file, retention, keep });
    let certification = (opt_bool(), opt_bool(), prop::option::of(seconds()), prop::option::of(0u64..10_000)).prop_map(
        |(enabled, certify_all, check_seconds, proof_megabytes)| CertificationOptions {
            enabled,
            certify_all,
            check_seconds,
            proof_megabytes,
        },
    );
    let solver = proptest::collection::vec(
        (name(), name(), prop::option::of(0u64..1000)).prop_map(|(name, prover, seed)| SolverOption { name, prover, seed }),
        0..4,
    );
    let portfolio = (
        prop::option::of(proptest::collection::vec(name(), 0..4)),
        prop::option::of(prop_oneof![Just(FirstPass::Local), Just(FirstPass::Cluster)]),
        opt_bool(),
    )
        .prop_map(|(race, first_pass, local_member)| PortfolioOptions { race, first_pass, local_member });
    (cluster, prop::option::of(0u64..64), tracing, certification, solver, portfolio).prop_map(
        |(cluster, jobs, tracing, certification, solver, portfolio)| ProjectConfig {
            cluster,
            local: LocalOptions { jobs },
            tracing,
            certification,
            solver,
            portfolio,
        },
    )
}

fn quoted(text: &str) -> String {
    format!("\"{text}\"")
}

/// The text of a `dp.toml` file with these contents.
pub fn print(config: &ProjectConfig) -> String {
    fn table(out: &mut String, header: &str, lines: Vec<Option<(&str, String)>>) {
        let lines: Vec<_> = lines.into_iter().flatten().collect();
        if lines.is_empty() {
            return;
        }
        out.push_str(header);
        out.push('\n');
        for (key, value) in lines {
            out.push_str(&format!("{key} = {value}\n"));
        }
    }
    let time = |t: f64| format!("{t:.3}");
    let mut out = String::new();
    let c = &config.cluster;
    table(
        &mut out,
        "[cluster]",
        vec![
            c.address.as_deref().map(|v| ("address", quoted(v))),
            c.registry.as_deref().map(|v| ("registry", quoted(v))),
            c.operator_key.as_deref().map(|v| ("operator_key", quoted(v))),
            c.portfolio_seeds.map(|v| ("portfolio_seeds", v.to_string())),
            c.job_margin_seconds.map(|v| ("job_margin_seconds", time(v))),
            c.silence_seconds.map(|v| ("silence_seconds", time(v))),
            c.beat_seconds.map(|v| ("beat_seconds", time(v))),
        ],
    );
    table(&mut out, "[local]", vec![config.local.jobs.map(|v| ("jobs", v.to_string()))]);
    let t = &config.tracing;
    table(
        &mut out,
        "[tracing]",
        vec![
            t.file.as_deref().map(|v| ("file", quoted(v))),
            t.retention.map(|v| {
                ("retention", quoted(if v == RetentionOption::Bounded { "bounded" } else { "complete" }))
            }),
            t.keep.map(|v| ("keep", v.to_string())),
        ],
    );
    let c = &config.certification;
    table(
        &mut out,
        "[certification]",
        vec![
            c.enabled.map(|v| ("enabled", v.to_string())),
            c.certify_all.map(|v| ("certify_all", v.to_string())),
            c.check_seconds.map(|v| ("check_seconds", time(v))),
            c.proof_megabytes.map(|v| ("proof_megabytes", v.to_string())),
        ],
    );
    let p = &config.portfolio;
    table(
        &mut out,
        "[portfolio]",
        vec![
            p.race.as_ref().map(|v| ("race", format!("[{}]", v.iter().map(|n| quoted(n)).collect::<Vec<_>>().join(", ")))),
            p.first_pass.map(|v| ("first_pass", quoted(if v == FirstPass::Local { "local" } else { "cluster" }))),
            p.local_member.map(|v| ("local_member", v.to_string())),
        ],
    );
    for solver in &config.solver {
        table(
            &mut out,
            "[[solver]]",
            vec![
                Some(("name", quoted(&solver.name))),
                Some(("prover", quoted(&solver.prover))),
                solver.seed.map(|v| ("seed", v.to_string())),
            ],
        );
    }
    out
}

/// A text of `dp.toml` that the format must refuse, with a word that the
/// error message has to contain.
#[derive(Clone, Debug)]
pub struct Refused {
    pub text: String,
    pub names: String,
}

/// A valid text with one fault added: an option that the format does not
/// have, in a table of the format or as a table of its own; a value of the
/// wrong type; a time that is negative or longer than a day.
pub fn refused_text() -> impl Strategy<Value = Refused> {
    let tables = ["cluster", "local", "tracing", "certification", "portfolio"];
    let fault = prop_oneof![
        (prop::sample::select(tables.to_vec()), name()).prop_map(|(table, key)| {
            let key = format!("x-{key}");
            (format!("[{table}]\n{key} = 1\n"), Some(table), key)
        }),
        name().prop_map(|key| {
            let key = format!("x-{key}");
            (format!("[{key}]\nvalue = 1\n"), None, key)
        }),
        Just(("[local]\njobs = \"four\"\n".to_string(), Some("local"), "jobs".to_string())),
        Just(("[cluster]\nportfolio_seeds = -1\n".to_string(), Some("cluster"), "portfolio_seeds".to_string())),
        Just(("[tracing]\nretention = \"some\"\n".to_string(), Some("tracing"), "retention".to_string())),
        prop_oneof![Just("-0.5"), Just("86400.5"), Just("inf"), Just("nan")].prop_map(|time| {
            (format!("[cluster]\nbeat_seconds = {time}\n"), Some("cluster"), "beat_seconds".to_string())
        }),
    ];
    (project_config(), fault).prop_map(|(mut config, (text, table, names))| {
        // The fault brings its table; the valid part gives it up.
        match table {
            Some("cluster") => config.cluster = ClusterOptions::default(),
            Some("local") => config.local = LocalOptions::default(),
            Some("tracing") => config.tracing = TracingOptions::default(),
            Some("certification") => config.certification = CertificationOptions::default(),
            Some("portfolio") => config.portfolio = PortfolioOptions::default(),
            _ => {}
        }
        // Tables of arrays come last in a TOML text, so the fault goes first.
        Refused { text: format!("{text}{}", print(&config)), names }
    })
}
