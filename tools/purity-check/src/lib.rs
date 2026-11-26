//! `ci/purity` (R7.3, design C20): dp-core is a pure decision core.
//!
//! Three checks:
//!
//! - the dependency closure of dp-core holds only crates of `allow.toml`;
//! - no source file of dp-core names an interface to the outside world or
//!   to shared state: files, the network, clocks, randomness, threads,
//!   processes, the environment, output, statics, `unsafe`;
//! - `registry.toml` lists exactly the public functions of dp-core, each
//!   as a decision function or as a helper, so that a new public function
//!   is classified before it is merged.

use std::{collections::BTreeSet, fs, path::Path, process::Command};

use serde::Deserialize;

/// What dp-core may not name, with the reason shown to the author.
pub const BANNED: &[(&str, &str)] = &[
    ("std::fs", "files"),
    ("std::net", "the network"),
    ("std::time", "clocks"),
    ("SystemTime", "clocks"),
    ("Instant", "clocks"),
    ("std::thread", "threads"),
    ("std::process", "processes"),
    ("std::env", "the environment"),
    ("std::io", "input and output"),
    ("std::sync", "shared state"),
    ("thread_local", "shared state"),
    ("OnceLock", "shared state"),
    ("OnceCell", "shared state"),
    ("lazy_static", "shared state"),
    ("static mut", "global mutable state"),
    ("rand::", "randomness"),
    ("getrandom", "randomness"),
    ("uuid::", "randomness"),
    ("println!", "output"),
    ("eprintln!", "output"),
    ("print!", "output"),
    ("dbg!", "output"),
    ("unsafe", "unsafe code"),
    ("extern \"", "foreign code"),
];

/// A source text without its comments and without the contents of its
/// string literals, line structure kept.
pub fn code_only(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for d in chars.by_ref() {
                    if d == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                for d in chars.by_ref() {
                    if d == '\n' {
                        out.push('\n');
                    }
                    if last == '*' && d == '/' {
                        break;
                    }
                    last = d;
                }
            }
            '"' => {
                out.push('"');
                let mut escaped = false;
                for d in chars.by_ref() {
                    if d == '\n' {
                        out.push('\n');
                    }
                    if !escaped && d == '"' {
                        break;
                    }
                    escaped = !escaped && d == '\\';
                }
                out.push('"');
            }
            other => out.push(other),
        }
    }
    out
}

/// The banned interfaces that a source file names: line number and reason.
pub fn scan(source: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (i, line) in code_only(source).lines().enumerate() {
        for (pattern, what) in BANNED {
            if line.contains(pattern) {
                found.push((i + 1, format!("names `{pattern}` ({what})")));
            }
        }
        // A static item is global state; `&'static` in a type is not.
        let trimmed = line.trim_start().trim_start_matches("pub ").trim_start_matches("pub(crate) ");
        if trimmed.starts_with("static ") {
            found.push((i + 1, "declares a static item (global state)".to_string()));
        }
    }
    found
}

/// The public functions of a source file of dp-core, as `module::name`.
/// Logic functions exist only for the proofs and are not listed.
pub fn public_functions(module: &str, source: &str) -> Vec<String> {
    let code = code_only(source);
    let lines: Vec<&str> = code.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line.trim_start().strip_prefix("pub fn ") else { continue };
        // The attributes of the function are the lines right above it.
        let logic = lines[..i]
            .iter()
            .rev()
            .take_while(|l| l.trim_start().starts_with('#') || l.trim().is_empty() || !l.contains(['{', '}', ';']))
            .any(|l| l.trim_start().starts_with("#[logic"));
        if logic {
            continue;
        }
        let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        out.push(format!("{module}::{name}"));
    }
    out
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    #[serde(default)]
    pub function: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// `module::name`.
    pub path: String,
    /// `decision`: its result decides a label, an acceptance, an
    /// assignment or a configuration. `helper`: anything else.
    pub kind: String,
    /// For a decision function: what it decides.
    pub decides: Option<String>,
}

/// Compares the registry with the public functions of dp-core.
pub fn check_registry(registry: &Registry, public: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for e in &registry.function {
        if !seen.insert(e.path.as_str()) {
            problems.push(format!("registry: `{}` is listed twice", e.path));
        }
        if !public.contains(&e.path) {
            problems.push(format!("registry: `{}` is no public function of dp-core", e.path));
        }
        match e.kind.as_str() {
            "decision" if e.decides.as_deref().is_none_or(|d| d.trim().is_empty()) => {
                problems.push(format!("registry: decision function `{}` does not say what it decides", e.path))
            }
            "decision" | "helper" => {}
            other => problems.push(format!("registry: `{}` has the unknown kind `{other}`", e.path)),
        }
    }
    for f in public {
        if !seen.contains(f.as_str()) {
            problems.push(format!("registry: the public function `{f}` of dp-core has no entry"));
        }
    }
    problems
}

/// The crates of the closure that the allow-list does not hold.
pub fn check_dependencies(closure: &[String], allowed: &[String]) -> Vec<String> {
    closure
        .iter()
        .filter(|c| c.as_str() != "dp-core" && !allowed.contains(c))
        .map(|c| format!("dependency: `{c}` is in the closure of dp-core and not in allow.toml"))
        .collect()
}

/// The normal (not build, not development) dependency closure of dp-core
/// on the platform of the build, as `cargo tree` resolves it from the lock
/// file.
pub fn dependency_closure(root: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("cargo")
        .args(["tree", "-p", "dp-core", "-e", "normal", "--prefix", "none", "--locked"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cargo tree: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo tree: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let names: BTreeSet<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect();
    if !names.contains("dp-core") {
        return Err("cargo tree: dp-core is not in its own closure".to_string());
    }
    Ok(names.into_iter().collect())
}

/// Every problem that the three checks find in the repository at `root`.
pub fn check(root: &Path) -> Result<Vec<String>, String> {
    let here = root.join("tools/purity-check");
    let read = |path: &Path| fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()));
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Allow {
        allowed: Vec<String>,
    }
    let allow: Allow = toml::from_str(&read(&here.join("allow.toml"))?).map_err(|e| format!("allow.toml: {e}"))?;
    let registry: Registry =
        toml::from_str(&read(&here.join("registry.toml"))?).map_err(|e| format!("registry.toml: {e}"))?;
    let mut problems = check_dependencies(&dependency_closure(root)?, &allow.allowed);
    let src = root.join("crates/dp-core/src");
    let mut files: Vec<_> = fs::read_dir(&src).map_err(|e| format!("{}: {e}", src.display()))?.flatten().collect();
    files.sort_by_key(|f| f.file_name());
    let mut public = Vec::new();
    for file in files {
        let path = file.path();
        if path.extension().is_none_or(|e| e != "rs") {
            // A directory would hide code from the scan.
            problems.push(format!("{}: not a Rust source file directly in dp-core/src", path.display()));
            continue;
        }
        let source = read(&path)?;
        let module = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        for (line, what) in scan(&source) {
            problems.push(format!("crates/dp-core/src/{module}.rs:{line}: {what}"));
        }
        public.extend(public_functions(&module, &source));
    }
    problems.extend(check_registry(&registry, &public));
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Feature: creusot-distributed-proving, Property 35: The decision core is pure, deterministic and exercised
    // P-ID: P7.3.2 (seeded violations of the purity rules are found)

    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn the_repository_passes() {
        let problems = check(&root()).unwrap();
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn a_call_to_each_banned_interface_is_found() {
        for (pattern, _) in BANNED {
            let source = format!("pub fn f() {{\n    let _ = {pattern};\n}}\n");
            assert_eq!(scan(&source).iter().filter(|(line, _)| *line == 2).count() >= 1, true, "{pattern}");
        }
        assert!(!scan("static COUNT: u64 = 0;\n").is_empty());
        assert!(!scan("pub static COUNT: u64 = 0;\n").is_empty());
    }

    #[test]
    fn comments_strings_and_static_lifetimes_are_not_violations() {
        let source = "// uses std::fs in a comment\n/* unsafe\n println! */\npub fn name() -> &'static str {\n    \"std::net and unsafe\"\n}\n";
        assert!(scan(source).is_empty(), "{:?}", scan(source));
    }

    #[test]
    fn a_dependency_outside_the_allow_list_is_found() {
        let allowed = vec!["creusot-std".to_string()];
        let closure = vec!["dp-core".to_string(), "creusot-std".to_string(), "tokio".to_string()];
        assert_eq!(check_dependencies(&closure, &allowed).len(), 1);
        assert!(check_dependencies(&closure[..2], &allowed).is_empty());
    }

    #[test]
    fn the_registry_and_the_public_functions_must_agree() {
        let source = "#[logic(open)]\npub fn spec(x: u64) -> bool { true }\n\n/// Decides.\n#[ensures(true)]\npub fn label(x: u64) -> u64 { x }\n\nimpl T {\n    pub fn rank(self) -> u8 { 0 }\n}\nfn private() {}\n";
        let public = public_functions("verdict", source);
        assert_eq!(public, vec!["verdict::label".to_string(), "verdict::rank".to_string()]);
        let entry = |path: &str, kind: &str, decides: Option<&str>| Entry {
            path: path.to_string(),
            kind: kind.to_string(),
            decides: decides.map(str::to_string),
        };
        let good = Registry {
            function: vec![entry("verdict::label", "decision", Some("the label")), entry("verdict::rank", "helper", None)],
        };
        assert!(check_registry(&good, &public).is_empty());
        // An entry that names no public function.
        let stale = Registry { function: vec![entry("verdict::gone", "helper", None)] };
        assert!(check_registry(&stale, &public).iter().any(|p| p.contains("`verdict::gone` is no public function")));
        // A public function without an entry.
        let missing = Registry { function: vec![entry("verdict::rank", "helper", None)] };
        assert!(check_registry(&missing, &public).iter().any(|p| p.contains("`verdict::label` of dp-core has no entry")));
        // A decision function that does not say what it decides, an
        // unknown kind, and an entry that is there twice.
        let vague = Registry {
            function: vec![
                entry("verdict::label", "decision", None),
                entry("verdict::rank", "other", None),
                entry("verdict::rank", "helper", None),
            ],
        };
        assert_eq!(check_registry(&vague, &public).len(), 3);
    }
}
