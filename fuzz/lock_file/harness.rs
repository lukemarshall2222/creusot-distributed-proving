//! The Lock_File (R7.7): the file that says which binaries a Session may
//! believe.
//!
//! - A text that `parse` accepts is the canonical print of what it reads
//!   (R7.10 criterion 8), and reads back to the same pins.
//! - What it accepts keeps the rules of the pins: a version, well-formed
//!   hashes, a prover name for a solver, and no patched build as an
//!   Unpatched_Solver.
//! - The lenient reader never accepts less than `parse`, and what it
//!   accepts prints to a text that `parse` accepts.

use dp_formats::lockfile::{Kind, LockFile, HEADER};

pub fn run(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let strict = LockFile::parse(text);
    let lenient = LockFile::parse_lenient(text);
    if let Ok(lock) = &strict {
        assert_eq!(lock.print(), text, "a Lock_File in another layout was accepted");
        assert_eq!(lenient.as_ref().ok(), Some(lock), "the lenient reader refuses a canonical Lock_File");
        for tool in &lock.tool {
            assert!(!tool.version.is_empty());
            assert!(tool.kind != Kind::UnpatchedSolver || tool.patches.is_empty(), "a patched Unpatched_Solver");
            assert!(!matches!(tool.kind, Kind::UnpatchedSolver | Kind::SharingSolver) || tool.prover.is_some());
        }
        for kind in [Kind::UnpatchedSolver, Kind::Checker, Kind::Why3find, Kind::Library] {
            for platform in ["aarch64-darwin", "x86_64-linux"] {
                let _ = lock.pins(kind, platform).count();
            }
        }
    }
    if let Ok(lock) = &lenient {
        let printed = lock.print();
        assert!(printed.starts_with(HEADER));
        assert_eq!(LockFile::parse(&printed).as_ref().ok(), Some(lock), "the print of a Lock_File is refused");
    }
}

pub fn seeds() -> Vec<Vec<u8>> {
    let h = format!("sha256:{}", "ab".repeat(32));
    let full = format!(
        "{HEADER}\n[[tool]]\nname = \"z3\"\nversion = \"1\"\nkind = \"unpatched-solver\"\nprover = \"z3\"\n\n\
[[tool.platform]]\nplatform = \"x86_64-linux\"\nhash = \"{h}\"\nupstream = {{ published_checksum = \"{h}\", tag = \"z3-1\", rebuild_hashes = [\"{h}\", \"{h}\"] }}\n\n\
[[tool.platform]]\nplatform = \"aarch64-darwin\"\nhash = \"{h}\"\nupstream = {{ tag = \"z3-1\", rebuild_hashes = [] }}\n\n\
[[tool]]\nname = \"dep\"\nversion = \"2\"\nkind = \"why3find\"\npatches = [\"patches/dep/0001.patch\"]\n\n\
[[tool.platform]]\nplatform = \"x86_64-linux\"\nhash = \"{h}\"\n\n\
[[tool]]\nname = \"later\"\nversion = \"3\"\nkind = \"lean\"\n"
    );
    vec![
        HEADER.as_bytes().to_vec(),
        full.clone().into_bytes(),
        // Another layout of the same pins, and pins that break a rule.
        full.replace("name = ", "name=").into_bytes(),
        full.replace("kind = \"why3find\"", "kind = \"unpatched-solver\"\nprover = \"dep\"").into_bytes(),
        full.replace("prover = \"z3\"\n", "").into_bytes(),
    ]
}
