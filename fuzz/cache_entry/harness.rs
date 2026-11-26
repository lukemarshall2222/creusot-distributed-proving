//! Result_Cache entries (R7.5): files of a directory that is not trusted
//! to hold what its names say.
//!
//! An entry that decodes encodes to the bytes that it was decoded from,
//! and its key is the key of its own lookup fields.

use dp_formats::{
    cache::{cache_key, Atom, Entry, Origin},
    dispatch::DriverAnswer,
    hash::Hash,
};

pub fn run(data: &[u8]) {
    let Ok(entry) = Entry::decode(data, crate::LIMIT) else { return };
    assert_eq!(entry.encode(), data, "two encodings of one entry");
    assert_eq!(Entry::decode(&entry.encode(), crate::LIMIT).as_ref(), Ok(&entry));
    assert_eq!(cache_key(&entry.lookup), cache_key(&entry.lookup.clone()));
}

pub fn seeds() -> Vec<Vec<u8>> {
    let local = Entry {
        lookup: vec![Atom::Bytes(b"goal/path".to_vec()), Atom::Num(250_000), Atom::Bytes(Vec::new())],
        binary_hash: Hash([2; 32]),
        velocity: 1_000_000,
        local_micros: 250_000,
        imports_hash: Hash([3; 32]),
        answer: DriverAnswer::Valid,
        micros: 21_000,
        origin: Origin::Local,
    };
    let worker = Entry {
        answer: DriverAnswer::Timeout,
        origin: Origin::Worker(match dp_formats::cluster::Message::decode(
            &dp_formats::sexp::parse(&crate::result_message::seeds()[0], crate::LIMIT).expect("a seed"),
        ) {
            Ok(dp_formats::cluster::Message::Result(signed)) => signed,
            _ => unreachable!("the first seed is a Result"),
        }),
        ..local.clone()
    };
    vec![local.encode(), worker.encode()]
}
