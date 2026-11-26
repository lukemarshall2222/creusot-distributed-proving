// Feature: creusot-distributed-proving, Property 34: Exchanged formats parse strictly and print canonically
// P-ID: P7.10.8

//! The canonical-input check over the four formats that it guards
//! (R7.10 criterion 8): whatever a decoder accepts is byte for byte the
//! print of what it returns, also after any byte of a valid input was
//! changed, removed or added; and an input that is refused for its form
//! is refused with the name of the format and an offset.

use dp_formats::{
    cache::Entry,
    canonical_check::{check, NotCanonical, CACHE_ENTRY, LOCK_FILE, PROOFJSON_EXTENSION, RESULT_MESSAGE},
    cluster::{Message, SignedResult},
    lockfile::LockFile,
    proofjson::DpEntry,
    sexp,
};
use dp_testkit::gen;
use proptest::prelude::*;

const LIMIT: usize = 1 << 20;

/// One change of a byte string: a byte set, removed or put in.
fn changed(bytes: &[u8], at: prop::sample::Index, kind: u8, byte: u8) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let i = at.index(out.len().max(1)).min(out.len().saturating_sub(1));
    match kind % 3 {
        0 if !out.is_empty() => out[i] = byte,
        1 if !out.is_empty() => {
            out.remove(i);
        }
        _ => out.insert(i.min(out.len()), byte),
    }
    out
}

fn lock_text() -> impl Strategy<Value = String> {
    let hash = "[0-9a-f]{64}".prop_map(|h| format!("sha256:{h}"));
    let name = "[a-z][a-z0-9.-]{0,8}";
    let platform = (name, hash.clone(), prop::option::of((prop::option::of(hash.clone()), prop::option::of(name), proptest::collection::vec(hash, 0..3))));
    let tool = (name, name, 0usize..4, proptest::collection::vec("[a-z/]{1,12}", 0..3), proptest::collection::vec(platform, 0..3));
    proptest::collection::vec(tool, 0..4).prop_map(|tools| {
        let mut text = String::new();
        for (name, version, kind, patches, platforms) in tools {
            // In any order of fields that TOML allows: the lenient reader
            // takes it, and its print is the canonical text.
            let kind = ["unpatched-solver", "sharing-solver", "checker", "why3find"][kind];
            let patches = if kind == "unpatched-solver" { Vec::new() } else { patches };
            text.push_str(&format!("[[tool]]\nkind = \"{kind}\"\nversion = \"{version}\"\nname = \"{name}\"\n"));
            if kind.ends_with("solver") {
                text.push_str("prover = \"p\"\n");
            }
            if !patches.is_empty() {
                text.push_str(&format!("patches = [{}]\n", patches.iter().map(|p| format!("\"{p}\"")).collect::<Vec<_>>().join(",")));
            }
            for (platform, hash, upstream) in platforms {
                text.push_str(&format!("[[tool.platform]]\nhash = \"{hash}\"\nplatform = \"{platform}\"\n"));
                if let Some((checksum, tag, rebuilds)) = upstream {
                    text.push_str("[tool.platform.upstream]\n");
                    if let Some(checksum) = checksum {
                        text.push_str(&format!("published_checksum = \"{checksum}\"\n"));
                    }
                    if let Some(tag) = tag {
                        text.push_str(&format!("tag = \"{tag}\"\n"));
                    }
                    text.push_str(&format!("rebuild_hashes = [{}]\n", rebuilds.iter().map(|r| format!("\"{r}\"")).collect::<Vec<_>>().join(", ")));
                }
            }
        }
        LockFile::parse_lenient(&text).unwrap_or_else(|e| panic!("{e}: {text}")).print()
    })
}

proptest! {
    #![proptest_config(dp_testkit::config::cases("prop_canonical", 300))]

    #[test]
    fn an_accepted_cache_entry_is_its_own_print(
        entry in gen::cache_entry::entry(), at in any::<prop::sample::Index>(), kind in any::<u8>(), byte in any::<u8>(),
    ) {
        let bytes = entry.encode();
        let read = Entry::decode(&bytes, LIMIT);
        prop_assert_eq!(read.as_ref(), Ok(&entry));
        let other = changed(&bytes, at, kind, byte);
        if let Ok(read) = Entry::decode(&other, LIMIT) {
            prop_assert_eq!(read.encode(), other);
        }
    }

    #[test]
    fn an_accepted_result_message_is_its_own_print(
        fields in gen::result::fields(), key in gen::result::key(),
        at in any::<prop::sample::Index>(), kind in any::<u8>(), byte in any::<u8>(),
    ) {
        let message = Message::Result(SignedResult::sign(fields, &key));
        let bytes = sexp::print(&message.encode());
        let read = Message::decode(&sexp::parse(&bytes, LIMIT).unwrap());
        prop_assert_eq!(read.as_ref(), Ok(&message));
        let other = changed(&bytes, at, kind, byte);
        if let Ok(read) = sexp::parse(&other, LIMIT).map_err(|e| e.to_string()).and_then(|s| Message::decode(&s).map_err(|e| e.to_string())) {
            prop_assert_eq!(sexp::print(&read.encode()), other);
        }
    }

    #[test]
    fn an_accepted_lock_file_is_its_own_print(
        text in lock_text(), at in any::<prop::sample::Index>(), kind in any::<u8>(), byte in 0u8..128,
    ) {
        let lock = LockFile::parse(&text);
        prop_assert!(lock.is_ok(), "{:?}: {}", lock.err(), text);
        let other = String::from_utf8(changed(text.as_bytes(), at, kind, byte)).unwrap();
        if let Ok(read) = LockFile::parse(&other) {
            prop_assert_eq!(read.print(), other);
        }
    }

    // The offset that the check names is the first byte that differs.
    #[test]
    fn the_check_names_the_first_differing_byte(
        a in proptest::collection::vec(any::<u8>(), 0..40), b in proptest::collection::vec(any::<u8>(), 0..40),
    ) {
        match check(CACHE_ENTRY, &a, &b) {
            Ok(()) => prop_assert_eq!(a, b),
            Err(NotCanonical { format, offset }) => {
                prop_assert_eq!(format, CACHE_ENTRY);
                prop_assert_eq!(&a[..offset], &b[..offset]);
                prop_assert!(a.get(offset) != b.get(offset));
            }
        }
    }
}

// An input that is refused for its form says which format and where.
#[test]
fn a_refusal_names_the_format_and_the_offset() {
    // The base64 text of a `dp` member with a bit set behind its last byte.
    let entry = DpEntry {
        label: "unknown".into(),
        replay: vec![],
        imports_hash: dp_formats::hash::Hash([0; 32]),
        certificate: None,
        conflict: false,
        invalid: vec![],
    };
    let member = entry.to_member();
    assert!(member.ends_with('='), "{member}");
    let last = member.trim_end_matches('=').len() - 1;
    // The next digit of the alphabet differs in a padding bit only.
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digit = ALPHABET.iter().position(|d| *d == member.as_bytes()[last]).unwrap();
    assert_eq!(digit % 2, 0, "the padding bits of a canonical member are zero");
    let other = format!("{}{}{}", &member[..last], ALPHABET[digit + 1] as char, &member[last + 1..]);
    // The base64 reader refuses it before the entry is read: no second
    // text of one entry gets as far as the check.
    assert!(DpEntry::from_member(&other, LIMIT).is_err());
    assert_eq!(DpEntry::from_member(&member, LIMIT), Ok(entry));
    // A Lock_File in another layout.
    let said = LockFile::parse("[[tool]]\nname = \"a\"\nversion = \"1\"\nkind = \"lean\"\n").unwrap_err().to_string();
    assert_eq!(said, format!("Lock_File: {LOCK_FILE}: not the canonical form of its value, at byte 0"));
    // The names are those of the format registry.
    let registry = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/formats/index.toml")).unwrap();
    for format in [RESULT_MESSAGE, CACHE_ENTRY, LOCK_FILE, PROOFJSON_EXTENSION] {
        assert!(registry.contains(&format!("name = \"{format}\"")), "{format}");
    }
}
