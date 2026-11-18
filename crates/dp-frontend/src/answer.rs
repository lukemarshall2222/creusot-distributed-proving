//! Reading a prover's answer from its output.
//!
//! The patterns below restate the answer patterns of the pinned Why3 drivers
//! (`smt-libv2.gen`, `cvc5.drv`, `z3_4_12.drv`, `cvc4_16.gen`,
//! `alt_ergo_smt.drv`), and the extensions restate their `filename` lines. They
//! are a stand-in: the answer should come from the pinned driver itself,
//! through the why3find bridge, which does not offer that yet. A driver that
//! is not listed here is refused, so no answer is read with the wrong patterns.

use dp_core::labels::ProverAnswer;

/// The input language of a prover, which decides its answer patterns and the
/// file extension of its input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// cvc5, cvc4 and Z3.
    SmtLib,
    /// Alt-Ergo 2.6 with SMT-LIB input (`alt_ergo_smt.drv`).
    AltErgoSmt,
}

impl Family {
    /// The family of the Why3 driver named `driver`, if it is a supported one.
    pub fn of_driver(driver: &str) -> Option<Family> {
        match driver {
            "cvc5" | "cvc4_17" | "z3_4_12" => Some(Family::SmtLib),
            "alt_ergo_26" => Some(Family::AltErgoSmt),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Family::SmtLib => "smt2",
            Family::AltErgoSmt => "psmt2",
        }
    }
}

/// The answer in `output`. Anything that matches no pattern is a failure, so
/// an unexpected output never counts as a proof.
pub fn read(family: Family, output: &str) -> ProverAnswer {
    for line in output.lines() {
        let line = line.trim_end();
        let smt = match line {
            "unsat" => Some(ProverAnswer::Valid),
            // Why3 reads `sat` as unknown: the encodings are incomplete.
            "sat" | "unknown" | "Fail" => Some(ProverAnswer::Unknown),
            "timeout" => Some(ProverAnswer::Timeout),
            _ if line.contains("interrupted by timeout") => Some(ProverAnswer::Timeout),
            _ => None,
        };
        let answer = match family {
            Family::SmtLib => smt,
            Family::AltErgoSmt => {
                let verdict = line
                    .strip_prefix("; File \"")
                    .and_then(|rest| rest.split_once("\", line "))
                    .and_then(|(_, rest)| rest.split_once(':'))
                    .map(|(_, verdict)| verdict.trim_start());
                match verdict {
                    Some(v) if v.starts_with("Valid") => Some(ProverAnswer::Valid),
                    Some(v) if v.starts_with("I don't know") => Some(ProverAnswer::Unknown),
                    _ if line == "unsupported" => Some(ProverAnswer::Unknown),
                    _ => smt,
                }
            }
        };
        if let Some(answer) = answer {
            return answer;
        }
    }
    ProverAnswer::Failure
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smt_answers() {
        assert_eq!(read(Family::SmtLib, "unsat\n(:reason-unknown ...)\n"), ProverAnswer::Valid);
        assert_eq!(read(Family::SmtLib, "sat\n"), ProverAnswer::Unknown);
        assert_eq!(read(Family::SmtLib, "unknown\n"), ProverAnswer::Unknown);
        assert_eq!(read(Family::SmtLib, "cvc5 interrupted by timeout.\n"), ProverAnswer::Timeout);
        assert_eq!(read(Family::SmtLib, ""), ProverAnswer::Failure);
        assert_eq!(read(Family::SmtLib, "(error \"unsat\")\n"), ProverAnswer::Failure);
        assert_eq!(read(Family::SmtLib, " unsat\n"), ProverAnswer::Failure);
        // the first answer line decides
        assert_eq!(read(Family::SmtLib, "sat\nunsat\n"), ProverAnswer::Unknown);
    }

    #[test]
    fn alt_ergo_answers() {
        let line = |v: &str| format!("; File \"/tmp/x.psmt2\", line 12, characters 3-40: {v}\n");
        let f = Family::AltErgoSmt;
        assert_eq!(read(f, &line("Valid (0.0081) (12 steps) (goal g)")), ProverAnswer::Valid);
        assert_eq!(read(f, &line("I don't know (0.1)")), ProverAnswer::Unknown);
        assert_eq!(read(f, "unsat\n"), ProverAnswer::Valid);
        assert_eq!(read(f, "Valid\n"), ProverAnswer::Failure);
        assert_eq!(read(f, "File \"x\", line 1, character 0-1:\nError Syntax error\n"), ProverAnswer::Failure);
    }

    #[test]
    fn only_listed_drivers_are_supported() {
        assert_eq!(Family::of_driver("cvc5"), Some(Family::SmtLib));
        assert_eq!(Family::of_driver("alt_ergo_26"), Some(Family::AltErgoSmt));
        assert_eq!(Family::of_driver("alt_ergo"), None);
        assert_eq!(Family::of_driver("coq"), None);
    }
}
