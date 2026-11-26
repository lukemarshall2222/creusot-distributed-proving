//! The specification mutator (R7.5 criterion 1): from functions that meet
//! their contracts, functions that do not.
//!
//! Three operators, each of which gives a contract that the unchanged body
//! violates, with a witness that is found by running the body:
//!
//! - `flip`: the comparison of one postcondition is replaced by its
//!   opposite. The original holds at the sample arguments, so the mutated
//!   one does not.
//! - `shift`: one is added to the right side of a postcondition that is an
//!   equation.
//! - `drop`: the preconditions are removed. At the `bad` arguments the
//!   body overflows, which Creusot's proof of the function excludes. The
//!   witness is the failure of a process that calls it there (`call_bad`;
//!   the harness starts the process, so that the witness does not depend
//!   on whether a panic can be caught).

/// A function of the originals file.
struct Original {
    name: String,
    /// `(name, type)` of each parameter.
    params: Vec<(String, String)>,
    /// The line of the signature, and the lines of the body after it.
    signature: String,
    body: Vec<String>,
    requires: Vec<String>,
    ensures: Vec<String>,
    sample: Option<Vec<String>>,
    bad: Option<Vec<String>>,
}

fn arguments(text: &str) -> Vec<String> {
    text.trim().trim_start_matches('(').trim_end_matches(')').split(',').map(|a| a.trim().to_string()).collect()
}

fn parse(originals: &str) -> Vec<Original> {
    let mut out = Vec::new();
    let (mut requires, mut ensures, mut sample, mut bad) = (Vec::new(), Vec::new(), None, None);
    let mut lines = originals.lines();
    while let Some(line) = lines.next() {
        if let Some(args) = line.strip_prefix("// sample:") {
            sample = Some(arguments(args));
        } else if let Some(args) = line.strip_prefix("// bad:") {
            bad = Some(arguments(args));
        } else if let Some(spec) = line.strip_prefix("#[requires(").and_then(|s| s.strip_suffix(")]")) {
            requires.push(spec.to_string());
        } else if let Some(spec) = line.strip_prefix("#[ensures(").and_then(|s| s.strip_suffix(")]")) {
            ensures.push(spec.to_string());
        } else if let Some(rest) = line.strip_prefix("pub fn ") {
            let (name, rest) = rest.split_once('(').expect("a parameter list");
            let params = rest
                .split_once(')')
                .expect("the end of the parameter list")
                .0
                .split(',')
                .filter(|p| !p.trim().is_empty())
                .map(|p| {
                    let (n, t) = p.split_once(':').expect("a typed parameter");
                    (n.trim().to_string(), t.trim().to_string())
                })
                .collect();
            let mut body = Vec::new();
            for l in lines.by_ref() {
                body.push(l.to_string());
                if l == "}" {
                    break;
                }
            }
            out.push(Original {
                name: name.to_string(),
                params,
                signature: line.to_string(),
                body,
                requires: std::mem::take(&mut requires),
                ensures: std::mem::take(&mut ensures),
                sample: sample.take(),
                bad: bad.take(),
            });
        }
    }
    out
}

/// The comparison of a contract replaced by its opposite.
fn flip(spec: &str) -> Option<String> {
    [(" == ", " != "), (" >= ", " < "), (" <= ", " > ")]
        .iter()
        .find(|(from, _)| spec.contains(from))
        .map(|(from, to)| spec.replacen(from, to, 1))
}

/// A contract as a Rust expression over the parameters and `result`: an
/// integer view `x@` is `(x as i128)`.
fn executable(spec: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    for c in spec.chars() {
        if c.is_alphanumeric() || c == '_' || c == ':' {
            word.push(c);
        } else if c == '@' {
            out.push_str(&format!("({word} as i128)"));
            word.clear();
        } else {
            out.push_str(&word);
            word.clear();
            out.push(c);
        }
    }
    out + &word
}

struct Mutant {
    id: String,
    function: String,
    operator: &'static str,
    source: String,
    witness: String,
}

fn mutants(originals: &str) -> Vec<Mutant> {
    let mut out: Vec<Mutant> = Vec::new();
    for o in parse(originals) {
        let mut add = |operator: &'static str, tag: String, requires: &[String], ensures: Vec<String>, witness: &dyn Fn(&str) -> String| {
            let id = format!("m{:02}", out.len() + 1);
            let function = format!("{id}_{}_{tag}", o.name);
            let mut source = format!("/// {id}: `{}`, operator `{operator}`.\n", o.name);
            for r in requires {
                source.push_str(&format!("#[requires({r})]\n"));
            }
            for e in &ensures {
                source.push_str(&format!("#[ensures({e})]\n"));
            }
            source.push_str(&o.signature.replacen(&format!("pub fn {}(", o.name), &format!("pub fn {function}("), 1));
            source.push('\n');
            source.push_str(&o.body.join("\n"));
            source.push('\n');
            let witness = witness(&function);
            out.push(Mutant { id, function, operator, source, witness });
        };
        // The body run at the sample arguments, and the mutated
        // postcondition evaluated on what it returned.
        let at_sample = |sample: &[String], spec: &str| {
            let spec = spec.to_string();
            let bindings: String =
                o.params.iter().zip(sample).map(|((n, t), v)| format!("let {n}: {t} = {v}; ")).collect();
            let names: Vec<&str> = o.params.iter().map(|(n, _)| n.as_str()).collect();
            move |function: &str| format!("{{ {bindings}let result = {function}({}); !({}) }}", names.join(", "), executable(&spec))
        };
        if let Some(sample) = &o.sample {
            for (i, e) in o.ensures.iter().enumerate() {
                let with = |mutated: String| -> Vec<String> {
                    o.ensures.iter().enumerate().map(|(j, old)| if i == j { mutated.clone() } else { old.clone() }).collect()
                };
                if let Some(flipped) = flip(e) {
                    add("flip", format!("flip{}", i + 1), &o.requires, with(flipped.clone()), &at_sample(sample, &flipped));
                }
                if e.contains(" == ") {
                    let shifted = format!("{e} + 1");
                    add("shift", format!("shift{}", i + 1), &o.requires, with(shifted.clone()), &at_sample(sample, &shifted));
                }
            }
        }
        if let (Some(bad), false) = (&o.bad, o.requires.is_empty()) {
            let call = bad.join(", ");
            add("drop", "drop".to_string(), &[], o.ensures.clone(), &|function: &str| format!("{function}({call})"));
        }
    }
    out
}

/// The text of `creusot/mutants.rs` for the text of `creusot/originals.rs`.
pub fn generate(originals: &str) -> String {
    let mutants = mutants(originals);
    let mut out = String::from(
        "//! Canary functions that the specification mutator wrote from\n\
         //! `originals.rs`. Generated: do not edit; `cargo run -p dp-canaries\n\
         //! --example bless` rewrites this file.\n\nuse creusot_std::prelude::*;\n",
    );
    for m in &mutants {
        out.push('\n');
        out.push_str(&m.source);
    }
    out.push_str("\n/// Identifier, function and operator of every mutant.\n#[cfg(not(creusot))]\npub const MUTANTS: &[(&str, &str, &str)] = &[\n");
    for m in &mutants {
        out.push_str(&format!("    (\"{}\", \"{}\", \"{}\"),\n", m.id, m.function, m.operator));
    }
    out.push_str(
        "];\n\n/// Whether the mutant's contract is violated at its recorded arguments,\n\
         /// by running its body.\n#[cfg(not(creusot))]\n#[allow(clippy::all, unused_parens)]\npub fn witness(id: &str) -> Option<bool> {\n    Some(match id {\n",
    );
    for m in mutants.iter().filter(|m| m.operator != "drop") {
        out.push_str(&format!("        \"{}\" => {},\n", m.id, m.witness));
    }
    out.push_str(
        "        _ => return None,\n    })\n}\n\n\
         /// Calls a mutant of the operator `drop` at arguments that violate the\n\
         /// precondition that it lost. The call does not return: the body overflows.\n\
         #[cfg(not(creusot))]\npub fn call_bad(id: &str) {\n    match id {\n",
    );
    for m in mutants.iter().filter(|m| m.operator == "drop") {
        out.push_str(&format!("        \"{}\" => {{\n            {};\n        }}\n", m.id, m.witness));
    }
    out.push_str("        _ => {}\n    }\n}\n");
    out
}
