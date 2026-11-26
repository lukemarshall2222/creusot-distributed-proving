// Feature: creusot-distributed-proving, Property 30: proof.json and the Creusot_CLI report labels faithfully
// P-ID: P7.2.2 (legacy reading), P7.5.10 (legacy isolation)

//! What unmodified pinned why3find makes of the files that a Session
//! leaves (R7.2 criterion 10, R7.5 criterion 7).
//!
//! - An entry with a non-proof label is not a proof for it, also when a
//!   solver reported a proof that the Front_End did not accept.
//! - It reads a file with the extension members as it reads the same file
//!   without them.
//! - Its own cache layout holds prover results only of proved-standalone
//!   Results, and of why3find's own Calibration runs.
//!
//! Needs the development shell and `target/debug/dp-frontend`.

use std::fs;

use dp_diff::{demoted_alt_ergo, leaves, legacy_cache, project, proof_file, session, tools, unmodified};

const PROOF_LABELS: [&str; 2] = ["proved-standalone", "certified"];

/// The summary of a Theory in why3find's report, such as `✘ (2/3)`.
fn theory_line(report: &str) -> String {
    report.lines().find(|l| l.starts_with("Theory arith.Arith:")).unwrap_or_else(|| panic!("{report}")).to_string()
}

#[test]
fn a_reported_proof_that_was_not_accepted_is_no_proof_for_a_legacy_reader() {
    let Some(t) = tools() else { return };
    // Alt-Ergo proves two Goals, but its binary is no Unpatched_Solver:
    // every proof that it reports is labelled unchecked.
    let dir = project("legacy-unchecked", r#""alt-ergo""#);
    let out = session(&t, &dir, &demoted_alt_ergo(&t.lock));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("0 proved-standalone Verdicts"), "{stderr}");
    let written = proof_file(&dir);
    let leaves = leaves(&written);
    assert!(leaves.values().any(|(label, _)| label == "unchecked"), "{leaves:?}");
    for (path, (label, legacy)) in &leaves {
        assert!(!PROOF_LABELS.contains(&label.as_str()), "{path}: {label}");
        assert!(!legacy, "{path}: a legacy proof with the label {label}");
    }
    // Nothing of these runs is in the layout that a legacy reader trusts.
    // why3find keeps the runs of its own Calibration of a prover there
    // (they never leave why3find), so the layout is compared with that of
    // a Session in which Alt-Ergo reports no proof at all: the same
    // entries, and so none for a Goal of this project.
    let calibration_only = project("legacy-calibration", r#""alt-ergo""#);
    let only_wrong = dp_diff::ARITH.lines().filter(|l| !l.contains("goal Easy") && !l.contains("goal Split")).collect::<Vec<_>>().join("\n");
    fs::write(calibration_only.join("arith.mlw"), only_wrong).unwrap();
    let out = session(&t, &calibration_only, &demoted_alt_ergo(&t.lock));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Goals by label: 1 ") && !stderr.contains("reports a proof"), "{stderr}");
    assert_eq!(legacy_cache(&dir), legacy_cache(&calibration_only));
    fs::remove_dir_all(calibration_only).unwrap();
    // Replay by unmodified why3find: nothing is proved, the file stays.
    let report = unmodified(&t, &dir, &["prove", "-r", "arith.mlw"]);
    // (Without a count: why3find gives one only when some Goal is proved.)
    assert_eq!(theory_line(&report), "Theory arith.Arith: ✘", "{report}");
    for goal in ["Easy", "Split", "Wrong"] {
        assert!(report.contains(&format!("Goal Arith.{goal}: ✘")), "{report}");
    }
    assert_eq!(proof_file(&dir), written);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_extension_members_change_nothing_for_a_legacy_reader() {
    let Some(t) = tools() else { return };
    let dir = project("legacy-members", r#""alt-ergo""#);
    session(&t, &dir, &t.lock);
    let written = proof_file(&dir);
    let leaves = leaves(&written);
    // A legacy proof exactly where the label is a proof label.
    for (path, (label, legacy)) in &leaves {
        assert_eq!(*legacy, PROOF_LABELS.contains(&label.as_str()), "{path}: {label}");
    }
    assert!(leaves.values().any(|(_, legacy)| *legacy) && leaves.values().any(|(_, legacy)| !*legacy), "{leaves:?}");
    let with_members = unmodified(&t, &dir, &["prove", "-r", "arith.mlw"]);
    assert_eq!(proof_file(&dir), written);
    // The same file without `dp` and `dp-lock`: an unproved Goal is `null`,
    // as why3find writes it.
    fn strip(node: &mut serde_json::Value) {
        if let Some(members) = node.as_object_mut() {
            members.remove("dp");
            members.remove("dp-lock");
            let empty = members.is_empty();
            for child in members.values_mut() {
                strip(child);
            }
            if empty {
                *node = serde_json::Value::Null;
            }
        } else if let Some(children) = node.as_array_mut() {
            children.iter_mut().for_each(strip);
        }
    }
    let mut plain: serde_json::Value = serde_json::from_str(&written).unwrap();
    strip(&mut plain);
    assert!(!plain.to_string().contains("\"dp"));
    fs::write(dir.join("why3find.proofs/arith.json"), serde_json::to_string_pretty(&plain).unwrap()).unwrap();
    let without_members = unmodified(&t, &dir, &["prove", "-r", "arith.mlw"]);
    assert_eq!(theory_line(&with_members), theory_line(&without_members));
    assert_eq!(theory_line(&with_members), "Theory arith.Arith: ✘ (2/3)");
    fs::remove_dir_all(dir).unwrap();
}
