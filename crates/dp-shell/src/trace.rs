//! Event traces (R1.5 criteria 10, 18 and 19; design decision D-11).
//!
//! Every step of a machine writes one record: the component, its sequence
//! number, the TLA+ action and the input. With the bounded retention setting
//! only the most recent records are kept; with the complete setting every
//! record is written.

use std::{
    collections::VecDeque,
    io,
    path::PathBuf,
};

use dp_formats::sexp::{self, Sexp};

use crate::store::Files;

/// The tracing retention setting of a component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retention {
    /// Keep the most recent records; the number is `<TBD-46>`.
    Bounded(usize),
    /// Keep every record. CI and evaluation runs set this.
    Complete,
}

/// How many bytes of records a complete trace holds back before it appends
/// them to its file.
const FLUSH: usize = 8192;

pub struct TraceWriter {
    component: String,
    seq: u64,
    sink: Sink,
}

enum Sink {
    None,
    /// The most recent records, kept in memory only: the default, so that
    /// a component that fails can say what it did last (D-11).
    Memory { keep: usize, ring: VecDeque<Vec<u8>> },
    /// Every record, appended to a file. Records wait in `buffer` until
    /// there are `FLUSH` bytes of them or the writer is finished.
    Complete { path: PathBuf, buffer: Vec<u8> },
    Bounded { path: PathBuf, keep: usize, ring: VecDeque<Vec<u8>> },
}

impl TraceWriter {
    /// A writer that keeps nothing.
    pub fn off(component: &str) -> TraceWriter {
        TraceWriter { component: component.to_string(), seq: 0, sink: Sink::None }
    }

    /// A writer that keeps the most recent `keep` records in memory. What
    /// it keeps is read with `kept`; nothing is written anywhere.
    pub fn in_memory(component: &str, keep: usize) -> TraceWriter {
        TraceWriter { component: component.to_string(), seq: 0, sink: Sink::Memory { keep, ring: VecDeque::new() } }
    }

    /// The records that a bounded writer holds, oldest first, one per line.
    pub fn kept(&self) -> Vec<u8> {
        match &self.sink {
            Sink::Memory { ring, .. } | Sink::Bounded { ring, .. } => ring.iter().flatten().copied().collect(),
            Sink::None | Sink::Complete { .. } => Vec::new(),
        }
    }

    /// Number of steps recorded so far, kept or not.
    pub fn steps(&self) -> u64 {
        self.seq
    }

    /// A writer to a file of the component's file store. The complete
    /// setting appends every record to it; the bounded one writes the
    /// records that it kept when the writer is finished.
    pub fn to_file(env: &mut impl Files, component: &str, path: PathBuf, retention: Retention) -> io::Result<TraceWriter> {
        let sink = match retention {
            Retention::Complete => {
                // The file starts empty.
                env.write_file(&path, b"")?;
                Sink::Complete { path, buffer: Vec::new() }
            }
            Retention::Bounded(keep) => Sink::Bounded { path, keep, ring: VecDeque::new() },
        };
        Ok(TraceWriter { component: component.to_string(), seq: 0, sink })
    }

    /// Whether records are kept at all.
    pub fn enabled(&self) -> bool {
        !matches!(self.sink, Sink::None)
    }

    /// Writes the record of one step, before its effects are carried out.
    pub fn record(&mut self, env: &mut impl Files, action: &str, now: u64, input: &str) -> io::Result<()> {
        self.record_step(env, action, now, input, "")
    }

    /// Writes the record of one step with the effects that it returned,
    /// which trace validation needs (R1.5 criterion 10).
    pub fn record_step(
        &mut self,
        env: &mut impl Files,
        action: &str,
        now: u64,
        input: &str,
        effects: &str,
    ) -> io::Result<()> {
        self.seq += 1;
        if matches!(self.sink, Sink::None) {
            return Ok(());
        }
        let mut line = sexp::print(&Sexp::list([
            Sexp::atom("trace"),
            Sexp::atom(&self.component),
            Sexp::atom(self.seq.to_string()),
            Sexp::atom(action),
            Sexp::atom(now.to_string()),
            Sexp::atom(input),
            Sexp::atom(effects),
        ]));
        line.push(b'\n');
        match &mut self.sink {
            Sink::None => Ok(()),
            Sink::Complete { path, buffer } => {
                buffer.extend_from_slice(&line);
                if buffer.len() < FLUSH {
                    return Ok(());
                }
                env.append_file(path, &std::mem::take(buffer))
            }
            Sink::Bounded { keep, ring, .. } | Sink::Memory { keep, ring } => {
                ring.push_back(line);
                while ring.len() > *keep {
                    ring.pop_front();
                }
                Ok(())
            }
        }
    }

    /// Flushes the records that are kept.
    pub fn finish(&mut self, env: &mut impl Files) -> io::Result<()> {
        match &mut self.sink {
            Sink::None | Sink::Memory { .. } => Ok(()),
            Sink::Complete { path, buffer } => env.append_file(path, &std::mem::take(buffer)),
            Sink::Bounded { path, ring, .. } => {
                let kept: Vec<u8> = ring.iter().flatten().copied().collect();
                env.write_file(path, &kept)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::OsFiles;
    use std::fs;

    fn lines(path: &PathBuf) -> Vec<String> {
        fs::read_to_string(path).unwrap().lines().map(str::to_string).collect()
    }

    // P-ID: P1.5.15
    #[test]
    fn bounded_keeps_the_last_records_in_order_and_complete_keeps_all() {
        let dir = std::env::temp_dir().join(format!("dp-trace-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut env = OsFiles;
        // 3 stands in for <TBD-46>; it is a test value.
        for (n, keep) in [(10usize, 3usize), (2, 3), (0, 3)] {
            let bounded = dir.join("bounded");
            let complete = dir.join("complete");
            let mut b = TraceWriter::to_file(&mut env, "fe", bounded.clone(), Retention::Bounded(keep)).unwrap();
            let mut c = TraceWriter::to_file(&mut env, "fe", complete.clone(), Retention::Complete).unwrap();
            for i in 0..n {
                b.record(&mut env, "Tick", i as u64, "x").unwrap();
                c.record(&mut env, "Tick", i as u64, "x").unwrap();
            }
            b.finish(&mut env).unwrap();
            c.finish(&mut env).unwrap();
            let all = lines(&complete);
            assert_eq!(all.len(), n);
            assert_eq!(lines(&bounded), all[n.saturating_sub(keep)..].to_vec());
        }
        fs::remove_dir_all(dir).unwrap();
    }

    // P-ID: P1.5.15
    #[test]
    fn the_default_writer_keeps_the_last_records_in_memory() {
        let mut env = OsFiles;
        let mut t = TraceWriter::in_memory("fe", 3);
        assert!(t.enabled() && t.kept().is_empty());
        for i in 0..10u64 {
            t.record(&mut env, "Tick", i, "x").unwrap();
        }
        t.finish(&mut env).unwrap();
        assert_eq!(t.steps(), 10);
        let kept = String::from_utf8(t.kept()).unwrap();
        let lines: Vec<&str> = kept.lines().collect();
        assert_eq!(lines.len(), 3);
        // The sequence numbers of the last three steps, in order.
        for (line, seq) in lines.iter().zip(["8", "9", "10"]) {
            let record = sexp::parse(line.as_bytes(), 1 << 16).unwrap();
            assert_eq!(record.as_list().unwrap()[2].as_str(), Some(seq), "{line}");
        }
        // A writer that is off keeps nothing, and still counts.
        let mut off = TraceWriter::off("fe");
        off.record(&mut env, "Tick", 0, "x").unwrap();
        assert!(!off.enabled() && off.kept().is_empty() && off.steps() == 1);
    }
}
