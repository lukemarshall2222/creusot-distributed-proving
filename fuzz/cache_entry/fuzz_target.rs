//! libFuzzer entry of the target `cache_entry`; built by the package in `fuzz/libfuzzer/`.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| dp_fuzz::cache_entry::run(data));
