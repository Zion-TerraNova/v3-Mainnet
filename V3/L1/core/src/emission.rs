//! ZION Emission Schedule — Decade Decay
//!
//! Constitutional reference: `docs/mainnet/MAINNET_CONSTITUTION.md`
//!
//! All values in flowers (1 ZION = 1_000_000 flowers, post-3.0.3 fork).
//! Pre-3.0.3: 1 ZION = 1_000_000_000_000 flowers (12 decimals).
//! Post-3.0.3 (block H+1): 1 ZION = 1_000_000 flowers (6 decimals).
//! Block height 0 is the genesis block (premine only, no mining reward).
//! Decade Decay: reward × (4/5) every 5,256,000 blocks.
//! After decade 10 (block 52,560,001+): perpetual tail emission.

/// 1 ZION = 1_000_000 flowers (6 decimal places, post-3.0.3 fork).
pub const FLOWERS_PER_ZION: u64 = 1_000_000;

/// Legacy: 1 ZION = 1_000_000_000_000 flowers (12 decimals, pre-3.0.3).
/// Used for genesis block values which are preserved on-disk (block hash continuity).
/// The migration block at H+1 divides all legacy values by 10⁶ to convert to new flowers.
pub const LEGACY_FLOWERS_PER_ZION: u64 = 1_000_000_000_000;

/// Legacy genesis premine in old-flower scale (12 decimals).
/// Genesis block bytes use this scale; post-migration it equals GENESIS_PREMINE.
pub const LEGACY_GENESIS_PREMINE: u128 = 16_780_000_000_u128 * LEGACY_FLOWERS_PER_ZION as u128;

/// Total supply: 144,000,000,000 ZION in flowers (u128 required).
pub const TOTAL_SUPPLY: u128 = 144_000_000_000_u128 * FLOWERS_PER_ZION as u128;

/// Genesis premine: 16,780,000,000 ZION in flowers.
pub const GENESIS_PREMINE: u128 = 16_780_000_000_u128 * FLOWERS_PER_ZION as u128;

/// Mining emission: total supply minus premine.
pub const MINING_EMISSION: u128 = TOTAL_SUPPLY - GENESIS_PREMINE;

/// Block time target in seconds.
pub const BLOCK_TIME_SECONDS: u64 = 60;

/// Blocks per year: 365 × 24 × 60 = 525,600.
pub const BLOCKS_PER_YEAR: u64 = 525_600;

/// Blocks per decade: 10 × 525,600 = 5,256,000.
pub const BLOCKS_PER_DECADE: u64 = 10 * BLOCKS_PER_YEAR;

/// Decay numerator (reward × 4/5 each decade).
pub const DECAY_NUMERATOR: u64 = 4;

/// Decay denominator.
pub const DECAY_DENOMINATOR: u64 = 5;

/// Number of decades with active decay before tail emission.
pub const MAX_DECAY_DECADES: u64 = 10;

/// Base block reward (Decade 1): 5,400.067 ZION = 5,400,067,000 flowers (post-3.0.3).
pub const BASE_REWARD: u64 = 5_400_067_000;

/// Tail emission reward: BASE_REWARD × (4/5)^9 = 724,784,723 flowers (~724.785 ZION).
/// This reward continues forever after decade 10.
pub const TAIL_REWARD: u64 = 724_784_723;

/// Coinbase maturity: outputs unspendable for this many blocks.
pub const COINBASE_MATURITY: u64 = 100;

// ─── Fee Split Constants (Protocol-Level) ────────────────────────────────────
//
// WARNING: These four percentages are duplicated in several crates.
// If you change any value here, you MUST also update:
//   1. V3/L1/cosmic-harmony/src/revenue.rs  (ZION_*_PCT constants)
//   2. V3/L1/pool/src/pplns.rs              (FeeConfig::default)
//   3. V3/L1/pool/src/bin/server.rs        (parse_env_u64 fallbacks)
//   4. V3/docs/MAINNET_CONSTANTS.md
//   5. docs/WP-Mainet/ whitepapers
// The `fee_split_consistency` test below guards against accidental drift.

/// Miner share: 89% of block subsidy.
pub const MINER_PCT: u64 = 89;

/// Humanitarian tithe: 5% of block subsidy.
pub const HUMANITARIAN_PCT: u64 = 5;

/// Issobella fund: 5% of block subsidy.
pub const ISSOBELLA_PCT: u64 = 5;

/// Pool fee: 1% of block subsidy. This slot is **BURNED** — it is never
/// minted into any wallet. The coinbase only creates the miner / humanitarian
/// / issobella outputs (89/5/5), so the effective per-block emission is 99% of
/// the subsidy and the remaining 1% is permanently removed from supply.
pub const POOL_FEE_PCT: u64 = 1;

/// Compute the fee split for a given block subsidy.
/// Returns (miner, humanitarian, issobella, pool_fee) in flowers.
///
/// `pool_fee` is the BURNED amount — it is NOT paid to any address. It is
/// returned for ratio/accounting reference only (see [`minted_subsidy`] and
/// [`burned_subsidy`]). The miner portion absorbs any rounding dust so the
/// four parts always sum to `subsidy`.
pub fn fee_split(subsidy: u64) -> (u64, u64, u64, u64) {
    let humanitarian = subsidy * HUMANITARIAN_PCT / 100;
    let issobella = subsidy * ISSOBELLA_PCT / 100;
    let pool_fee = subsidy * POOL_FEE_PCT / 100;
    let miner = subsidy - humanitarian - issobella - pool_fee;
    (miner, humanitarian, issobella, pool_fee)
}

/// Amount burned per block: the 1% pool-fee slot that is never minted.
pub fn burned_subsidy(subsidy: u64) -> u64 {
    let (_miner, _humanitarian, _issobella, pool_fee) = fee_split(subsidy);
    pool_fee
}

/// Total newly-minted coinbase amount per block: `subsidy` minus the burned
/// pool fee. This is the sum of the three coinbase outputs (miner 89% +
/// humanitarian 5% + issobella 5%).
pub fn minted_subsidy(subsidy: u64) -> u64 {
    subsidy - burned_subsidy(subsidy)
}

/// Block subsidy for a given height in flowers.
///
/// Height 0 is genesis (premine only — returns 0).
/// Heights 1..=5,256,000 earn the base reward (Decade 1).
/// Each subsequent decade decays by ×(4/5).
/// After decade 10 the tail reward continues forever.
pub fn block_subsidy(height: u64) -> u64 {
    if height == 0 {
        return 0;
    }

    let decade = (height - 1) / BLOCKS_PER_DECADE;

    if decade >= MAX_DECAY_DECADES {
        return TAIL_REWARD;
    }

    let mut reward = BASE_REWARD;
    for _ in 0..decade {
        reward = reward * DECAY_NUMERATOR / DECAY_DENOMINATOR;
    }
    reward
}

/// Convert flowers to whole ZION (truncating).
pub fn flowers_to_zion(flowers: u64) -> u64 {
    flowers / FLOWERS_PER_ZION
}

/// Convert whole ZION to flowers.
pub fn zion_to_flowers(zion: u64) -> u64 {
    zion.saturating_mul(FLOWERS_PER_ZION)
}

/// Display a flower amount as a human-readable ZION string (e.g. "5400.067000").
pub fn format_zion(flowers: u64) -> String {
    let whole = flowers / FLOWERS_PER_ZION;
    let frac = flowers % FLOWERS_PER_ZION;
    format!("{whole}.{frac:06}")
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_no_reward() {
        assert_eq!(block_subsidy(0), 0);
    }

    #[test]
    fn block_1_base_reward() {
        assert_eq!(block_subsidy(1), BASE_REWARD);
        assert_eq!(block_subsidy(1), 5_400_067_000);
    }

    #[test]
    fn decade_1_constant() {
        assert_eq!(block_subsidy(1), BASE_REWARD);
        assert_eq!(block_subsidy(1_000_000), BASE_REWARD);
        assert_eq!(block_subsidy(BLOCKS_PER_DECADE), BASE_REWARD);
    }

    #[test]
    fn decade_2_decay() {
        let d2 = BLOCKS_PER_DECADE + 1;
        let expected = BASE_REWARD * 4 / 5;
        assert_eq!(block_subsidy(d2), expected);
        assert_eq!(expected, 4_320_053_600);
    }

    #[test]
    fn decade_3_decay() {
        let d3 = 2 * BLOCKS_PER_DECADE + 1;
        let expected = BASE_REWARD * 4 / 5 * 4 / 5;
        assert_eq!(block_subsidy(d3), expected);
    }

    #[test]
    fn decade_boundary_exact() {
        // Last block of decade 1 still has decade-1 reward.
        assert_eq!(block_subsidy(BLOCKS_PER_DECADE), BASE_REWARD);
        // First block of decade 2 has decayed reward.
        assert_eq!(block_subsidy(BLOCKS_PER_DECADE + 1), BASE_REWARD * 4 / 5);
    }

    #[test]
    fn decade_10_last_decay() {
        let d10 = 9 * BLOCKS_PER_DECADE + 1;
        let mut expected = BASE_REWARD;
        for _ in 0..9 {
            expected = expected * 4 / 5;
        }
        assert_eq!(block_subsidy(d10), expected);
    }

    #[test]
    fn tail_emission_starts_at_decade_11() {
        let tail_start = 10 * BLOCKS_PER_DECADE + 1;
        assert_eq!(block_subsidy(tail_start), TAIL_REWARD);
        assert_eq!(block_subsidy(tail_start + 1_000_000), TAIL_REWARD);
        assert_eq!(block_subsidy(u64::MAX), TAIL_REWARD);
    }

    #[test]
    fn tail_reward_matches_constitution() {
        let zion = TAIL_REWARD as f64 / FLOWERS_PER_ZION as f64;
        assert!(
            (zion - 724.785).abs() < 0.001,
            "tail should be ~724.785 ZION, got {zion}"
        );
    }

    #[test]
    fn reward_monotonically_decreases() {
        let mut prev = block_subsidy(1);
        for d in 1..MAX_DECAY_DECADES {
            let curr = block_subsidy(d * BLOCKS_PER_DECADE + 1);
            assert!(curr < prev, "decade {d} not less than previous");
            prev = curr;
        }
    }

    #[test]
    fn reward_never_zero_after_genesis() {
        for h in [
            1,
            100,
            1_000_000,
            52_560_000,
            52_560_001,
            100_000_000,
            u64::MAX,
        ] {
            assert!(block_subsidy(h) > 0, "reward at height {h} should be > 0");
        }
    }

    #[test]
    fn constants_consistency() {
        assert_eq!(MINING_EMISSION, TOTAL_SUPPLY - GENESIS_PREMINE);
        assert_eq!(TOTAL_SUPPLY, 144_000_000_000_000_000_u128);
        assert_eq!(GENESIS_PREMINE, 16_780_000_000_000_000_u128);
        assert_eq!(MINING_EMISSION, 127_220_000_000_000_000_u128);
        assert_eq!(BLOCKS_PER_DECADE, 5_256_000);
        assert_eq!(BLOCKS_PER_YEAR, 525_600);
    }

    #[test]
    fn tail_reward_is_decade_10_decay() {
        // Tail = reward after 9 decay steps (decade 10 value), which then persists forever.
        // Decade 1: 0 decays, Decade 2: 1 decay, ..., Decade 10: 9 decays.
        // After decade 10, tail = decade-10 reward = BASE × (4/5)^9.
        let mut r = BASE_REWARD;
        for _ in 0..9 {
            r = r * DECAY_NUMERATOR / DECAY_DENOMINATOR;
        }
        assert_eq!(r, TAIL_REWARD);
    }

    #[test]
    fn hundred_year_emission_within_mining_supply() {
        let mut total: u128 = 0;
        let mut reward = BASE_REWARD as u128;
        for _ in 0..MAX_DECAY_DECADES {
            total += reward * BLOCKS_PER_DECADE as u128;
            reward = reward * DECAY_NUMERATOR as u128 / DECAY_DENOMINATOR as u128;
        }
        assert!(
            total <= MINING_EMISSION,
            "100-year emission {total} exceeds mining supply {MINING_EMISSION}"
        );
    }

    #[test]
    fn format_zion_display() {
        assert_eq!(format_zion(BASE_REWARD), "5400.067000");
        assert_eq!(format_zion(TAIL_REWARD), "724.784723");
        assert_eq!(format_zion(0), "0.000000");
        assert_eq!(format_zion(FLOWERS_PER_ZION), "1.000000");
    }

    #[test]
    fn flowers_conversion_roundtrip() {
        assert_eq!(flowers_to_zion(zion_to_flowers(5400)), 5400);
        assert_eq!(flowers_to_zion(BASE_REWARD), 5400);
        assert_eq!(zion_to_flowers(1), FLOWERS_PER_ZION);
    }

    #[test]
    fn fee_split_sums_to_subsidy() {
        let subsidy = BASE_REWARD;
        let (miner, humanitarian, issobella, pool_fee) = fee_split(subsidy);
        assert_eq!(miner + humanitarian + issobella + pool_fee, subsidy);
    }

    #[test]
    #[allow(clippy::identity_op)] // keep `* 1` to mirror the 89/5/5/1 split visually
    fn fee_split_percentages_correct() {
        let subsidy = BASE_REWARD;
        let (miner, humanitarian, issobella, pool_fee) = fee_split(subsidy);
        // 89%
        assert_eq!(
            miner,
            subsidy - subsidy * 5 / 100 - subsidy * 5 / 100 - subsidy * 1 / 100
        );
        // 5%
        assert_eq!(humanitarian, subsidy * 5 / 100);
        assert_eq!(issobella, subsidy * 5 / 100);
        // 1%
        assert_eq!(pool_fee, subsidy * 1 / 100);
    }

    #[test]
    fn fee_split_tail_sums_to_subsidy() {
        let (m, h, i, p) = fee_split(TAIL_REWARD);
        assert_eq!(m + h + i + p, TAIL_REWARD);
    }

    /// Guard against accidental drift between `emission.rs` and the duplicate
    /// constants in `cosmic-harmony/src/revenue.rs`.  If this fails, update
    /// ALL locations listed in the WARNING comment above `MINER_PCT`.
    #[test]
    fn fee_split_consistency_with_cosmic_harmony() {
        assert_eq!(MINER_PCT, zion_cosmic_harmony::ZION_MINER_PCT);
        assert_eq!(HUMANITARIAN_PCT, zion_cosmic_harmony::ZION_HUMANITARIAN_PCT);
        assert_eq!(ISSOBELLA_PCT, zion_cosmic_harmony::ZION_ISSOBELLA_PCT);
        assert_eq!(POOL_FEE_PCT, zion_cosmic_harmony::ZION_POOL_PCT);
    }

    #[test]
    fn fee_split_zero() {
        let (m, h, i, p) = fee_split(0);
        assert_eq!((m, h, i, p), (0, 0, 0, 0));
    }
}
