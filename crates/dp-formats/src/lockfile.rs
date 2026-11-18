//! The Lock_File `deps/lock.toml` (design: Data Models, R7.7).
//!
//! The canonical-input check of R7.10 criterion 8 is not applied yet; the
//! parser rejects unknown fields and unknown tool kinds.

use serde::Deserialize;

use crate::hash::Hash;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockFile {
    #[serde(default)]
    pub tool: Vec<Tool>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    UnpatchedSolver,
    SharingSolver,
    Checker,
    CheckerSignature,
    AletheRuleList,
    Why3,
    Why3find,
    Creusot,
    Lean,
    Tla,
    Kani,
    CargoMutants,
    Toolchain,
    Library,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub name: String,
    pub version: String,
    pub kind: Kind,
    /// The prover name, for solvers.
    pub prover: Option<String>,
    /// Paths under `patches/` that the build applies.
    #[serde(default)]
    pub patches: Vec<String>,
    #[serde(default)]
    pub platform: Vec<Platform>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Platform {
    pub platform: String,
    /// `sha256:<64 hex digits>`.
    pub hash: String,
    pub upstream: Option<Upstream>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Upstream {
    pub published_checksum: Option<String>,
    pub tag: Option<String>,
    #[serde(default)]
    pub rebuild_hashes: Vec<String>,
}

#[derive(Debug)]
pub struct LockError(pub String);

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Lock_File: {}", self.0)
    }
}

impl std::error::Error for LockError {}

fn parse_hash(text: &str) -> Option<Hash> {
    let hex = text.strip_prefix("sha256:")?;
    if hex.len() != 64 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(Hash(out))
}

impl LockFile {
    /// Parses a Lock_File. Every entry needs a version and well-formed
    /// content hashes (R7.7 criterion 6); a patched build may not be an
    /// Unpatched_Solver (R7.7 criterion 8).
    pub fn parse(text: &str) -> Result<LockFile, LockError> {
        let lock: LockFile = toml::from_str(text).map_err(|e| LockError(e.to_string()))?;
        for tool in &lock.tool {
            if tool.version.is_empty() {
                return Err(LockError(format!("{}: missing version", tool.name)));
            }
            if tool.kind == Kind::UnpatchedSolver && !tool.patches.is_empty() {
                return Err(LockError(format!("{}: a patched build marked unpatched-solver", tool.name)));
            }
            if matches!(tool.kind, Kind::UnpatchedSolver | Kind::SharingSolver) && tool.prover.is_none() {
                return Err(LockError(format!("{}: solver without a prover name", tool.name)));
            }
            for platform in &tool.platform {
                if parse_hash(&platform.hash).is_none() {
                    return Err(LockError(format!(
                        "{} ({}): malformed hash {:?}",
                        tool.name, platform.platform, platform.hash
                    )));
                }
            }
        }
        Ok(lock)
    }

    /// The hashes that the Lock_File pins on `platform` for tools of `kind`,
    /// with the tool.
    pub fn pins<'a>(
        &'a self,
        kind: Kind,
        platform: &'a str,
    ) -> impl Iterator<Item = (&'a Tool, Hash)> + 'a {
        self.tool.iter().filter(move |t| t.kind == kind).flat_map(move |t| {
            t.platform
                .iter()
                .filter(move |p| p.platform == platform)
                .map(move |p| (t, parse_hash(&p.hash).expect("checked by parse")))
        })
    }
}

/// The platform name of this build, as the Lock_File spells it.
pub fn current_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    format!("{}-{}", std::env::consts::ARCH, os)
}
