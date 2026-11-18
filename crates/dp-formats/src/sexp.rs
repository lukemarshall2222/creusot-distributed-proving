//! Canonical S-expressions (design decision D-7, R7.10 criteria 3 and 8).
//!
//! An atom is `<decimal length>:<bytes>` with no leading zeros in the length;
//! a list is `(` items `)`; there is no whitespace. Every value has exactly
//! one printed form, so for any accepted input `b`, `print(parse(b)) == b`.
//! The grammar is `docs/formats/canonical-sexp.abnf`.

use std::fmt;

/// A canonical S-expression.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Sexp {
    Atom(Vec<u8>),
    List(Vec<Sexp>),
}

impl Sexp {
    pub fn atom(bytes: impl AsRef<[u8]>) -> Sexp {
        Sexp::Atom(bytes.as_ref().to_vec())
    }

    pub fn list(items: impl IntoIterator<Item = Sexp>) -> Sexp {
        Sexp::List(items.into_iter().collect())
    }

    pub fn as_atom(&self) -> Option<&[u8]> {
        match self {
            Sexp::Atom(a) => Some(a),
            Sexp::List(_) => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Sexp]> {
        match self {
            Sexp::Atom(_) => None,
            Sexp::List(l) => Some(l),
        }
    }

    /// The atom as UTF-8 text, if it is an atom of valid UTF-8.
    pub fn as_str(&self) -> Option<&str> {
        self.as_atom().and_then(|a| std::str::from_utf8(a).ok())
    }
}

/// Why an input was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The input is longer than the size limit; nothing was parsed.
    TooLarge,
    /// The input ends inside a value.
    UnexpectedEnd,
    /// A byte that cannot start or continue a value at this position.
    UnexpectedByte,
    /// An atom length with a leading zero, which is not canonical.
    LeadingZero,
    /// An atom length that exceeds the size limit.
    LengthTooLarge,
    /// Lists nested deeper than [`MAX_DEPTH`].
    TooDeep,
    /// Bytes after the end of the value.
    TrailingBytes,
}

/// A rejection, with the byte offset at which it was detected (R7.10
/// criterion 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub offset: usize,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} at byte {}", self.kind, self.offset)
    }
}

impl std::error::Error for Error {}

/// Deepest list nesting that the parser accepts.
pub const MAX_DEPTH: usize = 64;

/// Prints the canonical form of `value`.
pub fn print(value: &Sexp) -> Vec<u8> {
    let mut out = Vec::new();
    print_into(value, &mut out);
    out
}

/// Appends the canonical form of `value` to `out`.
pub fn print_into(value: &Sexp, out: &mut Vec<u8>) {
    match value {
        Sexp::Atom(a) => {
            out.extend_from_slice(a.len().to_string().as_bytes());
            out.push(b':');
            out.extend_from_slice(a);
        }
        Sexp::List(items) => {
            out.push(b'(');
            for item in items {
                print_into(item, out);
            }
            out.push(b')');
        }
    }
}

/// Result of parsing the start of a stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prefix {
    /// One value and the number of bytes it occupies.
    Value(Sexp, usize),
    /// The bytes are a proper prefix of a value; more input is needed.
    Incomplete,
}

/// Parses exactly one value that spans all of `bytes`.
///
/// The size limit is checked before any parsing (R7.10 criterion 5).
pub fn parse(bytes: &[u8], limit: usize) -> Result<Sexp, Error> {
    if bytes.len() > limit {
        return Err(Error { kind: ErrorKind::TooLarge, offset: limit });
    }
    match parse_value(bytes, limit)? {
        Prefix::Incomplete => Err(Error { kind: ErrorKind::UnexpectedEnd, offset: bytes.len() }),
        Prefix::Value(_, used) if used < bytes.len() => {
            Err(Error { kind: ErrorKind::TrailingBytes, offset: used })
        }
        Prefix::Value(value, _) => Ok(value),
    }
}

/// Parses one value at the start of `bytes`, for reading a stream of
/// messages. A value longer than `limit` is rejected as soon as its length
/// is known or `bytes` holds more than `limit` bytes of it.
pub fn parse_prefix(bytes: &[u8], limit: usize) -> Result<Prefix, Error> {
    match parse_value(bytes, limit)? {
        Prefix::Incomplete if bytes.len() > limit => {
            Err(Error { kind: ErrorKind::TooLarge, offset: limit })
        }
        Prefix::Value(_, used) if used > limit => {
            Err(Error { kind: ErrorKind::TooLarge, offset: limit })
        }
        prefix => Ok(prefix),
    }
}

fn parse_value(bytes: &[u8], limit: usize) -> Result<Prefix, Error> {
    // Lists under construction; the parser uses no recursion, so hostile
    // nesting cannot exhaust the stack.
    let mut stack: Vec<Vec<Sexp>> = Vec::new();
    let mut pos = 0;
    loop {
        let Some(&byte) = bytes.get(pos) else {
            return Ok(Prefix::Incomplete);
        };
        let done = match byte {
            b'(' => {
                if stack.len() == MAX_DEPTH {
                    return Err(Error { kind: ErrorKind::TooDeep, offset: pos });
                }
                stack.push(Vec::new());
                pos += 1;
                None
            }
            b')' => {
                let Some(items) = stack.pop() else {
                    return Err(Error { kind: ErrorKind::UnexpectedByte, offset: pos });
                };
                pos += 1;
                Some(Sexp::List(items))
            }
            b'0'..=b'9' => {
                let start = pos;
                let mut len: usize = 0;
                loop {
                    match bytes.get(pos) {
                        None => return Ok(Prefix::Incomplete),
                        Some(d @ b'0'..=b'9') => {
                            if pos > start && bytes[start] == b'0' {
                                return Err(Error { kind: ErrorKind::LeadingZero, offset: start });
                            }
                            len = len
                                .checked_mul(10)
                                .and_then(|l| l.checked_add(usize::from(d - b'0')))
                                .filter(|l| *l <= limit)
                                .ok_or(Error { kind: ErrorKind::LengthTooLarge, offset: start })?;
                            pos += 1;
                        }
                        Some(b':') => break,
                        Some(_) => {
                            return Err(Error { kind: ErrorKind::UnexpectedByte, offset: pos });
                        }
                    }
                }
                pos += 1;
                let Some(atom) = bytes.get(pos..pos + len) else {
                    return Ok(Prefix::Incomplete);
                };
                pos += len;
                Some(Sexp::Atom(atom.to_vec()))
            }
            _ => return Err(Error { kind: ErrorKind::UnexpectedByte, offset: pos }),
        };
        if let Some(value) = done {
            match stack.last_mut() {
                Some(parent) => parent.push(value),
                None => return Ok(Prefix::Value(value, pos)),
            }
        }
    }
}

/// A readable rendering for logs and error messages. It is not canonical
/// and is never parsed.
pub fn pretty(value: &Sexp) -> String {
    let mut out = String::new();
    pretty_into(value, &mut out);
    out
}

fn pretty_into(value: &Sexp, out: &mut String) {
    match value {
        Sexp::Atom(a) => {
            let plain = !a.is_empty()
                && a.len() <= 64
                && a.iter().all(|b| b.is_ascii_graphic() && !matches!(b, b'(' | b')' | b'"'));
            if plain {
                out.push_str(std::str::from_utf8(a).expect("ASCII"));
            } else if a.len() > 64 {
                out.push_str(&format!("<{} bytes>", a.len()));
            } else {
                out.push_str(&format!("{:?}", String::from_utf8_lossy(a)));
            }
        }
        Sexp::List(items) => {
            out.push('(');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                pretty_into(item, out);
            }
            out.push(')');
        }
    }
}
