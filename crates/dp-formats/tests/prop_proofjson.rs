// Feature: creusot-distributed-proving, Property 30: proof.json and the Creusot_CLI report labels faithfully
// P-ID: P7.2.6 (encoding part), P7.10.1 (proof.json extension)

use dp_formats::hash::Hash;
use dp_formats::proofjson::{base64_decode, base64_encode, DpEntry};
use dp_testkit::gen::proofjson::entry;
use proptest::prelude::*;

proptest! {
    #![proptest_config(dp_testkit::config::cases("prop_proofjson", 256))]

    #[test]
    fn base64_round_trips(bytes in proptest::collection::vec(any::<u8>(), 0..40)) {
        let text = base64_encode(&bytes);
        prop_assert_eq!(text.len() % 4, 0);
        prop_assert_eq!(base64_decode(&text), Some(bytes));
    }

    #[test]
    fn base64_has_one_text_per_value(text in "[A-Za-z0-9+/=]{0,12}") {
        if let Some(bytes) = base64_decode(&text) {
            prop_assert_eq!(base64_encode(&bytes), text);
        }
    }

    #[test]
    fn an_entry_round_trips_with_its_label_and_absent_markers(e in entry()) {
        let member = e.to_member();
        prop_assert_eq!(DpEntry::from_member(&member, 1 << 20), Ok(e));
    }

    #[test]
    fn a_changed_member_is_not_read_as_another_label(e in entry(), at in any::<prop::sample::Index>(), c in "[A-Za-z0-9+/=]") {
        let member = e.to_member();
        let i = at.index(member.len());
        let mut changed = member.clone().into_bytes();
        changed[i] = c.as_bytes()[0];
        let changed = String::from_utf8(changed).unwrap();
        // Whatever is read back is exactly what those bytes canonically say.
        if let Ok(read) = DpEntry::from_member(&changed, 1 << 20) {
            prop_assert_eq!(read.to_member(), changed);
        }
    }
}

#[test]
fn known_base64_values() {
    // RFC 4648 section 10.
    for (plain, encoded) in
        [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]
    {
        assert_eq!(base64_encode(plain.as_bytes()), encoded);
        assert_eq!(base64_decode(encoded).as_deref(), Some(plain.as_bytes()));
    }
    // Not canonical: missing padding, white space, bits after the last byte.
    for bad in ["Zg", "Zg= ", "Zh==", "Zm9=", "=Zg=", "Zg==Zg=="] {
        assert_eq!(base64_decode(bad), None, "{bad}");
    }
}

#[test]
fn an_unknown_label_is_rejected() {
    let e = DpEntry { label: "proved".into(), replay: vec![], imports_hash: Hash([0; 32]), certificate: None, conflict: false, invalid: vec![] };
    assert!(DpEntry::from_member(&e.to_member(), 1 << 20).is_err());
}
