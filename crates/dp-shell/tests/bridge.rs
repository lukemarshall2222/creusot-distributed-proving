//! Tests of the bridge client against a real patched why3find.
//!
//! They need the patched why3find and at least one prover of the test
//! project on PATH. Set `DP_WHY3FIND` to the patched executable, as the
//! development shell does; without it the tests pass with a notice.

use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use dp_formats::dispatch::{FromWhy3find, ProverResult, StrategyStep, ToWhy3find};
use dp_shell::why3find::{Bridge, BridgeError, Poll};

const LIMIT: usize = 1 << 26; // a test value, not a proposal for <TBD-22>

const ARITH: &str = r#"module Arith
  use int.Int

  goal Easy: forall x y: int. x + y = y + x

  goal Split: (forall x: int. x * 0 = 0) /\ (forall a b: int. a < b -> a + 1 <= b)

  goal Wrong: forall x: int. x * x > x
end
"#;

fn why3find() -> Option<PathBuf> {
    let path = std::env::var_os("DP_WHY3FIND").map(PathBuf::from);
    if path.is_none() {
        eprintln!("DP_WHY3FIND is not set: bridge test skipped");
    }
    path
}

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dp-bridge-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("arith.mlw"), ARITH).unwrap();
    fs::write(
        dir.join("why3find.json"),
        r#"{ "fast": 0.2, "time": 1.0, "depth": 4, "packages": [], "provers": [ "z3", "cvc5" ], "tactics": [ "split_vc" ], "drivers": [], "warnoff": [] }"#,
    )
    .unwrap();
    dir
}

fn start(why3find: &Path, dir: &Path) -> Bridge {
    let args: Vec<OsString> = vec!["-f".into(), "arith.mlw".into()];
    Bridge::start(why3find, &args, dir, LIMIT).unwrap()
}

fn proof_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().unwrap() != ".why3find" {
                    walk(&path, out);
                }
            } else if path.extension().is_some_and(|e| e == "json")
                && path.file_name().unwrap() != "why3find.json"
            {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out);
    out
}

/// Answers every prover call with `answer` and returns what why3find sent.
fn run(bridge: &mut Bridge, answer: ProverResult) -> Vec<FromWhy3find> {
    let mut log = Vec::new();
    loop {
        match bridge.poll(Duration::from_secs(60)).unwrap() {
            Poll::Closed => return log,
            Poll::Idle => panic!("why3find is silent"),
            Poll::Message(msg) => {
                match &msg {
                    FromWhy3find::CacheGet { rid, .. } => bridge
                        .send(&ToWhy3find::Cache { rid: rid.clone(), result: ProverResult::NoResult })
                        .unwrap(),
                    FromWhy3find::Prove { rid, .. } => {
                        bridge.send(&ToWhy3find::Answer { rid: rid.clone(), result: answer }).unwrap()
                    }
                    FromWhy3find::CacheSet { rid, .. } => {
                        bridge.send(&ToWhy3find::Stored { rid: rid.clone(), store: false }).unwrap()
                    }
                    _ => {}
                }
                log.push(msg);
            }
        }
    }
}

#[test]
fn valid_answers_prove_every_goal() {
    let Some(why3find) = why3find() else { return };
    let dir = project("valid");
    let mut bridge = start(&why3find, &dir);
    let log = run(&mut bridge, ProverResult::Valid { micros: 5000 });
    assert_eq!(log.first(), Some(&FromWhy3find::Hello { version: 1 }));
    assert_eq!(log.last(), Some(&FromWhy3find::Done));
    let goals: BTreeSet<_> = log
        .iter()
        .filter_map(|m| match m {
            FromWhy3find::Goal { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(goals, BTreeSet::from(["Easy", "Split", "Wrong"]));
    assert!(bridge.wait().unwrap().success());
    assert_eq!(proof_files(&dir).len(), 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unknown_answers_prove_nothing_and_reach_every_strategy_step() {
    let Some(why3find) = why3find() else { return };
    let dir = project("unknown");
    let mut bridge = start(&why3find, &dir);
    let log = run(&mut bridge, ProverResult::Unknown { micros: 5000 });
    let steps: Vec<_> = log
        .iter()
        .filter_map(|m| match m {
            FromWhy3find::Prove { step, .. } => Some(*step),
            _ => None,
        })
        .collect();
    for step in [StrategyStep::Fast, StrategyStep::Parallel, StrategyStep::ParallelLong] {
        assert!(steps.contains(&step), "{step:?}");
    }
    // the tactic split_vc creates sub-goals, announced with their path
    assert!(log.iter().any(|m| matches!(m, FromWhy3find::Goal { path, .. } if !path.is_empty())));
    assert_eq!(bridge.wait().unwrap().code(), Some(1));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn requests_are_answered_while_prover_calls_are_open() {
    let Some(why3find) = why3find() else { return };
    let dir = project("requests");
    let mut bridge = start(&why3find, &dir);
    let (rid, gid, prover) = loop {
        match bridge.poll(Duration::from_secs(60)).unwrap() {
            Poll::Message(FromWhy3find::CacheGet { rid, .. }) => bridge
                .send(&ToWhy3find::Cache { rid, result: ProverResult::NoResult })
                .unwrap(),
            Poll::Message(FromWhy3find::Prove { rid, gid, prover, .. }) => break (rid, gid, prover),
            Poll::Message(_) => {}
            other => panic!("{other:?}"),
        }
    };
    let provers = bridge.provers().unwrap();
    assert!(provers.iter().any(|p| p.prover == prover));
    assert!(provers.iter().all(|p| p.command.contains("%f")));

    let task = bridge.task_print(&gid).unwrap();
    assert!(!task.is_empty());

    let printed = bridge.print(&gid, &prover, &[]).unwrap();
    assert_eq!(printed.tasks, 1);
    assert!(printed.bytes.windows(9).any(|w| w == b"check-sat"));
    assert_eq!(bridge.print(&gid, &prover, &[]).unwrap(), printed);

    assert!(matches!(bridge.print("no-such-goal", &prover, &[]), Err(BridgeError::Refused(_))));
    assert!(matches!(bridge.print(&gid, "no-such-prover", &[]), Err(BridgeError::Refused(_))));

    bridge.send(&ToWhy3find::Answer { rid, result: ProverResult::Unknown { micros: 1 } }).unwrap();
    let log = run(&mut bridge, ProverResult::Unknown { micros: 1 });
    assert_eq!(log.last(), Some(&FromWhy3find::Done));
    bridge.wait().unwrap();
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn closing_the_pipe_stops_why3find_without_a_proof_file() {
    let Some(why3find) = why3find() else { return };
    let dir = project("closed");
    let mut bridge = start(&why3find, &dir);
    loop {
        match bridge.poll(Duration::from_secs(60)).unwrap() {
            Poll::Message(FromWhy3find::CacheGet { .. }) => break,
            Poll::Message(_) => {}
            other => panic!("{other:?}"),
        }
    }
    bridge.close();
    assert_eq!(bridge.wait().unwrap().code(), Some(3));
    assert!(proof_files(&dir).is_empty());
    fs::remove_dir_all(dir).unwrap();
}
