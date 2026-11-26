//! What a proof producer and a checker print (R5.3): output of programs
//! that may be anything, read before any label is given.
//!
//! The first byte of the input stands for the exit status, the rest is
//! the output.
//!
//! - A checker run counts as an acceptance only with exit status 0 and
//!   `correct` as its last line that is not empty.
//! - A proof is extracted only from output that starts with `unsat`, and
//!   has a proof step.

use dp_cert::{accepted, extract_proof, has_trusted_step};

pub fn run(data: &[u8]) {
    let Some((first, output)) = data.split_first() else { return };
    let status = match first % 4 {
        0 => Some(0),
        1 => Some(1),
        2 => Some(u32::from(*first)),
        _ => None,
    };
    if accepted(status, output) {
        assert_eq!(status, Some(0), "an acceptance with another exit status");
        let text = String::from_utf8_lossy(output);
        let last = text.lines().filter(|l| !l.trim().is_empty()).last();
        assert_eq!(last, Some("correct"), "an acceptance without the checker's word");
    }
    if let Some(proof) = extract_proof(output) {
        assert!(output.starts_with(b"unsat\n") || output == b"unsat", "a proof from a run that did not say unsat");
        let proof_text = String::from_utf8(proof.clone()).expect("a proof is text");
        assert!(proof_text.contains("(step "), "a proof without a step");
        // No declaration reaches the checker twice: the reference has them.
        assert!(!proof_text.lines().any(|l| l.starts_with("(declare-")));
        let _ = has_trusted_step(&proof);
    }
    let _ = has_trusted_step(output);
}

pub fn seeds() -> Vec<Vec<u8>> {
    vec![
        b"\x00correct\n".to_vec(),
        b"\x00\nincorrect\ncorrect\n\n".to_vec(),
        b"\x01correct\n".to_vec(),
        b"\x00unsat\n(\n(declare-fun f () Int)\n(assume @p1 (= f f))\n(step @p2 :rule refl :premises (@p1))\n)\n".to_vec(),
        b"\x00unsat\n(step @p1 :rule trust :args (x))\n".to_vec(),
        b"\x00sat\n(step @p1 :rule refl)\n".to_vec(),
        b"\x00unsat\n(error \"no proof\")\n".to_vec(),
    ]
}
