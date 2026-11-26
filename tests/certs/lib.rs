//! Certificates with the real tools: cvc5 as the proof producer and the
//! pinned checker Ethos. The tests are the `[[test]]` targets.
//!
//! They need the development shell: `DP_ETHOS`, `DP_CPC_SIGNATURE`, and
//! cvc5 in `DP_CREUSOT_HOME/bin`.

use std::path::PathBuf;

use dp_cert::Checker;

pub struct Tools {
    pub cvc5: PathBuf,
    pub checker: Checker,
    /// A directory for the files of the runs.
    pub tmp: PathBuf,
}

/// The tools, or `None` with a notice when the development shell is
/// missing; with `DP_REQUIRE_TOOLS` that is a failure.
pub fn tools(name: &str) -> Option<Tools> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    match (var("DP_CREUSOT_HOME"), var("DP_ETHOS"), var("DP_CPC_SIGNATURE")) {
        (Some(home), Some(ethos), Some(signature)) if home.join("bin/cvc5").exists() => {
            let tmp = std::env::temp_dir().join(format!("dp-certs-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::create_dir_all(&tmp).unwrap();
            Some(Tools { cvc5: home.join("bin/cvc5"), checker: Checker { ethos, signature }, tmp })
        }
        _ => {
            // In CI a skipped test would be a check that checked nothing.
            assert!(std::env::var_os("DP_REQUIRE_TOOLS").is_none(), "cvc5, DP_ETHOS or DP_CPC_SIGNATURE missing");
            eprintln!("cvc5, DP_ETHOS or DP_CPC_SIGNATURE missing: test skipped");
            None
        }
    }
}
