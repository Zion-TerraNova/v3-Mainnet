//! GPU Guard — crash recovery wrapper for OpenCL / GPU operations.
//!
//! On Windows, AMD OpenCL driver crashes manifest as SEH
//! `EXCEPTION_ACCESS_VIOLATION` (0xC0000005) inside `amdocl64.dll`.
//! This module wraps GPU kernel enqueue / buffer transfers with a
//! vectored exception handler so the miner can recover instead of
//! terminating the whole process.
//!
//! Also provides:
//! - `GpuDeviceFamily`  (GCN vs RDNA vs Unknown) auto-detection
//! - `GpuAlgorithm`     (CosmicHarmony vs DeekshaLiteV1) selector
//! - `GpuTuning`        per-family optimal work_size / local_ws / build flags

// This module is only fully exercised under the GPU feature builds; without a
// GPU backend feature its types are unused. Integer tuning clamps mirror the
// kernel limits and are written as max().min() pairs for readability.
#![allow(dead_code)]
#![allow(clippy::manual_clamp)]

// ========================================================================
// 1) Windows SEH Vectored Exception Handler (raw FFI, no extra crates)
// ========================================================================

#[cfg(windows)]
mod win_seh {
    use std::os::raw::{c_long, c_void};
    use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

    pub const EXCEPTION_ACCESS_VIOLATION: u32 = 0xC0000005;

    #[repr(C)]
    pub struct ExceptionRecord {
        pub exception_code: u32,
        pub exception_flags: u32,
        pub exception_record: *mut ExceptionRecord,
        pub exception_address: *mut c_void,
        pub number_parameters: u32,
        pub exception_information: [usize; 15],
    }

    #[repr(C)]
    pub struct ContextRecord {
        _opaque: [u8; 0],
    }

    #[repr(C)]
    pub struct ExceptionPointers {
        pub exception_record: *mut ExceptionRecord,
        pub context_record: *mut ContextRecord,
    }

    pub type PVECTORED_EXCEPTION_HANDLER =
        Option<extern "system" fn(*mut ExceptionPointers) -> c_long>;

    extern "system" {
        pub fn AddVectoredExceptionHandler(
            first: u32,
            handler: PVECTORED_EXCEPTION_HANDLER,
        ) -> *mut c_void;
        pub fn RemoveVectoredExceptionHandler(handle: *mut c_void) -> u32;
    }

    pub const EXCEPTION_EXECUTE_HANDLER: c_long = 1;
    pub const EXCEPTION_CONTINUE_SEARCH: c_long = 0;

    static CAUGHT: AtomicBool = AtomicBool::new(false);

    extern "system" fn veh_handler(info: *mut ExceptionPointers) -> c_long {
        unsafe {
            if info.is_null() || (*info).exception_record.is_null() {
                return EXCEPTION_CONTINUE_SEARCH;
            }
            let code = (*(*info).exception_record).exception_code;
            if code == EXCEPTION_ACCESS_VIOLATION {
                CAUGHT.store(true, Ordering::SeqCst);
                // Tell Windows we handled it — Rust will see the flag
                return EXCEPTION_EXECUTE_HANDLER;
            }
        }
        EXCEPTION_CONTINUE_SEARCH
    }

    pub fn install() -> *mut c_void {
        CAUGHT.store(false, Ordering::SeqCst);
        unsafe { AddVectoredExceptionHandler(1, Some(veh_handler)) }
    }

    pub fn uninstall(handle: *mut c_void) {
        if !handle.is_null() {
            unsafe {
                RemoveVectoredExceptionHandler(handle);
            }
        }
    }

    pub fn was_caught() -> bool {
        CAUGHT.swap(false, Ordering::SeqCst)
    }
}

#[cfg(not(windows))]
mod win_seh {
    pub fn install() -> *mut std::os::raw::c_void {
        std::ptr::null_mut()
    }
    pub fn uninstall(_: *mut std::os::raw::c_void) {}
    pub fn was_caught() -> bool {
        false
    }
}

/// Guard that installs a vectored exception handler for the scope.
pub struct GpuGuard {
    handle: *mut std::os::raw::c_void,
}

impl GpuGuard {
    pub fn new() -> Self {
        Self {
            handle: win_seh::install(),
        }
    }

    /// Returns `true` if an access-violation was caught during the guard's
    /// lifetime (and resets the flag).
    pub fn was_caught(&self) -> bool {
        win_seh::was_caught()
    }
}

impl Drop for GpuGuard {
    fn drop(&mut self) {
        win_seh::uninstall(self.handle);
    }
}

// ========================================================================
// 2) Device Family & Algorithm enums
// ========================================================================

/// Detected GPU architecture family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuDeviceFamily {
    /// AMD GCN 1st–5th gen: gfx6xx–gfx9xx (Vega, Polaris, Fiji, Tonga, Hawaii)
    AmdGcn,
    /// AMD RDNA 1–3: gfx10xx+ (RX 5000, 6000, 7000, 9000)
    AmdRdna,
    /// NVIDIA CUDA (any arch)
    Nvidia,
    /// Apple Silicon / Intel iGPU / other
    Other,
}

impl GpuDeviceFamily {
    /// Auto-detect from OpenCL device name string.
    pub fn from_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        // RDNA MUST be checked first: "rx 5" (RX 5000 = RDNA1, gfx1010) would
        // otherwise fall into the GCN branch below ("rx 5" was listed there).
        // RX 5500/5600/5700 = RDNA1 (gfx1010/1011/1012), not GCN.
        if lower.contains("gfx10")
            || lower.contains("gfx11")
            || lower.contains("gfx12")
            || lower.contains("rdna")
            || lower.contains("rx 5")  // RX 5500/5600/5700 = RDNA1
            || lower.contains("rx 6")
            || lower.contains("rx 7")
            || lower.contains("rx 9")
            || lower.contains("rx 79")
            || lower.contains("rx 89")
        {
            return Self::AmdRdna;
        }
        if lower.contains("gfx6")
            || lower.contains("gfx7")
            || lower.contains("gfx8")
            || lower.contains("gfx9")
            || lower.contains("vega")
            || lower.contains("polaris")
            || lower.contains("fiji")
            || lower.contains("tonga")
            || lower.contains("hawaii")
            || lower.contains("rx 4")
            || lower.contains("r9 ")
            || lower.contains("r7 ")
            || lower.contains("r5 ")
            || lower.contains("hd 7")
            || lower.contains("hd 8")
        {
            return Self::AmdGcn;
        }
        // NVIDIA
        if lower.contains("nvidia")
            || lower.contains("geforce")
            || lower.contains("rtx")
            || lower.contains("gtx")
            || lower.contains("quadro")
        {
            return Self::Nvidia;
        }
        Self::Other
    }

    /// Is this an AMD GCN part that needs conservative compiler flags?
    pub fn needs_gcn_workarounds(self) -> bool {
        self == Self::AmdGcn
    }

    /// Is this an AMD RDNA part that can use the fast ulong-width path?
    pub fn is_rdna(self) -> bool {
        self == Self::AmdRdna
    }

    /// Is this any AMD GPU (GCN or RDNA)?
    pub fn is_amd(self) -> bool {
        self == Self::AmdGcn || self == Self::AmdRdna
    }
}

/// Canonical mining algorithms on GPU.
/// Three algorithms only: Deeksha (full Ekam), Lite v1, Fire.
/// Experimental variants live in DeekshaDebug/ sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuAlgorithm {
    /// Original cosmic_harmony Deeksha (full pipeline with NPU, Blake3, etc.)
    CosmicHarmony,
    /// Canonical deeksha_lite_v1 — 256 KiB scratchpad, SHA3-512, 64 reads, 4 AES rounds
    DeekshaLiteV1,
    /// Canonical deeksha_lite_fire — 256 KiB scratchpad + 65536-iter thermal loop
    DeekshaLiteFire,
}

impl GpuAlgorithm {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "deeksha_lite_v1" | "deeksha_lite" | "lite" | "dl" | "dlv1" => Self::DeekshaLiteV1,
            "deeksha_lite_fire" | "fire" | "dlfire" => Self::DeekshaLiteFire,
            _ => Self::CosmicHarmony,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CosmicHarmony => "cosmic_harmony",
            Self::DeekshaLiteV1 => "deeksha_lite_v1",
            Self::DeekshaLiteFire => "deeksha_lite_fire",
        }
    }
}

// ========================================================================
// 3) Per-family tuning defaults
// ========================================================================

/// Tuning parameters derived from device family and available VRAM.
#[derive(Debug, Clone)]
pub struct GpuTuning {
    /// Global work size (number of nonces per kernel enqueue)
    pub work_size: usize,
    /// Local work size (work-group size)
    pub local_ws: usize,
    /// OpenCL build flags
    pub build_opts: String,
    /// Percentage of VRAM to use (0–100)
    pub vram_pct: u8,
    /// Whether to enable the GCN s4_mode fallback (cosmic_harmony only)
    pub gcn_s4_mode: bool,
    /// Maximum scratchpad bytes per thread (algorithm-specific)
    pub scratchpad_bytes: usize,
}

impl GpuTuning {
    /// Compute optimal tuning for a given algorithm + device family + VRAM.
    pub fn auto_tune(algo: GpuAlgorithm, family: GpuDeviceFamily, vram_bytes: usize) -> Self {
        let scratchpad_bytes = match algo {
            GpuAlgorithm::CosmicHarmony => 256 * 1024, // 256 KiB per thread
            GpuAlgorithm::DeekshaLiteV1 => 256 * 1024, // 256 KiB per thread
            GpuAlgorithm::DeekshaLiteFire => 256 * 1024, // 256 KiB per thread (same as v1 + thermal loop)
        };

        let reserve = 512 * 1024 * 1024; // 512 MiB for driver + desktop
        let available = vram_bytes.saturating_sub(reserve);
        let per_thread = scratchpad_bytes + 128; // scratchpad + output + margin
        let max_by_vram = available / per_thread;

        let (work_size, local_ws, build_opts, vram_pct, gcn_s4_mode) = match (algo, family) {
            // ── DeekshaLite v1 ──────────────────────────────────────────
            (GpuAlgorithm::DeekshaLiteV1, GpuDeviceFamily::AmdGcn) => {
                // GCN Vega 64: 8GB HBM2 → 16384, stable local_ws=256
                let ws = (max_by_vram.min(16384).max(256)).next_power_of_two();
                let opts = "-cl-std=CL1.2".to_string();
                (ws, 256, opts, 85, false)
            }
            (GpuAlgorithm::DeekshaLiteV1, GpuDeviceFamily::AmdRdna) => {
                // RDNA: fast ulong-width path, smaller local size for better occupancy
                // NO -cl-fast-relaxed-math: causes AMD driver crashes on integer
                // code paths (Keccak scratchpad, AES) on some RDNA driver versions.
                let ws = (max_by_vram.min(8192).max(512)).next_power_of_two();
                let opts = "-cl-std=CL1.2 -cl-mad-enable".to_string();
                (ws, 128, opts, 85, false)
            }
            (GpuAlgorithm::DeekshaLiteV1, GpuDeviceFamily::Nvidia) => {
                let ws = (max_by_vram.min(8192).max(512)).next_power_of_two();
                let opts = "-cl-std=CL1.2 -cl-mad-enable -cl-fast-relaxed-math".to_string();
                (ws, 128, opts, 80, false)
            }
            (GpuAlgorithm::DeekshaLiteV1, GpuDeviceFamily::Other) => {
                let ws = (max_by_vram.min(4096).max(256)).next_power_of_two();
                let opts = "-cl-std=CL1.2 -cl-mad-enable".to_string();
                (ws, 128, opts, 70, false)
            }

            // ── DeekshaLite Fire (thermal-intensive) ────────────────────
            // 256 KiB scratchpad (same as v1) + 65536-iter integer thermal loop.
            (GpuAlgorithm::DeekshaLiteFire, GpuDeviceFamily::AmdGcn) => {
                let ws = (max_by_vram.min(16384).max(128)).next_power_of_two();
                let opts = "-cl-std=CL1.2".to_string();
                (ws, 256, opts, 85, false)
            }
            (GpuAlgorithm::DeekshaLiteFire, GpuDeviceFamily::AmdRdna) => {
                let ws = (max_by_vram.min(8192).max(512)).next_power_of_two();
                let opts = "-cl-std=CL1.2 -cl-mad-enable".to_string();
                (ws, 128, opts, 85, false)
            }
            (GpuAlgorithm::DeekshaLiteFire, GpuDeviceFamily::Nvidia) => {
                let ws = (max_by_vram.min(4096).max(256)).next_power_of_two();
                let opts = "-cl-std=CL1.2 -cl-mad-enable".to_string();
                (ws, 128, opts, 75, false)
            }
            (GpuAlgorithm::DeekshaLiteFire, GpuDeviceFamily::Other) => {
                let ws = (max_by_vram.min(2048).max(128)).next_power_of_two();
                let opts = "-cl-std=CL1.2 -cl-mad-enable".to_string();
                (ws, 128, opts, 65, false)
            }

            // ── Cosmic Harmony ──────────────────────────────────────────
            (GpuAlgorithm::CosmicHarmony, GpuDeviceFamily::AmdGcn) => {
                let ws = (max_by_vram.min(16384).max(128)).next_power_of_two();
                let opts = "-cl-std=CL1.2".to_string();
                (ws, 256, opts, 85, false)
            }
            (GpuAlgorithm::CosmicHarmony, GpuDeviceFamily::AmdRdna) => {
                let ws = (max_by_vram.min(8192).max(512)).next_power_of_two();
                let opts = "-cl-std=CL1.2 -cl-mad-enable -cl-fast-relaxed-math".to_string();
                (ws, 128, opts, 85, false)
            }
            (GpuAlgorithm::CosmicHarmony, GpuDeviceFamily::Nvidia) => {
                let ws = (max_by_vram.min(8192).max(512)).next_power_of_two();
                let opts = "-cl-std=CL1.2 -cl-mad-enable -cl-fast-relaxed-math".to_string();
                (ws, 128, opts, 80, false)
            }
            (GpuAlgorithm::CosmicHarmony, GpuDeviceFamily::Other) => {
                let ws = (max_by_vram.min(4096).max(256)).next_power_of_two();
                let opts = "-cl-std=CL1.2 -cl-mad-enable".to_string();
                (ws, 128, opts, 70, false)
            }
        };

        Self {
            work_size,
            local_ws,
            build_opts,
            vram_pct,
            gcn_s4_mode,
            scratchpad_bytes,
        }
    }
}
