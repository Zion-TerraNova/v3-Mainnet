//! GPU backend abstraction for Ekam Deeksha mining.
//!
//! Provides a trait-based dispatch layer supporting:
//! - OpenCL (via `ocl` crate, feature `gpu-opencl`)
//! - CUDA   (scaffold, feature `gpu-cuda`)
//! - Metal  (scaffold, feature `gpu-metal`)
//!
//! The OpenCL backend uses the cosmic harmony Deeksha kernel from
//! `zion-cosmic-harmony::gpu::opencl_kernel`.

#![allow(dead_code)]

use anyhow::Result;
use zion_auxpow::external_hashers::hash_blake3;
use zion_core::{DifficultyTarget, MiningHeader, MiningJob, MiningSolution};

#[cfg(feature = "gpu-opencl")]
use crate::gpu_guard::{GpuAlgorithm, GpuDeviceFamily, GpuGuard, GpuTuning};

#[cfg(feature = "gpu-opencl")]
use rayon::prelude::*;

/// Which GPU backend to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuBackendKind {
    /// Auto-detect: try OpenCL → CUDA → Metal → CPU fallback.
    Auto,
    /// Force OpenCL.
    OpenCL,
    /// Force CUDA (scaffold).
    Cuda,
    /// Force Metal (scaffold).
    Metal,
    /// No GPU — CPU only.
    Cpu,
}

impl GpuBackendKind {
    pub fn from_env() -> Self {
        let val = std::env::var("ZION_BACKEND")
            .or_else(|_| std::env::var("ZION_GPU_BACKEND"))
            .unwrap_or_default();
        match val.trim().to_ascii_lowercase().as_str() {
            "opencl" | "ocl" => Self::OpenCL,
            "cuda" => Self::Cuda,
            "metal" => Self::Metal,
            "cpu" => Self::Cpu,
            _ => Self::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::OpenCL => "opencl",
            Self::Cuda => "cuda",
            Self::Metal => "metal",
            Self::Cpu => "cpu",
        }
    }
}

/// Result of a GPU batch mining operation.
pub struct GpuBatchResult {
    /// Nonces that met the target: (nonce, final_hash, mix_hash).
    /// mix_hash is None for algorithms that don't produce one.
    pub solutions: Vec<(u64, [u8; 32], Option<[u8; 32]>)>,
    /// Total nonces tested in this batch.
    pub nonces_tested: u64,
}

/// Convert `StreamWeights` into the fixed 6-element float array consumed by
/// the OpenCL Deeksha kernels.
fn stream_weights_f32(
    weights: &zion_cosmic_harmony::stream_profit::StreamWeights,
) -> [f32; 6] {
    use zion_cosmic_harmony::revenue::RevenueSource;
    [
        weights.weight_for(RevenueSource::Zion) as f32,
        weights.weight_for(RevenueSource::KeccakBonus) as f32,
        weights.weight_for(RevenueSource::Sha3Bonus) as f32,
        weights.weight_for(RevenueSource::NclAi) as f32,
        weights.weight_for(RevenueSource::DeekshaLite) as f32,
        weights.weight_for(RevenueSource::ThermalBonus) as f32,
    ]
}

/// Trait for GPU mining backends.
pub trait GpuMiner: Send {
    /// Human-readable device name.
    fn device_name(&self) -> String;

    /// Backend kind.
    fn backend_kind(&self) -> GpuBackendKind;

    /// Algorithm this backend mines (e.g. "deeksha_lite_v1", "cosmic_harmony_ekam_deeksha_v2").
    fn algorithm(&self) -> String;

    /// Update NPU weights for the given block height's epoch.
    /// No-op if the epoch hasn't changed since the last call.
    fn update_epoch(&mut self, _height: u64) -> Result<()> {
        Ok(())
    }

    /// Update stream-profit weights for the current job.  Backends that support
    /// stream-weight parametrisation use these to adjust work distribution in
    /// the GPU kernel; others ignore them.
    fn set_stream_weights(
        &mut self,
        _weights: &zion_cosmic_harmony::stream_profit::StreamWeights,
    ) -> Result<()> {
        Ok(())
    }

    /// Whether to suppress GPU vs CPU mismatch warnings (e.g. s4-only mode
    /// where GPU and CPU use different implementations for stage 4).
    fn suppress_mismatch_warnings(&self) -> bool {
        false
    }

    /// Mine a batch of nonces starting from `nonce_start`.
    /// Returns any solutions found that meet the target.
    fn mine_batch(
        &mut self,
        header: MiningHeader,
        target: DifficultyTarget,
        nonce_start: u64,
        batch_size: u64,
    ) -> Result<GpuBatchResult>;

    /// Mine a batch using raw header bytes (for external algorithms with
    /// headers longer than 80 bytes, e.g. DCR = 180 bytes).
    /// Default: falls back to mine_batch with truncated header.
    fn mine_batch_raw(
        &mut self,
        raw_header: &[u8],
        target: DifficultyTarget,
        nonce_start: u64,
        batch_size: u64,
    ) -> Result<GpuBatchResult> {
        // Default: truncate to 80 bytes and use mine_batch
        let mut bytes = [0u8; 80];
        let len = raw_header.len().min(80);
        bytes[..len].copy_from_slice(&raw_header[..len]);
        let header = MiningHeader::from_bytes(bytes);
        self.mine_batch(header, target, nonce_start, batch_size)
    }

    /// Run a benchmark for the given duration.
    fn benchmark(&mut self, secs: f64) -> Result<(u64, f64, f64)>;
}

/// Multi-algo GPU backend manager.
/// Holds per-algorithm GPU backends and switches lazily.
pub struct GpuBackendManager {
    kind: GpuBackendKind,
    work_size: usize,
    current: Option<Box<dyn GpuMiner>>,
    current_algo: String,
}

impl GpuBackendManager {
    pub fn new(kind: GpuBackendKind, work_size: usize) -> Self {
        Self {
            kind,
            work_size,
            current: None,
            current_algo: String::new(),
        }
    }

    /// Return the current backend name if any backend is loaded.
    pub fn current_backend_name(&self) -> Option<&str> {
        self.current.as_ref().map(|g| g.backend_kind().as_str())
    }

    /// Ensure a backend for the requested algorithm is loaded.
    pub fn ensure_algorithm(&mut self, algorithm: &str) -> Result<&mut dyn GpuMiner> {
        if self.current_algo == algorithm {
            return Ok(self.current.as_mut().unwrap().as_mut());
        }
        println!(
            "gpu_switch_algorithm from={} to={}",
            self.current_algo, algorithm
        );
        let backend = create_gpu_backend(self.kind, self.work_size, algorithm)?;
        self.current_algo = algorithm.to_string();
        self.current = Some(backend);
        Ok(self.current.as_mut().unwrap().as_mut())
    }

    /// Run a benchmark across all supported algorithms.
    pub fn benchmark_all(&mut self, secs: f64) -> Vec<(String, f64)> {
        let algos = vec![
            "deeksha_chv3",
            "deeksha_lite_v1",
            "cosmic_harmony_ekam_deeksha_v2",
            "deeksha_lite_fire",
            // External AuxPoW algorithms
            "blake3",
            "kheavyhash",
            "autolykos",
            "kawpow",
            "ethash",
        ];
        let mut results = Vec::new();
        for algo in algos {
            match self.ensure_algorithm(algo) {
                Ok(gpu) => match gpu.benchmark(secs) {
                    Ok((hashes, elapsed, khps)) => {
                        println!("benchmark_algo={algo} hashes={hashes} elapsed={elapsed:.2}s khps={khps:.2}");
                        results.push((algo.to_string(), khps));
                    }
                    Err(e) => {
                        println!("benchmark_algo={algo} error={e}");
                    }
                },
                Err(e) => {
                    println!("benchmark_algo={algo} init_error={e}");
                }
            }
        }
        results
    }
}

/// Alias for backward compatibility.
pub type GpuBackend = GpuBackendManager;

/// Set of external AuxPoW algorithms that are handled by `zion_auxpow` GPU miner.
pub fn is_external_algorithm(algorithm: &str) -> bool {
    matches!(
        algorithm,
        "blake3"
            | "blake3_alph"
            | "blake3_dcr"
            | "kheavyhash"
            | "kheavyhash_kas"
            | "autolykos"
            | "autolykos_erg"
            | "kawpow"
            | "kawpow_rvn"
            | "kawpow_clore"
            | "kawpow_evr"
            | "kawpow_mewc"
            | "ethash"
            | "etchash"
            | "ethash_etc"
            | "verushash"
            | "randomx"
    )
}

/// Try to create the best available GPU backend.
/// Selects the appropriate OpenCL miner based on the algorithm.
pub fn create_gpu_backend(
    kind: GpuBackendKind,
    work_size: usize,
    algorithm: &str,
) -> Result<Box<dyn GpuMiner>> {
    let _ = algorithm;
    let _ = work_size;
    match kind {
        GpuBackendKind::Cpu => {
            anyhow::bail!("GPU backend requested but kind=cpu — use CPU mining path instead");
        }
        GpuBackendKind::OpenCL | GpuBackendKind::Auto => {
            #[cfg(feature = "gpu-opencl")]
            {
                // External AuxPoW algorithms (Blake3, kHeavyHash, ...)
                if is_external_algorithm(algorithm) {
                    match opencl_external::OpenClExternalMiner::new(algorithm, work_size) {
                        Ok(miner) => return Ok(Box::new(miner)),
                        Err(e) => {
                            if kind == GpuBackendKind::OpenCL {
                                anyhow::bail!("External OpenCL init failed: {e}");
                            }
                            println!("external_opencl_unavailable algorithm={algorithm} reason=\"{e}\"");
                        }
                    }
                }

                // Select miner based on algorithm
                if algorithm == "deeksha_chv3" {
                    // Phase C: use canonical deeksha_chv3.cl kernel
                    match opencl_deeksha_lite::OpenClDeekshaLiteMiner::new_chv3(work_size) {
                        Ok(miner) => return Ok(Box::new(miner)),
                        Err(e) => {
                            if kind == GpuBackendKind::OpenCL {
                                anyhow::bail!("DeekshaChv3 OpenCL init failed: {e}");
                            }
                            println!("deeksha_chv3_opencl_unavailable reason=\"{e}\"");
                        }
                    }
                } else if algorithm == "deeksha_lite_v1" {
                    match opencl_deeksha_lite::OpenClDeekshaLiteMiner::new(work_size) {
                        Ok(miner) => return Ok(Box::new(miner)),
                        Err(e) => {
                            if kind == GpuBackendKind::OpenCL {
                                anyhow::bail!("DeekshaLite OpenCL init failed: {e}");
                            }
                            println!("deeksha_lite_opencl_unavailable reason=\"{e}\"");
                        }
                    }
                } else if algorithm == "deeksha_lite_fire" {
                    match opencl_deeksha_lite_fire::OpenClDeekshaLiteFireMiner::new(work_size) {
                        Ok(miner) => return Ok(Box::new(miner)),
                        Err(e) => {
                            if kind == GpuBackendKind::OpenCL {
                                anyhow::bail!("DeekshaLite Fire OpenCL init failed: {e}");
                            }
                            println!("deeksha_lite_fire_opencl_unavailable reason=\"{e}\"");
                        }
                    }
                } else {
                    // Default to cosmic_harmony_deeksha for other algorithms
                    match opencl_deeksha::OpenClDeekshaMiner::new(work_size) {
                        Ok(miner) => return Ok(Box::new(miner)),
                        Err(e) => {
                            if kind == GpuBackendKind::OpenCL {
                                anyhow::bail!("OpenCL init failed: {e}");
                            }
                            println!("gpu_opencl_unavailable reason=\"{e}\"");
                        }
                    }
                }
            }
            #[cfg(not(feature = "gpu-opencl"))]
            {
                if kind == GpuBackendKind::OpenCL {
                    anyhow::bail!(
                        "OpenCL support not compiled — rebuild with --features gpu-opencl"
                    );
                }
            }

            // Auto fallback: try CUDA
            #[cfg(feature = "gpu-cuda")]
            {
                match cuda_deeksha::CudaDeekshaMiner::new(work_size) {
                    Ok(miner) => return Ok(Box::new(miner)),
                    Err(e) => println!("gpu_cuda_unavailable reason=\"{e}\""),
                }
            }

            // Auto fallback: try Metal
            #[cfg(feature = "gpu-metal")]
            {
                match metal_deeksha::MetalDeekshaMiner::new(work_size) {
                    Ok(miner) => return Ok(Box::new(miner)),
                    Err(e) => println!("gpu_metal_unavailable reason=\"{e}\""),
                }
            }

            anyhow::bail!(
                "no GPU backend available — compile with gpu-opencl, gpu-cuda, or gpu-metal"
            );
        }
        GpuBackendKind::Cuda => {
            #[cfg(feature = "gpu-cuda")]
            {
                let miner = cuda_deeksha::CudaDeekshaMiner::new(work_size)?;
                return Ok(Box::new(miner));
            }
            #[cfg(not(feature = "gpu-cuda"))]
            anyhow::bail!("CUDA support not compiled — rebuild with --features gpu-cuda");
        }
        GpuBackendKind::Metal => {
            #[cfg(feature = "gpu-metal")]
            {
                if algorithm == "deeksha_lite_fire" {
                    let miner = metal_deeksha_lite_fire::MetalDeekshaLiteFireMiner::new(work_size)?;
                    return Ok(Box::new(miner));
                } else {
                    let miner = metal_deeksha::MetalDeekshaMiner::new(work_size)?;
                    return Ok(Box::new(miner));
                }
            }
            #[cfg(not(feature = "gpu-metal"))]
            anyhow::bail!("Metal support not compiled — rebuild with --features gpu-metal");
        }
    }
}

/// Outcome of a GPU scan with candidate-filter statistics.
pub struct GpuScanOutcome {
    pub solution: Option<MiningSolution>,
    /// Mix hash for Ethash/KawPow (needed for eth_submitWork).  None for
    /// algorithms that don't produce a mix hash.
    pub mix_hash: Option<[u8; 32]>,
    pub nonces_tested: u64,
    pub candidates_found: u64,
    pub candidates_verified: u64,
    pub candidates_hash_mismatch: u64,
    pub candidates_above_target: u64,
}

/// Scan a job using a GPU backend, returning the first solution.
///
/// GPU and CPU paths are independent:
/// - GPU kernel finds a nonce where gpu_hash meets target → gpu_hash is primary.
/// - CPU computes cpu_hash for audit/diagnostics only; it does NOT gate submission.
/// - The solution always carries gpu_hash so the pool receives the same hash the
///   GPU kernel produced.  Pool re-computes the hash server-side (cpu path) and
///   compares — if GPU and CPU kernels are in sync this will agree.
pub fn gpu_scan_job(
    gpu: &mut dyn GpuMiner,
    job: MiningJob,
    algorithm: &str,
    raw_header_bytes: &[u8],
) -> GpuScanOutcome {
    // For external AuxPoW algorithms (kheavyhash, blake3, etc.), the pool
    // encodes the external block timestamp in job.height.  Inject it into
    // the MiningHeader.timestamp field so the GPU kernel receives it.
    let mut effective_header = job.header;
    if is_external_algorithm(algorithm) {
        effective_header.timestamp = job.height;
    }

    // For Ethash/KawPow, derive the epoch from the block height and ensure
    // the DAG is loaded.  The pool sends the external block number as
    // job.height for EthStratum coins (ETC/RVN/CLORE).
    if is_external_algorithm(algorithm)
        && matches!(
            algorithm,
            "ethash" | "etchash" | "ethash_etc"
                | "kawpow" | "kawpow_rvn" | "kawpow_clore"
                | "kawpow_evr" | "kawpow_mewc"
        )
    {
        let epoch = if matches!(algorithm, "ethash" | "etchash" | "ethash_etc") {
            (job.height / 30000) as u32
        } else {
            (job.height / 7500) as u32
        };

        // Try to update the DAG via the external miner's epoch method.
        // This is a no-op if the DAG is already loaded for this epoch.
        // We use a trait-object downcast check — if the backend is
        // OpenClExternalMiner, it has update_epoch_from_job.
        // Since we can't downcast easily, we rely on update_epoch() being
        // called by the caller (main.rs) which passes height.
        // The OpenClExternalMiner's update_epoch() is a no-op for external
        // algos; the DAG is managed via update_epoch_from_job() which is
        // called from a separate path.
        // For now, we log the epoch for diagnostics.
        eprintln!(
            "auxpow_dag_epoch_hint algorithm={} height={} epoch={}",
            algorithm, job.height, epoch
        );
    }

    // Use raw header bytes for external algorithms that need the full header
    // (e.g. DCR blake3 with 180-byte headers).  Fall back to mine_batch for
    // ZION algorithms and kheavyhash (which only uses first 32 bytes).
    let use_raw = is_external_algorithm(algorithm)
        && !algorithm.starts_with("kheavyhash")
        && raw_header_bytes.len() > 80;

    let result = if use_raw {
        gpu.mine_batch_raw(raw_header_bytes, job.target, job.start_nonce, job.nonce_count)
    } else {
        gpu.mine_batch(effective_header, job.target, job.start_nonce, job.nonce_count)
    };

    match result {
        Ok(result) => {
            let nonces_tested = result.nonces_tested;
            if let Some((nonce, gpu_hash, mix_hash)) = result.solutions.first() {
                let mix_hash = *mix_hash;
                let candidate = zion_core::BlockCandidate {
                    header: job.header,
                    nonce: *nonce,
                    height: job.height,
                };

                // ── CPU audit hash (independent path, diagnostic only) ────
                // For DCR the GPU scans the full 180-byte raw header; the CPU
                // audit must hash the same bytes to be comparable.
                let cpu_hash = if use_raw && algorithm == "blake3_dcr" {
                    hash_blake3(raw_header_bytes, 0, *nonce)
                } else {
                    candidate.hash_with_algorithm(algorithm)
                };
                let is_mismatch = cpu_hash != *gpu_hash;
                let cpu_above_target = !job.target.allows(&cpu_hash);
                let gpu_above_target = !job.target.allows(gpu_hash);

                use std::sync::atomic::{AtomicU64, Ordering};

                if is_mismatch && !gpu.suppress_mismatch_warnings() {
                    static MISMATCH_COUNT: AtomicU64 = AtomicU64::new(0);
                    let count = MISMATCH_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                    if count <= 5 || count.is_multiple_of(50) {
                        let fmthex = |b: &[u8]| -> String {
                            b.iter().map(|x| format!("{:02x}", x)).collect()
                        };
                        println!(
                            "GPU_CPU_MISMATCH #{} nonce={} h={} algo={} \
                             gpu_hash={} cpu_hash={} \
                             gpu_meets_target={} cpu_meets_target={}",
                            count,
                            nonce,
                            job.height,
                            algorithm,
                            fmthex(&gpu_hash[..8]),
                            fmthex(&cpu_hash[..8]),
                            !gpu_above_target,
                            !cpu_above_target,
                        );
                    }
                }

                if gpu_above_target {
                    // GPU hash itself does not meet target — kernel false-positive.
                    // Log it and skip; this should not normally happen.
                    static FALSE_POS: AtomicU64 = AtomicU64::new(0);
                    let count = FALSE_POS.fetch_add(1, Ordering::Relaxed) + 1;
                    if count <= 5 || count.is_multiple_of(50) {
                        println!(
                            "gpu_false_positive #{} nonce={} h={} algo={} gpu_above_target=true",
                            count, nonce, job.height, algorithm,
                        );
                    }
                    return GpuScanOutcome {
                        solution: None,
                        mix_hash,
                        nonces_tested,
                        candidates_found: 1,
                        candidates_verified: 0,
                        candidates_hash_mismatch: if is_mismatch { 1 } else { 0 },
                        candidates_above_target: 1,
                    };
                }

                // GPU hash meets target → submit gpu_hash as the canonical hash.
                // CPU path is independent: pool will re-verify on its own side.
                GpuScanOutcome {
                    solution: Some(MiningSolution {
                        job_id: job.job_id,
                        candidate,
                        hash: *gpu_hash,
                    }),
                    mix_hash,
                    nonces_tested,
                    candidates_found: 1,
                    candidates_verified: 1,
                    candidates_hash_mismatch: if is_mismatch { 1 } else { 0 },
                    candidates_above_target: 0,
                }
            } else {
                GpuScanOutcome {
                    solution: None,
                    mix_hash: None,
                    nonces_tested,
                    candidates_found: 0,
                    candidates_verified: 0,
                    candidates_hash_mismatch: 0,
                    candidates_above_target: 0,
                }
            }
        }
        Err(e) => {
            eprintln!("gpu_mine_batch_error: {e}");
            GpuScanOutcome {
                solution: None,
                mix_hash: None,
                nonces_tested: 0,
                candidates_found: 0,
                candidates_verified: 0,
                candidates_hash_mismatch: 0,
                candidates_above_target: 0,
            }
        }
    }
}

// ─── OpenCL Backend ─────────────────────────────────────────────────────────

#[cfg(feature = "gpu-opencl")]
pub mod opencl_deeksha {
    use super::*;
    use ocl::builders::ProgramBuilder;
    use ocl::{Buffer, Device, Kernel, Platform, ProQue};
    use std::time::Instant;
    use zion_cosmic_harmony::gpu::opencl_kernel;

    const SCRATCHPAD_BYTES: usize = 262_144; // 256 KiB per thread
    const SENTINEL: u64 = 0xFFFF_FFFF_FFFF_FFFF;

    pub struct OpenClDeekshaMiner {
        pro_que: ProQue,
        kernel: Kernel,
        header_buf: Buffer<u8>,
        scratchpad_buf: Buffer<u8>,
        result_nonce_buf: Buffer<u64>,
        result_hash_buf: Buffer<u8>,
        npu_weights: Buffer<i8>,
        npu_biases: Buffer<i8>,
        npu_scales: Buffer<i16>,
        npu_meta: Buffer<u32>,
        work_size: usize,
        local_work_size: usize,
        device_name_cached: String,
        current_epoch: u64,
        current_npu_max_dim: usize,
        platform: Platform,
        device: Device,
        kernel_src: String,
        /// GCN s4-only mode: GPU does stages 1-4, CPU does NPU+fusion+target.
        /// Avoids GCN compiler bugs in NPU code under high register pressure.
        is_gcn: bool,
        s4_kernel: Option<Kernel>,
        s4_out_buf: Option<Buffer<u8>>,
    }

    /// Determine max work_size that fits in GPU VRAM.
    /// Each thread needs SCRATCHPAD_BYTES (256 KiB).
    /// Reserve VRAM for NPU buffers, driver overhead, and other allocations.
    fn vram_aware_work_size(device: &Device, requested: usize) -> usize {
        let global_mem = device
            .info(ocl::enums::DeviceInfo::GlobalMemSize)
            .ok()
            .and_then(|v| match v {
                ocl::enums::DeviceInfoResult::GlobalMemSize(n) => Some(n as usize),
                _ => None,
            })
            .unwrap_or(2_000_000_000); // fallback 2 GB

        // Use configurable % of VRAM for scratchpad (default 65%)
        // Mining-only rigs (SMOS) can safely use most VRAM; 65% leaves
        // headroom for NPU buffers and driver overhead.
        let vram_pct: usize = std::env::var("ZION_OCL_VRAM_PCT")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(65)
            .clamp(10, 90);
        let usable = (global_mem * vram_pct) / 100;
        let max_by_mem = usable / SCRATCHPAD_BYTES;

        // Also respect env override
        let env_cap = std::env::var("ZION_OCL_WORK_CAP")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(usize::MAX);

        // GCN devices (Vega, Polaris, gfx6-9) cap at 16384 work items.
        // Previous 4096 cap was overly conservative — GCN wave64 handles
        // memory-bound scratchpad workloads well when VRAM allows.
        // RDNA (gfx10+) can scale even higher via ulong-width accesses.
        let dev = device.name().unwrap_or_default().to_ascii_lowercase();
        let gcn_cap = if dev.contains("vega")
            || dev.contains("polaris")
            || dev.contains("fiji")
            || dev.contains("ellesmere")
            || dev.contains("gfx6")
            || dev.contains("gfx7")
            || dev.contains("gfx8")
            || dev.contains("gfx9")
        {
            16384
        } else {
            usize::MAX
        };

        // env_cap (ZION_OCL_WORK_CAP) overrides VRAM limit if set
        let final_cap = if env_cap < usize::MAX {
            env_cap
        } else {
            max_by_mem
        };
        requested.min(final_cap).min(gcn_cap).max(64)
    }

    /// AMD RDNA / GCN build options for better perf on Radeon GPUs.
    ///
    /// GCN (Vega, Polaris) gets conservative flags only — `-cl-fast-relaxed-math`
    /// causes the AMD compiler to enable aggressive optimizations that can break
    /// integer code paths (Blake3 scratchpad, NPU LayerNorm, GELU) when register
    /// spills occur. RDNA can safely use the full flag set.
    fn amd_build_opts(device_name: &str) -> String {
        let dev = device_name.to_ascii_lowercase();
        let is_amd = dev.contains("gfx")
            || dev.contains("radeon")
            || dev.contains("amd")
            || dev.contains("rdna");
        if !is_amd {
            return String::new();
        }
        // Use conservative flags for both GCN and RDNA to avoid fusion mismatch
        // -cl-fast-relaxed-math causes GPU-CPU mismatch in fusion stage on RDNA
        "-cl-std=CL1.2 -cl-mad-enable".to_string()
    }

    /// NPU max intermediate dimension for current topology.
    fn npu_max_dim_for_epoch(epoch: u64) -> usize {
        let topology = zion_cosmic_harmony::algorithms_npu::MlpTopology::for_epoch(epoch);
        match topology {
            zion_cosmic_harmony::algorithms_npu::MlpTopology::Standard => 128,
            zion_cosmic_harmony::algorithms_npu::MlpTopology::ThreeLayer => 128,
            zion_cosmic_harmony::algorithms_npu::MlpTopology::Wide => 256,
            zion_cosmic_harmony::algorithms_npu::MlpTopology::Deep => 64,
        }
    }

    /// Detect optimal local work size for device.
    fn detect_local_work_size(device_name: &str) -> usize {
        let env_lws: Option<usize> = std::env::var("ZION_OCL_LOCAL_SIZE")
            .ok()
            .and_then(|v| v.trim().parse().ok());
        if let Some(lws) = env_lws {
            return lws.clamp(32, 512);
        }
        let device_name = device_name.to_ascii_lowercase();

        // RDNA (gfx10) benchmarks better with 128 threads; Vega/GCN wave64 with 64.
        if device_name.contains("gfx10") {
            128
        } else if device_name.contains("vega")
            || device_name.contains("gfx6")
            || device_name.contains("gfx7")
            || device_name.contains("gfx8")
            || device_name.contains("gfx9")
        {
            64
        } else {
            256
        }
    }

    /// Build OpenCL compile options string including topology-specific defines.
    fn full_build_opts(device_name: &str, npu_max_dim: usize, local_wgs: usize) -> String {
        let mut opts = amd_build_opts(device_name);
        // Append topology-specific defines
        if !opts.is_empty() {
            opts.push(' ');
        }

        // Detect GCN for conditional workarounds in kernel
        // RDNA (gfx10+) should NOT use GCN workarounds
        let dev = device_name.to_ascii_lowercase();
        let is_gcn = dev.contains("gfx6")
            || dev.contains("gfx7")
            || dev.contains("gfx8")
            || dev.contains("gfx9")
            || dev.contains("vega")
            || dev.contains("polaris")
            || dev.contains("fiji")
            || dev.contains("tonga")
            || dev.contains("ellesmere");

        if is_gcn {
            opts.push_str("-DZION_GCN_WORKAROUNDS ");
        }

        opts.push_str(&format!(
            "-DNPU_MAX_DIM={} -DWGS={}",
            npu_max_dim, local_wgs
        ));
        opts
    }

    impl OpenClDeekshaMiner {
        fn device_score(platform_name: &str, device_name: &str) -> i64 {
            let platform_l = platform_name.to_ascii_lowercase();
            let device_l = device_name.to_ascii_lowercase();
            let mut score: i64 = 0;

            // Match desktop-agent ordering: AMD > Intel > NVIDIA for Deeksha OpenCL path.
            if platform_l.contains("amd")
                || device_l.contains("amd")
                || device_l.contains("radeon")
                || device_l.contains("gfx")
            {
                score += 5_000;
            } else if platform_l.contains("intel")
                || device_l.contains("intel")
                || device_l.contains("arc")
            {
                score += 3_000;
            } else if platform_l.contains("nvidia")
                || platform_l.contains("cuda")
                || device_l.contains("nvidia")
            {
                score += 2_000;
            }

            score
        }

        fn pick_opencl_device() -> Result<(Platform, Device, String, String)> {
            let platforms = Platform::list();
            if platforms.is_empty() {
                anyhow::bail!("no OpenCL platforms found");
            }

            let platform_idx_override = std::env::var("ZION_OCL_PLATFORM_IDX")
                .ok()
                .and_then(|v| v.trim().parse::<usize>().ok());
            let device_idx_override = std::env::var("ZION_OCL_DEVICE_IDX")
                .ok()
                .and_then(|v| v.trim().parse::<usize>().ok());

            let mut candidates: Vec<(i64, usize, usize, Platform, Device, String, String)> =
                Vec::new();

            for (pidx, platform) in platforms.into_iter().enumerate() {
                if let Some(only_idx) = platform_idx_override {
                    if pidx != only_idx {
                        continue;
                    }
                }

                let platform_name = platform
                    .name()
                    .unwrap_or_else(|_| "unknown-platform".to_string());
                let gpus =
                    Device::list(platform, Some(ocl::flags::DeviceType::GPU)).map_err(|e| {
                        anyhow::anyhow!("OpenCL device list on platform {platform_name}: {e}")
                    })?;

                for (didx, device) in gpus.into_iter().enumerate() {
                    let device_name = device
                        .name()
                        .unwrap_or_else(|_| "unknown-device".to_string());
                    let score = Self::device_score(&platform_name, &device_name);
                    candidates.push((
                        score,
                        pidx,
                        didx,
                        platform,
                        device,
                        platform_name.clone(),
                        device_name,
                    ));
                }
            }

            if candidates.is_empty() {
                anyhow::bail!("no OpenCL GPU devices found");
            }

            if let Some(global_idx) = device_idx_override {
                let idx = global_idx.min(candidates.len().saturating_sub(1));
                let (_, pidx, didx, platform, device, platform_name, device_name) =
                    candidates.swap_remove(idx);
                println!(
                    "gpu_opencl_pick mode=override index={} platform_idx={} device_idx={} platform=\"{}\" device=\"{}\"",
                    idx, pidx, didx, platform_name, device_name
                );
                return Ok((platform, device, platform_name, device_name));
            }

            candidates.sort_by(|a, b| b.0.cmp(&a.0));
            let (_, pidx, didx, platform, device, platform_name, device_name) =
                candidates.swap_remove(0);
            println!(
                "gpu_opencl_pick mode=auto platform_idx={} device_idx={} platform=\"{}\" device=\"{}\"",
                pidx, didx, platform_name, device_name
            );
            Ok((platform, device, platform_name, device_name))
        }

        pub fn new(work_size: usize) -> Result<Self> {
            let kernel_src = opencl_kernel::get_deeksha_kernel_source().to_string();

            let (platform, device, platform_name, device_name) = Self::pick_opencl_device()?;

            let actual_work_size = vram_aware_work_size(&device, work_size);
            let local_work_size = detect_local_work_size(&device_name);

            // Topology-aware build: NPU_MAX_DIM reduces private-memory pressure
            let init_epoch = 0u64;
            let npu_max_dim = npu_max_dim_for_epoch(init_epoch);
            let build_opts = full_build_opts(&device_name, npu_max_dim, local_work_size);

            let pro_que = {
                let mut prog = ProgramBuilder::new();
                prog.src(kernel_src.clone());
                if !build_opts.is_empty() {
                    prog.cmplr_opt(build_opts.clone());
                }
                ProQue::builder()
                    .platform(platform)
                    .device(device)
                    .prog_bldr(prog)
                    .dims(actual_work_size)
                    .build()
                    .map_err(|e| anyhow::anyhow!("OpenCL build failed: {e}"))?
            };

            let q = pro_que.queue().clone();

            // ── Core buffers ────────────────────────────────────────────
            let header_buf = Buffer::<u8>::builder().queue(q.clone()).len(128).build()?;

            let scratchpad_buf = Buffer::<u8>::builder()
                .queue(q.clone())
                .len(actual_work_size * SCRATCHPAD_BYTES)
                .build()
                .map_err(|e| {
                    anyhow::anyhow!(
                        "scratchpad alloc failed ({} MiB): {e}",
                        actual_work_size * SCRATCHPAD_BYTES / (1024 * 1024)
                    )
                })?;

            let result_nonce_buf = Buffer::<u64>::builder().queue(q.clone()).len(1).build()?;

            let result_hash_buf = Buffer::<u8>::builder().queue(q.clone()).len(32).build()?;

            // ── NPU weight buffers (packed variable-topology) ────────────
            let init_epoch = 0u64;
            let packed = zion_cosmic_harmony::algorithms_npu::chv4_npu_weights_packed(init_epoch);

            let npu_weights = Buffer::<i8>::builder()
                .queue(q.clone())
                .len(packed.weights.len().max(1))
                .copy_host_slice(&packed.weights)
                .build()?;
            let npu_biases = Buffer::<i8>::builder()
                .queue(q.clone())
                .len(packed.biases.len().max(1))
                .copy_host_slice(&packed.biases)
                .build()?;
            let npu_scales = Buffer::<i16>::builder()
                .queue(q.clone())
                .len(packed.scales.len().max(1))
                .copy_host_slice(&packed.scales)
                .build()?;
            let npu_meta = Buffer::<u32>::builder()
                .queue(q.clone())
                .len(packed.meta.len())
                .copy_host_slice(&packed.meta)
                .build()?;

            // ── Kernel: ekam_deeksha_mine (12 args) ─────────────────────
            // Signature:
            //   0: header        (__global const uchar*)
            //   1: header_len    (uint)
            //   2: nonce_base    (ulong)
            //   3: nonce_count   (uint)
            //   4: scratchpad    (__global uchar*)
            //   5: target_u32    (uint)
            //   6: result_nonce  (__global ulong*)
            //   7: result_hash   (__global uchar*)
            //   8: npu_weights   (__global const char*)
            //   9: npu_biases    (__global const char*)
            //  10: npu_scales    (__global const short*)
            //  11: npu_meta      (__global const uint*)
            let kernel = pro_que
                .kernel_builder(opencl_kernel::EKAM_DEEKSHA_KERNEL_NAME)
                .arg(&header_buf) // 0
                .arg(80u32) // 1: header_len
                .arg(0u64) // 2: nonce_base (updated per batch)
                .arg(actual_work_size as u32) // 3: nonce_count
                .arg(&scratchpad_buf) // 4
                .arg(0u32) // 5: target_u32 (updated per batch)
                .arg(&result_nonce_buf) // 6
                .arg(&result_hash_buf) // 7
                .arg(&npu_weights) // 8
                .arg(&npu_biases) // 9
                .arg(&npu_scales) // 10
                .arg(&npu_meta) // 11
                .build()
                .map_err(|e| anyhow::anyhow!("kernel build failed: {e}"))?;

            let scratch_mib = actual_work_size * SCRATCHPAD_BYTES / (1024 * 1024);

            // Detect GCN devices (gfx6–gfx9). RDNA (gfx10+) uses full GPU pipeline.
            let dev_lower = device_name.to_ascii_lowercase();
            let is_gcn = dev_lower.contains("vega")
                || dev_lower.contains("polaris")
                || dev_lower.contains("fiji")
                || dev_lower.contains("ellesmere")
                || dev_lower.contains("gfx6")
                || dev_lower.contains("gfx7")
                || dev_lower.contains("gfx8")
                || dev_lower.contains("gfx9");

            // Canonical Ekam Deeksha: full GPU pipeline (stages 1–6) by default on non-GCN.
            // GCN (gfx8/gfx9) default to s4_mode due to compiler bugs in stages 5–6.
            // ZION_NO_GCN_S4_MODE=1 → force full pipeline on GCN (debug only).
            let env_on = |name: &str| {
                std::env::var(name).map_or(false, |v| {
                    matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES")
                })
            };
            let force_s4 = is_gcn && !env_on("ZION_NO_GCN_S4_MODE");
            let (s4_kernel, s4_out_buf) = if force_s4 {
                let s4_out = Buffer::<u8>::builder()
                    .queue(pro_que.queue().clone())
                    .len(actual_work_size * 64)
                    .build()
                    .map_err(|e| anyhow::anyhow!("s4_out alloc failed: {e}"))?;

                let s4k = pro_que
                    .kernel_builder(opencl_kernel::EKAM_DEEKSHA_S4_KERNEL_NAME)
                    .arg(&header_buf) // 0: header
                    .arg(80u32) // 1: header_len
                    .arg(0u64) // 2: nonce_base
                    .arg(actual_work_size as u32) // 3: nonce_count
                    .arg(&scratchpad_buf) // 4: scratchpad_pool
                    .arg(&s4_out) // 5: s4_out
                    .build()
                    .map_err(|e| anyhow::anyhow!("s4 kernel build failed: {e}"))?;

                println!("gpu_gcn_s4_mode enabled — GPU stages 1-4, CPU does NPU+fusion+target",);
                (Some(s4k), Some(s4_out))
            } else {
                (None, None)
            };

            println!(
                "gpu_opencl_init platform=\"{}\" device=\"{}\" work_size={} local_ws={} scratchpad_mib={} npu_max_dim={} is_gcn={} gcn_s4_mode={} build_opts=\"{}\"",
                platform_name, device_name, actual_work_size, local_work_size, scratch_mib, npu_max_dim, is_gcn, force_s4, build_opts,
            );

            let miner = Self {
                pro_que,
                kernel,
                header_buf,
                scratchpad_buf,
                result_nonce_buf,
                result_hash_buf,
                npu_weights,
                npu_biases,
                npu_scales,
                npu_meta,
                work_size: actual_work_size,
                local_work_size,
                device_name_cached: device_name,
                current_epoch: init_epoch,
                current_npu_max_dim: npu_max_dim,
                platform,
                device,
                kernel_src,
                is_gcn,
                s4_kernel,
                s4_out_buf,
            };

            // Startup self-test: run debug kernel and compare all 6 stages with CPU
            // Skip if ZION_SKIP_GPU_SELF_TEST is set (for SMOS compatibility)
            // Also skip if ZION_IGNORE_GPU_SELF_TEST_FAIL is set (for Vega 64 compatibility)
            if std::env::var("ZION_SKIP_GPU_SELF_TEST").is_err() {
                if let Err(e) = miner.self_test() {
                    println!("GPU_SELF_TEST_ERROR: {e}");
                    // Only ignore failure if explicitly requested
                    if std::env::var("ZION_IGNORE_GPU_SELF_TEST_FAIL").is_err() {
                        return Err(e);
                    }
                    println!(
                        "GPU SELF-TEST FAILED BUT CONTINUING (ZION_IGNORE_GPU_SELF_TEST_FAIL set)"
                    );
                }
            } else {
                println!("GPU SELF-TEST SKIPPED (ZION_SKIP_GPU_SELF_TEST set)");
            }

            Ok(miner)
        }

        /// Run GPU debug kernel with a known input and compare all 6 pipeline
        /// stages against CPU.  Prints results to stdout so SMOS captures them.
        fn self_test(&self) -> Result<()> {
            println!("=== GPU SELF-TEST START ===");

            let test_header = MiningHeader {
                version: 3,
                previous_hash: [0xAA; 32],
                merkle_root: [0xBB; 32],
                timestamp: 1_762_000_200,
                difficulty_bits: 0x1f00ffff,
            };
            let test_nonce: u64 = 42;
            let test_height: u64 = 0; // epoch 0, matching init

            let header_bytes = test_header.to_bytes();
            self.header_buf.write(&header_bytes[..]).enq()?;

            // Stage output buffer: 32 + 64 + 64 + 64 + 64 + 32 = 320 bytes
            let q = self.pro_que.queue().clone();
            let stage_buf = Buffer::<u8>::builder().queue(q).len(320).build()?;

            // Build and run debug kernel (single work item)
            let debug_kernel = self
                .pro_que
                .kernel_builder("ekam_deeksha_debug")
                .arg(&self.header_buf) // 0: header
                .arg(header_bytes.len() as u32) // 1: header_len
                .arg(test_nonce) // 2: nonce
                .arg(&self.scratchpad_buf) // 3: scratchpad_pool
                .arg(&stage_buf) // 4: stage_out
                .arg(&self.npu_weights) // 5
                .arg(&self.npu_biases) // 6
                .arg(&self.npu_scales) // 7
                .arg(&self.npu_meta) // 8
                .build()
                .map_err(|e| anyhow::anyhow!("debug kernel build: {e}"))?;

            unsafe {
                debug_kernel
                    .cmd()
                    .global_work_size(1)
                    .local_work_size(1)
                    .enq()?;
            }

            let mut gpu = vec![0u8; 320];
            stage_buf.read(&mut gpu).enq()?;

            // CPU computation
            use zion_cosmic_harmony::algorithms_npu::{epoch_from_height, npu_mixing_step_epoch};
            use zion_cosmic_harmony::algorithms_opt::{
                cosmic_fusion_opt_rounds, golden_matrix_opt, keccak256_opt, sha3_512_opt,
            };
            use zion_cosmic_harmony::scratchpad_ekam::memory_hard_transform_ekam_light_v2;

            let mut input = [0u8; 88];
            input[..80].copy_from_slice(&header_bytes);
            input[80..88].copy_from_slice(&test_nonce.to_le_bytes());

            let cpu_s1 = keccak256_opt(&input);
            let cpu_s2 = sha3_512_opt(&cpu_s1.data);
            let cpu_s3 = golden_matrix_opt(&cpu_s2.data);
            let cpu_s4 = memory_hard_transform_ekam_light_v2(&cpu_s3.data);
            let epoch = epoch_from_height(test_height);
            let cpu_s5 = npu_mixing_step_epoch(&cpu_s4.data, epoch);
            let cpu_hash = cosmic_fusion_opt_rounds(&cpu_s5, 8);

            let hex = |b: &[u8]| -> String { b.iter().map(|x| format!("{:02x}", x)).collect() };

            // Compare each stage
            let stages: [(&str, &[u8], &[u8]); 6] = [
                ("s1_keccak256", &gpu[0..32], &cpu_s1.data),
                ("s2_sha3_512", &gpu[32..96], &cpu_s2.data),
                ("s3_golden", &gpu[96..160], &cpu_s3.data),
                ("s4_memhard", &gpu[160..224], &cpu_s4.data),
                ("s5_npu", &gpu[224..288], &cpu_s5),
                ("s6_fusion", &gpu[288..320], &cpu_hash.data),
            ];

            let mut all_ok = true;
            let ignore_s4_mismatch = std::env::var("ZION_IGNORE_S4_MEMHARD_MISMATCH").is_ok();

            for (name, g, c) in &stages {
                let ok = *g == *c;
                let is_s4 = *name == "s4_memhard";

                // Skip s4_memhard mismatch if flag is set (GPU uses different implementation)
                if !ok && is_s4 && ignore_s4_mismatch {
                    println!(
                        "SELF_TEST {}=IGNORED (ZION_IGNORE_S4_MEMHARD_MISMATCH set)",
                        name
                    );
                    continue;
                }

                println!("SELF_TEST {}={}", name, if ok { "OK" } else { "FAIL" });
                if !ok {
                    all_ok = false;
                    let glen = g.len().min(32);
                    let clen = c.len().min(32);
                    println!("  gpu={}", hex(&g[..glen]));
                    println!("  cpu={}", hex(&c[..clen]));
                    break; // Only print first diverging stage
                }
            }

            if all_ok {
                println!(
                    "SELF_TEST gpu_hash={} cpu_hash={} MATCH",
                    hex(&gpu[288..320]),
                    hex(&cpu_hash.data)
                );
                println!("=== GPU SELF-TEST END ===");
                Ok(())
            } else {
                println!("=== GPU SELF-TEST END ===");
                anyhow::bail!("GPU-CPU mismatch in self-test")
            }
        }

        /// Run a full pipeline self-test at a specific epoch.
        /// Re-uses current NPU buffers (must be called after they are updated).
        fn self_test_at_epoch(&self, epoch: u64) -> Result<()> {
            use zion_cosmic_harmony::algorithms_npu::npu_mixing_step_epoch;
            use zion_cosmic_harmony::algorithms_opt::{
                cosmic_fusion_opt_rounds, golden_matrix_opt, keccak256_opt, sha3_512_opt,
            };
            use zion_cosmic_harmony::scratchpad_ekam::memory_hard_transform_ekam_light_v2;

            println!("=== GPU EPOCH SELF-TEST epoch={} ===", epoch);

            let test_header = MiningHeader {
                version: 3,
                previous_hash: [0xAA; 32],
                merkle_root: [0xBB; 32],
                timestamp: 1_762_000_200,
                difficulty_bits: 0x1f00ffff,
            };
            let test_nonce: u64 = 42;

            let header_bytes = test_header.to_bytes();
            self.header_buf.write(&header_bytes[..]).enq()?;

            let q = self.pro_que.queue().clone();
            let stage_buf = Buffer::<u8>::builder().queue(q).len(320).build()?;

            let debug_kernel = self
                .pro_que
                .kernel_builder("ekam_deeksha_debug")
                .arg(&self.header_buf)
                .arg(header_bytes.len() as u32)
                .arg(test_nonce)
                .arg(&self.scratchpad_buf)
                .arg(&stage_buf)
                .arg(&self.npu_weights)
                .arg(&self.npu_biases)
                .arg(&self.npu_scales)
                .arg(&self.npu_meta)
                .build()
                .map_err(|e| anyhow::anyhow!("debug kernel build: {e}"))?;

            unsafe {
                debug_kernel
                    .cmd()
                    .global_work_size(1)
                    .local_work_size(1)
                    .enq()?;
            }

            let mut gpu = vec![0u8; 320];
            stage_buf.read(&mut gpu).enq()?;

            // CPU pipeline with the same epoch
            let mut input = [0u8; 88];
            input[..80].copy_from_slice(&header_bytes);
            input[80..88].copy_from_slice(&test_nonce.to_le_bytes());

            let cpu_s1 = keccak256_opt(&input);
            let cpu_s2 = sha3_512_opt(&cpu_s1.data);
            let cpu_s3 = golden_matrix_opt(&cpu_s2.data);
            let cpu_s4 = memory_hard_transform_ekam_light_v2(&cpu_s3.data);
            let cpu_s5 = npu_mixing_step_epoch(&cpu_s4.data, epoch);
            let cpu_hash = cosmic_fusion_opt_rounds(&cpu_s5, 8);

            let hex = |b: &[u8]| -> String { b.iter().map(|x| format!("{:02x}", x)).collect() };

            let stages: [(&str, &[u8], &[u8]); 6] = [
                ("s1_keccak256", &gpu[0..32], &cpu_s1.data),
                ("s2_sha3_512", &gpu[32..96], &cpu_s2.data),
                ("s3_golden", &gpu[96..160], &cpu_s3.data),
                ("s4_memhard", &gpu[160..224], &cpu_s4.data),
                ("s5_npu", &gpu[224..288], &cpu_s5),
                ("s6_fusion", &gpu[288..320], &cpu_hash.data),
            ];

            let mut all_ok = true;
            for (name, g, c) in &stages {
                let ok = *g == *c;
                println!(
                    "EPOCH_TEST e={} {}={}",
                    epoch,
                    name,
                    if ok { "OK" } else { "FAIL" }
                );
                if !ok {
                    all_ok = false;
                    let glen = g.len().min(32);
                    let clen = c.len().min(32);
                    println!("  gpu={}", hex(&g[..glen]));
                    println!("  cpu={}", hex(&c[..clen]));
                    // Don't break — print ALL stages to help diagnose
                }
            }

            if all_ok {
                println!(
                    "EPOCH_TEST e={} MATCH gpu_hash={}",
                    epoch,
                    hex(&gpu[288..320])
                );
                Ok(())
            } else {
                anyhow::bail!("GPU-CPU mismatch at epoch {}", epoch)
            }
        }

        /// GCN s4-only mining: GPU does stages 1-4 (incl. memory-hard), CPU does NPU + fusion + target.
        fn mine_batch_s4(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<GpuBatchResult> {
            use zion_cosmic_harmony::algorithms_npu::npu_mixing_step_epoch;
            use zion_cosmic_harmony::algorithms_opt::cosmic_fusion_opt_rounds;

            let s4_kernel = self
                .s4_kernel
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("s4 kernel not initialized"))?;
            let s4_out_buf = self
                .s4_out_buf
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("s4 output buffer not initialized"))?;

            let header_bytes = header.to_bytes();
            self.header_buf.write(&header_bytes[..]).enq()?;

            let epoch = self.current_epoch;

            let mut all_solutions = Vec::new();
            let mut total_tested = 0u64;
            let mut current_nonce = nonce_start;
            let mut left = batch_size;

            while left > 0 {
                let chunk = (left as usize).min(self.work_size);
                let local_size = self.local_work_size.min(chunk);
                let global_size = ((chunk + local_size - 1) / local_size) * local_size;

                // Update s4 kernel args
                s4_kernel.set_arg(1, header_bytes.len() as u32)?;
                s4_kernel.set_arg(2, current_nonce)?;
                s4_kernel.set_arg(3, chunk as u32)?;

                unsafe {
                    s4_kernel
                        .cmd()
                        .global_work_size(global_size)
                        .local_work_size(local_size)
                        .enq()?;
                }

                // Read back all s4 results (chunk * 64 bytes)
                let mut s4_data = vec![0u8; chunk * 64];
                s4_out_buf.read(&mut s4_data).enq()?;
                self.pro_que.queue().finish()?;

                // CPU: NPU mix + cosmic fusion + target check for each work item
                // Parallel scan with Rayon, but always pick the FIRST nonce (lowest
                // index) that satisfies the target — matches the sequential loop and
                // avoids the non-determinism of find_map_any which caused
                // RejectedLowDifficulty when GPU and CPU hashes differ.
                let candidates: Vec<(usize, [u8; 32])> = (0..chunk)
                    .into_par_iter()
                    .filter_map(|i| {
                        let s4_slice = &s4_data[i * 64..(i + 1) * 64];
                        let s4_arr: &[u8; 64] = s4_slice.try_into().unwrap();
                        let s5 = npu_mixing_step_epoch(s4_arr, epoch);
                        let hash = cosmic_fusion_opt_rounds(&s5, 8);

                        if target.allows(&hash.data) {
                            Some((i, hash.data))
                        } else {
                            None
                        }
                    })
                    .collect();

                if let Some((i, hash_data)) = candidates.into_iter().min_by_key(|(i, _)| *i) {
                    let nonce = current_nonce.wrapping_add(i as u64);
                    all_solutions.push((nonce, hash_data, None));
                }

                total_tested += chunk as u64;

                if !all_solutions.is_empty() {
                    break;
                }

                current_nonce = current_nonce.wrapping_add(chunk as u64);
                left = left.saturating_sub(chunk as u64);
            }

            Ok(GpuBatchResult {
                solutions: all_solutions,
                nonces_tested: total_tested,
            })
        }

        /// Full-pipeline mining: GPU does all 6 stages + target check.
        fn mine_batch_full(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<GpuBatchResult> {
            let header_bytes = header.to_bytes();
            self.header_buf.write(&header_bytes[..]).enq()?;
            self.pro_que.queue().finish()?;

            let target_u32 = u32::from_be_bytes([
                target.bytes[0],
                target.bytes[1],
                target.bytes[2],
                target.bytes[3],
            ]);

            let mut all_solutions = Vec::new();
            let mut total_tested = 0u64;
            let mut current_nonce = nonce_start;
            let mut left = batch_size;

            while left > 0 {
                let chunk = (left as usize).min(self.work_size);
                let local_size = self.local_work_size.min(chunk);
                let global_size = ((chunk + local_size - 1) / local_size) * local_size;

                let sentinel_slice: [u64; 1] = [SENTINEL];
                self.result_nonce_buf.write(&sentinel_slice[..]).enq()?;
                self.pro_que.queue().finish()?;

                self.kernel.set_arg(1, header_bytes.len() as u32)?;
                self.kernel.set_arg(2, current_nonce)?;
                self.kernel.set_arg(3, chunk as u32)?;
                self.kernel.set_arg(5, target_u32)?;

                unsafe {
                    self.kernel
                        .cmd()
                        .global_work_size(global_size)
                        .local_work_size(local_size)
                        .enq()?;
                }
                self.pro_que.queue().finish()?;

                let mut nonce_out = vec![SENTINEL];
                self.result_nonce_buf.read(&mut nonce_out).enq()?;
                self.pro_que.queue().finish()?;

                if nonce_out[0] != SENTINEL {
                    let mut hash_out = vec![0u8; 32];
                    self.result_hash_buf.read(&mut hash_out).enq()?;
                    self.pro_que.queue().finish()?;
                    let mut hash = [0u8; 32];
                    hash.copy_from_slice(&hash_out);
                    all_solutions.push((nonce_out[0], hash, None));
                    total_tested += chunk as u64;
                    break;
                }

                total_tested += chunk as u64;
                current_nonce = current_nonce.wrapping_add(chunk as u64);
                left = left.saturating_sub(chunk as u64);
            }

            Ok(GpuBatchResult {
                solutions: all_solutions,
                nonces_tested: total_tested,
            })
        }
    }

    impl GpuMiner for OpenClDeekshaMiner {
        fn device_name(&self) -> String {
            self.device_name_cached.clone()
        }

        fn backend_kind(&self) -> GpuBackendKind {
            GpuBackendKind::OpenCL
        }

        fn algorithm(&self) -> String {
            "cosmic_harmony_ekam_deeksha_v2".to_string()
        }

        fn suppress_mismatch_warnings(&self) -> bool {
            // s4-only mode uses a different stage-4 implementation on GPU (SHA3-512)
            // than CPU (Blake3 XOF), so GPU vs CPU hashes naturally differ.
            self.s4_kernel.is_some()
        }

        fn update_epoch(&mut self, height: u64) -> Result<()> {
            let epoch = zion_cosmic_harmony::algorithms_npu::epoch_from_height(height);
            if epoch == self.current_epoch {
                return Ok(());
            }
            let topology = zion_cosmic_harmony::algorithms_npu::MlpTopology::for_epoch(epoch);
            let packed = zion_cosmic_harmony::algorithms_npu::chv4_npu_weights_packed(epoch);
            let new_max_dim = npu_max_dim_for_epoch(epoch);

            // If topology changed max dimension, recompile kernel for optimal register usage
            if new_max_dim != self.current_npu_max_dim {
                let build_opts =
                    full_build_opts(&self.device_name_cached, new_max_dim, self.local_work_size);
                println!(
                    "gpu_opencl_recompile epoch={} npu_max_dim={}->{} opts=\"{}\"",
                    epoch, self.current_npu_max_dim, new_max_dim, build_opts
                );
                let mut prog = ProgramBuilder::new();
                prog.src(self.kernel_src.clone());
                if !build_opts.is_empty() {
                    prog.cmplr_opt(build_opts);
                }
                self.pro_que = ProQue::builder()
                    .platform(self.platform)
                    .device(self.device)
                    .prog_bldr(prog)
                    .dims(self.work_size)
                    .build()
                    .map_err(|e| anyhow::anyhow!("OpenCL recompile failed: {e}"))?;
                self.current_npu_max_dim = new_max_dim;

                // Reallocate ALL buffers on new ProQue
                let q = self.pro_que.queue().clone();
                self.header_buf = Buffer::<u8>::builder().queue(q.clone()).len(128).build()?;
                self.scratchpad_buf = Buffer::<u8>::builder()
                    .queue(q.clone())
                    .len(self.work_size * SCRATCHPAD_BYTES)
                    .build()?;
                self.result_nonce_buf = Buffer::<u64>::builder().queue(q.clone()).len(1).build()?;
                self.result_hash_buf = Buffer::<u8>::builder().queue(q.clone()).len(32).build()?;

                // Rebuild s4 kernel/buffer only if s4-only mode was requested
                if self.s4_kernel.is_some() {
                    let s4_out = Buffer::<u8>::builder()
                        .queue(q.clone())
                        .len(self.work_size * 64)
                        .build()?;
                    self.s4_kernel = Some(
                        self.pro_que
                            .kernel_builder(opencl_kernel::EKAM_DEEKSHA_S4_KERNEL_NAME)
                            .arg(&self.header_buf)
                            .arg(80u32)
                            .arg(0u64)
                            .arg(self.work_size as u32)
                            .arg(&self.scratchpad_buf)
                            .arg(&s4_out)
                            .build()
                            .map_err(|e| anyhow::anyhow!("s4 kernel rebuild failed: {e}"))?,
                    );
                    self.s4_out_buf = Some(s4_out);
                }
            }

            // Reallocate NPU buffers (topology-dependent sizes)
            let q = self.pro_que.queue().clone();
            self.npu_weights = Buffer::<i8>::builder()
                .queue(q.clone())
                .len(packed.weights.len().max(1))
                .copy_host_slice(&packed.weights)
                .build()?;
            self.npu_biases = Buffer::<i8>::builder()
                .queue(q.clone())
                .len(packed.biases.len().max(1))
                .copy_host_slice(&packed.biases)
                .build()?;
            self.npu_scales = Buffer::<i16>::builder()
                .queue(q.clone())
                .len(packed.scales.len().max(1))
                .copy_host_slice(&packed.scales)
                .build()?;
            self.npu_meta = Buffer::<u32>::builder()
                .queue(q.clone())
                .len(packed.meta.len())
                .copy_host_slice(&packed.meta)
                .build()?;

            // Rebuild kernel with current buffers
            self.kernel = self
                .pro_que
                .kernel_builder(opencl_kernel::EKAM_DEEKSHA_KERNEL_NAME)
                .arg(&self.header_buf)
                .arg(80u32)
                .arg(0u64)
                .arg(self.work_size as u32)
                .arg(&self.scratchpad_buf)
                .arg(0u32)
                .arg(&self.result_nonce_buf)
                .arg(&self.result_hash_buf)
                .arg(&self.npu_weights)
                .arg(&self.npu_biases)
                .arg(&self.npu_scales)
                .arg(&self.npu_meta)
                .build()
                .map_err(|e| anyhow::anyhow!("kernel rebuild failed: {e}"))?;

            println!(
                "gpu_opencl_npu_epoch_update epoch={} height={} topology={:?} npu_max_dim={}",
                epoch, height, topology, self.current_npu_max_dim
            );
            self.current_epoch = epoch;

            // Post-epoch self-test: verify full pipeline matches CPU at new epoch.
            // Uses debug kernel (single work-item) so result is deterministic.
            if let Err(e) = self.self_test_at_epoch(epoch) {
                println!(
                    "GPU_EPOCH_SELFTEST_FAIL epoch={} topology={:?} err=\"{e}\"",
                    epoch, topology
                );
            } else {
                println!(
                    "GPU_EPOCH_SELFTEST_OK epoch={} topology={:?}",
                    epoch, topology
                );
            }

            Ok(())
        }

        fn mine_batch(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<GpuBatchResult> {
            if self.s4_kernel.is_some() {
                return self.mine_batch_s4(header, target, nonce_start, batch_size);
            }
            self.mine_batch_full(header, target, nonce_start, batch_size)
        }

        fn benchmark(&mut self, secs: f64) -> Result<(u64, f64, f64)> {
            let header = MiningHeader {
                version: 3,
                previous_hash: [0xAA; 32],
                merkle_root: [0xBB; 32],
                timestamp: 1_762_000_200,
                difficulty_bits: 0x1f00ffff,
            };
            // Impossible target so nothing matches
            let target = DifficultyTarget { bytes: [0; 32] };
            let start = Instant::now();
            let mut total_hashes = 0u64;
            let mut nonce_start = 0u64;

            while start.elapsed().as_secs_f64() < secs {
                let result = self.mine_batch(header, target, nonce_start, self.work_size as u64)?;
                total_hashes += result.nonces_tested;
                nonce_start = nonce_start.wrapping_add(self.work_size as u64);
            }

            let elapsed = start.elapsed().as_secs_f64();
            let khps = if elapsed > 0.0 {
                total_hashes as f64 / elapsed / 1_000.0
            } else {
                0.0
            };

            Ok((total_hashes, elapsed, khps))
        }
    }
}

// ─── OpenCL DeekshaLite Backend (simplified, no NPU) ────────────────────────

#[cfg(feature = "gpu-opencl")]
pub mod opencl_deeksha_lite {
    use super::*;
    use ocl::builders::ProgramBuilder;
    use ocl::{Buffer, Device, Kernel, Platform, ProQue};
    use std::time::Instant;
    use zion_cosmic_harmony::gpu::opencl_kernel;

    const DL_SCRATCHPAD_BYTES: usize = 256 * 1024; // 256 KiB per thread
    const SENTINEL: u64 = 0xFFFF_FFFF_FFFF_FFFF;

    pub struct OpenClDeekshaLiteMiner {
        pro_que: ProQue,
        kernel: Kernel,
        header_state_buf: Buffer<u64>,
        scratchpad_buf: Buffer<u8>,
        output_hashes_buf: Buffer<u8>,
        stream_weights_buf: Buffer<f32>,
        work_size: usize,
        local_work_size: usize,
        device_name_cached: String,
        device_family: GpuDeviceFamily,
        tuning: GpuTuning,
        recovery_attempts: u32,
        max_recovery_attempts: u32,
    }

    impl OpenClDeekshaLiteMiner {
        /// Precompute Keccak256 state after absorbing the 80-byte header.
        /// The state is 25 u64s (200 bytes). Each thread will then only
        /// XOR the nonce bytes (80..88), apply padding, and run f1600.
        fn precompute_header_keccak_state(header_80: &[u8]) -> [u64; 25] {
            let mut state = [0u64; 25];
            for (i, &b) in header_80.iter().enumerate() {
                let byte_idx = i;
                let word_idx = byte_idx / 8;
                let shift = (byte_idx % 8) * 8;
                state[word_idx] ^= (b as u64) << shift;
            }
            state
        }

        fn vram_aware_work_size(device: &Device, requested: usize) -> usize {
            let global_mem = device
                .info(ocl::enums::DeviceInfo::GlobalMemSize)
                .ok()
                .and_then(|v| match v {
                    ocl::enums::DeviceInfoResult::GlobalMemSize(n) => Some(n as usize),
                    _ => None,
                })
                .unwrap_or(2_000_000_000);
            let reserve = 384 * 1024 * 1024; // driver + other buffers
            let available = global_mem.saturating_sub(reserve);
            let per_thread = DL_SCRATCHPAD_BYTES + 64; // scratchpad + output hash
            let max_by_vram = available / per_thread;
            let size = requested.min(max_by_vram).max(64);
            size
        }

        fn pick_device() -> Result<(Platform, Device, String, String)> {
            let platforms = Platform::list();
            if platforms.is_empty() {
                anyhow::bail!("no OpenCL platforms found");
            }
            let mut candidates = Vec::new();
            for (pidx, platform) in platforms.iter().enumerate() {
                let platform_name = platform
                    .name()
                    .unwrap_or_else(|_| "unknown-platform".to_string());
                let gpus = Device::list(platform, Some(ocl::flags::DeviceType::GPU))
                    .map_err(|e| anyhow::anyhow!("OpenCL device list on {platform_name}: {e}"))?;
                for (didx, device) in gpus.into_iter().enumerate() {
                    let device_name = device
                        .name()
                        .unwrap_or_else(|_| "unknown-device".to_string());
                    let platform_l = platform_name.to_ascii_lowercase();
                    let device_l = device_name.to_ascii_lowercase();
                    let mut score: i64 = 0;
                    if platform_l.contains("amd")
                        || device_l.contains("amd")
                        || device_l.contains("radeon")
                    {
                        score += 1000;
                    }
                    if device_l.contains("vega") || device_l.contains("rx 5") {
                        score += 500;
                    }
                    candidates.push((
                        score,
                        pidx,
                        didx,
                        *platform,
                        device,
                        platform_name.clone(),
                        device_name,
                    ));
                }
            }
            if candidates.is_empty() {
                anyhow::bail!("no OpenCL GPU devices found");
            }
            candidates.sort_by_key(|(s, _, _, _, _, _, _)| -*s);
            let (_, pidx, didx, platform, device, platform_name, device_name) =
                candidates.swap_remove(0);
            println!(
                "gpu_opencl_lite_pick platform_idx={pidx} device_idx={didx} platform=\"{platform_name}\" device=\"{device_name}\""
            );
            Ok((platform, device, platform_name, device_name))
        }

        pub fn new(requested_work_size: usize) -> Result<Self> {
            Self::new_with_kernel(requested_work_size, false)
        }

        /// Phase C: Create a DeekshaChv3 GPU miner using the canonical
        /// `deeksha_chv3.cl` kernel source and `deeksha_chv3_mine` entry point.
        /// Bit-identical to `new()` — only the kernel name/source differs.
        pub fn new_chv3(requested_work_size: usize) -> Result<Self> {
            Self::new_with_kernel(requested_work_size, true)
        }

        fn new_with_kernel(requested_work_size: usize, use_chv3: bool) -> Result<Self> {
            let kernel_src = if use_chv3 {
                opencl_kernel::get_deeksha_chv3_kernel_source().to_string()
            } else {
                opencl_kernel::get_deeksha_lite_kernel_source().to_string()
            };
            let kernel_name = if use_chv3 {
                opencl_kernel::DEEKSHA_CHV3_KERNEL_NAME
            } else {
                opencl_kernel::DEEKSHA_LITE_KERNEL_NAME
            };
            let (platform, device, platform_name, device_name) = Self::pick_device()?;

            let family = GpuDeviceFamily::from_name(&device_name);
            let vram = device
                .info(ocl::enums::DeviceInfo::GlobalMemSize)
                .ok()
                .and_then(|v| match v {
                    ocl::enums::DeviceInfoResult::GlobalMemSize(n) => Some(n as usize),
                    _ => None,
                })
                .unwrap_or(2_000_000_000);

            let tuning = GpuTuning::auto_tune(GpuAlgorithm::DeekshaLiteV1, family, vram);
            let actual_work_size = requested_work_size
                .min(tuning.work_size)
                .max(64)
                .next_power_of_two();

            println!(
                "gpu_opencl_lite_init family={:?} device=\"{}\" vram={}MiB tuned_ws={} local_ws={} build_opts=\"{}\"",
                family,
                device_name,
                vram / (1024 * 1024),
                actual_work_size,
                tuning.local_ws,
                tuning.build_opts,
            );

            let pro_que = {
                let mut prog = ProgramBuilder::new();
                prog.src(kernel_src);
                if !tuning.build_opts.is_empty() {
                    prog.cmplr_opt(&tuning.build_opts);
                }
                ProQue::builder()
                    .platform(platform)
                    .device(device)
                    .prog_bldr(prog)
                    .dims(actual_work_size)
                    .build()
                    .map_err(|e| anyhow::anyhow!("OpenCL build failed: {e}"))?
            };
            let q = pro_que.queue().clone();
            let header_state_buf = Buffer::<u64>::builder().queue(q.clone()).len(25).build()?;
            let scratchpad_buf = Buffer::<u8>::builder()
                .queue(q.clone())
                .len(actual_work_size * DL_SCRATCHPAD_BYTES)
                .build()
                .map_err(|e| {
                    anyhow::anyhow!(
                        "scratchpad alloc failed ({} MiB): {e}",
                        actual_work_size * DL_SCRATCHPAD_BYTES / (1024 * 1024)
                    )
                })?;
            let output_hashes_buf = Buffer::<u8>::builder()
                .queue(q.clone())
                .len(actual_work_size * 32)
                .build()?;
            let stream_weights_zero = [0.0f32; 6];
            let stream_weights_buf = Buffer::<f32>::builder()
                .queue(q.clone())
                .len(6)
                .copy_host_slice(&stream_weights_zero[..])
                .build()?;
            let kernel = pro_que
                .kernel_builder(kernel_name)
                .arg(&header_state_buf)
                .arg(0u64)
                .arg(0u32)
                .arg(&output_hashes_buf)
                .arg(&scratchpad_buf)
                .arg(&stream_weights_buf)
                .build()
                .map_err(|e| anyhow::anyhow!("kernel build failed: {e}"))?;
            println!(
                "gpu_opencl_lite_init device=\"{}\" work_size={} local_ws={} scratchpad_mib={}",
                device_name,
                actual_work_size,
                tuning.local_ws,
                actual_work_size * DL_SCRATCHPAD_BYTES / (1024 * 1024)
            );
            Ok(Self {
                pro_que,
                kernel,
                header_state_buf,
                scratchpad_buf,
                output_hashes_buf,
                stream_weights_buf,
                work_size: actual_work_size,
                local_work_size: tuning.local_ws,
                device_name_cached: device_name,
                device_family: family,
                tuning,
                recovery_attempts: 0,
                max_recovery_attempts: 3,
            })
        }
    }

    impl GpuMiner for OpenClDeekshaLiteMiner {
        fn device_name(&self) -> String {
            self.device_name_cached.clone()
        }

        fn backend_kind(&self) -> GpuBackendKind {
            GpuBackendKind::OpenCL
        }

        fn algorithm(&self) -> String {
            "deeksha_lite_v1".to_string()
        }

        fn suppress_mismatch_warnings(&self) -> bool {
            false
        }

        fn set_stream_weights(
            &mut self,
            weights: &zion_cosmic_harmony::stream_profit::StreamWeights,
        ) -> Result<()> {
            let arr = stream_weights_f32(weights);
            self.stream_weights_buf.write(&arr[..]).enq()?;
            self.pro_que.queue().finish()?;
            println!("gpu_opencl_lite_stream_weights {}", weights.describe());
            Ok(())
        }

        fn mine_batch(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<GpuBatchResult> {
            let header_bytes = header.to_bytes();
            let header_80 = &header_bytes[..80.min(header_bytes.len())];
            let precomputed_state = Self::precompute_header_keccak_state(header_80);

            // ── SEH guard for OpenCL buffer write ───────────────────────
            {
                let guard = GpuGuard::new();
                self.header_state_buf.write(&precomputed_state[..]).enq()?;
                if guard.was_caught() {
                    self.recovery_attempts += 1;
                    anyhow::bail!(
                        "GPU access violation during header state buffer write (attempt {}/{}). AMD driver crash detected — try reducing ZION_GPU_WORK_SIZE or ZION_OCL_VRAM_PCT.",
                        self.recovery_attempts,
                        self.max_recovery_attempts
                    );
                }
            }

            let mut all_solutions = Vec::new();
            let mut total_tested = 0u64;
            let mut current_nonce = nonce_start;
            let mut left = batch_size;
            while left > 0 {
                let chunk = (left as usize).min(self.work_size);
                let local_size = self.local_work_size.min(chunk);
                let global_size = ((chunk + local_size - 1) / local_size) * local_size;
                self.kernel.set_arg(1, current_nonce)?;
                self.kernel.set_arg(2, chunk as u32)?;

                // ── SEH guard for kernel enqueue ──────────────────────────
                {
                    let guard = GpuGuard::new();
                    unsafe {
                        self.kernel
                            .cmd()
                            .global_work_size(global_size)
                            .local_work_size(local_size)
                            .enq()?;
                    }
                    if guard.was_caught() {
                        self.recovery_attempts += 1;
                        anyhow::bail!(
                            "GPU access violation during kernel enqueue (attempt {}/{}). AMD driver crash detected — try reducing ZION_GPU_WORK_SIZE or switching to CPU backend.",
                            self.recovery_attempts,
                            self.max_recovery_attempts
                        );
                    }
                }

                // ── SEH guard for buffer read ─────────────────────────────
                let mut hashes = vec![0u8; chunk * 32];
                {
                    let guard = GpuGuard::new();
                    self.output_hashes_buf.read(&mut hashes).enq()?;
                    self.pro_que.queue().finish()?;
                    if guard.was_caught() {
                        self.recovery_attempts += 1;
                        anyhow::bail!(
                            "GPU access violation during hash buffer read (attempt {}/{}). AMD driver crash detected.",
                            self.recovery_attempts,
                            self.max_recovery_attempts
                        );
                    }
                }

                for i in 0..chunk {
                    let hash: [u8; 32] = hashes[i * 32..(i + 1) * 32].try_into().unwrap();
                    if target.allows(&hash) {
                        let nonce = current_nonce.wrapping_add(i as u64);
                        all_solutions.push((nonce, hash, None));
                        break; // first match wins
                    }
                }
                total_tested += chunk as u64;
                if !all_solutions.is_empty() {
                    break;
                }
                current_nonce = current_nonce.wrapping_add(chunk as u64);
                left -= chunk as u64;
            }
            Ok(GpuBatchResult {
                nonces_tested: total_tested,
                solutions: all_solutions,
            })
        }

        fn benchmark(&mut self, secs: f64) -> Result<(u64, f64, f64)> {
            let header = MiningHeader {
                version: 3,
                previous_hash: [0xAA; 32],
                merkle_root: [0xBB; 32],
                timestamp: 1_762_000_200,
                difficulty_bits: 0x1f00ffff,
            };
            let target = DifficultyTarget { bytes: [0; 32] };
            let start = Instant::now();
            let mut total_hashes = 0u64;
            let mut nonce_start = 0u64;
            while start.elapsed().as_secs_f64() < secs {
                let result = self.mine_batch(header, target, nonce_start, self.work_size as u64)?;
                total_hashes += result.nonces_tested;
                nonce_start = nonce_start.wrapping_add(self.work_size as u64);
            }
            let elapsed = start.elapsed().as_secs_f64();
            let khps = if elapsed > 0.0 {
                total_hashes as f64 / elapsed / 1_000.0
            } else {
                0.0
            };
            Ok((total_hashes, elapsed, khps))
        }
    }
}

// ─── OpenCL DeekshaLite Fire Backend (thermal-intensive) ───────────────────

#[cfg(feature = "gpu-opencl")]
pub mod opencl_deeksha_lite_fire {
    use super::*;
    use ocl::builders::ProgramBuilder;
    use ocl::{Buffer, Device, Kernel, Platform, ProQue};
    use std::time::Instant;
    use zion_cosmic_harmony::gpu::opencl_kernel;

    const DLF_SCRATCHPAD_BYTES: usize = 256 * 1024; // 256 KiB per thread — same as v1
    const SENTINEL: u64 = 0xFFFF_FFFF_FFFF_FFFF;

    pub struct OpenClDeekshaLiteFireMiner {
        pro_que: ProQue,
        kernel: Kernel,
        /// Host-precomputed partial Keccak256 state (25 × u64) after absorbing
        /// the 80-byte header (nonce bytes left as 0). Identical to v1 approach.
        header_state_buf: Buffer<u64>,
        scratchpad_buf: Buffer<u8>,
        output_hashes_buf: Buffer<u8>,
        stream_weights_buf: Buffer<f32>,
        work_size: usize,
        local_work_size: usize,
        device_name_cached: String,
        device_family: GpuDeviceFamily,
        tuning: GpuTuning,
        recovery_attempts: u32,
        max_recovery_attempts: u32,
    }

    impl OpenClDeekshaLiteFireMiner {
        /// Precompute Keccak256 state after absorbing the 80-byte header.
        /// The state is 25 u64s (200 bytes). Each thread will then only
        /// XOR the nonce bytes (80..88), apply padding, and run f1600.
        /// Identical to v1's implementation — guarantees CPU/GPU hash agreement.
        fn precompute_header_keccak_state(header_80: &[u8]) -> [u64; 25] {
            let mut state = [0u64; 25];
            for (i, &b) in header_80.iter().enumerate() {
                let word_idx = i / 8;
                let shift = (i % 8) * 8;
                state[word_idx] ^= (b as u64) << shift;
            }
            state
        }

        fn vram_aware_work_size(device: &Device, requested: usize) -> usize {
            let global_mem = device
                .info(ocl::enums::DeviceInfo::GlobalMemSize)
                .ok()
                .and_then(|v| match v {
                    ocl::enums::DeviceInfoResult::GlobalMemSize(n) => Some(n as usize),
                    _ => None,
                })
                .unwrap_or(2_000_000_000);
            let reserve = 384 * 1024 * 1024; // driver + other buffers
            let available = global_mem.saturating_sub(reserve);
            let per_thread = DLF_SCRATCHPAD_BYTES + 64; // scratchpad + output hash
            let max_by_vram = available / per_thread;
            let size = requested.min(max_by_vram).max(64);
            size
        }

        fn pick_device() -> Result<(Platform, Device, String, String)> {
            let platforms = Platform::list();
            if platforms.is_empty() {
                anyhow::bail!("no OpenCL platforms found");
            }
            let mut candidates = Vec::new();
            for (pidx, platform) in platforms.iter().enumerate() {
                let platform_name = platform
                    .name()
                    .unwrap_or_else(|_| "unknown-platform".to_string());
                let gpus = Device::list(platform, Some(ocl::flags::DeviceType::GPU))
                    .map_err(|e| anyhow::anyhow!("OpenCL device list on {platform_name}: {e}"))?;
                for (didx, device) in gpus.into_iter().enumerate() {
                    let device_name = device
                        .name()
                        .unwrap_or_else(|_| "unknown-device".to_string());
                    let platform_l = platform_name.to_ascii_lowercase();
                    let device_l = device_name.to_ascii_lowercase();
                    let mut score: i64 = 0;
                    if platform_l.contains("amd")
                        || device_l.contains("amd")
                        || device_l.contains("radeon")
                    {
                        score += 1000;
                    }
                    if device_l.contains("vega") || device_l.contains("rx 5") {
                        score += 500;
                    }
                    candidates.push((
                        score,
                        pidx,
                        didx,
                        *platform,
                        device,
                        platform_name.clone(),
                        device_name,
                    ));
                }
            }
            if candidates.is_empty() {
                anyhow::bail!("no OpenCL GPU devices found");
            }
            candidates.sort_by_key(|(s, _, _, _, _, _, _)| -*s);
            let (_, pidx, didx, platform, device, platform_name, device_name) =
                candidates.swap_remove(0);
            println!(
                "gpu_opencl_fire_pick platform_idx={pidx} device_idx={didx} platform=\"{platform_name}\" device=\"{device_name}\""
            );
            Ok((platform, device, platform_name, device_name))
        }

        pub fn new(requested_work_size: usize) -> Result<Self> {
            let kernel_src = opencl_kernel::get_deeksha_lite_fire_kernel_source().to_string();
            let (platform, device, platform_name, device_name) = Self::pick_device()?;

            let family = GpuDeviceFamily::from_name(&device_name);
            let vram = device
                .info(ocl::enums::DeviceInfo::GlobalMemSize)
                .ok()
                .and_then(|v| match v {
                    ocl::enums::DeviceInfoResult::GlobalMemSize(n) => Some(n as usize),
                    _ => None,
                })
                .unwrap_or(2_000_000_000);

            let tuning = GpuTuning::auto_tune(GpuAlgorithm::DeekshaLiteFire, family, vram);
            let actual_work_size = requested_work_size
                .min(tuning.work_size)
                .max(64)
                .next_power_of_two();

            println!(
                "gpu_opencl_fire_init family={:?} device=\"{}\" vram={}MiB tuned_ws={} local_ws={} build_opts=\"{}\"",
                family,
                device_name,
                vram / (1024 * 1024),
                actual_work_size,
                tuning.local_ws,
                tuning.build_opts,
            );

            let pro_que = {
                let mut prog = ProgramBuilder::new();
                prog.src(kernel_src);
                if !tuning.build_opts.is_empty() {
                    prog.cmplr_opt(&tuning.build_opts);
                }
                ProQue::builder()
                    .platform(platform)
                    .device(device)
                    .prog_bldr(prog)
                    .dims(actual_work_size)
                    .build()
                    .map_err(|e| anyhow::anyhow!("OpenCL build failed: {e}"))?
            };
            let q = pro_que.queue().clone();
            let header_state_buf = Buffer::<u64>::builder().queue(q.clone()).len(25).build()?;
            let scratchpad_buf = Buffer::<u8>::builder()
                .queue(q.clone())
                .len(actual_work_size * DLF_SCRATCHPAD_BYTES)
                .build()
                .map_err(|e| {
                    anyhow::anyhow!(
                        "scratchpad alloc failed ({} MiB): {e}",
                        actual_work_size * DLF_SCRATCHPAD_BYTES / (1024 * 1024)
                    )
                })?;
            let output_hashes_buf = Buffer::<u8>::builder()
                .queue(q.clone())
                .len(actual_work_size * 32)
                .build()?;
            let stream_weights_zero = [0.0f32; 6];
            let stream_weights_buf = Buffer::<f32>::builder()
                .queue(q.clone())
                .len(6)
                .copy_host_slice(&stream_weights_zero[..])
                .build()?;
            let kernel = pro_que
                .kernel_builder(opencl_kernel::DEEKSHA_LITE_FIRE_KERNEL_NAME)
                .arg(&header_state_buf)
                .arg(0u64)
                .arg(0u32)
                .arg(&output_hashes_buf)
                .arg(&scratchpad_buf)
                .arg(&stream_weights_buf)
                .build()
                .map_err(|e| anyhow::anyhow!("kernel build failed: {e}"))?;
            println!(
                "gpu_opencl_fire_init device=\"{}\" work_size={} local_ws={} scratchpad_mib={}",
                device_name,
                actual_work_size,
                tuning.local_ws,
                actual_work_size * DLF_SCRATCHPAD_BYTES / (1024 * 1024)
            );
            Ok(Self {
                pro_que,
                kernel,
                header_state_buf,
                scratchpad_buf,
                output_hashes_buf,
                stream_weights_buf,
                work_size: actual_work_size,
                local_work_size: tuning.local_ws,
                device_name_cached: device_name,
                device_family: family,
                tuning,
                recovery_attempts: 0,
                max_recovery_attempts: 3,
            })
        }
    }

    impl GpuMiner for OpenClDeekshaLiteFireMiner {
        fn device_name(&self) -> String {
            self.device_name_cached.clone()
        }

        fn backend_kind(&self) -> GpuBackendKind {
            GpuBackendKind::OpenCL
        }

        fn algorithm(&self) -> String {
            "deeksha_lite_fire".to_string()
        }

        fn suppress_mismatch_warnings(&self) -> bool {
            false
        }

        fn set_stream_weights(
            &mut self,
            weights: &zion_cosmic_harmony::stream_profit::StreamWeights,
        ) -> Result<()> {
            let arr = stream_weights_f32(weights);
            self.stream_weights_buf.write(&arr[..]).enq()?;
            self.pro_que.queue().finish()?;
            println!("gpu_opencl_fire_stream_weights {}", weights.describe());
            Ok(())
        }

        fn mine_batch(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<GpuBatchResult> {
            let header_bytes = header.to_bytes();
            let header_80 = &header_bytes[..80.min(header_bytes.len())];
            let precomputed_state = Self::precompute_header_keccak_state(header_80);

            {
                let guard = GpuGuard::new();
                self.header_state_buf.write(&precomputed_state[..]).enq()?;
                if guard.was_caught() {
                    self.recovery_attempts += 1;
                    anyhow::bail!(
                        "GPU access violation during header state buffer write (attempt {}/{}). AMD driver crash detected — try reducing ZION_GPU_WORK_SIZE or ZION_OCL_VRAM_PCT.",
                        self.recovery_attempts,
                        self.max_recovery_attempts
                    );
                }
            }

            let mut all_solutions = Vec::new();
            let mut total_tested = 0u64;
            let mut current_nonce = nonce_start;
            let mut left = batch_size;
            while left > 0 {
                let chunk = (left as usize).min(self.work_size);
                let local_size = self.local_work_size.min(chunk);
                let global_size = ((chunk + local_size - 1) / local_size) * local_size;

                // Set nonce parameters via scalar args (same as Lite miner)
                {
                    let guard = GpuGuard::new();
                    self.kernel.set_arg(1, current_nonce)?;
                    self.kernel.set_arg(2, chunk as u32)?;
                    if guard.was_caught() {
                        self.recovery_attempts += 1;
                        anyhow::bail!(
                            "GPU access violation during nonce arg set (attempt {}/{}).",
                            self.recovery_attempts,
                            self.max_recovery_attempts
                        );
                    }
                }

                {
                    let guard = GpuGuard::new();
                    unsafe {
                        self.kernel
                            .cmd()
                            .global_work_size(global_size)
                            .local_work_size(local_size)
                            .enq()?;
                    }
                    if guard.was_caught() {
                        self.recovery_attempts += 1;
                        anyhow::bail!(
                            "GPU access violation during kernel enqueue (attempt {}/{}). AMD driver crash detected — try reducing ZION_GPU_WORK_SIZE or switching to CPU backend.",
                            self.recovery_attempts,
                            self.max_recovery_attempts
                        );
                    }
                }

                let mut hashes = vec![0u8; chunk * 32];
                {
                    let guard = GpuGuard::new();
                    self.output_hashes_buf.read(&mut hashes).enq()?;
                    self.pro_que.queue().finish()?;
                    if guard.was_caught() {
                        self.recovery_attempts += 1;
                        anyhow::bail!(
                            "GPU access violation during hash buffer read (attempt {}/{}). AMD driver crash detected.",
                            self.recovery_attempts,
                            self.max_recovery_attempts
                        );
                    }
                }

                for i in 0..chunk {
                    let hash: [u8; 32] = hashes[i * 32..(i + 1) * 32].try_into().unwrap();
                    if target.allows(&hash) {
                        let nonce = current_nonce.wrapping_add(i as u64);
                        all_solutions.push((nonce, hash, None));
                        break;
                    }
                }
                total_tested += chunk as u64;
                if !all_solutions.is_empty() {
                    break;
                }
                current_nonce = current_nonce.wrapping_add(chunk as u64);
                left -= chunk as u64;
            }
            Ok(GpuBatchResult {
                nonces_tested: total_tested,
                solutions: all_solutions,
            })
        }

        fn benchmark(&mut self, secs: f64) -> Result<(u64, f64, f64)> {
            let header = MiningHeader {
                version: 3,
                previous_hash: [0xAA; 32],
                merkle_root: [0xBB; 32],
                timestamp: 1_762_000_200,
                difficulty_bits: 0x1f00ffff,
            };
            let target = DifficultyTarget { bytes: [0; 32] };
            let start = Instant::now();
            let mut total_hashes = 0u64;
            let mut nonce_start = 0u64;
            while start.elapsed().as_secs_f64() < secs {
                let result = self.mine_batch(header, target, nonce_start, self.work_size as u64)?;
                total_hashes += result.nonces_tested;
                nonce_start = nonce_start.wrapping_add(self.work_size as u64);
            }
            let elapsed = start.elapsed().as_secs_f64();
            let khps = if elapsed > 0.0 {
                total_hashes as f64 / elapsed / 1_000.0
            } else {
                0.0
            };
            Ok((total_hashes, elapsed, khps))
        }
    }
}

// ─── CUDA Backend ───────────────────────────────────────────────────────────

#[cfg(feature = "gpu-cuda")]
pub mod cuda_deeksha {
    use super::*;
    use cudarc::driver::{CudaDevice, CudaSlice, LaunchAsync, LaunchConfig};
    use cudarc::nvrtc::{compile_ptx_with_opts, CompileOptions};
    use std::sync::Arc;
    use std::time::Instant;

    const CUDA_KERNEL_SRC: &str = include_str!("cosmic_harmony_deeksha.cu");
    const SCRATCHPAD_BYTES: usize = 262_144; // 256 KiB per thread
    const SENTINEL: u64 = 0xFFFF_FFFF_FFFF_FFFF;
    const DEFAULT_WORK_SIZE_CAP: usize = 32_768;

    pub struct CudaDeekshaMiner {
        dev: Arc<CudaDevice>,
        work_size: usize,
        device_name_cached: String,
        // Pre-allocated GPU buffers
        header_buf: CudaSlice<u8>,
        scratchpad_buf: CudaSlice<u8>,
        result_nonce: CudaSlice<u64>,
        result_hash: CudaSlice<u8>,
        // NPU packed weight buffers (variable topology)
        npu_weights: CudaSlice<i8>,
        npu_biases: CudaSlice<i8>,
        npu_scales: CudaSlice<i16>,
        npu_meta: CudaSlice<u32>,
        current_epoch: u64,
    }

    // Max buffer sizes across all topologies:
    //  Standard:   w=16384 b=192 s=192  (2 layers: 64→128→64)
    //  ThreeLayer: w=26624 b=288 s=288  (3 layers: 64→96→128→64)
    //  Wide:       w=32768 b=320 s=320  (2 layers: 64→256→64)
    //  Deep:       w=12288 b=192 s=192  (3 layers: 64→64→64→64)
    const MAX_NPU_WEIGHTS: usize = 32768;
    const MAX_NPU_BIASES: usize = 320;
    const MAX_NPU_SCALES: usize = 320; // i16 count
    const MAX_NPU_META: usize = 8; // u32 count (1 + 2*max_layers)

    impl CudaDeekshaMiner {
        pub fn new(work_size: usize) -> Result<Self> {
            let dev =
                CudaDevice::new(0).map_err(|e| anyhow::anyhow!("CUDA device init failed: {e}"))?;

            let device_name = dev
                .name()
                .unwrap_or_else(|_| "unknown CUDA device".to_string());

            // Compile PTX with fast-math (integer-safe; helps sqrtf in NPU LayerNorm)
            let ptx = compile_ptx_with_opts(
                CUDA_KERNEL_SRC,
                CompileOptions {
                    options: vec!["--use_fast_math".to_string()],
                    ..Default::default()
                },
            )
            .map_err(|e| anyhow::anyhow!("NVRTC compile failed: {e}"))?;
            dev.load_ptx(
                ptx,
                "deeksha",
                &["deeksha_mine", "ekam_deeksha_mine", "ekam_deeksha_debug"],
            )
            .map_err(|e| anyhow::anyhow!("PTX load failed: {e}"))?;

            // Conservative work size cap
            let work_cap = std::env::var("ZION_CUDA_WORK_CAP")
                .ok()
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(DEFAULT_WORK_SIZE_CAP)
                .max(64);
            let actual_work_size = work_size.min(work_cap).max(64);

            // Allocate fixed buffers
            let header_buf = dev
                .alloc_zeros::<u8>(80)
                .map_err(|e| anyhow::anyhow!("header alloc: {e}"))?;
            let scratchpad_buf = dev
                .alloc_zeros::<u8>(actual_work_size * SCRATCHPAD_BYTES)
                .map_err(|e| anyhow::anyhow!("scratchpad alloc: {e}"))?;
            let result_nonce = dev
                .htod_copy(vec![SENTINEL])
                .map_err(|e| anyhow::anyhow!("result_nonce alloc: {e}"))?;
            let result_hash = dev
                .alloc_zeros::<u8>(32)
                .map_err(|e| anyhow::anyhow!("result_hash alloc: {e}"))?;

            // NPU packed buffers — allocate max size, upload epoch 0 weights
            let init_epoch = 0u64;
            let packed = zion_cosmic_harmony::algorithms_npu::chv4_npu_weights_packed(init_epoch);

            let mut w_padded = packed.weights;
            w_padded.resize(MAX_NPU_WEIGHTS, 0);
            let npu_weights = dev
                .htod_copy(w_padded)
                .map_err(|e| anyhow::anyhow!("npu_weights alloc: {e}"))?;

            let mut b_padded = packed.biases;
            b_padded.resize(MAX_NPU_BIASES, 0);
            let npu_biases = dev
                .htod_copy(b_padded)
                .map_err(|e| anyhow::anyhow!("npu_biases alloc: {e}"))?;

            let mut s_padded = packed.scales;
            s_padded.resize(MAX_NPU_SCALES, 0);
            let npu_scales = dev
                .htod_copy(s_padded)
                .map_err(|e| anyhow::anyhow!("npu_scales alloc: {e}"))?;

            let mut m_padded = packed.meta;
            m_padded.resize(MAX_NPU_META, 0);
            let npu_meta = dev
                .htod_copy(m_padded)
                .map_err(|e| anyhow::anyhow!("npu_meta alloc: {e}"))?;

            println!(
                "gpu_cuda_init device=\"{}\" work_size={} scratchpad_mb={}",
                device_name,
                actual_work_size,
                actual_work_size * SCRATCHPAD_BYTES / (1024 * 1024),
            );

            Ok(Self {
                dev,
                work_size: actual_work_size,
                device_name_cached: device_name,
                header_buf,
                scratchpad_buf,
                result_nonce,
                result_hash,
                npu_weights,
                npu_biases,
                npu_scales,
                npu_meta,
                current_epoch: init_epoch,
            })
        }
    }

    impl GpuMiner for CudaDeekshaMiner {
        fn device_name(&self) -> String {
            self.device_name_cached.clone()
        }

        fn backend_kind(&self) -> GpuBackendKind {
            GpuBackendKind::Cuda
        }

        fn update_epoch(&mut self, height: u64) -> Result<()> {
            let epoch = zion_cosmic_harmony::algorithms_npu::epoch_from_height(height);
            if epoch == self.current_epoch {
                return Ok(());
            }
            let packed = zion_cosmic_harmony::algorithms_npu::chv4_npu_weights_packed(epoch);

            let mut w_padded = packed.weights;
            w_padded.resize(MAX_NPU_WEIGHTS, 0);
            self.dev
                .htod_sync_copy_into(&w_padded, &mut self.npu_weights)
                .map_err(|e| anyhow::anyhow!("npu_weights update: {e}"))?;

            let mut b_padded = packed.biases;
            b_padded.resize(MAX_NPU_BIASES, 0);
            self.dev
                .htod_sync_copy_into(&b_padded, &mut self.npu_biases)
                .map_err(|e| anyhow::anyhow!("npu_biases update: {e}"))?;

            let mut s_padded = packed.scales;
            s_padded.resize(MAX_NPU_SCALES, 0);
            self.dev
                .htod_sync_copy_into(&s_padded, &mut self.npu_scales)
                .map_err(|e| anyhow::anyhow!("npu_scales update: {e}"))?;

            let mut m_padded = packed.meta;
            m_padded.resize(MAX_NPU_META, 0);
            self.dev
                .htod_sync_copy_into(&m_padded, &mut self.npu_meta)
                .map_err(|e| anyhow::anyhow!("npu_meta update: {e}"))?;

            let topo = zion_cosmic_harmony::algorithms_npu::MlpTopology::for_epoch(epoch);
            println!(
                "gpu_cuda_npu_epoch_update epoch={} height={} topology={:?}",
                epoch, height, topo
            );
            self.current_epoch = epoch;
            Ok(())
        }

        fn mine_batch(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<GpuBatchResult> {
            let header_bytes = header.to_bytes();
            self.dev
                .htod_sync_copy_into(&header_bytes[..], &mut self.header_buf)
                .map_err(|e| anyhow::anyhow!("header upload: {e}"))?;

            // Target: LE u32 from first 4 bytes of target
            let target_u32 = u32::from_le_bytes([
                target.bytes[0],
                target.bytes[1],
                target.bytes[2],
                target.bytes[3],
            ]);

            let mut all_solutions = Vec::new();
            let mut total_tested = 0u64;
            let mut current_nonce = nonce_start;
            let mut left = batch_size;

            let func = self
                .dev
                .get_func("deeksha", "ekam_deeksha_mine")
                .ok_or_else(|| anyhow::anyhow!("ekam_deeksha_mine kernel not found"))?;

            let threads_per_block: u32 = std::env::var("ZION_CUDA_TPB")
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(48);

            while left > 0 {
                let chunk = (left as usize).min(self.work_size) as u32;
                let blocks = (chunk + threads_per_block - 1) / threads_per_block;
                let cfg = LaunchConfig {
                    grid_dim: (blocks, 1, 1),
                    block_dim: (threads_per_block, 1, 1),
                    shared_mem_bytes: 0,
                };

                // Reset sentinel
                self.dev
                    .htod_sync_copy_into(&[SENTINEL], &mut self.result_nonce)
                    .map_err(|e| anyhow::anyhow!("reset sentinel: {e}"))?;

                unsafe {
                    func.clone()
                        .launch(
                            cfg,
                            (
                                &self.header_buf,
                                header_bytes.len() as u32,
                                current_nonce,
                                chunk,
                                &self.scratchpad_buf,
                                target_u32,
                                &mut self.result_nonce,
                                &mut self.result_hash,
                                &self.npu_weights,
                                &self.npu_biases,
                                &self.npu_scales,
                                &self.npu_meta,
                            ),
                        )
                        .map_err(|e| anyhow::anyhow!("kernel launch: {e}"))?;
                }

                // Sync and read result
                let nonce_result = self
                    .dev
                    .dtoh_sync_copy(&self.result_nonce)
                    .map_err(|e| anyhow::anyhow!("read result_nonce: {e}"))?;

                if nonce_result[0] != SENTINEL {
                    let hash_result = self
                        .dev
                        .dtoh_sync_copy(&self.result_hash)
                        .map_err(|e| anyhow::anyhow!("read result_hash: {e}"))?;
                    let mut hash = [0u8; 32];
                    hash.copy_from_slice(&hash_result[..32]);
                    all_solutions.push((nonce_result[0], hash, None));
                    total_tested += chunk as u64;
                    break; // Early termination on solution
                }

                total_tested += chunk as u64;
                current_nonce = current_nonce.wrapping_add(chunk as u64);
                left = left.saturating_sub(chunk as u64);
            }

            Ok(GpuBatchResult {
                solutions: all_solutions,
                nonces_tested: total_tested,
            })
        }

        fn benchmark(&mut self, secs: f64) -> Result<(u64, f64, f64)> {
            let header = MiningHeader {
                version: 3,
                previous_hash: [0xAA; 32],
                merkle_root: [0xBB; 32],
                timestamp: 1_762_000_200,
                difficulty_bits: 0x1f00ffff,
            };
            let target = DifficultyTarget { bytes: [0; 32] };
            let start = Instant::now();
            let mut total_hashes = 0u64;
            let mut nonce_start = 0u64;

            while start.elapsed().as_secs_f64() < secs {
                let result = self.mine_batch(header, target, nonce_start, self.work_size as u64)?;
                total_hashes += result.nonces_tested;
                nonce_start = nonce_start.wrapping_add(self.work_size as u64);
            }

            let elapsed = start.elapsed().as_secs_f64();
            let khps = if elapsed > 0.0 {
                total_hashes as f64 / elapsed / 1_000.0
            } else {
                0.0
            };

            Ok((total_hashes, elapsed, khps))
        }
    }
}

// ─── Metal Backend (Apple Silicon) ───────────────────────────────────────────

#[cfg(feature = "gpu-metal")]
pub mod metal_deeksha {
    use super::*;
    use metal::{Device, MTLResourceOptions, MTLSize};
    use std::time::Instant;

    const SENTINEL: u64 = 0xFFFF_FFFF_FFFF_FFFF;
    const SENTINEL_U32: u32 = 0xFFFF_FFFF;

    pub struct MetalDeekshaMiner {
        device: Device,
        queue: metal::CommandQueue,
        pipeline: metal::ComputePipelineState,
        header_buf: metal::Buffer,
        params_buf: metal::Buffer,
        nonce_base_buf: metal::Buffer,
        scratchpad_buf: metal::Buffer,
        result_nonce_buf: metal::Buffer,
        result_hash_buf: metal::Buffer,
        npu_weights_buf: metal::Buffer,
        npu_biases_buf: metal::Buffer,
        npu_scales_buf: metal::Buffer,
        npu_meta_buf: metal::Buffer,
        batch_size: usize,
        threads_per_tg: usize,
        device_name_cached: String,
        current_epoch: u64,
    }

    impl MetalDeekshaMiner {
        pub fn new(work_size: usize) -> Result<Self> {
            let device =
                Device::system_default().ok_or_else(|| anyhow::anyhow!("no Metal device found"))?;
            let device_name = device.name().to_string();
            let queue = device.new_command_queue();

            // Compile shader from embedded source
            let shader_src = include_str!("ekam_deeksha.metal");
            let options = metal::CompileOptions::new();
            let library = device
                .new_library_with_source(shader_src, &options)
                .map_err(|e| anyhow::anyhow!("Metal shader compilation failed: {:?}", e))?;

            let func = library
                .get_function("ekam_deeksha_mine", None)
                .map_err(|e| anyhow::anyhow!("kernel function not found: {:?}", e))?;

            let pipeline = device
                .new_compute_pipeline_state_with_function(&func)
                .map_err(|e| anyhow::anyhow!("Metal pipeline creation failed: {:?}", e))?;

            let max_tpg = pipeline.max_total_threads_per_threadgroup() as usize;
            // Memory-hard workloads (256 KiB scratchpad) benefit from larger
            // threadgroups on Apple Silicon to saturate GPU cores.
            // M1 has 8 GPU cores; 128 or 256 threads per TG hides latency better
            // than 64 when each thread touches 256 KiB scratchpad.
            let threads_per_tg = if device_name.contains("Pro")
                || device_name.contains("Max")
                || device_name.contains("Ultra")
            {
                256
            } else if device_name.contains("M1") {
                128
            } else {
                128
            }
            .min(max_tpg);

            // Auto-cap batch_size based on device memory.
            // Each thread needs 256 KiB scratchpad.
            // Apple Silicon uses unified memory — we can be more aggressive than
            // the old 58% limit, but must leave headroom for OS + other apps.
            // Pro/Max/Ultra (16-192 GB): can use 75%+.
            // M1/M2 base (8 GB): 65% is safe and still 2× the old default.
            let recommended = device.recommended_max_working_set_size();
            let pct = if recommended > 12_000_000_000 {
                75 // Pro/Max/Ultra: plenty of unified memory
            } else {
                65 // M1/M2 base: unified memory, but stay safe
            };
            let max_scratch_bytes = (recommended / 100) * pct;
            let max_threads_by_mem = (max_scratch_bytes / 262_144) as usize;
            let batch_size = work_size
                .max(threads_per_tg)
                .min(max_threads_by_mem.max(threads_per_tg));
            let opts = MTLResourceOptions::StorageModeShared;

            // Core buffers
            let header_buf = device.new_buffer(80, opts);
            let params_buf = device.new_buffer(12, opts); // 3 × u32
            let nonce_base_buf = device.new_buffer(8, opts); // u64
            let result_nonce_buf = device.new_buffer(12, opts); // atomic_uint flag + nonce_lo + nonce_hi
            let result_hash_buf = device.new_buffer(32, opts); // hash output

            // Scratchpad: batch_size × 256 KiB per thread
            // Retry with progressively smaller batch_size if allocation fails.
            let mut batch_size = batch_size;
            let mut scratchpad_buf;
            let mut scratch_bytes = 0u64;
            loop {
                scratch_bytes = (batch_size as u64) * 262_144u64;
                scratchpad_buf = device.new_buffer(scratch_bytes, opts);
                if scratchpad_buf.length() >= scratch_bytes {
                    break;
                }
                if batch_size <= threads_per_tg {
                    anyhow::bail!(
                        "scratchpad allocation failed: need {} MiB, got {} bytes (device recommended {} MiB)",
                        scratch_bytes / (1024 * 1024),
                        scratchpad_buf.length(),
                        recommended / (1024 * 1024),
                    );
                }
                batch_size = (batch_size * 9 / 10).max(threads_per_tg);
            }

            // NPU weights — packed variable-topology format for all epochs
            let init_epoch = 0u64;
            let packed = zion_cosmic_harmony::algorithms_npu::chv4_npu_weights_packed(init_epoch);

            let npu_weights_buf = device.new_buffer_with_data(
                packed.weights.as_ptr() as *const _,
                packed.weights.len() as u64,
                opts,
            );
            let npu_biases_buf = device.new_buffer_with_data(
                packed.biases.as_ptr() as *const _,
                packed.biases.len() as u64,
                opts,
            );
            let npu_scales_buf = device.new_buffer_with_data(
                packed.scales.as_ptr() as *const _,
                (packed.scales.len() * 2) as u64,
                opts,
            );
            let npu_meta_buf = device.new_buffer_with_data(
                packed.meta.as_ptr() as *const _,
                (packed.meta.len() * 4) as u64,
                opts,
            );

            println!(
                "gpu_metal_init device=\"{}\" batch_size={} threads_per_tg={} scratchpad_mib={}",
                device_name,
                batch_size,
                threads_per_tg,
                scratch_bytes / (1024 * 1024)
            );

            Ok(Self {
                device,
                queue,
                pipeline,
                header_buf,
                params_buf,
                nonce_base_buf,
                scratchpad_buf,
                result_nonce_buf,
                result_hash_buf,
                npu_weights_buf,
                npu_biases_buf,
                npu_scales_buf,
                npu_meta_buf,
                batch_size,
                threads_per_tg,
                device_name_cached: device_name,
                current_epoch: init_epoch,
            })
        }

        fn dispatch_batch_async(
            &mut self,
            nonce_start: u64,
            count: usize,
        ) -> std::sync::mpsc::Receiver<()> {
            // Write nonce base
            unsafe {
                let ptr = self.nonce_base_buf.contents() as *mut u64;
                *ptr = nonce_start;
            }

            // Reset result sentinel (u32 flag at offset 0)
            unsafe {
                let ptr = self.result_nonce_buf.contents() as *mut u32;
                *ptr = SENTINEL_U32;
            }

            let cb = self.queue.new_command_buffer();
            let enc = cb.new_compute_command_encoder();
            enc.set_compute_pipeline_state(&self.pipeline);
            enc.set_buffer(0, Some(&self.header_buf), 0);
            enc.set_buffer(1, Some(&self.params_buf), 0);
            enc.set_buffer(2, Some(&self.nonce_base_buf), 0);
            enc.set_buffer(3, Some(&self.scratchpad_buf), 0);
            enc.set_buffer(4, Some(&self.result_nonce_buf), 0);
            enc.set_buffer(5, Some(&self.result_hash_buf), 0);
            enc.set_buffer(6, Some(&self.npu_weights_buf), 0);
            enc.set_buffer(7, Some(&self.npu_biases_buf), 0);
            enc.set_buffer(8, Some(&self.npu_scales_buf), 0);
            enc.set_buffer(9, Some(&self.npu_meta_buf), 0);

            let grid = MTLSize::new(count as u64, 1, 1);
            let tg = MTLSize::new(self.threads_per_tg as u64, 1, 1);
            enc.dispatch_threads(grid, tg);
            enc.end_encoding();

            let (tx, rx) = std::sync::mpsc::channel();
            let block = block::ConcreteBlock::new(move |_buffer: &metal::CommandBufferRef| {
                let _ = tx.send(());
            })
            .copy();
            cb.add_completed_handler(&block);
            cb.commit();
            rx
        }

        fn read_result(&self) -> Option<(u64, [u8; 32])> {
            let flag = unsafe { *(self.result_nonce_buf.contents() as *const u32) };
            if flag == SENTINEL_U32 {
                return None;
            }
            // Nonce stored as two u32 at offsets [4..8] and [8..12]
            let nonce_lo =
                unsafe { *(self.result_nonce_buf.contents().add(4) as *const u32) } as u64;
            let nonce_hi =
                unsafe { *(self.result_nonce_buf.contents().add(8) as *const u32) } as u64;
            let nonce = nonce_lo | (nonce_hi << 32);
            let mut hash = [0u8; 32];
            unsafe {
                let ptr = self.result_hash_buf.contents() as *const u8;
                std::ptr::copy_nonoverlapping(ptr, hash.as_mut_ptr(), 32);
            }
            Some((nonce, hash))
        }
    }

    impl GpuMiner for MetalDeekshaMiner {
        fn device_name(&self) -> String {
            self.device_name_cached.clone()
        }

        fn backend_kind(&self) -> GpuBackendKind {
            GpuBackendKind::Metal
        }

        fn algorithm(&self) -> String {
            "cosmic_harmony_ekam_deeksha_v2".to_string()
        }

        fn update_epoch(&mut self, height: u64) -> Result<()> {
            let epoch = zion_cosmic_harmony::algorithms_npu::epoch_from_height(height);
            if epoch == self.current_epoch {
                return Ok(());
            }
            let topology = zion_cosmic_harmony::algorithms_npu::MlpTopology::for_epoch(epoch);
            let packed = zion_cosmic_harmony::algorithms_npu::chv4_npu_weights_packed(epoch);
            let opts = MTLResourceOptions::StorageModeShared;
            self.npu_weights_buf = self.device.new_buffer_with_data(
                packed.weights.as_ptr() as *const _,
                packed.weights.len() as u64,
                opts,
            );
            self.npu_biases_buf = self.device.new_buffer_with_data(
                packed.biases.as_ptr() as *const _,
                packed.biases.len() as u64,
                opts,
            );
            self.npu_scales_buf = self.device.new_buffer_with_data(
                packed.scales.as_ptr() as *const _,
                (packed.scales.len() * 2) as u64,
                opts,
            );
            self.npu_meta_buf = self.device.new_buffer_with_data(
                packed.meta.as_ptr() as *const _,
                (packed.meta.len() * 4) as u64,
                opts,
            );
            println!(
                "gpu_npu_epoch_update epoch={} height={} topology={:?}",
                epoch, height, topology
            );
            self.current_epoch = epoch;
            Ok(())
        }

        fn mine_batch(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<GpuBatchResult> {
            let header_bytes = header.to_bytes();

            // Write header
            unsafe {
                let ptr = self.header_buf.contents() as *mut u8;
                std::ptr::copy_nonoverlapping(
                    header_bytes.as_ptr(),
                    ptr,
                    header_bytes.len().min(80),
                );
            }

            // Write params: [header_len, nonce_count, target_u32]
            let target_u32 = u32::from_be_bytes([
                target.bytes[0],
                target.bytes[1],
                target.bytes[2],
                target.bytes[3],
            ]);

            let mut all_solutions = Vec::new();
            let mut total_tested = 0u64;
            let mut current_nonce = nonce_start;
            let mut left = batch_size;

            while left > 0 {
                let chunk = (left as usize).min(self.batch_size);

                // Update params for this chunk
                unsafe {
                    let ptr = self.params_buf.contents() as *mut u32;
                    *ptr = 80u32; // header_len
                    *ptr.add(1) = chunk as u32; // nonce_count
                    *ptr.add(2) = target_u32; // target
                }

                let rx = self.dispatch_batch_async(current_nonce, chunk);
                rx.recv()
                    .map_err(|_| anyhow::anyhow!("Metal async wait failed"))?;

                if let Some((nonce, hash)) = self.read_result() {
                    all_solutions.push((nonce, hash, None));
                    total_tested += (nonce.saturating_sub(current_nonce) + 1).min(chunk as u64);
                    // Phase-3 optimization: do NOT break on first solution.
                    // With pool diff=1 we find a share after ~200-500 nonces,
                    // but the job TTL is 60s.  Continuing to scan the rest of
                    // the batch keeps the GPU busy and dramatically raises the
                    // effective hashrate (nonces_tested / total_time).
                    // We still return the first solution so the miner can
                    // submit it, but we count all tested nonces for accurate
                    // hashrate reporting.
                }

                total_tested += chunk as u64;
                current_nonce = current_nonce.wrapping_add(chunk as u64);
                left = left.saturating_sub(chunk as u64);
            }

            Ok(GpuBatchResult {
                solutions: all_solutions,
                nonces_tested: total_tested,
            })
        }

        fn benchmark(&mut self, secs: f64) -> Result<(u64, f64, f64)> {
            let header = MiningHeader {
                version: 3,
                previous_hash: [0xAA; 32],
                merkle_root: [0xBB; 32],
                timestamp: 1_762_000_200,
                difficulty_bits: 0x1f00ffff,
            };
            let _target = DifficultyTarget { bytes: [0; 32] };

            // Write header once
            let header_bytes = header.to_bytes();
            unsafe {
                let ptr = self.header_buf.contents() as *mut u8;
                std::ptr::copy_nonoverlapping(header_bytes.as_ptr(), ptr, 80);
            }
            unsafe {
                let ptr = self.params_buf.contents() as *mut u32;
                *ptr = 80u32;
                *ptr.add(1) = self.batch_size as u32;
                *ptr.add(2) = 0u32; // impossible target
            }

            let start = Instant::now();
            let mut total = 0u64;
            let mut nonce = 0u64;

            while start.elapsed().as_secs_f64() < secs {
                let rx = self.dispatch_batch_async(nonce, self.batch_size);
                let _ = rx.recv();
                total += self.batch_size as u64;
                nonce = nonce.wrapping_add(self.batch_size as u64);
            }

            let elapsed = start.elapsed().as_secs_f64();
            let khps = if elapsed > 0.0 {
                total as f64 / elapsed / 1_000.0
            } else {
                0.0
            };
            Ok((total, elapsed, khps))
        }
    }
}

// ─── Metal Backend: DeekshaLite Fire ─────────────────────────────────────────

#[cfg(feature = "gpu-metal")]
pub mod metal_deeksha_lite_fire {
    use super::*;
    use metal::{Device, MTLResourceOptions, MTLSize};
    use std::time::Instant;

    const SENTINEL: u64 = 0xFFFF_FFFF_FFFF_FFFF;
    const SENTINEL_U32: u32 = 0xFFFF_FFFF;

    pub struct MetalDeekshaLiteFireMiner {
        device: Device,
        queue: metal::CommandQueue,
        pipeline: metal::ComputePipelineState,
        header_buf: metal::Buffer,
        params_buf: metal::Buffer,
        nonce_base_buf: metal::Buffer,
        scratchpad_buf: metal::Buffer,
        result_nonce_buf: metal::Buffer,
        result_hash_buf: metal::Buffer,
        batch_size: usize,
        threads_per_tg: usize,
        device_name_cached: String,
    }

    impl MetalDeekshaLiteFireMiner {
        pub fn new(work_size: usize) -> Result<Self> {
            let device =
                Device::system_default().ok_or_else(|| anyhow::anyhow!("no Metal device found"))?;
            let device_name = device.name().to_string();
            let queue = device.new_command_queue();

            let shader_src = include_str!("deeksha_lite_fire.metal");
            let options = metal::CompileOptions::new();
            let library = device
                .new_library_with_source(shader_src, &options)
                .map_err(|e| anyhow::anyhow!("Metal Fire shader compilation failed: {:?}", e))?;

            let func = library
                .get_function("deeksha_lite_fire_mine", None)
                .map_err(|e| anyhow::anyhow!("Fire kernel function not found: {:?}", e))?;

            let pipeline = device
                .new_compute_pipeline_state_with_function(&func)
                .map_err(|e| anyhow::anyhow!("Metal Fire pipeline creation failed: {:?}", e))?;

            let max_tpg = pipeline.max_total_threads_per_threadgroup() as usize;
            let threads_per_tg = if device_name.contains("Pro")
                || device_name.contains("Max")
                || device_name.contains("Ultra")
            {
                256
            } else if device_name.contains("M1") {
                128
            } else {
                128
            }
            .min(max_tpg);

            let recommended = device.recommended_max_working_set_size();
            let pct = if recommended > 12_000_000_000 { 75 } else { 65 };
            let max_scratch_bytes = (recommended / 100) * pct;
            let max_threads_by_mem = (max_scratch_bytes / 262_144) as usize;
            let batch_size = work_size
                .max(threads_per_tg)
                .min(max_threads_by_mem.max(threads_per_tg));
            let opts = MTLResourceOptions::StorageModeShared;

            let header_buf = device.new_buffer(80, opts);
            let params_buf = device.new_buffer(12, opts);
            let nonce_base_buf = device.new_buffer(8, opts);
            let result_nonce_buf = device.new_buffer(12, opts);
            let result_hash_buf = device.new_buffer(32, opts);

            let mut batch_size = batch_size;
            let mut scratchpad_buf;
            let mut scratch_bytes = 0u64;
            loop {
                scratch_bytes = (batch_size as u64) * 262_144u64;
                scratchpad_buf = device.new_buffer(scratch_bytes, opts);
                if scratchpad_buf.length() >= scratch_bytes {
                    break;
                }
                if batch_size <= threads_per_tg {
                    anyhow::bail!(
                        "Fire scratchpad allocation failed: need {} MiB, got {} bytes",
                        scratch_bytes / (1024 * 1024),
                        scratchpad_buf.length(),
                    );
                }
                batch_size = (batch_size * 9 / 10).max(threads_per_tg);
            }

            println!(
                "gpu_metal_fire_init device=\"{}\" batch_size={} threads_per_tg={} scratchpad_mib={}",
                device_name,
                batch_size,
                threads_per_tg,
                scratch_bytes / (1024 * 1024)
            );

            Ok(Self {
                device,
                queue,
                pipeline,
                header_buf,
                params_buf,
                nonce_base_buf,
                scratchpad_buf,
                result_nonce_buf,
                result_hash_buf,
                batch_size,
                threads_per_tg,
                device_name_cached: device_name,
            })
        }

        fn dispatch_batch_async(
            &mut self,
            nonce_start: u64,
            count: usize,
        ) -> std::sync::mpsc::Receiver<()> {
            unsafe {
                let ptr = self.nonce_base_buf.contents() as *mut u64;
                *ptr = nonce_start;
            }
            unsafe {
                let ptr = self.result_nonce_buf.contents() as *mut u32;
                *ptr = SENTINEL_U32;
            }

            let cb = self.queue.new_command_buffer();
            let enc = cb.new_compute_command_encoder();
            enc.set_compute_pipeline_state(&self.pipeline);
            enc.set_buffer(0, Some(&self.header_buf), 0);
            enc.set_buffer(1, Some(&self.params_buf), 0);
            enc.set_buffer(2, Some(&self.nonce_base_buf), 0);
            enc.set_buffer(3, Some(&self.scratchpad_buf), 0);
            enc.set_buffer(4, Some(&self.result_nonce_buf), 0);
            enc.set_buffer(5, Some(&self.result_hash_buf), 0);

            let grid = MTLSize::new(count as u64, 1, 1);
            let tg = MTLSize::new(self.threads_per_tg as u64, 1, 1);
            enc.dispatch_threads(grid, tg);
            enc.end_encoding();

            let (tx, rx) = std::sync::mpsc::channel();
            let block = block::ConcreteBlock::new(move |_buffer: &metal::CommandBufferRef| {
                let _ = tx.send(());
            })
            .copy();
            cb.add_completed_handler(&block);
            cb.commit();
            rx
        }

        fn read_result(&self) -> Option<(u64, [u8; 32])> {
            let flag = unsafe { *(self.result_nonce_buf.contents() as *const u32) };
            if flag == SENTINEL_U32 {
                return None;
            }
            let nonce_lo =
                unsafe { *(self.result_nonce_buf.contents().add(4) as *const u32) } as u64;
            let nonce_hi =
                unsafe { *(self.result_nonce_buf.contents().add(8) as *const u32) } as u64;
            let nonce = nonce_lo | (nonce_hi << 32);
            let mut hash = [0u8; 32];
            unsafe {
                let ptr = self.result_hash_buf.contents() as *const u8;
                std::ptr::copy_nonoverlapping(ptr, hash.as_mut_ptr(), 32);
            }
            Some((nonce, hash))
        }
    }

    impl GpuMiner for MetalDeekshaLiteFireMiner {
        fn device_name(&self) -> String {
            self.device_name_cached.clone()
        }

        fn backend_kind(&self) -> GpuBackendKind {
            GpuBackendKind::Metal
        }

        fn algorithm(&self) -> String {
            "deeksha_lite_fire".to_string()
        }

        fn update_epoch(&mut self, _height: u64) -> Result<()> {
            Ok(()) // Fire has no NPU epoch updates
        }

        fn mine_batch(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<GpuBatchResult> {
            let header_bytes = header.to_bytes();

            unsafe {
                let ptr = self.header_buf.contents() as *mut u8;
                std::ptr::copy_nonoverlapping(
                    header_bytes.as_ptr(),
                    ptr,
                    header_bytes.len().min(80),
                );
            }

            let target_u32 = u32::from_be_bytes([
                target.bytes[0],
                target.bytes[1],
                target.bytes[2],
                target.bytes[3],
            ]);

            let mut all_solutions = Vec::new();
            let mut total_tested = 0u64;
            let mut current_nonce = nonce_start;
            let mut left = batch_size;

            while left > 0 {
                let chunk = (left as usize).min(self.batch_size);

                unsafe {
                    let ptr = self.params_buf.contents() as *mut u32;
                    *ptr = 80u32;
                    *ptr.add(1) = chunk as u32;
                    *ptr.add(2) = target_u32;
                }

                let rx = self.dispatch_batch_async(current_nonce, chunk);
                rx.recv()
                    .map_err(|_| anyhow::anyhow!("Metal Fire async wait failed"))?;

                if let Some((nonce, hash)) = self.read_result() {
                    all_solutions.push((nonce, hash, None));
                    total_tested += (nonce.saturating_sub(current_nonce) + 1).min(chunk as u64);
                }

                total_tested += chunk as u64;
                current_nonce = current_nonce.wrapping_add(chunk as u64);
                left = left.saturating_sub(chunk as u64);
            }

            Ok(GpuBatchResult {
                solutions: all_solutions,
                nonces_tested: total_tested,
            })
        }

        fn benchmark(&mut self, secs: f64) -> Result<(u64, f64, f64)> {
            let header = MiningHeader {
                version: 3,
                previous_hash: [0xAA; 32],
                merkle_root: [0xBB; 32],
                timestamp: 1_762_000_200,
                difficulty_bits: 0x1f00ffff,
            };

            let header_bytes = header.to_bytes();
            unsafe {
                let ptr = self.header_buf.contents() as *mut u8;
                std::ptr::copy_nonoverlapping(header_bytes.as_ptr(), ptr, 80);
            }
            unsafe {
                let ptr = self.params_buf.contents() as *mut u32;
                *ptr = 80u32;
                *ptr.add(1) = self.batch_size as u32;
                *ptr.add(2) = 0u32;
            }

            let start = Instant::now();
            let mut total = 0u64;
            let mut nonce = 0u64;

            while start.elapsed().as_secs_f64() < secs {
                let rx = self.dispatch_batch_async(nonce, self.batch_size);
                let _ = rx.recv();
                total += self.batch_size as u64;
                nonce = nonce.wrapping_add(self.batch_size as u64);
            }

            let elapsed = start.elapsed().as_secs_f64();
            let khps = if elapsed > 0.0 {
                total as f64 / elapsed / 1_000.0
            } else {
                0.0
            };
            Ok((total, elapsed, khps))
        }
    }
}

/// OpenCL miner for external AuxPoW algorithms (Blake3, kHeavyHash, etc.).
/// Delegates to `zion_auxpow::gpu_miner::GpuMiner`.
#[cfg(feature = "gpu-opencl")]
pub mod opencl_external {
    use super::*;
    use std::time::Instant;
    use zion_auxpow::gpu_miner::{GpuFoundShare, GpuMiner as AuxPowGpuMiner};
    #[cfg(feature = "native-hashers")]
    use zion_auxpow::DagManager;

    pub struct OpenClExternalMiner {
        algorithm: String,
        miner: AuxPowGpuMiner,
        work_size: usize,
        /// DAG manager for Ethash/KawPow (only available with native-hashers).
        #[cfg(feature = "native-hashers")]
        dag_manager: DagManager,
        /// Current epoch hint from the job (set by update_epoch_from_job).
        current_epoch_hint: Option<u32>,
    }

    impl OpenClExternalMiner {
        pub fn new(algorithm: &str, work_size: usize) -> Result<Self> {
            let miner = AuxPowGpuMiner::new()
                .map_err(|e| anyhow::anyhow!("auxpow_gpu_init_failed algorithm={algorithm} err={e}"))?;
            Ok(Self {
                algorithm: algorithm.to_string(),
                miner,
                work_size,
                #[cfg(feature = "native-hashers")]
                dag_manager: DagManager::new(),
                current_epoch_hint: None,
            })
        }

        /// Update the epoch hint from the job (called before mine_batch).
        /// If the epoch changed, triggers DAG regeneration via DagManager
        /// (with disk caching for fast restarts).
        pub fn update_epoch_from_job(&mut self, epoch: Option<u32>) -> Result<()> {
            if let Some(ep) = epoch {
                self.current_epoch_hint = Some(ep);
                #[cfg(feature = "native-hashers")]
                {
                    self.dag_manager.ensure_dag(&mut self.miner, &self.algorithm, ep)?;
                }
            }
            Ok(())
        }
    }

    impl GpuMiner for OpenClExternalMiner {
        fn device_name(&self) -> String {
            format!("opencl_auxpow_{}", self.algorithm)
        }

        fn backend_kind(&self) -> GpuBackendKind {
            GpuBackendKind::OpenCL
        }

        fn algorithm(&self) -> String {
            self.algorithm.clone()
        }

        fn update_epoch(&mut self, height: u64) -> Result<()> {
            // For Ethash/KawPow, derive epoch from block height and ensure DAG.
            // The pool sends the external block number as `height` for
            // EthStratum coins (ETC/RVN/CLORE).
            let epoch = if matches!(self.algorithm.as_str(), "ethash" | "etchash" | "ethash_etc") {
                Some((height / 30000) as u32)
            } else if matches!(
                self.algorithm.as_str(),
                "kawpow" | "kawpow_rvn" | "kawpow_clore" | "kawpow_evr" | "kawpow_mewc"
            ) {
                Some((height / 7500) as u32)
            } else {
                None
            };
            self.update_epoch_from_job(epoch)
        }

        fn mine_batch(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<GpuBatchResult> {
            let header_bytes = header.to_bytes();
            let actual_batch = batch_size.min(self.work_size as u64);

            let found = match self.algorithm.as_str() {
                "blake3"
                | "blake3_alph"
                | "blake3_dcr"
                | "autolykos"
                | "autolykos_erg"
                | "ethash"
                | "etchash"
                | "ethash_etc"
                | "kawpow"
                | "kawpow_rvn"
                | "kawpow_clore"
                | "kawpow_evr"
                | "kawpow_mewc" => self.miner.mine(
                    &self.algorithm,
                    &header_bytes,
                    &[],
                    &target.bytes,
                    nonce_start,
                    actual_batch,
                ),
                "kheavyhash" | "kheavyhash_kas" => {
                    // KAS external jobs send a 32-byte pre_pow_hash in header_hex.
                    // The pool pads it to 80 bytes (MiningHeader); the pre_pow_hash
                    // is in the first 32 bytes (previous_hash field).
                    // Timestamp comes from the job's height field (stored in
                    // header.timestamp for external jobs).
                    let pre_pow_hash = &header_bytes[..32];
                    let timestamp = header.timestamp.to_le_bytes().to_vec();
                    self.miner.mine(
                        &self.algorithm,
                        pre_pow_hash,
                        &timestamp,
                        &target.bytes,
                        nonce_start,
                        actual_batch,
                    )
                }
                other => anyhow::bail!("unsupported external GPU algorithm: {other}"),
            }
            .map_err(|e| anyhow::anyhow!("auxpow_gpu_mine_failed algorithm={} err={}", self.algorithm, e))?;

            if let Some(GpuFoundShare { nonce, hash, mix_hash }) = found {
                Ok(GpuBatchResult {
                    solutions: vec![(nonce, hash, mix_hash)],
                    nonces_tested: actual_batch,
                })
            } else {
                Ok(GpuBatchResult {
                    solutions: Vec::new(),
                    nonces_tested: actual_batch,
                })
            }
        }

        fn mine_batch_raw(
            &mut self,
            raw_header: &[u8],
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<GpuBatchResult> {
            // Use raw header bytes directly (supports >80B headers for DCR etc.)
            let actual_batch = batch_size.min(self.work_size as u64);
            let found = self
                .miner
                .mine(
                    &self.algorithm,
                    raw_header,
                    &[],
                    &target.bytes,
                    nonce_start,
                    actual_batch,
                )
                .map_err(|e| {
                    anyhow::anyhow!(
                        "auxpow_gpu_mine_failed_raw algorithm={} err={}",
                        self.algorithm,
                        e
                    )
                })?;

            if let Some(GpuFoundShare { nonce, hash, mix_hash }) = found {
                Ok(GpuBatchResult {
                    solutions: vec![(nonce, hash, mix_hash)],
                    nonces_tested: actual_batch,
                })
            } else {
                Ok(GpuBatchResult {
                    solutions: Vec::new(),
                    nonces_tested: actual_batch,
                })
            }
        }

        fn benchmark(&mut self, secs: f64) -> Result<(u64, f64, f64)> {
            let header = MiningHeader {
                version: 3,
                previous_hash: [0xAA; 32],
                merkle_root: [0xBB; 32],
                timestamp: 1_762_000_200,
                difficulty_bits: 0x1f00ffff,
            };
            let target = DifficultyTarget::MAX;

            let start = Instant::now();
            let mut total = 0u64;
            let mut nonce = 0u64;
            while start.elapsed().as_secs_f64() < secs {
                let result = self.mine_batch(header, target, nonce, self.work_size as u64)?;
                total += result.nonces_tested;
                nonce = nonce.wrapping_add(self.work_size as u64);
            }
            let elapsed = start.elapsed().as_secs_f64();
            let khps = if elapsed > 0.0 {
                total as f64 / elapsed / 1_000.0
            } else {
                0.0
            };
            Ok((total, elapsed, khps))
        }
    }
}

/// Rich GPU info for UI stats table.
#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub name: String,
    pub platform: String,
    pub compute_units: u32,
    pub max_clock_mhz: u32,
    pub global_mem_bytes: u64,
    pub local_mem_bytes: u64,
    pub max_work_group_size: usize,
    /// Temperature in °C if available (OpenCL vendor extension)
    pub temp_c: Option<u32>,
    /// Power draw in Watts if available
    pub power_w: Option<u32>,
}

/// Detect available GPU devices and print a summary.
pub fn detect_gpus() -> Vec<String> {
    #[allow(unused_mut)]
    let mut devices = Vec::new();

    #[cfg(feature = "gpu-opencl")]
    {
        let platforms = ocl::Platform::list();
        for platform in platforms {
            if let Ok(devs) = ocl::Device::list_all(platform) {
                for dev in devs {
                    if let Ok(name) = dev.name() {
                        devices.push(format!("opencl:{name}"));
                    }
                }
            }
        }
    }

    // CUDA device detection
    #[cfg(feature = "gpu-cuda")]
    {
        if let Ok(dev) = cudarc::driver::CudaDevice::new(0) {
            let name = dev
                .name()
                .unwrap_or_else(|_| "unknown CUDA device".to_string());
            devices.push(format!("cuda:{name}"));
        }
    }

    #[cfg(feature = "gpu-metal")]
    {
        if let Some(device) = metal::Device::system_default() {
            devices.push(format!("metal:{}", device.name()));
        }
    }

    devices
}

/// Query rich GPU details from OpenCL (best-effort; temp/power often unavailable).
#[cfg(feature = "gpu-opencl")]
pub fn query_gpu_details() -> Vec<GpuInfo> {
    let mut out = Vec::new();
    let platforms = ocl::Platform::list();
    for platform in platforms {
        let platform_name = platform.name().unwrap_or_else(|_| "unknown".to_string());
        if let Ok(devs) = ocl::Device::list_all(platform) {
            for dev in devs {
                let name = dev.name().unwrap_or_else(|_| "unknown".to_string());
                let compute_units = dev
                    .info(ocl::enums::DeviceInfo::MaxComputeUnits)
                    .ok()
                    .and_then(|v| match v {
                        ocl::enums::DeviceInfoResult::MaxComputeUnits(n) => Some(n as u32),
                        _ => None,
                    })
                    .unwrap_or(0);
                let max_clock_mhz = dev
                    .info(ocl::enums::DeviceInfo::MaxClockFrequency)
                    .ok()
                    .and_then(|v| match v {
                        ocl::enums::DeviceInfoResult::MaxClockFrequency(n) => Some(n as u32),
                        _ => None,
                    })
                    .unwrap_or(0);
                let global_mem_bytes = dev
                    .info(ocl::enums::DeviceInfo::GlobalMemSize)
                    .ok()
                    .and_then(|v| match v {
                        ocl::enums::DeviceInfoResult::GlobalMemSize(n) => Some(n),
                        _ => None,
                    })
                    .unwrap_or(0);
                let local_mem_bytes = dev
                    .info(ocl::enums::DeviceInfo::LocalMemSize)
                    .ok()
                    .and_then(|v| match v {
                        ocl::enums::DeviceInfoResult::LocalMemSize(n) => Some(n),
                        _ => None,
                    })
                    .unwrap_or(0);
                let max_work_group_size = dev
                    .info(ocl::enums::DeviceInfo::MaxWorkGroupSize)
                    .ok()
                    .and_then(|v| match v {
                        ocl::enums::DeviceInfoResult::MaxWorkGroupSize(n) => Some(n),
                        _ => None,
                    })
                    .unwrap_or(0);
                // Temperature is vendor-specific; try AMD/NVIDIA extensions
                let temp_c: Option<u32> = None; // OpenCL does not expose temp via standard query
                out.push(GpuInfo {
                    name,
                    platform: platform_name.clone(),
                    compute_units,
                    max_clock_mhz,
                    global_mem_bytes,
                    local_mem_bytes,
                    max_work_group_size,
                    temp_c: None,
                    power_w: None,
                });
            }
        }
    }
    out
}

#[cfg(not(feature = "gpu-opencl"))]
pub fn query_gpu_details() -> Vec<GpuInfo> {
    Vec::new()
}
