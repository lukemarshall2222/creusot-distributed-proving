//! Reading a certificate for the supported-fragment check (R5.2, R5.3
//! criterion 19): the commands of a CPC proof, and the rules that the
//! pinned signature declares. `dp_core::fragment::fragment_ok` decides.
//!
//! The reader is strict, because what it lets through is run by the
//! checker as a script. A proof is a sequence of parenthesised commands
//! with nothing between them but white space. Anything else at the top
//! level, a comment anywhere, an unbalanced parenthesis or an unclosed
//! string makes the proof one that is not in the fragment.

use std::{collections::BTreeSet, fs, io, path::Path};

use dp_core::fragment::{fragment_ok, Command, CommandKind};

/// One token of a script.
#[derive(Debug, PartialEq, Eq)]
enum Token<'a> {
    Open,
    Close,
    /// A symbol, keyword, number, string or quoted symbol, as written.
    Atom(&'a [u8]),
}

/// The tokens of a script; `None` if it has a comment, an unclosed string
/// or an unclosed quoted symbol.
fn tokens(text: &[u8]) -> Option<Vec<Token<'_>>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        match text[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'(' => {
                out.push(Token::Open);
                i += 1;
            }
            b')' => {
                out.push(Token::Close);
                i += 1;
            }
            // A comment hides what follows from this reader; cvc5 prints none.
            b';' => return None,
            b'"' => {
                // A string ends at a quote that is not doubled.
                let start = i;
                i += 1;
                loop {
                    match text.get(i)? {
                        b'"' if text.get(i + 1) == Some(&b'"') => i += 2,
                        b'"' => break,
                        _ => i += 1,
                    }
                }
                i += 1;
                out.push(Token::Atom(&text[start..i]));
            }
            b'|' => {
                let start = i;
                i += 1;
                while *text.get(i)? != b'|' {
                    i += 1;
                }
                i += 1;
                out.push(Token::Atom(&text[start..i]));
            }
            _ => {
                let start = i;
                while i < text.len() && !matches!(text[i], b' ' | b'\t' | b'\n' | b'\r' | b'(' | b')' | b';' | b'"' | b'|') {
                    i += 1;
                }
                out.push(Token::Atom(&text[start..i]));
            }
        }
    }
    Some(out)
}

/// The commands of a proof, with each step's rule looked up in `rules`.
/// `None` if the text is no sequence of commands.
pub fn commands(proof: &[u8], rules: &BTreeSet<String>) -> Option<Vec<Command>> {
    let tokens = tokens(proof)?;
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        // A command opens at the top level and has a name.
        if tokens[i] != Token::Open {
            return None;
        }
        let Some(Token::Atom(name)) = tokens.get(i + 1) else { return None };
        let kind = match *name {
            b"assume" => CommandKind::Assume,
            b"step" => CommandKind::Step,
            b"assume-push" => CommandKind::AssumePush,
            b"step-pop" => CommandKind::StepPop,
            _ => CommandKind::Other,
        };
        // The rule: the atom after the keyword `:rule` among the items of
        // the command itself. A command with two is malformed.
        let mut depth = 1usize;
        let mut rule: Option<&[u8]> = None;
        let mut named = 0;
        i += 2;
        while depth > 0 {
            match tokens.get(i)? {
                Token::Open => depth += 1,
                Token::Close => depth -= 1,
                Token::Atom(b":rule") if depth == 1 => {
                    named += 1;
                    rule = match tokens.get(i + 1) {
                        Some(Token::Atom(rule)) => Some(rule),
                        _ => None,
                    };
                }
                Token::Atom(_) => {}
            }
            i += 1;
        }
        let rule_pinned =
            named == 1 && rule.and_then(|r| std::str::from_utf8(r).ok()).is_some_and(|r| rules.contains(r));
        out.push(Command { kind, rule_pinned });
    }
    Some(out)
}

/// Whether a proof may be given to the checker: it reads as commands, has
/// at least one, and `fragment_ok` accepts them.
pub fn in_fragment(proof: &[u8], rules: &BTreeSet<String>) -> bool {
    commands(proof, rules).is_some_and(|commands| !commands.is_empty() && fragment_ok(&commands))
}

/// The rules that the signature files under `dir` declare: the names of
/// their `declare-rule` commands. The files are those whose hash the
/// Lock_File pins.
pub fn rules(dir: &Path) -> io::Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if fs::metadata(&path)?.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|e| e == "eo") {
                rules_of(&fs::read(&path)?, &mut out);
            }
        }
    }
    Ok(out)
}

/// The names after `(declare-rule` in a signature file. Signature files
/// have comments; a line is read up to its first `;`.
fn rules_of(text: &[u8], out: &mut BTreeSet<String>) {
    for line in String::from_utf8_lossy(text).lines() {
        let code = line.split(';').next().unwrap_or_default();
        let mut rest = code;
        while let Some(at) = rest.find("(declare-rule") {
            rest = &rest[at + "(declare-rule".len()..];
            let name: String = rest
                .trim_start()
                .chars()
                .take_while(|c| !c.is_whitespace() && !matches!(c, '(' | ')'))
                .collect();
            if rest.starts_with(char::is_whitespace) && !name.is_empty() {
                out.insert(name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Feature: creusot-distributed-proving, Property 25: Certificates stay inside the supported fragment
    // P-ID: P5.2.11, P5.3.1 (a proof with a banned command is not in the fragment)

    fn pinned() -> BTreeSet<String> {
        ["refl", "trans", "arith-elim-lt", "scope"].iter().map(|r| r.to_string()).collect()
    }

    const PROOF: &str = "(assume @p1 (> x 0))\n(step @p2 :rule refl :args (0))\n(assume-push @p3 (= x \"a ) b\"))\n\
(step-pop @p4 :rule scope :premises (@p2))\n(step @p5 false :rule trans :premises (@p1 @p4))\n";

    #[test]
    fn a_proof_of_the_four_commands_with_pinned_rules_is_in_the_fragment() {
        let read = commands(PROOF.as_bytes(), &pinned()).unwrap();
        assert_eq!(read.len(), 5);
        assert_eq!(read[2], Command { kind: CommandKind::AssumePush, rule_pinned: false });
        assert!(in_fragment(PROOF.as_bytes(), &pinned()));
        // On one line, and with other white space.
        assert!(in_fragment(PROOF.replace('\n', " ").as_bytes(), &pinned()));
        assert!(in_fragment(PROOF.replace('\n', "\r\n\t").as_bytes(), &pinned()));
    }

    #[test]
    fn every_command_that_changes_what_the_checker_accepts_is_banned() {
        let forged = "(declare-rule dp_bad () :conclusion false)\n(step @p1 false :rule dp_bad)\n";
        assert!(!in_fragment(forged.as_bytes(), &pinned()));
        // The rule alone, which the signature does not declare.
        assert!(!in_fragment(b"(step @p1 false :rule dp_bad)\n", &pinned()));
        for command in [
            "(declare-rule dp_bad () :conclusion false)",
            "(program p ((x Bool)) (Bool) Bool (((p x) x)))",
            "(declare-oracle-fun o (Int) Bool run)",
            "(include \"other.eo\")",
            "(reference \"other.smt2\")",
            "(reset)",
            "(exit)",
            "(set-option :x true)",
            "(define @t1 () (> x 0))",
            "(declare-const x Int)",
            "(declare-type T ())",
            "(define-fun f () Bool false)",
            "(assert false)",
            "(push)",
            "(echo \"correct\")",
            "()",
            "((step @p9 :rule refl))",
        ] {
            // Wherever it stands: first, last, and after a command on its line.
            for text in [format!("{command}\n{PROOF}"), format!("{PROOF}{command}\n"), PROOF.replacen('\n', &format!(" {command}\n"), 1)] {
                assert!(!in_fragment(text.as_bytes(), &pinned()), "{command}");
            }
        }
    }

    #[test]
    fn a_text_that_is_no_sequence_of_commands_is_not_in_the_fragment() {
        for text in [
            "",
            "   \n",
            "unsat\n(step @p1 :rule refl)\n",
            "(step @p1 :rule refl",
            "(step @p1 :rule refl))",
            "(step @p1 :rule refl) x",
            "(step @p1 :rule refl) ; (declare-rule dp_bad)\n",
            "(step @p1 :rule refl :args (\"unclosed))",
            "(step @p1 :rule refl :args (|unclosed))",
        ] {
            assert!(!in_fragment(text.as_bytes(), &pinned()), "{text:?}");
        }
    }

    #[test]
    fn the_rule_of_a_step_is_the_one_that_the_checker_reads() {
        let ok = |text: &str| in_fragment(text.as_bytes(), &pinned());
        assert!(ok("(step @p1 :rule refl)"));
        // No rule, a rule that is a list, two rules, and a rule keyword
        // that is deeper than the command or inside a string.
        assert!(!ok("(step @p1 false)"));
        assert!(!ok("(step @p1 :rule (refl))"));
        assert!(!ok("(step @p1 :rule)"));
        assert!(!ok("(step @p1 :rule refl :rule dp_bad)"));
        assert!(!ok("(step @p1 :rule dp_bad :rule refl)"));
        assert!(!ok("(step @p1 (:rule refl) :rule dp_bad)"));
        assert!(!ok("(step @p1 \":rule refl\" :rule dp_bad)"));
        assert!(!ok("(step @p1 |:rule refl| :rule dp_bad)"));
        assert!(ok("(step @p1 (= \":rule dp_bad\" x) :rule refl)"));
        assert!(ok("(step @p1 (f (:rule dp_bad)) :rule refl)"));
        // A string with a doubled quote does not end at it, wherever in
        // the proof it stands.
        assert!(ok("(step @p1 (= \"a\"\"b\" x) :rule refl)"));
        assert!(ok("(assume @p0 (= x 0))\n(step @p1 (= \"\"\"\" x) :rule refl)\n(step @p2 (= \"a\"\"\" \"\"\"b\") :rule trans)"));
        // Each alone, so that a reader that loses its place in one string
        // cannot find it again in the next: a string that is one quote, an
        // empty string, and an empty string before a closing parenthesis.
        assert!(ok("(step @p1 (= \"\"\"\" x) :rule refl)"));
        assert!(ok("(step @p1 (= \"\" x) :rule refl)"));
        assert!(ok("(step @p1 :rule refl :args (\"\"))"));
        assert!(!ok("(step @p1 :rule dp_bad :args (\"\" \":rule refl\"))"));
        assert!(!ok("(step @p1 (= \"a\"\" :rule refl) x\") :rule dp_bad)"));
        // The name of a step is no command name.
        assert!(!ok("(stepx @p1 :rule refl)"));
        assert!(!ok("(|step| @p1 :rule refl)"));
    }

    #[test]
    fn the_rules_are_those_that_the_signature_files_declare() {
        let dir = std::env::temp_dir().join(format!("dp-rules-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("rules")).unwrap();
        fs::write(dir.join("Cpc.eo"), "(include \"rules/A.eo\")\n; (declare-rule commented ())\n(declare-rule refl ((T Type))\n  :conclusion true)\n").unwrap();
        fs::write(dir.join("rules/A.eo"), "(declare-rule arith-elim-lt ()) (declare-rule\ttrans ())\n(declare-rules x)\n(program p ())\n").unwrap();
        fs::write(dir.join("rules/notes.txt"), "(declare-rule from_notes ())\n").unwrap();
        let found: Vec<String> = rules(&dir).unwrap().into_iter().collect();
        assert_eq!(found, ["arith-elim-lt", "refl", "trans"]);
        fs::remove_dir_all(dir).unwrap();
    }
}
