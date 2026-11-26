//! libFuzzer entry of the target `checker_output`; built by the package in `fuzz/libfuzzer/`.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| dp_fuzz::checker_output::run(data));
