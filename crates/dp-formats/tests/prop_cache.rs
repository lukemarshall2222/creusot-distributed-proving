// Feature: creusot-distributed-proving, Property 29: Cache hits reuse only what nothing could have changed
// P-ID: P7.5.1, P7.5.2, P7.10.1 (Result_Cache entry)

use dp_formats::cache::{cache_key, Atom, Entry};
use dp_testkit::gen::cache_entry::{change, entry, lookup, FIELDS};
use proptest::prelude::*;

proptest! {
    #![proptest_config(dp_testkit::config::cases("prop_cache", 256))]

    #[test]
    fn entries_round_trip(e in entry()) {
        prop_assert_eq!(Entry::decode(&e.encode(), 1 << 20), Ok(e));
    }

    /// Different lookup fields give different keys: an atom's kind, length
    /// and position are all part of the encoding (P7.5.1).
    #[test]
    fn different_lookup_fields_give_different_keys(a in lookup(), b in lookup()) {
        prop_assert_eq!(a == b, cache_key(&a) == cache_key(&b));
    }

    /// Bytes moved between neighbouring atoms, or an atom split in two,
    /// change the key.
    #[test]
    fn atom_boundaries_are_part_of_the_key(x in proptest::collection::vec(any::<u8>(), 1..12), at in any::<prop::sample::Index>()) {
        let i = at.index(x.len());
        let whole = vec![Atom::Bytes(x.clone())];
        let split = vec![Atom::Bytes(x[..i].to_vec()), Atom::Bytes(x[i..].to_vec())];
        prop_assert_ne!(cache_key(&whole), cache_key(&split));
        prop_assert_ne!(cache_key(&[Atom::Num(7)]), cache_key(&[Atom::Bytes(b"7".to_vec())]));
    }

    /// One changed byte of a stored entry is a parse failure or a different
    /// entry, never the same entry.
    #[test]
    fn a_changed_byte_never_gives_the_same_entry(e in entry(), at in any::<prop::sample::Index>()) {
        let mut bytes = e.encode();
        let i = at.index(bytes.len());
        bytes[i] ^= 1;
        if let Ok(other) = Entry::decode(&bytes, 1 << 20) {
            prop_assert_ne!(other, e);
        }
    }

    /// One changed lookup or validation field is another entry, with
    /// another encoding; the key changes exactly with the lookup fields.
    #[test]
    fn a_changed_field_is_another_entry(e in entry(), which in 0..FIELDS) {
        let g = change(&e, which);
        prop_assert_ne!(&g, &e);
        prop_assert_ne!(g.encode(), e.encode());
        prop_assert_eq!(Entry::decode(&g.encode(), 1 << 20), Ok(g.clone()));
        prop_assert_eq!(cache_key(&g.lookup) == cache_key(&e.lookup), which != FIELDS - 1);
    }
}
