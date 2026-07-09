//! DeekshaLite Fire — CPU reference implementation
//!
//! Fire = DeekshaLite v1 (identical, verified working) + thermal_loop step.
//! The thermal loop burns ALU cycles after the AES mix to maximize GPU heat.
//!
//! Pipeline (must match deeksha_lite_fire.cl exactly):
//!   1. Keccak256(header[0..80] || nonce_le)  → s1[32]  — same as v1
//!   2. Memory-hard scratchpad (256 KiB, 8192 × 32B, 2 passes, 64 reads)  — same as v1
//!   3. AES-128 CTR mix (3 full rounds + 1 final)  — same as v1
//!   4. Thermal loop (16384 iters, 8 ulong integer chains)  — extra vs v1
//!   5. Keccak256(s3_after_thermal)  → final hash[32]  — same as v1

// Hash loops intentionally mirror the GPU kernel (deeksha_lite_fire.cl) byte-for-byte.
#![allow(clippy::needless_range_loop)]

use crate::algorithms_opt::Hash32;
use sha3::{Digest, Keccak256, Sha3_512};

#[inline(always)]
fn meets_target(hash: &[u8; 32], target: &[u8; 32]) -> bool {
    hash <= target
}

pub const DEEKSHA_LITE_FIRE_PROFILE: &str = "deeksha_lite_fire";

// Constants — identical to deeksha_lite_v1 for memory management
pub const SCRATCHPAD_SIZE: usize = 256 * 1024; // 256 KiB — same as v1
pub const BLOCK_SIZE: usize = 32;
pub const BLOCK_COUNT: usize = SCRATCHPAD_SIZE / BLOCK_SIZE; // 8192
pub const PASSES: usize = 2; // same as v1
pub const RANDOM_READS: usize = 64; // same as v1
pub const AES_ROUNDS: usize = 4; // same as v1 (3 full + 1 final)
pub const THERMAL_ITERS: usize = 16384; // OPTIMIZED: reduced from 65536 (4x less) for better efficiency

// ============================================================
// AES-128 helpers — identical to deeksha_lite.rs
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

fn sha3_512(input: &[u8]) -> [u8; 64] {
    Sha3_512::digest(input).into()
}

// ============================================================
// Step 1: Keccak256 — identical to v1
// ============================================================
fn step1_keccak(header: &[u8], nonce: u64) -> [u8; 32] {
    let mut input = [0u8; 88];
    let hlen = header.len().min(80);
    input[..hlen].copy_from_slice(&header[..hlen]);
    input[80..88].copy_from_slice(&nonce.to_le_bytes());
    Keccak256::digest(input).into()
}

// ============================================================
// Step 2: Memory-hard scratchpad — identical to v1
// ============================================================
fn step2_memory_hard(seed: &[u8; 32]) -> [u8; 32] {
    let mut scratchpad = vec![0u8; SCRATCHPAD_SIZE];

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
// Step 3: AES-128 CTR mix — identical to v1
// ============================================================
fn step3_aes_mix(seed: &[u8; 32], nonce: u64) -> [u8; 32] {
    let mut key = [0u8; 16];
    key.copy_from_slice(&seed[..16]);

    let mut counter = [0u8; 16];
    counter[..8].copy_from_slice(&nonce.to_le_bytes());
    counter[8..16].copy_from_slice(&seed[16..24]);

    let mut block0 = counter;
    let mut block1 = counter;

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
// Step 4: Thermal loop — only addition over v1
//
// 8 independent ulong chains, 16384 iterations.
// Integer-only (no float) = deterministic on all GPU drivers.
// Identical logic in deeksha_lite_fire.cl thermal_loop().
// ============================================================
#[inline(never)]
fn step4_thermal_loop(data: &mut [u8; 32], nonce: u64) {
    let mut a = nonce ^ 0x9E3779B97F4A7C15u64;
    let mut b = nonce ^ 0xBF58476D1CE4E5B9u64;
    let mut c = nonce ^ 0x94D049BB133111EBu64;
    let mut d = nonce ^ 0x5851F42D4C957F2Du64;
    let mut e = nonce ^ 0xC0FFEE123456789Au64;
    let mut f = nonce ^ 0xDEADBEEFCAFEBABEu64;
    let mut g = nonce ^ 0xBADC0FFEE0DDF00Du64;
    let mut h = nonce ^ 0xFEEDFACECAFEBEEFu64;

    for i in 0..THERMAL_ITERS {
        a = a.rotate_left(17).wrapping_add(b);
        b = b.rotate_left(31) ^ a;
        c = c.rotate_left(13).wrapping_add(d);
        d = d.rotate_left(47) ^ c;
        e = e.rotate_left(23).wrapping_add(f);
        f = f.rotate_left(41) ^ e;
        g = g.rotate_left(11).wrapping_add(h);
        h = h.rotate_left(53) ^ g;
        a = a.wrapping_mul(0xFF51AFD7ED558CCDu64);
        b = b.wrapping_add(0xFF51AFD7ED558CCDu64);
        c = c.wrapping_mul(0x94D049BB133111EBu64);
        d = d.wrapping_add(0x5851F42D4C957F2Du64);
        e = e.wrapping_mul(0xC0FFEE123456789Au64);
        f = f.wrapping_add(0xDEADBEEFCAFEBABEu64);
        g = g.wrapping_mul(0xBADC0FFEE0DDF00Du64);
        h = h.wrapping_add(0xFEEDFACECAFEBEEFu64);
        a ^= data[i & 0x1F] as u64;
        b ^= data[(i + 8) & 0x1F] as u64;
        c ^= data[(i + 16) & 0x1F] as u64;
        d ^= data[(i + 24) & 0x1F] as u64;
        e ^= data[(i + 4) & 0x1F] as u64;
        f ^= data[(i + 12) & 0x1F] as u64;
        g ^= data[(i + 2) & 0x1F] as u64;
        h ^= data[(i + 6) & 0x1F] as u64;
    }
    // Fold back — prevents dead-code elimination
    data[0] ^= a as u8;
    data[1] ^= (a >> 8) as u8;
    data[2] ^= b as u8;
    data[3] ^= (b >> 8) as u8;
    data[4] ^= c as u8;
    data[5] ^= (c >> 8) as u8;
    data[6] ^= d as u8;
    data[7] ^= (d >> 8) as u8;
    data[8] ^= e as u8;
    data[9] ^= (e >> 8) as u8;
    data[10] ^= f as u8;
    data[11] ^= (f >> 8) as u8;
    data[12] ^= g as u8;
    data[13] ^= (g >> 8) as u8;
    data[14] ^= h as u8;
    data[15] ^= (h >> 8) as u8;
    data[16] ^= (a >> 16) as u8;
    data[17] ^= (b >> 16) as u8;
    data[18] ^= (c >> 16) as u8;
    data[19] ^= (d >> 16) as u8;
    data[20] ^= (e >> 16) as u8;
    data[21] ^= (f >> 16) as u8;
    data[22] ^= (g >> 16) as u8;
    data[23] ^= (h >> 16) as u8;
    data[24] ^= (a >> 24) as u8;
    data[25] ^= (b >> 24) as u8;
}

// ============================================================
// Step 5: Keccak256 final — identical to v1
// ============================================================
fn step5_keccak(input: &[u8; 32]) -> [u8; 32] {
    Keccak256::digest(input).into()
}

// ============================================================
// Public API
// ============================================================

/// Full DeekshaLite Fire hash (matches deeksha_lite_fire.cl exactly)
pub fn deeksha_lite_fire(header: &[u8], nonce: u64) -> [u8; 32] {
    let s1 = step1_keccak(header, nonce);
    let s2 = step2_memory_hard(&s1);
    let mut s3 = step3_aes_mix(&s2, nonce);
    step4_thermal_loop(&mut s3, nonce);
    step5_keccak(&s3)
}

/// Height-aware wrapper
pub fn deeksha_lite_fire_with_height(header: &[u8], nonce: u64, _height: u64) -> Hash32 {
    Hash32 {
        data: deeksha_lite_fire(header, nonce),
    }
}

/// Self-test
pub fn deeksha_lite_fire_self_test() -> bool {
    let header = b"ZION_FIRE_TEST_V1";
    let nonce: u64 = 0x123456789ABCDEF0;
    let h1 = deeksha_lite_fire(header, nonce);
    let h2 = deeksha_lite_fire(header, nonce);
    h1 == h2 && h1 != [0u8; 32]
}

/// Sequential nonce search
pub fn deeksha_lite_fire_find_nonce(
    header: &[u8],
    start_nonce: u64,
    count: u64,
    target: &[u8; 32],
) -> Option<(u64, [u8; 32])> {
    for offset in 0..count {
        let nonce = start_nonce.wrapping_add(offset);
        let hash = deeksha_lite_fire(header, nonce);
        if meets_target(&hash, target) {
            return Some((nonce, hash));
        }
    }
    None
}

/// Known-answer test vectors for DeekshaLite Fire.
/// Generated from this CPU implementation on 2026-06-17 (THERMAL_ITERS=16384).
/// If any of these change, the CPU↔GPU pipeline is broken — do NOT update
/// these constants without regenerating and re-verifying deeksha_lite_fire.cl too.
pub const FIRE_KAT_HEADER: &[u8] = b"ZION_FIRE_KAT_V1";
pub const FIRE_KAT: &[(&str, u64)] = &[
    (
        "c71feae825b4609f75a97fe70e01df3d5d7ca7b3d148188c9cc6259d5fbaf44d",
        0,
    ),
    (
        "ec48b909d8df962944cc2ffa3f745b8410c22800edc2c0937bbdb71f4444a1ae",
        1,
    ),
    (
        "e0e47d1df53aa6792fabf5fe6174f66eca619550bc546da66201b86c77d2e8e2",
        42,
    ),
    (
        "350f0a2ad5eead70a185c98276eddc925b7ed06c312febe3b82b897a688fddd2",
        0xDEADBEEF,
    ),
    (
        "3b820938f6988c11155526ab970621e094751b8cd4c0612151bfbfd741516629",
        u64::MAX,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    // ── Determinism ──────────────────────────────────────────────────────────

    #[test]
    fn test_fire_deterministic() {
        let header = b"test_header_fire";
        let nonce = 42u64;
        let h1 = deeksha_lite_fire(header, nonce);
        let h2 = deeksha_lite_fire(header, nonce);
        assert_eq!(h1, h2, "Fire must be deterministic");
        assert_ne!(h1, [0u8; 32], "Hash must not be all zeros");
    }

    #[test]
    fn test_fire_different_nonces() {
        let header = b"test_header_fire";
        let h1 = deeksha_lite_fire(header, 1u64);
        let h2 = deeksha_lite_fire(header, 2u64);
        assert_ne!(h1, h2, "Different nonces must produce different hashes");
    }

    #[test]
    fn test_fire_different_headers() {
        let h1 = deeksha_lite_fire(b"header_a", 0u64);
        let h2 = deeksha_lite_fire(b"header_b", 0u64);
        assert_ne!(
            h1, h2,
            "Different headers must produce different Fire hashes"
        );
    }

    // ── Cross-validation: Fire ≠ Lite v1 ────────────────────────────────────
    // Critical: ensures thermal loop actually changes the output.
    // If this fails, the thermal loop was dead-code-eliminated or has no effect.

    #[test]
    fn test_fire_different_from_v1() {
        use crate::deeksha_lite::deeksha_lite;
        let header = b"test_header_fire";
        let nonce = 42u64;
        let fire_hash = deeksha_lite_fire(header, nonce);
        let v1_hash = deeksha_lite(header, nonce);
        assert_ne!(
            fire_hash, v1_hash,
            "Fire hash must differ from Lite v1 — thermal loop must change output"
        );
    }

    #[test]
    fn test_fire_differs_from_v1_at_multiple_nonces() {
        use crate::deeksha_lite::deeksha_lite;
        let header = b"cross_validate";
        for nonce in [0u64, 1, 42, 100, 9999] {
            let fire = deeksha_lite_fire(header, nonce);
            let lite = deeksha_lite(header, nonce);
            assert_ne!(
                fire, lite,
                "Fire must differ from Lite v1 at nonce={}",
                nonce
            );
        }
    }

    // ── Thermal loop isolation ────────────────────────────────────────────────
    // step4_thermal_loop must actually modify the data, not be a no-op.

    #[test]
    fn test_thermal_loop_modifies_data() {
        let original = [0xABu8; 32];
        let mut data = original;
        step4_thermal_loop(&mut data, 42u64);
        assert_ne!(data, original, "Thermal loop must modify the input data");
    }

    #[test]
    fn test_thermal_loop_deterministic() {
        let mut d1 = [0x55u8; 32];
        let mut d2 = [0x55u8; 32];
        step4_thermal_loop(&mut d1, 12345u64);
        step4_thermal_loop(&mut d2, 12345u64);
        assert_eq!(d1, d2, "Thermal loop must be deterministic");
    }

    #[test]
    fn test_thermal_loop_nonce_sensitivity() {
        let mut d1 = [0x55u8; 32];
        let mut d2 = [0x55u8; 32];
        step4_thermal_loop(&mut d1, 0u64);
        step4_thermal_loop(&mut d2, 1u64);
        assert_ne!(d1, d2, "Thermal loop must be sensitive to nonce");
    }

    #[test]
    fn test_thermal_loop_nonzero_output() {
        let mut data = [0x00u8; 32];
        step4_thermal_loop(&mut data, 0u64);
        assert_ne!(
            data, [0x00u8; 32],
            "Thermal loop on zero input must not return all zeros"
        );
    }

    // ── Sub-step determinism ─────────────────────────────────────────────────

    #[test]
    fn test_memory_hard_deterministic() {
        let seed = [0xABu8; 32];
        let r1 = step2_memory_hard(&seed);
        let r2 = step2_memory_hard(&seed);
        assert_eq!(r1, r2);
    }

    // ── Edge-case nonces ─────────────────────────────────────────────────────

    #[test]
    fn test_fire_nonce_zero() {
        let h = deeksha_lite_fire(b"edge_nonce", 0u64);
        assert_ne!(h, [0u8; 32], "nonce=0 must not produce all-zero Fire hash");
    }

    #[test]
    fn test_fire_nonce_max() {
        let h = deeksha_lite_fire(b"edge_nonce", u64::MAX);
        assert_ne!(
            h, [0u8; 32],
            "nonce=MAX must not produce all-zero Fire hash"
        );
    }

    // ── Known-answer tests (KAT) — locks the exact output ───────────────────
    // These vectors were generated from this CPU implementation on 2026-06-09.
    // If these change: (1) GPU kernel deeksha_lite_fire.cl diverges, (2) chain freezes.

    #[test]
    fn test_fire_kat_vectors() {
        for &(expected_hex, nonce) in FIRE_KAT {
            let hash = deeksha_lite_fire(FIRE_KAT_HEADER, nonce);
            let got: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
            assert_eq!(
                got, expected_hex,
                "FIRE KAT MISMATCH: nonce={} — CPU pipeline changed or GPU kernel will diverge!",
                nonce
            );
        }
    }

    // ── Target comparison ────────────────────────────────────────────────────

    #[test]
    fn test_fire_target_all_ff_passes() {
        let target = [0xFFu8; 32];
        for nonce in 0u64..10 {
            let h = deeksha_lite_fire(b"fire_target", nonce);
            assert!(
                meets_target(&h, &target),
                "Fire hash must always meet all-FF target"
            );
        }
    }

    #[test]
    fn test_fire_find_nonce_returns_valid_hash() {
        let header = b"find_nonce_fire";
        let target = [0xFFu8; 32];
        let result = deeksha_lite_fire_find_nonce(header, 0u64, 500, &target);
        assert!(result.is_some(), "Should find a nonce with easy target");
        let (nonce, hash) = result.unwrap();
        let expected = deeksha_lite_fire(header, nonce);
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
    fn test_fire_find_nonce() {
        let header = b"find_nonce_fire";
        let target = [0xFFu8; 32];
        let result = deeksha_lite_fire_find_nonce(header, 0u64, 500, &target);
        assert!(result.is_some(), "Should find a nonce with easy target");
    }

    // ── Self-test ────────────────────────────────────────────────────────────

    #[test]
    fn test_fire_self_test_passes() {
        assert!(deeksha_lite_fire_self_test(), "Fire self-test must pass");
    }

    // ── Profile constant ─────────────────────────────────────────────────────

    #[test]
    fn test_fire_profile_string() {
        assert_eq!(DEEKSHA_LITE_FIRE_PROFILE, "deeksha_lite_fire");
    }

    // ── Avalanche ────────────────────────────────────────────────────────────

    #[test]
    fn test_fire_avalanche_header_bit_flip() {
        let h1 = deeksha_lite_fire(b"fire_avalanche\x00", 0u64);
        let h2 = deeksha_lite_fire(b"fire_avalanche\x01", 0u64);
        let differing = h1.iter().zip(h2.iter()).filter(|(a, b)| a != b).count();
        assert!(
            differing >= 8,
            "Fire avalanche: single-bit flip must affect >= 8 hash bytes (got {})",
            differing
        );
    }

    // ── Constants sanity ─────────────────────────────────────────────────────

    #[test]
    fn test_fire_scratchpad_equals_lite_v1() {
        // Fire scratchpad must be 256 KiB — same as Lite v1, NOT 128 KiB
        assert_eq!(
            SCRATCHPAD_SIZE,
            256 * 1024,
            "Fire scratchpad must be 256 KiB (same as Lite v1), got {} KiB",
            SCRATCHPAD_SIZE / 1024
        );
    }

    #[test]
    fn test_fire_thermal_iters_constant() {
        // 16384 is the optimized value matching deeksha_lite_fire.cl (reduced from 65536)
        assert_eq!(
            THERMAL_ITERS, 16384,
            "THERMAL_ITERS must be 16384 to match GPU kernel"
        );
    }
}
