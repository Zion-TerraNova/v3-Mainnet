#![no_main]
use libfuzzer_sys::fuzz_target;
use zion_core::validation::{validate_structure, validate_timestamp};
use zion_core::Transaction;

// Fuzz block-header validation functions with arbitrary numeric inputs.
// Goal: ensure validate_structure and validate_timestamp never panic.
fuzz_target!(|data: &[u8]| {
    if data.len() < 32 {
        return;
    }

    // Extract fuzzer-driven parameters from raw bytes
    let block_size = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
    let timestamp = u64::from_le_bytes(data[8..16].try_into().unwrap());
    let median_time = u64::from_le_bytes(data[16..24].try_into().unwrap());
    let current_time = u64::from_le_bytes(data[24..32].try_into().unwrap());
    let tx_count = (data.get(32).copied().unwrap_or(0) % 4) as usize;

    // Build a dummy transaction list (content doesn't matter for structure check)
    let txs: Vec<Transaction> = (0..tx_count)
        .map(|i| Transaction {
            tx_id: format!("fuzz_tx_{i}"),
            from: format!("fuzz_from_{i}"),
            to: format!("fuzz_to_{i}"),
            amount_zion: 1,
            fee_zion: 0,
            nonce: i as u64,
        })
        .collect();

    let _ = validate_structure(&txs, block_size);
    let _ = validate_timestamp(timestamp, median_time, current_time);
});
