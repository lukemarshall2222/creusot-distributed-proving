//! Certificates (design C11 to C13, R5): production of a proof by cvc5 in
//! its CPC format, the check by the pinned checker Ethos against a
//! reference, and the Certificate_Store.
//!
//! Nothing that produces a certificate is trusted. The label certified
//! rests on the checker binary and its signature files, both pinned in the
//! Lock_File, and on the reference being the Front_End's own print of the
//! Goal: Ethos accepts a proof only if its assumptions are assertions of
//! the reference (`--reference`) and it derives false
//! (`--require-proof-of-false`).

use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use dp_formats::{
    hash::{self, Hash},
    sexp::{self, Sexp},
};

/// Size limit of a certificate. A working value, not a proposal for an
/// open parameter.
pub const PROOF_LIMIT: u64 = 1 << 28;

/// The options with which cvc5 writes a proof that Ethos can check: the
/// CPC format, no trusted steps, and no sharing of terms across the proof,
/// which the checker does not read.
pub const PROOF_OPTIONS: [&str; 5] =
    ["--dump-proofs", "--proof-format=cpc", "--no-proof-dag-global", "--no-proof-allow-trust", "--dag-thresh=0"];

/// Runs a program with a time limit; returns its exit status and its
/// standard output, or `None` if it was ended at the limit.
fn run(command: &mut Command, limit: Duration, out: &Path) -> io::Result<Option<(Option<i32>, Vec<u8>)>> {
    let mut child = command.stdin(Stdio::null()).stdout(fs::File::create(out)?).stderr(Stdio::null()).spawn()?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let mut output = Vec::new();
    fs::File::open(out)?.take(PROOF_LIMIT).read_to_end(&mut output)?;
    Ok(Some((status.code(), output)))
}

/// The proof in what cvc5 printed: after the line `unsat`, the proof
/// commands without cvc5's outer parentheses and without declarations,
/// which the reference already has (the checker rejects a symbol that is
/// declared twice). `None` if cvc5 did not answer `unsat` with a proof.
pub fn extract_proof(output: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(output).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "unsat" {
        return None;
    }
    let mut proof = String::new();
    for line in lines {
        if line == "(" || line == ")" || line.starts_with("(declare-") || line.starts_with("(error ") {
            continue;
        }
        if !line.starts_with('(') {
            return None;
        }
        proof.push_str(line);
        proof.push('\n');
    }
    proof.contains("(step ").then(|| proof.into_bytes())
}

/// Whether the proof has a step that a checker takes on trust.
pub fn has_trusted_step(proof: &[u8]) -> bool {
    let text = String::from_utf8_lossy(proof);
    text.lines().any(|l| l.contains(":rule trust") || l.contains(":rule hole") || l.contains("(trust "))
}

/// Has cvc5 prove the goal again with a proof. The goal is the print for
/// cvc5's driver; `extension` is that of the driver's input files.
pub fn produce(cvc5: &Path, goal: &[u8], extension: &str, limit: Duration, tmp: &Path) -> io::Result<Option<Vec<u8>>> {
    let input = tmp.join(format!("goal{extension}"));
    fs::write(&input, goal)?;
    let mut command = Command::new(cvc5);
    command.arg(format!("--tlimit={}", limit.as_millis().max(1))).args(PROOF_OPTIONS).arg(&input);
    // cvc5 stops itself at its limit; the outer limit is for a cvc5 that
    // does not.
    let result = run(&mut command, limit * 2 + Duration::from_secs(1), &tmp.join("cvc5.out"))?;
    Ok(result.and_then(|(_, output)| extract_proof(&output)))
}

/// The pinned checker and its signature files.
#[derive(Clone, Debug)]
pub struct Checker {
    pub ethos: PathBuf,
    /// The directory of the CPC signature; `Cpc.eo` is its entry file.
    pub signature: PathBuf,
}

impl Checker {
    /// Runs the checker on a proof with `goal` as the reference. True only
    /// if the checker ended normally and said `correct`.
    pub fn check(&self, goal: &[u8], extension: &str, proof: &[u8], limit: Duration, tmp: &Path) -> io::Result<bool> {
        let reference = tmp.join(format!("reference{extension}"));
        let certificate = tmp.join("proof.cpc");
        fs::write(&reference, goal)?;
        fs::write(&certificate, proof)?;
        let mut command = Command::new(&self.ethos);
        command
            .arg(format!("--include={}", self.signature.join("Cpc.eo").display()))
            .arg(format!("--reference={}", reference.display()))
            .arg("--require-proof-of-false")
            .arg(&certificate);
        let Some((status, output)) = run(&mut command, limit, &tmp.join("ethos.out"))? else { return Ok(false) };
        let said = String::from_utf8_lossy(&output);
        Ok(status == Some(0) && said.lines().filter(|l| !l.trim().is_empty()).last() == Some("correct"))
    }

    pub fn binary_hash(&self) -> io::Result<Hash> {
        Ok(hash::sha256(&fs::read(fs::canonicalize(&self.ethos)?)?))
    }

    pub fn signature_hash(&self) -> io::Result<Hash> {
        tree_hash(&self.signature)
    }
}

/// The hash of a directory of files: the SHA-256 of one line
/// `<sha256 of the file>  ./<path>` per file, in the order of the paths.
/// `deps/gen-lock.sh` computes the same value for the Lock_File.
pub fn tree_hash(dir: &Path) -> io::Result<Hash> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, Hash)>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
            let path = entry.path();
            if fs::metadata(&path)?.is_dir() {
                walk(&path, &name, out)?;
            } else {
                out.push((name, hash::sha256(&fs::read(&path)?)));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, ".", &mut files)?;
    files.sort();
    let mut listing = String::new();
    for (name, hash) in files {
        listing.push_str(&format!("{}  {name}\n", hash.to_hex()));
    }
    Ok(hash::sha256(listing.as_bytes()))
}

/// One entry of the Certificate_Store: the certificate, the print it was
/// checked against, and what checked it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Certificate {
    /// The Printed_Goal that the checker had as its reference.
    pub goal: Vec<u8>,
    /// Extension of that print's file, with its dot.
    pub extension: String,
    pub proof: Vec<u8>,
    /// Task_Hash of the print, and the Driver that printed it.
    pub task_hash: Hash,
    pub driver: Vec<u8>,
    /// The prover whose driver printed the Goal, as why3find names it.
    pub prover: String,
    /// Hashes of the checker binary and of its signature files.
    pub checker_hash: Hash,
    pub signature_hash: Hash,
}

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    /// The entry is not complete, or its files do not match its manifest.
    Damaged(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "{e}"),
            StoreError::Damaged(m) => write!(f, "the certificate entry is damaged: {m}"),
        }
    }
}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        StoreError::Io(e)
    }
}

impl Certificate {
    /// The manifest: everything of the entry but the two files, which it
    /// names by their hashes. The content hash of the entry is the hash of
    /// the manifest.
    fn manifest(&self) -> Vec<u8> {
        sexp::print(&Sexp::list([
            Sexp::atom("dp-certificate"),
            Sexp::atom(hash::sha256(&self.goal).0),
            Sexp::atom(&self.extension),
            Sexp::atom(hash::sha256(&self.proof).0),
            Sexp::atom(self.task_hash.0),
            Sexp::atom(&self.driver),
            Sexp::atom(&self.prover),
            Sexp::atom(self.checker_hash.0),
            Sexp::atom(self.signature_hash.0),
        ]))
    }

    pub fn content_hash(&self) -> Hash {
        hash::sha256(&self.manifest())
    }

    /// Stores the entry under `store/<content hash>/` (R5.4): the files
    /// and the manifest are written and synced in a temporary directory,
    /// which is then renamed; the rename is the event that makes the entry
    /// complete. An entry that is there already is kept.
    pub fn store(&self, store: &Path) -> io::Result<Hash> {
        let content = self.content_hash();
        let target = store.join(content.to_hex());
        if target.is_dir() {
            return Ok(content);
        }
        fs::create_dir_all(store)?;
        let tmp = store.join(format!(".{}.{}.tmp", content.to_hex(), std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir(&tmp)?;
        let result = (|| {
            for (name, bytes) in [("goal", &self.goal), ("proof", &self.proof), ("manifest", &self.manifest())] {
                let mut file = fs::File::create(tmp.join(name))?;
                io::Write::write_all(&mut file, bytes)?;
                file.sync_all()?;
            }
            fs::File::open(&tmp)?.sync_all()?;
            fs::rename(&tmp, &target)?;
            fs::File::open(store)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&tmp);
            // Another writer may have completed the same entry meanwhile.
            if target.is_dir() {
                return Ok(content);
            }
        }
        result.map(|()| content)
    }

    /// Loads the entry with this content hash and checks it against its
    /// manifest.
    pub fn load(store: &Path, content: Hash) -> Result<Certificate, StoreError> {
        let dir = store.join(content.to_hex());
        let read = |name: &str| fs::read(dir.join(name)).map_err(|e| StoreError::Damaged(format!("{name}: {e}")));
        let manifest = read("manifest")?;
        if hash::sha256(&manifest) != content {
            return Err(StoreError::Damaged("the manifest does not have the content hash".into()));
        }
        let damaged = || StoreError::Damaged("the manifest is not a certificate manifest".into());
        let parsed = sexp::parse(&manifest, 1 << 16).map_err(|_| damaged())?;
        let Some([tag, goal_hash, extension, proof_hash, task_hash, driver, prover, checker, signature]) =
            parsed.as_list()
        else {
            return Err(damaged());
        };
        let hash32 = |s: &Sexp| s.as_atom().and_then(|b| <[u8; 32]>::try_from(b).ok()).map(Hash).ok_or_else(damaged);
        if tag.as_str() != Some("dp-certificate") {
            return Err(damaged());
        }
        let certificate = Certificate {
            goal: read("goal")?,
            extension: extension.as_str().ok_or_else(damaged)?.to_string(),
            proof: read("proof")?,
            task_hash: hash32(task_hash)?,
            driver: driver.as_atom().ok_or_else(damaged)?.to_vec(),
            prover: prover.as_str().ok_or_else(damaged)?.to_string(),
            checker_hash: hash32(checker)?,
            signature_hash: hash32(signature)?,
        };
        if hash::sha256(&certificate.goal) != hash32(goal_hash)? || hash::sha256(&certificate.proof) != hash32(proof_hash)?
        {
            return Err(StoreError::Damaged("a file does not have the hash that the manifest names".into()));
        }
        // The manifest is canonical: these fields print to the same bytes.
        if certificate.manifest() != manifest {
            return Err(damaged());
        }
        Ok(certificate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn certificate() -> Certificate {
        Certificate {
            goal: b"(assert false)\n".to_vec(),
            extension: ".smt2".into(),
            proof: b"(step @p1 :rule x)\n".to_vec(),
            task_hash: Hash([1; 32]),
            driver: b"driver".to_vec(),
            prover: "cvc5@".into(),
            checker_hash: Hash([2; 32]),
            signature_hash: Hash([3; 32]),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dp-cert-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // P-ID: P5.4.1, P5.4.5
    #[test]
    fn an_entry_is_read_back_by_its_content_hash_and_damage_is_found() {
        let store = scratch("store");
        let c = certificate();
        let content = c.store(&store).unwrap();
        assert_eq!(content, c.content_hash());
        assert_eq!(Certificate::load(&store, content).unwrap(), c);
        // storing again keeps the entry, and no temporary directory is left
        assert_eq!(c.store(&store).unwrap(), content);
        assert_eq!(fs::read_dir(&store).unwrap().count(), 1);
        // another certificate has another content hash
        let mut other = certificate();
        other.proof.push(b' ');
        assert_ne!(other.content_hash(), content);
        // a changed file, a changed manifest and a missing file are found
        let dir = store.join(content.to_hex());
        for (name, change) in [("proof", Some(b"x".to_vec())), ("goal", Some(b"x".to_vec())), ("manifest", Some(b"x".to_vec())), ("proof", None)] {
            let saved = fs::read(dir.join(name)).unwrap();
            match change {
                Some(bytes) => fs::write(dir.join(name), bytes).unwrap(),
                None => fs::remove_file(dir.join(name)).unwrap(),
            }
            assert!(matches!(Certificate::load(&store, content), Err(StoreError::Damaged(_))), "{name}");
            fs::write(dir.join(name), saved).unwrap();
        }
        assert!(Certificate::load(&store, Hash([9; 32])).is_err());
        fs::remove_dir_all(store).unwrap();
    }

    #[test]
    fn proofs_are_taken_from_cvc5_output_without_declarations() {
        let out = b"unsat\n(\n(declare-const x Int)\n(assume @p1 (not (= x x)))\n(step @p2 :rule refl :args (x))\n)\n(error \"x\")\n";
        assert_eq!(extract_proof(out).unwrap(), b"(assume @p1 (not (= x x)))\n(step @p2 :rule refl :args (x))\n");
        assert_eq!(extract_proof(b"unknown\n"), None);
        assert_eq!(extract_proof(b"unsat\n"), None);
        assert_eq!(extract_proof(b"unsat\n(\nnot a command\n)\n"), None);
        assert!(has_trusted_step(b"(step @p1 :rule trust :args (x))\n"));
        assert!(!has_trusted_step(b"(step @p1 :rule refl :args (x))\n"));
    }

    #[test]
    fn the_tree_hash_depends_on_names_and_contents() {
        let dir = scratch("tree");
        fs::create_dir_all(dir.join("rules")).unwrap();
        fs::write(dir.join("Cpc.eo"), "a").unwrap();
        fs::write(dir.join("rules/Arith.eo"), "b").unwrap();
        let first = tree_hash(&dir).unwrap();
        assert_eq!(tree_hash(&dir).unwrap(), first);
        fs::write(dir.join("rules/Arith.eo"), "c").unwrap();
        assert_ne!(tree_hash(&dir).unwrap(), first);
        fs::write(dir.join("rules/Arith.eo"), "b").unwrap();
        fs::rename(dir.join("rules/Arith.eo"), dir.join("rules/Other.eo")).unwrap();
        assert_ne!(tree_hash(&dir).unwrap(), first);
        fs::remove_dir_all(dir).unwrap();
    }
}
