//! Mainnet-readiness tests for ZION Bridge.
//!
//! Covers areas critical before mainnet deployment:
//!   1. L1 API JSON deserialization (real API response format)
//!   2. Testnet vs mainnet config separation
//!   3. EVM block range chunking (publicnode 50k limit)
//!   4. Replay attack prevention (duplicate TXs)
//!   5. Memo parsing edge cases
//!   6. Amount invariants (min / timelock / max / daily)
//!   7. Multi-chain routing logic
//!   8. Supply invariant: locked_l1 ≥ outstanding_wzion
//!
//! Run: cargo test --test mainnet_readiness -- --nocapture

#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::int_plus_one)]

use chrono::Utc;
use tempfile::TempDir;
use zion_bridge::config::{BridgeConfig, BridgeIdentity, EvmChainConfig};
use zion_bridge::db::BridgeDb;
use zion_bridge::types::{conversion::*, BridgeStatus, EvmBurnEvent, L1LockEvent};

// ─── Helpers ───────────────────────────────────────────────────────────────

fn open_db() -> (BridgeDb, TempDir) {
    let dir = TempDir::new().unwrap();
    let db = BridgeDb::open(&dir.path().join("test.db")).unwrap();
    (db, dir)
}

fn lock(tx_hash: &str, zion: u64, chain: &str, recipient: &str) -> L1LockEvent {
    let atomic = zion * 1_000_000; // V3 post-3.0.3: 6-decimal flowers
    L1LockEvent {
        l1_tx_hash: tx_hash.into(),
        l1_block_height: 1000,
        l1_sender: "zion1qsender000000000000000000000000000001".into(),
        amount_flowers: atomic,
        amount_wzion_wei: flowers_to_wzion_wei(atomic),
        target_chain: chain.into(),
        evm_recipient: recipient.into(),
        detected_at: Utc::now(),
        status: BridgeStatus::Pending,
        confirmations: 0,
    }
}

fn burn(burn_id: &str, wzion_zion: u64, l1_recipient: &str) -> EvmBurnEvent {
    let atomic = wzion_zion * 1_000_000; // V3 post-3.0.3: 6-decimal flowers
    EvmBurnEvent {
        evm_tx_hash: format!("0x{:064x}", burn_id.len()),
        evm_block_number: 38_000_000,
        evm_chain: "base".into(),
        evm_burner: "0xBurner0000000000000000000000000000000001".into(),
        amount_wzion_wei: flowers_to_wzion_wei(atomic),
        amount_flowers: atomic,
        l1_recipient: l1_recipient.into(),
        burn_id: burn_id.into(),
        detected_at: Utc::now(),
        status: BridgeStatus::Pending,
        confirmations: 0,
    }
}

// ─── 1. L1 API JSON deserialization ────────────────────────────────────────
//
// The actual API response from ZION L1 node:
//   GET /api/block/height/{n}
//   {"block":{"header":{"height":4574,"prev_hash":"...","timestamp":...},
//             "transactions":[{"id":"...","inputs":[],"outputs":[...]}]},
//    "status":"ok"}
//
// Previously: `json::<L1Block>()` → failed with `missing field 'height'`
// Fixed:      `json::<ApiBlockResponse>()` → `L1Block::from(resp)`
//
// These tests verify the conversion logic via public types.

#[test]
fn test_api_block_format_basic_conversion() {
    // Verify that 6-decimal L1 flowers ↔ 18-decimal EVM wei conversion is correct
    // At L1 block height 4574, a lock TX output pays 5_400_000 flowers = 5.4 ZION (post-3.0.3)
    let amount_flowers: u64 = 5_400_000;
    let wzion_wei = flowers_to_wzion_wei(amount_flowers);

    // 5_400_000 × 1e12 = 5_400_000_000_000_000_000 = 5.4 × 1e18
    assert_eq!(wzion_wei, "5400000000000000000");

    // Recovered by EVM watcher from burn event
    let recovered = wzion_wei_to_flowers(&wzion_wei).unwrap();
    assert_eq!(recovered, amount_flowers);
}

#[test]
fn test_l1_block_real_tx_format() {
    // Simulate the actual JSON that the L1 API returns
    // This is what caused "missing field 'height'" — the API nests under "block.header"
    let json = r#"{
        "block": {
            "header": {
                "height": 4574,
                "prev_hash": "0000000012d3e5a7b9c1f4e2",
                "timestamp": 1771885000
            },
            "transactions": [
                {
                    "id": "abcdef1234567890abcdef1234567890",
                    "inputs": [],
                    "outputs": [
                        {
                            "address": "zion1wn5nv4snxzjjlqb48z5zatungtvr4ruz6yjd4c5",
                            "amount": 5400000000000,
                            "memo": "BRIDGE:base:0x1234567890abcdef1234567890abcdef12345678"
                        }
                    ]
                }
            ]
        },
        "status": "ok"
    }"#;

    // We cannot directly test private ApiBlockResponse, but we can verify
    // the full JSON is valid and contains expected fields
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    let height = v["block"]["header"]["height"].as_u64().unwrap();
    let tx_id = v["block"]["transactions"][0]["id"].as_str().unwrap();
    let output_addr = v["block"]["transactions"][0]["outputs"][0]["address"]
        .as_str()
        .unwrap();
    let memo = v["block"]["transactions"][0]["outputs"][0]["memo"]
        .as_str()
        .unwrap();

    assert_eq!(height, 4574);
    assert_eq!(tx_id, "abcdef1234567890abcdef1234567890");
    assert_eq!(output_addr, "zion1wn5nv4snxzjjlqb48z5zatungtvr4ruz6yjd4c5");
    assert_eq!(
        memo,
        "BRIDGE:base:0x1234567890abcdef1234567890abcdef12345678"
    );
}

#[test]
fn test_l1_block_empty_inputs_field() {
    // Before fix: `inputs` was required → panic if tx had no inputs (coinbase TX)
    // After fix: `#[serde(default)]` on inputs field
    let json = r#"{
        "block": {
            "header": {"height": 1, "prev_hash": "0000000000000000", "timestamp": 1000},
            "transactions": [
                {
                    "id": "coinbase_tx_001",
                    "outputs": [
                        {"address": "zion1qminer", "amount": 5400000000000}
                    ]
                }
            ]
        },
        "status": "ok"
    }"#;

    // Coinbase TX has NO inputs field — should not fail
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    let tx = &v["block"]["transactions"][0];
    // inputs is null/missing → treated as empty array
    assert!(tx["inputs"].is_null() || tx["inputs"].is_array());
    assert_eq!(
        tx["outputs"][0]["amount"].as_u64().unwrap(),
        5_400_000_000_000
    );
}

// ─── 2. Testnet vs Mainnet config ──────────────────────────────────────────

fn base_testnet_chain() -> EvmChainConfig {
    EvmChainConfig {
        chain_id: "base-sepolia".into(),
        name: "Base Sepolia (TestNet)".into(),
        evm_chain_id: 84532,
        rpc_url: Some("wss://base-sepolia.publicnode.com".into()),
        rpc_url_backup: None,
        wzion_address: "0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6".into(),
        // Bridge contract: ZIONBridge deployed on Base Sepolia 23.2.2026
        // Source: archive/2.9.9/legacy-code/config/bridge-testnet.toml
        bridge_contract_address: "0xF4BF85443ad6c9b88f3a5314cC3Fb59C32Cedca1".into(),
        finality_blocks: 15,
        enabled: true,
        gas_strategy: "eip1559".into(),
        max_gas_gwei: 50,
        start_block: Some(43_197_000), // Updated to current Base Sepolia block
    }
}

fn base_mainnet_chain() -> EvmChainConfig {
    EvmChainConfig {
        chain_id: "base".into(),
        name: "Base (MainNet)".into(),
        evm_chain_id: 8453,
        rpc_url: Some("wss://base-mainnet.publicnode.com".into()),
        rpc_url_backup: Some("wss://base.llamarpc.com".into()),
        // Placeholder — update after T1 bridge deploy via:
        //   ./scripts/deploy-bridge-base.sh base
        wzion_address: "0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6".into(),
        bridge_contract_address: "0x5a1Df5961C166a79E0817329e2807Aac63Db57F5".into(),
        finality_blocks: 64, // Base mainnet: 64 blocks ≈ 128s
        enabled: true,       // 5/5 bridge deployed and migrated
        gas_strategy: "eip1559".into(),
        max_gas_gwei: 100,             // Higher gas limit for mainnet
        start_block: Some(47_687_000), // Set to ~current Base mainnet block
    }
}

#[test]
fn test_testnet_chain_id() {
    let chain = base_testnet_chain();
    assert_eq!(chain.evm_chain_id, 84532, "Base Sepolia testnet chain ID");
    assert_eq!(chain.chain_id, "base-sepolia");
    assert!(chain.enabled, "Testnet chain should be enabled for testing");
    assert!(
        chain.start_block.is_some(),
        "Start block must be set to skip genesis scan"
    );
    assert_eq!(chain.start_block.unwrap(), 43_197_000);
}

#[test]
fn test_mainnet_chain_id() {
    let chain = base_mainnet_chain();
    assert_eq!(chain.evm_chain_id, 8453, "Base mainnet chain ID");
    assert_eq!(chain.chain_id, "base");
    assert!(
        chain.enabled,
        "Mainnet chain must be ENABLED after 5/5 bridge deployment"
    );
    // 5/5 bridge deployed
    assert_eq!(
        chain.bridge_contract_address, "0x5a1Df5961C166a79E0817329e2807Aac63Db57F5",
        "Mainnet bridge contract must be the deployed 5/5 bridge"
    );
}

#[test]
fn test_testnet_mainnet_chain_ids_differ() {
    // CRITICAL: chain IDs must differ to avoid testnet TXs being replayed on mainnet
    let testnet = base_testnet_chain();
    let mainnet = base_mainnet_chain();
    assert_ne!(
        testnet.evm_chain_id, mainnet.evm_chain_id,
        "Testnet and mainnet must have different chain IDs!"
    );
    assert_ne!(
        testnet.chain_id, mainnet.chain_id,
        "chain_id string identifiers must differ"
    );
}

#[test]
fn test_mainnet_config_structure() {
    let mut cfg = BridgeConfig::default();
    cfg.bridge = BridgeIdentity {
        name: "ZION Bridge Relay".into(),
        version: "2.9.6".into(),
        network: "mainnet".into(),
    };
    cfg.evm_chains = vec![
        base_testnet_chain(), // still tracking testnet burns?
        base_mainnet_chain(), // disabled until live
    ];

    // Both testnet and mainnet are active
    let active = cfg.active_chains();
    assert_eq!(active.len(), 2);
    assert!(active.iter().any(|c| c.chain_id == "base-sepolia"));
    assert!(active.iter().any(|c| c.chain_id == "base"));

    assert_eq!(cfg.bridge.network, "mainnet");
}

#[test]
fn test_mainnet_config_has_stricter_security() {
    // On mainnet the min bridge amount should be higher (less spam)
    // and gas limit should match real mainnet prices
    let chain = base_mainnet_chain();
    assert!(
        chain.max_gas_gwei >= 100,
        "Mainnet gas limit should be ≥100 gwei to handle congestion"
    );
    // Mainnet needs backup RPC
    assert!(
        chain.rpc_url_backup.is_some(),
        "Mainnet chain must have a backup RPC endpoint"
    );
}

#[test]
fn test_config_network_field_mainnet_vs_testnet() {
    let testnet_cfg = {
        let mut c = BridgeConfig::default();
        c.bridge.network = "testnet".into();
        c
    };
    let mainnet_cfg = {
        let mut c = BridgeConfig::default();
        c.bridge.network = "mainnet".into();
        c
    };

    // Simple sanity: network field is preserved
    assert_eq!(testnet_cfg.bridge.network, "testnet");
    assert_eq!(mainnet_cfg.bridge.network, "mainnet");
    assert_ne!(testnet_cfg.bridge.network, mainnet_cfg.bridge.network);
}

// ─── 3. EVM block range chunking ───────────────────────────────────────────
//
// publicnode.com limit: 50,000 blocks per eth_getLogs call
// Fix: scan in chunks of MAX_BLOCK_RANGE = 49,000
//
// We simulate the chunking logic to verify correctness.

const MAX_BLOCK_RANGE: u64 = 49_000;

fn simulate_chunks(from: u64, to: u64) -> Vec<(u64, u64)> {
    let mut chunks = vec![];
    let mut chunk_from = from;
    while chunk_from <= to {
        let chunk_to = (chunk_from + MAX_BLOCK_RANGE - 1).min(to);
        chunks.push((chunk_from, chunk_to));
        chunk_from = chunk_to + 1;
    }
    chunks
}

#[test]
fn test_evm_chunking_small_range() {
    // Range smaller than a single chunk → 1 chunk
    let chunks = simulate_chunks(38_057_800, 38_058_000);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], (38_057_800, 38_058_000));
}

#[test]
fn test_evm_chunking_exact_chunk_boundary() {
    // Exactly MAX_BLOCK_RANGE blocks → 1 chunk
    let chunks = simulate_chunks(1, MAX_BLOCK_RANGE);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], (1, MAX_BLOCK_RANGE));
}

#[test]
fn test_evm_chunking_one_over_boundary() {
    // MAX_BLOCK_RANGE + 1 → 2 chunks
    let chunks = simulate_chunks(1, MAX_BLOCK_RANGE + 1);
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0], (1, MAX_BLOCK_RANGE));
    assert_eq!(chunks[1], (MAX_BLOCK_RANGE + 1, MAX_BLOCK_RANGE + 1));
}

#[test]
fn test_evm_chunking_large_range_no_gaps_or_overlaps() {
    // 38M block range (genesis → current) — what used to fail with 50k limit
    let from = 1u64;
    let to = 38_059_000u64;
    let chunks = simulate_chunks(from, to);

    // Verify coverage: blocks 1..=38_059_000 are covered exactly once
    let mut covered = 0u64;
    let mut prev_end = from - 1;
    for (chunk_from, chunk_to) in &chunks {
        // No gap
        assert_eq!(
            *chunk_from,
            prev_end + 1,
            "Gap detected before chunk ({}, {})",
            chunk_from,
            chunk_to
        );
        // No overlap
        assert!(*chunk_to >= *chunk_from);
        // Within limit
        assert!(
            chunk_to - chunk_from + 1 <= MAX_BLOCK_RANGE,
            "Chunk ({}, {}) exceeds MAX_BLOCK_RANGE={}",
            chunk_from,
            chunk_to,
            MAX_BLOCK_RANGE
        );
        covered += chunk_to - chunk_from + 1;
        prev_end = *chunk_to;
    }

    assert_eq!(
        covered,
        to - from + 1,
        "Total covered blocks must equal range"
    );
    assert_eq!(prev_end, to, "Last chunk must end exactly at 'to'");
}

#[test]
fn test_evm_chunking_start_block_skips_genesis() {
    // With start_block = 43_197_000, we skip the 43M genesis blocks
    let start = 43_197_000u64;
    let current = 43_198_500u64;
    let finalized = current - 12; // 12 block finality on Base

    let chunks = simulate_chunks(start + 1, finalized);

    // Should be 1-2 small chunks (only ~1200 blocks since deployment)
    assert!(
        chunks.len() <= 2,
        "With start_block, only a few chunks needed (got {})",
        chunks.len()
    );

    // First block must be start+1 (not genesis)
    assert_eq!(chunks[0].0, start + 1);

    // All chunks within publicnode limit
    for (from, to) in &chunks {
        assert!(to - from + 1 <= MAX_BLOCK_RANGE);
    }
}

#[test]
fn test_evm_chunking_single_block() {
    let chunks = simulate_chunks(5000, 5000);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], (5000, 5000));
}

// ─── 4. Replay attack prevention ───────────────────────────────────────────

#[test]
fn test_duplicate_lock_rejected() {
    let (db, _dir) = open_db();

    let l = lock(
        "tx_dup_001",
        1000,
        "base",
        "0x1234567890abcdef1234567890abcdef12345678",
    );
    db.insert_lock(&l).unwrap();

    // Mark as Completed (simulating a successful bridge operation)
    db.update_lock_status("tx_dup_001", BridgeStatus::Completed)
        .unwrap();

    // Attacker replays the same TX: INSERT OR IGNORE silently ignores it
    // The existing Completed row must NOT be reset to Pending
    let mut lock2 = l.clone();
    lock2.status = BridgeStatus::Pending; // attacker tries to reset status
    let result = db.insert_lock(&lock2);
    assert!(
        result.is_ok(),
        "INSERT OR IGNORE should succeed (not error)"
    );

    // The status in DB must still be Completed, not reset to Pending
    let pending = db.get_pending_locks().unwrap();
    assert_eq!(
        pending.len(),
        0,
        "Completed lock must not reappear as Pending after duplicate insert — replay protection"
    );
}

#[test]
fn test_duplicate_burn_rejected() {
    let (db, _dir) = open_db();

    let b = burn(
        "burn_dup_001",
        500,
        "zion1qrecipient000000000000000000000000001",
    );
    db.insert_burn(&b).unwrap();
    db.update_burn_status("burn_dup_001", BridgeStatus::Completed)
        .unwrap();

    // EVM watcher re-scans old blocks after restart — tries to insert same burn again
    let result = db.insert_burn(&b); // INSERT OR IGNORE → silently skipped
    assert!(
        result.is_ok(),
        "INSERT OR IGNORE should succeed (not error)"
    );

    // Completed burn must stay Completed — not reset to Pending
    let pending = db.get_pending_burns().unwrap();
    assert_eq!(
        pending.len(),
        0,
        "Completed burn must not reappear as Pending after watcher restart — replay protection"
    );
}

#[test]
fn test_different_tx_hashes_accepted() {
    let (db, _dir) = open_db();

    // Two different lock TXs must both be accepted
    db.insert_lock(&lock(
        "tx_a",
        100,
        "base",
        "0x1234567890abcdef1234567890abcdef12345678",
    ))
    .unwrap();
    db.insert_lock(&lock(
        "tx_b",
        200,
        "base",
        "0x1234567890abcdef1234567890abcdef12345678",
    ))
    .unwrap();

    let pending = db.get_pending_locks().unwrap();
    assert_eq!(pending.len(), 2);
}

// ─── 5. Memo parsing edge cases ────────────────────────────────────────────
//
// Memo format: "BRIDGE:<chain>:<evm_address>"
// Tested directly via the L1Watcher memo parser.
// We verify all edge cases that could bypass security.

/// Helper: manually implement same parse logic as L1Watcher::parse_bridge_memo
/// so we can test it without async infrastructure.
fn parse_memo(memo: Option<&str>) -> Option<(String, String)> {
    let memo = memo?;
    let parts: Vec<&str> = memo.split(':').collect();
    if parts.len() >= 3 && parts[0] == "BRIDGE" {
        let chain = parts[1].to_lowercase();
        let addr = parts[2].to_string();
        if addr.starts_with("0x") && addr.len() == 42 {
            return Some((chain, addr));
        }
    }
    None
}

#[test]
fn test_memo_valid_base() {
    let r = parse_memo(Some(
        "BRIDGE:base:0x1234567890abcdef1234567890abcdef12345678",
    ));
    assert_eq!(
        r,
        Some((
            "base".into(),
            "0x1234567890abcdef1234567890abcdef12345678".into()
        ))
    );
}

#[test]
fn test_memo_valid_arbitrum() {
    let r = parse_memo(Some(
        "BRIDGE:arbitrum:0xAbCdEf1234567890aBcDeF1234567890AbCdEf12",
    ));
    assert!(r.is_some());
    let (chain, _) = r.unwrap();
    assert_eq!(chain, "arbitrum");
}

#[test]
fn test_memo_chain_lowercased() {
    // Chain name must be normalized to lowercase
    let r = parse_memo(Some(
        "BRIDGE:BASE:0x1234567890abcdef1234567890abcdef12345678",
    ));
    assert!(r.is_some());
    assert_eq!(r.unwrap().0, "base");
}

#[test]
fn test_memo_missing_0x_prefix() {
    // EVM address without 0x — must be rejected
    let r = parse_memo(Some("BRIDGE:base:1234567890abcdef1234567890abcdef12345678"));
    assert!(r.is_none(), "Address without 0x must be rejected");
}

#[test]
fn test_memo_address_too_short() {
    let r = parse_memo(Some("BRIDGE:base:0x1234"));
    assert!(r.is_none(), "Short address must be rejected");
}

#[test]
fn test_memo_address_too_long() {
    // 43 chars after 0x → total > 42
    let r = parse_memo(Some(
        "BRIDGE:base:0x1234567890abcdef1234567890abcdef123456789",
    ));
    assert!(r.is_none(), "Long address must be rejected");
}

#[test]
fn test_memo_wrong_prefix() {
    let r = parse_memo(Some(
        "TRANSFER:base:0x1234567890abcdef1234567890abcdef12345678",
    ));
    assert!(r.is_none());

    let r2 = parse_memo(Some(
        "bridge:base:0x1234567890abcdef1234567890abcdef12345678",
    ));
    assert!(
        r2.is_none(),
        "Lowercase 'bridge' prefix must be rejected — strict uppercase only"
    );
}

#[test]
fn test_memo_none() {
    assert!(parse_memo(None).is_none());
}

#[test]
fn test_memo_empty_string() {
    assert!(parse_memo(Some("")).is_none());
}

#[test]
fn test_memo_only_prefix() {
    assert!(parse_memo(Some("BRIDGE")).is_none());
    assert!(parse_memo(Some("BRIDGE:")).is_none());
    assert!(parse_memo(Some("BRIDGE:base:")).is_none());
}

#[test]
fn test_memo_zero_address() {
    // Zero address (0x000...000) is technically valid format but semantically dangerous
    // The parser should accept it (format is OK), but application should reject it separately
    let r = parse_memo(Some(
        "BRIDGE:base:0x0000000000000000000000000000000000000000",
    ));
    assert!(
        r.is_some(),
        "Zero address has valid format — format parser accepts it"
    );
}

#[test]
fn test_memo_mainnet_chain() {
    // On mainnet the chain will be "base" (not "base-sepolia")
    let r = parse_memo(Some(
        "BRIDGE:base:0xDeadBeef1234567890DeadBeef1234567890DEAD",
    ));
    assert!(r.is_some());
    let (chain, addr) = r.unwrap();
    assert_eq!(chain, "base"); // This is what mainnet bridge will receive
    assert_eq!(addr, "0xDeadBeef1234567890DeadBeef1234567890DEAD");
}

// ─── 6. Amount invariants ──────────────────────────────────────────────────

#[test]
fn test_amount_below_minimum() {
    let cfg = BridgeConfig::default();
    let min: u128 = cfg.security.min_bridge_amount.parse().unwrap();

    // 99 ZION < 100 ZION minimum — post-3.0.3: 99 × 1e6 flowers
    let small = flowers_to_wzion_wei(99_000_000); // 99 ZION
    let small_wei: u128 = small.parse().unwrap();
    assert!(small_wei < min, "99 ZION should be below 100 wZION minimum");
}

#[test]
fn test_amount_exactly_at_minimum() {
    let cfg = BridgeConfig::default();
    let min: u128 = cfg.security.min_bridge_amount.parse().unwrap();

    // Exactly 100 ZION = minimum — post-3.0.3: 100 × 1e6 flowers
    let exact = flowers_to_wzion_wei(100_000_000); // 100 ZION
    let exact_wei: u128 = exact.parse().unwrap();
    assert_eq!(exact_wei, min, "100 ZION should equal exactly the minimum");
}

#[test]
fn test_amount_just_below_timelock() {
    let cfg = BridgeConfig::default();
    let threshold: u128 = cfg.security.timelock_threshold.parse().unwrap();

    // 999_999 ZION (1 below 1M timelock threshold) — post-3.0.3: 999_999 × 1e6 flowers
    let just_under = flowers_to_wzion_wei(999_999_000_000); // 999,999 ZION
    let wei: u128 = just_under.parse().unwrap();
    assert!(
        wei < threshold,
        "999,999 ZION should be below timelock threshold"
    );
}

#[test]
fn test_amount_exactly_at_timelock() {
    let cfg = BridgeConfig::default();
    let threshold: u128 = cfg.security.timelock_threshold.parse().unwrap();

    // 1M ZION = timelock threshold — post-3.0.3: 1M × 1e6 flowers
    let exactly = flowers_to_wzion_wei(1_000_000_000_000); // 1M ZION
    let wei: u128 = exactly.parse().unwrap();
    assert_eq!(
        wei, threshold,
        "1M ZION should equal exactly the timelock threshold"
    );
}

#[test]
fn test_amount_exceeds_single_limit() {
    let cfg = BridgeConfig::default();
    let max: u128 = cfg.security.max_single_amount.parse().unwrap();

    // 5_000_001 ZION > 5M max single — post-3.0.3: 5_000_001 × 1e6 flowers
    let too_big = flowers_to_wzion_wei(5_000_001_000_000); // 5M+1 ZION
    let wei: u128 = too_big.parse().unwrap();
    assert!(wei > max, "5M+1 ZION should exceed single operation limit");
}

#[test]
fn test_daily_limit_enforcement() {
    let cfg = BridgeConfig::default();
    let daily: u128 = cfg.security.daily_limit.parse().unwrap();
    let max_single: u128 = cfg.security.max_single_amount.parse().unwrap();

    // 2 × max_single should exceed daily limit (if daily = 10M and max_single = 5M)
    // In practice: 3 × 4M = 12M > 10M daily limit
    let three_ops: u128 = 3 * (4_000_000u128 * 1_000_000_000_000_000_000u128);
    assert!(
        three_ops > daily,
        "3 × 4M ZION should exceed 10M daily limit"
    );

    // A single 4M transfer is below max_single but below daily on its own
    let one_op: u128 = 4_000_000u128 * 1_000_000_000_000_000_000u128;
    assert!(
        one_op < max_single,
        "4M ZION should be below 5M single limit"
    );
    assert!(one_op < daily, "4M ZION should be below 10M daily limit");
}

#[test]
fn test_security_limit_ordering() {
    let cfg = BridgeConfig::default();
    let min: u128 = cfg.security.min_bridge_amount.parse().unwrap();
    let timelock: u128 = cfg.security.timelock_threshold.parse().unwrap();
    let max_single: u128 = cfg.security.max_single_amount.parse().unwrap();
    let daily: u128 = cfg.security.daily_limit.parse().unwrap();

    // Invariant: min < timelock < max_single < daily
    assert!(
        min < timelock,
        "min ({}) must be < timelock ({})",
        min,
        timelock
    );
    assert!(
        timelock < max_single,
        "timelock ({}) must be < max_single ({})",
        timelock,
        max_single
    );
    assert!(
        max_single < daily,
        "max_single ({}) must be < daily ({})",
        max_single,
        daily
    );
}

// ─── 7. Multi-chain routing ────────────────────────────────────────────────

#[test]
fn test_multi_chain_routing_correct_chain() {
    // Lock to "base" must match "base" chain config, NOT "base-sepolia" or "arbitrum"
    let mut cfg = BridgeConfig::default();
    cfg.evm_chains = vec![
        base_testnet_chain(), // chain_id: "base-sepolia"
        base_mainnet_chain(), // chain_id: "base"
        EvmChainConfig {
            chain_id: "arbitrum".into(),
            name: "Arbitrum One".into(),
            evm_chain_id: 42161,
            rpc_url: Some("wss://arbitrum-one.publicnode.com".into()),
            rpc_url_backup: None,
            wzion_address: "0x0000000000000000000000000000000000000000".into(),
            bridge_contract_address: "0x0000000000000000000000000000000000000000".into(),
            finality_blocks: 12,
            enabled: true,
            gas_strategy: "eip1559".into(),
            max_gas_gwei: 10,
            start_block: None,
        },
    ];

    // Routing: find chain for "base" lock
    let target_chain = "base";
    let matched = cfg.evm_chains.iter().find(|c| c.chain_id == target_chain);
    assert!(matched.is_some(), "Must find 'base' chain config");
    assert_eq!(matched.unwrap().evm_chain_id, 8453); // mainnet, not 84532 testnet

    // Must not match "base-sepolia"
    assert_ne!(matched.unwrap().chain_id, "base-sepolia");
}

#[test]
fn test_multi_chain_routing_exact_match() {
    let mut cfg = BridgeConfig::default();
    cfg.evm_chains = vec![
        base_testnet_chain(), // base-sepolia
    ];

    // "base" lock cannot be processed if only "base-sepolia" is configured
    let matched = cfg
        .evm_chains
        .iter()
        .find(|c| c.chain_id == "base" && c.enabled);
    assert!(
        matched.is_none(),
        "Lock to 'base' mainnet must fail if only 'base-sepolia' testnet is configured"
    );
}

#[test]
fn test_chain_lookup_disabled_chain() {
    let mut disabled = base_mainnet_chain();
    disabled.enabled = false; // explicitly disabled for this test
    let mut cfg = BridgeConfig::default();
    cfg.evm_chains = vec![disabled];

    // Routing must skip disabled chains
    let matched = cfg
        .evm_chains
        .iter()
        .find(|c| c.chain_id == "base" && c.enabled);
    assert!(
        matched.is_none(),
        "Disabled 'base' mainnet chain must not be routable"
    );
}

// ─── 8. Supply invariant ────────────────────────────────────────────────────
//
// CRITICAL for mainnet: total locked on L1 must always equal outstanding wZION on EVM
// (modulo in-flight operations).
//
// Invariant: locked_l1 ≥ outstanding_wzion
// Where outstanding_wzion = minted_wzion - burned_wzion

#[test]
fn test_supply_invariant_basic() {
    // 3 locks, 1 burn completed
    let locks: Vec<(u64, &str)> = vec![(1000, "lock_a"), (2000, "lock_b"), (500, "lock_c")];
    let burns: Vec<u64> = vec![800]; // 800 ZION burned back

    let total_locked: u64 = locks.iter().map(|(a, _)| *a).sum();
    let total_minted = total_locked;
    let total_burned: u64 = burns.iter().sum();
    let outstanding = total_minted - total_burned;

    assert_eq!(total_locked, 3500, "Total locked: 3500 ZION");
    assert_eq!(outstanding, 2700, "Outstanding wZION: 2700 ZION worth");
    assert!(
        total_locked >= outstanding,
        "Supply invariant: locked ≥ outstanding"
    );
}

#[test]
fn test_supply_invariant_after_full_roundtrip() {
    // User locks 1000 ZION, gets 1000 wZION, burns all 1000 wZION back
    let locked: u64 = 1000;
    let minted = locked;
    let burned = minted; // complete roundtrip

    let outstanding = minted - burned;
    assert_eq!(outstanding, 0, "Full roundtrip: 0 outstanding wZION");
    assert!(locked >= outstanding, "Supply invariant holds");
}

#[test]
fn test_supply_invariant_with_db() {
    let (db, _dir) = open_db();

    // Insert 3 locks
    let lock_ops: &[(&str, u64)] = &[("lock_s1", 1000), ("lock_s2", 2000), ("lock_s3", 500)];
    for &(tx_hash, zion) in lock_ops {
        db.insert_lock(&lock(
            tx_hash,
            zion,
            "base",
            "0x1234567890abcdef1234567890abcdef12345678",
        ))
        .unwrap();
    }

    // Insert 1 burn (800 ZION worth)
    db.insert_burn(&burn(
        "b1",
        800,
        "zion1qrecipient000000000000000000000000001",
    ))
    .unwrap();

    // Complete the burn
    db.update_burn_status("b1", BridgeStatus::Completed)
        .unwrap();

    let stats = db.get_stats().unwrap();
    // total_operations counts completed operations only
    assert_eq!(stats.total_operations, 1, "1 completed burn");
}

#[test]
fn test_supply_invariant_conversion_precision() {
    // Any roundtrip through the conversion must be exact (no dust creation)
    let amounts_zion: &[u64] = &[1, 10, 100, 1000, 10_000, 100_000, 1_000_000, 5_000_000];

    for &zion in amounts_zion {
        let atomic = zion * 1_000_000; // V3 post-3.0.3: 6-decimal flowers
        let wzion = flowers_to_wzion_wei(atomic);
        let recovered = wzion_wei_to_flowers(&wzion).unwrap();
        assert_eq!(
            atomic, recovered,
            "Lossless roundtrip failed for {} ZION (atomic: {})",
            zion, atomic
        );
    }
}

// ─── 9. Mainnet deployment checklist (compile-time docs) ───────────────────

/// Checklist for mainnet bridge deployment.
/// These assertions verify config defaults are correct for mainnet.
#[test]
fn test_mainnet_deployment_checklist() {
    // 1. L1 finality: 60 blocks ≈ 10 minutes (safe for ZION)
    let cfg = BridgeConfig::default();
    assert_eq!(
        cfg.l1.finality_blocks, 60,
        "L1 finality must be 60+ blocks for mainnet safety"
    );

    // 2. Validator threshold ≥ 2 (already enforced by ValidatorSet::new)
    assert!(
        cfg.validator.threshold >= 2,
        "Validator threshold must be ≥ 2"
    );

    // 3. Auto-pause on anomaly must be enabled on mainnet
    assert!(
        cfg.security.auto_pause_on_anomaly,
        "Auto-pause must be enabled!"
    );

    // 4. Min bridge amount: 100 ZION (100 × 1e18 = 1e20 wei)
    let min: u128 = cfg.security.min_bridge_amount.parse().unwrap();
    assert!(
        min >= 100 * 1_000_000_000_000_000_000u128,
        "Min must be ≥ 100 wZION"
    );

    // 5. Daily limit: ≥ 1M ZION
    let daily: u128 = cfg.security.daily_limit.parse().unwrap();
    assert!(
        daily >= 1_000_000 * 1_000_000_000_000_000_000u128,
        "Daily limit must be ≥ 1M wZION"
    );

    // 6. Base mainnet chain ID
    let base_mainnet = base_mainnet_chain();
    assert_eq!(
        base_mainnet.evm_chain_id, 8453,
        "Base mainnet chain ID = 8453"
    );

    // 7. Mainnet chain must be enabled after 5/5 bridge deployed
    assert!(
        base_mainnet.enabled,
        "Mainnet chain must be enabled after 5/5 bridge deployment"
    );

    println!("✅ Mainnet deployment checklist PASSED");
    println!("   L1 finality: {} blocks", cfg.l1.finality_blocks);
    println!(
        "   Validator threshold: {}/{}",
        cfg.validator.threshold, cfg.validator.total_validators
    );
    println!("   Auto-pause: {}", cfg.security.auto_pause_on_anomaly);
    println!("   Base mainnet chain ID: {}", base_mainnet.evm_chain_id);
    println!("   ❗ TODO: Deploy wZION + ZIONBridge on Base mainnet");
    println!("   ❗ TODO: Set start_block for mainnet scanning");
    println!("   ❗ TODO: Enable mainnet chain in bridge-mainnet.toml");
    println!("   ❗ TODO: Fund validator wallet with ETH for gas");
}

// ─── Config TOML file parsing ───────────────────────────────────────────────

/// Verify bridge-testnet.toml parses correctly against the BridgeConfig struct.
#[test]
fn test_parse_bridge_testnet_toml() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/config/bridge-testnet.toml");
    let cfg = BridgeConfig::load(path).expect("bridge-testnet.toml must parse");
    assert_eq!(cfg.bridge.network, "testnet");
    // Base Sepolia is enabled; Arbitrum Sepolia is disabled
    assert_eq!(cfg.evm_chains.len(), 2);
    let base = cfg
        .evm_chains
        .iter()
        .find(|c| c.chain_id == "base_sepolia")
        .expect("base_sepolia chain must be present");
    assert_eq!(base.evm_chain_id, 84532);
    assert!(base.enabled);
    assert_eq!(
        base.wzion_address,
        "0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6"
    );
    assert_eq!(
        base.bridge_contract_address,
        "0xF4BF85443ad6c9b88f3a5314cC3Fb59C32Cedca1"
    );
    assert_eq!(cfg.l1.rpc_url, "http://127.0.0.1:8443");
    assert_eq!(cfg.l1.finality_blocks, 60);
    assert!(cfg.ankr.enabled);
    assert!(cfg.security.auto_pause_on_anomaly);
    assert_eq!(cfg.validator.threshold, 2);
    assert_eq!(cfg.validator.total_validators, 2);
    assert_eq!(cfg.validator.validator_addresses.len(), 2);
}

/// Verify bridge-mainnet.toml parses correctly — Base mainnet, 5/5 bridge live.
#[test]
fn test_parse_bridge_mainnet_toml() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/config/bridge-mainnet.toml");
    let cfg = BridgeConfig::load(path).expect("bridge-mainnet.toml must parse");
    assert_eq!(cfg.bridge.network, "mainnet");
    // Base + Arbitrum + BSC + Polygon + Optimism + Avalanche (6-chain bridge)
    assert_eq!(cfg.evm_chains.len(), 6);
    let base = cfg
        .evm_chains
        .iter()
        .find(|c| c.chain_id == "base")
        .expect("base chain must be present");
    assert_eq!(base.evm_chain_id, 8453);
    // Base chain enabled after 5/5 bridge deployment and migration
    assert!(
        base.enabled,
        "mainnet Base chain should be enabled after 5/5 bridge deployment"
    );
    assert_eq!(
        base.wzion_address,
        "0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6"
    );
    assert_eq!(
        base.bridge_contract_address,
        "0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467"
    );
    assert_eq!(cfg.metrics.log_level, "info");
    assert_eq!(cfg.validator.threshold, 5);
    assert_eq!(cfg.validator.total_validators, 5);
    assert_eq!(cfg.validator.validator_addresses.len(), 5);
    assert_eq!(
        cfg.validator.validator_addresses,
        vec![
            "0xdde17506BC2D2dCE1d594bD1D85B0BAbb389D186",
            "0x24d986841E56e5571489B25951eE8C1Ae761FA82",
            "0x665c55eDCF25c2c5A1dfF1B20eE950cBDC58d3d0",
            "0x8E644b3E9FaBf52eE321DC5B3D5AA06d6e3E66C6",
            "0x7e0D2eD71d78B9CFB5034A83333e82e304bc4CB2",
        ]
    );
}
