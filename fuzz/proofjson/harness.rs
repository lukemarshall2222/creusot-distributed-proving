//! The member `dp` of a proof file (R7.2 criterion 10): a file that anyone
//! may have edited.
//!
//! - An entry that decodes encodes to the bytes that it was decoded from.
//! - A member is accepted only as the canonical base64 of an entry.
//! - Whatever is accepted has one of the label names of R7.2; a name of
//!   another version or a damaged one is no entry.

use dp_formats::{
    hash::Hash,
    proofjson::{base64_decode, base64_encode, DpEntry, InvalidEntry, Place, ReplayFields, LABELS},
};

pub fn run(data: &[u8]) {
    if let Ok(entry) = DpEntry::decode(data, crate::LIMIT) {
        assert_eq!(entry.encode(), data, "two encodings of one entry");
        assert!(LABELS.contains(&entry.label.as_str()), "an unknown label {:?} was read", entry.label);
    }
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Some(bytes) = base64_decode(text) {
        // What decodes is at most a non-canonical spelling of its bytes.
        assert_eq!(base64_decode(&base64_encode(&bytes)).as_ref(), Some(&bytes));
    }
    if let Ok(entry) = DpEntry::from_member(text, crate::LIMIT) {
        assert_eq!(entry.to_member(), text, "two members of one entry");
        assert!(LABELS.contains(&entry.label.as_str()));
    }
}

pub fn seeds() -> Vec<Vec<u8>> {
    let replay = ReplayFields {
        task_hash: Hash([1; 32]),
        driver: b"driver".to_vec(),
        prover: "alt-ergo".into(),
        version: "2.6.4".into(),
        binary_hash: Hash([2; 32]),
        place: Place::Worker("w1".into()),
        seed: Some(2),
        velocity: Some(1_000_000),
        local_micros: Some(250_000),
        reference_micros: 250_000,
    };
    let proved = DpEntry {
        label: "proved-standalone".into(),
        replay: vec![replay.clone()],
        imports_hash: Hash([3; 32]),
        certificate: None,
        conflict: true,
        invalid: vec![InvalidEntry { prover: "z3".into(), version: "4".into(), binary_hash: Hash([4; 32]), place: Place::Local }],
    };
    let certified = DpEntry {
        label: "certified".into(),
        replay: vec![ReplayFields { place: Place::Absent, seed: None, velocity: None, local_micros: None, ..replay }],
        certificate: Some(Hash([5; 32])),
        conflict: false,
        invalid: Vec::new(),
        ..proved.clone()
    };
    let unknown = DpEntry { label: "unknown".into(), replay: Vec::new(), certificate: None, conflict: false, invalid: Vec::new(), ..proved.clone() };
    let mut out = Vec::new();
    for entry in [proved, certified, unknown] {
        out.push(entry.encode());
        out.push(entry.to_member().into_bytes());
    }
    out
}
