use anyhow::Result;
use clap::Subcommand;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::ui;

#[derive(Subcommand)]
pub enum MineCmd {
    /// Start mining (pool mode by default)
    Start {
        /// Pool address host:port
        #[arg(long)]
        pool: Option<String>,
        /// Wallet address
        #[arg(long)]
        wallet: Option<String>,
        /// Number of threads (default: auto)
        #[arg(long)]
        threads: Option<String>,
        /// Backend: auto | cpu | gpu | metal | opencl | cuda
        #[arg(long)]
        backend: Option<String>,
        /// Profile: pool | solo | benchmark | dual
        #[arg(long)]
        profile: Option<String>,
        /// Algorithm: deeksha_lite_v1 | cosmic_harmony_ekam_deeksha_v2 | deeksha_lite_fire
        #[arg(long)]
        algorithm: Option<String>,
    },
    /// Stop the mining process
    Stop,
    /// CPU Blake3 benchmark
    Bench {
        #[arg(long)]
        gpu: bool,
        /// Cosmic Harmony Ekam Deeksha benchmark
        #[arg(long)]
        ekam: bool,
        /// Backend: auto | gpu | metal | opencl | cuda
        #[arg(long)]
        backend: Option<String>,
        /// Override GPU work size for GPU or Ekam benchmarks
        #[arg(long)]
        work_size: Option<usize>,
        /// Benchmark duration in seconds
        #[arg(long, default_value = "5")]
        secs: u64,
    },
    /// Show live mining status
    Status,
    /// DCR stealth worker control
    Dcr {
        #[command(subcommand)]
        cmd: DcrCmd,
    },
}

#[derive(Subcommand)]
pub enum DcrCmd {
    /// Start DCR stealth worker
    Start,
    /// Stop DCR stealth worker
    Stop,
    /// Show DCR stats
    Status,
}

pub async fn run(cfg: &Config, cmd: MineCmd) -> Result<()> {
    match cmd {
        MineCmd::Start {
            pool,
            wallet,
            threads,
            backend,
            profile,
            algorithm,
        } => {
            let start =
                resolve_start_options(cfg, pool, wallet, threads, backend, profile, algorithm)?;

            ui::print_header("Starting Miner");
            ui::print_row("Pool", &start.pool_addr);
            ui::print_row(
                "Wallet",
                if start.wallet_addr.is_empty() {
                    "(not set)"
                } else {
                    &start.wallet_addr
                },
            );
            ui::print_row("Backend", &start.backend_display_name);
            ui::print_row(
                "Algorithm",
                start.algorithm.as_deref().unwrap_or("deeksha_lite_v1"),
            );
            ui::print_row("Threads", &start.thread_count);
            ui::print_row("Profile", &start.normalized_profile);
            println!();

            if start.wallet_addr.is_empty() {
                ui::print_warn("No wallet set. Run: zion config set miner.wallet <address>");
                ui::print_warn("Or: zion wallet new");
                return Ok(());
            }

            // Build env for miner binary
            let mut env_args = vec![
                ("ZION_POOL_ADDR", start.pool_addr.clone()),
                ("ZION_PROFILE", start.normalized_profile.clone()),
                ("ZION_MINER_ID", start.wallet_addr.clone()),
            ];
            if start.thread_count != "auto" {
                env_args.push(("ZION_THREADS", start.thread_count.clone()));
            }
            if let Some(env_backend) = &start.backend_env {
                env_args.push(("ZION_BACKEND", env_backend.clone()));
            }
            if let Some(env_algorithm) = &start.algorithm {
                env_args.push(("ZION_MINER_ALGORITHM", env_algorithm.clone()));
            }
            if start.normalized_profile == "dual" {
                if !cfg.miner.btc_wallet.trim().is_empty() {
                    env_args.push(("ZION_BTC_WALLET", cfg.miner.btc_wallet.clone()));
                    ui::print_info(
                        "Dual profile: using configured BTC payout wallet for DCR sidecar.",
                    );
                } else {
                    ui::print_warn("Dual profile selected but miner.btc_wallet is not set.");
                    ui::print_warn("Set it with: zion config set miner.btc_wallet <bc1...>");
                }
            }

            let miner_bin = find_miner_binary()?;
            ui::print_info(&format!("Running: {}", miner_bin));

            let mut cmd_proc = std::process::Command::new(&miner_bin);
            for (k, v) in &env_args {
                cmd_proc.env(k, v);
            }
            if let Some(cli_backend) = &start.backend_cli_gpu_arg {
                cmd_proc.args(["--gpu", cli_backend]);
            }

            let interactive = std::env::var("ZION_INTERACTIVE")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true);
            if interactive {
                cmd_proc.status()?;
            } else {
                let child = cmd_proc.spawn()?;
                ui::print_info(&format!("Miner started in background (PID {})", child.id()));
            }
            Ok(())
        }

        MineCmd::Bench {
            gpu,
            ekam,
            backend,
            work_size,
            secs,
        } => {
            ui::print_header("Benchmark");
            let benchmark_mode = determine_benchmark_mode(gpu, ekam)?;
            let allow_cpu_backend = matches!(benchmark_mode, BenchmarkMode::CpuBlake3);
            let normalized_backend = normalize_backend(
                backend.as_deref().unwrap_or(
                    if matches!(benchmark_mode, BenchmarkMode::CpuBlake3) {
                        "cpu"
                    } else {
                        "auto"
                    },
                ),
                allow_cpu_backend,
            )?;

            if matches!(benchmark_mode, BenchmarkMode::CpuBlake3)
                && normalized_backend.mode != BackendMode::Cpu
            {
                anyhow::bail!("CPU benchmark does not accept GPU backends. Use --gpu or --ekam for GPU benchmark modes.");
            }

            let miner_bin = find_miner_binary()?;

            let mut cmd_proc = std::process::Command::new(&miner_bin);
            cmd_proc.env("ZION_BENCH_SECS", secs.to_string());
            if let Some(work_size) = work_size {
                cmd_proc.env("ZION_GPU_WORK_SIZE", work_size.to_string());
            }
            if let Some(env_backend) = normalized_backend.env_backend {
                cmd_proc.env("ZION_BACKEND", env_backend);
            }

            match benchmark_mode {
                BenchmarkMode::EkamDeeksha => {
                    ui::print_info("Mode: Cosmic Harmony Ekam Deeksha v2");
                    ui::print_row("Backend", normalized_backend.display_name);
                    if let Some(work_size) = work_size {
                        ui::print_row("Work Size", &work_size.to_string());
                    }
                    cmd_proc.arg("--ekam-bench");
                }
                BenchmarkMode::GpuBlake3 => {
                    ui::print_info("Mode: GPU Blake3");
                    ui::print_row("Backend", normalized_backend.display_name);
                    if let Some(work_size) = work_size {
                        ui::print_row("Work Size", &work_size.to_string());
                    }
                    cmd_proc.arg("--gpu-bench");
                }
                BenchmarkMode::CpuBlake3 => {
                    ui::print_info("Mode: CPU Blake3");
                    cmd_proc.arg("--bench");
                }
            }

            cmd_proc.status()?;
            Ok(())
        }

        MineCmd::Stop => {
            // Best-effort: kill any zion-miner process
            let _ = std::process::Command::new("pkill")
                .arg("-f")
                .arg("zion-miner")
                .status();
            ui::print_ok("Sent stop signal to miner processes");
            Ok(())
        }

        MineCmd::Status => {
            ui::print_header("Miner Status");
            let running = is_miner_running();
            if running {
                ui::print_ok("Miner is running");
            } else {
                ui::print_warn("No miner process detected");
            }
            println!();
            Ok(())
        }

        MineCmd::Dcr { cmd } => match cmd {
            DcrCmd::Status => {
                ui::print_header("DCR Stealth Worker");
                ui::print_info("DCR worker runs inside the miner process.");
                ui::print_info("Start with: zion mine start --profile dual");
                println!();
                Ok(())
            }
            DcrCmd::Start => {
                ui::print_info("Starting DCR-only stealth miner...");
                let miner_bin = find_miner_binary()?;
                std::process::Command::new(&miner_bin)
                    .env("ZION_DCR_ONLY", "1")
                    .status()?;
                Ok(())
            }
            DcrCmd::Stop => {
                let _ = std::process::Command::new("pkill")
                    .arg("-f")
                    .arg("zion-miner")
                    .status();
                ui::print_ok("Sent stop signal");
                Ok(())
            }
        },
    }
}

fn find_miner_binary() -> Result<String> {
    // 1. Try PATH
    if let Ok(p) = which_bin("zion-miner") {
        return Ok(p);
    }

    if let Some(candidate) = discover_miner_binary() {
        return Ok(candidate.display().to_string());
    }

    anyhow::bail!(
        "zion-miner binary not found. Build with:\n  cd V3 && cargo build -p zion-miner --release"
    )
}

pub(crate) fn discover_miner_binary() -> Option<PathBuf> {
    miner_binary_candidates()
        .into_iter()
        .find(|candidate| candidate.exists())
}

fn miner_binary_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // Same-directory as the running zion CLI (for bundled desktop-agent installs)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            push_unique_path(&mut candidates, dir.join(bin_name("zion-miner")));
        }
    }

    for relative in [
        PathBuf::from(format!("target/release/{}", bin_name("zion-miner"))),
        PathBuf::from(format!("target/debug/{}", bin_name("zion-miner"))),
        PathBuf::from(format!("V3/target/release/{}", bin_name("zion-miner"))),
        PathBuf::from(format!("V3/target/debug/{}", bin_name("zion-miner"))),
    ] {
        push_unique_path(&mut candidates, relative);
    }

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Some(workspace_root) = manifest_dir.parent() {
        push_unique_path(
            &mut candidates,
            workspace_root.join(format!("target/release/{}", bin_name("zion-miner"))),
        );
        push_unique_path(
            &mut candidates,
            workspace_root.join(format!("target/debug/{}", bin_name("zion-miner"))),
        );
    }

    candidates
}

fn bin_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{}.exe", base)
    } else {
        base.to_string()
    }
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|existing| existing == &candidate) {
        paths.push(candidate);
    }
}

fn which_bin(name: &str) -> Result<String> {
    let (cmd, arg) = if cfg!(windows) {
        ("where", name)
    } else {
        ("which", name)
    };
    let out = std::process::Command::new(cmd).arg(arg).output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        anyhow::bail!("not found")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BenchmarkMode {
    CpuBlake3,
    GpuBlake3,
    EkamDeeksha,
}

fn determine_benchmark_mode(gpu: bool, ekam: bool) -> Result<BenchmarkMode> {
    match (gpu, ekam) {
        (true, true) => anyhow::bail!("Choose either --gpu or --ekam, not both."),
        (true, false) => Ok(BenchmarkMode::GpuBlake3),
        (false, true) => Ok(BenchmarkMode::EkamDeeksha),
        (false, false) => Ok(BenchmarkMode::CpuBlake3),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendMode {
    Auto,
    Cpu,
    GpuAlias,
    Metal,
    OpenCl,
    Cuda,
}

struct NormalizedBackend<'a> {
    mode: BackendMode,
    display_name: &'a str,
    env_backend: Option<&'a str>,
    cli_gpu_arg: Option<&'a str>,
}

#[derive(Debug)]
struct ResolvedStartOptions {
    pool_addr: String,
    wallet_addr: String,
    thread_count: String,
    backend_display_name: String,
    backend_env: Option<String>,
    backend_cli_gpu_arg: Option<String>,
    normalized_profile: String,
    algorithm: Option<String>,
}

fn resolve_start_options(
    cfg: &Config,
    pool: Option<String>,
    wallet: Option<String>,
    threads: Option<String>,
    backend: Option<String>,
    profile: Option<String>,
    algorithm: Option<String>,
) -> Result<ResolvedStartOptions> {
    let requested_profile = profile.as_deref().unwrap_or(cfg.miner.profile.as_str());
    let normalized_profile = normalize_profile(requested_profile)?;
    let pool_addr = pool.unwrap_or_else(|| {
        let (host, port) = cfg.edge_pool();
        format!("{}:{}", host, port)
    });
    let wallet_addr = wallet.unwrap_or_else(|| cfg.miner.wallet.clone());
    if !wallet_addr.trim().is_empty() && !zion_core::crypto::is_valid_address(wallet_addr.trim()) {
        anyhow::bail!(
            "Invalid mining wallet '{}'. Expected a valid zion1... address.",
            wallet_addr.trim()
        );
    }
    let thread_count = threads.unwrap_or_else(|| cfg.miner.threads.clone());
    let requested_backend = backend.unwrap_or_else(|| cfg.miner.backend.clone());
    let normalized_backend = normalize_backend(&requested_backend, true)?;

    let algorithm = algorithm.or_else(|| {
        let cfg_algo = cfg.miner.algorithm.clone();
        if cfg_algo.is_empty() {
            None
        } else {
            Some(cfg_algo)
        }
    });

    Ok(ResolvedStartOptions {
        pool_addr,
        wallet_addr,
        thread_count,
        backend_display_name: normalized_backend.display_name.to_string(),
        backend_env: normalized_backend.env_backend.map(ToString::to_string),
        backend_cli_gpu_arg: normalized_backend.cli_gpu_arg.map(ToString::to_string),
        normalized_profile: normalized_profile.to_string(),
        algorithm,
    })
}

fn normalize_backend<'a>(backend: &'a str, allow_cpu: bool) -> Result<NormalizedBackend<'a>> {
    match backend.trim().to_ascii_lowercase().as_str() {
        "auto" => Ok(NormalizedBackend {
            mode: BackendMode::Auto,
            display_name: "auto",
            env_backend: Some("auto"),
            cli_gpu_arg: None,
        }),
        "cpu" if allow_cpu => Ok(NormalizedBackend {
            mode: BackendMode::Cpu,
            display_name: "cpu",
            env_backend: Some("cpu"),
            cli_gpu_arg: None,
        }),
        "gpu" => Ok(NormalizedBackend {
            mode: BackendMode::GpuAlias,
            display_name: "gpu (auto)",
            env_backend: Some("auto"),
            cli_gpu_arg: Some("auto"),
        }),
        "metal" => Ok(NormalizedBackend {
            mode: BackendMode::Metal,
            display_name: "metal",
            env_backend: Some("metal"),
            cli_gpu_arg: Some("metal"),
        }),
        "opencl" | "ocl" => Ok(NormalizedBackend {
            mode: BackendMode::OpenCl,
            display_name: "opencl",
            env_backend: Some("opencl"),
            cli_gpu_arg: Some("opencl"),
        }),
        "cuda" => Ok(NormalizedBackend {
            mode: BackendMode::Cuda,
            display_name: "cuda",
            env_backend: Some("cuda"),
            cli_gpu_arg: Some("cuda"),
        }),
        "cpu" => anyhow::bail!("This command does not accept backend 'cpu' in GPU mode."),
        other => anyhow::bail!(
            "Unsupported backend '{}'. Supported backends: auto, cpu, gpu, metal, opencl, cuda",
            other
        ),
    }
}

fn normalize_profile(profile: &str) -> Result<&str> {
    match profile.trim().to_ascii_lowercase().as_str() {
        "pool" => Ok("pool"),
        "solo" => Ok("solo"),
        "benchmark" | "bench" => Ok("benchmark"),
        "dual" => Ok("dual"),
        other => anyhow::bail!(
            "Unsupported miner profile '{}'. Supported profiles: pool, solo, benchmark, dual",
            other
        ),
    }
}

/// Cross-platform miner process detection.
/// On Unix: uses `pgrep -f zion-miner`.
/// On Windows: uses `tasklist /FI "IMAGENAME eq zion-miner.exe"`.
fn is_miner_running() -> bool {
    #[cfg(windows)]
    {
        let out = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq zion-miner.exe", "/NH"])
            .output();
        if let Ok(o) = out {
            let stdout = String::from_utf8_lossy(&o.stdout);
            return stdout.contains("zion-miner.exe");
        }
        false
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("pgrep")
            .args(["-f", "zion-miner"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, MinerConfig, PoolConfig};

    #[test]
    fn bench_mode_rejects_conflicting_flags() {
        assert!(determine_benchmark_mode(true, true).is_err());
    }

    #[test]
    fn backend_normalization_supports_opencl_and_cuda() {
        let opencl = normalize_backend("opencl", true).expect("opencl backend");
        assert_eq!(opencl.env_backend, Some("opencl"));
        assert_eq!(opencl.cli_gpu_arg, Some("opencl"));

        let cuda = normalize_backend("cuda", true).expect("cuda backend");
        assert_eq!(cuda.env_backend, Some("cuda"));
        assert_eq!(cuda.cli_gpu_arg, Some("cuda"));
    }

    #[test]
    fn profile_normalization_supports_bench_alias() {
        assert_eq!(
            normalize_profile("bench").expect("bench alias"),
            "benchmark"
        );
    }

    #[test]
    fn cpu_benchmark_accepts_default_cpu_backend() {
        let benchmark_mode = determine_benchmark_mode(false, false).expect("cpu benchmark mode");
        let allow_cpu_backend = matches!(benchmark_mode, BenchmarkMode::CpuBlake3);
        let normalized =
            normalize_backend("cpu", allow_cpu_backend).expect("cpu backend should be accepted");

        assert_eq!(normalized.mode, BackendMode::Cpu);
    }

    #[test]
    fn miner_binary_candidates_include_workspace_target() {
        let candidates = miner_binary_candidates();
        // Check that paths end with "zion-miner" (cross-platform path separator handling)
        let has_release = candidates.iter().any(|path| {
            path.file_name()
                .map(|n| n == "zion-miner.exe" || n == "zion-miner")
                .unwrap_or(false)
        });
        assert!(
            has_release,
            "miner_binary_candidates should include zion-miner binary path"
        );
    }

    #[test]
    fn start_options_use_config_profile_when_flag_missing() {
        let cfg = Config {
            pool: PoolConfig {
                host: "127.0.0.1".into(),
                port: 3333,
                metrics_port: 8455,
            },
            miner: MinerConfig {
                wallet: "zion16853d8r885l4g4u8p8t7v5n8u6v7e0f445dr3f8".into(),
                btc_wallet: String::new(),
                threads: "auto".into(),
                backend: "auto".into(),
                profile: "dual".into(),
                algorithm: "deeksha_lite_v1".into(),
            },
            ..Config::default()
        };

        let resolved = resolve_start_options(&cfg, None, None, None, None, None, None)
            .expect("start options should resolve from config");

        assert_eq!(resolved.normalized_profile, "dual");
    }

    #[test]
    fn start_options_reject_invalid_wallet_address() {
        let cfg = Config {
            miner: MinerConfig {
                wallet: "not-a-zion-address".into(),
                ..MinerConfig::default()
            },
            ..Config::default()
        };

        let error = resolve_start_options(&cfg, None, None, None, None, None, None)
            .expect_err("invalid wallet should fail preflight");

        assert!(error.to_string().contains("Invalid mining wallet"));
    }
}
