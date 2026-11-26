// Feature: creusot-distributed-proving, Property 34: Every exchanged format has one grammar and one strict codec
// P-ID: P7.10.5 (no input makes a parser panic; what a parser accepts is canonical)

//! `ci/fuzz-smoke`: every harness on its seeds, on mutations of its seeds
//! and on random bytes. The inputs come from a fixed generator, so a
//! failure is the same on every machine.

use dp_fuzz::TARGETS;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// One change of the kind that a fuzzer makes.
fn mutate(input: &[u8], other: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut out = input.to_vec();
    let at = rng.below(out.len() + 1);
    let last = at.min(out.len().saturating_sub(1));
    match rng.below(10) {
        0 if !out.is_empty() => out[last] ^= 1 << rng.below(8),
        1 if !out.is_empty() => out[last] = rng.next() as u8,
        2 => out.truncate(at),
        3 => {
            let end = (at + 1 + rng.below(8)).min(out.len());
            out.drain(at.min(end)..end);
        }
        4 => {
            for _ in 0..1 + rng.below(4) {
                out.insert(at, rng.next() as u8);
            }
        }
        5 => {
            // A piece of another seed, in place of the rest.
            out.truncate(at);
            out.extend_from_slice(&other[rng.below(other.len() + 1)..]);
        }
        6 => {
            // A length prefix or a number: one digit changed.
            if let Some(i) = (0..out.len()).cycle().skip(at).take(out.len()).find(|i| out[*i].is_ascii_digit()) {
                out[i] = b'0' + rng.below(10) as u8;
            }
        }
        7 => {
            let end = (at + 1 + rng.below(16)).min(out.len());
            let piece = out[at.min(end)..end].to_vec();
            out.splice(at..at, piece);
        }
        8 => out.extend_from_slice(b")"),
        _ => {
            // Structure characters of the formats.
            let c = b"():\"[]=\n #"[rng.below(10)];
            out.insert(at, c);
        }
    }
    out
}

#[test]
fn no_input_breaks_a_parser() {
    for (name, run, seeds) in TARGETS {
        let seeds = seeds();
        assert!(!seeds.is_empty(), "{name}: no seed");
        // `sched/fuzz-nightly` runs longer and from another start:
        // DP_FUZZ_ROUNDS multiplies the number of inputs, DP_FUZZ_SEED
        // changes the generator.
        let number = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u64>().ok());
        let rounds = number("DP_FUZZ_ROUNDS").unwrap_or(1).max(1) as usize;
        let mut rng = Rng((0x9e37_79b9_7f4a_7c15 ^ name.len() as u64 ^ number("DP_FUZZ_SEED").unwrap_or(0).rotate_left(17)) | 1);
        let mut inputs = 0u64;
        let mut try_one = |input: &[u8]| {
            inputs += 1;
            if std::panic::catch_unwind(|| run(input)).is_err() {
                panic!("{name}: the harness failed on {:?}", String::from_utf8_lossy(input));
            }
        };
        for seed in &seeds {
            try_one(seed);
        }
        // Mutations of the seeds, up to four deep.
        let per_seed = rounds * if name == "cert_manifest" { 400 } else { 3_000 };
        for (i, seed) in seeds.iter().enumerate() {
            for _ in 0..per_seed {
                let mut input = seed.clone();
                for _ in 0..1 + rng.below(4) {
                    let other = &seeds[rng.below(seeds.len())];
                    input = mutate(&input, other, &mut rng);
                }
                try_one(&input);
            }
            let _ = i;
        }
        // Bytes with no structure at all, and with the alphabet of the formats.
        for _ in 0..2_000 * rounds {
            let length = rng.below(64);
            let alphabet = rng.below(2) == 0;
            let input: Vec<u8> = (0..length)
                .map(|_| if alphabet { b"()0123456789:ab \n\"=[]"[rng.below(21)] } else { rng.next() as u8 })
                .collect();
            try_one(&input);
        }
        eprintln!("fuzz-smoke: {name}: {inputs} inputs");
    }
}

/// The seeds are what the harnesses are for: each is accepted by its
/// parser, so the mutations start from inside the format.
#[test]
fn the_seeds_are_well_formed() {
    use dp_formats::{cache::Entry, cluster::Message, proofjson::DpEntry, sexp};
    for seed in dp_fuzz::result_message::seeds() {
        assert!(Message::decode(&sexp::parse(&seed, dp_fuzz::LIMIT).unwrap()).is_ok());
    }
    for seed in dp_fuzz::cache_entry::seeds() {
        assert!(Entry::decode(&seed, dp_fuzz::LIMIT).is_ok());
    }
    let proofjson = dp_fuzz::proofjson::seeds();
    assert!(proofjson.iter().filter(|s| DpEntry::decode(s, dp_fuzz::LIMIT).is_ok()).count() >= 3);
    assert!(proofjson.iter().filter(|s| std::str::from_utf8(s).is_ok_and(|t| DpEntry::from_member(t, dp_fuzz::LIMIT).is_ok())).count() >= 3);
}
