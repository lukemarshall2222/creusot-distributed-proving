// Feature: creusot-distributed-proving, Property 33: Turning a feature on or off never flips a proof against a counter-model
// P-ID: P7.12.1 (legacy configuration: same goals proved as unmodified pinned why3find)

//! `dp-frontend prove` with every feature off, against unmodified pinned
//! why3find, on Creusot's example files: the same goals must be proved.
//!
//! Needs the development shell (`DP_WHY3FIND`, `DP_LOCK`, `DP_CREUSOT_HOME`,
//! `DP_CREUSOT_EXAMPLES`) and `target/debug/dp-frontend`
//! (`cargo build --workspace`). Without them the test passes with a notice.
//! `DP_DIFF_FILES` (comma-separated `.coma` names, or `all`) replaces the
//! default selection.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

use serde_json::Value;

const DEFAULT_FILES: &[&str] = &[
    "all_zero.coma",
    "binary_search.coma",
    "sum_first_n.coma",
    "hillel.coma",
    "linked_list.coma",
    "insertion_sort.coma",
    "knapsack.coma",
];

struct Env {
    home: PathBuf,
    examples: PathBuf,
    frontend: PathBuf,
}

fn env() -> Option<Env> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let frontend = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/dp-frontend");
    match (var("DP_CREUSOT_HOME"), var("DP_CREUSOT_EXAMPLES"), var("DP_WHY3FIND"), var("DP_LOCK")) {
        (Some(home), Some(examples), Some(_), Some(_)) if frontend.exists() => {
            Some(Env { home, examples, frontend })
        }
        _ => {
            // In CI a skipped test would be a check that checked nothing.
            assert!(
                std::env::var_os("DP_REQUIRE_TOOLS").is_none(),
                "development shell or target/debug/dp-frontend missing"
            );
            eprintln!("development shell or target/debug/dp-frontend missing: legacy_compat skipped");
            None
        }
    }
}

/// Whether a proof node is a complete proof: a prover, or a tactic whose
/// children are all complete.
fn complete(node: &Value) -> bool {
    match node {
        Value::Object(o) if o.contains_key("prover") => true,
        Value::Object(o) => o
            .get("children")
            .and_then(Value::as_array)
            .is_some_and(|children| children.iter().all(complete)),
        _ => false,
    }
}

/// Goal name to "proved", from the proof files under `dir`.
fn verdicts(dir: &Path) -> BTreeMap<String, bool> {
    let mut out = BTreeMap::new();
    let Ok(entries) = fs::read_dir(dir.join("why3find.proofs")) else { return out };
    for entry in entries.flatten() {
        let json: Value = serde_json::from_str(&fs::read_to_string(entry.path()).unwrap()).unwrap();
        for (theory, goals) in json["proofs"].as_object().into_iter().flatten() {
            for (goal, proof) in goals.as_object().into_iter().flatten() {
                out.insert(format!("{theory}.{goal}"), complete(proof));
            }
        }
    }
    out
}

/// Runs `program prove ...` on `file` as Creusot_CLI does and returns the
/// exit status, the verdicts and the seconds it took.
fn prove(e: &Env, program: &Path, file: &str, tag: &str) -> (Option<i32>, BTreeMap<String, bool>, f64) {
    let dir = std::env::temp_dir().join(format!("dp-diff-{}-{tag}-{file}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::copy(e.examples.join(file), dir.join(file)).unwrap();
    fs::copy(e.home.join("why3find.json"), dir.join("why3find.json")).unwrap();
    let path = format!("{}:{}", e.home.join("bin").display(), std::env::var("PATH").unwrap());
    let start = Instant::now();
    let out = Command::new(program)
        .args(["prove", "-f", "--summary", "--no-autodetect", "--no-autoload", file])
        .current_dir(&dir)
        .env("PATH", path)
        .env("DUNE_DIR_LOCATIONS", format!("why3find:lib:{}", e.home.join("share/why3find").display()))
        .env("WHY3CONFIG", e.home.join("creusot_why3.conf"))
        .output()
        .unwrap();
    let seconds = start.elapsed().as_secs_f64();
    let result = (out.status.code(), verdicts(&dir), seconds);
    if out.status.code().is_none_or(|c| c > 1) {
        eprintln!("{tag} {file}: {}", String::from_utf8_lossy(&out.stderr));
    }
    fs::remove_dir_all(dir).unwrap();
    result
}

#[test]
fn the_same_goals_are_proved_as_by_unmodified_why3find() {
    let Some(e) = env() else { return };
    let files: Vec<String> = match std::env::var("DP_DIFF_FILES").ok().as_deref() {
        Some("all") => {
            let mut all: Vec<String> = fs::read_dir(&e.examples)
                .unwrap()
                .flatten()
                .map(|f| f.file_name().to_string_lossy().into_owned())
                .filter(|f| f.ends_with(".coma"))
                .collect();
            all.sort();
            all
        }
        Some(list) => list.split(',').map(str::to_string).collect(),
        None => DEFAULT_FILES.iter().map(|f| f.to_string()).collect(),
    };
    let why3find = e.home.join("bin/why3find");
    let mut differences = Vec::new();
    let (mut goals, mut proved) = (0, 0);
    for file in &files {
        let (base_status, base, base_s) = prove(&e, &why3find, file, "base");
        let (status, ours, ours_s) = prove(&e, &e.frontend, file, "dp");
        goals += base.len();
        proved += base.values().filter(|p| **p).count();
        eprintln!(
            "{file}: {} goals, {} proved by why3find ({base_s:.1}s), {} by dp-frontend ({ours_s:.1}s)",
            base.len(),
            base.values().filter(|p| **p).count(),
            ours.values().filter(|p| **p).count(),
        );
        assert!(!base.is_empty(), "{file}: unmodified why3find wrote no proof file");
        if status != base_status {
            differences.push(format!("{file}: exit status {status:?}, why3find {base_status:?}"));
        }
        let names: std::collections::BTreeSet<_> = base.keys().chain(ours.keys()).collect();
        for name in names {
            if base.get(name) != ours.get(name) {
                differences.push(format!(
                    "{file}: {name}: why3find {:?}, dp-frontend {:?}",
                    base.get(name),
                    ours.get(name)
                ));
            }
        }
    }
    eprintln!("{} files, {goals} goals, {proved} proved by unmodified why3find", files.len());
    assert!(differences.is_empty(), "differences:\n{}", differences.join("\n"));
}
