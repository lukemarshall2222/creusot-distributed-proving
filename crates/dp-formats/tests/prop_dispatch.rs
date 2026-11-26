// Feature: creusot-distributed-proving, Property 34: Exchanged formats parse strictly and print canonically
// P-ID: P7.10.1 (dispatch format)

use dp_formats::dispatch::*;
use dp_formats::sexp::{parse, print, Sexp};
use dp_testkit::gen::dispatch::{from_why3find, to_why3find};
use proptest::prelude::*;

proptest! {
    #![proptest_config(dp_testkit::config::cases("prop_dispatch", 256))]

    #[test]
    fn messages_from_why3find_round_trip(m in from_why3find()) {
        let bytes = print(&m.encode());
        prop_assert_eq!(FromWhy3find::decode(&parse(&bytes, 1 << 20).unwrap()), Ok(m));
    }

    #[test]
    fn messages_to_why3find_round_trip(m in to_why3find()) {
        let bytes = print(&m.encode());
        prop_assert_eq!(ToWhy3find::decode(&parse(&bytes, 1 << 20).unwrap()), Ok(m));
    }
}

#[test]
fn numbers_must_be_canonical() {
    let msg = |t: &str| {
        Sexp::list([Sexp::atom("calibration"), Sexp::atom("q"), Sexp::atom(t)])
    };
    assert!(FromWhy3find::decode(&msg("1000000")).is_ok());
    assert!(FromWhy3find::decode(&msg("0")).is_ok());
    for bad in ["01", "+1", "-1", "", "1.0", "1e3", " 1", "18446744073709551616"] {
        assert!(FromWhy3find::decode(&msg(bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn unknown_tags_and_wrong_arity_are_rejected() {
    let l = |xs: &[&str]| Sexp::list(xs.iter().map(|x| Sexp::atom(*x)));
    assert!(FromWhy3find::decode(&l(&["done"])).is_ok());
    assert!(FromWhy3find::decode(&l(&["done", "x"])).is_err());
    assert!(FromWhy3find::decode(&l(&["bogus"])).is_err());
    assert!(FromWhy3find::decode(&l(&[])).is_err());
    assert!(FromWhy3find::decode(&Sexp::atom("done")).is_err());
    assert!(ToWhy3find::decode(&l(&["stored", "1", "maybe"])).is_err());
}
