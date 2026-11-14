#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| ponko_fuzz::action_map(data));
