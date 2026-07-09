#![no_main]
use libfuzzer_sys::fuzz_target;
use zion_pool::parse_fixed_hex;

// Fuzz the hex parser with arbitrary strings.
// Goal: ensure parse_fixed_hex never panics regardless of input.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = parse_fixed_hex::<32>(s, "fuzz");
        let _ = parse_fixed_hex::<2>(s, "fuzz");
        let _ = parse_fixed_hex::<64>(s, "fuzz");
    }
});
