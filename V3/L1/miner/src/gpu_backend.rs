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
use std::time::{SystemTime, UNIX_EPOCH};
use zion_auxpow::external_hashers::hash_blake3;
use zion_core::{DifficultyTarget, MiningHeader, MiningJob, MiningSolution};

#[cfg(feature = "gpu-opencl")]
use crate::gpu_guard::{GpuAlgorithm, GpuDeviceFamily, GpuGuard, GpuTuning};

#[cfg(feature = "gpu-opencl")]
use rayon::prelude::*;

// ── Global GPU memory budget tracker ──────────────────────────────────
// On Apple Silicon (unified memory), GPU and CPU share the same physical
// RAM. Multiple Metal miner instances (Stream 1 + Stream 2) each allocate
// large scratchpad buffers. Without a global budget, two instances can
// together consume >90% of system RAM, causing kernel panics and system
// freezes.
//
// This static atomic tracks the remaining GPU memory budget. Each Metal
// init claims a portion; the budget is computed once at startup from
// total system RAM.
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

static GPU_MEM_BUDGET_BYTES: AtomicU64 = AtomicU64::new(0);
static GPU_MEM_CLAIMED_BYTES: AtomicU64 = AtomicU64::new(0);

/// Initialize the global GPU memory budget using auto-tune.
/// Should be called once at startup. Uses `auto_tune_gpu_budget()`
/// to dynamically calculate a safe budget based on actual available memory.
///
/// Returns true if GPU mining is safe, false if auto-tune killed GPU
/// (available memory too low — CPU only mode).
///
/// `cpu_threads` is the number of CPU mining threads (for safety margin calc).
/// On systems with dedicated VRAM (OpenCL/CUDA), this budget is not used.
pub fn init_gpu_memory_budget_with_threads(cpu_threads: usize) -> bool {
    let budget = auto_tune_gpu_budget(cpu_threads);

    // Kill switch: budget == 0 means GPU is disabled
    if budget == 0 {
        GPU_MEM_BUDGET_BYTES.store(0, AtomicOrdering::SeqCst);
        GPU_MEM_CLAIMED_BYTES.store(0, AtomicOrdering::SeqCst);
        println!(
            "gpu_mem_budget_init DISABLED — auto-tune kill switch active (CPU only mode)"
        );
        return false;
    }

    // Only initialize if not already set (idempotent)
    let prev = GPU_MEM_BUDGET_BYTES.swap(budget, AtomicOrdering::SeqCst);
    if prev != 0 {
        GPU_MEM_BUDGET_BYTES.store(prev, AtomicOrdering::SeqCst);
        return true;
    }
    GPU_MEM_CLAIMED_BYTES.store(0, AtomicOrdering::SeqCst);
    println!(
        "gpu_mem_budget_init budget_mib={} (shared across all GPU streams)",
        budget / (1024 * 1024),
    );
    true
}

/// Reset the GPU memory budget (called on reconnect to clear claimed bytes).
/// This allows the new session to re-claim the full budget.
pub fn reset_gpu_memory_budget() {
    GPU_MEM_CLAIMED_BYTES.store(0, AtomicOrdering::SeqCst);
    // Also clear the budget so auto-tune runs again with fresh available memory
    GPU_MEM_BUDGET_BYTES.store(0, AtomicOrdering::SeqCst);
}

/// Legacy init without CPU thread count — assumes 4 threads.
pub fn init_gpu_memory_budget() -> bool {
    init_gpu_memory_budget_with_threads(4)
}

/// Claim a portion of the GPU memory budget for a Metal miner instance.
/// Returns the maximum scratchpad bytes this instance may allocate.
/// If no budget was initialized, falls back to the device's recommended
/// working set size (legacy behavior).
fn claim_gpu_memory_budget(device_recommended: u64) -> u64 {
    let budget = GPU_MEM_BUDGET_BYTES.load(AtomicOrdering::SeqCst);
    if budget == 0 {
        // Budget not initialized — use legacy per-device calculation
        return device_recommended;
    }

    let claimed = GPU_MEM_CLAIMED_BYTES.load(AtomicOrdering::SeqCst);
    let remaining = budget.saturating_sub(claimed);

    // Each instance gets at most 50% of the total budget.
    // On a two-stream system, each gets half. If only one stream runs,
    // it can use up to 50% (not the full budget — leave headroom).
    let max_per_instance = budget / 2;
    let allocation = remaining.min(max_per_instance);

    // Ensure minimum viable batch (threads_per_tg * 256 KiB = ~32 MiB)
    let min_viable = 32 * 1024 * 1024;
    if allocation < min_viable {
        // Budget exhausted — use minimum viable, NOT device_recommended
        // (device_recommended can be 4GB+ on M1, causing OOM freeze)
        println!(
            "gpu_mem_budget_exhausted budget_mib={} claimed_mib={} remaining_mib={} — using minimum viable {} MiB",
            budget / (1024 * 1024),
            claimed / (1024 * 1024),
            remaining / (1024 * 1024),
            min_viable / (1024 * 1024),
        );
        return min_viable;
    }

    GPU_MEM_CLAIMED_BYTES.fetch_add(allocation, AtomicOrdering::SeqCst);
    println!(
        "gpu_mem_budget_claim budget_mib={} previously_claimed_mib={} this_claim_mib={} total_claimed_mib={}",
        budget / (1024 * 1024),
        claimed / (1024 * 1024),
        allocation / (1024 * 1024),
        (claimed + allocation) / (1024 * 1024),
    );
    allocation
}

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

    /// Async launch: queue kernel work on GPU without waiting for completion.
    /// Returns a token that can be passed to `collect_batch` to retrieve results.
    /// Default implementation: falls back to synchronous mine_batch.
    fn launch_batch(
        &mut self,
        header: MiningHeader,
        target: DifficultyTarget,
        nonce_start: u64,
        batch_size: u64,
    ) -> Result<u64> {
        // Default: no pipelining, just run mine_batch and return a dummy token
        let _ = self.mine_batch(header, target, nonce_start, batch_size)?;
        Ok(0)
    }

    /// Collect results from a previously launched batch.
    /// `token` is the value returned by `launch_batch`.
    /// Default implementation: returns empty result (already collected in launch_batch).
    fn collect_batch(&mut self, _token: u64) -> Result<GpuBatchResult> {
        Ok(GpuBatchResult {
            solutions: Vec::new(),
            nonces_tested: 0,
        })
    }
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
        let all_algos = vec![
            "deeksha_chv3",
            "deeksha_lite_v1",
            "cosmic_harmony_ekam_deeksha_v2",
            "deeksha_lite_fire",
            // External AuxPoW algorithms
            "blake3",
            "kheavyhash",
            "autolykos",
            "zelhash",
            "kawpow",
            "ethash",
            "progpow",
        ];
        // Filter out DAG-based algorithms that the backend cannot safely handle
        // (e.g. Metal on Apple Silicon — unified memory OOM risk)
        let algos: Vec<&str> = all_algos
            .iter()
            .copied()
            .filter(|algo| backend_supports_algorithm(self.kind, algo))
            .collect();
        let skipped: Vec<&str> = all_algos
            .iter()
            .copied()
            .filter(|algo| !backend_supports_algorithm(self.kind, algo))
            .collect();
        if !skipped.is_empty() {
            println!("benchmark_skip_unsafe backend={} algos={:?}", self.kind.as_str(), skipped);
        }
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

/// Simple timestamp helper for TriGpuManager logging.
fn tri_gpu_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

/// Tri-GPU Manager — 3-stream parallel mining GPU context manager.
///
/// Manages the primary GPU backend for the 3-stream architecture.
///
/// In the 3.0.6 multi-stream design, only the **primary** (Stream 1:
/// ZION Deeksha) backend lives here. Pearl (Stream 2) and external GPU
/// streams (Stream 3) each run in their own persistent thread with a
/// dedicated OpenCL context, created via `create_gpu_backend` directly.
/// This avoids OpenCL context/thread-safety issues and keeps the primary
/// Deeksha pipeline isolated and never-switched.
///
/// This is the 3.0.6 canonical replacement for `GpuBackendManager`. The
/// legacy manager remains for backward compatibility.
pub struct TriGpuManager {
    /// Stream 1: ZION Deeksha — created at startup, never switched.
    primary: Option<Box<dyn GpuMiner>>,
    kind: GpuBackendKind,
}

impl TriGpuManager {
    /// Create a new TriGpuManager with the given GPU backend kind.
    /// The primary (Deeksha) backend is created immediately.
    pub fn new(kind: GpuBackendKind, primary_work_size: usize) -> Result<Self> {
        // CPU-only mode: no GPU backend, return a dummy manager.
        if kind == GpuBackendKind::Cpu {
            return Ok(Self {
                primary: None,
                kind,
            });
        }
        let primary_algo = std::env::var("ZION_MINER_ALGORITHM")
            .unwrap_or_else(|_| "deeksha_lite_fire".to_string());
        let primary = create_gpu_backend(kind, primary_work_size, &primary_algo)?;

        Ok(Self {
            primary: Some(primary),
            kind,
        })
    }

    /// Create a TriGpuManager with custom work sizes.
    ///
    /// `pearl_ws` and `secondary_ws` are accepted for backward compatibility
    /// with existing miner configs / environment variables, but are ignored:
    /// Pearl and external GPU streams own their own backends in dedicated
    /// threads and read their work sizes directly from the miner config.
    pub fn with_work_sizes(
        kind: GpuBackendKind,
        primary_ws: usize,
        _pearl_ws: usize,
        _secondary_ws: usize,
    ) -> Result<Self> {
        Self::new(kind, primary_ws)
    }

    /// Access the primary (Stream 1: ZION Deeksha) GPU backend.
    /// This is always available after construction.
    pub fn primary(&mut self) -> Result<&mut dyn GpuMiner> {
        match self.primary.as_mut() {
            Some(b) => Ok(b.as_mut()),
            None => Err(anyhow::anyhow!("primary GPU backend not initialized")),
        }
    }

    /// Primary backend kind string (for hello message / telemetry).
    pub fn primary_backend_kind(&self) -> GpuBackendKind {
        self.kind
    }

    /// Primary GPU device name (for logging).
    pub fn primary_device_name(&self) -> String {
        match self.primary.as_ref() {
            Some(g) => g.device_name(),
            None => "unknown".to_string(),
        }
    }

    /// Set stream weights on the primary backend (Deeksha Chv3 pipeline).
    pub fn set_stream_weights_primary(
        &mut self,
        weights: &zion_cosmic_harmony::stream_profit::StreamWeights,
    ) -> Result<()> {
        if let Some(ref mut g) = self.primary {
            g.set_stream_weights(weights)?;
        }
        Ok(())
    }
}

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
            | "evrprogpow"
            | "evrprogpow_evr"
            | "meowpow"
            | "meowpow_mewc"
            | "ethash"
            | "etchash"
            | "ethash_etc"
            | "zelhash"
            | "zelhash_flux"
            | "progpow"
            | "progpow_epic"
            | "beamhash"
            | "beamhash_beam"
            | "verushash"
            | "randomx"
            | "eaglesong" | "eaglesong_ckb"
            | "octopus" | "octopus_cfx"
            | "equihash" | "equihash_zec"
            | "neoscrypt" | "neoscrypt_phx"
    )
}

/// CPU-only algorithms that have no GPU kernel and must use CPU mining.
/// VerusHash v2.2 is designed to be GPU-resistant (AES-NI + CLHash).
/// RandomX is designed to be GPU/ASIC-resistant.
/// GhostRider (RTM) OpenCL kernel is a placeholder — real hashing via
/// native-ghostrider FFI (sphlib + CryptoNight) on CPU only.
pub fn is_cpu_only_algorithm(algorithm: &str) -> bool {
    matches!(
        algorithm,
        "verushash" | "randomx" | "ghostrider" | "ghostrider_rtm"
    )
}

/// DAG-based algorithms that require a large (~1-4 GB) DAG buffer on GPU.
/// These are dangerous on Metal (Apple Silicon unified memory) because
/// allocating a 2GB+ DAG can OOM the system and cause a kernel freeze.
pub fn is_dag_based_algorithm(algorithm: &str) -> bool {
    matches!(
        algorithm,
        "progpow"
            | "progpow_epic"
            | "ethash"
            | "etchash"
            | "ethash_etc"
            | "kawpow"
            | "kawpow_rvn"
            | "kawpow_clore"
            | "kawpow_evr"
            | "kawpow_mewc"
            | "kawpow_quai"
            | "evrprogpow"
            | "evrprogpow_evr"
            | "meowpow"
            | "meowpow_mewc"
    )
}

/// Memory-hard algorithms that are NOT DAG-based but still need large GPU
/// memory buffers (Equihash variants). These are unsafe on Metal with
/// limited unified memory, just like DAG-based algorithms.
pub fn is_memory_hard_algorithm(algorithm: &str) -> bool {
    matches!(
        algorithm,
        "zelhash" | "zelhash_flux" | "beamhash" | "beamhash_beam"
    )
}

/// Estimate the GPU memory (in bytes) needed for an algorithm, beyond the
/// standard scratchpad allocation. This includes DAG buffers, Equihash
/// state, Autolykos tables, etc.
///
/// Returns 0 for lightweight algorithms (blake3, kheavyhash) that only
/// need the standard per-thread scratchpad.
pub fn algorithm_extra_gpu_memory_bytes(algorithm: &str, height: u64) -> u64 {
    // DAG-based: DAG size = 1 GB + epoch × 8 MB
    if is_dag_based_algorithm(algorithm) {
        let epoch_divisor = match algorithm {
            "kawpow" | "kawpow_rvn" | "kawpow_clore" | "kawpow_evr"
            | "kawpow_mewc" | "kawpow_quai"
            | "evrprogpow" | "evrprogpow_evr"
            | "meowpow" | "meowpow_mewc" => 7500u64,
            "progpow" | "progpow_epic"
            | "ethash" | "etchash" | "ethash_etc" => 30000u64,
            _ => 30000u64,
        };
        let epoch = height / epoch_divisor;
        return 1024 * 1024 * 1024 + epoch * 8 * 1024 * 1024;
    }

    // Memory-hard (Equihash): ~1.3 GB for zelhash, ~1 GB for beamhash
    if is_memory_hard_algorithm(algorithm) {
        return match algorithm {
            "zelhash" | "zelhash_flux" => 1300 * 1024 * 1024, // ~1.3 GB
            "beamhash" | "beamhash_beam" => 1024 * 1024 * 1024, // ~1 GB
            _ => 1024 * 1024 * 1024,
        };
    }

    // Autolykos: 64 MB default, 512 MB mainnet (based on epoch)
    if algorithm == "autolykos" || algorithm == "autolykos_erg" {
        let epoch = height / 45000;
        // Table size grows: 2^23 (64MB) at epoch 0, up to 2^26 (512MB)
        let table_size = if epoch < 10 {
            64 * 1024 * 1024 // 64 MB
        } else {
            512 * 1024 * 1024 // 512 MB mainnet
        };
        return table_size;
    }

    // Lightweight: blake3, kheavyhash — no extra memory needed
    0
}

/// Check if an algorithm is safe to run on a given GPU backend with a given
/// memory budget. This is a per-algorithm, per-system check that replaces
/// the old blanket "disable Stream 2 on ≤8GB" guard.
///
/// `available_gpu_budget_bytes` is the remaining GPU memory budget after
/// Stream 1 has claimed its share.
pub fn algorithm_fits_gpu_budget(
    backend: GpuBackendKind,
    algorithm: &str,
    height: u64,
    available_gpu_budget_bytes: u64,
) -> bool {
    let resolved = match backend {
        GpuBackendKind::Auto => resolve_auto_backend(),
        other => other,
    };

    // CPU backend: no GPU algorithms
    if resolved == GpuBackendKind::Cpu {
        return false;
    }

    // CPU-only algorithms (verushash, randomx) — never on GPU
    if is_cpu_only_algorithm(algorithm) {
        return false;
    }

    // On Metal (unified memory): check if algorithm's memory needs fit
    if resolved == GpuBackendKind::Metal {
        // DAG-based and memory-hard algorithms are always blocked on Metal
        // (they need 1+ GB extra which is too much for unified memory)
        if is_dag_based_algorithm(algorithm) || is_memory_hard_algorithm(algorithm) {
            return false;
        }

        // Autolykos: check if the table fits in the remaining budget
        let extra = algorithm_extra_gpu_memory_bytes(algorithm, height);
        if extra > available_gpu_budget_bytes {
            return false;
        }

        // Lightweight algorithms (blake3, kheavyhash): always safe
        return true;
    }

    // OpenCL / CUDA: dedicated VRAM — check if algorithm fits in GPU VRAM
    // For dual-stream mining, each stream gets ~45% of VRAM (10% for driver).
    // DAG-based algorithms need DAG + scratchpad; check if both fit.
    let gpu_vram = detect_gpu_vram_bytes();
    if gpu_vram > 0 {
        let extra = algorithm_extra_gpu_memory_bytes(algorithm, height);
        // Each stream gets at most 45% of VRAM (leave 10% for driver/overhead)
        let per_stream_budget = (gpu_vram * 45) / 100;
        if extra > per_stream_budget {
            return false;
        }
        // Also check the provided budget (may be smaller for dual-stream)
        if extra > available_gpu_budget_bytes {
            return false;
        }
    }

    true
}

/// Detect GPU VRAM (dedicated video memory) in bytes.
/// Returns 0 if no dedicated GPU is found (e.g. Metal/unified memory).
/// For Metal, returns 0 (unified memory — use system RAM budget instead).
pub fn detect_gpu_vram_bytes() -> u64 {
    let gpus = query_gpu_details();
    if gpus.is_empty() {
        return 0;
    }
    // Return VRAM of the first (primary) GPU
    gpus[0].global_mem_bytes
}

/// Detect GPU compute units (CUs) of the primary GPU.
/// Returns 0 if no GPU is detected.
pub fn detect_gpu_compute_units() -> u32 {
    let gpus = query_gpu_details();
    if gpus.is_empty() {
        return 0;
    }
    gpus[0].compute_units
}

/// Round `n` up to the next power of two (or itself if already a power of two).
fn next_pow2(n: usize) -> usize {
    if n <= 1 {
        return 1;
    }
    let p = (n as u64).next_power_of_two() as usize;
    if p / 2 >= n {
        p / 2 // round to nearest, preferring lower
    } else {
        p
    }
}

/// Round `n` to the nearest power of two.
fn nearest_pow2(n: usize) -> usize {
    if n <= 1 {
        return 1;
    }
    let up = (n as u64).next_power_of_two() as usize;
    let down = up / 2;
    if n - down <= up - n {
        down
    } else {
        up
    }
}

/// Hardware-autotuned mining parameters.
///
/// Detects GPU compute units, GPU VRAM, CPU cores, and system RAM, then
/// computes optimal work sizes and thread count for maximum throughput.
///
/// ## Benchmark-derived heuristics
///
/// **gpu_work_size** (ZION deeksha, Stream 1):
///   - Scales with GPU compute units: `nearest_pow2(CUs * 512)`
///   - 18 CUs (RX 5700 XT) → 8192 ✓ (benchmarked optimal)
///   - 10 CUs (M2) → 4096
///   - 32 CUs (M4 Max) → 16384
///   - Clamped to [1024, 65536]
///
/// **secondary_gpu_work_size** (ProgPow/KawPow/Ethash, Stream 2):
///   - Scales with GPU VRAM: `clamp(VRAM_GB * 0.75M, 1M, 8M)`
///   - 6 GB (RX 5700 XT) → 4M ✓ (benchmarked optimal)
///   - 8 GB → 6M
///   - 16 GB → 8M (capped)
///   - Unified memory → 2M (conservative)
///
/// **threads** (CPU mining, Stream 3 VerusHash/RandomX):
///   - Use all logical cores (benchmarks show T=all wins for total throughput)
///   - On systems with ≥8 cores, reserve 0 threads (GPU driver has spare cycles)
///   - Minimum 1, maximum 64
pub struct AutoTuneResult {
    /// GPU work size for ZION deeksha (Stream 1)
    pub gpu_work_size: usize,
    /// GPU work size for ProgPow/KawPow (Stream 2)
    pub secondary_gpu_work_size: usize,
    /// CPU thread count for VerusHash/RandomX (Stream 3)
    pub threads: usize,
    /// Optimal nonce batch size for VerusHash CPU mining
    pub verushash_nonce_count: u64,
    /// Detected GPU name (for logging)
    pub gpu_name: String,
    /// Detected GPU compute units
    pub gpu_compute_units: u32,
    /// Detected GPU VRAM in bytes (0 = unified memory / no GPU)
    pub gpu_vram_bytes: u64,
    /// Detected system RAM in bytes
    pub sys_ram_bytes: u64,
    /// Detected CPU logical cores
    pub cpu_cores: usize,
    /// Detected CPU physical cores
    pub cpu_physical_cores: usize,
    /// Detected CPU vendor string
    pub cpu_vendor: String,
    /// Detected CPU model string
    pub cpu_model: String,
    /// Whether a dedicated GPU was detected
    pub has_gpu: bool,
}

// ── CPU detection ─────────────────────────────────────────────────────

/// Detect CPU vendor and model string.
/// On Linux reads /proc/cpuinfo. On macOS uses sysctl. On Windows uses wmic.
fn detect_cpu_info() -> (String, String, usize, usize) {
    let logical = num_cpus::get().max(1);

    // Physical cores: try to detect, fallback to logical/2 (typical SMT)
    let physical = detect_physical_cores().unwrap_or((logical + 1) / 2).max(1);

    #[cfg(target_os = "linux")]
    {
        let mut detected_vendor = String::new();
        let mut detected_model = String::new();
        if let Ok(content) = std::fs::read_to_string("/proc/cpuinfo") {
            for line in content.lines() {
                if let Some(val) = line.strip_prefix("vendor_id\t: ") {
                    if detected_vendor.is_empty() {
                        detected_vendor = val.to_string();
                    }
                } else if let Some(val) = line.strip_prefix("model name\t: ") {
                    if detected_model.is_empty() {
                        detected_model = val.to_string();
                    }
                }
            }
        }
        if !detected_vendor.is_empty() || !detected_model.is_empty() {
            return (detected_vendor, detected_model, physical, logical);
        }
    }

    #[cfg(target_os = "macos")]
    {
        let vendor = "Apple".to_string();
        let model = std::process::Command::new("sysctl")
            .arg("-n")
            .arg("machdep.cpu.brand_string")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "Apple Silicon".to_string());
        return (vendor, model, physical, logical);
    }

    #[cfg(target_os = "windows")]
    {
        let model = std::process::Command::new("wmic")
            .args(&["cpu", "get", "name"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.lines().nth(1).unwrap_or("").trim().to_string())
            .unwrap_or_default();
        let vendor = if model.contains("Intel") {
            "GenuineIntel".to_string()
        } else if model.contains("AMD") {
            "AuthenticAMD".to_string()
        } else if model.contains("Apple") {
            "Apple".to_string()
        } else {
            "unknown".to_string()
        };
        return (vendor, model, physical, logical);
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = physical; // suppress unused on non-target platforms
        return ("unknown".to_string(), "unknown".to_string(), physical, logical);
    }

    // Fallback (only reached on Linux if /proc/cpuinfo parsing failed)
    ("unknown".to_string(), "unknown".to_string(), physical, logical)
}

/// Detect physical CPU cores (not logical/SMT threads).
fn detect_physical_cores() -> Option<usize> {
    #[cfg(target_os = "linux")]
    {
        // Read /proc/cpuinfo and count unique "core id" values per "physical id"
        if let Ok(content) = std::fs::read_to_string("/proc/cpuinfo") {
            use std::collections::HashSet;
            let mut cores: HashSet<(String, String)> = HashSet::new();
            let mut current_physical = String::new();
            let mut current_core = String::new();
            for line in content.lines() {
                if line.is_empty() {
                    // New CPU entry
                    if !current_physical.is_empty() && !current_core.is_empty() {
                        cores.insert((current_physical.clone(), current_core.clone()));
                    }
                    current_physical.clear();
                    current_core.clear();
                } else if let Some(val) = line.strip_prefix("physical id\t: ") {
                    current_physical = val.to_string();
                } else if let Some(val) = line.strip_prefix("core id\t: ") {
                    current_core = val.to_string();
                }
            }
            // Last entry
            if !current_physical.is_empty() && !current_core.is_empty() {
                cores.insert((current_physical, current_core));
            }
            if !cores.is_empty() {
                return Some(cores.len());
            }
        }
        // Fallback: lscpu
        if let Ok(output) = std::process::Command::new("lscpu")
            .args(&["-p=Core"])
            .output()
        {
            if let Ok(text) = String::from_utf8(output.stdout) {
                use std::collections::HashSet as StdHashSet;
                let count = text
                    .lines()
                    .filter(|l| !l.starts_with('#'))
                    .filter(|l| !l.trim().is_empty())
                    .collect::<StdHashSet<_>>()
                    .len();
                if count > 0 {
                    return Some(count);
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("sysctl")
            .args(&["-n", "hw.physicalcpu"])
            .output()
        {
            if let Ok(text) = String::from_utf8(output.stdout) {
                if let Ok(n) = text.trim().parse::<usize>() {
                    return Some(n);
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = std::process::Command::new("wmic")
            .args(&["cpu", "get", "NumberOfCores"])
            .output()
        {
            if let Ok(text) = String::from_utf8(output.stdout) {
                if let Some(n) = text.lines().nth(1).and_then(|s| s.trim().parse::<usize>().ok()) {
                    return Some(n);
                }
            }
        }
    }

    None
}

/// CPU architecture profile for VerusHash tuning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuArch {
    /// AMD Zen (Ryzen/EPYC) — SMT, high IPC, AES-NI + CLMUL
    AmdZen,
    /// Intel Core (Sandy Bridge through Raptor Lake) — HT, AES-NI + CLMUL
    IntelCore,
    /// Apple Silicon (M1-M5) — unified memory, ARM AES
    AppleSilicon,
    /// Other/unknown — conservative defaults
    Other,
}

/// Auto-tune VerusHash CPU parameters based on detected CPU.
///
/// Returns (threads, nonce_count) optimized for VerusHash v2.2 two-stage mining.
///
/// Benchmarks (2026-07-16, fixupkey + batch C++ scan):
///   Ryzen 5 3600 (6C/12T):  T=12, N=5M → 13.0 MH/s peak
///   Ryzen 5 3600 (6C/12T):  T=10, N=1M → 11.9 MH/s
///   T=6 (physical only):    ~4.1 MH/s (SMT helps a lot for VerusHash)
///   T=14+ (oversubscribe):  degrades (cache contention)
///
/// Rules:
///   - VerusHash benefits from SMT (logical > physical) — use all logical cores
///   - But not more than physical+6 (oversubscription degrades due to 8.8KB key per thread)
///   - nonce_count: 5M for ≥8 threads, 2M for 4-7, 1M for ≤3
///   - Apple Silicon: fewer threads (unified memory with GPU)
fn auto_tune_verushash(physical: usize, logical: usize, arch: CpuArch, has_gpu: bool) -> (usize, u64) {
    let (threads, nonce_count) = match arch {
        CpuArch::AmdZen => {
            // AMD Zen: SMT helps, use all logical cores but cap at physical+6
            // to avoid L3 cache thrashing (each thread needs ~8.8KB CLHash key)
            let t = logical.min(physical + 6).max(1);
            let n = if t >= 8 { 5_000_000 } else if t >= 4 { 2_000_000 } else { 1_000_000 };
            (t, n)
        }
        CpuArch::IntelCore => {
            // Intel HT also helps, similar to AMD SMT
            let t = logical.min(physical + 4).max(1);
            let n = if t >= 8 { 5_000_000 } else if t >= 4 { 2_000_000 } else { 1_000_000 };
            (t, n)
        }
        CpuArch::AppleSilicon => {
            // Apple Silicon: unified memory, GPU competes for bandwidth
            // Use physical cores - 1 (leave 1 for OS + GPU driver)
            let t = if has_gpu { physical.saturating_sub(1).max(2) } else { physical };
            let n = if t >= 6 { 5_000_000 } else if t >= 3 { 2_000_000 } else { 1_000_000 };
            (t, n)
        }
        CpuArch::Other => {
            // Conservative: use physical cores only
            let t = physical.max(1);
            let n = if t >= 8 { 5_000_000 } else if t >= 4 { 2_000_000 } else { 1_000_000 };
            (t, n)
        }
    };
    (threads.min(64), nonce_count)
}

/// Classify CPU based on vendor and model string.
fn classify_cpu(vendor: &str, model: &str) -> CpuArch {
    let v = vendor.to_lowercase();
    let m = model.to_lowercase();

    if v.contains("apple") || m.contains("apple m") || m.contains("apple silicon") {
        return CpuArch::AppleSilicon;
    }
    if v.contains("amd") || m.contains("amd ryzen") || m.contains("amd epic")
        || m.contains("ryzen") || m.contains("epyc")
    {
        // Check for Zen architecture (all modern AMD CPUs are Zen)
        if m.contains("ryzen") || m.contains("epyc") || m.contains("threadripper") {
            return CpuArch::AmdZen;
        }
        return CpuArch::AmdZen; // default AMD = Zen
    }
    if v.contains("intel") || m.contains("intel") || m.contains("core i")
        || m.contains("xeon") || m.contains("pentium") || m.contains("celeron")
    {
        return CpuArch::IntelCore;
    }
    CpuArch::Other
}

/// Auto-detect hardware and compute optimal mining parameters.
///
/// This is the main entry point for hardware-based autotuning.
/// Called at miner startup when env vars are not explicitly set.
pub fn auto_tune_work_sizes() -> AutoTuneResult {
    let gpus = query_gpu_details();
    let sys_ram = detect_system_memory_bytes();

    // ── CPU detection ──
    let (cpu_vendor, cpu_model, cpu_physical_cores, cpu_cores) = detect_cpu_info();
    let cpu_arch = classify_cpu(&cpu_vendor, &cpu_model);

    let has_gpu = !gpus.is_empty();
    let gpu_info = gpus.first();
    let gpu_name = gpu_info
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "none".to_string());
    let gpu_compute_units = gpu_info.map(|g| g.compute_units).unwrap_or(0);
    let gpu_vram_bytes = gpu_info.map(|g| g.global_mem_bytes).unwrap_or(0);

    // ── gpu_work_size: ZION deeksha (Stream 1) ──
    // Formula: nearest_pow2(CUs * 512), clamped to [1024, 65536]
    // Benchmark: 18 CUs → 8192 (optimal on RX 5700 XT)
    let gpu_work_size = if gpu_compute_units > 0 {
        let raw = (gpu_compute_units as usize) * 512;
        nearest_pow2(raw).clamp(1024, 65536)
    } else {
        // No GPU — CPU mode, use default 256K
        1 << 18
    };

    // ── secondary_gpu_work_size: ProgPow/KawPow (Stream 2) ──
    // Formula: clamp(round(VRAM_MiB * 0.75 / 1024) * 1M, 1M, 8M)
    // Benchmark: 6128 MiB → 4596 → 4596/1024 = 4 → 4M (optimal on RX 5700 XT)
    let secondary_gpu_work_size = if gpu_vram_bytes > 0 {
        let vram_mib = gpu_vram_bytes / (1024 * 1024);
        // 0.75 of VRAM in MiB, then convert to M-units (1M = 1<<20 bytes)
        // vram_mib / 1024 ≈ vram_gb, then * 3/4 = 0.75 * vram_gb
        // Benchmark: 6128 MiB → 6128*3/(4*1024) = 4 → 4M (optimal on RX 5700 XT)
        let target_m_units = (vram_mib * 3) / (4 * 1024);
        let m_units = (target_m_units as usize).clamp(1, 8);
        m_units * (1 << 20)
    } else if has_gpu {
        // Unified memory (Metal) — conservative 2M
        2 << 20
    } else {
        // No GPU — CPU mode, use default 256K
        1 << 18
    };

    // ── threads + nonce_count: CPU mining (Stream 3, VerusHash) ──
    // Auto-tuned per CPU architecture (AMD Zen, Intel, Apple Silicon, Other)
    let (threads, verushash_nonce_count) =
        auto_tune_verushash(cpu_physical_cores, cpu_cores, cpu_arch, has_gpu);

    // Log CPU detection for diagnostics
    eprintln!(
        "[auto-tune] CPU: {} \"{}\" | physical={} logical={} arch={:?} | threads={} nonce_count={}",
        cpu_vendor, cpu_model, cpu_physical_cores, cpu_cores, cpu_arch, threads, verushash_nonce_count
    );

    AutoTuneResult {
        gpu_work_size,
        secondary_gpu_work_size,
        threads,
        verushash_nonce_count,
        gpu_name,
        gpu_compute_units,
        gpu_vram_bytes,
        sys_ram_bytes: sys_ram,
        cpu_cores,
        cpu_physical_cores,
        cpu_vendor,
        cpu_model,
        has_gpu,
    }
}

/// Resolve `Auto` to the concrete backend that will actually be used on this
/// platform. This is critical for memory safety: on macOS with only
/// `gpu-metal` compiled, `Auto` falls through OpenCL → CUDA → Metal, so it
/// effectively IS Metal. Without this resolution, the DAG-algorithm guard
/// would be bypassed (Auto was grouped with OpenCL/CUDA), causing system
/// freezes from unified-memory OOM.
#[allow(unreachable_code)]
pub fn resolve_auto_backend() -> GpuBackendKind {
    // Check which GPU features are compiled, in priority order
    #[cfg(feature = "gpu-opencl")]
    {
        // On macOS, OpenCL is deprecated and often unavailable even if compiled
        #[cfg(target_os = "macos")]
        {
            // Try to detect if OpenCL is actually available
            // On Apple Silicon, OpenCL is not available — fall through to Metal
            if !std::env::var("ZION_FORCE_OPENCL").is_ok() {
                #[cfg(feature = "gpu-metal")]
                {
                    return GpuBackendKind::Metal;
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            return GpuBackendKind::OpenCL;
        }
    }

    #[cfg(feature = "gpu-cuda")]
    {
        return GpuBackendKind::Cuda;
    }

    #[cfg(feature = "gpu-metal")]
    {
        return GpuBackendKind::Metal;
    }

    GpuBackendKind::Cpu
}

/// Detect total system physical memory in bytes.
/// Used to compute a safe GPU memory budget on unified-memory systems
/// (Apple Silicon) where GPU and CPU share the same RAM.
pub fn detect_system_memory_bytes() -> u64 {
    #[cfg(target_os = "macos")]
    {
        // sysctl hw.memsize returns total physical RAM on macOS
        if let Ok(out) = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout);
                if let Ok(bytes) = s.trim().parse::<u64>() {
                    return bytes;
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // /proc/meminfo → MemTotal (kB)
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    let kb: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                    if let Ok(kb_val) = kb.parse::<u64>() {
                        return kb_val * 1024;
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Use GlobalMemoryStatusEx via std::process on Windows
        if let Ok(out) = std::process::Command::new("wmic")
            .args(["ComputerSystem", "get", "TotalPhysicalMemory", "/value"])
            .output()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            for line in s.lines() {
                if let Some(rest) = line.strip_prefix("TotalPhysicalMemory=") {
                    if let Ok(bytes) = rest.trim().parse::<u64>() {
                        return bytes;
                    }
                }
            }
        }
    }

    // Fallback: assume 8 GB if detection fails
    8 * 1024 * 1024 * 1024
}

/// Detect currently AVAILABLE memory (free + inactive/purgeable + cached).
/// This is the memory that can actually be allocated without causing
/// swap storms or OOM freezes. Critical for unified-memory systems.
///
/// On macOS: uses `vm_stat` to compute free + inactive + purgeable pages.
/// On Linux: reads `/proc/meminfo` → MemAvailable.
/// On Windows: uses `wmic OS get FreePhysicalMemory`.
pub fn detect_available_memory_bytes() -> u64 {
    #[cfg(target_os = "macos")]
    {
        // vm_stat reports page counts; page size is typically 16384 on Apple Silicon
        let page_size = get_macos_page_size();
        if let Ok(out) = std::process::Command::new("vm_stat")
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout);
                let mut free: u64 = 0;
                let mut inactive: u64 = 0;
                let mut purgeable: u64 = 0;
                let mut speculative: u64 = 0;
                for line in s.lines() {
                    let count = parse_vm_stat_line(line);
                    if line.starts_with("Pages free:") {
                        free = count;
                    } else if line.starts_with("Pages inactive:") {
                        inactive = count;
                    } else if line.starts_with("Pages purgeable:") {
                        purgeable = count;
                    } else if line.starts_with("Pages speculative:") {
                        speculative = count;
                    }
                }
                // Available = free + inactive + purgeable + speculative
                // (inactive pages can be reclaimed, purgeable can be discarded)
                let avail_pages = free + inactive + purgeable + speculative;
                return avail_pages * page_size;
            }
        }
        // Fallback: assume 25% of total is available
        return detect_system_memory_bytes() / 4;
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("MemAvailable:") {
                    let kb: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                    if let Ok(kb_val) = kb.parse::<u64>() {
                        return kb_val * 1024;
                    }
                }
            }
        }
        return detect_system_memory_bytes() / 4;
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(out) = std::process::Command::new("wmic")
            .args(["OS", "get", "FreePhysicalMemory", "/value"])
            .output()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            for line in s.lines() {
                if let Some(rest) = line.strip_prefix("FreePhysicalMemory=") {
                    if let Ok(kb) = rest.trim().parse::<u64>() {
                        return kb * 1024;
                    }
                }
            }
        }
        return detect_system_memory_bytes() / 4;
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        detect_system_memory_bytes() / 4
    }
}

/// Get macOS VM page size (typically 16384 on Apple Silicon, 4096 on Intel)
#[cfg(target_os = "macos")]
fn get_macos_page_size() -> u64 {
    if let Ok(out) = std::process::Command::new("vm_stat")
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        // First line: "Mach Virtual Memory Statistics: (page size of 16384 bytes)"
        if let Some(start) = s.find("page size of ") {
            let rest = &s[start + 13..];
            if let Some(end) = rest.find(" bytes") {
                if let Ok(ps) = rest[..end].parse::<u64>() {
                    return ps;
                }
            }
        }
    }
    16384 // Default for Apple Silicon
}

/// Parse a vm_stat line and extract the page count.
/// Lines look like: "Pages free:                             73652."
#[cfg(target_os = "macos")]
fn parse_vm_stat_line(line: &str) -> u64 {
    // Find the number after the colon
    if let Some(colon_pos) = line.find(':') {
        let rest = &line[colon_pos + 1..];
        let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u64>() {
            return n;
        }
    }
    0
}

/// Auto-tune GPU memory budget based on actual system state.
///
/// This replaces the old fixed-percentage approach with a dynamic calculation
/// tuned per Apple Silicon generation (M1–M5) and per system RAM.
///
/// **Apple Silicon tuning table:**
///
/// | Model  | RAM    | GPU CUs | Max GPU budget | Notes                     |
/// |--------|--------|---------|----------------|---------------------------|
/// | M1     | 8 GB   | 7–8     | 600 MB         | Tightest — OS needs 5 GB  |
/// | M1     | 16 GB  | 8       | 1800 MB        | Comfortable               |
/// | M2     | 8 GB   | 10      | 600 MB         | Same RAM constraint as M1 |
/// | M2     | 16 GB  | 10      | 2000 MB        | More CUs, more budget     |
/// | M3     | 8 GB   | 10      | 600 MB         | Same RAM constraint       |
/// | M3     | 16 GB  | 10      | 2200 MB        | Better memory bandwidth   |
/// | M4     | 16 GB  | 10      | 2400 MB        | Best efficiency per watt  |
/// | M4 Pro | 24 GB  | 16      | 4000 MB        | 16 CUs, plenty of RAM     |
/// | M4 Max | 36 GB  | 32      | 7000 MB        | 32 CUs, huge RAM          |
/// | M5     | TBD    | TBD     | TBD            | Expected late 2025/2026   |
///
/// **Linux/OpenCL (dedicated GPU):** Uses system RAM for scratchpad budget,
/// GPU VRAM for algorithm check (separate path via `detect_gpu_vram_bytes`).
///
/// `cpu_threads` is the number of CPU mining threads (VerusHash/RandomX).
pub fn auto_tune_gpu_budget(cpu_threads: usize) -> u64 {
    let total_ram = detect_system_memory_bytes();
    let available = detect_available_memory_bytes();
    let total_mib = total_ram / (1024 * 1024);
    let avail_mib = available / (1024 * 1024);

    // ── Detect Apple Silicon model (M1–M5) ──
    let (chip_model, gpu_cores) = detect_apple_chip();

    // ── Hard kill switch: if available < 200 MB, GPU is too risky ──
    // On macOS, <200 MB available means the system is already under severe
    // memory pressure. Any GPU allocation can trigger a kernel freeze.
    // Return 0 to signal "disable GPU, CPU only".
    if avail_mib < 200 {
        println!(
            "gpu_auto_tune KILL_SWITCH available_mib={} < 200 — disabling GPU (CPU only mode). \
             System under severe memory pressure.",
            avail_mib,
        );
        return 0;
    }

    // ── Per-model max budget (hard cap based on chip + RAM) ──
    // These are empirically safe values that leave enough for OS + CPU mining.
    let max_budget_mib: u64 = match (&chip_model[..], total_mib) {
        // ── 8 GB RAM: tightest constraint (any M-chip) ──
        (_, 8192) => {
            // 8 GB total, OS needs ~4.5 GB, CPU mining ~0.8 GB, app ~0.2 GB
            // Safe GPU budget: ~600 MB (leaves ~1.5 GB headroom)
            600
        }

        // ── 12 GB RAM (M2/M3 base) ──
        (_, 12288) => 1200,

        // ── 16 GB RAM ──
        ("M1", 16384) => 1800,
        ("M2", 16384) => 2000,
        ("M3", 16384) => 2200,
        ("M4", 16384) => 2400,
        (_, 16384) => 2000, // unknown M-chip with 16GB

        // ── 24 GB RAM (M4 Pro) ──
        ("M4", 24576) => 4000,
        (_, 24576) => 3500,

        // ── 32 GB RAM ──
        (_, 32768) => 5500,

        // ── 36 GB RAM (M4 Max) ──
        ("M4", 36864) => 7000,
        (_, 36864) => 6000,

        // ── 64 GB RAM (M4 Max upgraded) ──
        (_, 65536) => 12000,

        // ── 96 GB / 128 GB (M4 Max / M5 Ultra) ──
        (_, 98304) => 18000,
        (_, 131072) => 24000,

        // ── Linux with dedicated GPU: use 30% of system RAM ──
        // (GPU scratchpad lives in system RAM, DAG lives in VRAM)
        _ if !cfg!(target_os = "macos") => (total_ram * 30) / 100 / (1024 * 1024),

        // ── Fallback: 15% of total RAM ──
        _ => (total_ram * 15) / 100 / (1024 * 1024),
    };

    // ── Available-based budget ──
    // Use a ratio that depends on how much is available:
    //   <500 MB available: 30% (very conservative — system is stressed)
    //   <1000 MB available: 40% (cautious)
    //   <2000 MB available: 50% (normal)
    //   ≥2000 MB available: 60% (comfortable)
    let avail_ratio: u64 = if avail_mib < 500 {
        30
    } else if avail_mib < 1000 {
        40
    } else if avail_mib < 2000 {
        50
    } else {
        60
    };
    let avail_based_mib = (avail_mib * avail_ratio) / 100;

    // ── CPU thread adjustment ──
    // VerusHash: ~50-80 MB per thread (hash state + buffers)
    // RandomX: ~200 MB per thread (dataset + cache, but shared dataset)
    // Use 75 MB per thread as a safe average. On 8GB with 4 threads = 300 MB.
    let cpu_adj_mib = (cpu_threads as u64) * 75;

    // ── Floor: minimum viable budget ──
    // On 8 GB: 64 MB floor (tiny but functional)
    // On ≥16 GB: 128 MB floor
    let floor_mib: u64 = if total_mib <= 8192 { 64 } else { 128 };

    // ── Final budget calculation ──
    // 1. Start with max(floor, avail_based)
    // 2. Cap at per-model max_budget
    // 3. Subtract CPU thread adjustment
    let raw_budget = avail_based_mib.max(floor_mib);
    let budget_mib = raw_budget.min(max_budget_mib).saturating_sub(cpu_adj_mib);

    // Ensure minimum viable (32 MB for a tiny scratchpad)
    let budget_mib = budget_mib.max(32);

    // ── Manual override ──
    // ZION_GPU_MEM_BUDGET_MIB allows the user to bypass the auto-tune
    // calculation entirely. Useful on Apple Silicon where the auto-tune
    // is too conservative (e.g. 8 GB M1 with 8 CPU threads → 32 MiB).
    // The override is still capped at max_budget_mib for safety.
    let budget_mib = std::env::var("ZION_GPU_MEM_BUDGET_MIB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(|v| v.min(max_budget_mib).max(32))
        .unwrap_or(budget_mib);

    let budget = budget_mib * 1024 * 1024;

    println!(
        "gpu_auto_tune chip={} gpu_cores={} sys_ram_mib={} available_mib={} avail_ratio={}{} cpu_threads={} cpu_adj_mib={} max_budget_mib={} floor_mib={} => budget_mib={}{}",
        chip_model,
        gpu_cores,
        total_mib,
        avail_mib,
        avail_ratio,
        "%",
        cpu_threads,
        cpu_adj_mib,
        max_budget_mib,
        floor_mib,
        budget_mib,
        std::env::var("ZION_GPU_MEM_BUDGET_MIB").ok().map(|_| " (override)").unwrap_or(""),
    );

    budget
}

/// Detect Apple Silicon chip model and GPU core count.
/// Returns (model_name, gpu_core_count).
/// On non-Apple platforms, returns ("Unknown", 0).
fn detect_apple_chip() -> (String, u32) {
    #[cfg(target_os = "macos")]
    {
        // sysctl machdep.cpu.brand_string returns e.g. "Apple M1", "Apple M2 Pro"
        if let Ok(out) = std::process::Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
        {
            if out.status.success() {
                let brand = String::from_utf8_lossy(&out.stdout).trim().to_string();

                // Parse model: "Apple M1" → "M1", "Apple M2 Pro" → "M2", "Apple M4 Max" → "M4"
                let model = if brand.starts_with("Apple M") {
                    // "Apple M" is 7 chars; rest is "1", "2 Pro", "4 Max", etc.
                    let rest = &brand["Apple M".len()..];
                    // Take digits (e.g. "1", "2", "3", "4", "5")
                    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if digits.is_empty() {
                        "Unknown".to_string()
                    } else {
                        format!("M{}", digits)
                    }
                } else {
                    "Unknown".to_string()
                };

                // Detect GPU core count from system_profiler
                let gpu_cores = detect_apple_gpu_cores();

                return (model, gpu_cores);
            }
        }
        ("Unknown".to_string(), 0)
    }

    #[cfg(not(target_os = "macos"))]
    {
        ("Unknown".to_string(), 0)
    }
}

/// Detect GPU core count on Apple Silicon.
#[cfg(target_os = "macos")]
fn detect_apple_gpu_cores() -> u32 {
    // system_profiler SPHardwareDataType shows "Total Number of Cores: 8 (4 Performance and 4 Efficiency)"
    // But GPU cores are different. Try SPDisplaysDataType.
    if let Ok(out) = std::process::Command::new("system_profiler")
        .args(["SPDisplaysDataType"])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        // Look for "Total Number of Cores: N" in the GPU section
        for line in s.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("Total Number of Cores:") {
                let rest = &trimmed["Total Number of Cores:".len()..];
                let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                if let Ok(n) = digits.parse::<u32>() {
                    // This is total cores (CPU+GPU on Apple Silicon).
                    // GPU cores = total - CPU cores.
                    // M1 8-core: 4P+4E CPU + 7-8 GPU = 15-16 total
                    // But system_profiler may report differently.
                    // Just return the number as a hint.
                    return n;
                }
            }
        }
    }
    0
}

#[cfg(not(target_os = "macos"))]
fn detect_apple_gpu_cores() -> u32 {
    0
}

/// Check if a GPU backend can safely handle an algorithm.
///
/// Metal (Apple Silicon) has unified memory — allocating large buffers
/// (DAG, Equihash state) can cause system freezes. Skip unsafe algorithms.
/// `Auto` is resolved to its concrete backend before checking.
pub fn backend_supports_algorithm(backend: GpuBackendKind, algorithm: &str) -> bool {
    let resolved = match backend {
        GpuBackendKind::Auto => resolve_auto_backend(),
        other => other,
    };
    match resolved {
        GpuBackendKind::Metal => {
            // Metal on Apple Silicon: skip DAG-based AND memory-hard algorithms
            // to prevent system freezes from unified memory OOM.
            if is_dag_based_algorithm(algorithm) || is_memory_hard_algorithm(algorithm) {
                return false;
            }
            // Non-DAG algorithms are safe on Metal
            true
        }
        GpuBackendKind::OpenCL | GpuBackendKind::Cuda => {
            // OpenCL and CUDA have dedicated VRAM — check if DAG/memory-hard
            // algorithms fit in the available VRAM.
            let gpu_vram = detect_gpu_vram_bytes();
            if gpu_vram > 0 {
                // For DAG-based: need at least 1.5 GB for DAG + scratchpad
                // For memory-hard: need at least 1.3 GB
                let min_needed = if is_dag_based_algorithm(algorithm) {
                    1536 * 1024 * 1024 // 1.5 GB minimum (epoch 0 DAG + scratchpad)
                } else if is_memory_hard_algorithm(algorithm) {
                    1400 * 1024 * 1024 // 1.4 GB for Equihash
                } else {
                    0 // Lightweight algorithms always fit
                };
                if min_needed > 0 && min_needed > gpu_vram / 2 {
                    // VRAM too small for this algorithm in dual-stream mode
                    // (each stream gets half the VRAM)
                    return false;
                }
            }
            true
        }
        GpuBackendKind::Auto => {
            // Should not reach here after resolve_auto_backend, but be safe
            true
        }
        GpuBackendKind::Cpu => {
            // CPU backend doesn't support any GPU algorithms
            false
        }
    }
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
                // External AuxPoW algorithms — try CUDA kernel first, then CPU fallback
                if is_external_algorithm(algorithm) {
                    // Algorithms with dedicated CUDA kernels
                    if crate::cuda_external::CudaExtAlgo::from_name(algorithm).is_some() {
                        match crate::cuda_external::CudaExternalMiner::new(algorithm, work_size) {
                            Ok(miner) => return Ok(Box::new(miner)),
                            Err(e) => {
                                eprintln!("[gpu_backend] CUDA external kernel failed for {}: {} — falling back to CPU", algorithm, e);
                            }
                        }
                    }
                    // Fall back to CPU for algorithms without CUDA kernels
                    // (ethash, kawpow, progpow, beamhash, eaglesong, octopus, equihash, neoscrypt)
                    #[cfg(feature = "native-kheavyhash")]
                    {
                        eprintln!("[gpu_backend] CUDA CPU fallback for algorithm={}", algorithm);
                        let miner = crate::gpu_backend::cpu_external_fallback::CpuExternalMiner::new(algorithm, work_size)?;
                        return Ok(Box::new(miner));
                    }
                    #[cfg(not(feature = "native-kheavyhash"))]
                    {
                        anyhow::bail!("External algorithm '{}' on CUDA requires native-kheavyhash feature", algorithm);
                    }
                }
                let miner: Box<dyn GpuMiner> = if algorithm == "deeksha_lite_fire" {
                    Box::new(cuda_deeksha_lite_fire::CudaDeekshaLiteFireMiner::new(work_size)?)
                } else if algorithm == "deeksha_lite_v1" || algorithm == "deeksha_chv3" {
                    Box::new(cuda_deeksha_lite::CudaDeekshaLiteMiner::new(work_size)?)
                } else {
                    Box::new(cuda_deeksha::CudaDeekshaMiner::new(work_size)?)
                };
                return Ok(miner);
            }
            #[cfg(not(feature = "gpu-cuda"))]
            anyhow::bail!("CUDA support not compiled — rebuild with --features gpu-cuda");
        }
        GpuBackendKind::Metal => {
            #[cfg(feature = "gpu-metal")]
            {
                // External AuxPoW algorithms (kheavyhash, blake3, etc.) have
                // no Metal kernel. Fall back to CPU via native-ffi.
                if is_external_algorithm(algorithm) {
                    #[cfg(feature = "native-kheavyhash")]
                    {
                        eprintln!("[gpu_backend] Metal CPU fallback for algorithm={}", algorithm);
                        let miner = crate::gpu_backend::cpu_external_fallback::CpuExternalMiner::new(algorithm, work_size)?;
                        return Ok(Box::new(miner));
                    }
                    #[cfg(not(feature = "native-kheavyhash"))]
                    {
                        anyhow::bail!("External algorithm '{}' on Metal requires native-kheavyhash feature", algorithm);
                    }
                }
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

    // For Ethash/KawPow/ProgPow, derive the epoch from the block height and ensure
    // the DAG is loaded.  The pool sends the external block number as
    // job.height for EthStratum coins (ETC/RVN/CLORE/EPIC).
    if is_external_algorithm(algorithm)
        && matches!(
            algorithm,
            "ethash" | "etchash" | "ethash_etc"
                | "kawpow" | "kawpow_rvn" | "kawpow_clore"
                | "kawpow_evr" | "kawpow_mewc"
                | "progpow" | "progpow_epic"
        )
    {
        let epoch = if matches!(algorithm, "ethash" | "etchash" | "ethash_etc" | "progpow" | "progpow_epic") {
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

    // Cap the batch size to avoid stale jobs.  With the batched launch
    // optimization (all chunks launched back-to-back, single sync at end),
    // we can safely process larger batches.  Default 262144 = 8× work_size
    // for deeksha_lite_fire, which takes ~3s on RTX 3090 — well within the
    // 60s job TTL.  Override with ZION_GPU_MAX_BATCH env var.
    let max_batch = std::env::var("ZION_GPU_MAX_BATCH")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(262_144);
    let effective_batch = job.nonce_count.min(max_batch);

    let result = if use_raw {
        gpu.mine_batch_raw(raw_header_bytes, job.target, job.start_nonce, effective_batch)
    } else {
        gpu.mine_batch(effective_header, job.target, job.start_nonce, effective_batch)
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

// ─── Pipeline State for overlapping pool I/O with GPU compute ──────────────

/// State for pipelined GPU mining: collect previous batch while launching next.
///
/// Usage:
/// ```ignore
/// let mut pipeline = GpuPipelineState::new();
/// loop {
///     let job = read_next_job();
///     // Collect previous batch results (if any), launch new batch (async)
///     let prev_result = pipeline.step(gpu, job, algorithm, &raw_header_bytes);
///     // Submit previous solution (overlaps with GPU computing current batch)
///     if let Some(outcome) = prev_result {
///         submit_solution(outcome);
///     }
/// }
/// // After loop: collect final batch
/// let final_result = pipeline.collect(gpu, algorithm, &raw_header_bytes);
/// ```
pub struct GpuPipelineState {
    /// Previous job's data, needed to process collected results.
    prev_job: Option<MiningJob>,
    prev_raw_header: Option<Vec<u8>>,
    /// Whether a batch is currently pending on the GPU.
    has_pending: bool,
}

impl GpuPipelineState {
    pub fn new() -> Self {
        Self {
            prev_job: None,
            prev_raw_header: None,
            has_pending: false,
        }
    }

    /// Collect previous batch (if any) and launch new batch (async).
    /// Returns the previous batch's GpuScanOutcome, or None if first iteration.
    pub fn step(
        &mut self,
        gpu: &mut dyn GpuMiner,
        job: MiningJob,
        algorithm: &str,
        raw_header_bytes: &[u8],
    ) -> Option<GpuScanOutcome> {
        let prev_outcome = if self.has_pending {
            // Collect previous batch results
            let prev_job = self.prev_job.take()?;
            let prev_raw = self.prev_raw_header.take().unwrap_or_default();
            let collect_result = gpu.collect_batch(0);

            // Process collected results using previous job's data
            let outcome = match collect_result {
                Ok(result) => {
                    let nonces_tested = result.nonces_tested;
                    if let Some((nonce, gpu_hash, mix_hash)) = result.solutions.first() {
                        let mix_hash = *mix_hash;
                        let candidate = zion_core::BlockCandidate {
                            header: prev_job.header,
                            nonce: *nonce,
                            height: prev_job.height,
                        };
                        let cpu_hash = candidate.hash_with_algorithm(algorithm);
                        let is_mismatch = cpu_hash != *gpu_hash;
                        let gpu_above_target = !prev_job.target.allows(gpu_hash);

                        if gpu_above_target {
                            GpuScanOutcome {
                                solution: None,
                                mix_hash,
                                nonces_tested,
                                candidates_found: 1,
                                candidates_verified: 0,
                                candidates_hash_mismatch: if is_mismatch { 1 } else { 0 },
                                candidates_above_target: 1,
                            }
                        } else {
                            GpuScanOutcome {
                                solution: Some(MiningSolution {
                                    job_id: prev_job.job_id,
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
                    eprintln!("gpu_pipeline_collect_error: {e}");
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
            };
            Some(outcome)
        } else {
            None
        };

        // Launch new batch (async)
        let max_batch = std::env::var("ZION_GPU_MAX_BATCH")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(262_144);
        let effective_batch = job.nonce_count.min(max_batch);

        let mut effective_header = job.header;
        if is_external_algorithm(algorithm) {
            effective_header.timestamp = job.height;
        }

        let use_raw = is_external_algorithm(algorithm)
            && !algorithm.starts_with("kheavyhash")
            && raw_header_bytes.len() > 80;

        let launch_result: Result<(), String> = if use_raw {
            // For raw headers, we need to use mine_batch_raw which is synchronous.
            // Fall back to synchronous mine_batch for raw algorithms.
            gpu.mine_batch_raw(raw_header_bytes, job.target, job.start_nonce, effective_batch)
                .map(|_| ())
                .map_err(|e| e.to_string())
        } else {
            gpu.launch_batch(effective_header, job.target, job.start_nonce, effective_batch)
                .map(|_| ())
                .map_err(|e| e.to_string())
        };

        if let Err(e) = launch_result {
            eprintln!("gpu_pipeline_launch_error: {e}");
            self.has_pending = false;
        } else {
            self.has_pending = true;
            self.prev_job = Some(job);
            self.prev_raw_header = Some(raw_header_bytes.to_vec());
        }

        prev_outcome
    }

    /// Collect the final pending batch (call after the loop ends).
    pub fn collect(
        &mut self,
        gpu: &mut dyn GpuMiner,
        algorithm: &str,
    ) -> Option<GpuScanOutcome> {
        if !self.has_pending {
            return None;
        }
        self.has_pending = false;
        let prev_job = self.prev_job.take()?;
        let prev_raw = self.prev_raw_header.take().unwrap_or_default();
        let _ = prev_raw; // unused for non-raw algorithms

        let collect_result = gpu.collect_batch(0);

        match collect_result {
            Ok(result) => {
                let nonces_tested = result.nonces_tested;
                if let Some((nonce, gpu_hash, mix_hash)) = result.solutions.first() {
                    let mix_hash = *mix_hash;
                    let candidate = zion_core::BlockCandidate {
                        header: prev_job.header,
                        nonce: *nonce,
                        height: prev_job.height,
                    };
                    let gpu_above_target = !prev_job.target.allows(gpu_hash);
                    if gpu_above_target {
                        return Some(GpuScanOutcome {
                            solution: None,
                            mix_hash,
                            nonces_tested,
                            candidates_found: 1,
                            candidates_verified: 0,
                            candidates_hash_mismatch: 0,
                            candidates_above_target: 1,
                        });
                    }
                    Some(GpuScanOutcome {
                        solution: Some(MiningSolution {
                            job_id: prev_job.job_id,
                            candidate,
                            hash: *gpu_hash,
                        }),
                        mix_hash,
                        nonces_tested,
                        candidates_found: 1,
                        candidates_verified: 1,
                        candidates_hash_mismatch: 0,
                        candidates_above_target: 0,
                    })
                } else {
                    Some(GpuScanOutcome {
                        solution: None,
                        mix_hash: None,
                        nonces_tested,
                        candidates_found: 0,
                        candidates_verified: 0,
                        candidates_hash_mismatch: 0,
                        candidates_above_target: 0,
                    })
                }
            }
            Err(e) => {
                eprintln!("gpu_pipeline_final_collect_error: {e}");
                None
            }
        }
    }
}

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
        /// FIX #9: Pending batch result from launch_batch, returned by collect_batch.
        pending: Option<GpuBatchResult>,
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
                pending: None,
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

        /// FIX #9: Override default launch_batch which discards mine_batch results.
        fn launch_batch(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<u64> {
            if self.pending.is_some() {
                self.pending = None;
            }
            let result = self.mine_batch(header, target, nonce_start, batch_size)?;
            self.pending = Some(result);
            Ok(0)
        }

        /// FIX #9: Return the pending batch result stored by launch_batch.
        fn collect_batch(&mut self, _token: u64) -> Result<GpuBatchResult> {
            self.pending
                .take()
                .ok_or_else(|| anyhow::anyhow!("no pending OpenCL deeksha batch to collect"))
        }
    }
}

// ─── OpenCL DeekshaLite Backend (simplified, no NPU) ────────────────────────

#[cfg(feature = "gpu-opencl")]
pub mod opencl_deeksha_lite {
    use super::*;
    use ocl::builders::ProgramBuilder;
    use ocl::{Buffer, Device, Event, Kernel, Platform, ProQue, Queue};
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
        /// Second output buffer for double-buffered async readback.
        output_hashes_buf_b: Buffer<u8>,
        /// Dedicated read queue — allows GPU compute (on main queue) to overlap
        /// with DMA readback (on this queue), hiding read latency.
        read_queue: Queue,
        stream_weights_buf: Buffer<f32>,
        work_size: usize,
        local_work_size: usize,
        device_name_cached: String,
        device_family: GpuDeviceFamily,
        tuning: GpuTuning,
        recovery_attempts: u32,
        max_recovery_attempts: u32,
        /// FIX #9: Pending batch result from launch_batch, returned by collect_batch.
        pending: Option<GpuBatchResult>,
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

            // Allow ZION_OCL_LOCAL_SIZE to override auto-tuned local_ws
            let local_ws = std::env::var("ZION_OCL_LOCAL_SIZE")
                .ok()
                .and_then(|v| v.trim().parse::<usize>().ok())
                .map(|v| v.clamp(32, 512))
                .unwrap_or(tuning.local_ws);

            println!(
                "gpu_opencl_lite_init family={:?} device=\"{}\" vram={}MiB tuned_ws={} local_ws={} build_opts=\"{}\"",
                family,
                device_name,
                vram / (1024 * 1024),
                actual_work_size,
                local_ws,
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
            let output_hashes_buf_b = Buffer::<u8>::builder()
                .queue(q.clone())
                .len(actual_work_size * 32)
                .build()?;
            // Dedicated read queue for double-buffered async readback.
            // Using a separate queue allows the GPU to execute the next kernel
            // on the compute queue while a buffer read is in-flight on this queue.
            let read_queue = Queue::new(
                &pro_que.context(),
                pro_que.queue().device(),
                None,
            )?;
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
                local_ws,
                actual_work_size * DL_SCRATCHPAD_BYTES / (1024 * 1024)
            );
            Ok(Self {
                pro_que,
                kernel,
                header_state_buf,
                scratchpad_buf,
                output_hashes_buf,
                output_hashes_buf_b,
                read_queue,
                stream_weights_buf,
                work_size: actual_work_size,
                local_work_size: local_ws,
                device_name_cached: device_name,
                device_family: family,
                tuning,
                recovery_attempts: 0,
                max_recovery_attempts: 3,
                pending: None,
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
            // ZION_GPU_NO_STREAM_BYPRODUCT=1 disables the stream byproduct work
            // in the kernel (extra keccak/AES/SHA3 calls per hash). This can
            // improve hashrate by 20-30% on GPUs where the byproduct work is
            // a significant overhead relative to the base hash.
            let weights = if std::env::var("ZION_GPU_NO_STREAM_BYPRODUCT")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false)
            {
                &zion_cosmic_harmony::stream_profit::StreamWeights::default()
            } else {
                weights
            };
            let arr = stream_weights_f32(weights);
            self.stream_weights_buf.write(&arr[..]).enq()?;
            self.pro_que.queue().finish()?;
            if std::env::var("ZION_QUIET").map(|v| v == "1").unwrap_or(false) {
                // suppressed in quiet/sticky mode
            } else {
                println!("gpu_opencl_lite_stream_weights {}", weights.describe());
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

            // Check if double-buffering is disabled (env override for debugging)
            let double_buffer_disabled = std::env::var("ZION_GPU_NO_DOUBLE_BUFFER")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

            // Early-break: return after the first chunk that finds a solution.
            // On GCN/Vega, each kernel launch has ~200ms driver overhead. With
            // low pool difficulty, every chunk finds a solution, so processing
            // the full batch wastes 7×200ms=1.4s of overhead. Early-break
            // launches only 1 kernel (8192 nonces in ~400ms = 20 KH/s) instead
            // of 8 kernels (65536 nonces in ~4900ms = 13 KH/s).
            // Default: false (multi-chunk batches + double-buffering enabled).
            // Set ZION_GPU_EARLY_BREAK=1 to force single-chunk (safe path).
            let early_break = std::env::var("ZION_GPU_EARLY_BREAK")
                .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
                .unwrap_or(false);

            let mut all_solutions = Vec::new();
            let mut total_tested = 0u64;
            let mut current_nonce = nonce_start;
            // Cap batch to single chunk when early_break is enabled.
            // This forces the single-buffer path, which is safe (no
            // pending DMA events to drop). The double-buffered path
            // with early_break causes heap corruption (use-after-free
            // when pending read events/buffers are dropped on return).
            let mut left = if early_break {
                batch_size.min(self.work_size as u64)
            } else {
                batch_size
            };

            // ── Double-buffered async readback path ──────────────────────
            // Uses two output buffers (A/B) and a dedicated read queue.
            // While the GPU computes chunk N+1 on the compute queue, the CPU
            // processes chunk N's results from the read queue. This hides the
            // DMA readback latency behind GPU compute time.
            if !double_buffer_disabled && left > self.work_size as u64 {
                let out_bufs = [&self.output_hashes_buf, &self.output_hashes_buf_b];
                let mut host_a = vec![0u8; self.work_size * 32];
                let mut host_b = vec![0u8; self.work_size * 32];
                let mut buf_idx = 0usize;

                let mut prev_read_event: Option<Event> = None;
                let mut prev_chunk = 0usize;
                let mut prev_nonce = 0u64;
                let mut prev_buf_idx = 0usize;

                while left > 0 {
                    let chunk = (left as usize).min(self.work_size);
                    let local_size = self.local_work_size.min(chunk);
                    let global_size = ((chunk + local_size - 1) / local_size) * local_size;
                    let out_buf = out_bufs[buf_idx];

                    self.kernel.set_arg(1, current_nonce)?;
                    self.kernel.set_arg(2, chunk as u32)?;
                    self.kernel.set_arg(3, out_buf)?;

                    // Enqueue kernel on compute queue (non-blocking)
                    let mut k_event = Event::empty();
                    {
                        let guard = GpuGuard::new();
                        unsafe {
                            self.kernel
                                .cmd()
                                .global_work_size(global_size)
                                .local_work_size(local_size)
                                .enew(&mut k_event)
                                .enq()?;
                        }
                        if guard.was_caught() {
                            self.recovery_attempts += 1;
                            anyhow::bail!(
                                "GPU access violation during kernel enqueue (attempt {}/{}). AMD driver crash detected.",
                                self.recovery_attempts,
                                self.max_recovery_attempts
                            );
                        }
                    }

                    // Enqueue async read on read queue (depends on kernel event)
                    let mut r_event = Event::empty();
                    {
                        let guard = GpuGuard::new();
                        let dst = if buf_idx == 0 { &mut host_a[..] } else { &mut host_b[..] };
                        unsafe {
                            out_buf
                                .read(&mut dst[..chunk * 32])
                                .queue(&self.read_queue)
                                .ewait(&k_event)
                                .enew(&mut r_event)
                                .block(false)
                                .enq()?;
                        }
                        if guard.was_caught() {
                            self.recovery_attempts += 1;
                            anyhow::bail!(
                                "GPU access violation during async hash buffer read (attempt {}/{}). AMD driver crash detected.",
                                self.recovery_attempts,
                                self.max_recovery_attempts
                            );
                        }
                    }

                    // Flush compute queue so GPU starts immediately
                    let _ = self.pro_que.queue().flush();

                    // Wait for PREVIOUS read to complete and process its results
                    // (GPU is computing current chunk in parallel)
                    if let Some(prev_ev) = prev_read_event.take() {
                        {
                            let guard = GpuGuard::new();
                            prev_ev.wait_for()?;
                            if guard.was_caught() {
                                self.recovery_attempts += 1;
                                anyhow::bail!(
                                    "GPU access violation during read event wait (attempt {}/{}). AMD driver crash detected.",
                                    self.recovery_attempts,
                                    self.max_recovery_attempts
                                );
                            }
                        }
                        let prev_host = if prev_buf_idx == 0 { &host_a[..] } else { &host_b[..] };
                        for i in 0..prev_chunk {
                            let hash: [u8; 32] =
                                prev_host[i * 32..(i + 1) * 32].try_into().unwrap();
                            if target.allows(&hash) {
                                let nonce = prev_nonce.wrapping_add(i as u64);
                                all_solutions.push((nonce, hash, None));
                                break;
                            }
                        }
                        total_tested += prev_chunk as u64;
                        // Early-break: wait for the pending chunk 1 read
                        // to complete before returning, to avoid
                        // use-after-free (the GPU DMA is writing to
                        // host_b which would be dropped on return).
                        if early_break {
                            r_event.wait_for()?;
                            return Ok(GpuBatchResult {
                                nonces_tested: total_tested,
                                solutions: all_solutions,
                            });
                        }
                    }

                    prev_read_event = Some(r_event);
                    prev_chunk = chunk;
                    prev_nonce = current_nonce;
                    prev_buf_idx = buf_idx;
                    current_nonce = current_nonce.wrapping_add(chunk as u64);
                    left -= chunk as u64;
                    buf_idx = 1 - buf_idx;
                }

                // Process the last pending read
                if let Some(prev_ev) = prev_read_event.take() {
                    {
                        let guard = GpuGuard::new();
                        prev_ev.wait_for()?;
                        if guard.was_caught() {
                            self.recovery_attempts += 1;
                            anyhow::bail!(
                                "GPU access violation during final read event wait (attempt {}/{}). AMD driver crash detected.",
                                self.recovery_attempts,
                                self.max_recovery_attempts
                            );
                        }
                    }
                    let prev_host = if prev_buf_idx == 0 { &host_a[..] } else { &host_b[..] };
                    for i in 0..prev_chunk {
                        let hash: [u8; 32] =
                            prev_host[i * 32..(i + 1) * 32].try_into().unwrap();
                        if target.allows(&hash) {
                            let nonce = prev_nonce.wrapping_add(i as u64);
                            all_solutions.push((nonce, hash, None));
                            break;
                        }
                    }
                    total_tested += prev_chunk as u64;
                }

                Ok(GpuBatchResult {
                    nonces_tested: total_tested,
                    solutions: all_solutions,
                })
            } else {
                // ── Simple single-buffer path (small batches or fallback) ──
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
                            break; // first match in this chunk
                        }
                    }
                    total_tested += chunk as u64;
                    if early_break {
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
            // Use a batch size larger than work_size to exercise double-buffered
            // async readback. Default 4× work_size; override with ZION_GPU_BENCH_BATCH.
            let batch_multiplier: u64 = std::env::var("ZION_GPU_BENCH_BATCH")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(4);
            let batch_size = self.work_size as u64 * batch_multiplier;
            let start = Instant::now();
            let mut total_hashes = 0u64;
            let mut nonce_start = 0u64;
            while start.elapsed().as_secs_f64() < secs {
                let result = self.mine_batch(header, target, nonce_start, batch_size)?;
                total_hashes += result.nonces_tested;
                nonce_start = nonce_start.wrapping_add(batch_size);
            }
            let elapsed = start.elapsed().as_secs_f64();
            let khps = if elapsed > 0.0 {
                total_hashes as f64 / elapsed / 1_000.0
            } else {
                0.0
            };
            Ok((total_hashes, elapsed, khps))
        }

        /// FIX #9: Override default launch_batch which discards mine_batch results.
        fn launch_batch(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<u64> {
            if self.pending.is_some() {
                self.pending = None;
            }
            let result = self.mine_batch(header, target, nonce_start, batch_size)?;
            self.pending = Some(result);
            Ok(0)
        }

        /// FIX #9: Return the pending batch result stored by launch_batch.
        fn collect_batch(&mut self, _token: u64) -> Result<GpuBatchResult> {
            self.pending
                .take()
                .ok_or_else(|| anyhow::anyhow!("no pending OpenCL lite batch to collect"))
        }
    }
}

// ─── OpenCL DeekshaLite Fire Backend (thermal-intensive) ───────────────────

#[cfg(feature = "gpu-opencl")]
pub mod opencl_deeksha_lite_fire {
    use super::*;
    use ocl::builders::ProgramBuilder;
    use ocl::{Buffer, Device, Event, Kernel, Platform, ProQue, Queue};
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
        /// Second output buffer for double-buffered async readback.
        output_hashes_buf_b: Buffer<u8>,
        /// Dedicated read queue for async readback overlap.
        read_queue: Queue,
        stream_weights_buf: Buffer<f32>,
        work_size: usize,
        local_work_size: usize,
        device_name_cached: String,
        device_family: GpuDeviceFamily,
        tuning: GpuTuning,
        recovery_attempts: u32,
        max_recovery_attempts: u32,
        /// FIX #9: Pending batch result from launch_batch, returned by collect_batch.
        /// Without this, the trait default discards mine_batch results → gpu_hps=0, tested=0.
        pending: Option<GpuBatchResult>,
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

            // Allow ZION_OCL_LOCAL_SIZE to override auto-tuned local_ws
            let local_ws = std::env::var("ZION_OCL_LOCAL_SIZE")
                .ok()
                .and_then(|v| v.trim().parse::<usize>().ok())
                .map(|v| v.clamp(32, 512))
                .unwrap_or(tuning.local_ws);

            println!(
                "gpu_opencl_fire_init family={:?} device=\"{}\" vram={}MiB tuned_ws={} local_ws={} build_opts=\"{}\"",
                family,
                device_name,
                vram / (1024 * 1024),
                actual_work_size,
                local_ws,
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
            let output_hashes_buf_b = Buffer::<u8>::builder()
                .queue(q.clone())
                .len(actual_work_size * 32)
                .build()?;
            let read_queue = Queue::new(
                &pro_que.context(),
                pro_que.queue().device(),
                None,
            )?;
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
                local_ws,
                actual_work_size * DLF_SCRATCHPAD_BYTES / (1024 * 1024)
            );
            Ok(Self {
                pro_que,
                kernel,
                header_state_buf,
                scratchpad_buf,
                output_hashes_buf,
                output_hashes_buf_b,
                read_queue,
                stream_weights_buf,
                work_size: actual_work_size,
                local_work_size: local_ws,
                device_name_cached: device_name,
                device_family: family,
                tuning,
                recovery_attempts: 0,
                max_recovery_attempts: 3,
                pending: None,
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
            let weights = if std::env::var("ZION_GPU_NO_STREAM_BYPRODUCT")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false)
            {
                &zion_cosmic_harmony::stream_profit::StreamWeights::default()
            } else {
                weights
            };
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

            let double_buffer_disabled = std::env::var("ZION_GPU_NO_DOUBLE_BUFFER")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

            let early_break = std::env::var("ZION_GPU_EARLY_BREAK")
                .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
                .unwrap_or(false);

            let mut all_solutions = Vec::new();
            let mut total_tested = 0u64;
            let mut current_nonce = nonce_start;
            // Cap batch to single chunk when early_break is enabled (safe path).
            let mut left = if early_break {
                batch_size.min(self.work_size as u64)
            } else {
                batch_size
            };

            // ── Double-buffered async readback path ──────────────────────
            if !double_buffer_disabled && left > self.work_size as u64 {
                let out_bufs = [&self.output_hashes_buf, &self.output_hashes_buf_b];
                let mut host_a = vec![0u8; self.work_size * 32];
                let mut host_b = vec![0u8; self.work_size * 32];
                let mut buf_idx = 0usize;

                let mut prev_read_event: Option<Event> = None;
                let mut prev_chunk = 0usize;
                let mut prev_nonce = 0u64;
                let mut prev_buf_idx = 0usize;

                while left > 0 {
                    let chunk = (left as usize).min(self.work_size);
                    let local_size = self.local_work_size.min(chunk);
                    let global_size = ((chunk + local_size - 1) / local_size) * local_size;
                    let out_buf = out_bufs[buf_idx];

                    self.kernel.set_arg(1, current_nonce)?;
                    self.kernel.set_arg(2, chunk as u32)?;
                    self.kernel.set_arg(3, out_buf)?;

                    let mut k_event = Event::empty();
                    {
                        let guard = GpuGuard::new();
                        unsafe {
                            self.kernel
                                .cmd()
                                .global_work_size(global_size)
                                .local_work_size(local_size)
                                .enew(&mut k_event)
                                .enq()?;
                        }
                        if guard.was_caught() {
                            self.recovery_attempts += 1;
                            anyhow::bail!(
                                "GPU access violation during kernel enqueue (attempt {}/{}). AMD driver crash detected.",
                                self.recovery_attempts,
                                self.max_recovery_attempts
                            );
                        }
                    }

                    let mut r_event = Event::empty();
                    {
                        let guard = GpuGuard::new();
                        let dst = if buf_idx == 0 { &mut host_a[..] } else { &mut host_b[..] };
                        unsafe {
                            out_buf
                                .read(&mut dst[..chunk * 32])
                                .queue(&self.read_queue)
                                .ewait(&k_event)
                                .enew(&mut r_event)
                                .block(false)
                                .enq()?;
                        }
                        if guard.was_caught() {
                            self.recovery_attempts += 1;
                            anyhow::bail!(
                                "GPU access violation during async hash buffer read (attempt {}/{}). AMD driver crash detected.",
                                self.recovery_attempts,
                                self.max_recovery_attempts
                            );
                        }
                    }

                    let _ = self.pro_que.queue().flush();

                    if let Some(prev_ev) = prev_read_event.take() {
                        {
                            let guard = GpuGuard::new();
                            prev_ev.wait_for()?;
                            if guard.was_caught() {
                                self.recovery_attempts += 1;
                                anyhow::bail!(
                                    "GPU access violation during read event wait (attempt {}/{}). AMD driver crash detected.",
                                    self.recovery_attempts,
                                    self.max_recovery_attempts
                                );
                            }
                        }
                        let prev_host = if prev_buf_idx == 0 { &host_a[..] } else { &host_b[..] };
                        for i in 0..prev_chunk {
                            let hash: [u8; 32] =
                                prev_host[i * 32..(i + 1) * 32].try_into().unwrap();
                            if target.allows(&hash) {
                                let nonce = prev_nonce.wrapping_add(i as u64);
                                all_solutions.push((nonce, hash, None));
                                break;
                            }
                        }
                        total_tested += prev_chunk as u64;
                        // Early-break: wait for pending read to avoid use-after-free
                        if early_break {
                            r_event.wait_for()?;
                            return Ok(GpuBatchResult {
                                nonces_tested: total_tested,
                                solutions: all_solutions,
                            });
                        }
                    }

                    prev_read_event = Some(r_event);
                    prev_chunk = chunk;
                    prev_nonce = current_nonce;
                    prev_buf_idx = buf_idx;
                    current_nonce = current_nonce.wrapping_add(chunk as u64);
                    left -= chunk as u64;
                    buf_idx = 1 - buf_idx;
                }

                if let Some(prev_ev) = prev_read_event.take() {
                    {
                        let guard = GpuGuard::new();
                        prev_ev.wait_for()?;
                        if guard.was_caught() {
                            self.recovery_attempts += 1;
                            anyhow::bail!(
                                "GPU access violation during final read event wait (attempt {}/{}). AMD driver crash detected.",
                                self.recovery_attempts,
                                self.max_recovery_attempts
                            );
                        }
                    }
                    let prev_host = if prev_buf_idx == 0 { &host_a[..] } else { &host_b[..] };
                    for i in 0..prev_chunk {
                        let hash: [u8; 32] =
                            prev_host[i * 32..(i + 1) * 32].try_into().unwrap();
                        if target.allows(&hash) {
                            let nonce = prev_nonce.wrapping_add(i as u64);
                            all_solutions.push((nonce, hash, None));
                            break;
                        }
                    }
                    total_tested += prev_chunk as u64;
                }

                Ok(GpuBatchResult {
                    nonces_tested: total_tested,
                    solutions: all_solutions,
                })
            } else {
                // ── Simple single-buffer path ──
                while left > 0 {
                    let chunk = (left as usize).min(self.work_size);
                    let local_size = self.local_work_size.min(chunk);
                    let global_size = ((chunk + local_size - 1) / local_size) * local_size;

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
                    if early_break {
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
            let batch_multiplier: u64 = std::env::var("ZION_GPU_BENCH_BATCH")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(4);
            let batch_size = self.work_size as u64 * batch_multiplier;
            let start = Instant::now();
            let mut total_hashes = 0u64;
            let mut nonce_start = 0u64;
            while start.elapsed().as_secs_f64() < secs {
                let result = self.mine_batch(header, target, nonce_start, batch_size)?;
                total_hashes += result.nonces_tested;
                nonce_start = nonce_start.wrapping_add(batch_size);
            }
            let elapsed = start.elapsed().as_secs_f64();
            let khps = if elapsed > 0.0 {
                total_hashes as f64 / elapsed / 1_000.0
            } else {
                0.0
            };
            Ok((total_hashes, elapsed, khps))
        }

        /// FIX #9: Override default launch_batch which discards mine_batch results.
        /// Store the synchronous mine_batch result in `pending` for collect_batch to return.
        /// This is synchronous (no true overlap) but CORRECT — nonces_tested and solutions
        /// are preserved instead of being dropped by the trait default.
        fn launch_batch(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<u64> {
            // If a previous batch is still pending (collect_batch not called), drop it.
            // This shouldn't happen in normal pipeline usage but guards against state corruption.
            if self.pending.is_some() {
                self.pending = None;
            }
            let result = self.mine_batch(header, target, nonce_start, batch_size)?;
            self.pending = Some(result);
            Ok(0)
        }

        /// FIX #9: Return the pending batch result stored by launch_batch.
        fn collect_batch(&mut self, _token: u64) -> Result<GpuBatchResult> {
            self.pending
                .take()
                .ok_or_else(|| anyhow::anyhow!("no pending OpenCL fire batch to collect"))
        }
    }
}

// ─── CUDA Backend ───────────────────────────────────────────────────────────

/// Detect GPU compute capability for NVRTC arch flag.
/// Falls back to ZION_CUDA_ARCH env var, then "sm_86".
#[cfg(feature = "gpu-cuda")]
fn detect_cuda_arch(dev: &cudarc::driver::CudaDevice) -> String {
    use cudarc::driver::sys::CUdevice_attribute;
    if let Ok(arch) = std::env::var("ZION_CUDA_ARCH") {
        return arch;
    }
    let major = dev.attribute(CUdevice_attribute::CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR);
    let minor = dev.attribute(CUdevice_attribute::CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR);
    match (major, minor) {
        (Ok(maj), Ok(min)) => {
            let arch = format!("sm_{}{}", maj, min);
            eprintln!("cuda_arch_detect: compute_capability={}.{} => arch={}", maj, min, arch);
            arch
        }
        _ => {
            eprintln!("cuda_arch_detect: failed to query compute capability, falling back to sm_86");
            "sm_86".to_string()
        }
    }
}

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

            // Compile PTX with arch-specific optimization
            // Note: cosmic_harmony kernel is 1187 lines with complex NPU code.
            // --ptxas-options=-O3 and -lineinfo both cause ptxas to hang.
            // Use minimal flags for this kernel.
            let arch = detect_cuda_arch(&dev);
            let ptx = compile_ptx_with_opts(
                CUDA_KERNEL_SRC,
                CompileOptions {
                    options: vec![
                        "--use_fast_math".to_string(),
                        format!("-arch={}", arch),
                        "--std=c++14".to_string(),
                    ],
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

        fn algorithm(&self) -> String {
            "cosmic_harmony_ekam_deeksha_v2".to_string()
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

            // Batched launch: reset sentinel ONCE, launch ALL chunks back-to-back
            // without syncing, then a single sync + read at the end. The kernel
            // checks the sentinel at entry and exits early if a solution was
            // already found by a previous chunk, so wasted GPU work is minimal.
            // This eliminates N-1 synchronous host↔device copies per batch
            // (8-12% hashrate gain vs per-chunk sync).
            self.dev
                .htod_sync_copy_into(&[SENTINEL], &mut self.result_nonce)
                .map_err(|e| anyhow::anyhow!("reset sentinel: {e}"))?;

            while left > 0 {
                let chunk = (left as usize).min(self.work_size) as u32;
                let blocks = (chunk + threads_per_block - 1) / threads_per_block;
                let cfg = LaunchConfig {
                    grid_dim: (blocks, 1, 1),
                    block_dim: (threads_per_block, 1, 1),
                    shared_mem_bytes: 0,
                };

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

                total_tested += chunk as u64;
                current_nonce = current_nonce.wrapping_add(chunk as u64);
                left = left.saturating_sub(chunk as u64);
            }

            // Single sync point: wait for ALL chunks to complete
            self.dev
                .synchronize()
                .map_err(|e| anyhow::anyhow!("device sync: {e}"))?;

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

// ─── CUDA Backend: DeekshaLite Fire ──────────────────────────────────────────

#[cfg(feature = "gpu-cuda")]
pub mod cuda_deeksha_lite_fire {
    use super::*;
    use cudarc::driver::{CudaDevice, CudaSlice, LaunchAsync, LaunchConfig};
    use cudarc::nvrtc::{compile_ptx_with_opts, CompileOptions};
    use std::sync::Arc;
    use std::time::Instant;

    const CUDA_KERNEL_SRC: &str = include_str!("deeksha_lite_fire.cu");
    const SCRATCHPAD_BYTES: usize = 262_144; // 256 KiB per thread
    const SENTINEL: u64 = 0xFFFF_FFFF_FFFF_FFFF;
    const DEFAULT_WORK_SIZE_CAP: usize = 65_536; // 16GB VRAM for 24GB GPU

    pub struct CudaDeekshaLiteFireMiner {
        dev: Arc<CudaDevice>,
        work_size: usize,
        device_name_cached: String,
        header_state_buf: CudaSlice<u64>,
        scratchpad_buf: CudaSlice<u8>,
        result_nonce: CudaSlice<u64>,
        result_hash: CudaSlice<u8>,
        output_hashes_buf: CudaSlice<u8>,
        /// Pending batch info for pipelined launch/collect.
        pending: Option<PendingBatch>,
    }

    /// Info stored by launch_batch, used by collect_batch.
    struct PendingBatch {
        nonce_start: u64,
        batch_size: u64,
        target_u32: u32,
        total_tested: u64,
    }

    impl CudaDeekshaLiteFireMiner {
        /// Precompute Keccak256 state after absorbing the 80-byte header.
        /// The state is 25 u64s (200 bytes). Each thread will then only
        /// XOR the nonce bytes (80..88), apply padding, and run f1600.
        /// Identical to OpenCL v1's implementation — guarantees CPU/GPU hash agreement.
        fn precompute_header_keccak_state(header_80: &[u8]) -> [u64; 25] {
            let mut state = [0u64; 25];
            for (i, &b) in header_80.iter().enumerate() {
                let word_idx = i / 8;
                let shift = (i % 8) * 8;
                state[word_idx] ^= (b as u64) << shift;
            }
            state
        }

        pub fn new(work_size: usize) -> Result<Self> {
            let dev =
                CudaDevice::new(0).map_err(|e| anyhow::anyhow!("CUDA device init failed: {e}"))?;

            let device_name = dev
                .name()
                .unwrap_or_else(|_| "unknown CUDA device".to_string());

            // Compile PTX with fast-math + arch-specific optimization
            // Auto-detect GPU compute capability (sm_61 Pascal, sm_86 Ampere, etc.)
            let arch = detect_cuda_arch(&dev);
            let mut opts = vec![
                "--use_fast_math".to_string(),
                format!("-arch={}", arch),
                "--std=c++14".to_string(),
                "-lineinfo".to_string(),
                "--ptxas-options=-O3".to_string(),
            ];
            // Allow override of max registers per thread
            if let Ok(maxreg) = std::env::var("ZION_CUDA_MAXREG") {
                opts.push(format!("--maxrregcount={}", maxreg));
            }
            let ptx = compile_ptx_with_opts(
                CUDA_KERNEL_SRC,
                CompileOptions {
                    options: opts,
                    ..Default::default()
                },
            )
            .map_err(|e| anyhow::anyhow!("NVRTC compile failed: {e}"))?;
            dev.load_ptx(
                ptx,
                "deeksha_fire",
                &["deeksha_lite_fire_mine", "deeksha_lite_fire_debug"],
            )
            .map_err(|e| anyhow::anyhow!("PTX load failed: {e}"))?;

            // Conservative work size cap
            let work_cap = std::env::var("ZION_CUDA_WORK_CAP")
                .ok()
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(DEFAULT_WORK_SIZE_CAP)
                .max(64);
            let actual_work_size = work_size.min(work_cap).max(64);

            // Allocate buffers
            let header_state_buf = dev
                .alloc_zeros::<u64>(25)
                .map_err(|e| anyhow::anyhow!("header_state alloc: {e}"))?;
            let scratchpad_buf = dev
                .alloc_zeros::<u8>(actual_work_size * SCRATCHPAD_BYTES)
                .map_err(|e| anyhow::anyhow!("scratchpad alloc: {e}"))?;
            let result_nonce = dev
                .htod_copy(vec![SENTINEL])
                .map_err(|e| anyhow::anyhow!("result_nonce alloc: {e}"))?;
            let result_hash = dev
                .alloc_zeros::<u8>(32)
                .map_err(|e| anyhow::anyhow!("result_hash alloc: {e}"))?;
            let output_hashes_buf = dev
                .alloc_zeros::<u8>(actual_work_size * 32)
                .map_err(|e| anyhow::anyhow!("output_hashes alloc: {e}"))?;

            println!(
                "gpu_cuda_fire_init device=\"{}\" work_size={} scratchpad_mb={}",
                device_name,
                actual_work_size,
                actual_work_size * SCRATCHPAD_BYTES / (1024 * 1024),
            );

            Ok(Self {
                dev,
                work_size: actual_work_size,
                device_name_cached: device_name,
                header_state_buf,
                scratchpad_buf,
                result_nonce,
                result_hash,
                output_hashes_buf,
                pending: None,
            })
        }
    }

    impl GpuMiner for CudaDeekshaLiteFireMiner {
        fn device_name(&self) -> String {
            self.device_name_cached.clone()
        }

        fn backend_kind(&self) -> GpuBackendKind {
            GpuBackendKind::Cuda
        }

        fn algorithm(&self) -> String {
            "deeksha_lite_fire".to_string()
        }

        fn update_epoch(&mut self, _height: u64) -> Result<()> {
            // deeksha_lite_fire has no epoch-based NPU weights
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

            // Precompute Keccak state on host (same as OpenCL)
            let keccak_state = Self::precompute_header_keccak_state(&header_bytes);
            // ASYNC copy: queued on default stream, kernel will wait for it.
            // This eliminates a host sync point — host can proceed immediately.
            self.dev
                .htod_copy_into(keccak_state.to_vec(), &mut self.header_state_buf)
                .map_err(|e| anyhow::anyhow!("header_state upload: {e}"))?;

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
                .get_func("deeksha_fire", "deeksha_lite_fire_mine")
                .ok_or_else(|| anyhow::anyhow!("deeksha_lite_fire_mine kernel not found"))?;

            let threads_per_block: u32 = std::env::var("ZION_CUDA_TPB")
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(64);

            // === BATCHED LAUNCH: reset sentinel ONCE, launch ALL chunks, sync ONCE ===
            // This eliminates N-1 synchronous host↔device copies per batch.
            // The kernel uses atomicExch for result_nonce, so only the first solution
            // across all chunks will be recorded. Subsequent chunks early-exit if
            // target_u32 != 0 and a solution was already found.

            // Reset sentinel once for the entire batch (ASYNC)
            self.dev
                .htod_copy_into(vec![SENTINEL], &mut self.result_nonce)
                .map_err(|e| anyhow::anyhow!("reset sentinel: {e}"))?;

            // Launch all chunks back-to-back without syncing between them
            while left > 0 {
                let chunk = (left as usize).min(self.work_size) as u32;
                let blocks = (chunk + threads_per_block - 1) / threads_per_block;
                let cfg = LaunchConfig {
                    grid_dim: (blocks, 1, 1),
                    block_dim: (threads_per_block, 1, 1),
                    shared_mem_bytes: 0,
                };

                unsafe {
                    func.clone()
                        .launch(
                            cfg,
                            (
                                &self.header_state_buf,
                                current_nonce,
                                chunk,
                                &self.output_hashes_buf,
                                &self.scratchpad_buf,
                                target_u32,
                                &mut self.result_nonce,
                                &mut self.result_hash,
                            ),
                        )
                        .map_err(|e| anyhow::anyhow!("kernel launch: {e}"))?;
                }

                total_tested += chunk as u64;
                current_nonce += chunk as u64;
                left = left.saturating_sub(chunk as u64);
            }

            // Single sync point: wait for ALL chunks to complete
            self.dev
                .synchronize()
                .map_err(|e| anyhow::anyhow!("device sync: {e}"))?;

            // Read result once
            let result_nonce_host = self
                .dev
                .dtoh_sync_copy(&self.result_nonce)
                .map_err(|e| anyhow::anyhow!("result_nonce download: {e}"))?;

            if result_nonce_host[0] != SENTINEL {
                let result_hash_host = self
                    .dev
                    .dtoh_sync_copy(&self.result_hash)
                    .map_err(|e| anyhow::anyhow!("result_hash download: {e}"))?;
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&result_hash_host);
                all_solutions.push((result_nonce_host[0], hash, None));
            }

            Ok(GpuBatchResult {
                solutions: all_solutions,
                nonces_tested: total_tested,
            })
        }

        fn mine_batch_raw(
            &mut self,
            raw_header: &[u8],
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<GpuBatchResult> {
            // For deeksha_lite_fire, raw header is the 80-byte mining header
            let mut bytes = [0u8; 80];
            let len = raw_header.len().min(80);
            bytes[..len].copy_from_slice(&raw_header[..len]);
            let header = MiningHeader::from_bytes(bytes);
            self.mine_batch(header, target, nonce_start, batch_size)
        }

        /// Async launch: queue all kernel chunks on the GPU stream without syncing.
        /// The host returns immediately after queueing. Call `collect_batch` to
        /// sync and read results. This enables overlapping pool I/O with GPU compute.
        fn launch_batch(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<u64> {
            // If there's already a pending batch, collect it first (blocking).
            if self.pending.is_some() {
                let _ = self.collect_batch(0)?;
            }

            let header_bytes = header.to_bytes();
            let keccak_state = Self::precompute_header_keccak_state(&header_bytes);
            self.dev
                .htod_copy_into(keccak_state.to_vec(), &mut self.header_state_buf)
                .map_err(|e| anyhow::anyhow!("header_state upload: {e}"))?;

            let target_u32 = u32::from_le_bytes([
                target.bytes[0],
                target.bytes[1],
                target.bytes[2],
                target.bytes[3],
            ]);

            let func = self
                .dev
                .get_func("deeksha_fire", "deeksha_lite_fire_mine")
                .ok_or_else(|| anyhow::anyhow!("deeksha_lite_fire_mine kernel not found"))?;

            let threads_per_block: u32 = std::env::var("ZION_CUDA_TPB")
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(64);

            // Reset sentinel (ASYNC)
            self.dev
                .htod_copy_into(vec![SENTINEL], &mut self.result_nonce)
                .map_err(|e| anyhow::anyhow!("reset sentinel: {e}"))?;

            // Launch all chunks back-to-back (async, no sync)
            let mut total_tested = 0u64;
            let mut current_nonce = nonce_start;
            let mut left = batch_size;

            while left > 0 {
                let chunk = (left as usize).min(self.work_size) as u32;
                let blocks = (chunk + threads_per_block - 1) / threads_per_block;
                let cfg = LaunchConfig {
                    grid_dim: (blocks, 1, 1),
                    block_dim: (threads_per_block, 1, 1),
                    shared_mem_bytes: 0,
                };

                unsafe {
                    func.clone()
                        .launch(
                            cfg,
                            (
                                &self.header_state_buf,
                                current_nonce,
                                chunk,
                                &self.output_hashes_buf,
                                &self.scratchpad_buf,
                                target_u32,
                                &mut self.result_nonce,
                                &mut self.result_hash,
                            ),
                        )
                        .map_err(|e| anyhow::anyhow!("kernel launch: {e}"))?;
                }

                total_tested += chunk as u64;
                current_nonce += chunk as u64;
                left = left.saturating_sub(chunk as u64);
            }

            // Store pending info — NO sync yet, host returns immediately
            self.pending = Some(PendingBatch {
                nonce_start,
                batch_size,
                target_u32,
                total_tested,
            });

            Ok(0) // token is unused, pending is stored in self
        }

        /// Collect results from a previously launched batch.
        /// Syncs the GPU and reads results.
        fn collect_batch(&mut self, _token: u64) -> Result<GpuBatchResult> {
            let pending = self
                .pending
                .take()
                .ok_or_else(|| anyhow::anyhow!("no pending batch to collect"))?;

            // Single sync point: wait for ALL chunks to complete
            self.dev
                .synchronize()
                .map_err(|e| anyhow::anyhow!("device sync: {e}"))?;

            // Read result once
            let result_nonce_host = self
                .dev
                .dtoh_sync_copy(&self.result_nonce)
                .map_err(|e| anyhow::anyhow!("result_nonce download: {e}"))?;

            let mut all_solutions = Vec::new();
            if result_nonce_host[0] != SENTINEL {
                let result_hash_host = self
                    .dev
                    .dtoh_sync_copy(&self.result_hash)
                    .map_err(|e| anyhow::anyhow!("result_hash download: {e}"))?;
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&result_hash_host);
                all_solutions.push((result_nonce_host[0], hash, None));
            }

            Ok(GpuBatchResult {
                solutions: all_solutions,
                nonces_tested: pending.total_tested,
            })
        }

        fn benchmark(&mut self, secs: f64) -> Result<(u64, f64, f64)> {
            let start = Instant::now();
            let mut total: u64 = 0;
            let mut nonce: u64 = 0;
            while start.elapsed().as_secs_f64() < secs {
                let header = MiningHeader::from_bytes([0u8; 80]);
                let target = DifficultyTarget { bytes: [0xFFu8; 32] };
                let result = self.mine_batch(header, target, nonce, 4096)?;
                total += result.nonces_tested;
                nonce += 4096;
            }
            let elapsed = start.elapsed().as_secs_f64();
            let hps = if elapsed > 0.0 { total as f64 / elapsed } else { 0.0 };
            Ok((total, elapsed, hps))
        }
    }
}

// ─── CUDA Backend: deeksha_lite_v1 / deeksha_chv3 (no thermal loop) ─────────

#[cfg(feature = "gpu-cuda")]
pub mod cuda_deeksha_lite {
    use super::*;
    use cudarc::driver::{CudaDevice, CudaSlice, LaunchAsync, LaunchConfig};
    use cudarc::nvrtc::{compile_ptx_with_opts, CompileOptions};
    use std::sync::Arc;
    use std::time::Instant;

    const CUDA_KERNEL_SRC: &str = include_str!("deeksha_lite.cu");
    const SCRATCHPAD_BYTES: usize = 262_144; // 256 KiB per thread
    const SENTINEL: u64 = 0xFFFF_FFFF_FFFF_FFFF;
    const DEFAULT_WORK_SIZE_CAP: usize = 65_536; // 16GB VRAM for 24GB GPU

    pub struct CudaDeekshaLiteMiner {
        dev: Arc<CudaDevice>,
        work_size: usize,
        device_name_cached: String,
        header_state_buf: CudaSlice<u64>,
        scratchpad_buf: CudaSlice<u8>,
        result_nonce: CudaSlice<u64>,
        result_hash: CudaSlice<u8>,
        output_hashes_buf: CudaSlice<u8>,
        pending: Option<PendingBatch>,
    }

    struct PendingBatch {
        nonce_start: u64,
        batch_size: u64,
        target_u32: u32,
        total_tested: u64,
    }

    impl CudaDeekshaLiteMiner {
        fn precompute_header_keccak_state(header_80: &[u8]) -> [u64; 25] {
            let mut state = [0u64; 25];
            for (i, &b) in header_80.iter().enumerate() {
                let word_idx = i / 8;
                let shift = (i % 8) * 8;
                state[word_idx] ^= (b as u64) << shift;
            }
            state
        }

        pub fn new(work_size: usize) -> Result<Self> {
            let dev =
                CudaDevice::new(0).map_err(|e| anyhow::anyhow!("CUDA device init failed: {e}"))?;

            let device_name = dev
                .name()
                .unwrap_or_else(|_| "unknown CUDA device".to_string());

            let arch = detect_cuda_arch(&dev);
            let mut opts = vec![
                "--use_fast_math".to_string(),
                format!("-arch={}", arch),
                "--std=c++14".to_string(),
                "-lineinfo".to_string(),
                "--ptxas-options=-O3".to_string(),
            ];
            if let Ok(maxreg) = std::env::var("ZION_CUDA_MAXREG") {
                opts.push(format!("--maxrregcount={}", maxreg));
            }
            let ptx = compile_ptx_with_opts(
                CUDA_KERNEL_SRC,
                CompileOptions {
                    options: opts,
                    ..Default::default()
                },
            )
            .map_err(|e| anyhow::anyhow!("NVRTC compile failed: {e}"))?;
            dev.load_ptx(
                ptx,
                "deeksha_lite",
                &["deeksha_lite_mine", "deeksha_lite_debug"],
            )
            .map_err(|e| anyhow::anyhow!("PTX load failed: {e}"))?;

            let work_cap = std::env::var("ZION_CUDA_WORK_CAP")
                .ok()
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(DEFAULT_WORK_SIZE_CAP)
                .max(64);
            let actual_work_size = work_size.min(work_cap).max(64);

            let header_state_buf = dev
                .alloc_zeros::<u64>(25)
                .map_err(|e| anyhow::anyhow!("header_state alloc: {e}"))?;
            let scratchpad_buf = dev
                .alloc_zeros::<u8>(actual_work_size * SCRATCHPAD_BYTES)
                .map_err(|e| anyhow::anyhow!("scratchpad alloc: {e}"))?;
            let result_nonce = dev
                .htod_copy(vec![SENTINEL])
                .map_err(|e| anyhow::anyhow!("result_nonce alloc: {e}"))?;
            let result_hash = dev
                .alloc_zeros::<u8>(32)
                .map_err(|e| anyhow::anyhow!("result_hash alloc: {e}"))?;
            let output_hashes_buf = dev
                .alloc_zeros::<u8>(actual_work_size * 32)
                .map_err(|e| anyhow::anyhow!("output_hashes alloc: {e}"))?;

            println!(
                "gpu_cuda_lite_init device=\"{}\" work_size={} scratchpad_mb={}",
                device_name,
                actual_work_size,
                actual_work_size * SCRATCHPAD_BYTES / (1024 * 1024),
            );

            Ok(Self {
                dev,
                work_size: actual_work_size,
                device_name_cached: device_name,
                header_state_buf,
                scratchpad_buf,
                result_nonce,
                result_hash,
                output_hashes_buf,
                pending: None,
            })
        }
    }

    impl GpuMiner for CudaDeekshaLiteMiner {
        fn device_name(&self) -> String {
            self.device_name_cached.clone()
        }

        fn backend_kind(&self) -> GpuBackendKind {
            GpuBackendKind::Cuda
        }

        fn algorithm(&self) -> String {
            "deeksha_lite_v1".to_string()
        }

        fn update_epoch(&mut self, _height: u64) -> Result<()> {
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
            let keccak_state = Self::precompute_header_keccak_state(&header_bytes);
            self.dev
                .htod_copy_into(keccak_state.to_vec(), &mut self.header_state_buf)
                .map_err(|e| anyhow::anyhow!("header_state upload: {e}"))?;

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
                .get_func("deeksha_lite", "deeksha_lite_mine")
                .ok_or_else(|| anyhow::anyhow!("deeksha_lite_mine kernel not found"))?;

            let threads_per_block: u32 = std::env::var("ZION_CUDA_TPB")
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(64);

            self.dev
                .htod_copy_into(vec![SENTINEL], &mut self.result_nonce)
                .map_err(|e| anyhow::anyhow!("reset sentinel: {e}"))?;

            while left > 0 {
                let chunk = (left as usize).min(self.work_size) as u32;
                let blocks = (chunk + threads_per_block - 1) / threads_per_block;
                let cfg = LaunchConfig {
                    grid_dim: (blocks, 1, 1),
                    block_dim: (threads_per_block, 1, 1),
                    shared_mem_bytes: 0,
                };

                unsafe {
                    func.clone()
                        .launch(
                            cfg,
                            (
                                &self.header_state_buf,
                                current_nonce,
                                chunk,
                                &self.output_hashes_buf,
                                &self.scratchpad_buf,
                                target_u32,
                                &mut self.result_nonce,
                                &mut self.result_hash,
                            ),
                        )
                        .map_err(|e| anyhow::anyhow!("kernel launch: {e}"))?;
                }

                total_tested += chunk as u64;
                current_nonce += chunk as u64;
                left = left.saturating_sub(chunk as u64);
            }

            self.dev
                .synchronize()
                .map_err(|e| anyhow::anyhow!("device sync: {e}"))?;

            let result_nonce_host = self
                .dev
                .dtoh_sync_copy(&self.result_nonce)
                .map_err(|e| anyhow::anyhow!("result_nonce download: {e}"))?;

            if result_nonce_host[0] != SENTINEL {
                let result_hash_host = self
                    .dev
                    .dtoh_sync_copy(&self.result_hash)
                    .map_err(|e| anyhow::anyhow!("result_hash download: {e}"))?;
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&result_hash_host);
                all_solutions.push((result_nonce_host[0], hash, None));
            }

            Ok(GpuBatchResult {
                solutions: all_solutions,
                nonces_tested: total_tested,
            })
        }

        fn mine_batch_raw(
            &mut self,
            raw_header: &[u8],
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<GpuBatchResult> {
            let mut bytes = [0u8; 80];
            let len = raw_header.len().min(80);
            bytes[..len].copy_from_slice(&raw_header[..len]);
            let header = MiningHeader::from_bytes(bytes);
            self.mine_batch(header, target, nonce_start, batch_size)
        }

        fn launch_batch(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<u64> {
            if self.pending.is_some() {
                let _ = self.collect_batch(0)?;
            }

            let header_bytes = header.to_bytes();
            let keccak_state = Self::precompute_header_keccak_state(&header_bytes);
            self.dev
                .htod_copy_into(keccak_state.to_vec(), &mut self.header_state_buf)
                .map_err(|e| anyhow::anyhow!("header_state upload: {e}"))?;

            let target_u32 = u32::from_le_bytes([
                target.bytes[0],
                target.bytes[1],
                target.bytes[2],
                target.bytes[3],
            ]);

            let func = self
                .dev
                .get_func("deeksha_lite", "deeksha_lite_mine")
                .ok_or_else(|| anyhow::anyhow!("deeksha_lite_mine kernel not found"))?;

            let threads_per_block: u32 = std::env::var("ZION_CUDA_TPB")
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(64);

            self.dev
                .htod_copy_into(vec![SENTINEL], &mut self.result_nonce)
                .map_err(|e| anyhow::anyhow!("reset sentinel: {e}"))?;

            let mut total_tested = 0u64;
            let mut current_nonce = nonce_start;
            let mut left = batch_size;

            while left > 0 {
                let chunk = (left as usize).min(self.work_size) as u32;
                let blocks = (chunk + threads_per_block - 1) / threads_per_block;
                let cfg = LaunchConfig {
                    grid_dim: (blocks, 1, 1),
                    block_dim: (threads_per_block, 1, 1),
                    shared_mem_bytes: 0,
                };

                unsafe {
                    func.clone()
                        .launch(
                            cfg,
                            (
                                &self.header_state_buf,
                                current_nonce,
                                chunk,
                                &self.output_hashes_buf,
                                &self.scratchpad_buf,
                                target_u32,
                                &mut self.result_nonce,
                                &mut self.result_hash,
                            ),
                        )
                        .map_err(|e| anyhow::anyhow!("kernel launch: {e}"))?;
                }

                total_tested += chunk as u64;
                current_nonce += chunk as u64;
                left = left.saturating_sub(chunk as u64);
            }

            self.pending = Some(PendingBatch {
                nonce_start,
                batch_size,
                target_u32,
                total_tested,
            });

            Ok(0)
        }

        fn collect_batch(&mut self, _token: u64) -> Result<GpuBatchResult> {
            let pending = self
                .pending
                .take()
                .ok_or_else(|| anyhow::anyhow!("no pending batch to collect"))?;

            self.dev
                .synchronize()
                .map_err(|e| anyhow::anyhow!("device sync: {e}"))?;

            let result_nonce_host = self
                .dev
                .dtoh_sync_copy(&self.result_nonce)
                .map_err(|e| anyhow::anyhow!("result_nonce download: {e}"))?;

            let mut all_solutions = Vec::new();
            if result_nonce_host[0] != SENTINEL {
                let result_hash_host = self
                    .dev
                    .dtoh_sync_copy(&self.result_hash)
                    .map_err(|e| anyhow::anyhow!("result_hash download: {e}"))?;
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&result_hash_host);
                all_solutions.push((result_nonce_host[0], hash, None));
            }

            Ok(GpuBatchResult {
                solutions: all_solutions,
                nonces_tested: pending.total_tested,
            })
        }

        fn benchmark(&mut self, secs: f64) -> Result<(u64, f64, f64)> {
            let start = Instant::now();
            let mut total: u64 = 0;
            let mut nonce: u64 = 0;
            while start.elapsed().as_secs_f64() < secs {
                let header = MiningHeader::from_bytes([0u8; 80]);
                let target = DifficultyTarget { bytes: [0xFFu8; 32] };
                let result = self.mine_batch(header, target, nonce, 4096)?;
                total += result.nonces_tested;
                nonce += 4096;
            }
            let elapsed = start.elapsed().as_secs_f64();
            let hps = if elapsed > 0.0 { total as f64 / elapsed } else { 0.0 };
            Ok((total, elapsed, hps))
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

            // Auto-cap batch_size based on GLOBAL GPU memory budget.
            // On Apple Silicon (unified memory), multiple Metal instances
            // share the same physical RAM. We use a global budget tracker
            // to prevent OOM system freezes.
            // Each thread needs 256 KiB scratchpad.
            let device_recommended = device.recommended_max_working_set_size();
            let budget_bytes = claim_gpu_memory_budget(device_recommended);
            let max_threads_by_mem = (budget_bytes / 262_144) as usize;
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
                        "scratchpad allocation failed: need {} MiB, got {} bytes (budget {} MiB, device recommended {} MiB)",
                        scratch_bytes / (1024 * 1024),
                        scratchpad_buf.length(),
                        budget_bytes / (1024 * 1024),
                        device_recommended / (1024 * 1024),
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

// ─── CPU Fallback for External Algos (kheavyhash, blake3, etc.) ──────────────
// On platforms without OpenCL (macOS Metal, CUDA-only), external AuxPoW
// algorithms have no GPU kernel. This module provides a CPU-based fallback
// that implements the GpuMiner trait using native-ffi hashers.

#[cfg(feature = "native-kheavyhash")]
pub mod cpu_external_fallback {
    use super::*;
    use std::time::Instant;

    pub struct CpuExternalMiner {
        algorithm: String,
        work_size: usize,
        device_name_cached: String,
    }

    impl CpuExternalMiner {
        pub fn new(algorithm: &str, work_size: usize) -> Result<Self> {
            Ok(Self {
                algorithm: algorithm.to_string(),
                work_size,
                device_name_cached: format!("Apple M1 (CPU fallback for {})", algorithm),
            })
        }
    }

    impl GpuMiner for CpuExternalMiner {
        fn device_name(&self) -> String {
            self.device_name_cached.clone()
        }

        fn backend_kind(&self) -> GpuBackendKind {
            GpuBackendKind::Metal
        }

        fn algorithm(&self) -> String {
            self.algorithm.clone()
        }

        fn update_epoch(&mut self, _height: u64) -> Result<()> {
            Ok(())
        }

        fn mine_batch(
            &mut self,
            header: MiningHeader,
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<GpuBatchResult> {
            let actual_batch = batch_size.min(self.work_size as u64).min(65536);
            let pre_pow_hash = &header.to_bytes()[..32];
            match self.algorithm.as_str() {
                "kheavyhash" | "kheavyhash_kas" => {
                    for i in 0..actual_batch {
                        let nonce = nonce_start.wrapping_add(i);
                        let hash = zion_native_ffi::kheavyhash::mine(pre_pow_hash, nonce, 0);
                        if hash_le_meets_target(&hash, &target.bytes)? {
                            return Ok(GpuBatchResult {
                                solutions: vec![(nonce, hash, None)],
                                nonces_tested: i + 1,
                            });
                        }
                    }
                    Ok(GpuBatchResult { solutions: Vec::new(), nonces_tested: actual_batch })
                }
                other => anyhow::bail!("cpu_external_fallback: unsupported algorithm '{}'", other),
            }
        }

        fn mine_batch_raw(
            &mut self,
            raw_header: &[u8],
            target: DifficultyTarget,
            nonce_start: u64,
            batch_size: u64,
        ) -> Result<GpuBatchResult> {
            let actual_batch = batch_size.min(self.work_size as u64).min(65536);
            match self.algorithm.as_str() {
                "kheavyhash" | "kheavyhash_kas" => {
                    let pre_pow_hash = &raw_header[..32.min(raw_header.len())];
                    for i in 0..actual_batch {
                        let nonce = nonce_start.wrapping_add(i);
                        let hash = zion_native_ffi::kheavyhash::mine(pre_pow_hash, nonce, 0);
                        if hash_le_meets_target(&hash, &target.bytes)? {
                            return Ok(GpuBatchResult {
                                solutions: vec![(nonce, hash, None)],
                                nonces_tested: i + 1,
                            });
                        }
                    }
                    Ok(GpuBatchResult { solutions: Vec::new(), nonces_tested: actual_batch })
                }
                other => anyhow::bail!("cpu_external_fallback: unsupported algorithm '{}'", other),
            }
        }

        fn benchmark(&mut self, secs: f64) -> Result<(u64, f64, f64)> {
            let start = Instant::now();
            let mut total: u64 = 0;
            let header = [0xA4u8; 32];
            let mut nonce: u64 = 0;
            while start.elapsed().as_secs_f64() < secs {
                for _ in 0..1000 {
                    let _ = zion_native_ffi::kheavyhash::mine(&header, nonce, 0);
                    nonce += 1;
                    total += 1;
                }
            }
            let elapsed = start.elapsed().as_secs_f64();
            let khps = if elapsed > 0.0 { total as f64 / elapsed / 1000.0 } else { 0.0 };
            Ok((total, elapsed, khps))
        }
    }

    /// Compare little-endian hash against big-endian target bytes.
    fn hash_le_meets_target(hash: &[u8; 32], target: &[u8]) -> Result<bool> {
        if target.len() < 32 {
            anyhow::bail!("target too short: {} bytes", target.len());
        }
        for i in 0..32 {
            let h = hash[31 - i];
            let t = target[i];
            if h < t { return Ok(true); }
            if h > t { return Ok(false); }
        }
        Ok(true)
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

            let device_recommended = device.recommended_max_working_set_size();
            let budget_bytes = claim_gpu_memory_budget(device_recommended);
            let max_threads_by_mem = (budget_bytes / 262_144) as usize;
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
                        "Fire scratchpad allocation failed: need {} MiB, got {} bytes (budget {} MiB)",
                        scratch_bytes / (1024 * 1024),
                        scratchpad_buf.length(),
                        budget_bytes / (1024 * 1024),
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
            let mut miner = AuxPowGpuMiner::new()
                .map_err(|e| anyhow::anyhow!("auxpow_gpu_init_failed algorithm={algorithm} err={e}"))?;

            // Verthash (VTC) requires a ~1.2GB data file (verthash.dat).
            // Try to load it from common locations. If not found, the miner
            // will still initialize but mine() will return an error when VTC
            // is attempted (clear "data file not loaded" message).
            if matches!(algorithm, "verthash" | "verthash_vtc") {
                let dat_path = std::env::var("ZION_VERTHASH_DAT")
                    .unwrap_or_else(|_| "verthash.dat".to_string());
                let candidates = [
                    std::path::PathBuf::from(&dat_path),
                    std::path::PathBuf::from("AuXpow/verthash.dat"),
                    std::path::PathBuf::from("verthash.dat"),
                ];
                let mut loaded = false;
                for path in &candidates {
                    if path.exists() {
                        eprintln!("auxpow_gpu_verthash loading data file: {:?}", path);
                        match std::fs::read(path) {
                            Ok(data) => {
                                if let Err(e) = miner.set_verthash_data(&data) {
                                    eprintln!("auxpow_gpu_verthash data load failed: {e}");
                                } else {
                                    loaded = true;
                                }
                                break;
                            }
                            Err(e) => {
                                eprintln!("auxpow_gpu_verthash read error {:?}: {e}", path);
                            }
                        }
                    }
                }
                if !loaded {
                    eprintln!(
                        "auxpow_gpu_verthash WARNING: verthash.dat not found in any location. \
                         Set ZION_VERTHASH_DAT env var or place verthash.dat in the working \
                         directory. VTC mining will fail until the data file is loaded."
                    );
                }
            }

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
                #[cfg(not(feature = "native-hashers"))]
                {
                    // Without native-hashers, the DagManager and C FFI for DAG
                    // generation are not compiled.  DAG-based algorithms
                    // (ethash/kawpow/progpow) cannot mine without a DAG, so
                    // return a clear error instead of silently succeeding and
                    // failing later with a confusing "DAG not set" message.
                    if matches!(
                        self.algorithm.as_str(),
                        "ethash" | "etchash" | "ethash_etc"
                            | "kawpow" | "kawpow_rvn" | "kawpow_clore" | "kawpow_evr" | "kawpow_mewc"
                            | "evrprogpow" | "evrprogpow_evr" | "meowpow" | "meowpow_mewc"
                            | "progpow" | "progpow_epic"
                    ) {
                        anyhow::bail!(
                            "DAG-based algorithm '{}' requires the 'native-hashers' feature \
                             to generate the per-epoch DAG.  Rebuild with: \
                             --features native-hashers",
                            self.algorithm
                        );
                    }
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
            // For Ethash/KawPow/ProgPow, derive epoch from block height and ensure DAG.
            // The pool sends the external block number as `height` for
            // EthStratum coins (ETC/RVN/CLORE/EPIC).

            // Set block_height on the miner so that ensure_proque_progpow()
            // generates the correct random math sequence for the current period.
            // KawPow period = 10 blocks, EPIC ProgPow period = 50 blocks.
            if matches!(
                self.algorithm.as_str(),
                "kawpow" | "kawpow_rvn" | "kawpow_clore" | "kawpow_evr" | "kawpow_mewc"
                    | "evrprogpow" | "evrprogpow_evr" | "meowpow" | "meowpow_mewc"
                    | "progpow" | "progpow_epic"
            ) {
                self.miner.set_block_height(height);
            }

            let epoch = if matches!(self.algorithm.as_str(), "ethash" | "etchash" | "ethash_etc") {
                Some((height / 30000) as u32)
            } else if matches!(
                self.algorithm.as_str(),
                "kawpow" | "kawpow_rvn" | "kawpow_clore" | "kawpow_evr" | "kawpow_mewc"
                    | "evrprogpow" | "evrprogpow_evr" | "meowpow" | "meowpow_mewc"
            ) {
                Some((height / 7500) as u32)
            } else if matches!(self.algorithm.as_str(), "progpow" | "progpow_epic") {
                Some((height / 30000) as u32)
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
                | "kawpow_mewc"
                | "evrprogpow"
                | "evrprogpow_evr"
                | "meowpow"
                | "meowpow_mewc"
                | "zelhash"
                | "zelhash_flux"
                | "progpow"
                | "progpow_epic"
                | "beamhash"
                | "beamhash_beam"
                | "fishhash"
                | "fishhash_iron"
                | "karlsenhash"
                | "karlsenhash_kls"
                | "verthash"
                | "verthash_vtc"
                | "equihashzero"
                | "equihashzero_zcl"
                | "nexapow"
                | "nexapow_nexa"
                | "qhash"
                | "qhash_qtc"
                | "dynexsolve"
                | "dynexsolve_dnx" => self.miner.mine(
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

            if let Some(GpuFoundShare { nonce, hash, mix_hash, .. }) = found {
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

            // The AuXpow GpuMiner internally caps the global work size at
            // its own work_size (detect_work_size), which may be smaller than
            // actual_batch.  Report the real number of nonces tested so the
            // caller advances nonce_offset correctly (no skipped nonces).
            let real_nonces = actual_batch.min(self.miner.internal_work_size() as u64);

            if let Some(GpuFoundShare { nonce, hash, mix_hash, .. }) = found {
                Ok(GpuBatchResult {
                    solutions: vec![(nonce, hash, mix_hash)],
                    nonces_tested: real_nonces,
                })
            } else {
                Ok(GpuBatchResult {
                    solutions: Vec::new(),
                    nonces_tested: real_nonces,
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
