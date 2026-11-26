//! The configuration files that a Session reads before it starts
//! anything: `dp.toml` (R2.1) and the Lock_File (R7.7).
//!
//! Parsing must not panic. A Lock_File that parses has no patched build
//! marked as an Unpatched_Solver and no solver without a prover name, and
//! every hash that it pins can be read.

use dp_formats::{
    dptoml::{cluster_address, ProjectConfig},
    lockfile::{Kind, LockFile},
};

pub fn run(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let _ = ProjectConfig::parse(text);
    if let Some(address) = cluster_address(text) {
        assert!(address.contains(':'), "an address without a port: {address:?}");
    }
    if let Ok(lock) = LockFile::parse(text) {
        for tool in &lock.tool {
            assert!(!tool.version.is_empty());
            assert!(tool.kind != Kind::UnpatchedSolver || tool.patches.is_empty(), "a patched Unpatched_Solver");
            if matches!(tool.kind, Kind::UnpatchedSolver | Kind::SharingSolver) {
                assert!(tool.prover.is_some());
            }
        }
        for kind in [Kind::UnpatchedSolver, Kind::Checker, Kind::Why3find] {
            for platform in ["aarch64-darwin", "x86_64-linux"] {
                // Reads every pinned hash; a malformed one must have been
                // rejected by `parse`.
                let _ = lock.pins(kind, platform).count();
            }
        }
    }
}

pub fn seeds() -> Vec<Vec<u8>> {
    let config = "[cluster]\naddress = \"tcp://host:5555\"\nregistry = \"registry\"\noperator_key = \"00\"\nportfolio_seeds = 2\n\n\
        [local]\njobs = 4\n\n[tracing]\nfile = \"t\"\nretention = \"complete\"\n\n\
        [certification]\nenabled = true\ncertify_all = false\ncheck_seconds = 60.0\nproof_megabytes = 16\n\n\
        [[solver]]\nname = \"a\"\nprover = \"z3\"\nseed = 3\n\n[portfolio]\nrace = [\"a\"]\nfirst_pass = \"cluster\"\nlocal_member = true\n";
    let lock = format!(
        "[[tool]]\nname = \"z3\"\nversion = \"4.13\"\nkind = \"unpatched-solver\"\nprover = \"z3\"\n\n\
         [[tool.platform]]\nplatform = \"aarch64-darwin\"\nhash = \"sha256:{}\"\nupstream = {{ tag = \"z3-4.13\", rebuild_hashes = [] }}\n\n\
         [[tool]]\nname = \"w\"\nversion = \"1\"\nkind = \"why3find\"\npatches = [\"patches/w/1.patch\"]\n\n\
         [[tool.platform]]\nplatform = \"x86_64-linux\"\nhash = \"sha256:{}\"\n",
        "ab".repeat(32),
        "cd".repeat(32)
    );
    vec![config.as_bytes().to_vec(), lock.into_bytes(), b"tcp://host:5555".to_vec(), b"host:1".to_vec()]
}
