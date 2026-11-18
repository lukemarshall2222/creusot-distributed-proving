//! The `proof.json` extension (R2.3, R7.2; design: Data Models).
//!
//! Each Goal entry of a proof file gets one member `"dp"`: a JSON string
//! holding the base64 (RFC 4648, padded) encoding of the canonical
//! S-expression of a [`DpEntry`]. The string is opaque to a legacy writer,
//! and the canonical-input check has exact bytes to check (R7.10
//! criterion 8).
//!
//! As built: the label, the replay fields of the run that proved the Goal,
//! the Invalid field (the runs on the Goal that answered Invalid, R7.2
//! criterion 11), the conflict field (set when a Goal has a proof label and
//! an Invalid answer, R7.2 criterion 7), and the import list, which is
//! empty, and for the label certified the content hash of the certificate's
//! entry in the Certificate_Store. The Partition tree hash is always the
//! atom `absent`; it is in the encoding so that adding it does not change
//! the shape.

use std::path::{Path, PathBuf};

use crate::{
    hash::Hash,
    sexp::{self, Sexp},
};

/// The labels of R7.2, by their names in `proof.json` and in reports.
pub const LABELS: [&str; 11] = [
    "proved-standalone",
    "certified",
    "invalid",
    "checked-incomplete",
    "unchecked",
    "timeout",
    "unknown",
    "failed",
    "unresolved",
    // Reserved for Lemma_Dependent_Results and Partitions.
    "proved-with-lemmas",
    "partition-proved",
];

/// Where the run of a replay entry took place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    Local,
    Worker(String),
    /// The Result came from the Result_Cache; the entry says where it ran.
    Absent,
}

/// What a replay needs to run the proving run again (R2.3 criterion 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayFields {
    pub task_hash: Hash,
    /// The Driver, as why3find's digest of the prover configuration.
    pub driver: Vec<u8>,
    pub prover: String,
    pub version: String,
    pub binary_hash: Hash,
    pub place: Place,
    /// The solver seed of a Race member, if it had one.
    pub seed: Option<u64>,
    /// The Calibration value in millionths, if the run was calibrated.
    pub velocity: Option<u64>,
    pub local_micros: Option<u64>,
    pub reference_micros: u64,
}

/// A run that answered Invalid on the Goal (R7.2 criterion 11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidEntry {
    pub prover: String,
    pub version: String,
    pub binary_hash: Hash,
    pub place: Place,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DpEntry {
    pub label: String,
    /// One entry for a proof label, none otherwise.
    pub replay: Vec<ReplayFields>,
    pub imports_hash: Hash,
    /// For the label certified: the content hash of the entry of the
    /// Certificate_Store that holds the certificate (R5.4 criterion 2).
    pub certificate: Option<Hash>,
    /// The Goal has a proof label and an Invalid answer of another run
    /// (R7.2 criterion 7); the runs are in `invalid`.
    pub conflict: bool,
    pub invalid: Vec<InvalidEntry>,
}

fn place_sexp(place: &Place) -> Sexp {
    match place {
        Place::Local => Sexp::list([Sexp::atom("local")]),
        Place::Worker(name) => Sexp::list([Sexp::atom("worker"), Sexp::atom(name)]),
        Place::Absent => Sexp::atom(ABSENT),
    }
}

fn place_of(place: &Sexp) -> Result<Place, DecodeError> {
    match place.as_list() {
        Some([tag]) if tag.as_str() == Some("local") => Ok(Place::Local),
        Some([tag, name]) if tag.as_str() == Some("worker") => Ok(Place::Worker(text(name)?)),
        None if place.as_str() == Some(ABSENT) => Ok(Place::Absent),
        _ => err("place"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not a dp member of proof.json: {}", self.0)
    }
}

fn err<T>(what: &str) -> Result<T, DecodeError> {
    Err(DecodeError(what.to_string()))
}

const ABSENT: &str = "absent";

fn opt_num(v: Option<u64>) -> Sexp {
    match v {
        Some(v) => Sexp::list([Sexp::atom(v.to_string())]),
        None => Sexp::atom(ABSENT),
    }
}

fn number(s: &Sexp) -> Result<u64, DecodeError> {
    let t = s.as_str().ok_or_else(|| DecodeError("number".into()))?;
    let canonical =
        !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) && (t == "0" || !t.starts_with('0'));
    match t.parse() {
        Ok(v) if canonical => Ok(v),
        _ => err("number"),
    }
}

fn opt_number(s: &Sexp) -> Result<Option<u64>, DecodeError> {
    match s.as_list() {
        Some([v]) => Ok(Some(number(v)?)),
        None if s.as_str() == Some(ABSENT) => Ok(None),
        _ => err("optional number"),
    }
}

fn hash32(s: &Sexp) -> Result<Hash, DecodeError> {
    s.as_atom().and_then(|b| <[u8; 32]>::try_from(b).ok()).map(Hash).ok_or_else(|| DecodeError("hash".into()))
}

fn text(s: &Sexp) -> Result<String, DecodeError> {
    s.as_str().map(str::to_string).ok_or_else(|| DecodeError("text".into()))
}

impl ReplayFields {
    fn sexp(&self) -> Sexp {
        Sexp::list([
            Sexp::atom(self.task_hash.0),
            Sexp::atom(&self.driver),
            Sexp::atom(&self.prover),
            Sexp::atom(&self.version),
            Sexp::atom(self.binary_hash.0),
            place_sexp(&self.place),
            opt_num(self.seed),
            opt_num(self.velocity),
            opt_num(self.local_micros),
            Sexp::atom(self.reference_micros.to_string()),
        ])
    }

    fn from_sexp(s: &Sexp) -> Result<ReplayFields, DecodeError> {
        let Some([task_hash, driver, prover, version, binary_hash, place, seed, velocity, local, reference]) =
            s.as_list()
        else {
            return err("replay fields");
        };
        let place = place_of(place)?;
        Ok(ReplayFields {
            task_hash: hash32(task_hash)?,
            driver: driver.as_atom().ok_or_else(|| DecodeError("driver".into()))?.to_vec(),
            prover: text(prover)?,
            version: text(version)?,
            binary_hash: hash32(binary_hash)?,
            place,
            seed: opt_number(seed)?,
            velocity: opt_number(velocity)?,
            local_micros: opt_number(local)?,
            reference_micros: number(reference)?,
        })
    }
}

impl DpEntry {
    /// The canonical S-expression of the entry.
    pub fn encode(&self) -> Vec<u8> {
        sexp::print(&Sexp::list([
            Sexp::atom("dp-entry"),
            Sexp::atom(&self.label),
            Sexp::list(self.replay.iter().map(ReplayFields::sexp)),
            Sexp::list([]), // imports
            Sexp::atom(self.imports_hash.0),
            match self.certificate {
                Some(content) => Sexp::list([Sexp::atom(content.0)]),
                None => Sexp::atom(ABSENT),
            },
            Sexp::atom(ABSENT), // Partition tree hash
            if self.conflict { Sexp::list([Sexp::atom("conflict")]) } else { Sexp::atom(ABSENT) },
            Sexp::list(self.invalid.iter().map(|i| {
                Sexp::list([
                    Sexp::atom(&i.prover),
                    Sexp::atom(&i.version),
                    Sexp::atom(i.binary_hash.0),
                    place_sexp(&i.place),
                ])
            })),
        ]))
    }

    pub fn decode(bytes: &[u8], limit: usize) -> Result<DpEntry, DecodeError> {
        let s = sexp::parse(bytes, limit).map_err(|e| DecodeError(e.to_string()))?;
        let Some([tag, label, replay, imports, imports_hash, certificate, tree, conflict, invalid]) =
            s.as_list()
        else {
            return err("entry");
        };
        if tag.as_str() != Some("dp-entry") {
            return err("entry");
        }
        let label = text(label)?;
        if !LABELS.contains(&label.as_str()) {
            return err("label");
        }
        // Members that this build never writes: anything else in their
        // place is not an entry that it can read faithfully.
        let absent = |s: &Sexp| s.as_str() == Some(ABSENT);
        let empty = |s: &Sexp| s.as_list().is_some_and(<[Sexp]>::is_empty);
        if !empty(imports) || !absent(tree) {
            return err("a member that this build does not read");
        }
        let certificate = match certificate.as_list() {
            Some([content]) => Some(hash32(content)?),
            None if absent(certificate) => None,
            _ => return err("certificate"),
        };
        let conflict = match conflict.as_list() {
            Some([tag]) if tag.as_str() == Some("conflict") => true,
            None if absent(conflict) => false,
            _ => return err("conflict"),
        };
        let invalid = invalid
            .as_list()
            .ok_or_else(|| DecodeError("invalid".into()))?
            .iter()
            .map(|i| match i.as_list() {
                Some([prover, version, binary_hash, place]) => Ok(InvalidEntry {
                    prover: text(prover)?,
                    version: text(version)?,
                    binary_hash: hash32(binary_hash)?,
                    place: place_of(place)?,
                }),
                _ => err("invalid entry"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let replay = replay
            .as_list()
            .ok_or_else(|| DecodeError("replay".into()))?
            .iter()
            .map(ReplayFields::from_sexp)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(DpEntry { label, replay, imports_hash: hash32(imports_hash)?, certificate, conflict, invalid })
    }

    /// The value of the member `"dp"`.
    pub fn to_member(&self) -> String {
        base64_encode(&self.encode())
    }

    /// Reads the value of a member `"dp"`. A string that is not the base64
    /// of the canonical form of an entry is rejected.
    pub fn from_member(member: &str, limit: usize) -> Result<DpEntry, DecodeError> {
        let bytes = base64_decode(member).ok_or_else(|| DecodeError("base64".into()))?;
        let entry = DpEntry::decode(&bytes, limit)?;
        if entry.to_member() != member {
            return err("not canonical");
        }
        Ok(entry)
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// RFC 4648 base64 with padding.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        let digit = |shift: u32| ALPHABET[(n >> shift & 63) as usize] as char;
        out.push(digit(18));
        out.push(digit(12));
        out.push(if chunk.len() > 1 { digit(6) } else { '=' });
        out.push(if chunk.len() > 2 { digit(0) } else { '=' });
    }
    out
}

/// Decodes RFC 4648 base64 with padding. Only the encoding that
/// [`base64_encode`] gives is accepted: no white space, no missing padding,
/// no non-zero bits after the last byte.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 4 != 0 {
        return None;
    }
    let value = |c: u8| ALPHABET.iter().position(|a| *a == c).map(|p| p as u32);
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let groups = bytes.len() / 4;
    for (i, group) in bytes.chunks(4).enumerate() {
        let padding = group.iter().rev().take_while(|c| **c == b'=').count();
        if padding > 2 || (padding > 0 && i + 1 != groups) {
            return None;
        }
        let mut n = 0u32;
        for c in &group[..4 - padding] {
            n = n << 6 | value(*c)?;
        }
        n <<= 6 * padding as u32;
        let decoded = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        let keep = 3 - padding;
        if decoded[keep..].iter().any(|b| *b != 0) {
            return None;
        }
        out.extend_from_slice(&decoded[..keep]);
    }
    Some(out)
}

/// The proof file of a source file, as why3find names it: the path of the
/// file below the project directory, without its extension and with dots
/// between its parts, in `why3find.proofs`.
pub fn proof_file(dir: &Path, file: &str) -> PathBuf {
    let file = Path::new(file).with_extension("");
    let absolute = if file.is_absolute() { file } else { dir.join(file) };
    let relative = absolute.strip_prefix(dir).unwrap_or(&absolute).to_path_buf();
    let parts: Vec<_> = relative
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    dir.join("why3find.proofs").join(format!("{}.json", parts.join(".")))
}

