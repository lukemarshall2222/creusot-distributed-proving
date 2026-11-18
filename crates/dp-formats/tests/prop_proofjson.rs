// Feature: creusot-distributed-proving, Property 30: proof.json and the Creusot_CLI report labels faithfully
// P-ID: P7.2.6 (encoding part), P7.10.1 (proof.json extension)

use dp_formats::hash::Hash;
use dp_formats::proofjson::{base64_decode, base64_encode, DpEntry, InvalidEntry, Place, ReplayFields, LABELS};
use proptest::prelude::*;

fn place() -> impl Strategy<Value = Place> {
    prop_oneof![Just(Place::Local), Just(Place::Absent), "[a-z0-9]{0,6}".prop_map(Place::Worker)]
}

fn replay() -> impl Strategy<Value = ReplayFields> {
    (
        any::<[u8; 32]>(),
        proptest::collection::vec(any::<u8>(), 0..12),
        "[a-z-]{0,8}",
        "[0-9.]{0,6}",
        any::<[u8; 32]>(),
        place(),
        any::<Option<u64>>(),
        any::<Option<u64>>(),
        any::<Option<u64>>(),
        any::<u64>(),
    )
        .prop_map(|(task, driver, prover, version, binary, place, seed, velocity, local, reference)| {
            ReplayFields {
                task_hash: Hash(task),
                driver,
                prover,
                version,
                binary_hash: Hash(binary),
                place,
                seed,
                velocity,
                local_micros: local,
                reference_micros: reference,
            }
        })
}

fn entry() -> impl Strategy<Value = DpEntry> {
    let invalid = ("[a-z-]{0,8}", "[0-9.]{0,6}", any::<[u8; 32]>(), place()).prop_map(
        |(prover, version, binary, place)| InvalidEntry { prover, version, binary_hash: Hash(binary), place },
    );
    (
        proptest::sample::select(LABELS.to_vec()),
        proptest::collection::vec(replay(), 0..3),
        any::<[u8; 32]>(),
        any::<bool>(),
        proptest::collection::vec(invalid, 0..3),
    )
        .prop_map(|(label, replay, imports, conflict, invalid)| DpEntry {
            label: label.to_string(),
            replay,
            imports_hash: Hash(imports),
            conflict,
            invalid,
        })
}

proptest! {
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
    let e = DpEntry { label: "proved".into(), replay: vec![], imports_hash: Hash([0; 32]), conflict: false, invalid: vec![] };
    assert!(DpEntry::from_member(&e.to_member(), 1 << 20).is_err());
}
