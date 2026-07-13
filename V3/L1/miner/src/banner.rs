//! Startup banner with hardware detection and version info.

use crate::ui;

/// Print the startup banner with version, consensus, and hardware info.
pub fn print_banner(threads: usize) {
    let backend = crate::gpu_backend::GpuBackendKind::from_env();
    ui::print_fancy_banner(threads, "3.0.5", backend.as_str());

    // ── System info table ──
    let mut rows: Vec<(String, String)> = Vec::new();
    rows.push(("version".to_string(), "3.0.5".to_string()));
    rows.push((
        "consensus".to_string(),
        zion_core::consensus_profile().to_string(),
    ));
    rows.push((
        "protocol".to_string(),
        zion_pool::protocol_version().to_string(),
    ));

    let logical_cpus = num_cpus::get();
    let physical_cpus = num_cpus::get_physical();
    rows.push((
        "cpu".to_string(),
        format!("{} cores / {} threads", physical_cpus, logical_cpus),
    ));

    let mut simd_caps = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx512f") {
            simd_caps.push("AVX-512");
        }
        if is_x86_feature_detected!("avx2") {
            simd_caps.push("AVX2");
        }
        if is_x86_feature_detected!("sse4.1") {
            simd_caps.push("SSE4.1");
        }
        if is_x86_feature_detected!("aes") {
            simd_caps.push("AES-NI");
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        simd_caps.push("NEON");
        #[cfg(target_feature = "aes")]
        simd_caps.push("AES");
    }
    rows.push((
        "simd".to_string(),
        if simd_caps.is_empty() {
            "none".to_string()
        } else {
            simd_caps.join(", ")
        },
    ));

    // GPU detection with rich details
    #[cfg(any(feature = "gpu-opencl", feature = "gpu-cuda", feature = "gpu-metal"))]
    {
        let gpus = crate::gpu_backend::query_gpu_details();
        if gpus.is_empty() {
            rows.push(("gpu".to_string(), "none".to_string()));
        } else {
            for (i, gpu) in gpus.iter().enumerate() {
                let vram_gb = gpu.global_mem_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
                rows.push((
                    format!("gpu[{}]", i),
                    format!(
                        "{} | {} CUs | {} MHz | {:.1} GiB VRAM",
                        gpu.name, gpu.compute_units, gpu.max_clock_mhz, vram_gb
                    ),
                ));
            }
        }
    }
    #[cfg(not(any(feature = "gpu-opencl", feature = "gpu-cuda", feature = "gpu-metal")))]
    {
        rows.push((
            "gpu".to_string(),
            "disabled (compile with --features gpu-opencl)".to_string(),
        ));
    }

    rows.push(("backend".to_string(), backend.as_str().to_string()));
    ui::print_kv_table(&rows);
    println!();
}
