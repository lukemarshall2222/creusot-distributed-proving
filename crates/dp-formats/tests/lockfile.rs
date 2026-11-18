use dp_formats::lockfile::{Kind, LockFile};

const H: &str = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn tool(extra: &str, hash: &str) -> String {
    format!(
        "[[tool]]\nname = \"cvc5\"\nversion = \"1.4.1\"\nkind = \"unpatched-solver\"\nprover = \"cvc5\"\n{extra}\n[[tool.platform]]\nplatform = \"x86_64-linux\"\nhash = \"{hash}\"\nupstream = {{ tag = \"cvc5-1.4.1\", rebuild_hashes = [] }}\n"
    )
}

#[test]
fn a_well_formed_lock_file_parses() {
    let lock = LockFile::parse(&tool("", H)).unwrap();
    let pins: Vec<_> = lock.pins(Kind::UnpatchedSolver, "x86_64-linux").collect();
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].1.to_hex(), &H[7..]);
    assert_eq!(lock.pins(Kind::UnpatchedSolver, "aarch64-darwin").count(), 0);
    assert_eq!(lock.pins(Kind::Checker, "x86_64-linux").count(), 0);
}

#[test]
fn malformed_entries_are_rejected() {
    assert!(LockFile::parse(&tool("", "sha256:abc")).is_err());
    assert!(LockFile::parse(&tool("", &H.to_uppercase())).is_err());
    assert!(LockFile::parse(&tool("", &H[7..])).is_err());
    assert!(LockFile::parse(&tool("surprise = 1", H)).is_err());
    assert!(LockFile::parse(&tool("", H).replace("1.4.1\"\nkind", "\"\nkind").replace("version = \"1.4.1", "version = \"")).is_err());
    assert!(LockFile::parse(&tool("", H).replace("unpatched-solver", "magic")).is_err());
    assert!(LockFile::parse(&tool("", H).replace("prover = \"cvc5\"\n", "")).is_err());
}

#[test]
fn a_patched_build_cannot_be_an_unpatched_solver() {
    assert!(LockFile::parse(&tool("patches = [\"patches/cvc5/0001.patch\"]", H)).is_err());
    assert!(LockFile::parse(
        &tool("patches = [\"patches/cvc5/0001.patch\"]", H).replace("unpatched-solver", "sharing-solver")
    )
    .is_ok());
}
