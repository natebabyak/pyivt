#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| pyivt::fuzzing::metadata_directory(data));
