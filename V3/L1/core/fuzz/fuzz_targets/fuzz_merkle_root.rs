#![no_main]
use libfuzzer_sys::fuzz_target;
use zion_core::validation::merkle_root;

// Fuzz the Merkle-root computation with arbitrary hash-sized slices.
// Goal: ensure merkle_root never panics on any number of inputs.
fuzz_target!(|data: &[u8]| {
    // Split fuzzer bytes into 32-byte chunks to form tx hash slices
    let hashes: Vec<[u8; 32]> = data
        .chunks_exact(32)
        .map(|chunk| {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(chunk);
            arr
        })
        .collect();
    let _ = merkle_root(&hashes);
});
