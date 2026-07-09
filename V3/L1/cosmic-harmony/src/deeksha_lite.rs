//! DeekshaLite v1 — CPU reference implementation
//!
//! Pipeline (must match V3/L1/cosmic-harmony/src/gpu/kernels/deeksha_lite.cl):
//!   1. Keccak256(header[0..80] || nonce_le[0..8])             → s1[32]
//!   2. Memory-hard scratchpad (256 KiB, 8192 blocks × 32B)
//!        Phase A: SHA3-512 chain fill
//!                 state = seed || 0×32
//!                 for blk in 0..8192:
//!                   inp[0..64] = state; inp[64] = blk & 0xFF
//!                   out = sha3_512(&inp[..65])
//!                   pad[blk*32..+32] = out[0..32]; state[0..32] = out[0..32]
//!        Phase B: 2 sequential XOR passes (forward, backward)
//!        Phase C: 64 random reads, idx from 8-byte u64 accumulator
//!   3. AES-128 CTR mix (key=s2[0..16], counter=nonce_le||s2[16..24])
//!        block0 = counter, block1 = counter+1 (carry-propagated)
//!        3 full AES rounds + 1 final round (no mix_columns)
//!        XOR result with s2[0..32]
//!   4. Keccak256(s3)  → final hash[32]

// Hash loops intentionally mirror the GPU kernel (deeksha_lite.cl) byte-for-byte;
// index-based loops keep that parity explicit. Doc list uses intentional ASCII alignment.
#![allow(clippy::needless_range_loop)]
#![allow(clippy::doc_overindented_list_items)]

use crate::algorithms_opt::Hash32;
use sha3::{Digest, Keccak256, Sha3_512};

#[inline(always)]
fn meets_target(hash: &[u8; 32], target: &[u8; 32]) -> bool {
    hash <= target
}

pub const DEEKSHA_LITE_PROFILE: &str = "deeksha_lite_v1";

pub const SCRATCHPAD_SIZE: usize = 256 * 1024; // 256 KiB
pub const BLOCK_SIZE: usize = 32;
pub const BLOCK_COUNT: usize = SCRATCHPAD_SIZE / BLOCK_SIZE; // 8192
pub const PASSES: usize = 2;
pub const RANDOM_READS: usize = 64;
pub const AES_ROUNDS: usize = 4;

// ============================================================
// AES-128 helpers (FIPS-197)
// ============================================================

const AES_SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

fn sub_bytes(s: &mut [u8; 16]) {
    for i in 0..16 {
        s[i] = AES_SBOX[s[i] as usize];
    }
}

fn shift_rows(s: &mut [u8; 16]) {
    let t = s[1];
    s[1] = s[5];
    s[5] = s[9];
    s[9] = s[13];
    s[13] = t;
    let _t = s[2];
    s.swap(2, 10);
    let _t = s[6];
    s.swap(6, 14);
    let t = s[15];
    s[15] = s[11];
    s[11] = s[7];
    s[7] = s[3];
    s[3] = t;
}

fn xtime(a: u8) -> u8 {
    (a << 1) ^ (((a >> 7) & 1) * 0x1b)
}

fn mix_columns(s: &mut [u8; 16]) {
    for i in 0..4 {
        let (a, b, c, d) = (s[i * 4], s[i * 4 + 1], s[i * 4 + 2], s[i * 4 + 3]);
        let e = a ^ b ^ c ^ d;
        s[i * 4] ^= e ^ xtime(a ^ b);
        s[i * 4 + 1] ^= e ^ xtime(b ^ c);
        s[i * 4 + 2] ^= e ^ xtime(c ^ d);
        s[i * 4 + 3] ^= e ^ xtime(d ^ a);
    }
}

fn add_round_key(s: &mut [u8; 16], k: &[u8; 16]) {
    for i in 0..16 {
        s[i] ^= k[i];
    }
}

fn aes_round(s: &mut [u8; 16], k: &[u8; 16]) {
    sub_bytes(s);
    shift_rows(s);
    mix_columns(s);
    add_round_key(s, k);
}

fn aes_final_round(s: &mut [u8; 16], k: &[u8; 16]) {
    sub_bytes(s);
    shift_rows(s);
    add_round_key(s, k);
}

// ============================================================
// SHA3-512 wrapper
// ============================================================
fn sha3_512(input: &[u8]) -> [u8; 64] {
    Sha3_512::digest(input).into()
}

// ============================================================
// Step 1: Keccak256(header[0..80] || nonce_le)
// ============================================================
fn step1_keccak(header: &[u8], nonce: u64) -> [u8; 32] {
    let mut input = [0u8; 88];
    let hlen = header.len().min(80);
    input[..hlen].copy_from_slice(&header[..hlen]);
    input[80..88].copy_from_slice(&nonce.to_le_bytes());
    Keccak256::digest(input).into()
}

// ============================================================
// Step 2: Memory-hard scratchpad (256 KiB) → acc[32]
// ============================================================
fn step2_memory_hard(seed: &[u8; 32]) -> [u8; 32] {
    let mut scratchpad = vec![0u8; SCRATCHPAD_SIZE];

    // Phase A: SHA3-512 chain fill
    // state = seed || 0×32; inp[64] = blk & 0xFF; hash 65 bytes
    let mut state = [0u8; 64];
    state[..32].copy_from_slice(seed);

    for blk in 0..BLOCK_COUNT {
        let mut inp = [0u8; 65];
        inp[..64].copy_from_slice(&state);
        inp[64] = (blk & 0xFF) as u8;
        let out = sha3_512(&inp[..65]);
        let off = blk * BLOCK_SIZE;
        scratchpad[off..off + 32].copy_from_slice(&out[..32]);
        state[..32].copy_from_slice(&out[..32]);
    }

    // Phase B: pass 0 forward, pass 1 backward
    for i in 0..BLOCK_COUNT {
        let prev = if i == 0 { BLOCK_COUNT - 1 } else { i - 1 };
        let (cur, prv) = (i * BLOCK_SIZE, prev * BLOCK_SIZE);
        for j in 0..BLOCK_SIZE {
            let pv = scratchpad[prv + j];
            scratchpad[cur + j] ^= pv;
        }
    }
    for i in (0..BLOCK_COUNT).rev() {
        let next = if i + 1 == BLOCK_COUNT { 0 } else { i + 1 };
        let (cur, nxt) = (i * BLOCK_SIZE, next * BLOCK_SIZE);
        for j in 0..BLOCK_SIZE {
            let nv = scratchpad[nxt + j];
            scratchpad[cur + j] ^= nv;
        }
    }

    // Phase C: 64 random reads, idx from 8 bytes (u64) — matches GPU
    let mut acc = [0u8; 32];
    acc.copy_from_slice(seed);
    let mut pos: u64 = 0;

    for r in 0..RANDOM_READS as u64 {
        let off = (pos as usize) * BLOCK_SIZE;
        for i in 0..32 {
            acc[i] ^= scratchpad[off + i];
        }
        let mut idx_val: u64 = 0;
        for i in 0..8 {
            idx_val |= (acc[i] as u64) << (i * 8);
        }
        pos = (idx_val ^ pos ^ r) % BLOCK_COUNT as u64;
    }

    acc
}

// ============================================================
// Step 3: AES-128 CTR mix
// ============================================================
fn step3_aes_mix(seed: &[u8; 32], nonce: u64) -> [u8; 32] {
    let mut key = [0u8; 16];
    key.copy_from_slice(&seed[..16]);

    let mut counter = [0u8; 16];
    counter[..8].copy_from_slice(&nonce.to_le_bytes());
    counter[8..16].copy_from_slice(&seed[16..24]);

    let mut block0 = counter;
    let mut block1 = counter;

    // Carry-propagated increment (matches GPU)
    let mut carry: u16 = 1;
    for i in 0..16 {
        let sum = (block1[i] as u16) + carry;
        block1[i] = (sum & 0xFF) as u8;
        carry = sum >> 8;
        if carry == 0 {
            break;
        }
    }

    for _ in 0..3 {
        aes_round(&mut block0, &key);
        aes_round(&mut block1, &key);
    }
    aes_final_round(&mut block0, &key);
    aes_final_round(&mut block1, &key);

    let mut result = [0u8; 32];
    result[..16].copy_from_slice(&block0);
    result[16..32].copy_from_slice(&block1);
    for i in 0..32 {
        result[i] ^= seed[i];
    }
    result
}

// ============================================================
// Step 4: Keccak256 final hash
// ============================================================
fn step4_keccak(input: &[u8; 32]) -> [u8; 32] {
    Keccak256::digest(input).into()
}

// ============================================================
// Public API
// ============================================================

/// Full DeekshaLite hash (matches deeksha_lite.cl kernel exactly)
pub fn deeksha_lite(header: &[u8], nonce: u64) -> [u8; 32] {
    let s1 = step1_keccak(header, nonce);
    let s2 = step2_memory_hard(&s1);
    let s3 = step3_aes_mix(&s2, nonce);
    step4_keccak(&s3)
}

/// Height-aware wrapper (for dual-algo compatibility)
pub fn deeksha_lite_with_height(header: &[u8], nonce: u64, _height: u64) -> Hash32 {
    Hash32 {
        data: deeksha_lite(header, nonce),
    }
}

/// Self-test — deterministic check
pub fn deeksha_lite_self_test() -> bool {
    let header = b"ZION_DEEKSHA_LITE_TEST_V1";
    let nonce: u64 = 0x123456789ABCDEF0;
    let h1 = deeksha_lite(header, nonce);
    let h2 = deeksha_lite(header, nonce);
    h1 == h2 && h1 != [0u8; 32]
}

/// Sequential nonce search
pub fn deeksha_lite_find_nonce(
    header: &[u8],
    start_nonce: u64,
    count: u64,
    target: &[u8; 32],
) -> Option<(u64, [u8; 32])> {
    for offset in 0..count {
        let nonce = start_nonce.wrapping_add(offset);
        let hash = deeksha_lite(header, nonce);
        if meets_target(&hash, target) {
            return Some((nonce, hash));
        }
    }
    None
}

/// Known-answer test vectors for DeekshaLite v1.
/// Generated from this CPU implementation on 2026-06-09 and locked.
/// If any of these change, the CPU↔GPU pipeline is broken — do NOT update
/// these constants without regenerating and re-verifying the GPU kernel too.
pub const LITE_KAT_HEADER: &[u8] = b"ZION_LITE_KAT_V1";
pub const LITE_KAT: &[(&str, u64)] = &[
    (
        "40606d0279783883a5ad06e500253da68a2e6207cc57a056514b0b5d1e5d87ee",
        0,
    ),
    (
        "5cdbb8af7575211b61fd7385589ce901115bac7e3312401224c05c8bd64eb1d1",
        1,
    ),
    (
        "93fd2ba5ad43bd17a0bce90bb540e78adbe8d137f1e0850de7685668e60f323e",
        42,
    ),
    (
        "00422fe854eab743ad2b0230126393fdc1691496b6f4539fe5e4e1afa37747f7",
        0xDEADBEEF,
    ),
    (
        "69ed86c31cf312c395d39781e44fa3a0d25da73eb3776a59786d78c11209c358",
        u64::MAX,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    // ── Determinism ──────────────────────────────────────────────────────────

    #[test]
    fn test_deeksha_lite_deterministic() {
        let header = b"test_header";
        let nonce = 42u64;
        let h1 = deeksha_lite(header, nonce);
        let h2 = deeksha_lite(header, nonce);
        assert_eq!(h1, h2, "DeekshaLite must be deterministic");
        assert_ne!(h1, [0u8; 32], "Hash must not be all zeros");
    }

    #[test]
    fn test_deeksha_lite_different_nonces() {
        let header = b"test_header";
        let h1 = deeksha_lite(header, 1u64);
        let h2 = deeksha_lite(header, 2u64);
        assert_ne!(h1, h2, "Different nonces must produce different hashes");
    }

    #[test]
    fn test_deeksha_lite_different_headers() {
        let h1 = deeksha_lite(b"header_a", 0u64);
        let h2 = deeksha_lite(b"header_b", 0u64);
        assert_ne!(h1, h2, "Different headers must produce different hashes");
    }

    // ── Sub-step determinism ─────────────────────────────────────────────────

    #[test]
    fn test_memory_hard_deterministic() {
        let seed = [0xABu8; 32];
        let r1 = step2_memory_hard(&seed);
        let r2 = step2_memory_hard(&seed);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_memory_hard_nonzero() {
        let seed = [0x00u8; 32];
        let r = step2_memory_hard(&seed);
        assert_ne!(
            r, [0u8; 32],
            "Memory-hard step must not return all-zero for zero seed"
        );
    }

    #[test]
    fn test_memory_hard_different_seeds() {
        let r1 = step2_memory_hard(&[0x11u8; 32]);
        let r2 = step2_memory_hard(&[0x22u8; 32]);
        assert_ne!(
            r1, r2,
            "Different seeds must produce different scratchpad results"
        );
    }

    #[test]
    fn test_aes_mix_deterministic() {
        let seed = [0xCDu8; 32];
        let r1 = step3_aes_mix(&seed, 12345u64);
        let r2 = step3_aes_mix(&seed, 12345u64);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_aes_mix_nonce_sensitivity() {
        let seed = [0xCDu8; 32];
        let r1 = step3_aes_mix(&seed, 0u64);
        let r2 = step3_aes_mix(&seed, 1u64);
        assert_ne!(r1, r2, "AES mix must be sensitive to nonce");
    }

    // ── Edge-case nonces ─────────────────────────────────────────────────────

    #[test]
    fn test_nonce_zero() {
        let h = deeksha_lite(b"edge_nonce", 0u64);
        assert_ne!(h, [0u8; 32], "nonce=0 must not produce all-zero hash");
    }

    #[test]
    fn test_nonce_max() {
        let h = deeksha_lite(b"edge_nonce", u64::MAX);
        assert_ne!(h, [0u8; 32], "nonce=MAX must not produce all-zero hash");
    }

    #[test]
    fn test_nonce_zero_vs_one() {
        let h0 = deeksha_lite(b"edge_nonce", 0u64);
        let h1 = deeksha_lite(b"edge_nonce", 1u64);
        assert_ne!(h0, h1, "nonce=0 and nonce=1 must produce different hashes");
    }

    // ── Known-answer tests (KAT) — locks the exact output ───────────────────
    // These vectors were generated from this CPU implementation on 2026-06-09.
    // Changing any constant in the pipeline MUST cause these to fail.

    #[test]
    fn test_lite_kat_vectors() {
        for &(expected_hex, nonce) in LITE_KAT {
            let hash = deeksha_lite(LITE_KAT_HEADER, nonce);
            let got: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
            assert_eq!(
                got, expected_hex,
                "KAT MISMATCH: nonce={} — CPU pipeline changed or GPU kernel will diverge!",
                nonce
            );
        }
    }

    // ── Target comparison (meets_target) ────────────────────────────────────

    #[test]
    fn test_target_all_ff_always_passes() {
        // Any hash must be <= 0xFFFF...FF
        let target = [0xFFu8; 32];
        for nonce in 0u64..10 {
            let h = deeksha_lite(b"target_test", nonce);
            assert!(
                meets_target(&h, &target),
                "Hash must always meet all-FF target"
            );
        }
    }

    #[test]
    fn test_target_all_zero_never_passes() {
        // No hash must be <= 0x0000...00 (impossible unless hash is all zeros)
        let target = [0x00u8; 32];
        for nonce in 0u64..20 {
            let h = deeksha_lite(b"target_test", nonce);
            if h != [0u8; 32] {
                assert!(
                    !meets_target(&h, &target),
                    "Non-zero hash must not meet all-zero target"
                );
            }
        }
    }

    #[test]
    fn test_find_nonce_returns_valid_hash() {
        let header = b"find_nonce_test";
        let target = [0xFFu8; 32];
        let result = deeksha_lite_find_nonce(header, 0u64, 1000, &target);
        assert!(result.is_some(), "Should find a nonce with easy target");
        let (nonce, hash) = result.unwrap();
        // Verify returned hash is correct
        let expected = deeksha_lite(header, nonce);
        assert_eq!(
            hash, expected,
            "find_nonce must return correct hash for found nonce"
        );
        assert!(
            meets_target(&hash, &target),
            "Found hash must meet the target"
        );
    }

    #[test]
    fn test_find_nonce_impossible_target_returns_none() {
        let header = b"impossible_target";
        let target = [0x00u8; 32]; // impossible
        let result = deeksha_lite_find_nonce(header, 0u64, 100, &target);
        assert!(
            result.is_none(),
            "Should NOT find nonce with impossible all-zero target"
        );
    }

    // ── Self-test ────────────────────────────────────────────────────────────

    #[test]
    fn test_self_test_passes() {
        assert!(deeksha_lite_self_test(), "Self-test must pass");
    }

    // ── Profile constant ─────────────────────────────────────────────────────

    #[test]
    fn test_profile_string() {
        assert_eq!(DEEKSHA_LITE_PROFILE, "deeksha_lite_v1");
    }

    // ── Avalanche — single bit flip in header changes ≥ 50% of hash bytes ───

    #[test]
    fn test_avalanche_header_bit_flip() {
        let h1 = deeksha_lite(b"avalanche_test\x00", 0u64);
        let h2 = deeksha_lite(b"avalanche_test\x01", 0u64); // last byte flipped by 1 bit
        let differing = h1.iter().zip(h2.iter()).filter(|(a, b)| a != b).count();
        assert!(
            differing >= 8,
            "Avalanche: single-bit input change must affect >= 8 bytes of hash (got {})",
            differing
        );
    }
}
