//! Event traces (R1.5 criteria 10, 18 and 19; design decision D-11).
//!
//! Every step of a machine writes one record: the component, its sequence
//! number, the TLA+ action and the input. With the bounded retention setting
//! only the most recent records are kept; with the complete setting every
//! record is written.

use std::{
    collections::VecDeque,
    fs,
    io::{self, BufWriter, Write},
    path::PathBuf,
};

use dp_formats::sexp::{self, Sexp};

/// The tracing retention setting of a component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retention {
    /// Keep the most recent records; the number is `<TBD-46>`.
    Bounded(usize),
    /// Keep every record. CI and evaluation runs set this.
    Complete,
}

pub struct TraceWriter {
    component: String,
    seq: u64,
    sink: Sink,
}

enum Sink {
    None,
    Complete(BufWriter<fs::File>),
    Bounded { path: PathBuf, keep: usize, ring: VecDeque<Vec<u8>> },
}

impl TraceWriter {
    /// A writer that keeps nothing.
    pub fn off(component: &str) -> TraceWriter {
        TraceWriter { component: component.to_string(), seq: 0, sink: Sink::None }
    }

    pub fn to_file(component: &str, path: PathBuf, retention: Retention) -> io::Result<TraceWriter> {
        let sink = match retention {
            Retention::Complete => Sink::Complete(BufWriter::new(fs::File::create(&path)?)),
            Retention::Bounded(keep) => Sink::Bounded { path, keep, ring: VecDeque::new() },
        };
        Ok(TraceWriter { component: component.to_string(), seq: 0, sink })
    }

    /// Whether records are kept at all.
    pub fn enabled(&self) -> bool {
        !matches!(self.sink, Sink::None)
    }

    /// Writes the record of one step, before its effects are carried out.
    pub fn record(&mut self, action: &str, now: u64, input: &str) -> io::Result<()> {
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
        ]));
        line.push(b'\n');
        match &mut self.sink {
            Sink::None => Ok(()),
            Sink::Complete(file) => file.write_all(&line),
            Sink::Bounded { keep, ring, .. } => {
                ring.push_back(line);
                while ring.len() > *keep {
                    ring.pop_front();
                }
                Ok(())
            }
        }
    }

    /// Flushes the records that are kept.
    pub fn finish(&mut self) -> io::Result<()> {
        match &mut self.sink {
            Sink::None => Ok(()),
            Sink::Complete(file) => file.flush(),
            Sink::Bounded { path, ring, .. } => {
                let mut file = BufWriter::new(fs::File::create(&*path)?);
                for line in ring.iter() {
                    file.write_all(line)?;
                }
                file.flush()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(path: &PathBuf) -> Vec<String> {
        fs::read_to_string(path).unwrap().lines().map(str::to_string).collect()
    }

    // P-ID: P1.5.15
    #[test]
    fn bounded_keeps_the_last_records_in_order_and_complete_keeps_all() {
        let dir = std::env::temp_dir().join(format!("dp-trace-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // 3 stands in for <TBD-46>; it is a test value.
        for (n, keep) in [(10usize, 3usize), (2, 3), (0, 3)] {
            let bounded = dir.join("bounded");
            let complete = dir.join("complete");
            let mut b = TraceWriter::to_file("fe", bounded.clone(), Retention::Bounded(keep)).unwrap();
            let mut c = TraceWriter::to_file("fe", complete.clone(), Retention::Complete).unwrap();
            for i in 0..n {
                b.record("Tick", i as u64, "x").unwrap();
                c.record("Tick", i as u64, "x").unwrap();
            }
            b.finish().unwrap();
            c.finish().unwrap();
            let all = lines(&complete);
            assert_eq!(all.len(), n);
            assert_eq!(lines(&bounded), all[n.saturating_sub(keep)..].to_vec());
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
