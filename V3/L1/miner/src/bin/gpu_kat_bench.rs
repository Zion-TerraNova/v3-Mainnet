//! GPU KAT benchmark — DeekshaLite v1 & Fire (full OpenCL pipeline, CPU-independent)
//!
//! Validates V3 OpenCL kernels against locked known-answer test (KAT) vectors.
//! No CPU reference is computed at runtime; correctness is verified against
//! pre-generated constants that lock the exact hash output.
//!
//! Also runs a throughput benchmark and reports H/s per algorithm.
//!
//! Usage:
//!   cargo run --release --manifest-path V3/Cargo.toml \
//!       -p zion-miner --bin gpu_kat_bench --features gpu-opencl

use ocl::{Buffer, MemFlags, ProQue};
use std::time::Instant;

use zion_cosmic_harmony::deeksha_lite::{LITE_KAT, LITE_KAT_HEADER};
use zion_cosmic_harmony::deeksha_lite_fire::{FIRE_KAT, FIRE_KAT_HEADER};
use zion_cosmic_harmony::gpu::opencl_kernel;

const SCRATCHPAD_BYTES: usize = 256 * 1024; // 256 KiB per thread

// ── Helpers ────────────────────────────────────────────────────────────────

fn hex_to_bytes(hex: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        if i >= 32 {
            break;
        }
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16).unwrap();
    }
    out
}

/// Build an 80-byte mining-style header from an arbitrary prefix.
/// The CPU reference pads shorter headers with zeros; we replicate that
/// by placing the prefix at the start of an 80-byte zero buffer.
fn build_80_byte_header(prefix: &[u8]) -> [u8; 80] {
    let mut h = [0u8; 80];
    let n = prefix.len().min(80);
    h[..n].copy_from_slice(&prefix[..n]);
    h
}

/// Precompute Keccak-256 state after absorbing an 80-byte header.
/// The state is 25 u64s (200 bytes). Each thread then only XORs the
/// nonce into bytes 80..87, applies padding, and runs f1600.
fn precompute_header_keccak_state(header_80: &[u8; 80]) -> [u64; 25] {
    let mut state = [0u64; 25];
    for (i, &b) in header_80.iter().enumerate() {
        let word_idx = i / 8;
        let shift = (i % 8) * 8;
        state[word_idx] ^= (b as u64) << shift;
    }
    state
}

// ── GPU KAT runner ─────────────────────────────────────────────────────────

struct KatResult {
    algo_name: String,
    all_pass: bool,
    mismatches: Vec<String>,
    gpu_throughput_hps: f64,
}

fn run_gpu_kat(
    algo_name: &str,
    kernel_src: &str,
    kernel_fn: &str,
    kat_header: &[u8],
    kat_vectors: &[(&str, u64)],
) -> KatResult {
    println!("\n========================================");
    println!("  GPU KAT: {}", algo_name);
    println!("========================================");

    // ── Build OpenCL program ──
    let pro_que = match ProQue::builder().src(kernel_src).dims(1).build() {
        Ok(pq) => pq,
        Err(e) => {
            println!("  OpenCL build FAILED: {}", e);
            return KatResult {
                algo_name: algo_name.to_string(),
                all_pass: false,
                mismatches: vec![format!("OpenCL build failed: {}", e)],
                gpu_throughput_hps: 0.0,
            };
        }
    };
    let device_name = pro_que
        .device()
        .name()
        .unwrap_or_else(|_| "unknown".to_string());
    println!("  Device: {}", device_name);

    // ── Prepare 80-byte header + precomputed Keccak state ──
    let header_80 = build_80_byte_header(kat_header);
    let keccak_state = precompute_header_keccak_state(&header_80);

    let state_buf: Buffer<u64> = Buffer::builder()
        .queue(pro_que.queue().clone())
        .flags(MemFlags::READ_ONLY)
        .len(25)
        .copy_host_slice(&keccak_state)
        .build()
        .unwrap();

    // ── KAT verification: one nonce per enqueue (KAT nonces are sparse) ──
    println!("\n  KAT verification ({} vectors):", kat_vectors.len());
    let mut all_pass = true;
    let mut mismatches = Vec::new();

    for &(expected_hex, nonce) in kat_vectors {
        let output_buf: Buffer<u8> = Buffer::builder()
            .queue(pro_que.queue().clone())
            .len(32)
            .build()
            .unwrap();

        let scratch_buf: Buffer<u8> = Buffer::builder()
            .queue(pro_que.queue().clone())
            .len(SCRATCHPAD_BYTES)
            .build()
            .unwrap();

        let kernel = pro_que
            .kernel_builder(kernel_fn)
            .arg(&state_buf)
            .arg(nonce)
            .arg(1u32)
            .arg(&output_buf)
            .arg(&scratch_buf)
            .build()
            .unwrap();

        unsafe {
            kernel.enq().unwrap();
        }
        pro_que.queue().finish().unwrap();

        let mut hash = vec![0u8; 32];
        output_buf.read(&mut hash).enq().unwrap();
        let hash: [u8; 32] = hash.try_into().unwrap();

        let expected = hex_to_bytes(expected_hex);
        if hash == expected {
            println!("    nonce={:>20}: PASS  {}", nonce, &expected_hex[..16]);
        } else {
            all_pass = false;
            let got_hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
            println!("    nonce={:>20}: FAIL", nonce);
            println!("      expected: {}", expected_hex);
            println!("      got:      {}", got_hex);
            mismatches.push(format!(
                "nonce={}: expected {} got {}",
                nonce, expected_hex, got_hex
            ));
        }
    }

    // ── Throughput benchmark ──
    println!("\n  Throughput benchmark:");
    let bench_count: u32 = 4096;

    let bench_output: Buffer<u8> = Buffer::builder()
        .queue(pro_que.queue().clone())
        .len((bench_count as usize) * 32)
        .build()
        .unwrap();

    let bench_scratch: Buffer<u8> = Buffer::builder()
        .queue(pro_que.queue().clone())
        .len((bench_count as usize) * SCRATCHPAD_BYTES)
        .build()
        .unwrap();

    let bench_kernel = pro_que
        .kernel_builder(kernel_fn)
        .global_work_size(bench_count as usize)
        .arg(&state_buf)
        .arg(0u64)
        .arg(bench_count)
        .arg(&bench_output)
        .arg(&bench_scratch)
        .build()
        .unwrap();

    // Warm-up
    unsafe {
        bench_kernel.enq().unwrap();
    }
    pro_que.queue().finish().unwrap();

    let t0 = Instant::now();
    unsafe {
        bench_kernel.enq().unwrap();
    }
    pro_que.queue().finish().unwrap();
    let elapsed_ms = t0.elapsed().as_millis() as f64;
    let hps = (bench_count as f64) / (elapsed_ms / 1000.0);

    println!(
        "    {} nonces in {:.1} ms = {:.0} H/s",
        bench_count, elapsed_ms, hps
    );
    println!(
        "    Effective (80% pool overhead est): {:.0} H/s",
        hps * 0.8
    );

    KatResult {
        algo_name: algo_name.to_string(),
        all_pass,
        mismatches,
        gpu_throughput_hps: hps,
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    println!("========================================================");
    println!("  Zion V3 — GPU KAT Benchmark + Throughput Test");
    println!("  DeekshaLite v1 & DeekshaLite Fire (OpenCL)");
    println!("========================================================");
    println!();
    println!("  This binary validates GPU kernels against LOCKED KAT");
    println!("  vectors WITHOUT computing CPU reference at runtime.");
    println!("  If a KAT test fails, the GPU kernel diverges from the");
    println!("  canonical CPU pipeline and mainnet will freeze.");
    println!();

    let lite_result = run_gpu_kat(
        "DeekshaLite v1",
        opencl_kernel::DEEKSHA_LITE_KERNEL,
        opencl_kernel::DEEKSHA_LITE_KERNEL_NAME,
        LITE_KAT_HEADER,
        LITE_KAT,
    );

    let fire_result = run_gpu_kat(
        "DeekshaLite Fire",
        opencl_kernel::DEEKSHA_LITE_FIRE_KERNEL,
        opencl_kernel::DEEKSHA_LITE_FIRE_KERNEL_NAME,
        FIRE_KAT_HEADER,
        FIRE_KAT,
    );

    // ── Summary ──
    println!("\n========================================");
    println!("  SUMMARY");
    println!("========================================");
    println!();
    println!("  {:24} {:8} {:>12}", "Algorithm", "KAT", "GPU H/s");
    println!("  {:24} {:8} {:>12}", "-", "-", "-");
    println!(
        "  {:24} {:8} {:>12.0}",
        lite_result.algo_name,
        if lite_result.all_pass { "PASS" } else { "FAIL" },
        lite_result.gpu_throughput_hps
    );
    println!(
        "  {:24} {:8} {:>12.0}",
        fire_result.algo_name,
        if fire_result.all_pass { "PASS" } else { "FAIL" },
        fire_result.gpu_throughput_hps
    );
    println!();

    if !lite_result.all_pass || !fire_result.all_pass {
        println!("  RESULT: FAILED — GPU kernel diverges from KAT vectors!");
        for m in &lite_result.mismatches {
            println!("    [Lite] {}", m);
        }
        for m in &fire_result.mismatches {
            println!("    [Fire] {}", m);
        }
        std::process::exit(1);
    }

    println!("  RESULT: ALL KAT CHECKS PASSED — GPU pipeline is canonical.");
    println!();
}
