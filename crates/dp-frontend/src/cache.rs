//! The Result_Cache directory `.dp-cache/` (R7.5; design C15).
//!
//! An entry is a file named by its Cache_Key, the hash of the lookup
//! fields of a run. The directory is not trusted to hold what its names
//! say: a file is an entry for a lookup only if it decodes and its stored
//! lookup fields are those of the lookup (criterion 5). Whether the entry
//! may answer in this Session, and with which label, dp-core decides
//! (`validate_entry`, `cache_label`) from facts that the Session computes.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use dp_core::cache_key;
use dp_formats::cache::{self, Entry};
use dp_shell::store;

/// Size limit of one entry. A working value, not a proposal for `<TBD-22>`.
pub const ENTRY_LIMIT: usize = 1 << 24;

pub fn formats_atoms(atoms: &[cache_key::Atom]) -> Vec<cache::Atom> {
    atoms
        .iter()
        .map(|a| match a {
            cache_key::Atom::Bytes(b) => cache::Atom::Bytes(b.clone()),
            cache_key::Atom::Num(n) => cache::Atom::Num(*n),
        })
        .collect()
}

pub fn core_atoms(atoms: &[cache::Atom]) -> Vec<cache_key::Atom> {
    atoms
        .iter()
        .map(|a| match a {
            cache::Atom::Bytes(b) => cache_key::Atom::Bytes(b.clone()),
            cache::Atom::Num(n) => cache_key::Atom::Num(*n),
        })
        .collect()
}

/// The file of the entry with these lookup fields in the project `dir`.
pub fn file(dir: &Path, lookup: &[cache::Atom]) -> PathBuf {
    dir.join(".dp-cache").join(cache::cache_key(lookup).to_hex())
}

/// What the directory holds for a lookup.
#[derive(Debug, PartialEq)]
pub enum Read {
    /// No file under the key.
    Absent,
    /// A file that is no entry for this lookup, with the reason.
    Miss(PathBuf, String),
    Entry(Entry),
}

/// Reads the entry for the encoded run description `wanted`.
pub fn read(dir: &Path, wanted: &[cache_key::Atom]) -> Read {
    let path = file(dir, &formats_atoms(wanted));
    let Ok(bytes) = fs::read(&path) else { return Read::Absent };
    let entry = match Entry::decode(&bytes, ENTRY_LIMIT) {
        Ok(entry) => entry,
        Err(e) => return Read::Miss(path, e.to_string()),
    };
    // The stored lookup fields themselves, not only the key (criterion 5).
    if !cache_key::lookup_ok(&core_atoms(&entry.lookup), &wanted.to_vec()) {
        return Read::Miss(path, "its lookup fields are not those of the lookup".to_string());
    }
    Read::Entry(entry)
}

/// Stores an entry under the key of its lookup fields, atomically.
pub fn write(dir: &Path, entry: &Entry) -> io::Result<()> {
    store::write_atomic(&file(dir, &entry.lookup), &entry.encode())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dp_formats::{cache::Origin, dispatch::DriverAnswer, hash::Hash};

    fn entry(lookup: Vec<cache_key::Atom>) -> Entry {
        Entry {
            lookup: formats_atoms(&lookup),
            binary_hash: Hash([2; 32]),
            velocity: 1_000_000,
            local_micros: 5,
            imports_hash: Hash([3; 32]),
            answer: DriverAnswer::Valid,
            micros: 10,
            origin: Origin::Local,
        }
    }

    #[test]
    fn an_entry_answers_only_the_lookup_of_its_own_fields() {
        let dir = std::env::temp_dir().join(format!("dp-cache-unit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let a = vec![cache_key::Atom::Bytes(b"goal".to_vec()), cache_key::Atom::Num(1)];
        let b = vec![cache_key::Atom::Bytes(b"goal".to_vec()), cache_key::Atom::Num(2)];
        assert_eq!(read(&dir, &a), Read::Absent);
        write(&dir, &entry(a.clone())).unwrap();
        assert_eq!(read(&dir, &a), Read::Entry(entry(a.clone())));
        assert_eq!(read(&dir, &b), Read::Absent);
        // The entry of `a` under the key of `b`: the stored fields decide.
        fs::copy(file(&dir, &formats_atoms(&a)), file(&dir, &formats_atoms(&b))).unwrap();
        assert!(matches!(read(&dir, &b), Read::Miss(_, reason) if reason.contains("lookup fields")));
        // Bytes that are no entry.
        fs::write(file(&dir, &formats_atoms(&a)), b"(not an entry").unwrap();
        assert!(matches!(read(&dir, &a), Read::Miss(..)));
        fs::remove_dir_all(dir).unwrap();
    }
}
