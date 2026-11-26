//! `cargo run -p dp-canaries --example bless`: writes `creusot/mutants.rs`
//! as the specification mutator makes it from `creusot/originals.rs`.

fn main() {
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let originals = std::fs::read_to_string(here.join("creusot/originals.rs")).expect("creusot/originals.rs");
    std::fs::write(here.join("creusot/mutants.rs"), dp_canaries::mutator::generate(&originals)).expect("creusot/mutants.rs");
}
