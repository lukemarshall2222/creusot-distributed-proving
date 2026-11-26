// Feature: creusot-distributed-proving, Property 34: Exchanged formats parse strictly and print canonically
// P-ID: P7.10.1, P7.10.2, P7.10.3, P7.10.4, P7.10.6, P7.10.7, P7.10.9

//! The laws of the Format_Codec, for every format of stage 1 in one place:
//! the canonical S-expression, the protocol messages (the Result message
//! and the status report among them), the Result_Cache entry, the
//! `proof.json` extension, the two directions of `dispatch`, the event
//! trace record and the Lock_File.
//!
//! For any value v and any input t of a format:
//!
//! - `parse(print(v)) = v`, and `print(parse(print(v))) = print(v)`;
//! - if `parse(t)` succeeds, then `print(parse(t)) = t` (only the
//!   canonical form is accepted) and `parse(print(parse(t))) = parse(t)`;
//! - `parse` ends without a panic on any bytes, and an error that names
//!   an offset names one within the input;
//! - an input longer than the size limit is refused, and one that is
//!   exactly as long is not;
//! - bytes after a complete value are refused, and an item that is there
//!   twice is never ignored.

use std::fmt::Debug;

use dp_formats::{
    cache::Entry,
    cluster::Message,
    dispatch::{FromWhy3find, ToWhy3find},
    lockfile::LockFile,
    proofjson::DpEntry,
    sexp::{self, Sexp},
    trace::Record,
};
use dp_testkit::gen;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

const LIMIT: usize = 1 << 20;

/// One format: how a value is printed and how bytes are read.
struct Format<V> {
    name: &'static str,
    print: fn(&V) -> Vec<u8>,
    parse: fn(&[u8], usize) -> Result<V, String>,
    /// The parser takes a size limit.
    limited: bool,
}

fn through_sexp<V>(bytes: &[u8], limit: usize, decode: impl Fn(&Sexp) -> Result<V, String>) -> Result<V, String> {
    decode(&sexp::parse(bytes, limit).map_err(|e| e.to_string())?)
}

const SEXP: Format<Sexp> =
    Format { name: "canonical-sexp", print: |v| sexp::print(v), parse: |b, l| sexp::parse(b, l).map_err(|e| e.to_string()), limited: true };
const MESSAGE: Format<Message> = Format {
    name: "protocol-messages",
    print: |v| sexp::print(&v.encode()),
    parse: |b, l| through_sexp(b, l, |s| Message::decode(s).map_err(|e| e.to_string())),
    limited: true,
};
const CACHE: Format<Entry> =
    Format { name: "cache-entry", print: |v| v.encode(), parse: |b, l| Entry::decode(b, l).map_err(|e| e.to_string()), limited: true };
const PROOFJSON: Format<DpEntry> = Format {
    name: "proofjson-extension",
    print: |v| v.encode(),
    parse: |b, l| DpEntry::decode(b, l).map_err(|e| e.to_string()),
    limited: true,
};
const FROM_WHY3FIND: Format<FromWhy3find> = Format {
    name: "dispatch (from why3find)",
    print: |v| sexp::print(&v.encode()),
    parse: |b, l| through_sexp(b, l, |s| FromWhy3find::decode(s).map_err(|e| e.to_string())),
    limited: true,
};
const TO_WHY3FIND: Format<ToWhy3find> = Format {
    name: "dispatch (to why3find)",
    print: |v| sexp::print(&v.encode()),
    parse: |b, l| through_sexp(b, l, |s| ToWhy3find::decode(s).map_err(|e| e.to_string())),
    limited: true,
};
const TRACE: Format<Record> =
    Format { name: "event-trace", print: |v| v.encode(), parse: |b, l| Record::decode(b, l).map_err(|e| e.to_string()), limited: true };
const LOCK: Format<LockFile> = Format {
    name: "lock-file",
    print: |v| v.print().into_bytes(),
    parse: |b, _| LockFile::parse(std::str::from_utf8(b).map_err(|e| e.to_string())?).map_err(|e| e.to_string()),
    limited: false,
};

/// The offset that an error names, if it names one.
fn offset(error: &str) -> Option<usize> {
    let at = error.rfind("at byte ")?;
    error[at + 8..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// A refusal is a refusal with an offset inside the input, where it has one.
fn refused<V: Debug>(format: &Format<V>, input: &[u8], what: &str) -> Result<(), TestCaseError> {
    match (format.parse)(input, LIMIT) {
        Ok(value) => Err(TestCaseError::fail(format!("{}: {what} was accepted as {value:?}", format.name))),
        Err(error) => {
            if let Some(offset) = offset(&error) {
                prop_assert!(offset <= input.len(), "{}: offset {} of {:?} is outside {} bytes", format.name, offset, error, input.len());
            }
            Ok(())
        }
    }
}

/// One change of a byte string: a byte set, removed or put in.
fn changed(bytes: &[u8], at: &prop::sample::Index, kind: u8, byte: u8) -> Vec<u8> {
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

fn laws<V: Debug + PartialEq>(
    format: &Format<V>,
    value: V,
    (at, kind, byte): (prop::sample::Index, u8, u8),
    junk: &[u8],
) -> Result<(), TestCaseError> {
    let name = format.name;
    let printed = (format.print)(&value);
    // parse(print(v)) = v, and print(parse(print(v))) = print(v).
    let read = (format.parse)(&printed, LIMIT);
    prop_assert_eq!(read.as_ref(), Ok(&value), "{}: parse(print(v))", name);
    prop_assert_eq!((format.print)(&read.unwrap()), printed.clone(), "{}: print(parse(print(v)))", name);

    // An input that differs from a valid one in one byte, and any bytes:
    // what is accepted is canonical and reads back to itself.
    for input in [changed(&printed, &at, kind, byte), junk.to_vec()] {
        match (format.parse)(&input, LIMIT) {
            Ok(other) => {
                let again = (format.print)(&other);
                prop_assert_eq!(&again, &input, "{}: an input that is not canonical was accepted", name);
                let reread = (format.parse)(&again, LIMIT);
                prop_assert_eq!(reread.as_ref(), Ok(&other), "{}: parse(print(parse(t)))", name);
            }
            Err(error) => {
                if let Some(offset) = offset(&error) {
                    prop_assert!(offset <= input.len(), "{}: offset {} of {:?} is outside {} bytes", name, offset, error, input.len());
                }
            }
        }
    }

    // The size limit: checked before anything is read.
    if format.limited {
        prop_assert!((format.parse)(&printed, printed.len()).is_ok(), "{}: refused at its own length", name);
        if !printed.is_empty() {
            let refused = (format.parse)(&printed, printed.len() - 1);
            prop_assert!(refused.is_err(), "{}: accepted over the size limit", name);
            prop_assert!(refused.unwrap_err().contains("TooLarge"), "{}: over the limit, and another error", name);
        }
    }

    // Bytes after the value, of every kind.
    for tail in [&b"0:"[..], b"()", b"\n", b" ", &[byte][..], &printed[..]] {
        refused(format, &[&printed[..], tail].concat(), "a value with bytes after it")?;
    }
    Ok(())
}

/// An item of the value that is there twice: the last item of the list
/// that the value prints as, again. The input is refused, or it is the
/// canonical print of another value: it is never read as the value
/// without the copy.
fn duplicated<V: Debug + PartialEq>(format: &Format<V>, value: &V) -> Result<(), TestCaseError> {
    let printed = (format.print)(value);
    let Ok(Sexp::List(mut items)) = sexp::parse(&printed, LIMIT) else { return Ok(()) };
    let Some(last) = items.last().cloned() else { return Ok(()) };
    items.push(last);
    let input = sexp::print(&Sexp::List(items));
    if let Ok(other) = (format.parse)(&input, LIMIT) {
        prop_assert!(&other != value, "{}: a duplicated item was ignored", format.name);
        prop_assert_eq!((format.print)(&other), input, "{}: a duplicated item was dropped", format.name);
    }
    Ok(())
}

fn change() -> impl Strategy<Value = (prop::sample::Index, u8, u8)> {
    (any::<prop::sample::Index>(), any::<u8>(), any::<u8>())
}

fn junk() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..64),
        // Bytes that look like S-expressions.
        proptest::collection::vec(prop::sample::select(b"()0123456789:ab \n".to_vec()), 0..48),
    ]
}

fn record() -> impl Strategy<Value = Record> {
    ("[a-z-]{1,12}", any::<u64>(), "[A-Za-z]{1,16}", any::<u64>(), "\\PC{0,40}", "(\\PC|\n){0,40}").prop_map(
        |(component, seq, action, now, input, effects)| Record { component, seq, action, now, input, effects },
    )
}

fn lock() -> impl Strategy<Value = LockFile> {
    let hash = "[0-9a-f]{64}".prop_map(|h| format!("sha256:{h}"));
    let name = "[a-z][a-z0-9.-]{0,8}";
    let platform = (name, hash.clone(), prop::option::of((prop::option::of(hash.clone()), prop::option::of(name), proptest::collection::vec(hash, 0..3))));
    let tool = (name, name, 0usize..5, proptest::collection::vec("[a-z/]{1,12}", 0..3), proptest::collection::vec(platform, 0..3));
    proptest::collection::vec(tool, 0..4).prop_map(|tools| {
        let mut text = String::new();
        for (name, version, kind, patches, platforms) in tools {
            let kind = ["unpatched-solver", "sharing-solver", "checker", "why3find", "library"][kind];
            let patches = if kind == "unpatched-solver" { Vec::new() } else { patches };
            text.push_str(&format!("[[tool]]\nname = \"{name}\"\nversion = \"{version}\"\nkind = \"{kind}\"\n"));
            if kind.ends_with("solver") {
                text.push_str("prover = \"p\"\n");
            }
            if !patches.is_empty() {
                text.push_str(&format!("patches = [{}]\n", patches.iter().map(|p| format!("\"{p}\"")).collect::<Vec<_>>().join(",")));
            }
            for (platform, hash, upstream) in platforms {
                text.push_str(&format!("[[tool.platform]]\nplatform = \"{platform}\"\nhash = \"{hash}\"\n"));
                if let Some((checksum, tag, rebuilds)) = upstream {
                    let mut fields: Vec<String> = Vec::new();
                    fields.extend(checksum.map(|c| format!("published_checksum = \"{c}\"")));
                    fields.extend(tag.map(|t| format!("tag = \"{t}\"")));
                    fields.push(format!("rebuild_hashes = [{}]", rebuilds.iter().map(|r| format!("\"{r}\"")).collect::<Vec<_>>().join(", ")));
                    text.push_str(&format!("upstream = {{ {} }}\n", fields.join(", ")));
                }
            }
        }
        LockFile::parse_lenient(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
    })
}

proptest! {
    #![proptest_config(dp_testkit::config::cases("roundtrip", 200))]

    #[test]
    fn canonical_sexp(v in gen::sexp::arb_sexp(), c in change(), j in junk()) {
        laws(&SEXP, v, c, &j)?;
    }

    #[test]
    fn protocol_messages(v in gen::message::message(), c in change(), j in junk()) {
        duplicated(&MESSAGE, &v)?;
        laws(&MESSAGE, v, c, &j)?;
    }

    #[test]
    fn cache_entry(v in gen::cache_entry::entry(), c in change(), j in junk()) {
        duplicated(&CACHE, &v)?;
        laws(&CACHE, v, c, &j)?;
    }

    #[test]
    fn proofjson_extension(v in gen::proofjson::entry(), c in change(), j in junk()) {
        duplicated(&PROOFJSON, &v)?;
        // The member of `proof.json`: the base64 text of the same bytes.
        let member = v.to_member();
        let from_member = DpEntry::from_member(&member, LIMIT);
        prop_assert_eq!(from_member.as_ref(), Ok(&v));
        let other = String::from_utf8_lossy(&changed(member.as_bytes(), &c.0, c.1, c.2)).into_owned();
        if let Ok(read) = DpEntry::from_member(&other, LIMIT) {
            prop_assert_eq!(read.to_member(), other);
        }
        laws(&PROOFJSON, v, c, &j)?;
    }

    #[test]
    fn dispatch_from_why3find(v in gen::dispatch::from_why3find(), c in change(), j in junk()) {
        duplicated(&FROM_WHY3FIND, &v)?;
        laws(&FROM_WHY3FIND, v, c, &j)?;
    }

    #[test]
    fn dispatch_to_why3find(v in gen::dispatch::to_why3find(), c in change(), j in junk()) {
        duplicated(&TO_WHY3FIND, &v)?;
        laws(&TO_WHY3FIND, v, c, &j)?;
    }

    #[test]
    fn event_trace(v in record(), c in change(), j in junk()) {
        duplicated(&TRACE, &v)?;
        // A trace of records reads back, and a trace that was cut does not.
        let line = [v.encode(), b"\n".to_vec()].concat();
        let trace = line.repeat(3);
        let read = Record::decode_all(&trace, LIMIT);
        prop_assert_eq!(read.as_ref().map(Vec::len), Ok(3));
        prop_assert!(Record::decode_all(&trace[..trace.len() - 1], LIMIT).is_err());
        prop_assert!(Record::decode_all(&trace[1..], LIMIT).is_err());
        laws(&TRACE, v, c, &j)?;
    }

    #[test]
    fn lock_file(v in lock(), c in change(), j in junk()) {
        // A key that is there twice, in a table of the file.
        let text = v.print();
        if let Some(at) = text.find("name = ") {
            let end = at + text[at..].find('\n').unwrap() + 1;
            let twice = format!("{}{}{}", &text[..end], &text[at..end], &text[end..]);
            prop_assert!(LockFile::parse(&twice).is_err() && LockFile::parse_lenient(&twice).is_err(), "a key twice: {}", twice);
        }
        laws(&LOCK, v, c, &j)?;
    }
}

// The offsets of the errors of the S-expression parser, which every format
// but the Lock_File reads through: each kind at the byte where it is.
#[test]
fn an_error_names_the_byte_where_the_input_goes_wrong() {
    use dp_formats::sexp::ErrorKind::*;
    for (input, kind, at) in [
        (&b""[..], UnexpectedEnd, 0),
        (b"(", UnexpectedEnd, 1),
        (b"3:ab", UnexpectedEnd, 4),
        (b"(1:a", UnexpectedEnd, 4),
        (b"x", UnexpectedByte, 0),
        (b"(1:a x)", UnexpectedByte, 4),
        (b"1a", UnexpectedByte, 1),
        (b")", UnexpectedByte, 0),
        (b"01:a", LeadingZero, 0),
        (b"(1:a01:b)", LeadingZero, 4),
        (b"1:ab", TrailingBytes, 3),
        (b"()()", TrailingBytes, 2),
        (b"() ", TrailingBytes, 2),
    ] {
        let error = sexp::parse(input, LIMIT).unwrap_err();
        assert_eq!((error.kind, error.offset), (kind, at), "{:?}", String::from_utf8_lossy(input));
        assert!(error.offset <= input.len());
        assert_eq!(offset(&error.to_string()), Some(at));
    }
    // Over the limit: nothing is read, and the error is at the limit.
    let error = sexp::parse(b"(1:a1:b)", 7).unwrap_err();
    assert_eq!((error.kind, error.offset), (TooLarge, 7));
    // An atom that says it is longer than the limit allows.
    let error = sexp::parse(b"(99999999:a)", 12).unwrap_err();
    assert_eq!(error.kind, LengthTooLarge);
    // Lists nested deeper than the parser goes.
    let deep = [vec![b'('; sexp::MAX_DEPTH + 1], vec![b')'; sexp::MAX_DEPTH + 1]].concat();
    assert_eq!(sexp::parse(&deep, LIMIT).unwrap_err().kind, TooDeep);
    let deepest = [vec![b'('; sexp::MAX_DEPTH], vec![b')'; sexp::MAX_DEPTH]].concat();
    assert!(sexp::parse(&deepest, LIMIT).is_ok());
}

// The grammars in docs/formats: the length before every literal atom is
// the length of the atom, and every literal of a codec's tag is in the
// grammar of its format.
#[test]
fn the_grammars_spell_their_atoms_with_the_right_lengths() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/formats");
    let mut literals = 0;
    let mut grammars = std::collections::BTreeMap::new();
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "abnf") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for part in text.split("%s\"").skip(1) {
            let literal = &part[..part.find('"').unwrap()];
            if let Some((length, atom)) = literal.split_once(':') {
                // `%s"32:" 32OCTET` is the length alone, before bytes that
                // the next rule gives.
                if !atom.is_empty() && !length.is_empty() && length.bytes().all(|b| b.is_ascii_digit()) {
                    assert_eq!(length.parse::<usize>().unwrap(), atom.len(), "{}: {literal:?}", path.display());
                    literals += 1;
                }
            }
        }
        grammars.insert(path.file_stem().unwrap().to_string_lossy().into_owned(), text);
    }
    assert!(literals > 80, "{literals} literal atoms");
    // The tags that the codecs print, from values of every kind.
    let tag = |bytes: Vec<u8>| -> String {
        let Ok(Sexp::List(items)) = sexp::parse(&bytes, LIMIT) else { panic!("not a list") };
        let atom = items[0].as_atom().unwrap();
        format!("%s\"{}:{}\"", atom.len(), String::from_utf8_lossy(atom))
    };
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    use proptest::strategy::ValueTree;
    for _ in 0..400 {
        let message = gen::message::message().new_tree(&mut runner).unwrap().current();
        assert!(grammars["protocol-messages"].contains(&tag(sexp::print(&message.encode()))), "{message:?}");
        let from = gen::dispatch::from_why3find().new_tree(&mut runner).unwrap().current();
        assert!(grammars["dispatch"].contains(&tag(sexp::print(&from.encode()))), "{from:?}");
        let to = gen::dispatch::to_why3find().new_tree(&mut runner).unwrap().current();
        assert!(grammars["dispatch"].contains(&tag(sexp::print(&to.encode()))), "{to:?}");
    }
    let entry = gen::cache_entry::entry().new_tree(&mut runner).unwrap().current();
    assert!(grammars["cache-entry"].contains(&tag(entry.encode())));
    let dp = gen::proofjson::entry().new_tree(&mut runner).unwrap().current();
    assert!(grammars["proofjson-extension"].contains(&tag(dp.encode())));
    let record = Record { component: "c".into(), seq: 1, action: "A".into(), now: 0, input: String::new(), effects: String::new() };
    assert!(grammars["event-trace"].contains(&tag(record.encode())));
}
