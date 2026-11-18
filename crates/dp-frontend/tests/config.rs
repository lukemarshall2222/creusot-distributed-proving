// Feature: creusot-distributed-proving, Property 5: An invalid configuration starts nothing
// P-ID: P1.1.6, P1.6.7
//
// `dp-frontend prove` with configurations that `validate_config` rejects:
// the option, its source and its value are named, the exit status is 2,
// and nothing is started or written. These tests need no prover: a valid
// configuration is recognised by the Session getting as far as the check
// of why3find against the Lock_File, which fails here.

use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

fn project(name: &str, dp_toml: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dp-config-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("lock.toml"), "").unwrap();
    if let Some(text) = dp_toml {
        fs::write(dir.join("dp.toml"), text).unwrap();
    }
    dir
}

fn prove(dir: &PathBuf, env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dp-frontend"));
    command.args(["prove", "x.mlw"]).current_dir(dir).env("DP_WHY3FIND", "/bin/sh").env("DP_LOCK", dir.join("lock.toml"));
    for name in [
        "DP_CLUSTER", "DP_REGISTRY", "DP_OPERATOR_KEY", "DP_PORTFOLIO_SEEDS", "DP_JOBS", "DP_SILENCE_SECONDS",
        "DP_BEAT_SECONDS", "DP_JOB_MARGIN_SECONDS", "DP_CERTIFY_ALL", "DP_TRACE", "DP_TRACE_RETENTION", "DP_TRACE_KEEP",
    ] {
        command.env_remove(name);
    }
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().unwrap()
}

/// The configuration was rejected: status 2, the text names the option,
/// and the project directory holds nothing new.
fn rejected(dir: &PathBuf, out: &Output, expected: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("dp-frontend: configuration:"), "{stderr}");
    assert!(stderr.contains(expected), "expected {expected:?} in {stderr}");
    let mut files: Vec<_> =
        fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    files.sort();
    assert!(files.iter().all(|f| f == "dp.toml" || f == "lock.toml"), "{files:?}");
}

/// The configuration was accepted: the Session went on to check why3find.
fn accepted(out: &Output) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("pinned dependency check failed: why3find"), "{stderr}");
}

#[test]
fn without_options_the_defaults_are_valid() {
    let dir = project("defaults", None);
    accepted(&prove(&dir, &[]));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_values_in_the_file_are_named_and_start_nothing() {
    let cases = [
        ("[local]\njobs = 0\n", "local.jobs (dp.toml) = 0"),
        ("[cluster]\nportfolio_seeds = 0\n", "cluster.portfolio_seeds (dp.toml) = 0"),
        ("[cluster]\nportfolio_seeds = 65\n", "cluster.portfolio_seeds (dp.toml) = 65: between 1 and 64"),
        ("[cluster]\nportfolio_seeds = 3\n", "a race of several seeds needs the cluster option"),
        ("[cluster]\naddress = \"nowhere\"\nregistry = \"r\"\noperator_key = \"k\"\n", "cluster.address (dp.toml) = \"nowhere\": not HOST:PORT"),
        ("[cluster]\naddress = \"host:0\"\nregistry = \"r\"\noperator_key = \"k\"\n", "not HOST:PORT"),
        ("[cluster]\naddress = \"host:1\"\n", "needs cluster.registry"),
        (
            "[cluster]\naddress = \"host:1\"\nregistry = \"r\"\noperator_key = \"k\"\nsilence_seconds = 3\nbeat_seconds = 2\n",
            "cluster.beat_seconds (dp.toml) = 2 with cluster.silence_seconds (dp.toml) = 3",
        ),
        ("[certification]\ncertify_all = true\n", "certification.certify_all (dp.toml) = true"),
        ("[tracing]\nfile = \"t\"\nkeep = 0\n", "tracing.keep (dp.toml) = 0"),
        // not the format at all
        ("[cluster]\nadress = \"host:1\"\n", "adress"),
        ("[local]\njobs = \"many\"\n", "dp.toml"),
        ("[local\n", "dp.toml"),
    ];
    for (i, (text, expected)) in cases.iter().enumerate() {
        let dir = project(&format!("file{i}"), Some(text));
        rejected(&dir, &prove(&dir, &[]), expected);
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn invalid_values_in_the_environment_are_named_and_start_nothing() {
    let cases: [(&[(&str, &str)], &str); 6] = [
        (&[("DP_JOBS", "0")], "local.jobs (DP_JOBS) = 0"),
        (&[("DP_PORTFOLIO_SEEDS", "many")], "cluster.portfolio_seeds (DP_PORTFOLIO_SEEDS) = \"many\""),
        (&[("DP_CLUSTER", "tcp://host")], "cluster.address (DP_CLUSTER) = \"tcp://host\""),
        (&[("DP_BEAT_SECONDS", "-1")], "cluster.beat_seconds (DP_BEAT_SECONDS) = \"-1\""),
        (&[("DP_TRACE_RETENTION", "all")], "tracing.retention (DP_TRACE_RETENTION) = \"all\""),
        (&[("DP_CERTIFY_ALL", "true")], "certification.certify_all (DP_CERTIFY_ALL) = true"),
    ];
    for (i, (env, expected)) in cases.iter().enumerate() {
        let dir = project(&format!("env{i}"), None);
        rejected(&dir, &prove(&dir, env), expected);
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn the_environment_goes_before_the_file() {
    // The file's value is invalid, the environment's is valid.
    let dir = project("precedence", Some("[local]\njobs = 0\n[cluster]\nportfolio_seeds = 0\n"));
    accepted(&prove(&dir, &[("DP_JOBS", "2"), ("DP_PORTFOLIO_SEEDS", "1")]));
    // And the other way round.
    fs::write(dir.join("dp.toml"), "[local]\njobs = 2\n").unwrap();
    rejected(&dir, &prove(&dir, &[("DP_JOBS", "0")]), "local.jobs (DP_JOBS) = 0");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_cluster_needs_a_registry_that_the_operator_signed() {
    // A well-formed cluster option whose registry file does not exist: the
    // error names the option, and nothing is started.
    let key = "00".repeat(32);
    let text = format!("[cluster]\naddress = \"127.0.0.1:1\"\nregistry = \"missing\"\noperator_key = \"{key}\"\n");
    let dir = project("registry", Some(&text));
    rejected(&dir, &prove(&dir, &[]), "cluster.registry (dp.toml) = \"missing\"");
    let text = "[cluster]\naddress = \"127.0.0.1:1\"\nregistry = \"lock.toml\"\noperator_key = \"zz\"\n";
    fs::write(dir.join("dp.toml"), text).unwrap();
    rejected(&dir, &prove(&dir, &[]), "cluster.operator_key (dp.toml) = \"zz\": not a verification key");
    fs::remove_dir_all(dir).unwrap();
}
