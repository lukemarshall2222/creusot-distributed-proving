// Feature: creusot-distributed-proving, Property 34: Exchanged formats parse strictly and print canonically
// P-ID: P7.10.1

use dp_formats::sexp::{parse, parse_prefix, print, Error, ErrorKind, Prefix, Sexp, MAX_DEPTH};
use proptest::prelude::*;

const LIMIT: usize = 1 << 20;

fn arb_sexp() -> impl Strategy<Value = Sexp> {
    let leaf = proptest::collection::vec(any::<u8>(), 0..20).prop_map(Sexp::Atom);
    leaf.prop_recursive(6, 64, 8, |inner| {
        proptest::collection::vec(inner, 0..8).prop_map(Sexp::List)
    })
}

proptest! {
    #[test]
    fn parse_inverts_print(v in arb_sexp()) {
        prop_assert_eq!(parse(&print(&v), LIMIT), Ok(v));
    }

    /// Any accepted input is the canonical form of its value.
    #[test]
    fn accepted_input_is_canonical(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        if let Ok(v) = parse(&bytes, LIMIT) {
            prop_assert_eq!(print(&v), bytes);
        }
    }

    /// Mutating one byte of a canonical form gives a rejection or a different value.
    #[test]
    fn one_byte_mutations_never_alias(v in arb_sexp(), at in any::<prop::sample::Index>(), byte in any::<u8>()) {
        let mut bytes = print(&v);
        let i = at.index(bytes.len());
        prop_assume!(bytes[i] != byte);
        bytes[i] = byte;
        if let Ok(w) = parse(&bytes, LIMIT) {
            prop_assert_ne!(w, v);
        }
    }

    /// Every proper prefix of a canonical form is incomplete, never a value.
    #[test]
    fn proper_prefixes_are_incomplete(v in arb_sexp(), at in any::<prop::sample::Index>()) {
        let bytes = print(&v);
        let n = at.index(bytes.len());
        prop_assert_eq!(parse_prefix(&bytes[..n], LIMIT), Ok(Prefix::Incomplete));
        prop_assert_eq!(parse(&bytes[..n], LIMIT).map_err(|e| e.kind), Err(ErrorKind::UnexpectedEnd));
    }

    /// A stream of values is split at the right offsets.
    #[test]
    fn streams_split_into_values(vs in proptest::collection::vec(arb_sexp(), 1..5)) {
        let mut bytes = Vec::new();
        for v in &vs { bytes.extend(print(v)); }
        let mut pos = 0;
        for v in &vs {
            match parse_prefix(&bytes[pos..], LIMIT) {
                Ok(Prefix::Value(w, used)) => { prop_assert_eq!(&w, v); pos += used; }
                other => prop_assert!(false, "unexpected {:?}", other),
            }
        }
        prop_assert_eq!(pos, bytes.len());
    }
}

#[test]
fn rejections_name_the_offset() {
    let err = |b: &[u8]| parse(b, 16).unwrap_err();
    assert_eq!(err(b"3:abcd"), Error { kind: ErrorKind::TrailingBytes, offset: 5 });
    assert_eq!(err(b"03:abc"), Error { kind: ErrorKind::LeadingZero, offset: 0 });
    assert_eq!(err(b"(1:a 1:b)"), Error { kind: ErrorKind::UnexpectedByte, offset: 4 });
    assert_eq!(err(b")"), Error { kind: ErrorKind::UnexpectedByte, offset: 0 });
    assert_eq!(err(b"3:ab"), Error { kind: ErrorKind::UnexpectedEnd, offset: 4 });
    assert_eq!(err(b"(1:a"), Error { kind: ErrorKind::UnexpectedEnd, offset: 4 });
    assert_eq!(err(b"99:x"), Error { kind: ErrorKind::LengthTooLarge, offset: 0 });
    assert_eq!(err(b"1x"), Error { kind: ErrorKind::UnexpectedByte, offset: 1 });
    assert_eq!(err(b""), Error { kind: ErrorKind::UnexpectedEnd, offset: 0 });
    // the size limit is checked before parsing: the bad byte at 0 is not reported
    assert_eq!(err(b"!!!!!!!!!!!!!!!!!"), Error { kind: ErrorKind::TooLarge, offset: 16 });
    assert_eq!(parse(b"0:", 16), Ok(Sexp::atom(b"")));
    assert_eq!(parse(b"()", 16), Ok(Sexp::List(vec![])));
}

#[test]
fn nesting_is_bounded() {
    let deep = |n: usize| [vec![b'('; n], vec![b')'; n]].concat();
    assert!(parse(&deep(MAX_DEPTH), LIMIT).is_ok());
    assert_eq!(
        parse(&deep(MAX_DEPTH + 1), LIMIT).unwrap_err(),
        Error { kind: ErrorKind::TooDeep, offset: MAX_DEPTH }
    );
    // a hostile depth does not overflow the stack
    assert_eq!(parse(&vec![b'('; 1_000_000], 2_000_000).unwrap_err().kind, ErrorKind::TooDeep);
}

#[test]
fn stream_values_over_the_limit_are_rejected() {
    assert_eq!(parse_prefix(b"5:abcde", 4).unwrap_err().kind, ErrorKind::LengthTooLarge);
    assert_eq!(parse_prefix(b"(1:a1:b1:c", 4).unwrap_err().kind, ErrorKind::TooLarge);
    assert_eq!(parse_prefix(b"(1:a)", 4).unwrap_err().kind, ErrorKind::TooLarge);
}
