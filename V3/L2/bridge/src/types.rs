//! Shared types for bridge operations.
//!
//! ## V3 Decimal Convention (post-3.0.3 fork)
//!
//! L1 atomic unit (flowers): 1 ZION = 1,000,000 flowers (6 decimals)
//! EVM wZION: 1 wZION = 1e18 wei (18 decimals)
//! Conversion factor: 1e12 (18 - 6 = 12)
//!
//! **CRITICAL**: Pre-3.0.3 L1 used 12 decimals (1e12 flowers/ZION).
//! Post-3.0.3 L1 uses 6 decimals (1e6 flowers/ZION). The bridge
//! FLOWERS_TO_WEI_FACTOR is now 1e12 (18-6=12).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── V3 Constants ──────────────────────────────────────────────────────────────

/// V3 canonical: 1 ZION = 1e6 flowers (6 decimals, post-3.0.3 fork).
pub const FLOWERS_PER_ZION: u64 = 1_000_000;

/// Pre-3.0.3: 1 ZION = 1e12 flowers (12 decimals).
pub const FLOWERS_PER_ZION_LEGACY: u64 = 1_000_000_000_000;

/// Conversion factor between L1 flowers (6 dec) and EVM wei (18 dec).
/// 18 - 6 = 12 → multiply/divide by 1e12.
pub const FLOWERS_TO_WEI_FACTOR: u128 = 1_000_000_000_000;

/// Pre-3.0.3 conversion factor: 18 - 12 = 6 → multiply/divide by 1e6.
pub const FLOWERS_TO_WEI_FACTOR_LEGACY: u128 = 1_000_000;

/// 3.0.3 migration height. Locks before this height use legacy 1e12 scale.
pub const MIGRATION_HEIGHT: u64 = 18_850;

/// Minimum bridge amount: 100 ZION in flowers.
pub const MIN_BRIDGE_AMOUNT: u64 = 100 * FLOWERS_PER_ZION;

/// Bridge fee: 0.1% (10 basis points).
pub const BRIDGE_FEE_BPS: u64 = 10;

// ── Enums ─────────────────────────────────────────────────────────────────────

/// Status of a bridge operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BridgeStatus {
    /// Detected on source chain, waiting for finality
    Pending,
    /// Finality confirmed, waiting for validator consensus
    Confirmed,
    /// Validator threshold reached, executing on destination
    Executing,
    /// Successfully completed on both chains
    Completed,
    /// Failed (will be retried)
    Failed,
    /// Timelocked (large amount, waiting for delay)
    Timelocked,
}

/// Direction of bridge transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BridgeDirection {
    /// ZION L1 → EVM (lock on L1, mint wZION on EVM)
    L1ToEvm,
    /// EVM → ZION L1 (burn wZION on EVM, unlock on L1)
    EvmToL1,
}

// ── Events ────────────────────────────────────────────────────────────────────

/// A lock event detected on ZION L1.
/// User sent ZION to the bridge vault address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L1LockEvent {
    /// L1 transaction hash (hex)
    pub l1_tx_hash: String,

    /// L1 block height where the lock TX was confirmed
    pub l1_block_height: u64,

    /// L1 sender address (zion1...)
    pub l1_sender: String,

    /// Amount locked in flowers (1 ZION = 1e6 flowers, post-3.0.3)
    pub amount_flowers: u64,

    /// Amount in wZION wei (18 decimals) — converted by relay
    pub amount_wzion_wei: String,

    /// Target EVM chain (e.g., "base", "arbitrum")
    pub target_chain: String,

    /// Recipient EVM address (parsed from TX memo)
    pub evm_recipient: String,

    /// Timestamp of detection
    pub detected_at: DateTime<Utc>,

    /// Current status
    pub status: BridgeStatus,

    /// Number of validator confirmations
    pub confirmations: u8,
}

/// A burn event detected on EVM chain.
/// User burned wZION via bridgeBurn().
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmBurnEvent {
    /// EVM transaction hash
    pub evm_tx_hash: String,

    /// EVM block number
    pub evm_block_number: u64,

    /// EVM chain ID (e.g., "base")
    pub evm_chain: String,

    /// Address that burned wZION
    pub evm_burner: String,

    /// Amount burned in wZION wei (18 decimals)
    pub amount_wzion_wei: String,

    /// Amount to unlock on L1 in flowers — converted by relay
    pub amount_flowers: u64,

    /// ZION L1 recipient address (zion1...)
    pub l1_recipient: String,

    /// Burn ID from wZION contract
    pub burn_id: String,

    /// Timestamp of detection
    pub detected_at: DateTime<Utc>,

    /// Current status
    pub status: BridgeStatus,

    /// Number of validator confirmations for L1 unlock
    pub confirmations: u8,
}

/// Bridge statistics snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BridgeStats {
    /// Total ZION locked on L1 (flowers)
    pub total_locked_flowers: u64,

    /// Total wZION minted across all EVM chains (wei string)
    pub total_minted_wzion: String,

    /// Total wZION burned across all EVM chains (wei string)
    pub total_burned_wzion: String,

    /// Outstanding wZION (minted - burned, should equal locked L1)
    pub outstanding_wzion: String,

    /// Total bridge operations (both directions)
    pub total_operations: u64,

    /// Operations in last 24h
    pub operations_24h: u64,

    /// Bridge uptime (seconds)
    pub uptime_secs: u64,

    /// Per-chain stats
    pub chain_stats: Vec<ChainBridgeStats>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChainBridgeStats {
    pub chain_id: String,
    pub chain_name: String,
    pub total_minted: String,
    pub total_burned: String,
    pub outstanding: String,
    pub operations: u64,
}

// ── Decimal conversion helpers (V3: 6 decimals, post-3.0.3) ───────────────────

pub mod conversion {
    use super::{
        FLOWERS_PER_ZION, FLOWERS_TO_WEI_FACTOR, FLOWERS_TO_WEI_FACTOR_LEGACY, MIGRATION_HEIGHT,
    };

    /// Convert L1 flowers to wZION wei string (18 decimals), height-aware.
    ///
    /// Post-3.0.3 (height >= MIGRATION_HEIGHT): 1 ZION = 1e6 flowers, factor = 1e12
    /// Pre-3.0.3  (height <  MIGRATION_HEIGHT): 1 ZION = 1e12 flowers, factor = 1e6
    ///
    /// Example: 1e6 flowers at height 20000 → "1000000000000000000" (1e18 wei, 1 ZION)
    /// Example: 1e14 flowers at height 11324 → "100000000000000000000" (1e20 wei, 100 ZION)
    pub fn flowers_to_wzion_wei_at(flowers: u64, block_height: u64) -> String {
        let factor = if block_height >= MIGRATION_HEIGHT {
            FLOWERS_TO_WEI_FACTOR // 1e12 (post-fork)
        } else {
            FLOWERS_TO_WEI_FACTOR_LEGACY // 1e6 (pre-fork)
        };
        let wei = (flowers as u128) * factor;
        wei.to_string()
    }

    /// Convert L1 flowers (6 decimals, post-3.0.3) to wZION wei string.
    ///
    /// **WARNING**: Only use for post-3.0.3 locks. For pre-fork locks,
    /// use `flowers_to_wzion_wei_at(flowers, block_height)`.
    ///
    /// V3 post-3.0.3: 1 ZION = 1e6 flowers. EVM: 1 wZION = 1e18 wei.
    /// Factor = 1e12 (18 - 6 = 12).
    ///
    /// Example: 1e6 flowers (1 ZION) → "1000000000000000000" (1e18 wei)
    pub fn flowers_to_wzion_wei(flowers: u64) -> String {
        let wei = (flowers as u128) * FLOWERS_TO_WEI_FACTOR; // × 1e12
        wei.to_string()
    }

    /// Convert wZION wei string (18 decimals) to L1 flowers (6 decimals, post-3.0.3).
    /// Rounds down (truncates sub-flower dust).
    pub fn wzion_wei_to_flowers(wei_str: &str) -> Result<u64, String> {
        let wei: u128 = wei_str
            .parse()
            .map_err(|e| format!("Invalid wei amount: {}", e))?;
        let flowers = wei / FLOWERS_TO_WEI_FACTOR; // ÷ 1e12
        if flowers > u64::MAX as u128 {
            return Err("Amount exceeds u64 max".into());
        }
        Ok(flowers as u64)
    }

    /// Format flowers to human-readable ZION (post-3.0.3, 6 decimals).
    /// Example: 5_400_067_000 flowers → "5400.067"
    pub fn flowers_to_zion_display(flowers: u64) -> String {
        let whole = flowers / FLOWERS_PER_ZION;
        let frac = flowers % FLOWERS_PER_ZION;
        if frac == 0 {
            format!("{}", whole)
        } else {
            format!("{}.{:06}", whole, frac)
                .trim_end_matches('0')
                .to_string()
        }
    }

    /// Format flowers to human-readable ZION, height-aware.
    /// Pre-3.0.3: 1 ZION = 1e12 flowers (12 decimals)
    /// Post-3.0.3: 1 ZION = 1e6 flowers (6 decimals)
    pub fn flowers_to_zion_display_at(flowers: u64, block_height: u64) -> String {
        use super::{FLOWERS_PER_ZION, FLOWERS_PER_ZION_LEGACY, MIGRATION_HEIGHT};
        let divisor = if block_height >= MIGRATION_HEIGHT {
            FLOWERS_PER_ZION
        } else {
            FLOWERS_PER_ZION_LEGACY
        };
        let whole = flowers / divisor;
        let frac = flowers % divisor;
        if frac == 0 {
            format!("{}", whole)
        } else if divisor == FLOWERS_PER_ZION {
            format!("{}.{:06}", whole, frac)
                .trim_end_matches('0')
                .to_string()
        } else {
            format!("{}.{:012}", whole, frac)
                .trim_end_matches('0')
                .to_string()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_flowers_to_wzion_1_zion() {
            // 1 ZION = 1e6 flowers → 1e18 wei (post-3.0.3)
            assert_eq!(
                flowers_to_wzion_wei(1_000_000),
                "1000000000000000000" // 1e18
            );
        }

        #[test]
        fn test_flowers_to_wzion_wei_at_post_fork() {
            // Post-fork: 1 ZION = 1e6 flowers, factor = 1e12
            // 100 ZION = 100e6 flowers → 100e18 wei
            assert_eq!(
                flowers_to_wzion_wei_at(100_000_000, 20_000),
                "100000000000000000000" // 100 × 1e18
            );
        }

        #[test]
        fn test_flowers_to_wzion_wei_at_pre_fork() {
            // Pre-fork: 1 ZION = 1e12 flowers, factor = 1e6
            // 100 ZION = 100e12 = 1e14 flowers → 1e14 × 1e6 = 1e20 wei
            assert_eq!(
                flowers_to_wzion_wei_at(100_000_000_000_000, 11_324),
                "100000000000000000000" // 100 × 1e18
            );
        }

        #[test]
        fn test_flowers_to_wzion_wei_at_migration_boundary() {
            // At exactly MIGRATION_HEIGHT: use post-fork factor (1e12)
            assert_eq!(
                flowers_to_wzion_wei_at(1_000_000, super::super::MIGRATION_HEIGHT),
                "1000000000000000000" // 1 ZION
            );
            // One block before: use pre-fork factor (1e6)
            assert_eq!(
                flowers_to_wzion_wei_at(1_000_000, super::super::MIGRATION_HEIGHT - 1),
                "1000000000000" // 1e6 flowers × 1e6 = 1e12 wei = 0.000001 ZION (tiny!)
            );
        }

        #[test]
        fn test_display_at_pre_fork() {
            // 1e14 flowers at height 11324 (pre-fork) = 100 ZION
            assert_eq!(
                flowers_to_zion_display_at(100_000_000_000_000, 11_324),
                "100"
            );
        }

        #[test]
        fn test_display_at_post_fork() {
            // 1e8 flowers at height 20000 (post-fork) = 100 ZION
            assert_eq!(flowers_to_zion_display_at(100_000_000, 20_000), "100");
        }

        #[test]
        fn test_flowers_to_wzion_block_reward() {
            // 5400.067 ZION = 5_400_067_000 flowers → × 1e12 = 5400067000000000000000
            assert_eq!(
                flowers_to_wzion_wei(5_400_067_000),
                "5400067000000000000000" // 5400.067 × 1e18
            );
        }

        #[test]
        fn test_wzion_to_flowers_1_zion() {
            // 1e18 wei → 1e6 flowers (1 ZION, post-3.0.3)
            assert_eq!(
                wzion_wei_to_flowers("1000000000000000000").unwrap(),
                1_000_000
            );
        }

        #[test]
        fn test_wzion_to_flowers_100_zion() {
            // 100 wZION = 100e18 wei → 100e6 flowers
            assert_eq!(
                wzion_wei_to_flowers("100000000000000000000").unwrap(),
                100_000_000
            );
        }

        #[test]
        fn test_roundtrip_lossless() {
            let original_flowers = 1_000_000_000u64; // 1000 ZION in new flowers
            let wzion_wei = flowers_to_wzion_wei(original_flowers);
            let recovered = wzion_wei_to_flowers(&wzion_wei).unwrap();
            assert_eq!(original_flowers, recovered, "Roundtrip must be lossless");
        }

        #[test]
        fn test_zero_conversion() {
            assert_eq!(flowers_to_wzion_wei(0), "0");
            assert_eq!(wzion_wei_to_flowers("0").unwrap(), 0);
            assert_eq!(flowers_to_zion_display(0), "0");
        }

        #[test]
        fn test_dust_truncation() {
            // Sub-flower dust: 999_999_999_999 wei < 1 flower (1e12 wei), truncated to 0
            assert_eq!(wzion_wei_to_flowers("999999999999").unwrap(), 0);
            // Exactly 1 flower = 1e12 wei
            assert_eq!(wzion_wei_to_flowers("1000000000000").unwrap(), 1);
        }

        #[test]
        fn test_display_whole() {
            assert_eq!(flowers_to_zion_display(1_000_000), "1");
        }

        #[test]
        fn test_display_fractional() {
            assert_eq!(flowers_to_zion_display(5_400_067_000), "5400.067");
        }

        #[test]
        fn test_display_sub_zion() {
            assert_eq!(flowers_to_zion_display(500_000), "0.5");
            assert_eq!(flowers_to_zion_display(100_000), "0.1");
            assert_eq!(flowers_to_zion_display(1), "0.000001");
        }

        #[test]
        fn test_min_bridge_amount() {
            // 100 ZION = 100e6 flowers (post-3.0.3)
            let min = 100_000_000u64;
            let wzion = flowers_to_wzion_wei(min);
            assert_eq!(wzion, "100000000000000000000"); // 100 × 1e18
        }

        #[test]
        fn test_large_amount() {
            // 10M ZION = 10_000_000e6 flowers
            let amount = 10_000_000_000_000u64;
            let wzion = flowers_to_wzion_wei(amount);
            assert_eq!(wzion_wei_to_flowers(&wzion).unwrap(), amount);
        }

        #[test]
        fn test_invalid_wei_string() {
            assert!(wzion_wei_to_flowers("not_a_number").is_err());
            assert!(wzion_wei_to_flowers("").is_err());
            assert!(wzion_wei_to_flowers("-1").is_err());
        }

        #[test]
        fn test_flowers_per_zion_constant() {
            assert_eq!(super::FLOWERS_PER_ZION, 1_000_000);
        }

        #[test]
        fn test_conversion_factor() {
            // 18 (EVM) - 6 (L1) = 12 → factor is 1e12
            assert_eq!(super::FLOWERS_TO_WEI_FACTOR, 1_000_000_000_000);
        }
    }
}

#[cfg(test)]
mod type_tests {
    use crate::{BridgeDirection, BridgeStats, BridgeStatus, EvmBurnEvent, L1LockEvent};

    #[test]
    fn test_bridge_status_serialization() {
        let status = BridgeStatus::Pending;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"Pending\"");
        let deserialized: BridgeStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, BridgeStatus::Pending);
    }

    #[test]
    fn test_all_statuses() {
        let statuses = vec![
            BridgeStatus::Pending,
            BridgeStatus::Confirmed,
            BridgeStatus::Executing,
            BridgeStatus::Completed,
            BridgeStatus::Failed,
            BridgeStatus::Timelocked,
        ];
        for s in statuses {
            let json = serde_json::to_string(&s).unwrap();
            let back: BridgeStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(s, back);
        }
    }

    #[test]
    fn test_bridge_direction() {
        assert_ne!(BridgeDirection::L1ToEvm, BridgeDirection::EvmToL1);
        let d = BridgeDirection::L1ToEvm;
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, "\"L1ToEvm\"");
    }

    #[test]
    fn test_l1_lock_event_serialization() {
        let lock = L1LockEvent {
            l1_tx_hash: "abc123".into(),
            l1_block_height: 1000,
            l1_sender: "zion1qtest".into(),
            amount_flowers: 5_000_000_000_000, // 5 ZION
            amount_wzion_wei: "5000000000000000000".into(),
            target_chain: "base".into(),
            evm_recipient: "0x1234567890abcdef1234567890abcdef12345678".into(),
            detected_at: chrono::Utc::now(),
            status: BridgeStatus::Pending,
            confirmations: 0,
        };
        let json = serde_json::to_string(&lock).unwrap();
        assert!(json.contains("\"l1_tx_hash\":\"abc123\""));
        assert!(json.contains("\"amount_flowers\":5000000000000"));
        let back: L1LockEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.l1_tx_hash, "abc123");
        assert_eq!(back.amount_flowers, 5_000_000_000_000);
    }

    #[test]
    fn test_evm_burn_event_serialization() {
        let burn = EvmBurnEvent {
            evm_tx_hash: "0xdeadbeef".into(),
            evm_block_number: 50000,
            evm_chain: "base".into(),
            evm_burner: "0xaaa".into(),
            amount_wzion_wei: "1000000000000000000".into(),
            amount_flowers: 1_000_000, // 1 ZION (post-3.0.3)
            l1_recipient: "zion1qrecipient".into(),
            burn_id: "burn001".into(),
            detected_at: chrono::Utc::now(),
            status: BridgeStatus::Confirmed,
            confirmations: 2,
        };
        let json = serde_json::to_string(&burn).unwrap();
        let back: EvmBurnEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.burn_id, "burn001");
        assert_eq!(back.confirmations, 2);
    }

    #[test]
    fn test_bridge_stats_default() {
        let stats = BridgeStats::default();
        assert_eq!(stats.total_locked_flowers, 0);
        assert_eq!(stats.total_operations, 0);
        assert!(stats.chain_stats.is_empty());
    }
}
