//! ZION V3 — LWMA Difficulty Adjustment Algorithm
//!
//! Constitutional requirements (MAINNET_CONSTITUTION §3):
//!   - Target block time:    60 seconds
//!   - LWMA window:          60 blocks
//!   - Per-block clamp:      ±25 %
//!   - Solve-time clamp:     30–120 s per interval
//!   - Minimum difficulty:   1 000
//!   - Maximum difficulty:   u64::MAX / 1 000
//!
//! Reference: Zawy's LWMA (used by Monero, Grin, LOKI, etc.)

use crate::DifficultyTarget;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Target block time in seconds.
pub const TARGET_BLOCK_TIME: u64 = 60;

/// Number of previous blocks considered by the LWMA window.
pub const LWMA_WINDOW: usize = 60;

/// Minimum per-interval solve time (clamped), TARGET / 2.
pub const MIN_SOLVE_TIME: u64 = 30;

/// Maximum per-interval solve time (clamped), TARGET × 2.
pub const MAX_SOLVE_TIME: u64 = 120;

/// Absolute difficulty floor.
pub const MIN_DIFFICULTY: u64 = 1_000;

/// Absolute difficulty ceiling.
pub const MAX_DIFFICULTY: u64 = u64::MAX / 1_000;

/// Difficulty used for the genesis block and early chain bootstrap.
pub const GENESIS_DIFFICULTY: u64 = MIN_DIFFICULTY;

// ±25 % as exact integer fractions — avoids f64 non-determinism.
const CLAMP_UP_NUM: u128 = 5;
const CLAMP_UP_DEN: u128 = 4; // 5/4 = 1.25
const CLAMP_DN_NUM: u128 = 3;
const CLAMP_DN_DEN: u128 = 4; // 3/4 = 0.75

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Timestamp (seconds) + difficulty pair consumed by the LWMA algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockInfo {
    pub timestamp: u64,
    pub difficulty: u64,
}

// ---------------------------------------------------------------------------
// LWMA core
// ---------------------------------------------------------------------------

/// Calculate the difficulty for the *next* block using LWMA.
///
/// `window` must be **oldest-first**.  An ideal input contains
/// `LWMA_WINDOW + 1` entries (N + 1 timestamps → N solve-time intervals).
/// The algorithm adapts gracefully with fewer entries.
pub fn lwma_next_difficulty(window: &[BlockInfo]) -> u64 {
    if window.len() < 2 {
        return window
            .last()
            .map(|b| b.difficulty.max(MIN_DIFFICULTY))
            .unwrap_or(GENESIS_DIFFICULTY);
    }

    let n = window.len() - 1;

    let mut weighted_solve_sum: u128 = 0;
    let mut weighted_diff_sum: u128 = 0;
    let mut weight_sum: u128 = 0;

    for i in 1..=n {
        let raw = window[i].timestamp.saturating_sub(window[i - 1].timestamp);
        let solve = raw.clamp(MIN_SOLVE_TIME, MAX_SOLVE_TIME);
        let w = i as u128;
        weighted_solve_sum += solve as u128 * w;
        weighted_diff_sum += window[i].difficulty as u128 * w;
        weight_sum += w;
    }

    if weight_sum == 0 || weighted_solve_sum == 0 {
        return window.last().unwrap().difficulty.max(MIN_DIFFICULTY);
    }

    // next = Σ(diff·w) × TARGET / Σ(solve·w)
    let next_128 = weighted_diff_sum * TARGET_BLOCK_TIME as u128 / weighted_solve_sum;
    let mut next = if next_128 > MAX_DIFFICULTY as u128 {
        MAX_DIFFICULTY
    } else {
        next_128 as u64
    };

    // ±25 % clamp relative to the most recent block (integer arithmetic).
    let prev = window.last().unwrap().difficulty as u128;
    let max_allowed = (prev * CLAMP_UP_NUM / CLAMP_UP_DEN) as u64;
    let min_allowed = (prev * CLAMP_DN_NUM / CLAMP_DN_DEN) as u64;
    next = next.clamp(min_allowed, max_allowed);

    // Global floor / ceiling.
    next.clamp(MIN_DIFFICULTY, MAX_DIFFICULTY)
}

// ---------------------------------------------------------------------------
// Target ↔ difficulty conversion
// ---------------------------------------------------------------------------

/// Convert a u64 difficulty to a 256-bit target: `target = (2²⁵⁶ − 1) / difficulty`.
pub fn difficulty_to_target(difficulty: u64) -> DifficultyTarget {
    if difficulty <= 1 {
        return DifficultyTarget::MAX;
    }
    let d = difficulty as u128;
    let mut bytes = [0u8; 32];
    let mut remainder: u128 = 0;

    // Long division of [0xFF; 32] (= 2²⁵⁶ − 1) by d, 8 bytes per iteration.
    for chunk in 0..4 {
        let dividend = (remainder << 64) | 0xFFFF_FFFF_FFFF_FFFFu128;
        let quotient = dividend / d;
        remainder = dividend % d;
        bytes[chunk * 8..(chunk + 1) * 8].copy_from_slice(&(quotient as u64).to_be_bytes());
    }

    DifficultyTarget { bytes }
}

/// Convert a 256-bit target back to a u64 difficulty: `difficulty = (2²⁵⁶ − 1) / target`.
///
/// Returns [`MIN_DIFFICULTY`] when `target` is all-zeros (division by zero guard).
pub fn target_to_difficulty(target: &DifficultyTarget) -> u64 {
    // Check for all-zero target.
    if target.bytes.iter().all(|&b| b == 0) {
        return MIN_DIFFICULTY;
    }
    // If target == MAX, difficulty is 1.
    if target.bytes == [0xFF; 32] {
        return 1;
    }
    // Use leading byte position for fast approximation:
    // difficulty ≈ 2^256 / target.  We compute via 128-bit division of the
    // most-significant 16 bytes of MAX / target.
    let mut target_val: u128 = 0;
    for &b in &target.bytes[..16] {
        target_val = (target_val << 8) | b as u128;
    }
    if target_val == 0 {
        // Target is extremely small (only lower 16 bytes set) → very high difficulty.
        return MAX_DIFFICULTY;
    }
    let max_val: u128 = u128::MAX; // top 16 bytes of 2^256-1
    let diff_128 = max_val / target_val;
    if diff_128 > MAX_DIFFICULTY as u128 {
        MAX_DIFFICULTY
    } else {
        (diff_128 as u64).max(MIN_DIFFICULTY)
    }
}

/// Encode a `DifficultyTarget` as compact nBits (Bitcoin-style).
///
/// Format: `(size << 24) | mantissa` where
/// `target ≈ mantissa × 256^(size − 3)`.
pub fn target_to_compact(target: &DifficultyTarget) -> u32 {
    let first_nz = match target.bytes.iter().position(|&b| b != 0) {
        Some(i) => i,
        None => return 0,
    };

    let mut size = (32 - first_nz) as u32;
    let b0 = target.bytes[first_nz] as u32;
    let b1 = if first_nz + 1 < 32 {
        target.bytes[first_nz + 1] as u32
    } else {
        0
    };
    let b2 = if first_nz + 2 < 32 {
        target.bytes[first_nz + 2] as u32
    } else {
        0
    };
    let mut compact = (b0 << 16) | (b1 << 8) | b2;

    // If top bit of mantissa is set, shift right to avoid sign ambiguity.
    if compact & 0x0080_0000 != 0 {
        compact >>= 8;
        size += 1;
    }

    (size << 24) | (compact & 0x007F_FFFF)
}

/// Decode compact nBits into a `DifficultyTarget`.
pub fn compact_to_target(bits: u32) -> DifficultyTarget {
    let size = (bits >> 24) as usize;
    let mantissa = bits & 0x007F_FFFF;

    if size == 0 || mantissa == 0 {
        return DifficultyTarget { bytes: [0u8; 32] };
    }

    let mut bytes = [0u8; 32];

    if size <= 3 {
        let word = mantissa >> (8 * (3 - size));
        for i in (0..size).rev() {
            let byte_pos = 32 - 1 - i;
            bytes[byte_pos] = ((word >> (8 * i)) & 0xFF) as u8;
        }
    } else {
        let start = 32 - size;
        bytes[start] = ((mantissa >> 16) & 0xFF) as u8;
        if start + 1 < 32 {
            bytes[start + 1] = ((mantissa >> 8) & 0xFF) as u8;
        }
        if start + 2 < 32 {
            bytes[start + 2] = (mantissa & 0xFF) as u8;
        }
    }

    DifficultyTarget { bytes }
}

// ---------------------------------------------------------------------------
// Difficulty monitor — runtime analytics for live mining
// ---------------------------------------------------------------------------

/// Statistics about recent mining difficulty / block-time behavior.
#[derive(Debug, Clone)]
pub struct DifficultyStats {
    /// Number of blocks in the sample.
    pub sample_size: usize,
    /// Mean observed solve time over the sample (seconds).
    pub mean_solve_time: f64,
    /// Ratio of mean_solve_time / TARGET_BLOCK_TIME (1.0 = perfect).
    pub timing_ratio: f64,
    /// Estimated network hashrate (hashes/second) derived from difficulty and
    /// block times.  Uses: hashrate ≈ difficulty × 2³² / mean_solve_time.
    pub estimated_hashrate: f64,
    /// Current difficulty (most recent block in window).
    pub current_difficulty: u64,
    /// Predicted difficulty for the next block (LWMA forward projection).
    pub predicted_next: u64,
}

/// Analyse a recent window of blocks and produce mining-relevant statistics.
///
/// `window` must be **oldest-first** and contain at least 2 entries.
/// The same input format as [`lwma_next_difficulty`].
pub fn difficulty_stats(window: &[BlockInfo]) -> Option<DifficultyStats> {
    if window.len() < 2 {
        return None;
    }

    let n = window.len() - 1;

    // Compute mean solve time (clamped same as LWMA sees it).
    let mut sum_solve: u64 = 0;
    for i in 1..=n {
        let raw = window[i].timestamp.saturating_sub(window[i - 1].timestamp);
        sum_solve += raw.clamp(MIN_SOLVE_TIME, MAX_SOLVE_TIME);
    }
    let mean_solve = sum_solve as f64 / n as f64;
    let timing_ratio = mean_solve / TARGET_BLOCK_TIME as f64;

    let current_difficulty = window.last().unwrap().difficulty;

    // Estimated hashrate: hashrate ≈ difficulty × 2^32 / solve_time
    // (Bitcoin convention — difficulty 1 ≈ 2^32 hashes per block)
    let estimated_hashrate = if mean_solve > 0.0 {
        current_difficulty as f64 * (1u64 << 32) as f64 / mean_solve
    } else {
        0.0
    };

    let predicted_next = lwma_next_difficulty(window);

    Some(DifficultyStats {
        sample_size: n,
        mean_solve_time: mean_solve,
        timing_ratio,
        estimated_hashrate,
        current_difficulty,
        predicted_next,
    })
}

/// Predict difficulty N blocks into the future assuming constant hashrate.
///
/// Returns a Vec of predicted difficulties for blocks 1..=horizon.
/// Assumes each future block takes `mean_solve_time` seconds (from current
/// window stats).  This is an approximation — real difficulty will vary as
/// new blocks arrive with different timestamps.
pub fn predict_difficulty(window: &[BlockInfo], horizon: usize) -> Vec<u64> {
    if window.len() < 2 || horizon == 0 {
        return Vec::new();
    }

    let stats = match difficulty_stats(window) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let solve_time = (stats.mean_solve_time as u64).clamp(MIN_SOLVE_TIME, MAX_SOLVE_TIME);

    let mut chain: Vec<BlockInfo> = window.to_vec();
    let mut predictions = Vec::with_capacity(horizon);

    for _ in 0..horizon {
        let start = chain.len().saturating_sub(LWMA_WINDOW + 1);
        let next_diff = lwma_next_difficulty(&chain[start..]);
        let last_ts = chain.last().unwrap().timestamp;
        chain.push(BlockInfo {
            timestamp: last_ts + solve_time,
            difficulty: next_diff,
        });
        predictions.push(next_diff);
    }

    predictions
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_window(n: usize, base_diff: u64, solve_time: u64) -> Vec<BlockInfo> {
        (0..=n)
            .map(|i| BlockInfo {
                timestamp: 1_000_000 + (i as u64) * solve_time,
                difficulty: base_diff,
            })
            .collect()
    }

    // --- LWMA algorithm tests ---

    #[test]
    fn perfect_timing_preserves_difficulty() {
        let window = make_window(60, 10_000, TARGET_BLOCK_TIME);
        assert_eq!(lwma_next_difficulty(&window), 10_000);
    }

    #[test]
    fn fast_blocks_increase_clamped() {
        let window = make_window(60, 10_000, 30);
        assert_eq!(lwma_next_difficulty(&window), 12_500);
    }

    #[test]
    fn slow_blocks_decrease_clamped() {
        let window = make_window(60, 10_000, 120);
        assert_eq!(lwma_next_difficulty(&window), 7_500);
    }

    #[test]
    fn extreme_fast_clamped_up() {
        let window = make_window(60, 10_000, 1);
        // Solve times clamped to MIN_SOLVE_TIME=30, so ratio=60/30=2× but ±25% clamp
        assert_eq!(lwma_next_difficulty(&window), 12_500);
    }

    #[test]
    fn extreme_slow_clamped_down() {
        let window = make_window(60, 10_000, 600);
        // Solve times clamped to MAX_SOLVE_TIME=120, so ratio=60/120=0.5× but ±25% clamp
        assert_eq!(lwma_next_difficulty(&window), 7_500);
    }

    #[test]
    fn minimum_difficulty_floor() {
        let window = make_window(60, MIN_DIFFICULTY, 120);
        assert!(lwma_next_difficulty(&window) >= MIN_DIFFICULTY);
    }

    #[test]
    fn short_window_two_blocks() {
        let window = vec![
            BlockInfo {
                timestamp: 1000,
                difficulty: 5_000,
            },
            BlockInfo {
                timestamp: 1060,
                difficulty: 5_000,
            },
        ];
        assert_eq!(lwma_next_difficulty(&window), 5_000);
    }

    #[test]
    fn single_block_returns_its_difficulty() {
        let window = vec![BlockInfo {
            timestamp: 1000,
            difficulty: 8_000,
        }];
        assert_eq!(lwma_next_difficulty(&window), 8_000);
    }

    #[test]
    fn empty_window_returns_genesis() {
        assert_eq!(lwma_next_difficulty(&[]), GENESIS_DIFFICULTY);
    }

    #[test]
    fn recent_blocks_weighted_more() {
        let mut window = Vec::new();
        let mut ts = 1_000_000u64;
        window.push(BlockInfo {
            timestamp: ts,
            difficulty: 10_000,
        });
        for _ in 1..=50 {
            ts += 60;
            window.push(BlockInfo {
                timestamp: ts,
                difficulty: 10_000,
            });
        }
        for _ in 51..=60 {
            ts += 30;
            window.push(BlockInfo {
                timestamp: ts,
                difficulty: 10_000,
            });
        }
        assert!(
            lwma_next_difficulty(&window) > 10_000,
            "recent fast blocks should increase difficulty"
        );
    }

    #[test]
    fn stability_simulation_200_blocks() {
        let solve_times = [55u64, 65, 58, 62, 50, 70, 57, 63, 59, 61];
        let mut blocks = vec![BlockInfo {
            timestamp: 1_000_000,
            difficulty: 10_000,
        }];
        let mut ts = 1_000_000u64;

        for i in 0..200 {
            ts += solve_times[i % solve_times.len()];
            let start = blocks.len().saturating_sub(LWMA_WINDOW + 1);
            let diff = lwma_next_difficulty(&blocks[start..]);
            blocks.push(BlockInfo {
                timestamp: ts,
                difficulty: diff,
            });
        }

        let final_diff = blocks.last().unwrap().difficulty;
        assert!(
            (5_000..=20_000).contains(&final_diff),
            "after 200 varied blocks, difficulty {final_diff} should stabilize near 10k"
        );
    }

    #[test]
    fn no_overflow_high_difficulty() {
        let window = make_window(60, MAX_DIFFICULTY / 2, 30);
        let next = lwma_next_difficulty(&window);
        assert!(next > 0 && next <= MAX_DIFFICULTY);
    }

    #[test]
    fn deterministic() {
        let window = make_window(60, 10_000, 45);
        let r1 = lwma_next_difficulty(&window);
        let r2 = lwma_next_difficulty(&window);
        assert_eq!(r1, r2);
    }

    // --- Target conversion tests ---

    #[test]
    fn difficulty_1_is_max_target() {
        assert_eq!(difficulty_to_target(1), DifficultyTarget::MAX);
    }

    #[test]
    fn higher_difficulty_lower_target() {
        let t1 = difficulty_to_target(100);
        let t2 = difficulty_to_target(1000);
        assert!(t2.bytes < t1.bytes);
    }

    #[test]
    fn target_allows_low_hash() {
        let target = difficulty_to_target(1_000);
        assert!(target.allows(&[0u8; 32]));
    }

    #[test]
    fn target_rejects_high_hash() {
        let target = difficulty_to_target(1_000_000);
        assert!(!target.allows(&[0xFF; 32]));
    }

    // --- Compact nBits round-trip tests ---

    #[test]
    fn compact_round_trip_high_target() {
        let bits = 0x1f00ffff_u32;
        let target = compact_to_target(bits);
        assert_eq!(target_to_compact(&target), bits);
    }

    #[test]
    fn compact_round_trip_genesis_difficulty() {
        let target = difficulty_to_target(GENESIS_DIFFICULTY);
        let bits = target_to_compact(&target);
        let recovered = compact_to_target(bits);
        // Compact encoding loses low-order bits; compare only leading 3 significant bytes.
        assert_eq!(&recovered.bytes[..4], &target.bytes[..4]);
    }

    #[test]
    fn compact_round_trip_medium_difficulty() {
        let target = difficulty_to_target(100_000);
        let bits = target_to_compact(&target);
        let recovered = compact_to_target(bits);
        assert_eq!(&recovered.bytes[..4], &target.bytes[..4]);
    }

    #[test]
    fn compact_zero_returns_zero_target() {
        let target = compact_to_target(0);
        assert_eq!(target.bytes, [0u8; 32]);
    }

    // --- Difficulty monitor tests ---

    #[test]
    fn stats_perfect_timing() {
        let window = make_window(60, 10_000, TARGET_BLOCK_TIME);
        let stats = difficulty_stats(&window).unwrap();
        assert_eq!(stats.sample_size, 60);
        assert!((stats.mean_solve_time - 60.0).abs() < 0.01);
        assert!((stats.timing_ratio - 1.0).abs() < 0.01);
        assert_eq!(stats.current_difficulty, 10_000);
        assert_eq!(stats.predicted_next, 10_000);
        assert!(stats.estimated_hashrate > 0.0);
    }

    #[test]
    fn stats_fast_blocks() {
        let window = make_window(60, 10_000, 30);
        let stats = difficulty_stats(&window).unwrap();
        assert!((stats.mean_solve_time - 30.0).abs() < 0.01);
        assert!(stats.timing_ratio < 0.6);
        assert!(stats.predicted_next > stats.current_difficulty);
    }

    #[test]
    fn stats_slow_blocks() {
        let window = make_window(60, 10_000, 120);
        let stats = difficulty_stats(&window).unwrap();
        assert!((stats.mean_solve_time - 120.0).abs() < 0.01);
        assert!(stats.timing_ratio > 1.5);
        assert!(stats.predicted_next < stats.current_difficulty);
    }

    #[test]
    fn stats_returns_none_for_single_block() {
        let window = vec![BlockInfo {
            timestamp: 1000,
            difficulty: 5000,
        }];
        assert!(difficulty_stats(&window).is_none());
    }

    #[test]
    fn stats_returns_none_for_empty() {
        assert!(difficulty_stats(&[]).is_none());
    }

    #[test]
    fn predict_stable_chain_stays_stable() {
        let window = make_window(60, 10_000, TARGET_BLOCK_TIME);
        let preds = predict_difficulty(&window, 10);
        assert_eq!(preds.len(), 10);
        for &d in &preds {
            assert_eq!(d, 10_000, "stable chain should keep difficulty constant");
        }
    }

    #[test]
    fn predict_fast_chain_ramps_up() {
        let window = make_window(60, 10_000, 30);
        let preds = predict_difficulty(&window, 5);
        assert_eq!(preds.len(), 5);
        assert!(preds[0] > 10_000, "first prediction should be higher");
        // Each prediction should be >= the previous (chain is consistently fast)
        for i in 1..preds.len() {
            assert!(preds[i] >= preds[i - 1], "difficulty should keep climbing");
        }
    }

    #[test]
    fn predict_empty_horizon_returns_empty() {
        let window = make_window(60, 10_000, 60);
        assert!(predict_difficulty(&window, 0).is_empty());
    }

    #[test]
    fn predict_short_window() {
        let window = vec![
            BlockInfo {
                timestamp: 1000,
                difficulty: 5000,
            },
            BlockInfo {
                timestamp: 1060,
                difficulty: 5000,
            },
        ];
        let preds = predict_difficulty(&window, 3);
        assert_eq!(preds.len(), 3);
    }

    #[test]
    fn hashrate_estimate_scales_with_difficulty() {
        let low = make_window(20, 1_000, 60);
        let high = make_window(20, 100_000, 60);
        let s_low = difficulty_stats(&low).unwrap();
        let s_high = difficulty_stats(&high).unwrap();
        assert!(
            s_high.estimated_hashrate > s_low.estimated_hashrate * 50.0,
            "100x difficulty should produce much higher hashrate estimate"
        );
    }
}
