//! Result_Cache entries (R7.5 criterion 3; design: Data Models).
//!
//! An entry stores the lookup fields of its run description, its validation
//! fields, and the Result as accepted: the answer that the pinned driver
//! read, with the mark local or the Worker's signed Result. The Cache_Key is
//! the hash of the canonical S-expression of the lookup fields, whose atoms
//! are length-prefixed, so the encoding is injective.

use crate::{
    cluster::SignedResult,
    dispatch::DriverAnswer,
    hash::{self, Hash, TAG_CACHE},
    sexp::{self, Sexp},
};

/// One atom of an encoded run description, as dp-core's `encode_key` gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Atom {
    Bytes(Vec<u8>),
    Num(u64),
}

/// Where the stored Result came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The mark local: a run on the local machine.
    Local,
    /// A Worker's Result, with its fields and tag as accepted.
    Worker(SignedResult),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The lookup fields of the run description.
    pub lookup: Vec<Atom>,
    // Validation fields.
    pub binary_hash: Hash,
    /// The Calibration value, in millionths.
    pub velocity: u64,
    pub local_micros: u64,
    pub imports_hash: Hash,
    // The Result.
    pub answer: DriverAnswer,
    /// Time of the run, in reference microseconds.
    pub micros: u64,
    pub origin: Origin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not a Result_Cache entry: {}", self.0)
    }
}

fn err<T>(what: &str) -> Result<T, DecodeError> {
    Err(DecodeError(what.to_string()))
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

fn hash32(s: &Sexp) -> Result<Hash, DecodeError> {
    s.as_atom().and_then(|b| <[u8; 32]>::try_from(b).ok()).map(Hash).ok_or_else(|| DecodeError("hash".into()))
}

fn lookup_sexp(lookup: &[Atom]) -> Sexp {
    Sexp::list(lookup.iter().map(|a| match a {
        Atom::Bytes(b) => Sexp::list([Sexp::atom("b"), Sexp::atom(b)]),
        Atom::Num(n) => Sexp::list([Sexp::atom("n"), Sexp::atom(n.to_string())]),
    }))
}

/// `cache_key = H("dp/cache/v1", encode_key(lookup))`.
pub fn cache_key(lookup: &[Atom]) -> Hash {
    hash::h(TAG_CACHE, &[lookup_sexp(lookup)])
}

impl Entry {
    pub fn encode(&self) -> Vec<u8> {
        let n = |v: u64| Sexp::atom(v.to_string());
        sexp::print(&Sexp::list([
            Sexp::atom("cache-entry"),
            lookup_sexp(&self.lookup),
            Sexp::list([
                Sexp::atom(self.binary_hash.0),
                n(self.velocity),
                n(self.local_micros),
                Sexp::atom(self.imports_hash.0),
            ]),
            Sexp::list([Sexp::atom(self.answer.name()), n(self.micros)]),
            match &self.origin {
                Origin::Local => Sexp::list([Sexp::atom("local")]),
                Origin::Worker(signed) => {
                    Sexp::List(std::iter::once(Sexp::atom("worker")).chain(signed.encode()).collect())
                }
            },
        ]))
    }

    /// Parses an entry. Bytes that are not the canonical form of an entry
    /// are rejected (R7.10 criterion 8), and the caller treats the lookup as
    /// a miss (R7.5 criterion 5).
    pub fn decode(bytes: &[u8], limit: usize) -> Result<Entry, DecodeError> {
        let entry = Entry::decode_value(bytes, limit)?;
        crate::canonical_check::check(crate::canonical_check::CACHE_ENTRY, bytes, &entry.encode())
            .map_err(|e| DecodeError(e.to_string()))?;
        Ok(entry)
    }

    fn decode_value(bytes: &[u8], limit: usize) -> Result<Entry, DecodeError> {
        let s = sexp::parse(bytes, limit).map_err(|e| DecodeError(e.to_string()))?;
        let Some([tag, lookup, validation, result, origin]) = s.as_list() else {
            return err("entry");
        };
        if tag.as_str() != Some("cache-entry") {
            return err("entry");
        }
        let lookup = lookup
            .as_list()
            .ok_or_else(|| DecodeError("lookup".into()))?
            .iter()
            .map(|a| match a.as_list() {
                Some([k, v]) if k.as_str() == Some("b") => {
                    v.as_atom().map(|b| Atom::Bytes(b.to_vec())).ok_or_else(|| DecodeError("atom".into()))
                }
                Some([k, v]) if k.as_str() == Some("n") => Ok(Atom::Num(number(v)?)),
                _ => err("atom"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let Some([binary_hash, velocity, local_micros, imports_hash]) = validation.as_list() else {
            return err("validation");
        };
        let Some([answer, micros]) = result.as_list() else {
            return err("result");
        };
        let origin = match origin.as_list() {
            Some([tag]) if tag.as_str() == Some("local") => Origin::Local,
            Some([tag, rest @ ..]) if tag.as_str() == Some("worker") => Origin::Worker(
                SignedResult::decode(rest).map_err(|e| DecodeError(e.to_string()))?,
            ),
            _ => return err("origin"),
        };
        Ok(Entry {
            lookup,
            binary_hash: hash32(binary_hash)?,
            velocity: number(velocity)?,
            local_micros: number(local_micros)?,
            imports_hash: hash32(imports_hash)?,
            answer: DriverAnswer::decode(answer).map_err(|e| DecodeError(e.to_string()))?,
            micros: number(micros)?,
            origin,
        })
    }
}
