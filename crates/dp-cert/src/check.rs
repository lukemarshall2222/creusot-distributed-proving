//! The check of a certificate (design C12, R5.3): the command line of the
//! pinned checker, what counts as its acceptance, and what is read of a
//! proof before it is trusted with anything.
//!
//! The checker is Ethos with the CPC signature files, both pinned in the
//! Lock_File. It accepts a proof only if the proof's assumptions are
//! assertions of the reference (`--reference`) and it derives false
//! (`--require-proof-of-false`). A run counts as an acceptance only with
//! exit status 0 and `correct` as the last line that is not empty; a
//! proof with a step that the checker takes on trust is not complete.

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use dp_formats::hash::{self, Hash};

use crate::run;

/// The command line of a check of a certificate, in the same form: `%r` is
/// the reference and `%f` the proof.
pub fn check_command(ethos: &str, signature: &Path) -> String {
    format!("'{ethos}' '--include={}' --reference=%r --require-proof-of-false %f", signature.join("Cpc.eo").display())
}

/// Whether what the checker printed and its exit status are an acceptance.
pub fn accepted(status: Option<u32>, output: &[u8]) -> bool {
    let said = String::from_utf8_lossy(output);
    status == Some(0) && said.lines().filter(|l| !l.trim().is_empty()).last() == Some("correct")
}

/// Whether the proof has a step that a checker takes on trust.
pub fn has_trusted_step(proof: &[u8]) -> bool {
    let text = String::from_utf8_lossy(proof);
    text.lines().any(|l| l.contains(":rule trust") || l.contains(":rule hole") || l.contains("(trust "))
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
        Ok(accepted(status.and_then(|s| u32::try_from(s).ok()), &output))
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
