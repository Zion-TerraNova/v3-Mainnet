//! Deeksha Stream Layers — revenue-aware telemetry for the CH pipeline.
//!
//! This module provides **consensus-safe** stream telemetry for the Deeksha
//! hash pipeline. It does NOT modify hash outputs; it only records which
//! computational steps were performed and maps them to revenue streams.
//!
//! Design principles (Rule D — Revenue Dharma Continuity):
//! - The canonical `cosmic_harmony_ekam_deeksha_v2()` remains untouched.
//! - Stream functions are **additive wrappers** that compute the same hash.
//! - Telemetry is used by the pool/miner for granular revenue accounting.
//!
//! Pipeline mapping:
//!   Step 1 Keccak256    → RevenueSource::KeccakBonus  (byproduct stream)
//!   Step 2 SHA3-512     → RevenueSource::Sha3Bonus     (byproduct stream)
//!   Step 3 GoldenMatrix  → RevenueSource::Zion          (core ZION work)
//!   Step 4 MemoryHard   → RevenueSource::Zion          (ASIC-resistant core)
//!   Step 5 NPU Mix      → RevenueSource::NclAi         (AI compute layer)
//!   Step 6 CosmicFusion  → RevenueSource::Zion          (finalization)

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::algorithms_opt::{
    cosmic_fusion_opt_rounds, golden_matrix_opt, keccak256_opt, sha3_512_opt, Hash32, Hash64,
};
use crate::deeksha::EKAM_FUSION_ROUNDS;
use crate::revenue::RevenueSource;
use crate::scratchpad_ekam::memory_hard_transform_ekam_light_v2;

// ============================================================================
// STEP DEFINITIONS
// ============================================================================

/// A single computational step in the Deeksha pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeekshaStep {
    /* ── Cosmic Harmony v2 steps ── */
    Keccak256,
    Sha3_512,
    GoldenMatrix,
    MemoryHard,
    NpuMix,
    CosmicFusion,
    /* ── DeekshaLite v1 steps ── */
    AesMix,
    ThermalLoop,
    KeccakFinal,
    /* ── DeekshaLite Fire steps ── */
    AesMixFire,
    ThermalLoopFire,
}

impl DeekshaStep {
    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Keccak256 => "keccak256",
            Self::Sha3_512 => "sha3_512",
            Self::GoldenMatrix => "golden_matrix",
            Self::MemoryHard => "memory_hard",
            Self::NpuMix => "npu_mix",
            Self::CosmicFusion => "cosmic_fusion",
            Self::AesMix => "aes_mix",
            Self::ThermalLoop => "thermal_loop",
            Self::KeccakFinal => "keccak_final",
            Self::AesMixFire => "aes_mix_fire",
            Self::ThermalLoopFire => "thermal_loop_fire",
        }
    }

    /// Map this step to its primary revenue stream.
    pub fn revenue_stream(self) -> RevenueSource {
        match self {
            Self::Keccak256 => RevenueSource::KeccakBonus,
            Self::Sha3_512 => RevenueSource::Sha3Bonus,
            Self::GoldenMatrix | Self::MemoryHard | Self::CosmicFusion | Self::KeccakFinal => {
                RevenueSource::Zion
            }
            Self::NpuMix => RevenueSource::NclAi,
            Self::AesMix => RevenueSource::DeekshaLite,
            Self::ThermalLoop => RevenueSource::DeekshaLite,
            Self::AesMixFire => RevenueSource::ThermalBonus,
            Self::ThermalLoopFire => RevenueSource::ThermalBonus,
        }
    }

    /// Relative work-unit weight for this step.
    ///
    /// Weights are calibrated so that the entire pipeline sums to a round
    /// number (100 work units) for easy percentage calculation.
    pub fn work_units(self) -> u64 {
        match self {
            // Lightweight hashing steps
            Self::Keccak256 => 5,
            Self::Sha3_512 => 5,
            // Matrix transform
            Self::GoldenMatrix => 10,
            // ASIC-resistant memory-hard core (heaviest step)
            Self::MemoryHard => 55,
            // NPU / AI compute layer
            Self::NpuMix => 15,
            // Final fusion rounds
            Self::CosmicFusion => 10,
            // Lite v1: AES (3 rounds) + thermal (light)
            Self::AesMix => 5,
            Self::ThermalLoop => 3,
            Self::KeccakFinal => 2,
            // Fire: AES (10 rounds) + heavy thermal
            Self::AesMixFire => 10,
            Self::ThermalLoopFire => 15,
        }
    }
}

// ============================================================================
// STREAM TELEMETRY
// ============================================================================

/// Telemetry captured during a Deeksha hash computation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeekshaStreamTelemetry {
    /// Per-step work-unit breakdown.
    pub steps: Vec<(DeekshaStep, u64)>,
    /// Total work units across all steps.
    pub total_work: u64,
    /// Aggregated work units per revenue stream.
    pub stream_breakdown: HashMap<String, u64>,
}

impl DeekshaStreamTelemetry {
    fn new() -> Self {
        Self::default()
    }

    fn record(&mut self, step: DeekshaStep) {
        let units = step.work_units();
        self.steps.push((step, units));
        self.total_work += units;
        *self
            .stream_breakdown
            .entry(step.revenue_stream().as_str().to_string())
            .or_insert(0) += units;
    }

    /// Return the percentage (0–100) of total work attributed to a given stream.
    pub fn pct_for(&self, source: RevenueSource) -> f64 {
        if self.total_work == 0 {
            return 0.0;
        }
        let units = self
            .stream_breakdown
            .get(source.as_str())
            .copied()
            .unwrap_or(0);
        (units as f64 / self.total_work as f64) * 100.0
    }
}

// ============================================================================
// STREAM-AWARE HASH FUNCTIONS (consensus-safe wrappers)
// ============================================================================

/// Deeksha v2 with stream telemetry.
///
/// Computes the **identical** hash as `cosmic_harmony_ekam_deeksha_v2`, but also
/// returns per-step telemetry for revenue accounting.
#[inline]
pub fn cosmic_harmony_ekam_deeksha_v2_with_streams(
    block_header: &[u8],
    nonce: u64,
    block_height: u64,
) -> (Hash32, DeekshaStreamTelemetry) {
    use crate::algorithms_npu::{epoch_from_height, npu_mixing_step_epoch};

    let mut telemetry = DeekshaStreamTelemetry::new();

    let mut input = [0u8; 88];
    let len = block_header.len().min(80);
    input[..len].copy_from_slice(&block_header[..len]);
    input[80..88].copy_from_slice(&nonce.to_le_bytes());

    // Step 1: Keccak-256
    let s1 = keccak256_opt(&input);
    telemetry.record(DeekshaStep::Keccak256);

    // Step 2: SHA3-512
    let s2 = sha3_512_opt(&s1.data);
    telemetry.record(DeekshaStep::Sha3_512);

    // Step 3: Golden Matrix
    let s3 = golden_matrix_opt(&s2.data);
    telemetry.record(DeekshaStep::GoldenMatrix);

    // Step 4: Memory-Hard (256 KiB, 4 passes, 256 reads)
    let s4 = memory_hard_transform_ekam_light_v2(&s3.data);
    telemetry.record(DeekshaStep::MemoryHard);

    // Step 5: NPU Mix (epoch-rotating weights)
    let epoch = epoch_from_height(block_height);
    let s5 = npu_mixing_step_epoch(&s4.data, epoch);
    telemetry.record(DeekshaStep::NpuMix);

    // Step 6: Cosmic Fusion
    let hash = cosmic_fusion_opt_rounds(&s5, EKAM_FUSION_ROUNDS);
    telemetry.record(DeekshaStep::CosmicFusion);

    (hash, telemetry)
}

/// Deeksha v1 with stream telemetry (legacy compat).
#[inline]
pub fn cosmic_harmony_ekam_deeksha_with_streams(
    block_header: &[u8],
    nonce: u64,
) -> (Hash32, DeekshaStreamTelemetry) {
    use crate::algorithms_npu::npu_mixing_step;
    use crate::scratchpad_ekam::memory_hard_transform_ekam_light;

    let mut telemetry = DeekshaStreamTelemetry::new();

    let mut input = [0u8; 88];
    let len = block_header.len().min(80);
    input[..len].copy_from_slice(&block_header[..len]);
    input[80..88].copy_from_slice(&nonce.to_le_bytes());

    let s1 = keccak256_opt(&input);
    telemetry.record(DeekshaStep::Keccak256);

    let s2 = sha3_512_opt(&s1.data);
    telemetry.record(DeekshaStep::Sha3_512);

    let s3 = golden_matrix_opt(&s2.data);
    telemetry.record(DeekshaStep::GoldenMatrix);

    let s4 = memory_hard_transform_ekam_light(&s3.data);
    telemetry.record(DeekshaStep::MemoryHard);

    let s5 = npu_mixing_step(&s4.data);
    telemetry.record(DeekshaStep::NpuMix);

    let hash = cosmic_fusion_opt_rounds(&s5, EKAM_FUSION_ROUNDS);
    telemetry.record(DeekshaStep::CosmicFusion);

    (hash, telemetry)
}

/// DeekshaLite v1 with stream telemetry.
#[inline]
pub fn deeksha_lite_v1_with_streams(
    block_header: &[u8],
    nonce: u64,
) -> (Hash32, DeekshaStreamTelemetry) {
    let mut telemetry = DeekshaStreamTelemetry::new();

    // Step 1: Keccak-256 (header||nonce)
    let hash = crate::deeksha_lite::deeksha_lite_with_height(block_header, nonce, 0);
    telemetry.record(DeekshaStep::Keccak256);
    telemetry.record(DeekshaStep::MemoryHard);
    telemetry.record(DeekshaStep::AesMix);
    telemetry.record(DeekshaStep::KeccakFinal);

    (hash, telemetry)
}

/// DeekshaLite Fire with stream telemetry.
#[inline]
pub fn deeksha_lite_fire_with_streams(
    block_header: &[u8],
    nonce: u64,
) -> (Hash32, DeekshaStreamTelemetry) {
    let mut telemetry = DeekshaStreamTelemetry::new();

    // Fire pipeline: Keccak -> MemoryHard(512K) -> AES-128x10 -> ThermalLoop -> Keccak
    let hash = crate::deeksha_lite_fire::deeksha_lite_fire_with_height(block_header, nonce, 0);
    telemetry.record(DeekshaStep::Keccak256);
    telemetry.record(DeekshaStep::MemoryHard);
    telemetry.record(DeekshaStep::AesMixFire);
    telemetry.record(DeekshaStep::ThermalLoopFire);
    telemetry.record(DeekshaStep::KeccakFinal);

    (hash, telemetry)
}

// ============================================================================
// BYPRODUCT EXTRACTORS (for external pool submission)
// ============================================================================

/// Extract a Keccak-compatible byproduct from Step 1 output.
///
/// This 32-byte value is the raw Keccak-256 digest. In theory it could be
/// formatted and submitted to an ETC pool, but ETC expects full Ethash
/// validation — this is **NOT** a valid standalone share. Use only for
/// telemetry / accounting.
pub fn extract_keccak_byproduct(s1: &Hash32) -> [u8; 32] {
    s1.data
}

/// Extract a SHA3-compatible byproduct from Step 2 output.
///
/// This 64-byte value is the raw SHA3-512 digest. Similar caveat as Keccak:
/// external pools expect their own header formatting.
pub fn extract_sha3_byproduct(s2: &Hash64) -> [u8; 64] {
    s2.data
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_streams_produces_same_hash_as_v2() {
        let header = b"stream parity test header";
        let nonce = 42u64;
        let height = 0u64;

        let hash_plain = crate::deeksha::cosmic_harmony_ekam_deeksha_v2(header, nonce, height);
        let (hash_streams, telemetry) =
            cosmic_harmony_ekam_deeksha_v2_with_streams(header, nonce, height);

        assert_eq!(
            hash_plain.data, hash_streams.data,
            "with_streams must NOT change hash output"
        );
        assert_eq!(telemetry.steps.len(), 6);
        assert_eq!(telemetry.total_work, 100);
    }

    #[test]
    fn with_streams_produces_same_hash_as_v1() {
        let header = b"stream parity test header v1";
        let nonce = 42u64;

        let hash_plain = crate::deeksha::cosmic_harmony_ekam_deeksha(header, nonce);
        let (hash_streams, telemetry) = cosmic_harmony_ekam_deeksha_with_streams(header, nonce);

        assert_eq!(
            hash_plain.data, hash_streams.data,
            "with_streams must NOT change v1 hash output"
        );
        assert_eq!(telemetry.steps.len(), 6);
    }

    #[test]
    fn stream_breakdown_sums_to_100() {
        let header = b"breakdown test";
        let (hash, telemetry) = cosmic_harmony_ekam_deeksha_v2_with_streams(header, 0, 0);

        // Hash must be valid
        assert!(hash.data.iter().any(|&b| b != 0));

        // All 6 steps recorded
        assert_eq!(telemetry.steps.len(), 6);

        // Total work = 100 units
        assert_eq!(telemetry.total_work, 100);

        // Breakdown percentages must sum to ~100
        let zion_pct = telemetry.pct_for(RevenueSource::Zion);
        let keccak_pct = telemetry.pct_for(RevenueSource::KeccakBonus);
        let sha3_pct = telemetry.pct_for(RevenueSource::Sha3Bonus);
        let ncl_pct = telemetry.pct_for(RevenueSource::NclAi);

        let sum = zion_pct + keccak_pct + sha3_pct + ncl_pct;
        assert!(
            (sum - 100.0).abs() < 0.1,
            "stream percentages must sum to 100, got {}",
            sum
        );
    }

    #[test]
    fn zion_stream_is_majority() {
        let (_hash, telemetry) =
            cosmic_harmony_ekam_deeksha_v2_with_streams(b"majority test", 0, 0);
        let zion_pct = telemetry.pct_for(RevenueSource::Zion);
        assert!(
            zion_pct > 50.0,
            "ZION stream must be majority of work, got {}%",
            zion_pct
        );
    }

    #[test]
    fn memory_hard_is_heaviest_step() {
        let step = DeekshaStep::MemoryHard;
        assert_eq!(step.work_units(), 55);
        assert_eq!(step.revenue_stream(), RevenueSource::Zion);
    }

    #[test]
    fn npu_step_maps_to_ncl() {
        assert_eq!(DeekshaStep::NpuMix.revenue_stream(), RevenueSource::NclAi);
        assert_eq!(DeekshaStep::NpuMix.work_units(), 15);
    }

    #[test]
    fn byproduct_extractors_return_correct_sizes() {
        let header = b"byproduct test";
        let nonce = 0u64;
        let mut input = [0u8; 88];
        input[..header.len()].copy_from_slice(header);
        input[80..88].copy_from_slice(&nonce.to_le_bytes());

        let s1 = keccak256_opt(&input);
        let s2 = sha3_512_opt(&s1.data);

        assert_eq!(extract_keccak_byproduct(&s1).len(), 32);
        assert_eq!(extract_sha3_byproduct(&s2).len(), 64);
    }

    #[test]
    fn deeksha_lite_v1_with_streams_parity() {
        let header = b"lite v1 stream test";
        let nonce = 42u64;

        let hash_plain = crate::deeksha_lite::deeksha_lite_with_height(header, nonce, 0);
        let (hash_streams, telemetry) = deeksha_lite_v1_with_streams(header, nonce);

        assert_eq!(hash_plain.data, hash_streams.data);
        assert_eq!(telemetry.steps.len(), 4);
        assert!(telemetry.total_work > 0);
    }

    #[test]
    fn deeksha_lite_fire_with_streams_parity() {
        let header = b"fire stream test";
        let nonce = 99u64;

        let hash_plain = crate::deeksha_lite_fire::deeksha_lite_fire_with_height(header, nonce, 0);
        let (hash_streams, telemetry) = deeksha_lite_fire_with_streams(header, nonce);

        assert_eq!(hash_plain.data, hash_streams.data);
        assert_eq!(telemetry.steps.len(), 5);
        assert!(telemetry.total_work > 0);

        let thermal_pct = telemetry.pct_for(RevenueSource::ThermalBonus);
        assert!(
            thermal_pct > 0.0,
            "Fire must attribute some work to ThermalBonus"
        );
    }

    #[test]
    fn deeksha_lite_v1_revenue_is_deeksha_lite_stream() {
        let (_hash, telemetry) = deeksha_lite_v1_with_streams(b"lite revenue", 1);
        let lite_pct = telemetry.pct_for(RevenueSource::DeekshaLite);
        assert!(
            lite_pct > 0.0,
            "Lite v1 must have DeekshaLite revenue stream"
        );
    }
}
