//! libFuzzer entry of the target `lock_file`; built by the package in `fuzz/libfuzzer/`.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| dp_fuzz::lock_file::run(data));
