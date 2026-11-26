//! The event trace (R1.5 criterion 10): the file that trace validation
//! reads, written by a component that may have stopped in the middle of
//! it.
//!
//! - A record that decodes encodes to the bytes that it was decoded from.
//! - A trace that reads as records is exactly those records, each followed
//!   by a line feed; nothing else is taken for a trace.

use dp_formats::trace::Record;

pub fn run(data: &[u8]) {
    if let Ok(record) = Record::decode(data, crate::LIMIT) {
        assert_eq!(record.encode(), data, "two encodings of one record");
    }
    if let Ok(records) = Record::decode_all(data, crate::LIMIT) {
        let mut again = Vec::new();
        for record in &records {
            again.extend(record.encode());
            again.push(b'\n');
        }
        assert_eq!(again, data, "a trace that is not its records line by line was read");
    }
}

pub fn seeds() -> Vec<Vec<u8>> {
    let record = |seq: u64, input: &str, effects: &str| Record {
        component: "coordinator".to_string(),
        seq,
        action: "Assign".to_string(),
        now: 1_000 * seq,
        input: input.to_string(),
        effects: effects.to_string(),
    };
    let one = record(1, "Submit { job: 4 }", "[Send(10, Assign)]").encode();
    // An input with a line feed and parentheses of its own.
    let two = record(2, "a (\nb", "").encode();
    let trace = [one.as_slice(), b"\n", two.as_slice(), b"\n"].concat();
    vec![one.clone(), two, trace.clone(), trace[..trace.len() - 1].to_vec(), trace[..one.len() / 2].to_vec()]
}
