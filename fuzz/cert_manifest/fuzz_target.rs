//! libFuzzer entry of the target `cert_manifest`; built by the package in `fuzz/libfuzzer/`.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| dp_fuzz::cert_manifest::run(data));
