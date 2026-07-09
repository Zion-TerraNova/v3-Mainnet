//! ZION Fee Market — 100% Fee Burn Model
//!
//! All transaction fees are **burned** (destroyed). The coinbase output is
//! capped at the block reward only — miners do NOT receive transaction fees.
//!
//! This makes ZION deflationary over time as fees reduce circulating supply.
//!
//! All values in flowers (1 ZION = 1,000,000 flowers, post-3.0.3 fork).

use crate::emission;

// ── Constants ──────────────────────────────────────────────────────────

/// Minimum transaction fee: 1 flower (minimum unit, post-3.0.3 fork).
/// Pre-3.0.3: 1,000 flowers (0.000001 ZION at 12 decimals).
/// Post-3.0.3: 1 flower (0.000001 ZION at 6 decimals).
pub const MIN_TX_FEE: u64 = 1;

/// Minimum fee rate: 1 flower per byte of serialized transaction.
pub const MIN_FEE_RATE: u64 = 1;

/// Maximum transaction size: 100 KB.
pub const MAX_TX_SIZE: usize = 100_000;

/// Maximum single output amount: total supply in flowers.
/// Note: TOTAL_SUPPLY is u128 but no single output can exceed u64::MAX flowers,
/// which is ~18.4 × 10^18 — enough for the entire mining supply.
pub const MAX_OUTPUT_AMOUNT: u64 = u64::MAX;

// ── Fee calculation ────────────────────────────────────────────────────

/// Estimate serialized transaction size in bytes.
///
/// - Base: 32 (id) + 4 (version) + 8 (fee) + 8 (timestamp) = 52
/// - Per input: 32 (prev_hash) + 4 (index) + 64 (signature) + 32 (pubkey) = 132
/// - Per output: 8 (amount) + 44 (address) + 8 (memo overhead) = 60
pub fn estimate_tx_size(num_inputs: usize, num_outputs: usize) -> usize {
    52 + num_inputs * 132 + num_outputs * 60
}

/// Fee rate in flowers per byte.
pub fn fee_rate(fee: u64, tx_size_bytes: usize) -> u64 {
    if tx_size_bytes == 0 {
        return 0;
    }
    fee / tx_size_bytes as u64
}

/// Minimum required fee for a transaction of the given size.
///
/// Returns `max(MIN_TX_FEE, size × MIN_FEE_RATE)`.
pub fn minimum_fee_for_size(tx_size_bytes: usize) -> u64 {
    let rate_based = tx_size_bytes as u64 * MIN_FEE_RATE;
    rate_based.max(MIN_TX_FEE)
}

// ── Fee validation ─────────────────────────────────────────────────────

/// Validate that a transaction's fee meets minimum requirements.
pub fn validate_fee(fee: u64, tx_size_bytes: usize) -> Result<(), String> {
    if fee < MIN_TX_FEE {
        return Err(format!(
            "fee {} below minimum {} flowers (0.000001 ZION)",
            fee, MIN_TX_FEE
        ));
    }
    let min_for_size = minimum_fee_for_size(tx_size_bytes);
    if fee < min_for_size {
        return Err(format!(
            "fee {} below minimum for {} bytes (need {})",
            fee, tx_size_bytes, min_for_size
        ));
    }
    Ok(())
}

/// Validate all output amounts: non-zero, within supply cap, total within cap.
#[allow(clippy::absurd_extreme_comparisons)]
pub fn validate_outputs(outputs: &[(u64, &str)]) -> Result<(), String> {
    let mut total: u128 = 0;
    for (i, &(amount, _)) in outputs.iter().enumerate() {
        if amount == 0 {
            return Err(format!("output {} has zero amount", i));
        }
        if amount > MAX_OUTPUT_AMOUNT {
            return Err(format!(
                "output {} amount {} exceeds max {}",
                i, amount, MAX_OUTPUT_AMOUNT
            ));
        }
        total += amount as u128;
    }
    if total > MAX_OUTPUT_AMOUNT as u128 {
        return Err(format!(
            "total output {} exceeds max {}",
            total, MAX_OUTPUT_AMOUNT
        ));
    }
    Ok(())
}

// ── Fee burning ────────────────────────────────────────────────────────

/// Maximum allowed coinbase output for a block (reward only, no fees).
pub fn max_coinbase_output(block_height: u64) -> u64 {
    emission::block_subsidy(block_height)
}

/// Total fees burned in a block (sum of all non-coinbase tx fees).
pub fn total_fees_burned(fees: &[u64]) -> u64 {
    // First entry is coinbase (fee=0), rest are real txs
    fees.iter().skip(1).sum()
}

// ── Burn addresses ─────────────────────────────────────────────────────

/// Provable-burn address (no known private key).
pub const BURN_ADDRESS: &str = "zion1burn0000000000000000000000000000000dead";

/// DAO treasury address (main — Community Governance, 2.5B ZION).
pub const DAO_ADDRESS: &str = "zion1u5u7k43240d5l4d0x7q5m3c4a838z4k000cv3q0";

/// Bridge vault address.
///
/// This address is intentionally keyless at the protocol level. Unlocks back to
/// L1 must flow through a dedicated bridge authorization path rather than a
/// normal wallet-controlled spend.
///
/// Derived deterministically from seed `"ZION Bridge Vault V3 Mainnet v2 2026-07-06-HARD-RESET"`
/// via SHA-256 → derive_address. See `crypto::bridge_vault_address()`.
///
/// Hard reset 2026-07-06: new seed → new address.
/// Old vault: zion1w0r0a560l3j2y6f3v2f457n2u4d0n5v2g79w0t0 (from "ZION Bridge Vault V3 Mainnet")
pub const BRIDGE_VAULT_ADDRESS: &str = "zion1j53677g5k83030x3s2z2z644e7h07792q0u02t7";

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_fee_constant() {
        assert_eq!(MIN_TX_FEE, 1);
    }

    #[test]
    fn bridge_vault_address_matches_crypto_derivation() {
        assert_eq!(BRIDGE_VAULT_ADDRESS, crate::crypto::bridge_vault_address());
    }

    /// The provable-burn address must be deliberately unreachable by normal
    /// wallet flows: it is a hand-picked literal, not derived, and its
    /// checksum is not the canonical 4-char suffix produced by
    /// `derive_address`. `is_valid_address` must therefore reject it, so
    /// that any RPC / mempool path which front-gates deposits with
    /// `is_valid_address` cannot be tricked into treating `BURN_ADDRESS`
    /// as a regular recipient. Sending *to* this address is still allowed
    /// for `record_revenue` / fee-burn code paths that reference the
    /// literal constant directly, without going through address
    /// validation. (Audit finding §15.3.)
    #[test]
    fn burn_address_is_rejected_by_is_valid_address() {
        assert!(
            !crate::crypto::is_valid_address(BURN_ADDRESS),
            "BURN_ADDRESS must not pass is_valid_address; if this starts \
             passing, an attacker could spoof an addr that collides with \
             this checksum form",
        );
        assert_eq!(BURN_ADDRESS.len(), 44, "ZION addresses are 44 chars");
        assert!(
            BURN_ADDRESS.starts_with("zion1"),
            "ZION addresses must use the zion1 HRP",
        );
    }

    #[test]
    fn fee_rate_calculation() {
        assert_eq!(fee_rate(1000, 250), 4);
        assert_eq!(fee_rate(500, 250), 2);
        assert_eq!(fee_rate(100, 250), 0);
        assert_eq!(fee_rate(1000, 0), 0);
    }

    #[test]
    fn minimum_fee_for_size_small_tx() {
        // With MIN_TX_FEE=1 and MIN_FEE_RATE=1, rate-based fee dominates for any size >= 1.
        // minimum_fee_for_size(n) = max(MIN_TX_FEE, n * MIN_FEE_RATE) = max(1, n)
        assert_eq!(minimum_fee_for_size(100), 100);
        assert_eq!(minimum_fee_for_size(500), 500);
    }

    #[test]
    fn minimum_fee_for_size_large_tx() {
        assert_eq!(minimum_fee_for_size(2000), 2000);
        assert_eq!(minimum_fee_for_size(10_000), 10_000);
    }

    #[test]
    fn estimate_tx_size_typical() {
        // 1 input, 2 outputs (send + change)
        let size = estimate_tx_size(1, 2);
        assert_eq!(size, 52 + 132 + 120);
    }

    #[test]
    fn validate_fee_ok() {
        assert!(validate_fee(1_000, 250).is_ok());
        assert!(validate_fee(100_000, 500).is_ok());
    }

    #[test]
    fn validate_fee_too_low() {
        assert!(validate_fee(0, 250).is_err());
        // 249 < minimum_fee_for_size(250) = max(1, 250) = 250
        assert!(validate_fee(249, 250).is_err());
    }

    #[test]
    fn validate_fee_rate_too_low_for_large_tx() {
        assert!(validate_fee(1_000, 2_000).is_err());
    }

    #[test]
    fn validate_outputs_ok() {
        let outputs = vec![(1_000_000u64, "a"), (5_000_000u64, "b")];
        assert!(validate_outputs(&outputs).is_ok());
    }

    #[test]
    fn validate_outputs_zero_rejected() {
        assert!(validate_outputs(&[(0u64, "a")]).is_err());
    }

    #[test]
    fn validate_outputs_overflow_rejected() {
        let half = MAX_OUTPUT_AMOUNT / 2 + 1;
        assert!(validate_outputs(&[(half, "a"), (half, "b")]).is_err());
    }

    #[test]
    fn max_coinbase_block_1() {
        assert_eq!(max_coinbase_output(1), 5_400_067_000);
    }

    #[test]
    fn max_coinbase_genesis_is_zero() {
        assert_eq!(max_coinbase_output(0), 0);
    }

    #[test]
    fn max_coinbase_tail() {
        let tail_block = 10 * 5_256_000 + 1;
        assert_eq!(max_coinbase_output(tail_block), 724_784_723);
    }

    #[test]
    fn fees_burned_not_in_coinbase() {
        let reward = emission::block_subsidy(1);
        assert_eq!(max_coinbase_output(1), reward);
    }

    #[test]
    fn total_fees_burned_skips_coinbase() {
        let fees = vec![0, 5_000, 3_000]; // coinbase=0, tx1=5000, tx2=3000
        assert_eq!(total_fees_burned(&fees), 8_000);
    }
}
