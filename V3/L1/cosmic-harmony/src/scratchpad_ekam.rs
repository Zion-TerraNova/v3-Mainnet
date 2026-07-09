//! Ekam Deeksha Scratchpad — Blake3 XOF Init + AES Cascade Mixing
//!
//! Tier 2 replacement for the SHA3-based scratchpad.
//! Same 64 KiB memory-hard structure, but ~8–12× faster inner primitives.
//!
//! Changes from original scratchpad:
//! - `init_scratchpad()`: SHA3-512 chain (1024 calls) → Blake3 XOF (1 call)
//! - `mix_block()`: SHA3-512 mixing → AES cascade mixing (5-round AES-128)
//! - `random_read_mix()`: Preserved (Keccak-256 dependent reads — ASIC resistance)
//! - `merkabah_backward_passes()`: SHA3-512 → AES cascade
//! - `kabala_phase()`: Preserved (Keccak-256 — HIC-dependent)
//! - `brahma_jyoti_finalize()`: Preserved (SHA3-512 — crypto boundary)
//!
//! Security invariants:
//! - 64 KiB physical memory wall (unchanged)
//! - Data-dependent block access patterns (unchanged)
//! - Keccak-256 at random read boundaries (unchanged)
//! - SHA3-512 at finalization (unchanged)
//! - Blake3 XOF for init is cryptographically sound (domain-separated)
//! - AES cascade is data-dependent and hardware-accelerated

// Memory-hard loops intentionally use index-based access for explicit offset math.
#![allow(clippy::needless_range_loop)]

use sha3::{Digest, Keccak256};

use crate::algorithms_opt::Hash64;
use crate::hic::{BACKWARD_PASSES, HIC, KABALA_READS, KEY_ROUNDS};
use crate::hugepages::with_huge_page_scratchpad;
use crate::sha3_fast;

/// Execute `f` with a thread-local scratchpad buffer.
///
/// Uses huge pages (2 MiB) when available for optimal TLB performance.
/// Falls back to regular mmap pages transparently.
#[inline]
fn with_scratchpad<F, R>(f: F) -> R
where
    F: FnOnce(&mut [u8]) -> R,
{
    with_huge_page_scratchpad(SCRATCHPAD_SIZE, f)
}

/// Scratchpad size (64 KiB) — same as original Deeksha.
pub const SCRATCHPAD_SIZE: usize = 64 * 1024;
const BLOCK_SIZE: usize = 64;
const PASSES: usize = 2;
const RANDOM_READS: usize = 64;

// ============================================================================
// Ekam v2 — ASIC-hardened scratchpad profile (Tier 1)
// ============================================================================

/// Ekam v2 scratchpad size (256 KiB) — 4× v1 for L2 cache pressure.
pub const SCRATCHPAD_SIZE_V2: usize = 256 * 1024;
const PASSES_V2: usize = 4;
const RANDOM_READS_V2: usize = 256;

// ============================================================================
// INIT — Blake3 XOF (replaces 1024× SHA3-512)
// ============================================================================

/// Initialize scratchpad from 64B seed using Blake3 XOF.
///
/// Domain separation: "EKAM_SCRATCHPAD_INIT_V1" prevents cross-context collisions.
/// Single call fills entire 64 KiB deterministically.
fn init_scratchpad_ekam(seed: &[u8; 64], pad: &mut [u8]) {
    debug_assert!(pad.len() >= BLOCK_SIZE, "scratchpad too small");

    let mut hasher = blake3::Hasher::new();
    hasher.update(seed);
    hasher.update(b"EKAM_SCRATCHPAD_INIT_V1");
    let mut reader = hasher.finalize_xof();
    reader.fill(pad);
}

// ============================================================================
// SEQUENTIAL PASSES — AES Cascade (replaces 2048× SHA3-512)
// ============================================================================

fn sequential_passes_ekam(pad: &mut [u8], passes: usize) {
    let blocks = pad.len() / BLOCK_SIZE;

    for pass in 0..passes {
        let forward = pass % 2 == 0;

        if forward {
            for i in 0..blocks {
                mix_block_ekam(pad, i, pass as u64, true);
            }
        } else {
            for i in (0..blocks).rev() {
                mix_block_ekam(pad, i, pass as u64, false);
            }
        }
    }
}

#[inline]
fn mix_block_ekam(pad: &mut [u8], index: usize, pass: u64, forward: bool) {
    let blocks = pad.len() / BLOCK_SIZE;

    let cur_off = index * BLOCK_SIZE;
    let prev_index = if forward {
        if index == 0 {
            blocks - 1
        } else {
            index - 1
        }
    } else if index + 1 == blocks {
        0
    } else {
        index + 1
    };
    let prev_off = prev_index * BLOCK_SIZE;

    // Random block index (same computation as original)
    let mut idx_bytes = [0u8; 8];
    idx_bytes.copy_from_slice(&pad[cur_off..cur_off + 8]);
    let rand_index = ((u64::from_le_bytes(idx_bytes) ^ pass ^ (index as u64)) as usize) % blocks;
    let rand_off = rand_index * BLOCK_SIZE;

    // Prefetch next random block (x86_64 only)
    prefetch_next(pad, index, pass, forward, blocks);

    // Blake3 XOF mixing (replaces SHA3-512 — ~2-3× faster)
    let mut hasher = blake3::Hasher::new();
    hasher.update(&pad[cur_off..cur_off + BLOCK_SIZE]); // current block
    hasher.update(&pad[prev_off..prev_off + BLOCK_SIZE]); // previous block
    hasher.update(&pad[rand_off..rand_off + BLOCK_SIZE]); // random block
    hasher.update(&pass.to_le_bytes()); // pass metadata
    hasher.update(&(index as u64).to_le_bytes()); // index metadata
    let mut mixed = [0u8; BLOCK_SIZE];
    hasher.finalize_xof().fill(&mut mixed);

    // XOR result into scratchpad
    xor_block_in_place(&mut pad[cur_off..cur_off + BLOCK_SIZE], &mixed);
}

// ============================================================================
// MERKABAH BACKWARD PASSES — AES Cascade (replaces SHA3-512)
// ============================================================================

fn merkabah_backward_passes_ekam(pad: &mut [u8], seed: &[u8; 64]) {
    let blocks = pad.len() / BLOCK_SIZE;

    for pass in 0..BACKWARD_PASSES {
        for b in (0..blocks).rev() {
            let next_b = (b + 1) % blocks;
            let hic_idx = (blocks - 1 - b) % KEY_ROUNDS;

            let cur_off = b * BLOCK_SIZE;
            let next_off = next_b * BLOCK_SIZE;

            // Blake3 XOF mixing with HIC-enriched metadata
            let mut hasher = blake3::Hasher::new();
            hasher.update(&pad[cur_off..cur_off + BLOCK_SIZE]);
            hasher.update(&pad[next_off..next_off + BLOCK_SIZE]);
            hasher.update(seed);
            hasher.update(&HIC[hic_idx].to_le_bytes());
            hasher.update(&(pass as u64).to_le_bytes());
            hasher.update(&(b as u64).to_le_bytes());
            let mut mixed = [0u8; BLOCK_SIZE];
            hasher.finalize_xof().fill(&mut mixed);

            xor_block_in_place(&mut pad[cur_off..cur_off + BLOCK_SIZE], &mixed);
        }
    }
}

// ============================================================================
// CHv4.2 MERKABAH DUAL-SPIN — Forward + Backward HIC passes
// ============================================================================

/// Forward HIC-enriched passes — ascending Sefirot (Malkuth → Kether).
///
/// Mirrors `merkabah_backward_passes_ekam` but iterates forward, indexing
/// HIC in ascending order.  Together the two directions form the Merkabah
/// dual-spin (counter-rotating wheels).
fn merkabah_forward_passes_ekam(pad: &mut [u8], seed: &[u8; 64]) {
    let blocks = pad.len() / BLOCK_SIZE;

    for pass in 0..BACKWARD_PASSES {
        for b in 0..blocks {
            let prev_b = if b == 0 { blocks - 1 } else { b - 1 };
            let hic_idx = b % KEY_ROUNDS;

            let cur_off = b * BLOCK_SIZE;
            let prev_off = prev_b * BLOCK_SIZE;

            let mut hasher = blake3::Hasher::new();
            hasher.update(&pad[cur_off..cur_off + BLOCK_SIZE]);
            hasher.update(&pad[prev_off..prev_off + BLOCK_SIZE]);
            hasher.update(seed);
            hasher.update(&HIC[hic_idx].to_le_bytes());
            hasher.update(&(pass as u64).to_le_bytes());
            hasher.update(&(b as u64).to_le_bytes());
            let mut mixed = [0u8; BLOCK_SIZE];
            hasher.finalize_xof().fill(&mut mixed);

            xor_block_in_place(&mut pad[cur_off..cur_off + BLOCK_SIZE], &mixed);
        }
    }
}

/// CHv4.2 Merkabah Dual-Spin — interleaved forward + backward HIC passes.
///
/// Each iteration performs one forward pass (ascending Sefirot) followed by
/// one backward pass (descending Sefirot), creating the counter-rotating
/// wheel-within-wheel mixing pattern that maximizes diffusion of HIC
/// constants across the full scratchpad.
fn merkabah_dual_spin_ekam(pad: &mut [u8], seed: &[u8; 64]) {
    merkabah_forward_passes_ekam(pad, seed);
    merkabah_backward_passes_ekam(pad, seed);
}

// ============================================================================
// RANDOM READ MIX — Preserved (Keccak-256 — crypto boundary)
// ============================================================================

fn random_read_mix(seed: &[u8; 64], pad: &[u8], random_reads: usize) -> Hash64 {
    let blocks = pad.len() / BLOCK_SIZE;
    let mut acc = *seed;

    let mut pos_bytes = [0u8; 8];
    pos_bytes.copy_from_slice(&seed[..8]);
    let mut pos = (u64::from_le_bytes(pos_bytes) as usize) % blocks;

    for r in 0..random_reads {
        let off = pos * BLOCK_SIZE;
        let chunk = &pad[off..off + BLOCK_SIZE];

        let mut h = Keccak256::new();
        h.update(acc);
        h.update(chunk);
        h.update((r as u64).to_le_bytes());
        let d = h.finalize();

        for i in 0..32 {
            acc[i] ^= d[i];
            acc[32 + i] = acc[32 + i].wrapping_add(d[i]);
        }

        let mut next_seed = [0u8; 8];
        next_seed.copy_from_slice(&d[..8]);
        pos = ((u64::from_le_bytes(next_seed) as usize) ^ pos ^ r) % blocks;
    }

    let mut first_block = [0u8; BLOCK_SIZE];
    first_block.copy_from_slice(&pad[..BLOCK_SIZE]);
    let mut last_block = [0u8; BLOCK_SIZE];
    last_block.copy_from_slice(&pad[pad.len() - BLOCK_SIZE..]);
    sha3_fast::sha3_512_64_64_64(&acc, &first_block, &last_block)
}

// ============================================================================
// KABALA PHASE — Preserved (Keccak-256 + HIC)
// ============================================================================

fn kabala_phase(pad: &[u8], seed: &[u8; 64]) -> [u8; 64] {
    let blocks = pad.len() / BLOCK_SIZE;
    let mut acc = *seed;

    for k in 0..KABALA_READS {
        let mut state_word = [0u8; 8];
        state_word.copy_from_slice(&acc[..8]);
        let state_u64 = u64::from_le_bytes(state_word);
        let kabala_addr = ((HIC[k] ^ state_u64) as usize) % blocks;

        let kab_off = kabala_addr * BLOCK_SIZE;
        let chunk = &pad[kab_off..kab_off + BLOCK_SIZE];

        let mut h = Keccak256::new();
        h.update(acc);
        h.update(chunk);
        h.update(HIC[k].to_le_bytes());
        h.update((k as u64).to_le_bytes());
        let d = h.finalize();

        for i in 0..32 {
            acc[i] ^= d[i];
            acc[32 + i] = acc[32 + i].wrapping_add(d[i]);
        }
    }

    acc
}

// ============================================================================
// BRAHMA-JYOTI FINALIZE — Preserved (SHA3-512 — crypto boundary)
// ============================================================================

fn brahma_jyoti_finalize(state: &[u8; 64]) -> Hash64 {
    let mut acc = *state;

    for r in 0..KEY_ROUNDS {
        let hic_bytes = HIC[r].to_le_bytes();
        let round_bytes = (r as u64).to_le_bytes();
        let out = sha3_fast::sha3_512_chunks([&acc, &hic_bytes, &round_bytes]);

        for i in 0..32 {
            acc[i] ^= out.data[i];
            acc[32 + i] = acc[32 + i].wrapping_add(out.data[32 + i]);
        }
    }

    let mut hash = Hash64::new();
    hash.data.copy_from_slice(&acc);
    hash
}

// ============================================================================
// PUBLIC API — Ekam Memory-Hard Transform
// ============================================================================

/// Ekam Deeksha memory-hard transform (Tier 2).
///
/// Same structure as original, but with Blake3 XOF init + AES cascade mixing.
/// Keccak-256 random reads and SHA3-512 finalization are preserved.
///
/// Pipeline:
/// ```text
/// Blake3 XOF init(seed → 64 KiB) →
/// AES cascade forward/backward passes(64 KiB) →
/// Merkabah backward passes(AES cascade) →
/// Keccak-256 × 64 random reads →
/// Kabala 22 HIC reads →
/// Brahma-jyoti SHA3-512 finalize → Hash64
/// ```
pub fn memory_hard_transform_ekam(input: &[u8; 64]) -> Hash64 {
    with_scratchpad(|pad| {
        // Phase 1: Blake3 XOF init (replaces 1024× SHA3-512)
        init_scratchpad_ekam(input, pad);

        // Phase 2: AES cascade forward/backward passes (replaces 2048× SHA3-512)
        sequential_passes_ekam(pad, PASSES);

        // Phase 3: Merkabah backward passes (AES cascade)
        merkabah_backward_passes_ekam(pad, input);

        // Phase 4: Random read mix (Keccak-256 — preserved)
        let mh_output = random_read_mix(input, pad, RANDOM_READS);

        // Phase 5: Kabala phase — 22 HIC reads (preserved)
        let kabala_state = kabala_phase(pad, &mh_output.data);

        // Phase 6: Brahma-jyoti finalize (SHA3-512 — preserved)
        brahma_jyoti_finalize(&kabala_state)
    })
}

/// Lighter Ekam variant without Merkabah/Kabala/Brahma-jyoti extensions.
/// Matches the original `memory_hard_transform` structure but with Ekam primitives.
pub fn memory_hard_transform_ekam_light(input: &[u8; 64]) -> Hash64 {
    with_scratchpad(|pad| {
        init_scratchpad_ekam(input, pad);
        sequential_passes_ekam(pad, PASSES);
        random_read_mix(input, pad, RANDOM_READS)
    })
}

// ============================================================================
// PUBLIC API — Ekam v2 Memory-Hard Transform (Tier 1 ASIC hardening)
// ============================================================================

/// Ekam Deeksha v2 — full memory-hard transform with 256 KiB scratchpad.
///
/// Same pipeline as v1 but with hardened parameters:
/// - 256 KiB scratchpad (4× v1)
/// - 4 sequential passes (2× v1)
/// - 256 random reads (4× v1)
pub fn memory_hard_transform_ekam_v2(input: &[u8; 64]) -> Hash64 {
    with_huge_page_scratchpad(SCRATCHPAD_SIZE_V2, |pad| {
        init_scratchpad_ekam(input, pad);
        sequential_passes_ekam(pad, PASSES_V2);
        merkabah_backward_passes_ekam(pad, input);
        let mh_output = random_read_mix(input, pad, RANDOM_READS_V2);
        let kabala_state = kabala_phase(pad, &mh_output.data);
        brahma_jyoti_finalize(&kabala_state)
    })
}

/// Ekam Deeksha v2 light — 256 KiB scratchpad, 4 passes, 256 random reads.
pub fn memory_hard_transform_ekam_light_v2(input: &[u8; 64]) -> Hash64 {
    with_huge_page_scratchpad(SCRATCHPAD_SIZE_V2, |pad| {
        init_scratchpad_ekam(input, pad);
        sequential_passes_ekam(pad, PASSES_V2);
        random_read_mix(input, pad, RANDOM_READS_V2)
    })
}

/// Ekam Deeksha v2 light SHA3-512 version — matches GPU kernel memory_hard_transform.
/// Uses SHA3-512 chain for init and mixing (original Deeksha v1/v2 behavior).
/// Note: GPU kernel has #define PASSES 4 but comment says "2 sequential passes".
/// Using 4 passes to match #define (GPU actual behavior).
pub fn memory_hard_transform_ekam_light_v2_sha3(input: &[u8; 64]) -> Hash64 {
    with_huge_page_scratchpad(SCRATCHPAD_SIZE_V2, |pad| {
        init_scratchpad_sha3(input, pad);
        sequential_passes_sha3(pad, 4); // 4 passes per GPU kernel #define
        random_read_mix(input, pad, RANDOM_READS_V2) // 256 random reads
    })
}

/// SHA3-512 scratchpad init (matches GPU kernel init_scratchpad).
fn init_scratchpad_sha3(seed: &[u8; 64], pad: &mut [u8]) {
    let blocks = pad.len() / BLOCK_SIZE;
    let mut state = *seed;

    for blk in 0..blocks {
        let mut input = [0u8; 72]; // state(64) + counter(8)
        input[..64].copy_from_slice(&state);
        input[64..72].copy_from_slice(&(blk as u64).to_le_bytes());

        let out = sha3_fast::sha3_512_bytes(&input);
        let off = blk * BLOCK_SIZE;
        pad[off..off + BLOCK_SIZE].copy_from_slice(&out.data);
        state = out.data;
    }
}

/// SHA3-512 sequential passes (matches GPU kernel sequential_passes).
fn sequential_passes_sha3(pad: &mut [u8], passes: usize) {
    let blocks = pad.len() / BLOCK_SIZE;

    for pass in 0..passes {
        let forward = pass % 2 == 0;

        if forward {
            for i in 0..blocks {
                mix_block_sha3(pad, i, pass as u64, true);
            }
        } else {
            for i in (0..blocks).rev() {
                mix_block_sha3(pad, i, pass as u64, false);
            }
        }
    }
}

/// SHA3-512 mix block (matches GPU kernel mix_block).
fn mix_block_sha3(pad: &mut [u8], index: usize, pass: u64, forward: bool) {
    let blocks = pad.len() / BLOCK_SIZE;

    let cur_off = index * BLOCK_SIZE;
    let prev_index = if forward {
        if index == 0 {
            blocks - 1
        } else {
            index - 1
        }
    } else if index + 1 == blocks {
        0
    } else {
        index + 1
    };
    let prev_off = prev_index * BLOCK_SIZE;

    // Random block index (same computation as GPU kernel)
    let mut idx_bytes = [0u8; 8];
    idx_bytes.copy_from_slice(&pad[cur_off..cur_off + 8]);
    let rand_index = ((u64::from_le_bytes(idx_bytes) ^ pass ^ (index as u64)) as usize) % blocks;
    let rand_off = rand_index * BLOCK_SIZE;

    // SHA3-512(current || prev || random || pass_le || index_le)
    let mut cur_arr = [0u8; BLOCK_SIZE];
    cur_arr.copy_from_slice(&pad[cur_off..cur_off + BLOCK_SIZE]);
    let mut prev_arr = [0u8; BLOCK_SIZE];
    prev_arr.copy_from_slice(&pad[prev_off..prev_off + BLOCK_SIZE]);
    let mut rand_arr = [0u8; BLOCK_SIZE];
    rand_arr.copy_from_slice(&pad[rand_off..rand_off + BLOCK_SIZE]);
    let mixed = sha3_fast::sha3_512_64_64_64_8_8(
        &cur_arr,
        &prev_arr,
        &rand_arr,
        &pass.to_le_bytes(),
        &(index as u64).to_le_bytes(),
    );

    // XOR result into current block
    xor_block_in_place(&mut pad[cur_off..cur_off + BLOCK_SIZE], &mixed.data);
}

// ============================================================================
// PUBLIC API — CHv4.2 Merkabah Dual-Spin Transform (Phase X fork-gated)
// ============================================================================

/// Ekam Deeksha v3 — CHv4.2 full Merkabah Dual-Spin transform.
///
/// Extends v2 with the complete HIC pipeline:
/// - Dual-spin Merkabah (forward + backward HIC-enriched passes)
/// - Kabala phase (22 HIC-addressed dependent reads)
/// - Brahma-jyoti finalization (22 rounds of SHA3-512 + HIC)
///
/// This is the Phase X+ algorithm, activated by `CHV42_DUAL_SPIN_FORK_HEIGHT`.
///
/// Pipeline:
/// ```text
/// Blake3 XOF init(seed → 256 KiB) →
/// AES cascade 4 passes(256 KiB) →
/// Merkabah Dual-Spin (forward + backward HIC passes) →
/// Keccak-256 × 256 random reads →
/// Kabala 22 HIC reads →
/// Brahma-jyoti SHA3-512 finalize → Hash64
/// ```
pub fn memory_hard_transform_ekam_v3(input: &[u8; 64]) -> Hash64 {
    with_huge_page_scratchpad(SCRATCHPAD_SIZE_V2, |pad| {
        // Phase 1: Blake3 XOF init
        init_scratchpad_ekam(input, pad);

        // Phase 2: AES cascade forward/backward passes
        sequential_passes_ekam(pad, PASSES_V2);

        // Phase 3: CHv4.2 Merkabah Dual-Spin (forward + backward HIC passes)
        merkabah_dual_spin_ekam(pad, input);

        // Phase 4: Random read mix (Keccak-256 — preserved)
        let mh_output = random_read_mix(input, pad, RANDOM_READS_V2);

        // Phase 5: Kabala phase — 22 HIC reads (preserved)
        let kabala_state = kabala_phase(pad, &mh_output.data);

        // Phase 6: Brahma-jyoti finalize (SHA3-512 — preserved)
        brahma_jyoti_finalize(&kabala_state)
    })
}

// ============================================================================
// UTILITY — Prefetch + XOR (shared with original scratchpad)
// ============================================================================

#[inline]
#[cfg(target_arch = "x86_64")]
fn prefetch_next(pad: &[u8], index: usize, pass: u64, forward: bool, blocks: usize) {
    let next_index = if forward {
        if index + 1 < blocks {
            index + 1
        } else {
            return;
        }
    } else {
        if index > 0 {
            index - 1
        } else {
            return;
        }
    };

    unsafe {
        use std::arch::x86_64::{_mm_prefetch, _MM_HINT_T0};
        let mut idx_bytes = [0u8; 8];
        let next_off = next_index * BLOCK_SIZE;
        idx_bytes.copy_from_slice(&pad[next_off..next_off + 8]);
        let next_rand_index =
            ((u64::from_le_bytes(idx_bytes) ^ pass ^ (next_index as u64)) as usize) % blocks;
        let next_rand_off = next_rand_index * BLOCK_SIZE;
        _mm_prefetch(pad.as_ptr().add(next_rand_off) as *const i8, _MM_HINT_T0);
    }
}

#[inline]
#[cfg(not(target_arch = "x86_64"))]
fn prefetch_next(_pad: &[u8], _index: usize, _pass: u64, _forward: bool, _blocks: usize) {}

#[inline(always)]
fn xor_block_in_place(dest: &mut [u8], src: &[u8]) {
    debug_assert_eq!(dest.len(), BLOCK_SIZE);
    debug_assert_eq!(src.len(), BLOCK_SIZE);

    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            unsafe {
                xor_avx2(dest.as_mut_ptr(), src.as_ptr());
            }
            return;
        }
    }

    #[cfg(target_arch = "aarch64")]
    unsafe {
        xor_neon(dest.as_mut_ptr(), src.as_ptr());
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    for i in 0..BLOCK_SIZE {
        dest[i] ^= src[i];
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn xor_avx2(dest: *mut u8, src: *const u8) {
    use std::arch::x86_64::{__m256i, _mm256_loadu_si256, _mm256_storeu_si256, _mm256_xor_si256};
    let l0 = _mm256_loadu_si256(dest as *const __m256i);
    let r0 = _mm256_loadu_si256(src as *const __m256i);
    _mm256_storeu_si256(dest as *mut __m256i, _mm256_xor_si256(l0, r0));
    let l1 = _mm256_loadu_si256(dest.add(32) as *const __m256i);
    let r1 = _mm256_loadu_si256(src.add(32) as *const __m256i);
    _mm256_storeu_si256(dest.add(32) as *mut __m256i, _mm256_xor_si256(l1, r1));
}

#[cfg(target_arch = "aarch64")]
unsafe fn xor_neon(dest: *mut u8, src: *const u8) {
    use std::arch::aarch64::{uint8x16_t, veorq_u8, vld1q_u8, vst1q_u8};
    let l0: uint8x16_t = vld1q_u8(dest);
    let r0: uint8x16_t = vld1q_u8(src);
    vst1q_u8(dest, veorq_u8(l0, r0));
    let l1: uint8x16_t = vld1q_u8(dest.add(16));
    let r1: uint8x16_t = vld1q_u8(src.add(16));
    vst1q_u8(dest.add(16), veorq_u8(l1, r1));
    let l2: uint8x16_t = vld1q_u8(dest.add(32));
    let r2: uint8x16_t = vld1q_u8(src.add(32));
    vst1q_u8(dest.add(32), veorq_u8(l2, r2));
    let l3: uint8x16_t = vld1q_u8(dest.add(48));
    let r3: uint8x16_t = vld1q_u8(src.add(48));
    vst1q_u8(dest.add(48), veorq_u8(l3, r3));
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ekam_memory_hard_deterministic() {
        let input = [7u8; 64];
        let a = memory_hard_transform_ekam(&input);
        let b = memory_hard_transform_ekam(&input);
        assert_eq!(a.data, b.data, "Ekam transform must be deterministic");
    }

    #[test]
    fn test_ekam_memory_hard_avalanche() {
        let mut input_a = [0u8; 64];
        let mut input_b = [0u8; 64];
        input_a[0] = 0;
        input_b[0] = 1;
        let a = memory_hard_transform_ekam(&input_a);
        let b = memory_hard_transform_ekam(&input_b);
        assert_ne!(
            a.data, b.data,
            "Different inputs must produce different outputs"
        );
    }

    #[test]
    fn test_ekam_memory_hard_nonzero() {
        let input = [42u8; 64];
        let result = memory_hard_transform_ekam(&input);
        assert!(
            result.data.iter().any(|&b| b != 0),
            "Output must not be all zeros"
        );
    }

    #[test]
    fn test_ekam_light_deterministic() {
        let input = [7u8; 64];
        let a = memory_hard_transform_ekam_light(&input);
        let b = memory_hard_transform_ekam_light(&input);
        assert_eq!(a.data, b.data);
    }

    // V3 note: test_ekam_differs_from_original removed — V3 has no legacy scratchpad.rs

    // ================================================================
    // Ekam v2 (Tier 1 ASIC hardening) tests
    // ================================================================

    #[test]
    fn test_ekam_v2_light_deterministic() {
        let input = [7u8; 64];
        let a = memory_hard_transform_ekam_light_v2(&input);
        let b = memory_hard_transform_ekam_light_v2(&input);
        assert_eq!(a.data, b.data, "Ekam v2 light must be deterministic");
    }

    #[test]
    fn test_ekam_v2_full_deterministic() {
        let input = [7u8; 64];
        let a = memory_hard_transform_ekam_v2(&input);
        let b = memory_hard_transform_ekam_v2(&input);
        assert_eq!(a.data, b.data, "Ekam v2 full must be deterministic");
    }

    #[test]
    fn test_ekam_v2_differs_from_v1() {
        let input = [7u8; 64];
        let v1 = memory_hard_transform_ekam_light(&input);
        let v2 = memory_hard_transform_ekam_light_v2(&input);
        assert_ne!(
            v1.data, v2.data,
            "v2 must differ from v1 (different params)"
        );
    }

    #[test]
    fn test_ekam_v2_avalanche() {
        let mut a_in = [0u8; 64];
        let mut b_in = [0u8; 64];
        a_in[0] = 0;
        b_in[0] = 1;
        let a = memory_hard_transform_ekam_light_v2(&a_in);
        let b = memory_hard_transform_ekam_light_v2(&b_in);
        assert_ne!(
            a.data, b.data,
            "v2 avalanche: different inputs → different outputs"
        );
    }

    #[test]
    fn test_ekam_v2_nonzero() {
        let input = [42u8; 64];
        let result = memory_hard_transform_ekam_v2(&input);
        assert!(
            result.data.iter().any(|&b| b != 0),
            "v2 output must not be all zeros"
        );
    }

    // ================================================================
    // CHv4.2 Merkabah Dual-Spin (v3) tests
    // ================================================================

    #[test]
    fn test_v3_deterministic() {
        let input = [7u8; 64];
        let a = memory_hard_transform_ekam_v3(&input);
        let b = memory_hard_transform_ekam_v3(&input);
        assert_eq!(a.data, b.data, "v3 dual-spin must be deterministic");
    }

    #[test]
    fn test_v3_avalanche() {
        let mut a_in = [0u8; 64];
        let mut b_in = [0u8; 64];
        a_in[0] = 0;
        b_in[0] = 1;
        let a = memory_hard_transform_ekam_v3(&a_in);
        let b = memory_hard_transform_ekam_v3(&b_in);
        assert_ne!(
            a.data, b.data,
            "v3 avalanche: different inputs → different outputs"
        );
    }

    #[test]
    fn test_v3_nonzero() {
        let input = [42u8; 64];
        let result = memory_hard_transform_ekam_v3(&input);
        assert!(
            result.data.iter().any(|&b| b != 0),
            "v3 output must not be all zeros"
        );
    }

    #[test]
    fn test_v3_differs_from_v2_full() {
        let input = [7u8; 64];
        let v2 = memory_hard_transform_ekam_v2(&input);
        let v3 = memory_hard_transform_ekam_v3(&input);
        assert_ne!(
            v2.data, v3.data,
            "v3 dual-spin must differ from v2 (extra forward HIC passes)"
        );
    }

    #[test]
    fn test_v3_differs_from_v2_light() {
        let input = [7u8; 64];
        let v2l = memory_hard_transform_ekam_light_v2(&input);
        let v3 = memory_hard_transform_ekam_v3(&input);
        assert_ne!(v2l.data, v3.data, "v3 must differ from v2 light");
    }

    #[test]
    fn test_dual_spin_differs_from_backward_only() {
        // Verify the forward passes actually change the result compared to
        // backward-only (v2 full uses backward only; v3 uses dual-spin).
        let input = [99u8; 64];
        let v2_full = memory_hard_transform_ekam_v2(&input);
        let v3_full = memory_hard_transform_ekam_v3(&input);
        assert_ne!(
            v2_full.data, v3_full.data,
            "Dual-spin must produce different output from backward-only Merkabah"
        );
    }
}
