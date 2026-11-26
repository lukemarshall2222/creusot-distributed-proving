//! libFuzzer entry of the target `trace_record`; built by the package in `fuzz/libfuzzer/`.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| dp_fuzz::trace_record::run(data));
