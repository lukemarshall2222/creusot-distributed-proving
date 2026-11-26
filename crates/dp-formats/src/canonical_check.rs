//! The canonical-input check (R7.10 criterion 8, design C16).
//!
//! Every value of an exchanged format has one printed form. For the four
//! formats whose bytes are signed, hashed or compared across machines, an
//! input is taken only if it is that form: `print(parse(b)) = b`. The
//! formats are the Result message, the Result_Cache entry, the Lock_File
//! and the `proof.json` extension field.
//!
//! An input that fails the check is discarded with an error that names the
//! format and the offset of the first byte that differs (criterion 2): a
//! Result message is dropped (R1.3 criterion 8), a cache lookup is a miss
//! (R7.5 criterion 5), a Lock_File pins nothing (R7.7 criterion 3), and
//! the Replay_Tool reports the goal as failed.
//!
//! Each decoder of those formats calls `check` on what it parsed, so no
//! caller can take a value without it.

/// An input that parses and is not the canonical print of its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotCanonical {
    /// The exchanged format, as the format registry names it.
    pub format: &'static str,
    /// The offset of the first byte of the input that is not the byte of
    /// the canonical print; the length of the shorter one if one is a
    /// prefix of the other.
    pub offset: usize,
}

impl std::fmt::Display for NotCanonical {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: not the canonical form of its value, at byte {}", self.format, self.offset)
    }
}

impl std::error::Error for NotCanonical {}

pub const RESULT_MESSAGE: &str = "result-message";
pub const PROTOCOL_MESSAGE: &str = "protocol-messages";
pub const CACHE_ENTRY: &str = "cache-entry";
pub const LOCK_FILE: &str = "lock-file";
pub const PROOFJSON_EXTENSION: &str = "proofjson-extension";

/// `input` is what was read and `printed` the canonical print of the value
/// that it parsed to.
pub fn check(format: &'static str, input: &[u8], printed: &[u8]) -> Result<(), NotCanonical> {
    if input == printed {
        return Ok(());
    }
    let offset = input.iter().zip(printed).position(|(a, b)| a != b).unwrap_or(input.len().min(printed.len()));
    Err(NotCanonical { format, offset })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Feature: creusot-distributed-proving, Property 34: Exchanged formats parse strictly and print canonically
    // P-ID: P7.10.8

    #[test]
    fn the_offset_is_that_of_the_first_differing_byte() {
        assert_eq!(check("f", b"abc", b"abc"), Ok(()));
        assert_eq!(check("f", b"", b""), Ok(()));
        assert_eq!(check("f", b"abX", b"abc"), Err(NotCanonical { format: "f", offset: 2 }));
        assert_eq!(check("f", b"Xbc", b"abc"), Err(NotCanonical { format: "f", offset: 0 }));
        // A prefix, either way: the offset is where the shorter one ends.
        assert_eq!(check("f", b"ab", b"abc"), Err(NotCanonical { format: "f", offset: 2 }));
        assert_eq!(check("f", b"abcd", b"abc"), Err(NotCanonical { format: "f", offset: 3 }));
        assert_eq!(check("f", b"", b"a"), Err(NotCanonical { format: "f", offset: 0 }));
        let said = check(LOCK_FILE, b"a", b"b").unwrap_err().to_string();
        assert_eq!(said, "lock-file: not the canonical form of its value, at byte 0");
    }
}
