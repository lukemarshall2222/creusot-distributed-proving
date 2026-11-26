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

/// The width that `pretty` keeps a line within where it can.
const PRETTY_WIDTH: usize = 100;

/// A rendering for people: in logs, in error messages and in the files
/// that show what a format looks like. It is not the canonical form, and
/// nothing that is signed, hashed or exchanged is in it; but it loses
/// nothing, and `parse_pretty` reads it back to the same value.
///
/// - An atom of printable ASCII without white space, parentheses, quotes
///   or a backslash, that does not start with `#`, is written as it is.
/// - Any other atom that is text (UTF-8 without control characters) is
///   written between double quotes, with `\"` and `\\` for those two
///   characters.
/// - Every other atom, and the empty one, is written as `#` and the
///   hexadecimal digits of its bytes.
/// - A list is written on one line if it fits, and otherwise with its
///   first item after the parenthesis and every further item on a line of
///   its own, two columns in.
pub fn pretty(value: &Sexp) -> String {
    let mut out = String::new();
    pretty_into(value, 0, &mut out);
    out
}

fn pretty_atom(a: &[u8], out: &mut String) {
    let bare = !a.is_empty()
        && a[0] != b'#'
        && a.iter().all(|b| b.is_ascii_graphic() && !matches!(b, b'(' | b')' | b'"' | b'\\'));
    if bare {
        out.push_str(std::str::from_utf8(a).expect("ASCII"));
        return;
    }
    match std::str::from_utf8(a) {
        Ok(text) if !a.is_empty() && !text.chars().any(char::is_control) => {
            out.push('"');
            for c in text.chars() {
                if matches!(c, '"' | '\\') {
                    out.push('\\');
                }
                out.push(c);
            }
            out.push('"');
        }
        _ => {
            out.push('#');
            for byte in a {
                out.push_str(&format!("{byte:02x}"));
            }
        }
    }
}

/// The value on one line.
fn pretty_flat(value: &Sexp, out: &mut String) {
    match value {
        Sexp::Atom(a) => pretty_atom(a, out),
        Sexp::List(items) => {
            out.push('(');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                pretty_flat(item, out);
            }
            out.push(')');
        }
    }
}

fn pretty_into(value: &Sexp, indent: usize, out: &mut String) {
    let mut flat = String::new();
    pretty_flat(value, &mut flat);
    let Sexp::List(items) = value else {
        out.push_str(&flat);
        return;
    };
    if indent + flat.chars().count() <= PRETTY_WIDTH || items.len() < 2 && !matches!(items.first(), Some(Sexp::List(_))) {
        out.push_str(&flat);
        return;
    }
    out.push('(');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(&" ".repeat(indent + 2));
        }
        pretty_into(item, indent + if i == 0 { 1 } else { 2 }, out);
    }
    out.push(')');
}

/// Why a pretty-printed text was not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrettyError {
    pub what: &'static str,
    /// The byte offset in the text.
    pub offset: usize,
}

impl fmt::Display for PrettyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.what, self.offset)
    }
}

impl std::error::Error for PrettyError {}

/// Reads what `pretty` writes, and the same notation written by hand:
/// one value, with white space between items wherever a person put it.
pub fn parse_pretty(text: &str) -> Result<Sexp, PrettyError> {
    let bytes = text.as_bytes();
    let mut at = 0;
    let value = pretty_value(text, &mut at, 0)?;
    while at < bytes.len() && bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    if at < bytes.len() {
        return Err(PrettyError { what: "text after the value", offset: at });
    }
    Ok(value)
}

fn pretty_value(text: &str, at: &mut usize, depth: usize) -> Result<Sexp, PrettyError> {
    let bytes = text.as_bytes();
    let fail = |what, offset| Err(PrettyError { what, offset });
    while *at < bytes.len() && bytes[*at].is_ascii_whitespace() {
        *at += 1;
    }
    let start = *at;
    match bytes.get(start) {
        None => fail("the text ends where a value is expected", start),
        Some(b')') => fail("a closing parenthesis without its list", start),
        Some(b'(') => {
            if depth >= MAX_DEPTH {
                return fail("lists nested too deep", start);
            }
            *at += 1;
            let mut items = Vec::new();
            loop {
                while *at < bytes.len() && bytes[*at].is_ascii_whitespace() {
                    *at += 1;
                }
                match bytes.get(*at) {
                    None => return fail("the text ends inside a list", *at),
                    Some(b')') => {
                        *at += 1;
                        return Ok(Sexp::List(items));
                    }
                    Some(_) => items.push(pretty_value(text, at, depth + 1)?),
                }
            }
        }
        Some(b'"') => {
            let mut out = String::new();
            let mut chars = text[start + 1..].char_indices();
            loop {
                match chars.next() {
                    None => return fail("the text ends inside a quoted atom", bytes.len()),
                    Some((i, '"')) => {
                        *at = start + 1 + i + 1;
                        return Ok(Sexp::atom(out));
                    }
                    Some((i, '\\')) => match chars.next() {
                        Some((_, c @ ('"' | '\\'))) => out.push(c),
                        _ => return fail("a backslash before something other than a quote or a backslash", start + 1 + i),
                    },
                    Some((i, c)) if c.is_control() => return fail("a control character in a quoted atom", start + 1 + i),
                    Some((_, c)) => out.push(c),
                }
            }
        }
        Some(b'#') => {
            let mut end = start + 1;
            while end < bytes.len() && bytes[end].is_ascii_hexdigit() {
                end += 1;
            }
            let digits = &text[start + 1..end];
            let ends = bytes.get(end).is_none_or(|b| b.is_ascii_whitespace() || matches!(b, b'(' | b')'));
            if digits.len() % 2 != 0 || !ends || digits.bytes().any(|b| b.is_ascii_uppercase()) {
                return fail("`#` is followed by pairs of lower-case hexadecimal digits", start);
            }
            let value = (0..digits.len() / 2)
                .map(|i| u8::from_str_radix(&digits[2 * i..2 * i + 2], 16).expect("hexadecimal digits"))
                .collect::<Vec<u8>>();
            *at = end;
            Ok(Sexp::atom(value))
        }
        Some(_) => {
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_graphic() && !matches!(bytes[end], b'(' | b')' | b'"' | b'\\') {
                end += 1;
            }
            if end == start || bytes.get(end).is_some_and(|b| !b.is_ascii_whitespace() && !matches!(b, b'(' | b')')) {
                return fail("a character that no atom is written with", end);
            }
            *at = end;
            Ok(Sexp::atom(&bytes[start..end]))
        }
    }
}

