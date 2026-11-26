//! Canonical S-expressions (Property 34): atoms of any bytes, lists
//! nested a few levels.

use dp_formats::sexp::Sexp;
use proptest::prelude::*;

pub fn arb_sexp() -> impl Strategy<Value = Sexp> {
    let leaf = proptest::collection::vec(any::<u8>(), 0..20).prop_map(Sexp::Atom);
    leaf.prop_recursive(6, 64, 8, |inner| {
        proptest::collection::vec(inner, 0..8).prop_map(Sexp::List)
    })
}

/// Values whose atoms are of every kind that the pretty printer tells
/// apart: bare words, text that needs quotes, bytes that are no text, and
/// the empty atom.
pub fn readable_sexp() -> impl Strategy<Value = Sexp> {
    let leaf = prop_oneof![
        3 => "[a-z0-9@._:-]{1,12}".prop_map(Sexp::atom),
        2 => "[ -~]{0,16}".prop_map(Sexp::atom),
        1 => "\\PC{0,8}".prop_map(Sexp::atom),
        2 => proptest::collection::vec(any::<u8>(), 0..40).prop_map(Sexp::Atom),
        1 => Just(Sexp::atom("")),
        1 => "#[a-f0-9]{0,6}".prop_map(Sexp::atom),
    ];
    leaf.prop_recursive(6, 96, 10, |inner| proptest::collection::vec(inner, 0..10).prop_map(Sexp::List))
}
