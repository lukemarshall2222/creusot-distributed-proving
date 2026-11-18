//! The Worker's own prover table, read from its Why3 configuration file
//! (design decision D-16). A Worker runs only commands from this table,
//! never a command line that arrives over the network.

use std::collections::BTreeMap;

/// Command lines by prover, keyed as the prover configuration names provers:
/// the lower-case name, `@`, and the version (empty when none is stated).
pub fn parse(conf: &str) -> BTreeMap<String, String> {
    let mut provers = BTreeMap::new();
    let mut section: Option<BTreeMap<String, String>> = None;
    let mut finish = |section: Option<BTreeMap<String, String>>| {
        let Some(fields) = section else { return };
        // Alternative configurations of a prover are separate entries that
        // why3find does not name in prover calls.
        if fields.contains_key("alternative") {
            return;
        }
        if let (Some(name), Some(command)) = (fields.get("name"), fields.get("command")) {
            let version = fields.get("version").map(String::as_str).unwrap_or("");
            provers.insert(format!("{}@{version}", name.to_lowercase()), command.clone());
        }
    };
    for line in conf.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            finish(section.take());
            if line == "[prover]" {
                section = Some(BTreeMap::new());
            }
        } else if let (Some(fields), Some((key, value))) = (section.as_mut(), line.split_once('=')) {
            let value = value.trim();
            if let Some(text) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                fields.insert(key.trim().to_string(), text.replace("\\\"", "\"").replace("\\\\", "\\"));
            }
        }
    }
    finish(section.take());
    provers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provers_are_keyed_like_prover_calls() {
        let conf = r#"
[main]
magic = 14

[prover]
name = "Alt-Ergo"
command = "alt-ergo --timelimit %.t %f"
driver = "alt_ergo_26"

[prover]
name = "Alt-Ergo"
alternative = "manual"
command = "alt-ergo --other %f"
driver = "alt_ergo_26"

[prover]
name = "Z3"
version = "5.3.2"
command = "'/a b/z3' -smt2 -T:%t %f"
driver = "z3_4_12"

[strategy]
name = "Auto"
"#;
        let provers = parse(conf);
        assert_eq!(provers.len(), 2);
        assert_eq!(provers["alt-ergo@"], "alt-ergo --timelimit %.t %f");
        assert_eq!(provers["z3@5.3.2"], "'/a b/z3' -smt2 -T:%t %f");
    }
}
