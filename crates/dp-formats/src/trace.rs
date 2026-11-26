//! The event trace record (R1.5 criterion 10, design decision D-11): one
//! step of a machine, as a component writes it to its trace and as trace
//! validation reads it. A trace is one record per line; a record is one
//! canonical S-expression.

use crate::sexp::{self, Sexp};

/// One step: which component took it, its number, the action of the TLA+
/// specification that it is, the time, and the input and the effects in
/// the component's own rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub component: String,
    /// The number of the step in its component's trace, from 1.
    pub seq: u64,
    pub action: String,
    /// The component's clock at the step, in microseconds.
    pub now: u64,
    pub input: String,
    /// Empty where the component records no effects.
    pub effects: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "event-trace: {}", self.0)
    }
}

impl std::error::Error for DecodeError {}

fn number(s: &Sexp, what: &str) -> Result<u64, DecodeError> {
    let text = s.as_str().ok_or_else(|| DecodeError(format!("{what}: expected a number")))?;
    let canonical = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) && (text == "0" || !text.starts_with('0'));
    match text.parse() {
        Ok(value) if canonical => Ok(value),
        _ => Err(DecodeError(format!("{what}: expected a canonical decimal number"))),
    }
}

fn text(s: &Sexp, what: &str) -> Result<String, DecodeError> {
    s.as_str().map(str::to_string).ok_or_else(|| DecodeError(format!("{what}: expected text")))
}

impl Record {
    /// The record, without the line feed that ends its line.
    pub fn encode(&self) -> Vec<u8> {
        sexp::print(&Sexp::list([
            Sexp::atom("trace"),
            Sexp::atom(&self.component),
            Sexp::atom(self.seq.to_string()),
            Sexp::atom(&self.action),
            Sexp::atom(self.now.to_string()),
            Sexp::atom(&self.input),
            Sexp::atom(&self.effects),
        ]))
    }

    /// Reads one record. Bytes that are not the canonical print of a
    /// record are refused.
    pub fn decode(bytes: &[u8], limit: usize) -> Result<Record, DecodeError> {
        let value = sexp::parse(bytes, limit).map_err(|e| DecodeError(e.to_string()))?;
        let Some([tag, component, seq, action, now, input, effects]) = value.as_list() else {
            return Err(DecodeError("a record has seven items".into()));
        };
        if tag.as_str() != Some("trace") {
            return Err(DecodeError("a record starts with `trace`".into()));
        }
        Ok(Record {
            component: text(component, "component")?,
            seq: number(seq, "sequence number")?,
            action: text(action, "action")?,
            now: number(now, "time")?,
            input: text(input, "input")?,
            effects: text(effects, "effects")?,
        })
    }

    /// The records of a trace: one per line, every line ended by a line
    /// feed. An input or the effects of a step may hold line feeds of
    /// their own, so the lines are found by the lengths of the records
    /// and not by searching for line feeds.
    pub fn decode_all(mut bytes: &[u8], limit: usize) -> Result<Vec<Record>, DecodeError> {
        let mut out = Vec::new();
        while !bytes.is_empty() {
            match sexp::parse_prefix(bytes, limit).map_err(|e| DecodeError(e.to_string()))? {
                sexp::Prefix::Incomplete => return Err(DecodeError("the trace ends inside a record".into())),
                sexp::Prefix::Value(_, used) => {
                    out.push(Record::decode(&bytes[..used], limit)?);
                    if bytes.get(used) != Some(&b'\n') {
                        return Err(DecodeError("a record is not followed by a line feed".into()));
                    }
                    bytes = &bytes[used + 1..];
                }
            }
        }
        Ok(out)
    }
}
