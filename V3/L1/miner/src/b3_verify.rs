//! Blake3 GPU kernel verification test.
//!
//! Simulates the GPU's Blake3 implementation in Rust and compares against the
//! blake3 crate to find divergence between GPU and CPU hash paths.
//!
//! Run: cargo run --bin b3-verify

#![allow(clippy::needless_range_loop)] // loops mirror the GPU Blake3 kernel layout

// ──── Blake3 constants ────────────────────────────────────────────────────
const IV: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];

const MSG_PERM: [usize; 16] = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];

const CHUNK_START: u32 = 1;
const CHUNK_END: u32 = 2;
const ROOT: u32 = 8;

// ──── GPU-equivalent Blake3 in Rust ───────────────────────────────────────

fn rotr32(x: u32, n: u32) -> u32 {
    x.rotate_right(n)
}

fn b3_g(st: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, mx: u32, my: u32) {
    st[a] = st[a].wrapping_add(st[b]).wrapping_add(mx);
    st[d] = rotr32(st[d] ^ st[a], 16);
    st[c] = st[c].wrapping_add(st[d]);
    st[b] = rotr32(st[b] ^ st[c], 12);
    st[a] = st[a].wrapping_add(st[b]).wrapping_add(my);
    st[d] = rotr32(st[d] ^ st[a], 8);
    st[c] = st[c].wrapping_add(st[d]);
    st[b] = rotr32(st[b] ^ st[c], 7);
}

fn b3_round(st: &mut [u32; 16], msg: &[u32; 16]) {
    b3_g(st, 0, 4, 8, 12, msg[0], msg[1]);
    b3_g(st, 1, 5, 9, 13, msg[2], msg[3]);
    b3_g(st, 2, 6, 10, 14, msg[4], msg[5]);
    b3_g(st, 3, 7, 11, 15, msg[6], msg[7]);
    b3_g(st, 0, 5, 10, 15, msg[8], msg[9]);
    b3_g(st, 1, 6, 11, 12, msg[10], msg[11]);
    b3_g(st, 2, 7, 8, 13, msg[12], msg[13]);
    b3_g(st, 3, 4, 9, 14, msg[14], msg[15]);
}

fn b3_permute(msg: &mut [u32; 16]) {
    let old = *msg;
    for i in 0..16 {
        msg[i] = old[MSG_PERM[i]];
    }
}

fn b3_compress(
    cv: &[u32; 8],
    bw: &[u32; 16],
    counter: u64,
    block_len: u32,
    flags: u32,
) -> [u32; 16] {
    let mut st: [u32; 16] = [
        cv[0],
        cv[1],
        cv[2],
        cv[3],
        cv[4],
        cv[5],
        cv[6],
        cv[7],
        IV[0],
        IV[1],
        IV[2],
        IV[3],
        (counter & 0xFFFFFFFF) as u32,
        (counter >> 32) as u32,
        block_len,
        flags,
    ];
    let mut msg = *bw;

    // 7 rounds, 6 permutations
    for _i in 0..6 {
        b3_round(&mut st, &msg);
        b3_permute(&mut msg);
    }
    b3_round(&mut st, &msg);

    // Feed-forward
    for i in 0..8 {
        st[i] ^= st[i + 8];
        st[i + 8] ^= cv[i];
    }
    st
}

fn b3_compress_cv(
    cv: &[u32; 8],
    bw: &[u32; 16],
    counter: u64,
    block_len: u32,
    flags: u32,
) -> [u32; 8] {
    let full = b3_compress(cv, bw, counter, block_len, flags);
    let mut out = [0u32; 8];
    out.copy_from_slice(&full[..8]);
    out
}

fn load_words(buf: &[u8]) -> [u32; 16] {
    let mut words = [0u32; 16];
    for i in 0..buf.len() {
        words[i / 4] |= (buf[i] as u32) << ((i % 4) * 8);
    }
    words
}

struct B3ChunkOut {
    input_cv: [u32; 8],
    block_words: [u32; 16],
    block_len: u32,
    flags: u32,
}

fn b3_hash_single_chunk(input: &[u8]) -> B3ChunkOut {
    let mut cv = IV;
    let mut offset = 0usize;
    let input_len = input.len();

    loop {
        let remaining = input_len - offset;
        let this_len = if remaining > 64 { 64 } else { remaining };
        let is_first = offset == 0;
        let is_last = offset + this_len >= input_len;
        let mut fl = 0u32;
        if is_first {
            fl |= CHUNK_START;
        }
        if is_last {
            fl |= CHUNK_END;
        }

        let bw = load_words(&input[offset..offset + this_len]);
        if is_last {
            return B3ChunkOut {
                input_cv: cv,
                block_words: bw,
                block_len: this_len as u32,
                flags: fl,
            };
        }
        cv = b3_compress_cv(&cv, &bw, 0, this_len as u32, fl);
        offset += this_len;
    }
}

fn b3_xof_squeeze(co: &B3ChunkOut, buf: &mut [u8]) {
    let buf_len = buf.len();
    let mut ob = 0u64;
    let mut written = 0usize;
    while written < buf_len {
        let st = b3_compress(
            &co.input_cv,
            &co.block_words,
            ob,
            co.block_len,
            co.flags | ROOT,
        );
        let to_write = std::cmp::min(64, buf_len - written);
        for i in 0..to_write {
            buf[written + i] = (st[i / 4] >> ((i % 4) * 8)) as u8;
        }
        written += to_write;
        ob += 1;
    }
}

// ──── Tests ──────────────────────────────────────────────────────────────

fn test_init_scratchpad() {
    println!("=== Test: init_scratchpad (87 bytes → 256 KiB XOF) ===\n");

    // Construct seed: SHA3-512 output (we use a simple known pattern)
    let seed = [0xABu8; 64];
    let domain = b"EKAM_SCRATCHPAD_INIT_V1";

    // --- CPU path: blake3 crate ---
    let mut hasher = blake3::Hasher::new();
    hasher.update(&seed);
    hasher.update(domain);
    let mut cpu_first_64 = [0u8; 64];
    hasher.finalize_xof().fill(&mut cpu_first_64);

    // --- GPU path: manual chunk processing ---
    let mut input = [0u8; 87];
    input[..64].copy_from_slice(&seed);
    input[64..87].copy_from_slice(domain);
    let co = b3_hash_single_chunk(&input);
    let mut gpu_first_64 = [0u8; 64];
    b3_xof_squeeze(&co, &mut gpu_first_64);

    println!("CPU first 16: {:02x?}", &cpu_first_64[..16]);
    println!("GPU first 16: {:02x?}", &gpu_first_64[..16]);

    if cpu_first_64 == gpu_first_64 {
        println!("✓ Init scratchpad: MATCH\n");
    } else {
        println!("✗ Init scratchpad: MISMATCH\n");
        // Find first diverging byte
        for i in 0..64 {
            if cpu_first_64[i] != gpu_first_64[i] {
                println!(
                    "  First divergence at byte {}: cpu=0x{:02x} gpu=0x{:02x}",
                    i, cpu_first_64[i], gpu_first_64[i]
                );
                break;
            }
        }
    }
}

fn test_mix_block_blake3() {
    println!("=== Test: mix_block Blake3 (208 bytes → 64 bytes XOF) ===\n");

    // Simulate: cur(64) || prev(64) || rand(64) || pass(8) || index(8)
    let cur = [0x11u8; 64];
    let prev = [0x22u8; 64];
    let rand = [0x33u8; 64];
    let pass: u64 = 0;
    let index: u64 = 42;

    // --- CPU path: blake3 crate ---
    let mut hasher = blake3::Hasher::new();
    hasher.update(&cur);
    hasher.update(&prev);
    hasher.update(&rand);
    hasher.update(&pass.to_le_bytes());
    hasher.update(&index.to_le_bytes());
    let mut cpu_out = [0u8; 64];
    hasher.finalize_xof().fill(&mut cpu_out);

    // --- GPU path: manual 4-block chunk ---
    let mut cv = IV;
    let bw0 = load_words(&cur);
    cv = b3_compress_cv(&cv, &bw0, 0, 64, CHUNK_START);
    let bw1 = load_words(&prev);
    cv = b3_compress_cv(&cv, &bw1, 0, 64, 0);
    let bw2 = load_words(&rand);
    cv = b3_compress_cv(&cv, &bw2, 0, 64, 0);

    // Block 3: pass(8) || index(8) = 16 bytes
    let mut bw3 = [0u32; 16];
    bw3[0] = (pass & 0xFFFFFFFF) as u32;
    bw3[1] = (pass >> 32) as u32;
    bw3[2] = (index & 0xFFFFFFFF) as u32;
    bw3[3] = (index >> 32) as u32;

    let co = B3ChunkOut {
        input_cv: cv,
        block_words: bw3,
        block_len: 16,
        flags: CHUNK_END,
    };
    let mut gpu_out = [0u8; 64];
    b3_xof_squeeze(&co, &mut gpu_out);

    println!("CPU first 16: {:02x?}", &cpu_out[..16]);
    println!("GPU first 16: {:02x?}", &gpu_out[..16]);

    if cpu_out == gpu_out {
        println!("✓ Mix block Blake3: MATCH\n");
    } else {
        println!("✗ Mix block Blake3: MISMATCH\n");
        for i in 0..64 {
            if cpu_out[i] != gpu_out[i] {
                println!(
                    "  First divergence at byte {}: cpu=0x{:02x} gpu=0x{:02x}",
                    i, cpu_out[i], gpu_out[i]
                );
                break;
            }
        }
    }
}

fn test_keccak256() {
    println!("=== Test: Keccak-256 (88 bytes) ===\n");

    // Simulate: header(80) || nonce(8) = 88 bytes
    let mut input = [0u8; 88];
    for i in 0..80 {
        input[i] = (i as u8).wrapping_mul(7);
    }
    input[80..88].copy_from_slice(&42u64.to_le_bytes());

    // CPU path: use the cosmic-harmony crate's keccak
    let hash = zion_cosmic_harmony::algorithms_opt::keccak256_opt(&input);
    println!("Keccak-256: {:02x?}", &hash.data[..16]);
    println!("(No GPU equivalent to compare without hardware — this is reference only)\n");
}

fn test_full_pipeline_cpu() {
    println!("=== Test: Full Deeksha v2 pipeline (CPU reference) ===\n");

    // Construct a dummy header + nonce + height
    let header = zion_core::MiningHeader {
        version: 3,
        previous_hash: [0xAA; 32],
        merkle_root: [0xBB; 32],
        timestamp: 1_762_000_200,
        difficulty_bits: 0x1f00ffff,
    };
    let nonce = 12345u64;
    let height = 2583u64;

    let candidate = zion_core::BlockCandidate {
        header,
        nonce,
        height,
    };
    let hash = candidate.hash();
    println!("Height {}: hash = {:02x?}", height, &hash[..16]);

    // Same with height 0 (epoch 0 — what GPU always uses)
    let candidate0 = zion_core::BlockCandidate {
        header,
        nonce,
        height: 0,
    };
    let hash0 = candidate0.hash();
    println!("Height 0 : hash = {:02x?}", &hash0[..16]);

    if hash == hash0 {
        println!("✓ Height doesn't affect hash (same epoch)\n");
    } else {
        println!("✗ Height DOES affect hash (different epochs)\n");
        println!("  → GPU uses epoch from height (update_epoch called), but if still wrong...");
    }

    // Check what epoch height 2583 maps to
    let epoch = zion_cosmic_harmony::algorithms_npu::epoch_from_height(height);
    let epoch0 = zion_cosmic_harmony::algorithms_npu::epoch_from_height(0);
    println!("  epoch(2583) = {}", epoch);
    println!("  epoch(0)    = {}", epoch0);
}

fn test_s4_memhard_exact() {
    println!("=== Test: s4_memhard exact match (self_test input) ===\n");

    // Exact same input as in gpu_backend.rs self_test
    use zion_cosmic_harmony::algorithms_opt::{golden_matrix_opt, keccak256_opt, sha3_512_opt};

    use zion_cosmic_harmony::scratchpad_ekam::memory_hard_transform_ekam_light_v2;

    let header = zion_core::MiningHeader {
        version: 3,
        previous_hash: [0xAA; 32],
        merkle_root: [0xBB; 32],
        timestamp: 1_762_000_200,
        difficulty_bits: 0x1f00ffff,
    };

    let header_bytes = header.to_bytes();
    let test_nonce = 42u64;
    let _test_height = 0u64;

    let mut input = [0u8; 88];
    input[..80].copy_from_slice(&header_bytes);
    input[80..88].copy_from_slice(&test_nonce.to_le_bytes());

    let cpu_s1 = keccak256_opt(&input);
    let cpu_s2 = sha3_512_opt(&cpu_s1.data);
    let cpu_s3 = golden_matrix_opt(&cpu_s2.data);
    let cpu_s4 = memory_hard_transform_ekam_light_v2(&cpu_s3.data);

    println!("s1_keccak256: {:02x?}", &cpu_s1.data[..16]);
    println!("s2_sha3_512:  {:02x?}", &cpu_s2.data[..16]);
    println!("s3_golden:    {:02x?}", &cpu_s3.data[..16]);
    println!("s4_memhard:   {:02x?}", &cpu_s4.data[..16]);
}

fn main() {
    println!("╔══════════════════════════════════════════════════╗");
    println!("║    Blake3 GPU ↔ CPU Verification Test            ║");
    println!("╚══════════════════════════════════════════════════╝\n");

    test_init_scratchpad();
    test_mix_block_blake3();
    test_keccak256();
    test_full_pipeline_cpu();
    test_s4_memhard_exact();
}
