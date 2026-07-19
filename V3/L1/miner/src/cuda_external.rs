/// CUDA miner for external AuxPoW algorithms (kheavyhash, blake3, autolykos, zelhash,
/// ethash, kawpow).
///
/// Uses the existing CUDA kernels from AuXpow/csrc/cuda/ and compiles them
/// via NVRTC at runtime. This eliminates the CPU fallback for external
/// algorithms when using the CUDA backend.
///
/// Supported algorithms:
///   - kheavyhash / kheavyhash_kas (Kaspa)
///   - blake3 / blake3_alph (Alephium)
///   - blake3_dcr (Decred)
///   - autolykos / autolykos_erg (Ergo)
///   - zelhash / zelhash_flux (FLUX)
///   - ethash / ethash_etc (Ethereum Classic / ETHW)
///   - kawpow / kawpow_rvn (Ravencoin / CLORE / EVR / MEWC)

use anyhow::{Context, Result};
use cudarc::driver::{CudaDevice, CudaSlice, LaunchAsync, LaunchConfig};
use cudarc::driver::sys::CUdevice_attribute;
use cudarc::nvrtc::{compile_ptx_with_opts, CompileOptions};
use std::sync::Arc;
use std::time::Instant;

/// Detect the GPU's compute capability and return an NVRTC-compatible arch string
/// (e.g. "sm_61" for Pascal, "sm_86" for Ampere, "sm_89" for Ada).
/// Falls back to the ZION_CUDA_ARCH env var, then to "sm_86" if detection fails.
fn detect_cuda_arch(dev: &CudaDevice) -> String {
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

use crate::gpu_backend::{GpuBatchResult, GpuMiner, GpuBackendKind};
use zion_core::{DifficultyTarget, MiningHeader};

const SENTINEL_NONCE: u64 = 0xFFFF_FFFF_FFFF_FFFF;
const SENTINEL_FOUND: u32 = 0;

// Kernel sources — included at compile time from AuXpow/csrc/cuda/
const KHEAVYHASH_CU: &str = include_str!("../../../../AuXpow/csrc/cuda/kheavyhash_kernel.cu");
const BLAKE3_CU: &str = include_str!("../../../../AuXpow/csrc/cuda/blake3_kernel.cu");
const AUTOLYKOS_CU: &str = include_str!("../../../../AuXpow/csrc/cuda/autolykos_kernel.cu");
const ZELHASH_CU: &str = include_str!("../../../../AuXpow/csrc/cuda/zelhash_kernel.cu");
const ETHASH_CU: &str = include_str!("../../../../AuXpow/csrc/cuda/ethash_kernel.cu");
const KAWPOW_CU: &str = include_str!("../../../../AuXpow/csrc/cuda/kawpow_kernel.cu");
const ETHASH_DAG_GEN_CU: &str = include_str!("../../../../AuXpow/csrc/cuda/ethash_dag_gen.cu");
const VERUSHASH_CU: &str = include_str!("../../../../AuXpow/csrc/cuda/verushash_kernel.cu");

/// Preprocess kernel source: strip #pragma once and #include lines,
/// prepend standard typedefs, fix NVRTC-incompatible constructs.
fn preprocess_kernel(src: &str) -> String {
    let mut out = String::new();
    // Prepend typedefs that the kernels need
    out.push_str("typedef unsigned char uint8_t;\n");
    out.push_str("typedef unsigned short uint16_t;\n");
    out.push_str("typedef unsigned int uint32_t;\n");
    out.push_str("typedef int int32_t;\n");
    out.push_str("typedef unsigned long long uint64_t;\n");
    out.push_str("typedef long long int64_t;\n");

    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#pragma once")
            || trimmed.starts_with("#include <cuda_runtime.h>")
            || trimmed.starts_with("#include <stdint.h>")
        {
            continue;
        }
        // Fix: __constant__ cannot be used as a function parameter qualifier
        // or local variable qualifier in NVRTC — only for global declarations.
        // Strategy: keep __constant__ only on unindented lines (global
        // declarations like arrays). Remove it from all indented lines
        // (local variables inside device functions) and function parameters.
        let line = if line.starts_with("__constant__") {
            // Global declaration (no indentation) — keep as-is
            line.to_string()
        } else {
            // Indented line or function parameter — strip __constant__
            line.replace("__constant__ ", "")
        };
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Algorithm type for CUDA external mining.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CudaExtAlgo {
    Kheavyhash,
    Blake3Alph,
    Blake3Dcr,
    Autolykos,
    Zelhash,
    Ethash,
    Kawpow,
    Verushash,
}

impl CudaExtAlgo {
    pub fn from_name(algorithm: &str) -> Option<Self> {
        match algorithm {
            "kheavyhash" | "kheavyhash_kas" => Some(Self::Kheavyhash),
            "blake3" | "blake3_alph" => Some(Self::Blake3Alph),
            "blake3_dcr" => Some(Self::Blake3Dcr),
            "autolykos" | "autolykos_erg" => Some(Self::Autolykos),
            "zelhash" | "zelhash_flux" => Some(Self::Zelhash),
            "ethash" | "ethash_etc" | "ethash_ethw" => Some(Self::Ethash),
            "kawpow" | "kawpow_rvn" | "kawpow_clore" | "kawpow_evr" | "kawpow_mewc" => Some(Self::Kawpow),
            "verushash" | "verushash_vrsc" | "verus" => Some(Self::Verushash),
            _ => None,
        }
    }

    fn kernel_name(&self) -> &'static str {
        match self {
            Self::Kheavyhash => "kheavyhash_mine",
            Self::Blake3Alph => "blake3_alph_mine",
            Self::Blake3Dcr => "blake3_dcr_mine",
            Self::Autolykos => "autolykos_mine",
            Self::Zelhash => "zelhash_mine",
            Self::Ethash => "ethash_mine",
            Self::Kawpow => "kawpow_mine",
            Self::Verushash => "verus_mine",
        }
    }

    fn module_name(&self) -> &'static str {
        match self {
            Self::Kheavyhash => "kheavyhash",
            Self::Blake3Alph => "blake3_alph",
            Self::Blake3Dcr => "blake3_dcr",
            Self::Autolykos => "autolykos",
            Self::Zelhash => "zelhash",
            Self::Ethash => "ethash",
            Self::Kawpow => "kawpow",
            Self::Verushash => "verushash",
        }
    }

    fn kernel_source(&self) -> &'static str {
        match self {
            Self::Kheavyhash => KHEAVYHASH_CU,
            Self::Blake3Alph | Self::Blake3Dcr => BLAKE3_CU,
            Self::Autolykos => AUTOLYKOS_CU,
            Self::Zelhash => ZELHASH_CU,
            Self::Ethash => ETHASH_CU,
            Self::Kawpow => KAWPOW_CU,
            Self::Verushash => VERUSHASH_CU,
        }
    }

    /// Returns true if this algorithm requires a DAG buffer.
    fn needs_dag(&self) -> bool {
        matches!(self, Self::Ethash | Self::Kawpow)
    }

    /// Epoch length for DAG-based algorithms.
    fn epoch_length(&self) -> u32 {
        match self {
            Self::Ethash => 30000,
            Self::Kawpow => 7500,
            _ => 0,
        }
    }
}

pub struct CudaExternalMiner {
    dev: Arc<CudaDevice>,
    algo: CudaExtAlgo,
    algorithm: String,
    work_size: usize,
    device_name_cached: String,
    // Common buffers
    header_buf: CudaSlice<u8>,
    target_buf: CudaSlice<u8>,
    output_nonce: CudaSlice<u64>,
    output_hash: CudaSlice<u8>,
    output_solution: CudaSlice<u8>, // 52-byte Equihash solution (zelhash only)
    output_mix: CudaSlice<u8>,      // 32-byte mix hash (ethash/kawpow)
    found_flag: CudaSlice<u32>,
    // Algorithm-specific buffers
    kheavy_matrix: Option<CudaSlice<u16>>,
    autolykos_table: Option<CudaSlice<u64>>,
    autolykos_table_size: u32,
    // DAG buffer for ethash/kawpow
    dag_buf: Option<CudaSlice<u64>>,
    dag_size_entries: u64,
    dag_epoch: u32, // 0xFFFFFFFF = no DAG loaded
    // Light cache for DAG generation (uploaded to GPU for on-GPU DAG gen)
    light_cache_buf: Option<CudaSlice<u64>>,
    light_cache_items: u64,
    // DAG generation kernel module (separate from mining kernel)
    dag_gen_loaded: bool,
    // Cached timestamp for kheavyhash
    kheavy_timestamp: u64,
    // Verushash: precomputed key (552 uint4 = 8832 bytes) and blockhash_half (4 uint4 = 64 bytes)
    verus_vkey: Option<CudaSlice<u32>>,
    verus_blockhash_half: Option<CudaSlice<u32>>,
    // Verushash: per-thread scratch buffer for key workspace
    // TOTAL_MAX (0x10000) * VERUS_KEY_SIZE128 (552) uint4 = 0x10000 * 552 * 16 bytes = ~578 MB
    // This is too large — use a smaller buffer that covers threads_per_block * blocks
    verus_scratch: Option<CudaSlice<u32>>,
}

impl CudaExternalMiner {
    pub fn new(algorithm: &str, work_size: usize) -> Result<Self> {
        let algo = CudaExtAlgo::from_name(algorithm)
            .ok_or_else(|| anyhow::anyhow!("unsupported CUDA external algorithm: {}", algorithm))?;

        let dev = CudaDevice::new(0)
            .map_err(|e| anyhow::anyhow!("CUDA device init failed: {e}"))?;

        let device_name = dev
            .name()
            .unwrap_or_else(|_| "unknown CUDA device".to_string());

        // Compile kernel via NVRTC — auto-detect GPU compute capability
        let arch = detect_cuda_arch(&dev);
        let processed = preprocess_kernel(algo.kernel_source());
        let ptx = compile_ptx_with_opts(
            &processed,
            CompileOptions {
                options: vec![
                    "--use_fast_math".to_string(),
                    format!("-arch={}", arch),
                    "--std=c++14".to_string(),
                ],
                ..Default::default()
            },
        )
        .map_err(|e| anyhow::anyhow!("NVRTC compile failed for {}: {e}", algorithm))?;

        let module_name = algo.module_name();
        let kernel_name = algo.kernel_name();
        dev.load_ptx(ptx, module_name, &[kernel_name])
            .map_err(|e| anyhow::anyhow!("PTX load failed for {}: {e}", algorithm))?;

        // Allocate common buffers
        let header_buf = dev
            .alloc_zeros::<u8>(256)
            .map_err(|e| anyhow::anyhow!("header alloc: {e}"))?;
        let target_buf = dev
            .alloc_zeros::<u8>(32)
            .map_err(|e| anyhow::anyhow!("target alloc: {e}"))?;
        let output_nonce = dev
            .htod_copy(vec![SENTINEL_NONCE])
            .map_err(|e| anyhow::anyhow!("output_nonce alloc: {e}"))?;
        let output_hash = dev
            .alloc_zeros::<u8>(32)
            .map_err(|e| anyhow::anyhow!("output_hash alloc: {e}"))?;
        let output_solution = dev
            .alloc_zeros::<u8>(52)
            .map_err(|e| anyhow::anyhow!("output_solution alloc: {e}"))?;
        let output_mix = dev
            .alloc_zeros::<u8>(32)
            .map_err(|e| anyhow::anyhow!("output_mix alloc: {e}"))?;
        let found_flag = dev
            .htod_copy(vec![SENTINEL_FOUND])
            .map_err(|e| anyhow::anyhow!("found_flag alloc: {e}"))?;

        // Algorithm-specific buffers
        let kheavy_matrix = if algo == CudaExtAlgo::Kheavyhash {
            let matrix = generate_kheavy_matrix_cuda();
            Some(
                dev.htod_copy(matrix.to_vec())
                    .map_err(|e| anyhow::anyhow!("kheavy_matrix alloc: {e}"))?,
            )
        } else {
            None
        };

        let autolykos_table = None; // Generated on first mine_batch

        let actual_work_size = work_size.max(256).min(1 << 20);

        println!(
            "gpu_cuda_ext_init device=\"{}\" algorithm={} work_size={}",
            device_name, algorithm, actual_work_size,
        );

        Ok(Self {
            dev,
            algo,
            algorithm: algorithm.to_string(),
            work_size: actual_work_size,
            device_name_cached: device_name,
            header_buf,
            target_buf,
            output_nonce,
            output_hash,
            output_solution,
            output_mix,
            found_flag,
            kheavy_matrix,
            autolykos_table,
            autolykos_table_size: 0,
            dag_buf: None,
            dag_size_entries: 0,
            dag_epoch: 0xFFFFFFFF,
            light_cache_buf: None,
            light_cache_items: 0,
            dag_gen_loaded: false,
            kheavy_timestamp: 0,
            verus_vkey: None,
            verus_blockhash_half: None,
            verus_scratch: None,
        })
    }

    /// Generate the Autolykos v2 table on the host and upload to GPU.
    fn ensure_autolykos_table(&mut self, header: &[u8], height: u32) -> Result<()> {
        let table_size = autolykos_table_size_cuda();
        let table = generate_autolykos_table_cuda(header, height, table_size);
        self.autolykos_table_size = table_size as u32;
        let table_buf = self
            .dev
            .htod_copy(table)
            .map_err(|e| anyhow::anyhow!("autolykos_table upload: {e}"))?;
        self.autolykos_table = Some(table_buf);
        Ok(())
    }

    /// Precompute the Verushash key and blockhash_half from the block header,
    /// then upload to GPU. Uses the native-ffi VerusHash CPU implementation
    /// for key generation (haraka256 chain hashing).
    fn ensure_verus_key(&mut self, header: &[u8]) -> Result<()> {
        // The Verushash V2.2 key is derived from the block header via:
        //   1. hash_half: Haraka512 chain → 64-byte intermediate
        //   2. prepare_key: GenNewCLKey from intermediate → 8832-byte key
        //   3. get_gpu_keydata: extract key + blockhash_half
        //
        // This must be called on the mining thread (thread-local state).
        #[cfg(feature = "native-verushash")]
        {
            // Use at most 64 bytes of header for hash_half
            let header_padded = {
                let mut buf = vec![0u8; 64];
                let len = header.len().min(64);
                buf[..len].copy_from_slice(&header[..len]);
                buf
            };
            let intermediate = zion_native_ffi::verushash::hash_half(&header_padded);
            zion_native_ffi::verushash::prepare_key(&intermediate);
            let (key_bytes, blockhash_half_bytes) =
                zion_native_ffi::verushash::get_gpu_keydata()
                    .ok_or_else(|| anyhow::anyhow!("verus key precomputation failed"))?;

            // Upload key as u32 array (552 uint4 = 2208 uint32 = 8832 bytes)
            let key_u32: Vec<u32> = key_bytes
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            let blockhash_half_u32: Vec<u32> = blockhash_half_bytes
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();

            let key_buf = self
                .dev
                .htod_copy(key_u32)
                .map_err(|e| anyhow::anyhow!("verus vkey upload: {e}"))?;
            let blockhash_buf = self
                .dev
                .htod_copy(blockhash_half_u32)
                .map_err(|e| anyhow::anyhow!("verus blockhash_half upload: {e}"))?;

            // Allocate scratch buffer: TOTAL_MAX (4096) * VERUS_KEY_SIZE128 (552) uint4
            // = 4096 * 552 * 4 uint32 = 9,043,968 uint32 = ~36MB
            let scratch_size = 4096 * 552 * 4; // uint32 elements
            let scratch_zeros = vec![0u32; scratch_size];
            let scratch_buf = self
                .dev
                .htod_copy(scratch_zeros)
                .map_err(|e| anyhow::anyhow!("verus scratch alloc: {e}"))?;

            self.verus_vkey = Some(key_buf);
            self.verus_blockhash_half = Some(blockhash_buf);
            self.verus_scratch = Some(scratch_buf);
        }

        #[cfg(not(feature = "native-verushash"))]
        {
            let _ = header;
            anyhow::bail!("Verushash CUDA kernel requires native-verushash feature for key precomputation");
        }

        Ok(())
    }

    /// Ensure the DAG for the current epoch is loaded on the GPU.
    /// Generates the light cache on CPU (~16-100MB, fast), uploads it,
    /// then computes the full DAG (1-6GB) IN PARALLEL ON THE GPU using
    /// the ethash_calculate_dag kernel. No multi-GB CPU→GPU transfer.
    fn ensure_dag(&mut self, epoch: u32) -> Result<()> {
        if self.dag_epoch == epoch && self.dag_buf.is_some() {
            return Ok(());
        }

        let algo_name = self.algo.module_name();
        eprintln!(
            "dag_manager: generating {} DAG epoch={} on GPU...",
            algo_name, epoch,
        );
        let start = Instant::now();

        // Step 1: Generate light cache on CPU (small, ~16-100MB, fast)
        let cache_bytes = generate_light_cache(epoch);
        let cache_items = cache_bytes.len() / 64;
        let dag_size_entries = dataset_size_for_epoch(epoch) / 128;
        let dag_nodes = dag_size_entries * 2; // each 128-byte entry = 2 nodes
        let dag_u64s = dag_nodes * 8;         // each 64-byte node = 8 u64

        eprintln!(
            "dag_manager: light cache ready ({} items = {:.1} MB), DAG will be {} nodes = {:.2} GB",
            cache_items,
            cache_bytes.len() as f64 / (1024.0 * 1024.0),
            dag_nodes,
            (dag_u64s as f64 * 8.0) / (1024.0 * 1024.0 * 1024.0),
        );

        // Step 2: Convert cache to u64 array and upload to GPU
        let cache_u64s = cache_items * 8;
        let mut cache_u64 = Vec::with_capacity(cache_u64s as usize);
        for i in 0..cache_u64s as usize {
            let off = i * 8;
            cache_u64.push(u64::from_le_bytes(
                cache_bytes[off..off + 8].try_into().unwrap(),
            ));
        }
        let light_cache_buf = self
            .dev
            .htod_copy(cache_u64)
            .map_err(|e| anyhow::anyhow!("light cache upload: {e}"))?;
        self.light_cache_buf = Some(light_cache_buf);
        self.light_cache_items = cache_items as u64;

        // Step 3: Allocate DAG buffer on GPU (zero-initialized)
        eprintln!(
            "dag_manager: allocating DAG buffer on GPU ({:.2} GB)...",
            (dag_u64s as f64 * 8.0) / (1024.0 * 1024.0 * 1024.0),
        );
        let dag_buf = self
            .dev
            .alloc_zeros::<u64>(dag_u64s as usize)
            .map_err(|e| anyhow::anyhow!("DAG alloc on GPU: {e}"))?;

        // Step 4: Compile and load DAG generation kernel if not already loaded
        if !self.dag_gen_loaded {
            let arch = detect_cuda_arch(&self.dev);
            let processed = preprocess_kernel(ETHASH_DAG_GEN_CU);
            let ptx = compile_ptx_with_opts(
                &processed,
                CompileOptions {
                    options: vec![
                        "--use_fast_math".to_string(),
                        format!("-arch={}", arch),
                        "--std=c++14".to_string(),
                    ],
                    ..Default::default()
                },
            )
            .map_err(|e| anyhow::anyhow!("NVRTC compile failed for dag_gen: {e}"))?;
            self.dev
                .load_ptx(ptx, "dag_gen", &["ethash_calculate_dag"])
                .map_err(|e| anyhow::anyhow!("PTX load failed for dag_gen: {e}"))?;
            self.dag_gen_loaded = true;
        }

        // Step 5: Launch DAG generation kernel in batches
        let dag_gen_func = self
            .dev
            .get_func("dag_gen", "ethash_calculate_dag")
            .ok_or_else(|| anyhow::anyhow!("dag_gen kernel not found"))?;

        let threads_per_block: u32 = 256;
        let batch_nodes: u32 = 8192; // nodes per kernel launch
        let light_cache_ref = self.light_cache_buf.as_ref().unwrap();

        eprintln!(
            "dag_manager: computing DAG on GPU ({} nodes in batches of {})...",
            dag_nodes, batch_nodes,
        );

        let mut node_start: u64 = 0;
        while node_start < dag_nodes {
            let chunk = (dag_nodes - node_start).min(batch_nodes as u64);
            let blocks = ((chunk as u32) + threads_per_block - 1) / threads_per_block;
            let cfg = LaunchConfig {
                grid_dim: (blocks, 1, 1),
                block_dim: (threads_per_block, 1, 1),
                shared_mem_bytes: 0,
            };

            unsafe {
                dag_gen_func
                    .clone()
                    .launch(
                        cfg,
                        (
                            node_start,
                            light_cache_ref,
                            self.light_cache_items,
                            &dag_buf,
                        ),
                    )
                    .map_err(|e| anyhow::anyhow!("dag_gen launch: {e}"))?;
            }

            self.dev
                .synchronize()
                .map_err(|e| anyhow::anyhow!("dag_gen sync: {e}"))?;

            node_start += chunk;

            let pct = (node_start * 100 / dag_nodes).min(100);
            if pct % 10 == 0 || node_start == dag_nodes {
                eprintln!(
                    "dag_manager: DAG generation {}% ({}/{}, {:.1}s)",
                    pct, node_start, dag_nodes,
                    start.elapsed().as_secs_f64(),
                );
            }
        }

        self.dag_buf = Some(dag_buf);
        self.dag_size_entries = dag_size_entries;
        self.dag_epoch = epoch;

        eprintln!(
            "dag_manager: {} DAG epoch={} ready on GPU ({:.1}s total)",
            algo_name, epoch,
            start.elapsed().as_secs_f64(),
        );

        Ok(())
    }

    fn run_kernel(
        &mut self,
        header: &[u8],
        target: &[u8; 32],
        nonce_start: u64,
        batch_size: u64,
    ) -> Result<GpuBatchResult> {
        // Reset found flag and sentinel
        self.dev
            .htod_copy_into(vec![SENTINEL_FOUND], &mut self.found_flag)
            .map_err(|e| anyhow::anyhow!("reset found: {e}"))?;
        self.dev
            .htod_copy_into(vec![SENTINEL_NONCE], &mut self.output_nonce)
            .map_err(|e| anyhow::anyhow!("reset nonce: {e}"))?;

        // Upload header (pad to buffer size — htod_copy_into requires matching lengths)
        let header_len = header.len().min(256);
        let mut header_padded = vec![0u8; 256];
        header_padded[..header_len].copy_from_slice(&header[..header_len]);
        self.dev
            .htod_copy_into(header_padded, &mut self.header_buf)
            .map_err(|e| anyhow::anyhow!("header upload: {e}"))?;

        // Upload target
        self.dev
            .htod_copy_into(target.to_vec(), &mut self.target_buf)
            .map_err(|e| anyhow::anyhow!("target upload: {e}"))?;

        let func = self
            .dev
            .get_func(self.algo.module_name(), self.algo.kernel_name())
            .ok_or_else(|| {
                anyhow::anyhow!("kernel {} not found", self.algo.kernel_name())
            })?;

        let threads_per_block: u32 = if self.algo == CudaExtAlgo::Verushash {
            128 // Verushash kernel uses __launch_bounds__(128)
        } else {
            // Configurable via ZION_CUDA_BLOCK_SIZE env var.
            // Default 256 (optimal for Ampere/Ada). For Pascal/Turing (GTX 1080, etc.),
            // 128 or 192 may give better occupancy due to smaller register file.
            // The kernel __launch_bounds__(256) allows up to 256; lower values are safe.
            std::env::var("ZION_CUDA_BLOCK_SIZE")
                .ok()
                .and_then(|v| v.trim().parse::<u32>().ok())
                .filter(|&v| v > 0 && v <= 256)
                .unwrap_or(256)
        };
        // Run multiple kernel launches to cover the full batch_size.
        // Each launch covers at most self.work_size nonces.
        let mut total_tested: u64 = 0;
        let mut current_nonce = nonce_start;
        let mut left = batch_size;

        while left > 0 {
            let chunk = (left as u32).min(self.work_size as u32);
            let blocks = (chunk + threads_per_block - 1) / threads_per_block;
            let cfg = LaunchConfig {
                grid_dim: (blocks, 1, 1),
                block_dim: (threads_per_block, 1, 1),
                shared_mem_bytes: 0,
            };

            unsafe {
                match self.algo {
                    CudaExtAlgo::Kheavyhash => {
                        let matrix = self.kheavy_matrix.as_ref().unwrap();
                        func
                            .clone()
                            .launch(
                                cfg,
                                (
                                    &self.header_buf,
                                    self.kheavy_timestamp,
                                    &self.target_buf,
                                    current_nonce,
                                    matrix,
                                    &mut self.output_nonce,
                                    &mut self.output_hash,
                                    &mut self.found_flag,
                                ),
                            )
                            .map_err(|e| anyhow::anyhow!("kheavyhash launch: {e}"))?;
                    }
                    CudaExtAlgo::Blake3Alph => {
                        let header_len_u32 = header_len as u32;
                        func
                            .clone()
                            .launch(
                                cfg,
                                (
                                    &self.header_buf,
                                    header_len_u32,
                                    &self.target_buf,
                                    current_nonce,
                                    &mut self.output_nonce,
                                    &mut self.output_hash,
                                    &mut self.found_flag,
                                ),
                            )
                            .map_err(|e| anyhow::anyhow!("blake3_alph launch: {e}"))?;
                    }
                    CudaExtAlgo::Blake3Dcr => {
                        let header_len_u32 = header_len as u32;
                        func
                            .clone()
                            .launch(
                                cfg,
                                (
                                    &self.header_buf,
                                    header_len_u32,
                                    &self.target_buf,
                                    current_nonce,
                                    &mut self.output_nonce,
                                    &mut self.output_hash,
                                    &mut self.found_flag,
                                ),
                            )
                            .map_err(|e| anyhow::anyhow!("blake3_dcr launch: {e}"))?;
                    }
                    CudaExtAlgo::Autolykos => {
                        let table = self
                            .autolykos_table
                            .as_ref()
                            .ok_or_else(|| anyhow::anyhow!("autolykos table not generated"))?;
                        let header_len_u32 = header_len as u32;
                        let table_size_u32 = self.autolykos_table_size;
                        func
                            .clone()
                            .launch(
                                cfg,
                                (
                                    &self.header_buf,
                                    header_len_u32,
                                    &self.target_buf,
                                    current_nonce,
                                    table,
                                    table_size_u32,
                                    &mut self.output_nonce,
                                    &mut self.output_hash,
                                    &mut self.found_flag,
                                ),
                            )
                            .map_err(|e| anyhow::anyhow!("autolykos launch: {e}"))?;
                    }
                    CudaExtAlgo::Zelhash => {
                        let header_len_u32 = header_len as u32;
                        func
                            .clone()
                            .launch(
                                cfg,
                                (
                                    &self.header_buf,
                                    header_len_u32,
                                    &self.target_buf,
                                    current_nonce,
                                    &mut self.output_nonce,
                                    &mut self.output_hash,
                                    &mut self.output_solution,
                                    &mut self.found_flag,
                                ),
                            )
                            .map_err(|e| anyhow::anyhow!("zelhash launch: {e}"))?;
                    }
                    CudaExtAlgo::Ethash => {
                        let dag = self
                            .dag_buf
                            .as_ref()
                            .ok_or_else(|| anyhow::anyhow!("ethash DAG not loaded"))?;
                        let dag_size = self.dag_size_entries;
                        func
                            .clone()
                            .launch(
                                cfg,
                                (
                                    &self.header_buf,
                                    &self.target_buf,
                                    current_nonce,
                                    1u64, // stride
                                    dag,
                                    dag_size,
                                    &mut self.output_nonce,
                                    &mut self.output_hash,
                                    &mut self.output_mix,
                                    &mut self.found_flag,
                                ),
                            )
                            .map_err(|e| anyhow::anyhow!("ethash launch: {e}"))?;
                    }
                    CudaExtAlgo::Kawpow => {
                        let dag = self
                            .dag_buf
                            .as_ref()
                            .ok_or_else(|| anyhow::anyhow!("kawpow DAG not loaded"))?;
                        let dag_entries = self.dag_size_entries;
                        func
                            .clone()
                            .launch(
                                cfg,
                                (
                                    &self.header_buf,
                                    &self.target_buf,
                                    current_nonce,
                                    dag,
                                    dag_entries,
                                    &mut self.output_nonce,
                                    &mut self.output_hash,
                                    &mut self.output_mix,
                                    &mut self.found_flag,
                                ),
                            )
                            .map_err(|e| anyhow::anyhow!("kawpow launch: {e}"))?;
                    }
                    CudaExtAlgo::Verushash => {
                        let vkey = self.verus_vkey.as_ref().ok_or_else(|| {
                            anyhow::anyhow!("verus vkey not precomputed")
                        })?;
                        let blockhash_half = self.verus_blockhash_half.as_ref().ok_or_else(|| {
                            anyhow::anyhow!("verus blockhash_half not set")
                        })?;
                        let scratch = self.verus_scratch.as_ref().ok_or_else(|| {
                            anyhow::anyhow!("verus scratch not allocated")
                        })?;
                        func
                            .clone()
                            .launch(
                                cfg,
                                (
                                    vkey,
                                    blockhash_half,
                                    &self.target_buf,
                                    scratch,
                                    current_nonce,
                                    &mut self.output_nonce,
                                    &mut self.output_hash,
                                    &mut self.found_flag,
                                ),
                            )
                            .map_err(|e| anyhow::anyhow!("verushash launch: {e}"))?;
                    }
                }
            }

            total_tested += chunk as u64;
            current_nonce += chunk as u64;
            left = left.saturating_sub(chunk as u64);
        }

        // Single sync point: wait for ALL chunks to complete
        self.dev
            .synchronize()
            .map_err(|e| anyhow::anyhow!("device sync: {e}"))?;

        let found_host = self
            .dev
            .dtoh_sync_copy(&self.found_flag)
            .map_err(|e| anyhow::anyhow!("found download: {e}"))?;

        if found_host[0] != 0 {
            let nonce_host = self
                .dev
                .dtoh_sync_copy(&self.output_nonce)
                .map_err(|e| anyhow::anyhow!("nonce download: {e}"))?;
            let hash_host = self
                .dev
                .dtoh_sync_copy(&self.output_hash)
                .map_err(|e| anyhow::anyhow!("hash download: {e}"))?;
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&hash_host);

            // Read mix hash for ethash/kawpow (needed for pool submission)
            let mix_hash = if self.algo == CudaExtAlgo::Ethash
                || self.algo == CudaExtAlgo::Kawpow
            {
                let mix_host = self
                    .dev
                    .dtoh_sync_copy(&self.output_mix)
                    .map_err(|e| anyhow::anyhow!("mix download: {e}"))?;
                let mut mix = [0u8; 32];
                mix.copy_from_slice(&mix_host);
                Some(mix)
            } else {
                None
            };

            Ok(GpuBatchResult {
                solutions: vec![(nonce_host[0], hash, mix_hash)],
                nonces_tested: total_tested,
            })
        } else {
            Ok(GpuBatchResult {
                solutions: Vec::new(),
                nonces_tested: total_tested,
            })
        }
    }
}

impl GpuMiner for CudaExternalMiner {
    fn device_name(&self) -> String {
        self.device_name_cached.clone()
    }

    fn backend_kind(&self) -> GpuBackendKind {
        GpuBackendKind::Cuda
    }

    fn algorithm(&self) -> String {
        self.algorithm.clone()
    }

    fn update_epoch(&mut self, height: u64) -> Result<()> {
        if self.algo.needs_dag() {
            let epoch = (height / self.algo.epoch_length() as u64) as u32;
            self.ensure_dag(epoch)?;
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

        if self.algo == CudaExtAlgo::Kheavyhash {
            let pre_pow_hash = &header_bytes[..32];
            self.kheavy_timestamp = header.timestamp;
            return self.run_kernel(pre_pow_hash, &target.bytes, nonce_start, batch_size);
        }

        if self.algo == CudaExtAlgo::Autolykos {
            let height = header.timestamp as u32;
            self.ensure_autolykos_table(&header_bytes, height)?;
        }

        // Verushash: precompute key from block header
        if self.algo == CudaExtAlgo::Verushash {
            self.ensure_verus_key(&header_bytes)?;
        }

        // Ethash/Kawpow: header is 32-byte block header hash, epoch from height
        if self.algo.needs_dag() {
            // NOTE: update_epoch(height) is called by the external GPU thread
            // before mine_batch, which loads the correct DAG for the block's
            // epoch. Do NOT recompute epoch from header.timestamp here — for
            // external ethash/kawpow jobs, the header is just a 32-byte hash
            // padded to 80 bytes, so timestamp=0 → epoch=0, which would
            // overwrite the correct DAG with the wrong one.
            // Only call ensure_dag if no DAG is loaded yet (e.g. benchmark).
            if self.dag_epoch == 0xFFFFFFFF {
                self.ensure_dag(0)?;
            }
            // For ethash/kawpow, only the first 32 bytes (header hash) are used
            let header_hash = &header_bytes[..32.min(header_bytes.len())];
            return self.run_kernel(header_hash, &target.bytes, nonce_start, batch_size);
        }

        self.run_kernel(&header_bytes, &target.bytes, nonce_start, batch_size)
    }

    fn mine_batch_raw(
        &mut self,
        raw_header: &[u8],
        target: DifficultyTarget,
        nonce_start: u64,
        batch_size: u64,
    ) -> Result<GpuBatchResult> {
        if self.algo == CudaExtAlgo::Kheavyhash {
            let pre_pow_hash = &raw_header[..32.min(raw_header.len())];
            self.kheavy_timestamp = 0;
            return self.run_kernel(pre_pow_hash, &target.bytes, nonce_start, batch_size);
        }

        if self.algo == CudaExtAlgo::Autolykos {
            let height = 0u32;
            self.ensure_autolykos_table(raw_header, height)?;
        }

        // Verushash: precompute key from raw header
        if self.algo == CudaExtAlgo::Verushash {
            self.ensure_verus_key(raw_header)?;
        }

        // Ethash/Kawpow: ensure DAG for epoch 0 (benchmark mode)
        if self.algo.needs_dag() {
            self.ensure_dag(0)?;
            let header_hash = &raw_header[..32.min(raw_header.len())];
            return self.run_kernel(header_hash, &target.bytes, nonce_start, batch_size);
        }

        self.run_kernel(raw_header, &target.bytes, nonce_start, batch_size)
    }

    fn benchmark(&mut self, secs: f64) -> Result<(u64, f64, f64)> {
        // For DAG-based algorithms, use mine_batch_raw which calls ensure_dag(0)
        if self.algo.needs_dag() {
            self.ensure_dag(0)?;
            let start = Instant::now();
            let mut total: u64 = 0;
            let mut nonce: u64 = 0;
            let header = [0xAAu8; 32];
            let target = DifficultyTarget { bytes: [0xFFu8; 32] };
            while start.elapsed().as_secs_f64() < secs {
                let result = self.run_kernel(&header, &target.bytes, nonce, self.work_size as u64)?;
                total += result.nonces_tested;
                nonce = nonce.wrapping_add(self.work_size as u64);
            }
            let elapsed = start.elapsed().as_secs_f64();
            let hps = if elapsed > 0.0 { total as f64 / elapsed } else { 0.0 };
            return Ok((total, elapsed, hps));
        }

        let start = Instant::now();
        let mut total: u64 = 0;
        let mut nonce: u64 = 0;
        let header = MiningHeader {
            version: 3,
            previous_hash: [0xAA; 32],
            merkle_root: [0xBB; 32],
            timestamp: 1_762_000_200,
            difficulty_bits: 0x1f00ffff,
        };
        let target = DifficultyTarget { bytes: [0xFFu8; 32] };
        while start.elapsed().as_secs_f64() < secs {
            let result = self.mine_batch(header, target, nonce, self.work_size as u64)?;
            total += result.nonces_tested;
            nonce = nonce.wrapping_add(self.work_size as u64);
        }
        let elapsed = start.elapsed().as_secs_f64();
        let hps = if elapsed > 0.0 { total as f64 / elapsed } else { 0.0 };
        Ok((total, elapsed, hps))
    }
}

// ── Host-side helper functions ─────────────────────────────────────────────

/// Generate the 64x64 kHeavyHash matrix (4096 u16 values).
fn generate_kheavy_matrix_cuda() -> [u16; 4096] {
    use std::sync::OnceLock;
    static MATRIX: OnceLock<[u16; 4096]> = OnceLock::new();
    *MATRIX.get_or_init(|| {
        use sha3::{Digest, Sha3_256};
        let seed = Sha3_256::digest(b"KHeavyHash");
        let mut rng = XoShiRo256PlusPlus::new(seed.into());
        loop {
            let mut mat = [[0u16; 64]; 64];
            for row in &mut mat {
                let mut val = 0u64;
                for (j, elem) in row.iter_mut().enumerate() {
                    let shift = j % 16;
                    if shift == 0 {
                        val = rng.next();
                    }
                    *elem = ((val >> (4 * shift)) & 0x0F) as u16;
                }
            }
            if compute_rank_64(&mat) == 64 {
                let mut flat = [0u16; 4096];
                for i in 0..64 {
                    for j in 0..64 {
                        flat[i * 64 + j] = mat[i][j];
                    }
                }
                return flat;
            }
        }
    })
}

fn autolykos_table_size_cuda() -> usize {
    std::env::var("ZION_AUTOLYKOS_TABLE_SIZE")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(1 << 23)
}

fn generate_autolykos_table_cuda(header: &[u8], height: u32, table_size: usize) -> Vec<u64> {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(header);
    let seed: [u8; 32] = h.finalize().into();
    (0..table_size)
        .map(|i| gen_autolykos_element_cuda(i as u64, &seed, height))
        .collect()
}

fn gen_autolykos_element_cuda(i: u64, seed: &[u8; 32], height: u32) -> u64 {
    use blake2::digest::{Update, VariableOutput};
    let mut hasher = blake2::Blake2bVar::new(32).expect("blake2b256");
    hasher.update(seed);
    hasher.update(&i.to_be_bytes());
    hasher.update(&height.to_be_bytes());
    let mut out = [0u8; 32];
    hasher.finalize_variable(&mut out).expect("blake2b256 finalize");
    u64::from_be_bytes(out[0..8].try_into().unwrap())
}

// ── XoShiRo256++ PRNG ──────────────────────────────────────────────────────

struct XoShiRo256PlusPlus {
    state: [u64; 4],
}

impl XoShiRo256PlusPlus {
    fn new(seed: [u8; 32]) -> Self {
        let mut s = [0u64; 4];
        for i in 0..4 {
            s[i] = u64::from_le_bytes(seed[i * 8..(i + 1) * 8].try_into().unwrap());
        }
        Self { state: s }
    }

    fn next(&mut self) -> u64 {
        let result = Self::rotl(self.state[0].wrapping_add(self.state[3]), 23)
            .wrapping_add(self.state[0]);
        let t = self.state[1] << 17;
        self.state[2] ^= self.state[0];
        self.state[3] ^= self.state[1];
        self.state[1] ^= self.state[2];
        self.state[0] ^= self.state[3];
        self.state[2] ^= t;
        self.state[3] = Self::rotl(self.state[3], 45);
        result
    }

    fn rotl(x: u64, k: u32) -> u64 {
        (x << k) | (x >> (64 - k))
    }
}

/// Compute the rank of a 64x64 matrix over GF(2^4) (4-bit entries).
fn compute_rank_64(mat: &[[u16; 64]; 64]) -> usize {
    let mut m = mat.map(|row| row.map(|v| v as u32));
    let mut rank = 0;
    let mut col = 0;
    while col < 64 && rank < 64 {
        let mut pivot = None;
        for r in rank..64 {
            if m[r][col] != 0 {
                pivot = Some(r);
                break;
            }
        }
        if let Some(p) = pivot {
            m.swap(rank, p);
            let pivot_val = m[rank][col];
            if pivot_val != 0 {
                let inv = mod_inv_15(pivot_val);
                for c in col..64 {
                    m[rank][c] = (m[rank][c] * inv) & 0x0F;
                }
            }
            for r in 0..64 {
                if r != rank && m[r][col] != 0 {
                    let factor = m[r][col];
                    for c in col..64 {
                        m[r][c] = ((m[r][c] + 16) - ((m[rank][c] * factor) & 0x0F)) & 0x0F;
                    }
                }
            }
            rank += 1;
        }
        col += 1;
    }
    rank
}

fn mod_inv_15(a: u32) -> u32 {
    for x in 1..16u32 {
        if (a * x) & 0x0F == 1 {
            return x;
        }
    }
    1
}

// ── Ethash/Kawpow DAG generation (CPU-side) ────────────────────────────────
//
// These functions implement the Ethash light cache + full DAG generation
// in pure Rust, matching the algorithm in AuXpow/src/native_ffi.rs.
// The DAG is generated on the CPU and uploaded to the GPU as a u64 buffer.

const DAG_CACHE_ROUNDS: usize = 3;

/// Compute the cache size for a given epoch.
/// Formula: 16 MB + epoch * 128 KB, rounded to 64-byte boundary.
fn cache_size_for_epoch(epoch: u32) -> u64 {
    let size = 16u64 * 1024 * 1024 + epoch as u64 * 128 * 1024;
    (size / 64) * 64
}

/// Compute the dataset (DAG) size for a given epoch.
/// Formula: 1 GB + epoch * 8 MB, rounded to 128-byte boundary.
fn dataset_size_for_epoch(epoch: u32) -> u64 {
    let size = 1024u64 * 1024 * 1024 + epoch as u64 * 8 * 1024 * 1024;
    (size / 128) * 128
}

/// Compute the seed hash for an epoch by keccak-256 chaining.
fn seed_hash_for_epoch(epoch: u32) -> [u8; 32] {
    use sha3::{Digest, Keccak256};
    let mut seed = [0u8; 32];
    for _ in 0..epoch {
        let mut hasher = Keccak256::new();
        hasher.update(&seed);
        seed = hasher.finalize().into();
    }
    seed
}

/// Generate the Ethash/Kawpow light cache for a given epoch.
/// Returns a Vec<u8> of size cache_size_for_epoch(epoch).
fn generate_light_cache(epoch: u32) -> Vec<u8> {
    use sha3::{Digest, Keccak512};

    let cache_size = cache_size_for_epoch(epoch) as usize;
    let cache_items = cache_size / 64;
    let seed = seed_hash_for_epoch(epoch);

    let mut cache = vec![0u8; cache_size];

    // First item = keccak512(seed)
    {
        let mut hasher = Keccak512::new();
        hasher.update(&seed);
        let hash = hasher.finalize();
        cache[..64].copy_from_slice(&hash);
    }

    // Chain: each item = keccak512(prev_item)
    for i in 1..cache_items {
        let mut hasher = Keccak512::new();
        hasher.update(&cache[(i - 1) * 64..i * 64]);
        let hash = hasher.finalize();
        cache[i * 64..(i + 1) * 64].copy_from_slice(&hash);
    }

    // RANDMEMOHASH mixing rounds
    for _r in 0..DAG_CACHE_ROUNDS {
        for i in 0..cache_items {
            let v = u32::from_le_bytes([
                cache[i * 64],
                cache[i * 64 + 1],
                cache[i * 64 + 2],
                cache[i * 64 + 3],
            ]) % cache_items as u32;
            let prev = (i + cache_items - 1) % cache_items;

            let mut tmp = [0u8; 64];
            for j in 0..64 {
                tmp[j] = cache[prev * 64 + j] ^ cache[v as usize * 64 + j];
            }

            let mut hasher = Keccak512::new();
            hasher.update(&tmp);
            let hash = hasher.finalize();
            cache[i * 64..(i + 1) * 64].copy_from_slice(&hash);
        }
    }

    cache
}

/// FNV-1a hash for u32 pairs (used in light cache generation only).
fn fnv1a_u32(a: u32, b: u32) -> u32 {
    a.wrapping_mul(0x01000193) ^ b
}
