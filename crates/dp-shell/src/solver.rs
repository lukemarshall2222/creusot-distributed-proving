//! Running a solver process on a Printed_Goal (design decision D-16).
//!
//! The solver is started directly, as the leader of its own process group,
//! with the command line of the Why3 prover configuration. Its output and
//! exit status are kept; reading an answer from them is the job of the
//! pinned Why3 driver.

use std::{
    fs,
    io::{self, Read},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// Most output bytes of a run that are kept.
pub const OUTPUT_LIMIT: usize = 1 << 16;

/// Memory limit passed to provers in megabytes, Why3's default.
const MEMORY_LIMIT_MB: u64 = 1000;

/// The file that starting `program` runs: a path as given, or the first
/// match on PATH for a bare name, with symbolic links resolved.
pub fn find_executable(program: &str) -> io::Result<PathBuf> {
    if program.contains('/') {
        return fs::canonicalize(program);
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return fs::canonicalize(candidate);
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "not found on PATH"))
}

/// Splits a Why3 prover command line into words. Single quotes group a word.
pub fn words(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in command.chars() {
        match c {
            '\'' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            c => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out
}

/// Replaces Why3's placeholders (`%f`, `%t`, `%.t`, `%T`, `%m`, `%%`) in one
/// word of a prover command line. `micros` is the time limit.
pub fn substitute(word: &str, file: &Path, micros: u64) -> io::Result<String> {
    let mut out = String::new();
    let mut rest = word;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let (value, len) = if after.starts_with(".t") {
            (format!("{:.6}", micros as f64 / 1e6), 2)
        } else {
            match after.chars().next() {
                Some('f') => (file.display().to_string(), 1),
                Some('t') => (micros.div_ceil(1_000_000).max(1).to_string(), 1),
                Some('T') => (micros.div_ceil(1000).max(1).to_string(), 1),
                Some('m') => (MEMORY_LIMIT_MB.to_string(), 1),
                Some('%') => ("%".to_string(), 1),
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        format!("placeholder in prover command word {word:?}"),
                    ));
                }
            }
        };
        out.push_str(&value);
        rest = &after[len..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The command line of `command` with `seed` as the solver's random seed.
///
/// The seed is the only part of a command line that a Job can choose (the
/// rest is the Worker's own prover configuration), and it is a number. How a
/// seed is passed depends on the solver, which is recognized by the name of
/// its program; a solver without a known seed option runs unchanged.
pub fn with_seed(command: &str, seed: u64) -> String {
    let all = words(command);
    let program = all.first().map(|p| p.rsplit('/').next().unwrap_or(p).to_string()).unwrap_or_default();
    let quote = |w: &String| if w.contains(char::is_whitespace) { format!("'{w}'") } else { w.clone() };
    let mut out: Vec<String> = Vec::new();
    match program.as_str() {
        "z3" => {
            // Why3's command sets `sat.random_seed` and `smt.random_seed`.
            let mut found = false;
            for w in &all {
                match w.split_once("random_seed=") {
                    Some((prefix, _)) => {
                        found = true;
                        out.push(format!("{prefix}random_seed={seed}"));
                    }
                    None => out.push(quote(w)),
                }
            }
            if !found {
                out.push(format!("sat.random_seed={seed}"));
                out.push(format!("smt.random_seed={seed}"));
            }
        }
        "cvc5" | "cvc4" => {
            out.extend(all.iter().filter(|w| !w.starts_with("--seed=")).map(quote));
            // Keep the input file last.
            let file = out.iter().rposition(|w| w == "%f");
            let at = file.unwrap_or(out.len());
            out.insert(at, format!("--seed={seed}"));
        }
        _ => out.extend(all.iter().map(quote)),
    }
    out.join(" ")
}

/// A solver process that is running.
pub struct Run {
    child: Child,
    started: Instant,
    deadline: Instant,
    input: PathBuf,
    output: PathBuf,
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finished {
    /// The exit status, or `None` when the process tree was killed at its
    /// deadline or died from a signal.
    pub status: Option<u32>,
    /// True when the run was killed because its time was up.
    pub timed_out: bool,
    /// The first [`OUTPUT_LIMIT`] bytes of standard output and error.
    pub output: Vec<u8>,
    pub elapsed: Duration,
}

/// Starts `exe` on `input`, with the arguments of `command` (whose first
/// word, the program, is ignored in favour of `exe`).
///
/// Like why3find, the process tree is killed when half as much time again as
/// the limit has passed.
pub fn start(
    exe: &Path,
    command: &str,
    input: &[u8],
    extension: &str,
    micros: u64,
    dir: &Path,
    tag: &str,
) -> io::Result<Run> {
    let input_path = dir.join(format!("{tag}{extension}"));
    fs::write(&input_path, input)?;
    let output_path = dir.join(format!("{tag}.out"));
    let out = fs::File::create(&output_path)?;
    let args = words(command)
        .into_iter()
        .skip(1)
        .map(|w| substitute(&w, &input_path, micros))
        .collect::<io::Result<Vec<_>>>()?;
    let child = Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(out)
        .process_group(0)
        .spawn()?;
    let started = Instant::now();
    let deadline = started + Duration::from_micros(micros.saturating_add(micros / 2));
    Ok(Run { child, started, deadline, input: input_path, output: output_path })
}

impl Run {
    fn kill_group(&mut self) {
        // The solver leads its own process group: this ends the solver and
        // every process it started (R1.4 criterion 5, R1.5 criterion 17).
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", self.child.id())])
            .stderr(Stdio::null())
            .status();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn clean(&self) {
        let _ = fs::remove_file(&self.input);
        let _ = fs::remove_file(&self.output);
    }

    /// `Some` once the solver has exited or its time is up.
    pub fn poll(&mut self) -> io::Result<Option<Finished>> {
        let status = self.child.try_wait()?;
        let late = Instant::now() >= self.deadline;
        if status.is_none() && !late {
            return Ok(None);
        }
        let elapsed = self.started.elapsed();
        // Also ends whatever an exited solver left running in its group.
        self.kill_group();
        let mut output = Vec::new();
        fs::File::open(&self.output)?.take(OUTPUT_LIMIT as u64).read_to_end(&mut output)?;
        self.clean();
        Ok(Some(Finished {
            status: status.and_then(|s| s.code()).and_then(|c| u32::try_from(c).ok()),
            timed_out: status.is_none(),
            output,
            elapsed,
        }))
    }

    /// Ends the process tree and discards its output.
    pub fn cancel(mut self) {
        self.kill_group();
        self.clean();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_change_only_the_seed_options() {
        assert_eq!(
            with_seed("z3 -smt2 -T:%t sat.random_seed=42 nlsat.randomize=false smt.random_seed=42 -st %f", 7),
            "z3 -smt2 -T:%t sat.random_seed=7 nlsat.randomize=false smt.random_seed=7 -st %f"
        );
        assert_eq!(with_seed("'/a b/z3' -smt2 %f", 7), "'/a b/z3' -smt2 %f sat.random_seed=7 smt.random_seed=7");
        assert_eq!(
            with_seed("cvc5 --stats-internal --tlimit=%T %f", 3),
            "cvc5 --stats-internal --tlimit=%T --seed=3 %f"
        );
        assert_eq!(with_seed("cvc4 --seed=1 --lang=smt2 %f", 3), "cvc4 --lang=smt2 --seed=3 %f");
        // no known seed option: unchanged
        assert_eq!(with_seed("alt-ergo --timelimit %.t %f", 3), "alt-ergo --timelimit %.t %f");
        // the words of the seeded command are those of the original, plus the seed
        assert_eq!(words(&with_seed("'/a b/z3' -smt2 %f", 7))[0], "/a b/z3");
    }

    #[test]
    fn command_lines_split_and_substitute() {
        assert_eq!(
            words("'/a b/z3' -smt2  -T:%t %f"),
            vec!["/a b/z3", "-smt2", "-T:%t", "%f"]
        );
        let f = Path::new("/tmp/x.smt2");
        assert_eq!(substitute("-T:%t", f, 1_500_000).unwrap(), "-T:2");
        assert_eq!(substitute("--tlimit=%T", f, 1_500_000).unwrap(), "--tlimit=1500");
        assert_eq!(substitute("%.t", f, 200_000).unwrap(), "0.200000");
        assert_eq!(substitute("%f", f, 1).unwrap(), "/tmp/x.smt2");
        assert_eq!(substitute("100%%", f, 1).unwrap(), "100%");
        assert!(substitute("%l/lib", f, 1).is_err());
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dp-solver-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn wait(mut run: Run) -> Finished {
        loop {
            if let Some(f) = run.poll().unwrap() {
                return f;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn output_and_status_are_kept() {
        let d = dir("ok");
        let run = start(Path::new("/bin/sh"), "sh -c 'cat %f; exit 7'", b"hello\n", ".txt", 5_000_000, &d, "j").unwrap();
        let f = wait(run);
        assert_eq!((f.status, f.timed_out, f.output.as_slice()), (Some(7), false, &b"hello\n"[..]));
        assert_eq!(fs::read_dir(&d).unwrap().count(), 0);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_solver_that_ignores_termination_and_starts_children_is_killed() {
        let d = dir("stubborn");
        let marker = d.join("child-alive");
        // The child would create the marker after the deadline if it survived.
        let script = format!("trap \"\" TERM; (sleep 2; touch {}) & sleep 30", marker.display());
        let run = start(Path::new("/bin/sh"), &format!("sh -c '{script}'"), b"", ".txt", 200_000, &d, "j").unwrap();
        let f = wait(run);
        assert!(f.timed_out && f.status.is_none());
        assert!(f.elapsed < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(2500));
        assert!(!marker.exists(), "a process of the solver's group survived");
        fs::remove_dir_all(d).unwrap();
    }
}
