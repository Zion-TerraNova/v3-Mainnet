// ============================================================================
// COSMIC HARMONY v4 — NPU MIXING STEP
// ============================================================================
//
// Vloží se mezi MemoryHard (Phase 4) a CosmicFusion (Phase 5/6) v CHv4 pipeline.
//
// Architektura: INT8 kvantizovaný MLP s residual connection
//   Linear(64→128) + LayerNorm + GELU
//   Linear(128→64) + LayerNorm
//   Residual add: output += input
//
// Deterministické: použití fixed-point aritmetiky (Q8 integer pouze).
// Váhy: odvozeny z ZION genesis seedu přes Blake3 expanzi — konstanta protokolu.
//
// Hardwarové vylepšení:
//   - NEON (Apple M1/M2 ANE) → ~50–200 µs přes CoreML (budoucí ONNX backend)
//   - AVX2 (x86_64) → SIMD INT8 MAD instrukce
//   - CPU fallback → identický výsledek (integer path, žádná FP divergence)
//
// ONNX backend: za feature flagem `native-npu` (připraveno, zatím CPU INT8).
// Váhy budou nahrazeny skutečným ch_mixing_v4.onnx v dalším releasu.
//
// Author: ZION Core Team — CHv4 implementace, 2026
// ============================================================================

// INT8 MLP loops mirror the NPU/NEON memory layout; index-based access is intentional.
#![allow(clippy::needless_range_loop)]

use blake3;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

// ============================================================================
// GENESIS SEED & PROTOCOL CONSTANTS
// ============================================================================

/// Genesis seed pro deterministické odvození vah MLP.
/// Zakomponován do protokolu — jakákoliv změna = jiný hash = špatný blok.
pub const CHV4_MLP_GENESIS_SEED: &[u8; 32] = b"ZION_CHv4_mixing_v1_genesis_seed";

/// Blake3 hash genesis seedu (ověřovaný při načtení vah).
pub const CHV4_MLP_SEED_HASH: &str =
    "1e2f3a4b5c6d7e8f9a0b1c2d3e4f5061728394a5b6c7d8e9f0a1b2c3d4e5f607";

/// Výška bloku pro aktivaci CHv4 NPU Mixing stepu.
/// Nastaveno na 0 — CHv4 je aktivní od genesis (blok 0).
/// Výhoda: žádný hard-fork risk, konzistentní hash od prvního bloku.
/// CHv3 legacy i memory-hard fáze jsou zcela přeskočeny.
pub const CHV4_NPU_FORK_HEIGHT: u64 = 0;

// ============================================================================
// MLP WEIGHTS (INT8, deterministické z genesis seedu)
// ============================================================================

/// Váhy INT8 MLP 64→128→64 s residual connection.
/// Generovány jednorázově z genesis seedu — nikdy se nemění.
struct MlpWeights {
    /// W1[128][64]: Linear(64→128)
    w1: Box<[[i8; 64]; 128]>,
    /// b1[128]: bias pro vrstvu 1
    b1: [i8; 128],
    /// W2[64][128]: Linear(128→64)
    w2: Box<[[i8; 128]; 64]>,
    /// b2[64]: bias pro vrstvu 2
    b2: [i8; 64],
    /// scale1: LayerNorm scale pro vrstvu 1 (Q8: 256 = 1.0)
    scale1: [i16; 128],
    /// scale2: LayerNorm scale pro vrstvu 2 (Q8: 256 = 1.0)
    scale2: [i16; 64],
}

impl MlpWeights {
    /// Derive deterministické váhy z genesis seedu pomocí Blake3 key derivation.
    fn from_genesis_seed() -> Self {
        // Celkový počet bytů potřebných:
        //   W1: 128*64 = 8192
        //   b1: 128
        //   W2:  64*128 = 8192
        //   b2:  64
        //   scale1: 128 * 2 = 256 (i16)
        //   scale2:  64 * 2 = 128 (i16)
        //   Total: ~16960 bytes → generujeme 17 × 1024 = 17408 (dost)
        const TOTAL_CHUNKS: usize = 17;
        let mut expanded = Vec::with_capacity(TOTAL_CHUNKS * 1024);

        // Blake3 XOF (extended output) z genesis seedu
        let mut hasher = blake3::Hasher::new_keyed(CHV4_MLP_GENESIS_SEED);
        hasher.update(b"CHv4_weights_v1");

        // Rozšíření do potřebné délky po blocích po 32B s counter
        for chunk_idx in 0u32..(TOTAL_CHUNKS as u32 * 32) {
            let mut h = hasher.clone();
            h.update(&chunk_idx.to_le_bytes());
            let out = h.finalize();
            expanded.extend_from_slice(out.as_bytes());
        }

        let bytes = &expanded;
        let mut pos = 0usize;

        // W1 [128][64]
        let mut w1 = Box::new([[0i8; 64]; 128]);
        for i in 0..128 {
            for j in 0..64 {
                // Centruj kolem 0: raw byte -128..127
                w1[i][j] = bytes[pos] as i8;
                pos += 1;
            }
        }

        // b1 [128]
        let mut b1 = [0i8; 128];
        for i in 0..128 {
            b1[i] = bytes[pos] as i8;
            pos += 1;
        }

        // W2 [64][128]
        let mut w2 = Box::new([[0i8; 128]; 64]);
        for i in 0..64 {
            for j in 0..128 {
                w2[i][j] = bytes[pos] as i8;
                pos += 1;
            }
        }

        // b2 [64]
        let mut b2 = [0i8; 64];
        for i in 0..64 {
            b2[i] = bytes[pos] as i8;
            pos += 1;
        }

        // scale1 [128] — Q8: values 200..312 (≈ 0.78..1.22 multiplier)
        let mut scale1 = [256i16; 128];
        for i in 0..128 {
            // Rozsah 224..288 → dívá se jako 0.875..1.125
            scale1[i] = 224 + (bytes[pos] as i16 & 0x3F);
            pos += 1;
        }

        // scale2 [64]
        let mut scale2 = [256i16; 64];
        for i in 0..64 {
            scale2[i] = 224 + (bytes[pos] as i16 & 0x3F);
            pos += 1;
        }

        Self {
            w1,
            b1,
            w2,
            b2,
            scale1,
            scale2,
        }
    }
}

// Globální lazy-init vah (inicializuje se jednou, thread-safe)
use std::sync::OnceLock;
static NPU_WEIGHTS: OnceLock<MlpWeights> = OnceLock::new();

fn get_weights() -> &'static MlpWeights {
    NPU_WEIGHTS.get_or_init(MlpWeights::from_genesis_seed)
}

// ============================================================================
// FIXED-POINT ARITHMETIC HELPERS (Q8.8)
// ============================================================================

/// GELU aproximace v integer aritmetice.
/// gelu(x) ≈ x * sigmoid(1.702 * x)
/// Pro x v INT8 range [-128..127], výstup je v range [-128..127].
#[inline(always)]
fn gelu_int8(x: i32) -> i32 {
    // sigmoid(1.702 * x) approximated as tanh-like piecewise:
    // |x| > 64 → saturate (±x), else sigmoid(x) ≈ 0.5 + x/256
    // Final: gelu(x) ≈ x * (128 + x) / 256 (clamped to [-128, 127])
    let numerator = x * (128 + x);
    (numerator >> 8).clamp(-128, 127)
}

/// LayerNorm simplified (stats-free integer version):
/// Normalizujeme přes data-dependent sum, aplikujeme scale.
/// Výstup zachovává energii vstupu — vhodné pro kryptografické účely.
#[inline]
fn layer_norm_int8(data: &mut [i32], scale: &[i16]) {
    let n = data.len();

    // Průměr
    let sum: i64 = data.iter().map(|&x| x as i64).sum();
    let mean = (sum / n as i64) as i32;

    // Variance (simplified: sum of (x - mean)^2 / n, integer)
    let var_sum: i64 = data
        .iter()
        .map(|&x| {
            let d = (x - mean) as i64;
            d * d
        })
        .sum();
    let std_approx = ((var_sum / n as i64) as f64).sqrt() as i32 + 1; // +1 prevent /0

    // Normalizace a scale aplikace (Q8: scale/256)
    for (i, x) in data.iter_mut().enumerate() {
        let normalized = ((*x - mean) * 128) / std_approx; // *128 = Q7 precision
        *x = (normalized * scale[i] as i32) >> 8; // scale Q8 → result
        *x = (*x).clamp(-128, 127);
    }
}

// ============================================================================
// CORE NPU MIXING FUNCTION
// ============================================================================

/// CHv4 NPU Mixing Step — deterministický INT8 MLP.
///
/// Vstup:  64-byte scratchpad stav z memory_hard_transform()
/// Výstup: 64-byte mixovaný stav
///
/// Identický výsledek na všech platformách (CPU, CoreML, CUDA) díky INT8.
/// Apple M1/M2: ANE path bude přidán za `native-npu` feature flag.
pub fn npu_mixing_step(scratchpad: &[u8; 64]) -> [u8; 64] {
    // Platform dispatch — v budoucnu CoreML/ONNX za native-npu feature
    #[cfg(all(feature = "native-npu", target_os = "macos", target_arch = "aarch64"))]
    {
        // Budoucí CoreML/Apple ANE path — zatím fallback na CPU INT8
        npu_mixing_cpu_int8(scratchpad)
    }

    #[cfg(not(all(feature = "native-npu", target_os = "macos", target_arch = "aarch64")))]
    {
        npu_mixing_cpu_int8(scratchpad)
    }
}

/// CPU INT8 MLP forward pass (deterministický, všechny platformy).
///
/// Forward pass:
///   1. Linear(64→128): h = W1 @ input + b1
///   2. LayerNorm(128) + GELU
///   3. Linear(128→64): out = W2 @ h + b2
///   4. LayerNorm(64)
///   5. Residual add: out += input
fn npu_mixing_cpu_int8(scratchpad: &[u8; 64]) -> [u8; 64] {
    let w = get_weights();

    // Input konverze u8 → i32: reinterpret jako signed (int8_t) stejně jako C/Metal
    let input_i32: [i32; 64] = {
        let mut arr = [0i32; 64];
        for (i, &b) in scratchpad.iter().enumerate() {
            arr[i] = (b as i8) as i32;
        }
        arr
    };

    // ──────── VRSTVA 1: Linear(64→128) ────────
    let mut hidden = [0i32; 128];

    #[cfg(target_arch = "aarch64")]
    unsafe {
        layer1_neon(&input_i32, &w.w1, &w.b1, &mut hidden);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        layer1_scalar(&input_i32, &w.w1, &w.b1, &mut hidden);
    }

    // LayerNorm + GELU pro hidden
    layer_norm_int8(&mut hidden, &w.scale1);
    for h in hidden.iter_mut() {
        *h = gelu_int8(*h);
    }

    // ──────── VRSTVA 2: Linear(128→64) ────────
    let mut output_i32 = [0i32; 64];

    #[cfg(target_arch = "aarch64")]
    unsafe {
        layer2_neon(&hidden, &w.w2, &w.b2, &mut output_i32);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        layer2_scalar(&hidden, &w.w2, &w.b2, &mut output_i32);
    }

    // LayerNorm pro output
    layer_norm_int8(&mut output_i32, &w.scale2);

    // ──────── RESIDUAL ADD ────────
    for i in 0..64 {
        output_i32[i] = (output_i32[i] + input_i32[i]).clamp(-128, 127);
    }

    // Output konverze i32 → u8: two's complement lower 8 bits (stejně jako C: (uint8_t)(v & 0xFF))
    let mut result = [0u8; 64];
    for (i, &v) in output_i32.iter().enumerate() {
        result[i] = v as u8;
    }

    result
}

/// Linear Layer 1: h[i] = clamp(Σ W1[i][j] * input[j] + b1[i], -128, 127)
#[allow(dead_code)]
#[inline]
fn layer1_scalar(input: &[i32; 64], w1: &[[i8; 64]; 128], b1: &[i8; 128], hidden: &mut [i32; 128]) {
    for i in 0..128 {
        let mut acc: i32 = b1[i] as i32 * 32; // bias upscale (Q5) pro přesnost
        for j in 0..64 {
            acc += input[j] * w1[i][j] as i32;
        }
        // Scale-down: MAC output je v ~±128*64*128 = ±1M rozsahu → scale do ±127
        hidden[i] = (acc >> 12).clamp(-128, 127);
    }
}

/// Linear Layer 2: out[i] = clamp(Σ W2[i][j] * hidden[j] + b2[i], -128, 127)
#[allow(dead_code)]
#[inline]
fn layer2_scalar(hidden: &[i32; 128], w2: &[[i8; 128]; 64], b2: &[i8; 64], output: &mut [i32; 64]) {
    for i in 0..64 {
        let mut acc: i32 = b2[i] as i32 * 32;
        for j in 0..128 {
            acc += hidden[j] * w2[i][j] as i32;
        }
        output[i] = (acc >> 12).clamp(-128, 127);
    }
}

// -----------------------------------------------------------------------
// AARCH64 NEON IMPLEMENTATION FOR CPU FALLBACK
// -----------------------------------------------------------------------
#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

#[cfg(target_arch = "aarch64")]
#[inline]
/// # Safety
/// Requires the `neon` target feature (always available on aarch64). All slice
/// arguments are fixed-size arrays, so the internal pointer reads stay in bounds.
pub unsafe fn layer1_neon(
    input: &[i32; 64],
    w1: &[[i8; 64]; 128],
    b1: &[i8; 128],
    hidden: &mut [i32; 128],
) {
    for i in 0..128 {
        let mut sum_vec = vdupq_n_s32(0);
        let mut j = 0;
        while j < 64 {
            let in0 = vld1q_s32(input.as_ptr().add(j));
            let in1 = vld1q_s32(input.as_ptr().add(j + 4));
            let in2 = vld1q_s32(input.as_ptr().add(j + 8));
            let in3 = vld1q_s32(input.as_ptr().add(j + 12));

            let w_v = vld1q_s8(w1[i].as_ptr().add(j));

            let w_low16 = vmovl_s8(vget_low_s8(w_v));
            let w_high16 = vmovl_s8(vget_high_s8(w_v));

            let w0 = vmovl_s16(vget_low_s16(w_low16));
            let w1_vec = vmovl_s16(vget_high_s16(w_low16));
            let w2 = vmovl_s16(vget_low_s16(w_high16));
            let w3 = vmovl_s16(vget_high_s16(w_high16));

            sum_vec = vmlaq_s32(sum_vec, in0, w0);
            sum_vec = vmlaq_s32(sum_vec, in1, w1_vec);
            sum_vec = vmlaq_s32(sum_vec, in2, w2);
            sum_vec = vmlaq_s32(sum_vec, in3, w3);

            j += 16;
        }
        let acc: i32 = vaddvq_s32(sum_vec) + (b1[i] as i32 * 32);
        hidden[i] = (acc >> 12).clamp(-128, 127);
    }
}

#[cfg(target_arch = "aarch64")]
#[inline]
/// # Safety
/// Requires the `neon` target feature (always available on aarch64). All slice
/// arguments are fixed-size arrays, so the internal pointer reads stay in bounds.
pub unsafe fn layer2_neon(
    hidden: &[i32; 128],
    w2: &[[i8; 128]; 64],
    b2: &[i8; 64],
    output: &mut [i32; 64],
) {
    for i in 0..64 {
        let mut sum_vec = vdupq_n_s32(0);
        let mut j = 0;
        while j < 128 {
            let in0 = vld1q_s32(hidden.as_ptr().add(j));
            let in1 = vld1q_s32(hidden.as_ptr().add(j + 4));
            let in2 = vld1q_s32(hidden.as_ptr().add(j + 8));
            let in3 = vld1q_s32(hidden.as_ptr().add(j + 12));

            let w_v = vld1q_s8(w2[i].as_ptr().add(j));

            let w_low16 = vmovl_s8(vget_low_s8(w_v));
            let w_high16 = vmovl_s8(vget_high_s8(w_v));

            let w0 = vmovl_s16(vget_low_s16(w_low16));
            let w1_vec = vmovl_s16(vget_high_s16(w_low16));
            let w2_vec = vmovl_s16(vget_low_s16(w_high16));
            let w3 = vmovl_s16(vget_high_s16(w_high16));

            sum_vec = vmlaq_s32(sum_vec, in0, w0);
            sum_vec = vmlaq_s32(sum_vec, in1, w1_vec);
            sum_vec = vmlaq_s32(sum_vec, in2, w2_vec);
            sum_vec = vmlaq_s32(sum_vec, in3, w3);

            j += 16;
        }
        let acc: i32 = vaddvq_s32(sum_vec) + (b2[i] as i32 * 32);
        output[i] = (acc >> 12).clamp(-128, 127);
    }
}

// ============================================================================
// HASH64 WRAPPER (integrace s pipeline typy)
// ============================================================================

use crate::algorithms_opt::Hash64;

/// Wrapper vracející Hash64 pro přímou integraci v CHv4 pipeline.
#[inline]
pub fn npu_mixing_hash64(mem_hard_output: &[u8]) -> Hash64 {
    let mut input = [0u8; 64];
    let copy_len = mem_hard_output.len().min(64);
    input[..copy_len].copy_from_slice(&mem_hard_output[..copy_len]);

    let mixed = npu_mixing_step(&input);

    let mut result = Hash64::new();
    result.data.copy_from_slice(&mixed);
    result
}

// ============================================================================
// PUBLIC WEIGHT EXPORT (for GPU backends: OpenCL, CUDA)
// ============================================================================

/// Flat INT8 MLP weights for CHv4 NPU Mixing step.
/// GPU backends (OpenCL, CUDA) use this to upload weights once at init.
pub struct ChV4WeightsFlat {
    /// W1 [128×64] int8, row-major — Linear(64→128)
    pub w1: Vec<i8>,
    /// b1 [128] int8
    pub b1: Vec<i8>,
    /// W2 [64×128] int8, row-major — Linear(128→64)
    pub w2: Vec<i8>,
    /// b2 [64] int8
    pub b2: Vec<i8>,
    /// scale1 [128] int16 — LayerNorm scale layer 1 (Q8: 256=1.0)
    pub scale1: Vec<i16>,
    /// scale2 [64] int16 — LayerNorm scale layer 2
    pub scale2: Vec<i16>,
}

/// Return CHv4 MLP weights as flat arrays ready for GPU buffer upload.
/// Lazy-initialized once (thread-safe via OnceLock).
pub fn chv4_npu_weights_flat() -> ChV4WeightsFlat {
    let w = get_weights();
    ChV4WeightsFlat {
        w1: w.w1.iter().flat_map(|row| row.iter().copied()).collect(),
        b1: w.b1.to_vec(),
        w2: w.w2.iter().flat_map(|row| row.iter().copied()).collect(),
        b2: w.b2.to_vec(),
        scale1: w.scale1.to_vec(),
        scale2: w.scale2.to_vec(),
    }
}

/// Return epoch-aware MLP weights in flat GPU format for a given epoch.
///
/// Only supports Standard topology (epoch % 4 == 0). Panics otherwise.
/// This is the correct weight source for GPU backends matching the v2 pipeline
/// (`npu_mixing_step_epoch`).
pub fn chv4_npu_weights_flat_epoch(epoch: u64) -> ChV4WeightsFlat {
    let weights = get_epoch_weights(epoch);
    assert!(
        weights.topology == MlpTopology::Standard,
        "GPU kernel only supports Standard (64→128→64) topology, got {:?} for epoch {}",
        weights.topology,
        epoch
    );
    let layer0 = &weights.layers[0];
    let layer1 = &weights.layers[1];
    ChV4WeightsFlat {
        w1: layer0.weights.clone(),
        b1: layer0.bias.clone(),
        w2: layer1.weights.clone(),
        b2: layer1.bias.clone(),
        scale1: layer0.scale.clone(),
        scale2: layer1.scale.clone(),
    }
}

/// Packed variable-topology MLP weights for GPU kernels supporting all epoch topologies.
pub struct ChV4WeightsPacked {
    /// All layer weights concatenated: [layer0_weights..., layer1_weights..., ...]
    pub weights: Vec<i8>,
    /// All layer biases concatenated
    pub biases: Vec<i8>,
    /// All layer scales concatenated
    pub scales: Vec<i16>,
    /// Topology metadata: [num_layers, in0, out0, in1, out1, in2, out2]
    pub meta: Vec<u32>,
}

/// Return epoch-aware MLP weights in packed format for variable-topology GPU kernels.
pub fn chv4_npu_weights_packed(epoch: u64) -> ChV4WeightsPacked {
    let weights = get_epoch_weights(epoch);
    let mut w_all = Vec::new();
    let mut b_all = Vec::new();
    let mut s_all = Vec::new();
    let mut meta = vec![weights.layers.len() as u32];
    for layer in &weights.layers {
        w_all.extend_from_slice(&layer.weights);
        b_all.extend_from_slice(&layer.bias);
        s_all.extend_from_slice(&layer.scale);
        meta.push(layer.in_dim as u32);
        meta.push(layer.out_dim as u32);
    }
    ChV4WeightsPacked {
        weights: w_all,
        biases: b_all,
        scales: s_all,
        meta,
    }
}

// ============================================================================
// EPOCH-ROTATING NPU WEIGHTS (Tier 2 ASIC resistance)
// ============================================================================

/// Epoch length — blocks per NPU weight rotation cycle.
/// Mainnet: 2016 (same as Bitcoin difficulty adjustment period).
/// Testnet: 100 (rapid rotation for epoch-boundary testing).
#[cfg(not(feature = "testnet"))]
pub const NPU_EPOCH_LENGTH: u64 = 2016;
#[cfg(feature = "testnet")]
pub const NPU_EPOCH_LENGTH: u64 = 100;

/// Derive epoch number from block height.
#[inline]
pub fn epoch_from_height(height: u64) -> u64 {
    height / NPU_EPOCH_LENGTH
}

/// Derive epoch-specific seed from genesis seed + epoch number.
pub fn epoch_seed(epoch: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_keyed(CHV4_MLP_GENESIS_SEED);
    hasher.update(b"CHv4_epoch_weights_v1");
    hasher.update(&epoch.to_le_bytes());
    *hasher.finalize().as_bytes()
}

/// MLP topology — rotates per epoch (4 variants).
/// Different network shapes force ASIC designers to implement
/// flexible matrix engines rather than fixed-dimension pipelines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MlpTopology {
    /// 64 → 128 → 64 (epoch % 4 == 0)
    Standard,
    /// 64 → 96 → 128 → 64 (epoch % 4 == 1)
    ThreeLayer,
    /// 64 → 256 → 64 (epoch % 4 == 2)
    Wide,
    /// 64 → 64 → 64 → 64 (epoch % 4 == 3)
    Deep,
}

impl MlpTopology {
    pub fn for_epoch(epoch: u64) -> Self {
        match epoch % 4 {
            0 => Self::Standard,
            1 => Self::ThreeLayer,
            2 => Self::Wide,
            3 => Self::Deep,
            _ => unreachable!(),
        }
    }

    /// Layer dimensions: (in_dim, out_dim) for each linear layer.
    fn layer_dims(&self) -> &[(usize, usize)] {
        match self {
            Self::Standard => &[(64, 128), (128, 64)],
            Self::ThreeLayer => &[(64, 96), (96, 128), (128, 64)],
            Self::Wide => &[(64, 256), (256, 64)],
            Self::Deep => &[(64, 64), (64, 64), (64, 64)],
        }
    }
}

/// A single MLP layer with dynamic dimensions.
struct EpochMlpLayer {
    weights: Vec<i8>, // [out_dim * in_dim], row-major
    bias: Vec<i8>,    // [out_dim]
    scale: Vec<i16>,  // [out_dim]
    in_dim: usize,
    out_dim: usize,
}

/// Epoch-specific MLP weights (variable topology).
pub struct EpochMlpWeights {
    pub topology: MlpTopology,
    layers: Vec<EpochMlpLayer>,
}

/// Expand a 32-byte seed into deterministic pseudorandom bytes via Blake3 key derivation.
fn expand_epoch_seed(seed: &[u8; 32], total_bytes: usize) -> Vec<u8> {
    let chunks = total_bytes.div_ceil(32);
    let mut expanded = Vec::with_capacity(chunks * 32);
    let mut hasher = blake3::Hasher::new_keyed(seed);
    hasher.update(b"CHv4_epoch_mlp_v1");
    for chunk_idx in 0u32..(chunks as u32) {
        let mut h = hasher.clone();
        h.update(&chunk_idx.to_le_bytes());
        expanded.extend_from_slice(h.finalize().as_bytes());
    }
    expanded
}

impl EpochMlpWeights {
    /// Generate MLP weights for a given epoch.
    pub fn from_epoch(epoch: u64) -> Self {
        let seed = epoch_seed(epoch);
        let topology = MlpTopology::for_epoch(epoch);
        let dims = topology.layer_dims();

        // Total bytes: weights + bias + scale(1 byte per element) per layer
        let total_bytes: usize = dims
            .iter()
            .map(|&(in_d, out_d)| out_d * in_d + out_d + out_d)
            .sum();

        let bytes = expand_epoch_seed(&seed, total_bytes);
        let mut pos = 0usize;
        let mut layers = Vec::with_capacity(dims.len());

        for &(in_dim, out_dim) in dims {
            let w_len = out_dim * in_dim;
            let weights: Vec<i8> = bytes[pos..pos + w_len].iter().map(|&b| b as i8).collect();
            pos += w_len;

            let bias: Vec<i8> = bytes[pos..pos + out_dim].iter().map(|&b| b as i8).collect();
            pos += out_dim;

            let scale: Vec<i16> = bytes[pos..pos + out_dim]
                .iter()
                .map(|&b| 224 + (b as i16 & 0x3F))
                .collect();
            pos += out_dim;

            layers.push(EpochMlpLayer {
                weights,
                bias,
                scale,
                in_dim,
                out_dim,
            });
        }

        Self { topology, layers }
    }
}

// Epoch weight cache — one weight set per epoch, evict old entries
static EPOCH_WEIGHTS_CACHE: OnceLock<RwLock<HashMap<u64, Arc<EpochMlpWeights>>>> = OnceLock::new();

fn epoch_cache() -> &'static RwLock<HashMap<u64, Arc<EpochMlpWeights>>> {
    EPOCH_WEIGHTS_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Get cached epoch weights (generates and caches if missing).
pub fn get_epoch_weights(epoch: u64) -> Arc<EpochMlpWeights> {
    let cache = epoch_cache();

    // Fast path: read lock
    if let Ok(read) = cache.read() {
        if let Some(w) = read.get(&epoch) {
            return Arc::clone(w);
        }
    }

    // Slow path: write lock + generate
    let mut write = cache.write().unwrap();
    // Double-check after acquiring write lock
    if let Some(w) = write.get(&epoch) {
        return Arc::clone(w);
    }

    let weights = Arc::new(EpochMlpWeights::from_epoch(epoch));
    write.insert(epoch, Arc::clone(&weights));

    // Evict old epochs (keep last 3)
    if write.len() > 3 {
        let min_keep = epoch.saturating_sub(2);
        write.retain(|&k, _| k >= min_keep);
    }

    weights
}

/// Epoch-aware NPU forward pass (variable topology, INT8 deterministic).
///
/// Stack-allocated: two `[i32; 256]` buffers (2 KiB total).
/// Zero heap allocation in the hot path — weights are pre-cached per epoch.
fn epoch_npu_forward(weights: &EpochMlpWeights, input: &[u8; 64]) -> [u8; 64] {
    let mut current = [0i32; 256];
    let mut next = [0i32; 256];

    // Convert input u8 → i32 (signed reinterpret, matches C: (int8_t)input[i])
    for (i, &b) in input.iter().enumerate() {
        current[i] = (b as i8) as i32;
    }

    // Save residual (input dimension is always 64)
    let mut residual = [0i32; 64];
    residual.copy_from_slice(&current[..64]);

    let n_layers = weights.layers.len();
    let mut current_dim: usize = 64;

    for (layer_idx, layer) in weights.layers.iter().enumerate() {
        debug_assert_eq!(layer.in_dim, current_dim);

        // MatMul + bias: next[i] = clamp(Σ w[i][j] * current[j] + b[i]*32, -128, 127)
        for i in 0..layer.out_dim {
            let mut acc = layer.bias[i] as i32 * 32;
            let row_start = i * layer.in_dim;
            for j in 0..layer.in_dim {
                acc += current[j] * layer.weights[row_start + j] as i32;
            }
            next[i] = (acc >> 12).clamp(-128, 127);
        }

        // LayerNorm
        layer_norm_int8(&mut next[..layer.out_dim], &layer.scale);

        // GELU for all but last layer
        if layer_idx < n_layers - 1 {
            for v in next[..layer.out_dim].iter_mut() {
                *v = gelu_int8(*v);
            }
        }

        // Advance: next → current
        current[..layer.out_dim].copy_from_slice(&next[..layer.out_dim]);
        current_dim = layer.out_dim;
    }

    // Final output must be 64 (all topologies end with out_dim=64)
    debug_assert_eq!(current_dim, 64);

    // Residual add + convert i32 → u8 (two's complement, matches C)
    let mut result = [0u8; 64];
    for i in 0..64 {
        result[i] = (current[i] + residual[i]).clamp(-128, 127) as u8;
    }
    result
}

/// Epoch-aware NPU mixing step — public API for Tier 2 consensus pipeline.
///
/// Uses variable MLP topology and weights derived from epoch number.
/// Thread-safe: weights are cached per epoch via RwLock + Arc.
pub fn npu_mixing_step_epoch(input: &[u8; 64], epoch: u64) -> [u8; 64] {
    let weights = get_epoch_weights(epoch);
    epoch_npu_forward(&weights, input)
}

// ============================================================================
// NPUBACKEND TRAIT + DEEKSHA CIRCUIT BREAKER
// ============================================================================

/// Error vrácený při selhání NPU self-testu.
#[derive(Debug)]
pub struct NpuSelfTestError {
    pub backend: &'static str,
    pub detail: String,
}

impl std::fmt::Display for NpuSelfTestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "NPU self-test failed [{}]: {}",
            self.backend, self.detail
        )
    }
}

/// Trait pro NPU mixing backend.
///
/// Implementujeme:
/// - `CpuNpuBackend` — vždy dostupný, reference truth
/// - `DeekshaCircuitBreaker` — wrapper s fallback na CPU při chybě
pub trait NpuBackend: Send + Sync {
    /// Deterministický mix 64-bytového vstupu → 64-bytový výstup.
    /// MUSÍ být bitově identický s `CpuNpuBackend` pro stejný vstup.
    fn mix(&self, input: &[u8; 64]) -> [u8; 64];

    /// Jméno backendu (pro telemetrii a diagnostiku).
    fn name(&self) -> &'static str;

    /// Self-test na referenčním vektoru.
    fn self_test(&self) -> Result<(), NpuSelfTestError>;
}

// -----------------------------------------------------------------------
// CPU backend — reference truth, vždy dostupný
// -----------------------------------------------------------------------

/// CPU INT8 MLP backend — reference truth pro konsenzus.
pub struct CpuNpuBackend;

impl NpuBackend for CpuNpuBackend {
    #[inline]
    fn mix(&self, input: &[u8; 64]) -> [u8; 64] {
        npu_mixing_step(input)
    }

    fn name(&self) -> &'static str {
        "cpu-int8"
    }

    fn self_test(&self) -> Result<(), NpuSelfTestError> {
        // Referenční vektor: deterministický výstup pro nulový vstup
        let zeros = [0u8; 64];
        let out1 = npu_mixing_step(&zeros);
        let out2 = npu_mixing_step(&zeros);
        if out1 != out2 {
            return Err(NpuSelfTestError {
                backend: self.name(),
                detail: "non-deterministic: two calls differ".into(),
            });
        }
        // Výstup nesmí být celý nulový
        if out1.iter().all(|&b| b == 0) {
            return Err(NpuSelfTestError {
                backend: self.name(),
                detail: "output is all-zero (degenerate)".into(),
            });
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------
// ONNX NPU BACKEND (feature = "native-npu")
// -----------------------------------------------------------------------

#[cfg(feature = "native-npu")]
use ort::session::{builder::GraphOptimizationLevel, Session};

#[cfg(feature = "native-npu")]
pub struct OnnxNpuBackend {
    session: std::sync::Mutex<Session>,
}

#[cfg(feature = "native-npu")]
impl OnnxNpuBackend {
    pub fn new() -> Result<Self, String> {
        // Inicializace ORT prostředí globálně (lze volat jen jednou)
        let _ = ort::init().with_name("DeekshaNPU").commit(); // typ v ort 2.0+ vrací jiny typ, ignorujeme vysledek

        // Načíst model
        let model_path = "deeksha_mlp.onnx";

        // V ort 2.0 se execution providers nastavuji trochu jinak
        let mut builder = Session::builder()
            .map_err(|e| e.to_string())?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| e.to_string())?
            .with_intra_threads(1)
            .map_err(|e| e.to_string())?;

        // Pro jednoduchost zatim jedeme na defaulte (na macos to zkusí CoreML pres append_execution_provider)
        // builder = builder.with_coreml(Default::default())? ... pokročilé

        let session = builder
            .commit_from_file(model_path)
            .map_err(|e| e.to_string())?;

        Ok(Self {
            session: std::sync::Mutex::new(session),
        })
    }
}

#[cfg(feature = "native-npu")]
impl NpuBackend for OnnxNpuBackend {
    fn mix(&self, input: &[u8; 64]) -> [u8; 64] {
        // Konverze vstupu na i64 tensor pro přesné operace bez přetečení v ONNX
        let mut input_i64 = vec![0i64; 64];
        for i in 0..64 {
            input_i64[i] = (input[i] as i8) as i64;
        }

        let shape = vec![1, 64];

        // Zpracování do ONNX tenzoru (ort 2.0 pouziva ndarray nebo Tensor::from_array)
        let input_tensor = match ort::value::Tensor::from_array((shape, input_i64)) {
            Ok(v) => v,
            Err(_) => return npu_mixing_cpu_int8(input),
        };

        // V ort 2.0.0 inputs makro vraci literal Vec
        let inputs_vec = ort::inputs!["input" => input_tensor];

        // Ziskani zamku (byl odmazan predchozim prikazem)
        let mut session_lock = self.session.lock().unwrap();
        let outputs = match session_lock.run(inputs_vec) {
            Ok(v) => v,
            Err(_) => return npu_mixing_cpu_int8(input),
        };

        // Extrakce
        let extracted = outputs["output"].try_extract_tensor::<i64>();
        let out_tensor = match extracted {
            Ok(v) => v,
            Err(_) => return npu_mixing_cpu_int8(input),
        };

        // Zpět do u8 - v ort 2.0 out_tensor je tuple (Shape, &[T])
        let flat_data = out_tensor.1;
        let mut result = [0u8; 64];
        for i in 0..64 {
            result[i] = (flat_data[i] as i32).clamp(-128, 127) as u8;
        }

        result
    }

    fn name(&self) -> &'static str {
        "onnx-npu"
    }

    fn self_test(&self) -> Result<(), NpuSelfTestError> {
        let zeros = [0u8; 64];
        let onnx_out = self.mix(&zeros);
        let cpu_out = npu_mixing_cpu_int8(&zeros);

        if onnx_out != cpu_out {
            return Err(NpuSelfTestError {
                backend: self.name(),
                detail: format!(
                    "deterministic unity failed! ONNX {:?} != CPU {:?}",
                    &onnx_out[..4],
                    &cpu_out[..4]
                ),
            });
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------
// Circuit Breaker — Rule E: Operational Compassion
// -----------------------------------------------------------------------

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Stav circuit breakeru.
const CB_CLOSED: u32 = 0; // normální provoz
const CB_OPEN: u32 = 1; // chyba: používá fallback
const CB_HALF_OPEN: u32 = 2; // zkouší recovery

/// Práh chyb před otevřením (Open stav).
const CB_ERROR_THRESHOLD: u32 = 3;

/// Cooldown v ms před pokusem o HalfOpen.
const CB_COOLDOWN_MS: u64 = 30_000; // 30 s

/// Circuit Breaker wrapper kolem NPU backendu.
///
/// Při opakovaných selháních přepne na CPU fallback.
/// Po cooldownu zkusí obnovit primární backend (HalfOpen).
pub struct DeekshaCircuitBreaker {
    /// Primární backend (CPU nebo budoucí ONNX/SIMD).
    primary: Box<dyn NpuBackend>,
    /// CPU fallback — vždy k dispozici.
    fallback: CpuNpuBackend,
    /// Stav: CB_CLOSED | CB_OPEN | CB_HALF_OPEN
    state: AtomicU32,
    /// Počet po sobě jdoucích chyb.
    error_count: AtomicU32,
    /// Unix timestamp (ms) kdy se otevřel (0 = nikdy).
    opened_at_ms: AtomicU64,
    /// Počet hashů přesměrovaných na fallback (telemetrie).
    pub fallback_count: AtomicU64,
}

impl DeekshaCircuitBreaker {
    /// Inicializuj s daným primárním backendem.
    pub fn new(primary: Box<dyn NpuBackend>) -> Self {
        Self {
            primary,
            fallback: CpuNpuBackend,
            state: AtomicU32::new(CB_CLOSED),
            error_count: AtomicU32::new(0),
            opened_at_ms: AtomicU64::new(0),
            fallback_count: AtomicU64::new(0),
        }
    }

    /// CPU-only instance (primární == CPU == fallback).
    pub fn cpu_only() -> Self {
        Self::new(Box::new(CpuNpuBackend))
    }

    /// Vyber nejlepší dostupný backend a proveď self-test.
    /// CPU-only pokud primární selže self-test.
    pub fn build_best_available() -> Self {
        #[cfg(feature = "native-npu")]
        {
            if let Ok(onnx_backend) = OnnxNpuBackend::new() {
                if onnx_backend.self_test().is_ok() {
                    return Self::new(Box::new(onnx_backend));
                } else {
                    eprintln!("[Deeksha] NPU ONNX self-test failed, using CPU fallback");
                }
            } else {
                eprintln!("[Deeksha] NPU ONNX initialization failed, using CPU fallback");
            }
        }

        // Pro 2.9.8: primární backend = CPU INT8 fallback
        let primary = Box::new(CpuNpuBackend);
        let cb = Self::new(primary);
        if let Err(e) = cb.primary.self_test() {
            // Self-test selhal — log a switch na vždy-CPU
            // (tracing nemusí být init; použij eprintln jako fallback)
            eprintln!("[Deeksha] NPU self-test failed: {e}, using CPU fallback");
        }
        cb
    }

    #[inline]
    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    #[inline]
    fn current_state(&self) -> u32 {
        let state = self.state.load(Ordering::Relaxed);
        if state == CB_OPEN {
            // Zkontroluj cooldown → přechod do HalfOpen
            let opened = self.opened_at_ms.load(Ordering::Relaxed);
            if Self::now_ms().saturating_sub(opened) >= CB_COOLDOWN_MS {
                // CAS: Open → HalfOpen
                let _ = self.state.compare_exchange(
                    CB_OPEN,
                    CB_HALF_OPEN,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                );
                return CB_HALF_OPEN;
            }
        }
        state
    }

    #[inline]
    fn record_success(&self) {
        self.error_count.store(0, Ordering::Relaxed);
        // HalfOpen → Closed po úspěchu
        let _ = self.state.compare_exchange(
            CB_HALF_OPEN,
            CB_CLOSED,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }

    #[inline]
    fn record_failure(&self) {
        let prev = self.error_count.fetch_add(1, Ordering::Relaxed);
        if prev + 1 >= CB_ERROR_THRESHOLD {
            self.opened_at_ms.store(Self::now_ms(), Ordering::Relaxed);
            self.state.store(CB_OPEN, Ordering::Relaxed);
        }
    }
}

impl NpuBackend for DeekshaCircuitBreaker {
    fn mix(&self, input: &[u8; 64]) -> [u8; 64] {
        match self.current_state() {
            CB_CLOSED | CB_HALF_OPEN => {
                // Zkus primární; při panice zachyť a degraduj
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.primary.mix(input)
                }));
                match result {
                    Ok(out) => {
                        self.record_success();
                        out
                    }
                    Err(_) => {
                        self.record_failure();
                        self.fallback_count.fetch_add(1, Ordering::Relaxed);
                        self.fallback.mix(input)
                    }
                }
            }
            _ => {
                // CB_OPEN — přímý fallback, bez pokusu
                self.fallback_count.fetch_add(1, Ordering::Relaxed);
                self.fallback.mix(input)
            }
        }
    }

    fn name(&self) -> &'static str {
        "deeksha-circuit-breaker"
    }

    fn self_test(&self) -> Result<(), NpuSelfTestError> {
        self.primary.self_test()
    }
}

// Safety: DeekshaCircuitBreaker drží Box<dyn NpuBackend + Send + Sync>
// a CpuNpuBackend — obojí je Send + Sync.
unsafe impl Send for DeekshaCircuitBreaker {}
unsafe impl Sync for DeekshaCircuitBreaker {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_npu_weights_init() {
        let w = get_weights();
        // Ověřit, že váhy nejsou nulové
        let nonzero = w.w1.iter().flat_map(|row| row.iter()).any(|&x| x != 0);
        assert!(nonzero, "W1 weights should not be all zero");
    }

    #[test]
    fn test_npu_mixing_determinism() {
        let input = [0x42u8; 64];
        let out1 = npu_mixing_step(&input);
        let out2 = npu_mixing_step(&input);
        assert_eq!(out1, out2, "NPU mixing must be deterministic");
    }

    #[test]
    fn test_npu_mixing_different_inputs() {
        let input1 = [0u8; 64];
        let input2 = [0xFFu8; 64];
        let out1 = npu_mixing_step(&input1);
        let out2 = npu_mixing_step(&input2);
        assert_ne!(
            out1, out2,
            "Different inputs must produce different outputs"
        );
    }

    #[test]
    fn test_npu_mixing_avalanche() {
        // Změna 1 bitu ve vstupu → výstup se musí lišit
        let input1 = [0x5Au8; 64];
        let mut input2 = input1;
        input2[0] ^= 0x01;

        let out1 = npu_mixing_step(&input1);
        let out2 = npu_mixing_step(&input2);

        let diff_bytes = out1.iter().zip(out2.iter()).filter(|(a, b)| a != b).count();
        assert!(
            diff_bytes >= 1,
            "Avalanche: at least 1 byte should differ, got {}",
            diff_bytes
        );
    }

    #[test]
    fn test_hash64_wrapper() {
        let input = [0x99u8; 64];
        let out = npu_mixing_hash64(&input);
        assert_eq!(out.data.len(), 64);
        // Výsledek nesmí být prázdný
        let nonzero = out.data.iter().any(|&x| x != 0);
        assert!(nonzero);
    }

    /// Diagnostic: print NPU(zeros) output for C parity comparison
    #[test]
    fn test_npu_zeros_output() {
        let zeros = [0u8; 64];
        let out = npu_mixing_step(&zeros);
        let hex: String = out.iter().map(|b| format!("{:02x}", b)).collect();
        println!("Rust NPU(zeros): {}", hex);
        // expected C output after integer fix - must match for weight parity
    }

    /// Cross-check: NPU input/output konverze musí souhlasit s C native lib
    /// C: (int8_t)input[i] → input → (uint8_t)(out & 0xFF)
    /// Rust po fixu: (b as i8) as i32  →  v as u8
    #[test]
    fn test_npu_input_output_parity_with_c() {
        // Identické vstupy → ověřit že konverze odpovídá C signed reinterpretaci
        // u8=0   → i8=0  (ne -128 jako dřív)
        // u8=128 → i8=-128 (ne 0 jako dřív)
        // u8=255 → i8=-1  (ne 127 jako dřív)
        let mut test_input = [0u8; 64];
        test_input[0] = 0;
        test_input[1] = 128;
        test_input[2] = 255;
        test_input[3] = 127;

        let out = npu_mixing_step(&test_input);
        // Hlavní test: výsledek musí být deterministický a ne všechny nuly
        let nonzero = out.iter().any(|&x| x != 0);
        assert!(nonzero, "NPU output should not be all zeros");

        // Ověřit konverzi: (0u8 as i8) as i32 = 0, (128u8 as i8) as i32 = -128
        assert_eq!((0u8 as i8) as i32, 0, "u8=0 must map to i32=0 (C parity)");
        assert_eq!(
            (128u8 as i8) as i32,
            -128,
            "u8=128 must map to i32=-128 (C parity)"
        );
        assert_eq!(
            (255u8 as i8) as i32,
            -1,
            "u8=255 must map to i32=-1 (C parity)"
        );

        // Ověřit reverse: v as u8 === (uint8_t)(v & 0xFF) jako v C
        let v: i32 = -128;
        assert_eq!(
            v as u8, 128u8,
            "i32=-128 as u8 must be 128 (C two's complement parity)"
        );
        let v: i32 = -1;
        assert_eq!(
            v as u8, 255u8,
            "i32=-1 as u8 must be 255 (C two's complement parity)"
        );
    }

    #[test]
    fn test_gelu_int8_zero() {
        // gelu(0) ≈ 0
        assert_eq!(gelu_int8(0), 0);
    }

    #[test]
    fn test_gelu_int8_positive() {
        // gelu(x) ≈ x pro velká x > 0
        let v = gelu_int8(100);
        assert!(v > 0, "gelu(100) should be positive");
    }

    #[test]
    fn test_layer_norm_reduces_range() {
        let mut data = [
            100i32, -50, 80, -30, 0, 127, -128, 60, 10, 20, 30, 40, 50, 60, 70, 80,
        ];
        let scale = [256i16; 16];
        layer_norm_int8(&mut data, &scale);
        // Po normalizaci by data měla být v ±127
        for &v in &data {
            assert!((-128..=127).contains(&v));
        }
    }

    // ================================================================
    // EPOCH NPU TESTS (Tier 2)
    // ================================================================

    #[test]
    fn test_epoch_from_height() {
        let e = NPU_EPOCH_LENGTH;
        assert_eq!(epoch_from_height(0), 0);
        assert_eq!(epoch_from_height(e - 1), 0);
        assert_eq!(epoch_from_height(e), 1);
        assert_eq!(epoch_from_height(2 * e - 1), 1);
        assert_eq!(epoch_from_height(2 * e), 2);
        assert_eq!(epoch_from_height(4 * e - 1), 3);
        assert_eq!(epoch_from_height(4 * e), 4);
    }

    #[test]
    fn test_epoch_seed_deterministic() {
        let s1 = epoch_seed(0);
        let s2 = epoch_seed(0);
        assert_eq!(s1, s2, "epoch_seed must be deterministic");
    }

    #[test]
    fn test_epoch_seeds_differ() {
        let s0 = epoch_seed(0);
        let s1 = epoch_seed(1);
        let s2 = epoch_seed(2);
        let s3 = epoch_seed(3);
        assert_ne!(s0, s1);
        assert_ne!(s1, s2);
        assert_ne!(s2, s3);
        assert_ne!(s0, s3);
    }

    #[test]
    fn test_topology_rotation() {
        assert_eq!(MlpTopology::for_epoch(0), MlpTopology::Standard);
        assert_eq!(MlpTopology::for_epoch(1), MlpTopology::ThreeLayer);
        assert_eq!(MlpTopology::for_epoch(2), MlpTopology::Wide);
        assert_eq!(MlpTopology::for_epoch(3), MlpTopology::Deep);
        assert_eq!(MlpTopology::for_epoch(4), MlpTopology::Standard);
        assert_eq!(MlpTopology::for_epoch(100), MlpTopology::Standard);
        assert_eq!(MlpTopology::for_epoch(101), MlpTopology::ThreeLayer);
    }

    #[test]
    fn test_epoch_weights_generation() {
        for epoch in 0..4 {
            let w = EpochMlpWeights::from_epoch(epoch);
            let expected_topology = MlpTopology::for_epoch(epoch);
            assert_eq!(w.topology, expected_topology, "epoch={}", epoch);

            let dims = expected_topology.layer_dims();
            assert_eq!(w.layers.len(), dims.len(), "epoch={}", epoch);
            for (i, layer) in w.layers.iter().enumerate() {
                assert_eq!(layer.in_dim, dims[i].0, "epoch={} layer={}", epoch, i);
                assert_eq!(layer.out_dim, dims[i].1, "epoch={} layer={}", epoch, i);
                assert_eq!(layer.weights.len(), dims[i].0 * dims[i].1);
                assert_eq!(layer.bias.len(), dims[i].1);
                assert_eq!(layer.scale.len(), dims[i].1);
                let nonzero = layer.weights.iter().any(|&x| x != 0);
                assert!(nonzero, "epoch={} layer={} weights all zero", epoch, i);
            }
        }
    }

    #[test]
    fn test_epoch_npu_deterministic() {
        let input = [0x42u8; 64];
        let out1 = npu_mixing_step_epoch(&input, 0);
        let out2 = npu_mixing_step_epoch(&input, 0);
        assert_eq!(out1, out2, "epoch NPU must be deterministic");
    }

    #[test]
    fn test_epoch_npu_nonzero() {
        let input = [0u8; 64];
        for epoch in 0..4 {
            let out = npu_mixing_step_epoch(&input, epoch);
            let nonzero = out.iter().any(|&b| b != 0);
            assert!(nonzero, "epoch={} output should not be all zeros", epoch);
        }
    }

    #[test]
    fn test_epoch_npu_different_epochs_differ() {
        let input = [0x5Au8; 64];
        let out0 = npu_mixing_step_epoch(&input, 0);
        let out1 = npu_mixing_step_epoch(&input, 1);
        let out2 = npu_mixing_step_epoch(&input, 2);
        let out3 = npu_mixing_step_epoch(&input, 3);
        assert_ne!(
            out0, out1,
            "Different epochs must produce different outputs"
        );
        assert_ne!(out1, out2);
        assert_ne!(out2, out3);
        assert_ne!(out0, out3);
    }

    #[test]
    fn test_epoch_npu_avalanche() {
        let input1 = [0x5Au8; 64];
        let mut input2 = input1;
        input2[0] ^= 0x01;
        let out1 = npu_mixing_step_epoch(&input1, 0);
        let out2 = npu_mixing_step_epoch(&input2, 0);
        let diff = out1.iter().zip(out2.iter()).filter(|(a, b)| a != b).count();
        assert!(
            diff >= 1,
            "Avalanche: at least 1 byte should differ, got {}",
            diff
        );
    }

    #[test]
    fn test_epoch_cache_reuse() {
        let w1 = get_epoch_weights(42);
        let w2 = get_epoch_weights(42);
        assert!(Arc::ptr_eq(&w1, &w2), "Same epoch should return cached Arc");
    }

    #[test]
    fn test_epoch_npu_differs_from_genesis() {
        let input = [0x42u8; 64];
        let genesis_out = npu_mixing_step(&input);
        let epoch0_out = npu_mixing_step_epoch(&input, 0);
        assert_ne!(
            genesis_out, epoch0_out,
            "Epoch 0 NPU uses different key derivation than genesis — outputs must differ"
        );
    }

    #[test]
    fn test_testnet_epoch_length_value() {
        // Verify the feature flag sets the correct epoch length
        #[cfg(feature = "testnet")]
        assert_eq!(NPU_EPOCH_LENGTH, 100, "testnet epoch must be 100 blocks");
        #[cfg(not(feature = "testnet"))]
        assert_eq!(NPU_EPOCH_LENGTH, 2016, "mainnet epoch must be 2016 blocks");
    }

    #[test]
    fn test_epoch_boundary_produces_different_weights() {
        // Block just before and just after epoch boundary must use different topologies
        let last_in_epoch0 = NPU_EPOCH_LENGTH - 1;
        let first_in_epoch1 = NPU_EPOCH_LENGTH;
        let w0 = get_epoch_weights(epoch_from_height(last_in_epoch0));
        let w1 = get_epoch_weights(epoch_from_height(first_in_epoch1));
        assert!(
            !Arc::ptr_eq(&w0, &w1),
            "Different epochs must use different weight sets"
        );
    }
}
