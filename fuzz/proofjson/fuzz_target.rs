//! libFuzzer entry of the target `proofjson`; built by the package in `fuzz/libfuzzer/`.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| dp_fuzz::proofjson::run(data));
