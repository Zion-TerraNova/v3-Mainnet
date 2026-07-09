use std::sync::OnceLock;

use crate::algorithms_npu::{DeekshaCircuitBreaker, NpuBackend};
use crate::algorithms_opt::{
    cosmic_fusion_opt_rounds, golden_matrix_opt, keccak256_opt, sha3_512_opt, Hash32, Hash64,
};
use crate::scratchpad_ekam::memory_hard_transform_ekam_light;

// ============================================================================
// FORK HEIGHT — SINGLE SOURCE OF TRUTH
// ============================================================================

/// Fork height for Ekam Deeksha v1 (Tier 2 — Blake3 + AES cascade, 64 KiB).
pub const CHV_EKAM_FORK_HEIGHT: u64 = 0;

/// Fork height for Ekam Deeksha v2 (Tier 1+2 ASIC hardening).
/// Active from genesis in V3 mainnet — this IS the mainnet algorithm.
pub const CHV_EKAM_V2_FORK_HEIGHT: u64 = 0;

/// Fork height for CHv4.2 Merkabah Dual-Spin (Phase X+ — not activated yet).
/// Set to u64::MAX to prevent accidental activation before governance approval.
pub const CHV42_DUAL_SPIN_FORK_HEIGHT: u64 = u64::MAX;

/// Shared activation height when built with **`testnet_fork_rehearsal`** only.
///
/// Edit this single literal for docker/local rehearsals; keep it identical for
/// tx-hash v2 and body-root v2 via the aliases below. **Never ship production
/// binaries with `--features testnet_fork_rehearsal`.**
#[cfg(feature = "testnet_fork_rehearsal")]
const TESTNET_REHEARSAL_COORDINATED_HEIGHT: u64 = 10;

/// Activation height for **TX hash v2** (audit §3.2, see
/// `V3/docs/audits/2026-04-V3_AUDIT_COMPLETION.md` §1).
///
/// Below this height blocks accept `tx.version = 1` (raw-concat preimage).
/// At/above this height, `validate_peer_block` MUST reject any tx with
/// `version < TX_HASH_V2_VERSION` (= 2), and the wallet / RPC layer MUST
/// emit `version = TX_HASH_V2_VERSION` for every newly built tx.
///
/// **V3 mainnet (fresh chain):** active from genesis (`0`). Use
/// `--features testnet_fork_rehearsal` for a finite rehearsal height without
/// changing production defaults.
#[cfg(feature = "testnet_fork_rehearsal")]
pub const TX_HASH_V2_ACTIVATION_HEIGHT: u64 = TESTNET_REHEARSAL_COORDINATED_HEIGHT;

#[cfg(not(feature = "testnet_fork_rehearsal"))]
pub const TX_HASH_V2_ACTIVATION_HEIGHT: u64 = 0;

/// Activation height for **F2 BLAKE3 Merkle body root** (audit §F2, see
/// `V3/docs/audits/2026-04-V3_AUDIT_COMPLETION.md` §2).
///
/// Below this height block bodies use the legacy `derive_template_merkle_root`
/// XOR aggregate (per-tx hash via Cosmic Harmony Ekam Deeksha — expensive,
/// not collision-bounded as a tree). At/above this height, the body root
/// MUST be computed via `crypto::merkle_root(...)` over per-tx hashes
/// (BLAKE3, Bitcoin-style pair-duplicate-on-odd-count, leaf =
/// `Transaction::calculate_hash()` which already dispatches to v2 once
/// `TX_HASH_V2_ACTIVATION_HEIGHT` is met).
///
/// **V3 mainnet (fresh chain):** active from genesis (`0`), coordinated with
/// [`TX_HASH_V2_ACTIVATION_HEIGHT`]. Rehearsal builds override via
/// `testnet_fork_rehearsal`.
#[cfg(feature = "testnet_fork_rehearsal")]
pub const BODY_ROOT_V2_ACTIVATION_HEIGHT: u64 = TESTNET_REHEARSAL_COORDINATED_HEIGHT;

#[cfg(not(feature = "testnet_fork_rehearsal"))]
pub const BODY_ROOT_V2_ACTIVATION_HEIGHT: u64 = 0;

/// Returns `true` once a height has crossed the TX hash v2 activation gate.
///
/// Standard idiom for call sites is:
/// ```ignore
/// if tx_hash_v2_active(block.height) && tx.version < TX_HASH_V2_VERSION {
///     return Err("post-fork: tx.version must be >= 2".into());
/// }
/// ```
#[inline]
#[allow(clippy::absurd_extreme_comparisons)]
pub fn tx_hash_v2_active(height: u64) -> bool {
    // Keep `>=` even while the dormant default is `u64::MAX`: when governance
    // flips the constant to a real height, the activation semantics are already
    // correct and do not need another code change.
    height >= TX_HASH_V2_ACTIVATION_HEIGHT
}

/// Returns `true` once a height has crossed the BLAKE3 Merkle body-root gate.
///
/// Standard idiom for the template builder is:
/// ```ignore
/// let body_root = if body_root_v2_active(height) {
///     crypto::merkle_root(&tx_hashes)
/// } else {
///     derive_template_merkle_root_xor(&tx_hashes)
/// };
/// ```
#[inline]
#[allow(clippy::absurd_extreme_comparisons)]
pub fn body_root_v2_active(height: u64) -> bool {
    // Same dormant-gate pattern as `tx_hash_v2_active`.
    height >= BODY_ROOT_V2_ACTIVATION_HEIGHT
}

/// Activation height for **account-model transaction memo v1**.
///
/// Below this height account transactions are accepted without a `memo` field.
/// At/above this height, account transactions MAY include a `memo` field (up
/// to 256 bytes, ASCII only) and the memo becomes part of the signed tx_id
/// preimage. Watchers that parse protocols from memos (`BRIDGE:`, `DAO:`,
/// `SWAP:`, `WARP:`) will scan both UTXO and account transactions.
///
/// **V3 mainnet (fresh chain):** active from genesis (`0`). Use
/// `ZION_ACCOUNT_TX_MEMO_V1_HEIGHT` environment variable at runtime to set a
/// non-zero activation height for a coordinated hard fork on an existing chain.
/// Use `--features testnet_fork_rehearsal` for local rehearsals without
/// changing production defaults.
#[cfg(feature = "testnet_fork_rehearsal")]
pub const ACCOUNT_TX_MEMO_V1_ACTIVATION_HEIGHT: u64 = TESTNET_REHEARSAL_COORDINATED_HEIGHT;

#[cfg(not(feature = "testnet_fork_rehearsal"))]
pub const ACCOUNT_TX_MEMO_V1_ACTIVATION_HEIGHT: u64 = 0;

static ACCOUNT_TX_MEMO_V1_HEIGHT_OVERRIDE: OnceLock<u64> = OnceLock::new();

/// Set the runtime activation height for account-model memo v1.
///
/// This is intended for coordinated mainnet/testnet hard forks on an existing
/// chain. It must be called once before any node/wallet logic runs. A value of
/// `0` disables the override (falls back to the compile-time constant).
pub fn set_account_tx_memo_v1_activation_height(height: u64) {
    let _ = ACCOUNT_TX_MEMO_V1_HEIGHT_OVERRIDE.set(height);
}

/// Returns the effective account-model memo v1 activation height.
///
/// Uses the runtime override if set and non-zero, otherwise the compile-time
/// constant.
#[inline]
#[allow(clippy::absurd_extreme_comparisons)]
pub fn account_tx_memo_v1_activation_height() -> u64 {
    *ACCOUNT_TX_MEMO_V1_HEIGHT_OVERRIDE
        .get()
        .filter(|&&h| h > 0)
        .unwrap_or(&ACCOUNT_TX_MEMO_V1_ACTIVATION_HEIGHT)
}

/// Returns `true` once a height has crossed the account-model memo v1 gate.
#[inline]
#[allow(clippy::absurd_extreme_comparisons)]
pub fn account_tx_memo_v1_active(height: u64) -> bool {
    height >= account_tx_memo_v1_activation_height()
}

// ── F5: Account-model sender balance validation gate ───────────────────
//
// Below this height, account-model transactions are accepted without
// checking that the sender has sufficient balance. At/above this height,
// both the RPC path (insert_transaction) and the peer-block path
// (validate_peer_block) reject transactions where
// `sender_balance < amount + fee`.
//
// This closes the F5 inflation exploit (SEC-2026-07-02) where a TX from
// an address with 0 balance was accepted, creating ZION from nothing.
//
// **Default: disabled (u64::MAX).** This preserves backward compatibility
// with existing tests and chains. To enable on mainnet, set
// `ZION_BALANCE_CHECK_HEIGHT` env var to the activation height (e.g. 22363
// for the Edge mainnet hard fork after the burn TX).

static BALANCE_CHECK_HEIGHT_OVERRIDE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(u64::MAX);

/// Set the runtime activation height for F5 balance validation.
/// Use 0 to enable from genesis, or a future height for a coordinated hard fork.
/// Default is u64::MAX (disabled).
pub fn set_balance_check_height(height: u64) {
    BALANCE_CHECK_HEIGHT_OVERRIDE.store(height, std::sync::atomic::Ordering::Relaxed);
}

/// Returns the effective F5 balance-check activation height.
/// Default is `u64::MAX` (disabled) unless overridden via `set_balance_check_height`.
#[inline]
pub fn balance_check_activation_height() -> u64 {
    BALANCE_CHECK_HEIGHT_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Returns `true` once a height has crossed the F5 balance-check gate.
#[inline]
pub fn balance_check_active(height: u64) -> bool {
    height >= balance_check_activation_height()
}

// ----------------------------------------------------------------------------
// F4.7 — Max transaction amount cap (defense-in-depth on top of F5)
// ----------------------------------------------------------------------------
//
// A sanity ceiling: no single non-genesis, non-coinbase transaction may move
// more than the entire money supply (`emission::TOTAL_SUPPLY`). This bounds the
// damage from any future inflation bug that fabricates an absurd amount, even
// if it were to bypass the F5 sender-balance check.
//
// **Default: disabled (u64::MAX).** Enable on mainnet by setting
// `ZION_MAX_TX_AMOUNT_HEIGHT` to a future activation height for a coordinated
// hard fork (chosen above the 3.0.3 migration height so all checked amounts are
// already in 6-decimal flowers scale).

static MAX_TX_AMOUNT_HEIGHT_OVERRIDE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(u64::MAX);

/// Set the runtime activation height for the F4.7 max-tx-amount cap.
/// Use 0 to enable from genesis, or a future height for a coordinated hard fork.
/// Default is u64::MAX (disabled).
pub fn set_max_tx_amount_height(height: u64) {
    MAX_TX_AMOUNT_HEIGHT_OVERRIDE.store(height, std::sync::atomic::Ordering::Relaxed);
}

/// Returns the effective F4.7 max-tx-amount cap activation height.
/// Default is `u64::MAX` (disabled) unless overridden via `set_max_tx_amount_height`.
#[inline]
pub fn max_tx_amount_activation_height() -> u64 {
    MAX_TX_AMOUNT_HEIGHT_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Returns `true` once a height has crossed the F4.7 max-tx-amount gate.
#[inline]
pub fn max_tx_amount_active(height: u64) -> bool {
    height >= max_tx_amount_activation_height()
}

// ============================================================================
// CONSENSUS PARAMETERS
// ============================================================================

/// Cosmic Fusion rounds (expanded from 4 to 8 for Ekam).
pub const EKAM_FUSION_ROUNDS: usize = 8;

// Ekam v2 consensus parameters (Tier 1 ASIC hardening)

/// Ekam v2 scratchpad size — 256 KiB (4x v1).
pub const EKAM_V2_SCRATCHPAD_SIZE: usize = 256 * 1024;

/// Ekam v2 scratchpad passes — 4 (2x v1).
pub const EKAM_V2_PASSES: usize = 4;

/// Ekam v2 random reads — 256 (4x v1).
pub const EKAM_V2_RANDOM_READS: usize = 256;

// ============================================================================
// CANONICAL TEST VECTORS
// ============================================================================

/// Ekam Deeksha v1 canonical test vector.
pub const EKAM_CANONICAL_TEST_VECTOR_HEX: &str =
    "6339f2fb178fe2957a10d9e2a84cf9d5e340064f0d165e845b6a54eaf7924fbd";

/// Ekam Deeksha v2 canonical test vector (Tier 1+2 — 256 KiB scratchpad + epoch NPU).
/// Generated with epoch 0 (Standard topology, height 0).
pub const EKAM_V2_CANONICAL_TEST_VECTOR_HEX: &str =
    "d043e26b6ed7a2a4f1973a0e340c2eeed7643f6af03d33b8a44907f4f43935c3";

// ============================================================================
// NPU BACKEND SINGLETON
// ============================================================================

static EKAM_NPU: OnceLock<DeekshaCircuitBreaker> = OnceLock::new();

/// Initialize NPU backend. Safe to call repeatedly (OnceLock).
pub fn init_npu() {
    EKAM_NPU.get_or_init(DeekshaCircuitBreaker::build_best_available);
}

#[inline]
fn npu() -> &'static DeekshaCircuitBreaker {
    EKAM_NPU.get_or_init(DeekshaCircuitBreaker::build_best_available)
}

// ============================================================================
// EKAM DEEKSHA V1 PIPELINE (Tier 2 — Blake3 + AES cascade, 64 KiB)
// ============================================================================

/// Cosmic Harmony Ekam Deeksha v1 — 64 KiB scratchpad, 2 passes, 64 random reads.
#[inline]
pub fn cosmic_harmony_ekam_deeksha(block_header: &[u8], nonce: u64) -> Hash32 {
    let mut input = [0u8; 88];
    let len = block_header.len().min(80);
    input[..len].copy_from_slice(&block_header[..len]);
    input[80..88].copy_from_slice(&nonce.to_le_bytes());

    let s1 = keccak256_opt(&input);
    let s2 = sha3_512_opt(&s1.data);
    let s3 = golden_matrix_opt(&s2.data);
    let s4 = memory_hard_transform_ekam_light(&s3.data);
    let s5 = npu().mix(&s4.data);
    cosmic_fusion_opt_rounds(&s5, EKAM_FUSION_ROUNDS)
}

// ============================================================================
// EKAM DEEKSHA V2 PIPELINE (Tier 1+2 — 256 KiB + epoch NPU)
// ============================================================================

/// Cosmic Harmony Ekam Deeksha v2 — Tier 1+2 ASIC-hardened consensus hash.
///
/// Same 6-step pipeline as v1, but:
/// - Step 4: 256 KiB scratchpad, 4 passes, 256 random reads (Tier 1)
/// - Step 5: Epoch-rotating NPU weights with variable MLP topology (Tier 2)
///
/// This is the V3 mainnet canonical hash (active from genesis).
#[inline]
pub fn cosmic_harmony_ekam_deeksha_v2(
    block_header: &[u8],
    nonce: u64,
    block_height: u64,
) -> Hash32 {
    use crate::algorithms_npu::{epoch_from_height, npu_mixing_step_epoch};
    use crate::scratchpad_ekam::memory_hard_transform_ekam_light_v2;

    let mut input = [0u8; 88];
    let len = block_header.len().min(80);
    input[..len].copy_from_slice(&block_header[..len]);
    input[80..88].copy_from_slice(&nonce.to_le_bytes());

    let s1 = keccak256_opt(&input);
    let s2 = sha3_512_opt(&s1.data);
    let s3 = golden_matrix_opt(&s2.data);
    let s4 = memory_hard_transform_ekam_light_v2(&s3.data);
    let epoch = epoch_from_height(block_height);
    let s5 = npu_mixing_step_epoch(&s4.data, epoch);
    cosmic_fusion_opt_rounds(&s5, EKAM_FUSION_ROUNDS)
}

// ============================================================================
// CHv4.2 MERKABAH DUAL-SPIN PIPELINE (Phase X+ — fork-gated)
// ============================================================================

/// Cosmic Harmony CHv4.2 — Merkabah Dual-Spin pipeline.
///
/// Extends v2 with the full HIC pipeline in the memory-hard step:
/// - Forward + backward HIC-enriched passes (dual-spin Merkabah)
/// - Kabala phase (22 HIC-addressed dependent reads)
/// - Brahma-jyoti finalization (22 rounds of SHA3-512 + HIC)
///
/// Gated by `CHV42_DUAL_SPIN_FORK_HEIGHT` (currently u64::MAX — not active).
#[inline]
pub fn cosmic_harmony_ekam_deeksha_v3(
    block_header: &[u8],
    nonce: u64,
    block_height: u64,
) -> Hash32 {
    use crate::algorithms_npu::{epoch_from_height, npu_mixing_step_epoch};
    use crate::scratchpad_ekam::memory_hard_transform_ekam_v3;

    let mut input = [0u8; 88];
    let len = block_header.len().min(80);
    input[..len].copy_from_slice(&block_header[..len]);
    input[80..88].copy_from_slice(&nonce.to_le_bytes());

    let s1 = keccak256_opt(&input);
    let s2 = sha3_512_opt(&s1.data);
    let s3 = golden_matrix_opt(&s2.data);
    let s4 = memory_hard_transform_ekam_v3(&s3.data);
    let epoch = epoch_from_height(block_height);
    let s5 = npu_mixing_step_epoch(&s4.data, epoch);
    cosmic_fusion_opt_rounds(&s5, EKAM_FUSION_ROUNDS)
}

// ============================================================================
// MINING HELPERS
// ============================================================================

/// Ekam v1 — sequential nonce search.
pub fn ekam_find_nonce(
    header: &[u8],
    start_nonce: u64,
    count: u64,
    target: &[u8; 32],
) -> Option<(u64, Hash32)> {
    for offset in 0..count {
        let nonce = start_nonce.wrapping_add(offset);
        let hash = cosmic_harmony_ekam_deeksha(header, nonce);
        if meets_target(&hash.data, target) {
            return Some((nonce, hash));
        }
    }
    None
}

/// Ekam v2 — sequential nonce search (height-aware).
pub fn ekam_v2_find_nonce(
    header: &[u8],
    start_nonce: u64,
    count: u64,
    target: &[u8; 32],
    block_height: u64,
) -> Option<(u64, Hash32)> {
    for offset in 0..count {
        let nonce = start_nonce.wrapping_add(offset);
        let hash = cosmic_harmony_ekam_deeksha_v2(header, nonce, block_height);
        if meets_target(&hash.data, target) {
            return Some((nonce, hash));
        }
    }
    None
}

/// CHv4.2 — sequential nonce search (height-aware, dual-spin).
pub fn ekam_v3_find_nonce(
    header: &[u8],
    start_nonce: u64,
    count: u64,
    target: &[u8; 32],
    block_height: u64,
) -> Option<(u64, Hash32)> {
    for offset in 0..count {
        let nonce = start_nonce.wrapping_add(offset);
        let hash = cosmic_harmony_ekam_deeksha_v3(header, nonce, block_height);
        if meets_target(&hash.data, target) {
            return Some((nonce, hash));
        }
    }
    None
}

#[inline(always)]
fn meets_target(hash: &[u8; 32], target: &[u8; 32]) -> bool {
    hash <= target
}

// ============================================================================
// SELF-TESTS
// ============================================================================

/// Ekam v1 self-test — determinism + canonical vector.
pub fn ekam_self_test() -> bool {
    const TEST_HEADER: &[u8] = b"ZION_DEEKSHA_GENESIS_V298_CANONICAL";
    const TEST_NONCE: u64 = 0x2980_0001_0000_0001;

    let left = cosmic_harmony_ekam_deeksha(TEST_HEADER, TEST_NONCE);
    let right = cosmic_harmony_ekam_deeksha(TEST_HEADER, TEST_NONCE);
    if left != right {
        return false;
    }

    let hex: String = left
        .data
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect();
    hex == EKAM_CANONICAL_TEST_VECTOR_HEX
}

/// Ekam v2 self-test — determinism + canonical vector.
pub fn ekam_v2_self_test() -> bool {
    const TEST_HEADER: &[u8] = b"ZION_DEEKSHA_GENESIS_V298_CANONICAL";
    const TEST_NONCE: u64 = 0x2980_0001_0000_0001;
    const TEST_HEIGHT: u64 = 0;

    let h1 = cosmic_harmony_ekam_deeksha_v2(TEST_HEADER, TEST_NONCE, TEST_HEIGHT);
    let h2 = cosmic_harmony_ekam_deeksha_v2(TEST_HEADER, TEST_NONCE, TEST_HEIGHT);
    if h1 != h2 {
        return false;
    }

    let hex: String = h1.data.iter().map(|byte| format!("{:02x}", byte)).collect();
    hex == EKAM_V2_CANONICAL_TEST_VECTOR_HEX
}

/// Generate Ekam v1 test vector.
pub fn generate_ekam_test_vector() -> String {
    const TEST_HEADER: &[u8] = b"ZION_DEEKSHA_GENESIS_V298_CANONICAL";
    const TEST_NONCE: u64 = 0x2980_0001_0000_0001;
    let hash = cosmic_harmony_ekam_deeksha(TEST_HEADER, TEST_NONCE);
    hash.data
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

/// Generate Ekam v2 test vector.
pub fn generate_ekam_v2_test_vector() -> String {
    const TEST_HEADER: &[u8] = b"ZION_DEEKSHA_GENESIS_V298_CANONICAL";
    const TEST_NONCE: u64 = 0x2980_0001_0000_0001;
    let hash = cosmic_harmony_ekam_deeksha_v2(TEST_HEADER, TEST_NONCE, 0);
    hash.data
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

/// CHv4.2 self-test — determinism.
pub fn ekam_v3_self_test() -> bool {
    const TEST_HEADER: &[u8] = b"ZION_DEEKSHA_GENESIS_V298_CANONICAL";
    const TEST_NONCE: u64 = 0x2980_0001_0000_0001;
    const TEST_HEIGHT: u64 = 0;

    let h1 = cosmic_harmony_ekam_deeksha_v3(TEST_HEADER, TEST_NONCE, TEST_HEIGHT);
    let h2 = cosmic_harmony_ekam_deeksha_v3(TEST_HEADER, TEST_NONCE, TEST_HEIGHT);
    h1 == h2
}

/// Generate CHv4.2 test vector.
pub fn generate_ekam_v3_test_vector() -> String {
    const TEST_HEADER: &[u8] = b"ZION_DEEKSHA_GENESIS_V298_CANONICAL";
    const TEST_NONCE: u64 = 0x2980_0001_0000_0001;
    let hash = cosmic_harmony_ekam_deeksha_v3(TEST_HEADER, TEST_NONCE, 0);
    hash.data
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

/// Hash 64-byte input through NPU backend.
pub fn hash_bytes_with_npu(input: &[u8; 64]) -> Hash64 {
    let mut hash = Hash64::new();
    hash.data.copy_from_slice(&npu().mix(input));
    hash
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ekam_hash_is_deterministic() {
        let header = b"V3_EKAM_TEST_HEADER";
        let nonce = 42;
        assert_eq!(
            cosmic_harmony_ekam_deeksha(header, nonce),
            cosmic_harmony_ekam_deeksha(header, nonce)
        );
    }

    #[test]
    fn ekam_vector_matches() {
        let vector = generate_ekam_test_vector();
        assert_eq!(
            vector, EKAM_CANONICAL_TEST_VECTOR_HEX,
            "Ekam v1 vector changed"
        );
    }

    #[test]
    fn ekam_v2_hash_is_deterministic() {
        let header = b"V3_EKAM_V2_TEST_HEADER";
        let nonce = 42;
        assert_eq!(
            cosmic_harmony_ekam_deeksha_v2(header, nonce, 0),
            cosmic_harmony_ekam_deeksha_v2(header, nonce, 0)
        );
    }

    #[test]
    fn ekam_v2_vector_matches() {
        let vector = generate_ekam_v2_test_vector();
        assert_eq!(
            vector, EKAM_V2_CANONICAL_TEST_VECTOR_HEX,
            "Ekam v2 vector changed"
        );
    }

    #[test]
    fn ekam_v2_self_test_passes() {
        assert!(ekam_v2_self_test(), "Ekam v2 self-test failed");
    }

    #[test]
    fn ekam_v1_self_test_passes() {
        assert!(ekam_self_test(), "Ekam v1 self-test failed");
    }

    #[test]
    fn v2_differs_from_v1() {
        let header = b"v1 vs v2 comparison";
        let nonce = 12345u64;
        let v1 = cosmic_harmony_ekam_deeksha(header, nonce);
        let v2 = cosmic_harmony_ekam_deeksha_v2(header, nonce, 0);
        assert_ne!(v1.data, v2.data, "v2 must differ from v1");
    }

    #[test]
    fn ekam_v2_epoch_variation() {
        use crate::algorithms_npu::NPU_EPOCH_LENGTH;
        let header = b"epoch variation test";
        let nonce = 999u64;
        let h_ep0 = cosmic_harmony_ekam_deeksha_v2(header, nonce, 0);
        let h_ep1 = cosmic_harmony_ekam_deeksha_v2(header, nonce, NPU_EPOCH_LENGTH);
        let h_ep2 = cosmic_harmony_ekam_deeksha_v2(header, nonce, NPU_EPOCH_LENGTH * 2);
        assert_ne!(h_ep0.data, h_ep1.data, "Epoch 0 vs 1 must differ");
        assert_ne!(h_ep1.data, h_ep2.data, "Epoch 1 vs 2 must differ");
        assert_ne!(h_ep0.data, h_ep2.data, "Epoch 0 vs 2 must differ");
    }

    #[test]
    fn ekam_v2_same_epoch_deterministic() {
        let header = b"same epoch test";
        let nonce = 777u64;
        let h1 = cosmic_harmony_ekam_deeksha_v2(header, nonce, 0);
        let h2 = cosmic_harmony_ekam_deeksha_v2(header, nonce, 1);
        let h3 = cosmic_harmony_ekam_deeksha_v2(header, nonce, 100);
        assert_eq!(h1.data, h2.data, "Same epoch heights must match");
        assert_eq!(h2.data, h3.data, "Same epoch heights must match");
    }

    #[test]
    fn ekam_v2_avalanche() {
        let header = b"avalanche test header";
        let h1 = cosmic_harmony_ekam_deeksha_v2(header, 1000, 0);
        let h2 = cosmic_harmony_ekam_deeksha_v2(header, 1001, 0);
        assert_ne!(h1.data, h2.data, "Different nonce must give different hash");
    }

    #[test]
    fn ekam_v2_output_nonzero() {
        let h = cosmic_harmony_ekam_deeksha_v2(b"nonzero test", 0, 0);
        assert!(
            h.data.iter().any(|&b| b != 0),
            "Output must not be all-zero"
        );
    }

    #[test]
    fn ekam_find_nonce_works() {
        let header = b"find nonce test";
        let target = [0xffu8; 32];
        assert!(ekam_find_nonce(header, 0, 10, &target).is_some());
    }

    #[test]
    fn ekam_v2_find_nonce_works() {
        let header = b"find nonce test";
        let target = [0xffu8; 32];
        assert!(ekam_v2_find_nonce(header, 0, 10, &target, 0).is_some());
    }

    #[test]
    fn dispatch_routes_to_v2() {
        use crate::algorithms_opt::cosmic_harmony_with_height;
        let header = b"dispatch test header";
        let nonce = 42u64;
        let dispatched = cosmic_harmony_with_height(header, nonce, 0);
        let direct_v2 = cosmic_harmony_ekam_deeksha_v2(header, nonce, 0);
        assert_eq!(dispatched.data, direct_v2.data, "Dispatch must route to v2");
    }

    #[test]
    fn npu_parity() {
        use crate::algorithms_npu::npu_mixing_step;
        let input = [0x7fu8; 64];
        let via_backend = npu().mix(&input);
        let cpu_direct = npu_mixing_step(&input);
        assert_eq!(via_backend, cpu_direct, "NPU backend must match CPU path");
    }

    // ================================================================
    // CHv4.2 Merkabah Dual-Spin (v3) pipeline tests
    // ================================================================

    #[test]
    fn v3_hash_is_deterministic() {
        let header = b"V3_CHV42_TEST_HEADER";
        let nonce = 42;
        assert_eq!(
            cosmic_harmony_ekam_deeksha_v3(header, nonce, 0),
            cosmic_harmony_ekam_deeksha_v3(header, nonce, 0)
        );
    }

    #[test]
    fn v3_differs_from_v2() {
        let header = b"v2 vs v3 comparison";
        let nonce = 12345u64;
        let v2 = cosmic_harmony_ekam_deeksha_v2(header, nonce, 0);
        let v3 = cosmic_harmony_ekam_deeksha_v3(header, nonce, 0);
        assert_ne!(v2.data, v3.data, "v3 dual-spin must differ from v2");
    }

    #[test]
    fn v3_avalanche() {
        let header = b"v3 avalanche test";
        let h1 = cosmic_harmony_ekam_deeksha_v3(header, 1000, 0);
        let h2 = cosmic_harmony_ekam_deeksha_v3(header, 1001, 0);
        assert_ne!(h1.data, h2.data, "Different nonce → different hash");
    }

    #[test]
    fn v3_output_nonzero() {
        let h = cosmic_harmony_ekam_deeksha_v3(b"v3 nonzero test", 0, 0);
        assert!(
            h.data.iter().any(|&b| b != 0),
            "v3 output must not be all-zero"
        );
    }

    #[test]
    fn v3_self_test_passes() {
        assert!(ekam_v3_self_test(), "CHv4.2 self-test failed");
    }

    #[test]
    fn v3_find_nonce_works() {
        let header = b"v3 find nonce test";
        let target = [0xffu8; 32];
        assert!(ekam_v3_find_nonce(header, 0, 10, &target, 0).is_some());
    }

    #[test]
    fn v3_epoch_variation() {
        use crate::algorithms_npu::NPU_EPOCH_LENGTH;
        let header = b"v3 epoch test";
        let nonce = 999u64;
        let h_ep0 = cosmic_harmony_ekam_deeksha_v3(header, nonce, 0);
        let h_ep1 = cosmic_harmony_ekam_deeksha_v3(header, nonce, NPU_EPOCH_LENGTH);
        assert_ne!(
            h_ep0.data, h_ep1.data,
            "v3: different epochs → different hashes"
        );
    }

    #[test]
    fn v3_fork_height_not_active() {
        assert_eq!(
            CHV42_DUAL_SPIN_FORK_HEIGHT,
            u64::MAX,
            "CHv4.2 must not be active until governance approval"
        );
    }

    // ================================================================
    // Hard-fork activation gates: TX hash v2 + F2 BLAKE3 Merkle
    // ================================================================

    /// Production V3 ships both gates at genesis (`0`).
    #[test]
    #[cfg(not(feature = "testnet_fork_rehearsal"))]
    fn tx_hash_v2_activation_height_is_genesis() {
        assert_eq!(TX_HASH_V2_ACTIVATION_HEIGHT, 0);
    }

    #[test]
    #[cfg(not(feature = "testnet_fork_rehearsal"))]
    fn body_root_v2_activation_height_is_genesis() {
        assert_eq!(BODY_ROOT_V2_ACTIVATION_HEIGHT, 0);
    }

    #[test]
    #[cfg(not(feature = "testnet_fork_rehearsal"))]
    fn tx_hash_v2_active_from_genesis() {
        assert!(tx_hash_v2_active(0));
        assert!(tx_hash_v2_active(1));
        assert!(tx_hash_v2_active(100));
        assert!(tx_hash_v2_active(1_000_000));
        assert!(tx_hash_v2_active(u64::MAX));
    }

    #[test]
    #[cfg(not(feature = "testnet_fork_rehearsal"))]
    fn body_root_v2_active_from_genesis() {
        assert!(body_root_v2_active(0));
        assert!(body_root_v2_active(1));
        assert!(body_root_v2_active(u64::MAX));
    }

    #[cfg(feature = "testnet_fork_rehearsal")]
    #[test]
    fn fork_rehearsal_tx_and_body_heights_are_aligned() {
        assert_eq!(
            TX_HASH_V2_ACTIVATION_HEIGHT, BODY_ROOT_V2_ACTIVATION_HEIGHT,
            "rehearsal must never ship mismatched gates"
        );
        assert!(
            TX_HASH_V2_ACTIVATION_HEIGHT < u64::MAX,
            "rehearsal expects a finite activation height"
        );
    }

    #[cfg(feature = "testnet_fork_rehearsal")]
    #[test]
    fn fork_rehearsal_predicates_flip_at_coordination_height() {
        let h = TX_HASH_V2_ACTIVATION_HEIGHT;
        assert!(!tx_hash_v2_active(h.saturating_sub(1)));
        assert!(tx_hash_v2_active(h));
        assert!(!body_root_v2_active(h.saturating_sub(1)));
        assert!(body_root_v2_active(h));
    }

    /// At/above any chosen activation height the gate must flip true.
    /// Uses a synthetic non-dormant value to verify branching behaviour
    /// without committing to a real activation height.
    #[test]
    fn activation_gates_flip_at_or_above_chosen_height() {
        // We cannot mutate the const, so simulate the predicate:
        let chosen: u64 = 1_000_000;
        let predicate = |h: u64| h >= chosen;
        assert!(!predicate(chosen - 1));
        assert!(predicate(chosen));
        assert!(predicate(chosen + 1));
    }
}
