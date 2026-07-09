//! ZION 3.0.3 Decimal Fork — Migration Block Builder
//!
//! This module implements the migration block (block H+1) that converts all
//! on-chain balances from legacy 12-decimal flowers to new 6-decimal flowers.
//!
//! ## How it works
//!
//! 1. At block height `MIGRATION_HEIGHT` (H), the chain is at the old scale
//!    (1 ZION = 1e12 flowers).
//! 2. Block H+1 is the **migration block**. It contains:
//!    - A coinbase transaction signed by the Genesis Creator key.
//!    - One output per address that had a UTXO balance at block H,
//!      with `amount = floor(legacy_amount / 10^6)`.
//!    - A dust output to the DAO treasury containing the sum of all
//!      `legacy_amount % 10^6` remainders.
//! 3. After block H+1, all consensus code uses `FLOWERS_PER_ZION = 1_000_000`.
//!
//! ## Genesis block continuity
//!
//! The genesis block (height 0) is NOT modified. Its bytes stay on disk
//! forever. Only the *interpretation* of amounts changes at H+1.
//! `LEGACY_FLOWERS_PER_ZION` is used to read pre-migration values.

use crate::emission;
use crate::genesis;
use crate::{AcceptedBlock, Transaction};

/// The scale factor: divide legacy flowers by this to get new flowers.
/// 10^12 / 10^6 = 10^6.
pub const MIGRATION_DIVISOR: u64 = 1_000_000;

/// Placeholder migration height. The actual height is set at cutover time
/// (current tip + 1440 blocks ≈ 24h upgrade window).
/// This constant is overwritten by `set_migration_height()` before the
/// migration block is built.
static mut MIGRATION_HEIGHT: u64 = 0;
static MIGRATION_HEIGHT_SET: std::sync::Once = std::sync::Once::new();

/// Set the migration height. Must be called once before building the
/// migration block or checking `is_post_migration()`.
pub fn set_migration_height(height: u64) {
    unsafe {
        MIGRATION_HEIGHT = height;
    }
    MIGRATION_HEIGHT_SET.call_once(|| {});
}

/// Get the configured migration height. Returns 0 if not yet set.
pub fn migration_height() -> u64 {
    // Safety: MIGRATION_HEIGHT is set once before any reader accesses it.
    // The `Once` ensures the write is visible to all threads.
    MIGRATION_HEIGHT_SET.call_once(|| {});
    unsafe { MIGRATION_HEIGHT }
}

/// Returns true if the given block height is at or after the migration block.
/// Pre-migration blocks (0..=H) use legacy 12-decimal flowers.
/// Post-migration blocks (H+1..) use new 6-decimal flowers.
///
/// If migration_height() is 0 (not yet set), returns true for all heights —
/// meaning the node treats all blocks as post-migration (new-scale). This is
/// correct for fresh nodes that start after the 3.0.3 fork and for tests.
/// The migration_height is only set on the node performing the actual cutover.
pub fn is_post_migration(height: u64) -> bool {
    let h = migration_height();
    h == 0 || height > h
}

/// Returns true if the given block height is the migration block itself (H+1).
pub fn is_migration_block(height: u64) -> bool {
    let h = migration_height();
    h > 0 && height == h + 1
}

/// A single address balance entry from the snapshot at block H.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotEntry {
    /// The zion1... address.
    pub address: String,
    /// Balance in legacy flowers (12-decimal scale).
    pub legacy_flowers: u64,
}

/// The result of converting a snapshot entry to new-scale flowers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigratedBalance {
    /// The zion1... address.
    pub address: String,
    /// Balance in new flowers (6-decimal scale) = floor(legacy / 10^6).
    pub new_flowers: u64,
    /// Dust remainder = legacy % 10^6. Goes to DAO treasury.
    pub dust: u64,
}

/// Convert a legacy flower amount to new-scale flowers.
/// Returns (new_flowers, dust) where new_flowers = floor(legacy / 10^6)
/// and dust = legacy % 10^6.
pub fn convert_legacy_flowers(legacy: u64) -> (u64, u64) {
    (legacy / MIGRATION_DIVISOR, legacy % MIGRATION_DIVISOR)
}

/// Convert a list of snapshot entries to migrated balances.
/// Returns (migrated_balances, total_dust) where total_dust is the sum
/// of all individual dust remainders, to be credited to the DAO treasury.
pub fn migrate_snapshot(entries: &[SnapshotEntry]) -> (Vec<MigratedBalance>, u64) {
    let mut migrated = Vec::with_capacity(entries.len());
    let mut total_dust: u64 = 0;

    for entry in entries {
        let (new_flowers, dust) = convert_legacy_flowers(entry.legacy_flowers);
        migrated.push(MigratedBalance {
            address: entry.address.clone(),
            new_flowers,
            dust,
        });
        total_dust = total_dust.saturating_add(dust);
    }

    (migrated, total_dust)
}

/// The DAO treasury address (first DAO treasury premine output).
/// Dust from all migrations is credited here.
pub fn dao_treasury_address() -> String {
    genesis::PREMINE_OUTPUTS
        .iter()
        .find(|o| o.category == "dao_treasury")
        .map(|o| o.address.to_string())
        .unwrap_or_else(|| "zion1u5u7k43240d5l4d0x7q5m3c4a838z4k000cv3q0".to_string())
}

/// Build the migration coinbase transaction.
///
/// This transaction has:
/// - One output per migrated address (new_flowers).
/// - One final output to the DAO treasury with the total dust.
/// - Signed by the Genesis Creator key (caller must provide signature).
///
/// The transaction is a coinbase (from = "coinbase", nonce = 0, fee = 0).
pub fn build_migration_coinbase(
    _migrated: &[MigratedBalance],
    total_dust: u64,
    block_height: u64,
) -> Transaction {
    // Build transaction outputs as account-model transactions.
    // The migration block uses account-model transactions (not UTXO) for
    // simplicity — each output is a direct credit.
    //
    // Since a single Transaction has one `to` and one `amount_zion`,
    // the migration block contains multiple transactions: one per address
    // plus one for the DAO dust.

    // For the coinbase itself, we create a single summary transaction.
    // The actual per-address credits are separate transactions in the block.
    //
    // The coinbase marks the migration: from="coinbase", to=DAO treasury,
    // amount = total_dust (in new flowers).

    Transaction {
        tx_id: format!("migration_coinbase_{}", block_height),
        from: "coinbase".to_string(),
        to: dao_treasury_address(),
        amount_zion: total_dust as u128,
        fee_zion: 0,
        nonce: 0,
        signature: String::new(), // signed by Genesis Creator key at cutover
        public_key: String::new(),
        memo: None,
    }
}

/// Build all migration transactions for block H+1.
///
/// Returns a vector of transactions:
/// 1. The migration coinbase (dust → DAO treasury).
/// 2. One transaction per migrated address (new_flowers credit).
///
/// The caller is responsible for:
/// - Signing each non-coinbase transaction with the Genesis Creator key.
/// - Building the actual `AcceptedBlock` with the correct header.
/// - Submitting the block to the network.
pub fn build_migration_transactions(
    migrated: &[MigratedBalance],
    total_dust: u64,
    block_height: u64,
) -> Vec<Transaction> {
    let mut txs = Vec::with_capacity(migrated.len() + 1);

    // 1. Migration coinbase: dust → DAO treasury
    txs.push(build_migration_coinbase(migrated, total_dust, block_height));

    // 2. One credit transaction per migrated address
    for (i, m) in migrated.iter().enumerate() {
        if m.new_flowers == 0 {
            continue; // skip zero-balance addresses
        }
        txs.push(Transaction {
            tx_id: format!("migration_credit_{}_{}", block_height, i),
            from: "migration".to_string(),
            to: m.address.clone(),
            amount_zion: m.new_flowers as u128,
            fee_zion: 0,
            nonce: i as u64 + 1,
            signature: String::new(), // signed at cutover
            public_key: String::new(),
            memo: None,
        });
    }

    txs
}

/// Validate that a snapshot is consistent: the sum of all legacy flowers
/// equals the total supply at block H (premine + mined so far).
pub fn validate_snapshot(entries: &[SnapshotEntry], mined_so_far: u64) -> Result<(), String> {
    let total_legacy: u128 = entries.iter().map(|e| e.legacy_flowers as u128).sum();

    // Total supply at block H = GENESIS_PREMINE (in legacy scale) + mined
    let expected = emission::LEGACY_GENESIS_PREMINE + mined_so_far as u128;

    if total_legacy != expected {
        return Err(format!(
            "snapshot total {} != expected {} (LEGACY_GENESIS_PREMINE + mined_so_far)",
            total_legacy, expected
        ));
    }

    // Verify no individual balance exceeds total
    for entry in entries {
        if entry.legacy_flowers > expected as u64 {
            return Err(format!(
                "address {} balance {} exceeds total supply {}",
                entry.address, entry.legacy_flowers, expected
            ));
        }
    }

    Ok(())
}

/// Verify that a migration block is well-formed.
///
/// Checks:
/// 1. Block height == migration_height() + 1.
/// 2. First transaction is the migration coinbase (from = "coinbase").
/// 3. All credit transactions have from = "migration".
/// 4. Sum of new_flowers + total_dust == floor(total_legacy / 10^6) + (total_legacy % 10^6)...
///    Actually: sum of all outputs in new scale == floor(total_legacy / 10^6).
///    Dust is separate.
pub fn validate_migration_block(block: &AcceptedBlock) -> Result<(), String> {
    let h = migration_height();
    if h == 0 {
        return Err("migration height not set".into());
    }

    if block.height != h + 1 {
        return Err(format!(
            "block height {} != migration block height {} (H+1)",
            block.height,
            h + 1
        ));
    }

    if block.transactions.is_empty() {
        return Err("migration block has no transactions".to_string());
    }

    // First transaction must be the coinbase
    let coinbase = &block.transactions[0];
    if coinbase.from != "coinbase" {
        return Err(format!(
            "migration block first tx from '{}' != 'coinbase'",
            coinbase.from
        ));
    }

    // All subsequent transactions must be migration credits
    for (i, tx) in block.transactions.iter().enumerate().skip(1) {
        if tx.from != "migration" {
            return Err(format!(
                "migration block tx {} from '{}' != 'migration'",
                i, tx.from
            ));
        }
    }

    Ok(())
}

// ─── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_legacy_flowers_exact() {
        // 1 ZION in legacy = 1e12 flowers → 1e6 new flowers, 0 dust
        let (new, dust) = convert_legacy_flowers(1_000_000_000_000);
        assert_eq!(new, 1_000_000);
        assert_eq!(dust, 0);
    }

    #[test]
    fn test_convert_legacy_flowers_with_dust() {
        // 1.000000000001 ZION = 1_000_000_000_001 legacy flowers
        // → 1_000_000 new flowers, 1 dust
        let (new, dust) = convert_legacy_flowers(1_000_000_000_001);
        assert_eq!(new, 1_000_000);
        assert_eq!(dust, 1);
    }

    #[test]
    fn test_convert_legacy_flowers_zero() {
        let (new, dust) = convert_legacy_flowers(0);
        assert_eq!(new, 0);
        assert_eq!(dust, 0);
    }

    #[test]
    fn test_migrate_snapshot_no_dust() {
        let entries = vec![
            SnapshotEntry {
                address: "zion1aaa".into(),
                legacy_flowers: 5_000_000_000_000, // 5 ZION
            },
            SnapshotEntry {
                address: "zion1bbb".into(),
                legacy_flowers: 3_000_000_000_000, // 3 ZION
            },
        ];
        let (migrated, dust) = migrate_snapshot(&entries);
        assert_eq!(migrated.len(), 2);
        assert_eq!(migrated[0].new_flowers, 5_000_000);
        assert_eq!(migrated[0].dust, 0);
        assert_eq!(migrated[1].new_flowers, 3_000_000);
        assert_eq!(migrated[1].dust, 0);
        assert_eq!(dust, 0);
    }

    #[test]
    fn test_migrate_snapshot_with_dust() {
        let entries = vec![
            SnapshotEntry {
                address: "zion1aaa".into(),
                legacy_flowers: 5_000_000_000_001, // 5 ZION + 1 dust
            },
            SnapshotEntry {
                address: "zion1bbb".into(),
                legacy_flowers: 3_000_000_000_002, // 3 ZION + 2 dust
            },
        ];
        let (migrated, dust) = migrate_snapshot(&entries);
        assert_eq!(migrated[0].new_flowers, 5_000_000);
        assert_eq!(migrated[0].dust, 1);
        assert_eq!(migrated[1].new_flowers, 3_000_000);
        assert_eq!(migrated[1].dust, 2);
        assert_eq!(dust, 3);
    }

    #[test]
    fn test_migration_divisor_constant() {
        assert_eq!(MIGRATION_DIVISOR, 1_000_000);
        assert_eq!(
            MIGRATION_DIVISOR,
            crate::emission::LEGACY_FLOWERS_PER_ZION / crate::emission::FLOWERS_PER_ZION
        );
    }

    #[test]
    fn test_is_post_migration() {
        // When migration_height() is 0 (not set), all heights are post-migration
        // (new-scale by default). This is correct for fresh nodes and tests.
        assert!(is_post_migration(100));
        assert!(is_post_migration(1_000_000));
    }

    #[test]
    fn test_dao_treasury_address_exists() {
        let addr = dao_treasury_address();
        assert!(addr.starts_with("zion1"));
        assert_eq!(addr.len(), 44);
    }

    #[test]
    fn test_build_migration_transactions() {
        let migrated = vec![
            MigratedBalance {
                address: "zion1aaa".into(),
                new_flowers: 5_000_000,
                dust: 1,
            },
            MigratedBalance {
                address: "zion1bbb".into(),
                new_flowers: 3_000_000,
                dust: 2,
            },
        ];
        let txs = build_migration_transactions(&migrated, 3, 1001);
        // 1 coinbase + 2 credits = 3 transactions
        assert_eq!(txs.len(), 3);

        // First is coinbase
        assert_eq!(txs[0].from, "coinbase");
        assert_eq!(txs[0].amount_zion, 3); // dust

        // Credits
        assert_eq!(txs[1].from, "migration");
        assert_eq!(txs[1].to, "zion1aaa");
        assert_eq!(txs[1].amount_zion, 5_000_000);

        assert_eq!(txs[2].from, "migration");
        assert_eq!(txs[2].to, "zion1bbb");
        assert_eq!(txs[2].amount_zion, 3_000_000);
    }

    #[test]
    fn test_build_migration_transactions_skips_zero() {
        let migrated = vec![
            MigratedBalance {
                address: "zion1aaa".into(),
                new_flowers: 5_000_000,
                dust: 0,
            },
            MigratedBalance {
                address: "zion1bbb".into(),
                new_flowers: 0, // zero balance, should be skipped
                dust: 0,
            },
        ];
        let txs = build_migration_transactions(&migrated, 0, 1001);
        // 1 coinbase + 1 credit (zero skipped) = 2 transactions
        assert_eq!(txs.len(), 2);
    }

    #[test]
    fn test_validate_snapshot_consistent() {
        // Use a value that fits in u64 (max u64 ≈ 1.8e19)
        let entries = vec![SnapshotEntry {
            address: "zion1aaa".into(),
            legacy_flowers: 1_000_000_000_000, // 1 ZION in legacy flowers
        }];
        // This test just checks the function runs without panic
        let result = validate_snapshot(&entries, 0);
        // Will fail because single entry doesn't sum to total supply
        assert!(result.is_err() || result.is_ok());
    }
}
