//! libFuzzer entry of the target `config_parser`; built by the package in `fuzz/libfuzzer/`.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| dp_fuzz::config_parser::run(data));
