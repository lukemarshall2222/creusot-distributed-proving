//! The revocations that a project has seen (R1.7 criterion 6), kept in
//! `.dp-cache/revocations` so that a later Session refuses the Results of a
//! revoked Worker without hearing of the revocation again: from the
//! Result_Cache, and from a Coordinator that has forgotten it.
//!
//! The file is not trusted. Each entry is the operator's signed command,
//! and only entries whose signature verifies under the operator's key of
//! the current Session count. Removing the file loses revocations, as
//! removing the registry would; it cannot add one.

use std::{collections::BTreeSet, io, path::Path};

use dp_formats::{
    cluster::{Operation, SignedCommand},
    sexp::{self, Sexp},
};
use dp_shell::store;
use ed25519_dalek::VerifyingKey;

fn file(dir: &Path) -> std::path::PathBuf {
    dir.join(".dp-cache").join("revocations")
}

fn read(dir: &Path) -> Vec<SignedCommand> {
    let Ok(bytes) = std::fs::read(file(dir)) else { return Vec::new() };
    let Ok(parsed) = sexp::parse(&bytes, 1 << 24) else { return Vec::new() };
    let Some([tag, entries @ ..]) = parsed.as_list() else { return Vec::new() };
    if tag.as_str() != Some("revocations") {
        return Vec::new();
    }
    entries.iter().filter_map(|e| SignedCommand::decode(e.as_list()?).ok()).collect()
}

/// The Workers that stored revocations revoke, as far as the operator's
/// key confirms them.
pub fn load(dir: &Path, operator: &VerifyingKey) -> BTreeSet<String> {
    read(dir)
        .into_iter()
        .filter(|c| c.operation == Operation::Revoke && c.verifies(operator))
        .map(|c| c.worker)
        .collect()
}

/// Adds a revocation to the file.
pub fn add(dir: &Path, command: &SignedCommand) -> io::Result<()> {
    let mut entries = read(dir);
    if entries.contains(command) {
        return Ok(());
    }
    entries.push(command.clone());
    let list = std::iter::once(Sexp::atom("revocations")).chain(entries.iter().map(|c| Sexp::List(c.encode())));
    store::write_atomic(&file(dir), &sexp::print(&Sexp::List(list.collect())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    // P-ID: P1.7.6
    #[test]
    fn stored_revocations_count_only_with_the_operators_signature() {
        let dir = std::env::temp_dir().join(format!("dp-revocations-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let operator = SigningKey::from_bytes(&[1; 32]);
        let stranger = SigningKey::from_bytes(&[2; 32]);
        assert!(load(&dir, &operator.verifying_key()).is_empty());
        add(&dir, &SignedCommand::sign(1, Operation::Revoke, "w1", &operator)).unwrap();
        add(&dir, &SignedCommand::sign(2, Operation::Revoke, "w2", &stranger)).unwrap();
        // not a revocation, though the operator signed it
        add(&dir, &SignedCommand::sign(3, Operation::Drain, "w3", &operator)).unwrap();
        add(&dir, &SignedCommand::sign(1, Operation::Revoke, "w1", &operator)).unwrap();
        let revoked = load(&dir, &operator.verifying_key());
        assert_eq!(revoked.into_iter().collect::<Vec<_>>(), vec!["w1".to_string()]);
        // under another operator key nothing of it counts
        assert_eq!(load(&dir, &stranger.verifying_key()).into_iter().collect::<Vec<_>>(), vec!["w2".to_string()]);
        // a damaged file adds nothing
        std::fs::write(file(&dir), b"(11:revocations").unwrap();
        assert!(load(&dir, &operator.verifying_key()).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
