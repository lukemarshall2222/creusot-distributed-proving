// Feature: creusot-distributed-proving, Property 10: A certificate certifies the reference that it was checked against, and nothing else
// P-ID: P5.3.2, P5.3.4, P5.3.6, P5.3.9

//! The pinned checker on certificates and references that were changed
//! (R5.3): a proof is accepted for the problem that it proves, and for no
//! other reference, and a proof that was cut or edited is not accepted.

use std::time::Duration;

use dp_cert::{has_trusted_step, produce};
use dp_certs_tests::{tools, Tools};

/// Unsatisfiable: x is above y and y above x.
const PROBLEM: &str = "(set-logic ALL)\n(declare-const x Int)\n(declare-const y Int)\n(assert (> x y))\n(assert (> y x))\n(check-sat)\n";

const LIMIT: Duration = Duration::from_secs(60);

fn check(t: &Tools, reference: &str, proof: &[u8]) -> bool {
    t.checker.check(reference.as_bytes(), ".smt2", proof, LIMIT, &t.tmp).unwrap()
}

fn proof(t: &Tools) -> Vec<u8> {
    let proof = produce(&t.cvc5, PROBLEM.as_bytes(), ".smt2", LIMIT, &t.tmp).unwrap().expect("cvc5 proves the problem with a proof");
    assert!(!has_trusted_step(&proof), "{}", String::from_utf8_lossy(&proof));
    proof
}

#[test]
fn a_proof_is_accepted_for_its_own_reference_only() {
    let Some(t) = tools("reference") else { return };
    let proof = proof(&t);
    assert!(check(&t, PROBLEM, &proof), "the honest certificate is not accepted");
    // More assertions in the reference take nothing away.
    let more = PROBLEM.replace("(check-sat)", "(assert (> x 7))\n(check-sat)");
    assert!(check(&t, &more, &proof));
    // An assertion that the proof uses is missing, weaker, or another one.
    let missing = PROBLEM.replace("(assert (> y x))\n", "");
    let weaker = PROBLEM.replace("(assert (> y x))", "(assert (>= y x))");
    let other = PROBLEM.replace("(assert (> x y))", "(assert (> x 0))");
    // The same text about other symbols.
    let renamed = PROBLEM.replace("y", "z");
    // A satisfiable problem: no proof may be accepted for it.
    let satisfiable = "(set-logic ALL)\n(declare-const x Int)\n(declare-const y Int)\n(assert (> x y))\n(check-sat)\n";
    for (what, reference) in [("missing", &missing), ("weaker", &weaker), ("other", &other), ("renamed", &renamed)] {
        assert!(!check(&t, reference, &proof), "accepted for a reference with a {what} assertion");
    }
    assert!(!check(&t, satisfiable, &proof), "accepted for a satisfiable reference");
    std::fs::remove_dir_all(&t.tmp).unwrap();
}

#[test]
fn a_changed_proof_is_not_accepted() {
    let Some(t) = tools("proof") else { return };
    let proof = proof(&t);
    let text = String::from_utf8(proof.clone()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.len() >= 2, "{text}");
    // Nothing, and everything but the last step: no derivation of false.
    assert!(!check(&t, PROBLEM, b""));
    let cut = lines[..lines.len() - 1].join("\n") + "\n";
    assert!(!check(&t, PROBLEM, cut.as_bytes()), "accepted without its last step");
    // Only the last step: its premises are gone.
    let last = format!("{}\n", lines[lines.len() - 1]);
    assert!(!check(&t, PROBLEM, last.as_bytes()), "accepted with its last step alone");
    // Each step in turn under another rule.
    let mut renamed = 0;
    for (i, line) in lines.iter().enumerate() {
        let Some(at) = line.find(":rule ") else { continue };
        let end = at + 6 + line[at + 6..].find([' ', ')']).unwrap_or(line.len() - at - 6);
        let rule = &line[at + 6..end];
        let other = if rule == "refl" { "symm" } else { "refl" };
        let mut changed = lines.clone();
        let new_line = format!("{}{other}{}", &line[..at + 6], &line[end..]);
        changed[i] = &new_line;
        let changed = changed.join("\n") + "\n";
        assert!(!check(&t, PROBLEM, changed.as_bytes()), "accepted with step {i} under the rule {other}");
        renamed += 1;
    }
    assert!(renamed > 0, "no step with a rule in {text}");
    // An assumption that is no assertion of the reference, added in front.
    let assumed = format!("(assume @extra (> y 100))\n{text}");
    assert!(!check(&t, PROBLEM, assumed.as_bytes()), "accepted with an assumption that the reference lacks");
    // A step that the checker would have to take on trust is seen before
    // any check.
    assert!(has_trusted_step(b"(step @p1 false :rule trust)\n"));
    std::fs::remove_dir_all(&t.tmp).unwrap();
}

// P-ID: P5.2.11, P5.3.1
//
// R5.3 criterion 19. The checker takes a proof file as a script, and a
// script can declare the rule that it then applies: left to itself, the
// pinned checker says `correct` to two lines that prove false from
// nothing, for a satisfiable reference (found 2026-11-26). A certificate
// is therefore read first, and one with any command outside the supported
// fragment is not run at all.
#[test]
fn a_proof_that_declares_its_own_rule_is_not_given_to_the_checker() {
    let Some(t) = tools("fragment") else { return };
    let satisfiable = "(set-logic ALL)\n(declare-const x Int)\n(assert (> x 0))\n(check-sat)\n";
    let forged = "(declare-rule dp_bad () :conclusion false)\n(step @p1 false :rule dp_bad)\n";
    // The checker alone, as it was run before the screen existed.
    let reference = t.tmp.join("alone.smt2");
    let proof_file = t.tmp.join("alone.cpc");
    std::fs::write(&reference, satisfiable).unwrap();
    std::fs::write(&proof_file, forged).unwrap();
    let alone = std::process::Command::new(&t.checker.ethos)
        .arg(format!("--include={}", t.checker.signature.join("Cpc.eo").display()))
        .arg(format!("--reference={}", reference.display()))
        .arg("--require-proof-of-false")
        .arg(&proof_file)
        .output()
        .unwrap();
    assert!(
        dp_cert::accepted(alone.status.code().map(|c| c as u32), &alone.stdout),
        "the pinned checker no longer accepts a proof with its own rule; the screen is still required by R5.3"
    );
    // With the screen, it is not accepted, for any reference.
    let rules = t.checker.rules().unwrap();
    assert!(rules.len() > 100, "{} rules in the signature", rules.len());
    assert!(!dp_cert::fragment::in_fragment(forged.as_bytes(), &rules));
    assert!(!check(&t, satisfiable, forged.as_bytes()));
    assert!(!check(&t, PROBLEM, forged.as_bytes()));
    // The honest proof is in the fragment: every rule that cvc5 used is a
    // rule of the pinned signature.
    let honest = proof(&t);
    assert!(dp_cert::fragment::in_fragment(&honest, &rules), "{}", String::from_utf8_lossy(&honest));
    assert!(check(&t, PROBLEM, &honest));
    // The forged lines hidden in an honest proof: behind its first
    // command on the same line, in front, and at its end.
    let text = String::from_utf8(honest).unwrap();
    let declare = "(declare-rule dp_bad () :conclusion false)";
    for hidden in [
        text.replacen('\n', &format!(" {declare}\n"), 1),
        format!("{declare}\n{text}"),
        format!("{text}{declare}\n(step @p999 false :rule dp_bad)\n"),
        format!("{text}(program dp_p ((x Bool)) (Bool) Bool (((dp_p x) x)))\n"),
        format!("(define @dp_t () false)\n{text}"),
    ] {
        assert!(!check(&t, PROBLEM, hidden.as_bytes()), "accepted: {hidden}");
    }
    std::fs::remove_dir_all(&t.tmp).unwrap();
}
