//! The canonical S-expression codec (R7.10, design decision D-7): the
//! parser under every other format.
//!
//! - What the parser accepts is exactly the canonical print of the value:
//!   no two byte strings are one value.
//! - The prefix parser, which reads a stream, agrees with the parser of a
//!   whole value: a complete value is taken with exactly its bytes, and a
//!   proper prefix of one is never taken for a value.
//! - An input over the size limit is refused, whatever it holds.

use dp_formats::sexp::{self, Prefix, Sexp};

pub fn run(data: &[u8]) {
    let whole = sexp::parse(data, crate::LIMIT);
    if let Ok(value) = &whole {
        let printed = sexp::print(value);
        assert_eq!(printed, data, "the parser accepted a print that is not canonical");
        assert_eq!(sexp::parse(&printed, crate::LIMIT).as_ref(), Ok(value));
        // The pretty print is for people; it never fails.
        let _ = sexp::pretty(value);
    }
    match sexp::parse_prefix(data, crate::LIMIT) {
        Ok(Prefix::Value(value, used)) => {
            assert!(used <= data.len());
            assert_eq!(sexp::print(&value), &data[..used], "the prefix is not the print of its value");
            if used == data.len() {
                assert_eq!(whole.as_ref(), Ok(&value), "the two parsers disagree on a whole value");
            } else {
                assert!(whole.is_err(), "bytes after a complete value were accepted");
            }
        }
        Ok(Prefix::Incomplete) => assert!(whole.is_err(), "an incomplete value was accepted"),
        Err(_) => assert!(whole.is_err(), "the prefix parser refuses what the parser accepts"),
    }
    // The limit is checked before anything is read.
    if data.len() > 8 {
        assert!(sexp::parse(data, data.len() - 1).is_err(), "an input over the limit was accepted");
    }
}

pub fn seeds() -> Vec<Vec<u8>> {
    let nested = Sexp::list([
        Sexp::atom("tag"),
        Sexp::list([]),
        Sexp::atom([0u8, 255, b'(', b')', b':']),
        Sexp::list([Sexp::atom(""), Sexp::list([Sexp::atom("deep"), Sexp::list([Sexp::atom("er")])])]),
        Sexp::atom("0123456789"),
    ]);
    vec![
        sexp::print(&Sexp::atom("")),
        sexp::print(&Sexp::atom("atom")),
        sexp::print(&Sexp::list([])),
        sexp::print(&nested),
        // Two values, as on a connection.
        [sexp::print(&nested), sexp::print(&Sexp::atom("next"))].concat(),
        // Not canonical: a length with a leading zero, and white space.
        b"(01:a)".to_vec(),
        b"(1:a 1:b)".to_vec(),
    ]
}
