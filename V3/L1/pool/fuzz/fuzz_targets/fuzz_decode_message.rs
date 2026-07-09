#![no_main]
use libfuzzer_sys::fuzz_target;
use zion_pool::decode_message;

// Fuzz the pool wire-protocol decoder with arbitrary byte sequences.
// Goal: ensure decode_message never panics on any input.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = decode_message(s);
    }
});
