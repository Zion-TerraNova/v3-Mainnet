//! Integration tests for ZION Bridge.
//!
//! Tests the full bridge flow: L1 lock → DB → validator consensus → status update.
//! These tests run against in-memory SQLite and simulate the bridge pipeline.

use chrono::Utc;
use tempfile::TempDir;

use zion_bridge::config::BridgeConfig;
use zion_bridge::db::BridgeDb;
use zion_bridge::metrics::BridgeMetrics;
use zion_bridge::types::conversion::*;
use zion_bridge::types::*;
use zion_bridge::validator::{ConsensusTracker, ValidatorSet};

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

fn setup_db() -> (BridgeDb, TempDir) {
    let dir = TempDir::new().unwrap();
    let db = BridgeDb::open(&dir.path().join("integration_test.db")).unwrap();
    (db, dir)
}

fn make_lock(tx_hash: &str, amount_zion: u64, chain: &str, recipient: &str) -> L1LockEvent {
    let atomic = amount_zion * 1_000_000; // V3 post-3.0.3: 6-decimal flowers
    L1LockEvent {
        l1_tx_hash: tx_hash.into(),
        l1_block_height: 1000,
        l1_sender: "zion1qsender_integration".into(),
        amount_flowers: atomic,
        amount_wzion_wei: flowers_to_wzion_wei(atomic),
        target_chain: chain.into(),
        evm_recipient: recipient.into(),
        detected_at: Utc::now(),
        status: BridgeStatus::Pending,
        confirmations: 0,
    }
}

fn make_burn(burn_id: &str, amount_wzion_wei_zion: u64, recipient: &str) -> EvmBurnEvent {
    let atomic = amount_wzion_wei_zion * 1_000_000; // V3 post-3.0.3: 6-decimal flowers
    EvmBurnEvent {
        evm_tx_hash: format!("0x{}", burn_id),
        evm_block_number: 50000,
        evm_chain: "base".into(),
        evm_burner: "0xBurnerAddress000000000000000000000000001".into(),
        amount_wzion_wei: flowers_to_wzion_wei(atomic),
        amount_flowers: atomic,
        l1_recipient: recipient.into(),
        burn_id: burn_id.into(),
        detected_at: Utc::now(),
        status: BridgeStatus::Pending,
        confirmations: 0,
    }
}

// ──────────────────────────────────────────────
// Test: Full L1→EVM lock flow
// ──────────────────────────────────────────────

#[test]
fn test_full_lock_flow_l1_to_evm() {
    let (db, _dir) = setup_db();

    // Step 1: L1 watcher detects lock TX
    let lock = make_lock(
        "lock_integ_001",
        5000, // 5000 ZION
        "base",
        "0x1234567890abcdef1234567890abcdef12345678",
    );
    assert_eq!(lock.amount_flowers, 5_000_000_000); // 5000 ZION × 1e6 (post-3.0.3)
    assert_eq!(lock.amount_wzion_wei, "5000000000000000000000"); // 5000 × 1e18

    db.insert_lock(&lock).unwrap();

    // Step 2: Verify lock is pending
    let pending = db.get_pending_locks().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].l1_tx_hash, "lock_integ_001");

    // Step 3: Validators confirm (3-of-5)
    let validators = ValidatorSet::new(
        3,
        vec![
            "0xV1".into(),
            "0xV2".into(),
            "0xV3".into(),
            "0xV4".into(),
            "0xV5".into(),
        ],
    );
    let mut tracker = ConsensusTracker::new("lock_integ_001".into(), validators.threshold);

    db.add_confirmation("lock", "lock_integ_001", "0xV1")
        .unwrap();
    assert!(!tracker.add_confirmation("0xV1")); // 1/3

    db.add_confirmation("lock", "lock_integ_001", "0xV2")
        .unwrap();
    assert!(!tracker.add_confirmation("0xV2")); // 2/3

    db.add_confirmation("lock", "lock_integ_001", "0xV3")
        .unwrap();
    assert!(tracker.add_confirmation("0xV3")); // 3/3 → consensus!

    assert_eq!(
        db.get_confirmation_count("lock", "lock_integ_001").unwrap(),
        3
    );
    assert!(tracker.reached);

    // Step 4: After EVM mint, mark as completed
    db.update_lock_status("lock_integ_001", BridgeStatus::Completed)
        .unwrap();

    // Step 5: Verify completed
    let pending = db.get_pending_locks().unwrap();
    assert_eq!(pending.len(), 0);
    let stats = db.get_stats().unwrap();
    assert_eq!(stats.total_operations, 1);
}

// ──────────────────────────────────────────────
// Test: Full EVM→L1 burn flow
// ──────────────────────────────────────────────

#[test]
fn test_full_burn_flow_evm_to_l1() {
    let (db, _dir) = setup_db();

    // Step 1: EVM watcher detects burn
    let burn = make_burn("burn_integ_001", 1000, "zion1qrecipient_burn");
    assert_eq!(burn.amount_flowers, 1_000_000_000); // 1000 ZION × 1e6 (post-3.0.3)

    db.insert_burn(&burn).unwrap();

    // Step 2: Verify pending
    let pending = db.get_pending_burns().unwrap();
    assert_eq!(pending.len(), 1);

    // Step 3: Validators confirm
    let mut tracker = ConsensusTracker::new("burn_integ_001".into(), 3);
    for addr in &["0xV1", "0xV2", "0xV3"] {
        db.add_confirmation("burn", "burn_integ_001", addr).unwrap();
        tracker.add_confirmation(addr);
    }
    assert!(tracker.reached);
    assert_eq!(
        db.get_confirmation_count("burn", "burn_integ_001").unwrap(),
        3
    );

    // Step 4: Unlock on L1, mark completed
    db.update_burn_status("burn_integ_001", BridgeStatus::Completed)
        .unwrap();

    let pending = db.get_pending_burns().unwrap();
    assert_eq!(pending.len(), 0);
    let stats = db.get_stats().unwrap();
    assert_eq!(stats.total_operations, 1);
}

// ──────────────────────────────────────────────
// Test: E2E Burn→Unlock request construction
// ──────────────────────────────────────────────

#[test]
fn test_e2e_burn_to_unlock_request() {
    let (db, _dir) = setup_db();

    // Step 1: EVM watcher detects burn of 500 ZION
    let burn = make_burn("burn_e2e_001", 500, "zion1qrecipient_unlock_e2e");
    assert_eq!(burn.amount_flowers, 500_000_000); // 500 ZION × 1e6 (post-3.0.3)
    assert_eq!(burn.l1_recipient, "zion1qrecipient_unlock_e2e");

    db.insert_burn(&burn).unwrap();

    // Step 2: 3-of-5 validator consensus
    let mut tracker = ConsensusTracker::new("burn_e2e_001".into(), 3);
    for addr in &["0xV1", "0xV2", "0xV3"] {
        db.add_confirmation("burn", "burn_e2e_001", addr).unwrap();
        tracker.add_confirmation(addr);
    }
    assert!(tracker.reached);

    // Step 3: Build unlock request JSON (same logic as relayer)
    let operation_message = format!(
        "unlock|recipient={}|amount={}|chain={}|burn_id={}|evm_tx={}",
        burn.l1_recipient, burn.amount_flowers, burn.evm_chain, burn.burn_id, burn.evm_tx_hash
    );

    // Simulate validator proofs (structure check)
    let validator_proofs: Vec<serde_json::Value> = vec![
        serde_json::json!({
            "validator_id": "validator-1",
            "validator_address": "0xdeadbeef00000000000000000000000000000001",
            "signature": format!("0x{}", "a".repeat(130)),
            "message_hash": format!("0x{}", "b".repeat(64)),
            "synthetic": false
        }),
        serde_json::json!({
            "validator_id": "validator-2",
            "validator_address": "0xdeadbeef00000000000000000000000000000002",
            "signature": format!("0x{}", "a".repeat(130)),
            "message_hash": format!("0x{}", "b".repeat(64)),
            "synthetic": false
        }),
        serde_json::json!({
            "validator_id": "validator-3",
            "validator_address": "0xdeadbeef00000000000000000000000000000003",
            "signature": format!("0x{}", "a".repeat(130)),
            "message_hash": format!("0x{}", "b".repeat(64)),
            "synthetic": false
        }),
    ];

    let unlock_request = serde_json::json!({
        "recipient": burn.l1_recipient,
        "amount_flowers": burn.amount_flowers,
        "amount_wzion_wei": burn.amount_wzion_wei,
        "evm_chain": burn.evm_chain,
        "burn_id": burn.burn_id,
        "evm_tx_hash": burn.evm_tx_hash,
        "validator_proofs": validator_proofs,
        "operation_message": operation_message,
    });

    // Step 4: Verify unlock request structure
    assert_eq!(
        unlock_request["recipient"].as_str().unwrap(),
        "zion1qrecipient_unlock_e2e"
    );
    assert_eq!(
        unlock_request["amount_flowers"].as_u64().unwrap(),
        500_000_000 // 500 ZION × 1e6 (post-3.0.3)
    );
    assert_eq!(unlock_request["evm_chain"].as_str().unwrap(), "base");
    assert_eq!(unlock_request["burn_id"].as_str().unwrap(), "burn_e2e_001");
    assert!(unlock_request["validator_proofs"].as_array().unwrap().len() >= 3);
    assert!(
        !validator_proofs
            .iter()
            .any(|p| p["synthetic"].as_bool().unwrap()),
        "no synthetic proofs allowed"
    );

    // Step 5: Verify operation_message format
    assert!(operation_message.contains("unlock|recipient="));
    assert!(operation_message.contains("|amount=500000000|")); // 500 ZION × 1e6 flowers
    assert!(operation_message.contains("|chain=base|"));
    assert!(operation_message.contains("|burn_id=burn_e2e_001|"));

    // Step 6: Mark completed and verify DB state
    db.update_burn_status("burn_e2e_001", BridgeStatus::Completed)
        .unwrap();
    let pending = db.get_pending_burns().unwrap();
    assert_eq!(pending.len(), 0);
    let stats = db.get_stats().unwrap();
    assert_eq!(stats.total_operations, 1);
}

// ──────────────────────────────────────────────
// Test: Decimal conversion roundtrip (full flow)
// ──────────────────────────────────────────────

#[test]
fn test_decimal_conversion_full_roundtrip() {
    // Simulate: user locks 1234.567890 ZION on L1 (post-3.0.3: 6 decimals)
    let zion_amount = "1234.567890";
    let parts: Vec<&str> = zion_amount.split('.').collect();
    let whole: u64 = parts[0].parse().unwrap();
    let frac: u64 = parts[1].parse().unwrap();
    // 6-decimal flowers: 1234 ZION = 1234e6 flowers, 0.567890 ZION = 567890 flowers
    let atomic = whole * 1_000_000 + frac; // V3 post-3.0.3: 6-decimal flowers
    assert_eq!(atomic, 1_234_567_890);

    // Convert to wZION wei
    let wzion_wei = flowers_to_wzion_wei(atomic);
    assert_eq!(wzion_wei, "1234567890000000000000"); // 1234.56789 × 1e18

    // Simulate burn: convert back
    let recovered = wzion_wei_to_flowers(&wzion_wei).unwrap();
    assert_eq!(recovered, atomic);

    // Display (6-decimal precision)
    let display = flowers_to_zion_display(recovered);
    assert!(
        display.starts_with("1234.56789"),
        "display should show 1234.56789..., got {display}"
    );
}

// ──────────────────────────────────────────────
// Test: Security limits
// ──────────────────────────────────────────────

#[test]
fn test_security_limits() {
    let config = BridgeConfig::default();

    let min: u128 = config.security.min_bridge_amount.parse().unwrap();
    let max_single: u128 = config.security.max_single_amount.parse().unwrap();
    let daily: u128 = config.security.daily_limit.parse().unwrap();
    let timelock: u128 = config.security.timelock_threshold.parse().unwrap();

    // 100 wZION minimum
    let min_zion = min / 1_000_000_000_000_000_000;
    assert_eq!(min_zion, 100);

    // 5M single max
    let max_zion = max_single / 1_000_000_000_000_000_000;
    assert_eq!(max_zion, 5_000_000);

    // 10M daily limit
    let daily_zion = daily / 1_000_000_000_000_000_000;
    assert_eq!(daily_zion, 10_000_000);

    // 1M timelock threshold
    let timelock_zion = timelock / 1_000_000_000_000_000_000;
    assert_eq!(timelock_zion, 1_000_000);

    // Invariants
    assert!(min < timelock, "Min < timelock");
    assert!(timelock < max_single, "Timelock < max single");
    assert!(max_single < daily, "Max single < daily limit");
}

// ──────────────────────────────────────────────
// Test: Amount validation (below min / above max)
// ──────────────────────────────────────────────

#[test]
fn test_amount_validation_ranges() {
    let config = BridgeConfig::default();
    let min: u128 = config.security.min_bridge_amount.parse().unwrap();
    let max: u128 = config.security.max_single_amount.parse().unwrap();

    // Below minimum (50 wZION) — post-3.0.3: 50 ZION = 50e6 flowers
    let small = flowers_to_wzion_wei(50_000_000); // 50 ZION
    let small_wei: u128 = small.parse().unwrap();
    assert!(small_wei < min, "50 ZION should be below 100 wZION minimum");

    // Valid amount (1000 wZION) — 1000 ZION = 1000e6 flowers
    let normal = flowers_to_wzion_wei(1_000_000_000); // 1000 ZION
    let normal_wei: u128 = normal.parse().unwrap();
    assert!(
        normal_wei >= min && normal_wei <= max,
        "1000 ZION should be in valid range"
    );

    // Above single max (6M wZION) — 6M ZION = 6e6 × 1e6 flowers
    let big = flowers_to_wzion_wei(6_000_000_000_000); // 6M ZION = 6e6 × 1e6 flowers
    let big_wei: u128 = big.parse().unwrap();
    assert!(big_wei > max, "6M ZION should exceed 5M single max");
}

// ──────────────────────────────────────────────
// Test: Timelock detection
// ──────────────────────────────────────────────

#[test]
fn test_timelock_detection() {
    let config = BridgeConfig::default();
    let timelock_threshold: u128 = config.security.timelock_threshold.parse().unwrap();

    // 500K ZION — no timelock — post-3.0.3: 500K × 1e6 flowers
    let amount_500k = flowers_to_wzion_wei(500_000_000_000u64); // 500K × 1e6 flowers
    let wei_500k: u128 = amount_500k.parse().unwrap();
    assert!(
        wei_500k < timelock_threshold,
        "500K should not trigger timelock"
    );

    // 2M ZION — timelock! — post-3.0.3: 2M × 1e6 flowers
    let amount_2m = flowers_to_wzion_wei(2_000_000_000_000u64); // 2M × 1e6 flowers
    let wei_2m: u128 = amount_2m.parse().unwrap();
    assert!(
        wei_2m > timelock_threshold,
        "2M should trigger 24h timelock"
    );
}

// ──────────────────────────────────────────────
// Test: Validator consensus edge cases
// ──────────────────────────────────────────────

#[test]
fn test_validator_consensus_exact_threshold() {
    let set = ValidatorSet::new(
        3,
        vec![
            "0xA".into(),
            "0xB".into(),
            "0xC".into(),
            "0xD".into(),
            "0xE".into(),
        ],
    );

    let mut tracker = ConsensusTracker::new("op_exact".into(), set.threshold);

    // Confirm exactly threshold
    assert!(!tracker.add_confirmation("0xA"));
    assert!(!tracker.add_confirmation("0xB"));
    assert!(tracker.add_confirmation("0xC")); // Exactly 3 = threshold

    assert!(tracker.reached);
    assert_eq!(tracker.confirmation_count(), 3);
}

#[test]
fn test_validator_duplicate_confirmation() {
    let mut tracker = ConsensusTracker::new("op_dup".into(), 3);

    tracker.add_confirmation("0xA");
    tracker.add_confirmation("0xA"); // duplicate
    tracker.add_confirmation("0xA"); // duplicate again

    assert_eq!(tracker.confirmation_count(), 1);
    assert!(!tracker.reached);
}

#[test]
fn test_validator_set_case_insensitive() {
    let set = ValidatorSet::new(2, vec!["0xAbCd".into(), "0x1234".into()]);
    assert!(set.is_validator("0xABCD")); // uppercase
    assert!(set.is_validator("0xabcd")); // lowercase
    assert!(!set.is_validator("0x9999"));
}

// ──────────────────────────────────────────────
// Test: DB persistence across operations
// ──────────────────────────────────────────────

#[test]
fn test_db_persistence_state() {
    let (db, _dir) = setup_db();

    // Set various state keys
    db.set_state("last_l1_height", "12345").unwrap();
    db.set_state("bridge_status", "running").unwrap();
    db.set_state("daily_volume", "500000000000").unwrap();

    assert_eq!(db.get_state("last_l1_height").unwrap().unwrap(), "12345");
    assert_eq!(db.get_state("bridge_status").unwrap().unwrap(), "running");
    assert_eq!(
        db.get_state("daily_volume").unwrap().unwrap(),
        "500000000000"
    );
    assert!(db.get_state("nonexistent").unwrap().is_none());
}

// ──────────────────────────────────────────────
// Test: Metrics tracking during operations
// ──────────────────────────────────────────────

#[test]
fn test_metrics_during_bridge_flow() {
    use std::sync::atomic::Ordering;

    let metrics = BridgeMetrics::new();

    // Simulate lock detection
    metrics.l1_locks_detected.fetch_add(1, Ordering::Relaxed);
    metrics.last_l1_height.store(1000, Ordering::Relaxed);

    // Simulate finalization
    metrics.l1_locks_finalized.fetch_add(1, Ordering::Relaxed);

    // Simulate mint
    metrics.evm_mints_submitted.fetch_add(1, Ordering::Relaxed);
    metrics.evm_mints_confirmed.fetch_add(1, Ordering::Relaxed);

    // Simulate burn
    metrics.evm_burns_detected.fetch_add(1, Ordering::Relaxed);
    metrics.l1_unlocks_submitted.fetch_add(1, Ordering::Relaxed);
    metrics.l1_unlocks_confirmed.fetch_add(1, Ordering::Relaxed);
    metrics.last_evm_block.store(50000, Ordering::Relaxed);

    let snap = metrics.snapshot();
    assert_eq!(snap.l1_locks_detected, 1);
    assert_eq!(snap.l1_locks_finalized, 1);
    assert_eq!(snap.evm_mints_submitted, 1);
    assert_eq!(snap.evm_mints_confirmed, 1);
    assert_eq!(snap.evm_burns_detected, 1);
    assert_eq!(snap.l1_unlocks_submitted, 1);
    assert_eq!(snap.l1_unlocks_confirmed, 1);
    assert_eq!(snap.last_l1_height, 1000);
    assert_eq!(snap.last_evm_block, 50000);
    assert_eq!(snap.errors, 0);
}

// ──────────────────────────────────────────────
// Test: Multi-chain support
// ──────────────────────────────────────────────

#[test]
fn test_multi_chain_locks() {
    let (db, _dir) = setup_db();

    // Lock to Base
    db.insert_lock(&make_lock(
        "tx_base",
        1000,
        "base",
        "0x1111111111111111111111111111111111111111",
    ))
    .unwrap();
    // Lock to Arbitrum
    db.insert_lock(&make_lock(
        "tx_arb",
        2000,
        "arbitrum",
        "0x2222222222222222222222222222222222222222",
    ))
    .unwrap();
    // Lock to BSC
    db.insert_lock(&make_lock(
        "tx_bsc",
        3000,
        "bsc",
        "0x3333333333333333333333333333333333333333",
    ))
    .unwrap();

    let pending = db.get_pending_locks().unwrap();
    assert_eq!(pending.len(), 3);

    // Verify different chains
    let chains: Vec<String> = pending.iter().map(|l| l.target_chain.clone()).collect();
    assert!(chains.contains(&"base".to_string()));
    assert!(chains.contains(&"arbitrum".to_string()));
    assert!(chains.contains(&"bsc".to_string()));
}

// ──────────────────────────────────────────────
// Test: Supply invariant (locked ≥ outstanding)
// ──────────────────────────────────────────────

#[test]
fn test_supply_invariant() {
    // Simulate: 3 locks (total 10K ZION), 1 burn (1K ZION)
    // locked = 10K, minted = 10K, burned = 1K, outstanding = 9K
    // invariant: locked (10K) ≥ outstanding (9K) ✓

    let lock_amounts: Vec<u64> = vec![3_000, 5_000, 2_000]; // ZION
    let burn_amount: u64 = 1_000;

    let total_locked: u64 = lock_amounts.iter().sum();
    let total_minted = total_locked; // Each lock mints equal wZION
    let outstanding = total_minted - burn_amount;

    assert_eq!(total_locked, 10_000);
    assert_eq!(outstanding, 9_000);
    assert!(total_locked >= outstanding, "Supply invariant violated!");

    // Verify conversion consistency
    for amount in &lock_amounts {
        let atomic = amount * 1_000_000; // V3 post-3.0.3: 6-decimal flowers
        let wzion = flowers_to_wzion_wei(atomic);
        let back = wzion_wei_to_flowers(&wzion).unwrap();
        assert_eq!(atomic, back, "Roundtrip failed for {} ZION", amount);
    }
}

// ──────────────────────────────────────────────
// Test: Bridge status state machine
// ──────────────────────────────────────────────

#[test]
fn test_bridge_status_transitions() {
    let (db, _dir) = setup_db();
    let lock = make_lock(
        "tx_states",
        100,
        "base",
        "0x1234567890abcdef1234567890abcdef12345678",
    );
    db.insert_lock(&lock).unwrap();

    // Pending → Confirmed
    db.update_lock_status("tx_states", BridgeStatus::Confirmed)
        .unwrap();
    assert_eq!(db.count_by_status("l1_locks", "Confirmed").unwrap(), 1);

    // Confirmed → Executing
    db.update_lock_status("tx_states", BridgeStatus::Executing)
        .unwrap();
    assert_eq!(db.count_by_status("l1_locks", "Executing").unwrap(), 1);

    // Executing → Completed
    db.update_lock_status("tx_states", BridgeStatus::Completed)
        .unwrap();
    assert_eq!(db.count_by_status("l1_locks", "Completed").unwrap(), 1);

    let stats = db.get_stats().unwrap();
    assert_eq!(stats.total_operations, 1);
}

// ──────────────────────────────────────────────
// Test: Timelock flow
// ──────────────────────────────────────────────

#[test]
fn test_timelock_flow() {
    let (db, _dir) = setup_db();

    // Large lock — 2M ZION → should be timelocked
    let lock = make_lock(
        "tx_timelock",
        2_000_000,
        "base",
        "0x1234567890abcdef1234567890abcdef12345678",
    );
    db.insert_lock(&lock).unwrap();

    let config = BridgeConfig::default();
    let timelock_threshold: u128 = config.security.timelock_threshold.parse().unwrap();
    let lock_wei: u128 = lock.amount_wzion_wei.parse().unwrap();

    assert!(
        lock_wei > timelock_threshold,
        "2M should exceed 1M timelock threshold"
    );

    // Set to Timelocked
    db.update_lock_status("tx_timelock", BridgeStatus::Timelocked)
        .unwrap();
    assert_eq!(db.count_by_status("l1_locks", "Timelocked").unwrap(), 1);

    // After 24h delay → Completed
    db.update_lock_status("tx_timelock", BridgeStatus::Completed)
        .unwrap();
    assert_eq!(db.count_by_status("l1_locks", "Completed").unwrap(), 1);
}

// ──────────────────────────────────────────────
// Test: Config → TOML → Config roundtrip
// ──────────────────────────────────────────────

#[test]
fn test_config_toml_roundtrip() {
    let config = BridgeConfig::default();
    let toml_str = toml::to_string_pretty(&config).unwrap();

    // Should be deserializable back
    let recovered: BridgeConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(recovered.bridge.name, config.bridge.name);
    assert_eq!(recovered.l1.finality_blocks, config.l1.finality_blocks);
    assert_eq!(recovered.validator.threshold, config.validator.threshold);
    assert_eq!(recovered.security.daily_limit, config.security.daily_limit);
}
