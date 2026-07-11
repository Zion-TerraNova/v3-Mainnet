//! Local miner control — start, stop, status.
//!
//! Manages the `zion-miner` process via PID file `~/.zion/miner.pid`. The
//! miner can connect to the public Edge pool (default) or to a local pool.
//!
//! Autonomous mode: when `miner.auto_start_node` is true in config, the CLI
//! will start the local node first if it's not running, then start the miner.

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use std::path::PathBuf;
use std::time::Duration;

use crate::config::{self, Config};
use crate::process;
use crate::ui;
use crate::commands::node;
use crate::commands::pool;

#[derive(Subcommand)]
pub enum MineCmd {
    /// Start mining (connects to the configured pool)
    Start {
        /// Override pool address (host:port)
        #[arg(long)]
        pool: Option<String>,
        /// Override wallet address
        #[arg(long)]
        wallet: Option<String>,
        /// Override algorithm: deeksha_lite_v1 | cosmic_harmony_ekam_deeksha_v2 | deeksha_lite_fire
        #[arg(long)]
        algorithm: Option<String>,
        /// Override backend: cpu | opencl | cuda | metal
        #[arg(long)]
        backend: Option<String>,
        /// Override worker name
        #[arg(long)]
        worker: Option<String>,
        /// Start a local node first if not running
        #[arg(long)]
        auto_node: bool,
        /// Start a local pool first if not running
        #[arg(long)]
        auto_pool: bool,
        /// Run miner in a visible console window
        #[arg(long)]
        console: bool,
        /// Follow miner log output in real-time (Ctrl+C to stop following, miner keeps running)
        #[arg(long)]
        follow: bool,
    },
    /// Stop the running miner
    Stop,
    /// Show miner process status + live stats (hashrate, shares, uptime)
    Status,
    /// Live monitoring dashboard — refreshes every 2 seconds (Ctrl+C to exit, miner keeps running)
    Monitor,
    /// Show recent miner log output
    Log {
        /// Number of lines to show (default: 50)
        #[arg(long, short = 'n')]
        lines: Option<usize>,
        /// Follow log output in real-time
        #[arg(long, short = 'f')]
        follow: bool,
    },
}

pub async fn run(cfg: &Config, cmd: MineCmd) -> Result<()> {
    match cmd {
        MineCmd::Start {
            pool, wallet, algorithm, backend, worker, auto_node, auto_pool, console, follow,
        } => start_mining(cfg, pool, wallet, algorithm, backend, worker, auto_node, auto_pool, console, follow).await,
        MineCmd::Stop => stop_mining(),
        MineCmd::Status => miner_status(),
        MineCmd::Monitor => miner_monitor().await,
        MineCmd::Log { lines, follow } => miner_log(lines, follow),
    }
}

async fn start_mining(
    cfg: &Config,
    pool_override: Option<String>,
    wallet_override: Option<String>,
    algo_override: Option<String>,
    backend_override: Option<String>,
    worker_override: Option<String>,
    auto_node: bool,
    auto_pool: bool,
    console: bool,
    follow: bool,
) -> Result<()> {
    ui::print_header("Start Mining");

    let wallet = wallet_override.unwrap_or_else(|| cfg.miner.wallet.clone());
    if wallet.is_empty() {
        ui::print_err("No wallet address configured.");
        ui::print_info("Run: zion wallet new --mnemonic --set-default");
        ui::print_info("Or:  zion config set miner.wallet <your_address>");
        return Ok(());
    }

    // ── Autonomous dependency startup ─────────────────────────────────────────
    if auto_node || cfg.miner.auto_start_node {
        if process::status("node").is_none() {
            ui::print_info("Local node not running — starting it automatically...");
            node::run(cfg, node::NodeCmd::Start {
                p2p_bind: None,
                rpc_bind: None,
                seed_peers: None,
                state_path: None,
                console: false,
            }).await?;
            // Give the node a few seconds to open RPC.
            tokio::time::sleep(Duration::from_secs(3)).await;
        } else {
            ui::print_info("Local node is already running.");
        }
    }

    if auto_pool || cfg.miner.auto_start_pool {
        if process::status("pool").is_none() {
            ui::print_info("Local pool not running — starting it automatically...");
            pool::run(cfg, pool::PoolCmd::Start {
                bind: None,
                node_rpc: None,
                wallet: None,
                console: false,
            }).await?;
            tokio::time::sleep(Duration::from_secs(2)).await;
        } else {
            ui::print_info("Local pool is already running.");
        }
    }

    // ── Resolve pool address ────────────────────────────────────────────────
    let pool_addr = pool_override.unwrap_or_else(|| {
        if cfg.miner.auto_start_pool || auto_pool {
            cfg.pool.bind.clone()
        } else {
            format!("{}:{}", cfg.pool.host, cfg.pool.port)
        }
    });
    let algorithm = algo_override.unwrap_or_else(|| cfg.miner.algorithm.clone());
    let backend = backend_override.unwrap_or_else(|| cfg.miner.backend.clone());
    let worker = worker_override.unwrap_or_else(|| cfg.miner.worker_name.clone());

    // Check if already running
    if let Some(pid) = process::status("miner") {
        ui::print_warn(&format!("Miner already running (PID {}). Run 'zion mine stop' first.", pid));
        return Ok(());
    }

    // Find the miner binary
    let bin = find_miner_binary()?;

    ui::print_row("Miner binary", &bin.display().to_string());
    ui::print_row("Pool", &pool_addr);
    ui::print_row("Wallet", &wallet);
    ui::print_row("Worker", &worker);
    ui::print_row("Algorithm", &algorithm);
    ui::print_row("Backend", &backend);
    println!();

    let mut envs: Vec<(&str, String)> = Vec::new();
    envs.push(("ZION_POOL_ADDR", pool_addr));
    envs.push(("ZION_WORKER_NAME", worker));
    envs.push(("ZION_MINER_ALGORITHM", algorithm));
    envs.push(("ZION_PAYOUT_ADDRESS", wallet));
    envs.push(("ZION_LOOP_COUNT", "1000000".into()));
    // Stats file — miner writes JSON stats here for live monitoring
    if let Some(home) = std::env::var("HOME").ok().or_else(|| std::env::var("USERPROFILE").ok()) {
        let stats_path = format!("{}/.zion/miner-stats.json", home);
        envs.push(("ZION_STATS_FILE", stats_path));
    }
    // Write stats every 5 seconds (default 30 is too slow for live monitoring)
    envs.push(("ZION_METRICS_REPORT_SECS", "5".into()));

    match backend.as_str() {
        "opencl" => {
            envs.push(("ZION_GPU_BACKEND", "opencl".into()));
            envs.push(("ZION_NONCE_COUNT_GPU", "262144".into()));
        }
        "cuda" => {
            envs.push(("ZION_GPU_BACKEND", "cuda".into()));
            envs.push(("ZION_NONCE_COUNT_GPU", "262144".into()));
        }
        "metal" => {
            envs.push(("ZION_GPU_BACKEND", "metal".into()));
            envs.push(("ZION_NONCE_COUNT_GPU", "262144".into()));
        }
        _ => {
            envs.push(("ZION_NONCE_COUNT", "4096".into()));
        }
    }

    let pid = process::start("miner", &bin, &[], &envs, console)?;

    ui::print_ok(&format!("Miner started (PID {})", pid));
    ui::print_info("Stop with: zion mine stop");
    ui::print_info("Check status: zion mine status");

    // Show log file path
    if let Some(log_path) = process::get_log_path("miner") {
        ui::print_info(&format!("Log file: {}", log_path.display()));
        ui::print_info("View logs: zion mine log");
    }

    println!();

    // If --follow, tail the log file. Ctrl+C stops following but miner keeps running.
    if follow {
        follow_log("miner")?;
    }

    Ok(())
}

fn stop_mining() -> Result<()> {
    ui::print_header("Stop Mining");
    match process::stop("miner")? {
        true => ui::print_ok("Miner stopped."),
        false => ui::print_warn("Miner was not running."),
    }
    println!();
    Ok(())
}

fn miner_status() -> Result<()> {
    ui::print_header("Miner Status");
    match process::status("miner") {
        Some(pid) => {
            ui::print_ok(&format!("Miner is running (PID {})", pid));
            println!();
            // Show live stats from miner-stats.json
            if let Some(stats) = read_miner_live_stats() {
                print_miner_stats(&stats);
            } else {
                ui::print_info("Stats not available yet (miner may have just started).");
                ui::print_info("Run 'zion mine monitor' for live dashboard.");
            }
        }
        None => {
            ui::print_warn("Miner is not running.");
            ui::print_info("Start with: zion mine start");
        }
    }
    println!();
    Ok(())
}

/// Live stats written by the miner to ~/.zion/miner-stats.json
#[derive(Debug, Default, serde::Deserialize)]
struct MinerLiveStats {
    #[serde(default)]
    hashrate: f64,
    #[serde(default)]
    hashrate_10s: f64,
    #[serde(default)]
    hashrate_60s: f64,
    #[serde(default)]
    hashrate_15m: f64,
    #[serde(default)]
    hashrate_max: f64,
    #[serde(default)]
    hashrate_gpu: f64,
    #[serde(default)]
    hashrate_cpu: f64,
    #[serde(default)]
    shares_accepted: u64,
    #[serde(default)]
    shares_rejected: u64,
    #[serde(default)]
    shares_sent: u64,
    #[serde(default)]
    pool_height: u64,
    #[serde(default)]
    current_epoch: u64,
    #[serde(default)]
    total_hashes: u64,
    #[serde(default)]
    pool_latency_ms: u64,
    #[serde(default)]
    backend: String,
    #[serde(default)]
    gpu_name: String,
    #[serde(default)]
    worker: String,
    #[serde(default)]
    algorithm: String,
    #[serde(default)]
    cpu_threads: usize,
    #[serde(default)]
    uptime_sec: u64,
    #[serde(default)]
    status: String,
    #[serde(default)]
    miner_id: String,
    #[serde(default)]
    pool_addr: String,
}

fn read_miner_live_stats() -> Option<MinerLiveStats> {
    let path = stats_file_path()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn stats_file_path() -> Option<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    Some(PathBuf::from(home).join(".zion").join("miner-stats.json"))
}

fn format_hashrate(hps: f64) -> String {
    if hps >= 1e9 {
        format!("{:.2} GH/s", hps / 1e9)
    } else if hps >= 1e6 {
        format!("{:.2} MH/s", hps / 1e6)
    } else if hps >= 1e3 {
        format!("{:.2} KH/s", hps / 1e3)
    } else {
        format!("{:.1} H/s", hps)
    }
}

fn format_uptime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{}h {}m {}s", h, m, s)
    } else if m > 0 {
        format!("{}m {}s", m, s)
    } else {
        format!("{}s", s)
    }
}

fn print_miner_stats(stats: &MinerLiveStats) {
    ui::print_section("Mining Stats");
    ui::print_row("Status", &stats.status);
    ui::print_row("Uptime", &format_uptime(stats.uptime_sec));
    ui::print_row("Algorithm", &stats.algorithm);
    ui::print_row("Backend", &stats.backend);
    ui::print_row("Worker", &stats.worker);
    ui::print_row("CPU threads", &stats.cpu_threads.to_string());
    if !stats.gpu_name.is_empty() && stats.gpu_name != "none" {
        ui::print_row("GPU", &stats.gpu_name);
    }
    println!();

    ui::print_section("Hashrate");
    ui::print_row("Current (10s)", &format_hashrate(stats.hashrate_10s));
    ui::print_row("Average (60s)", &format_hashrate(stats.hashrate_60s));
    ui::print_row("Long-term (15m)", &format_hashrate(stats.hashrate_15m));
    ui::print_row("Peak", &format_hashrate(stats.hashrate_max));
    if stats.hashrate_gpu > 0.0 {
        ui::print_row("GPU", &format_hashrate(stats.hashrate_gpu));
        ui::print_row("CPU", &format_hashrate(stats.hashrate_cpu));
    }
    println!();

    ui::print_section("Shares");
    ui::print_row("Accepted", &stats.shares_accepted.to_string());
    ui::print_row("Rejected", &stats.shares_rejected.to_string());
    let total = stats.shares_accepted + stats.shares_rejected;
    let pct = if total > 0 {
        stats.shares_accepted as f64 * 100.0 / total as f64
    } else {
        0.0
    };
    ui::print_row("Accept rate", &format!("{:.1}%", pct));
    println!();

    ui::print_section("Pool");
    ui::print_row("Pool", &stats.pool_addr);
    ui::print_row("Pool height", &stats.pool_height.to_string());
    ui::print_row("Latency", &format!("{} ms", stats.pool_latency_ms));
    ui::print_row("Total hashes", &stats.total_hashes.to_string());
}

/// Live monitoring dashboard — refreshes every 2 seconds.
/// Ctrl+C exits the dashboard but the miner keeps running.
async fn miner_monitor() -> Result<()> {
    // Check if miner is running
    if process::status("miner").is_none() {
        ui::print_header("Miner Monitor");
        ui::print_err("Miner is not running. Start it first: zion mine start");
        return Ok(());
    }

    let pid = process::status("miner").unwrap();
    ui::print_header(&format!("Miner Live Monitor (PID {}) — Ctrl+C to exit", pid));
    println!();

    // Set up Ctrl+C handler
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        r.store(false, std::sync::atomic::Ordering::Relaxed);
    });

    let mut tick = 0u64;
    while running.load(std::sync::atomic::Ordering::Relaxed) {
        // Clear screen and move cursor to top
        print!("\x1b[2J\x1b[H");

        // Header
        println!("  {} — Live Monitor (PID {}) {}", "ZION Miner".cyan().bold(), pid, "─".dimmed());
        println!();

        // Check if still running
        if process::status("miner").is_none() {
            println!("  {} Miner process has stopped.", "✗".red().bold());
            println!();
            println!("  {} Press Ctrl+C to exit...", "◉".dimmed());
            tokio::time::sleep(Duration::from_secs(2)).await;
            break;
        }

        // Read live stats
        match read_miner_live_stats() {
            Some(stats) => {
                // Status line
                let status_color = if stats.status == "running" {
                    "RUNNING".green().bold()
                } else {
                    stats.status.yellow().bold()
                };
                println!("  Status: {}  |  Uptime: {}  |  Tick: {}", status_color, format_uptime(stats.uptime_sec), tick);
                println!("  {}", "─".repeat(70).dimmed());

                // Hashrate
                println!("  {} Hashrate", "▸".cyan());
                println!("    Current (10s): {:>14}  |  Avg (60s): {:>14}  |  Peak: {:>14}",
                    format_hashrate(stats.hashrate_10s),
                    format_hashrate(stats.hashrate_60s),
                    format_hashrate(stats.hashrate_max));
                if stats.hashrate_gpu > 0.0 {
                    println!("    GPU:            {:>14}  |  CPU:       {:>14}",
                        format_hashrate(stats.hashrate_gpu),
                        format_hashrate(stats.hashrate_cpu));
                }
                println!();

                // Shares
                let total = stats.shares_accepted + stats.shares_rejected;
                let pct = if total > 0 {
                    stats.shares_accepted as f64 * 100.0 / total as f64
                } else { 0.0 };
                let acc_str = stats.shares_accepted.to_string().green();
                let rej_str = if stats.shares_rejected > 0 {
                    stats.shares_rejected.to_string().red()
                } else {
                    stats.shares_rejected.to_string().dimmed()
                };
                println!("  {} Shares", "▸".cyan());
                println!("    Accepted: {}  |  Rejected: {}  |  Accept rate: {:.1}%  |  Total hashes: {}",
                    acc_str, rej_str, pct, stats.total_hashes);
                println!();

                // Pool / mining info
                println!("  {} Pool & Mining", "▸".cyan());
                println!("    Pool: {}  |  Height: {}  |  Latency: {} ms",
                    stats.pool_addr, stats.pool_height, stats.pool_latency_ms);
                println!("    Algorithm: {}  |  Backend: {}  |  Worker: {}  |  Threads: {}",
                    stats.algorithm, stats.backend, stats.worker, stats.cpu_threads);
                println!();

                // Last few log lines
                if let Some(log_path) = process::get_log_path("miner") {
                    if log_path.exists() {
                        println!("  {} Recent Log", "▸".cyan());
                        if let Ok(content) = std::fs::read_to_string(&log_path) {
                            let lines: Vec<&str> = content.lines().collect();
                            let start = lines.len().saturating_sub(5);
                            for line in &lines[start..] {
                                // Strip ANSI codes for cleaner display
                                let clean = strip_ansi(line);
                                if !clean.is_empty() {
                                    println!("    {}", clean.chars().take(80).collect::<String>());
                                }
                            }
                        }
                    }
                }

                println!();
                println!("  {} Refreshing every 2s  |  {} Ctrl+C to exit (miner keeps running)", "◉".dimmed(), "⚠".yellow());
            }
            None => {
                println!("  {} Waiting for miner stats...", "◉".yellow());
                println!("  The miner writes stats every 3 seconds. If this persists,");
                println!("  check the log: zion mine log");
                println!();
                println!("  {} Ctrl+C to exit", "◉".dimmed());
            }
        }

        tick += 1;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    println!();
    ui::print_ok("Monitor stopped. Miner is still running in the background.");
    ui::print_info("Check status: zion mine status  |  Stop: zion mine stop");
    println!();
    Ok(())
}

/// Strip ANSI escape codes from a string for clean display.
fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip escape sequence: ESC [ ... letter
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(c) = chars.next() {
                    if c.is_ascii_alphabetic() { break; }
                }
            } else {
                // Skip other escape sequences
                while let Some(&c) = chars.peek() {
                    if c.is_alphabetic() || c == 'm' { chars.next(); break; }
                    chars.next();
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn find_miner_binary() -> Result<PathBuf> {
    if let Some(path) = config::load(None).ok().and_then(|c| c.binaries.miner) {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    for c in ["zion-miner-windows-x86_64", "zion-miner"] {
        if let Some(p) = process::find_binary(c) {
            return Ok(p);
        }
    }

    // Self-contained bundle.
    if let Ok(p) = crate::bundle::ensure_binary("miner") {
        if p.exists() {
            return Ok(p);
        }
    }

    // Bare `miner` is generic; search only safe locations.
    if let Some(p) = process::find_binary_safely("miner") {
        return Ok(p);
    }

    Err(anyhow::anyhow!(
        "miner binary not found. Download from https://github.com/Zion-TerraNova/v3-Mainnet/releases or build: cargo build --release -p zion-miner"
    ))
}

/// Show recent miner log output.
fn miner_log(lines: Option<usize>, follow: bool) -> Result<()> {
    ui::print_header("Miner Log");

    let log_path = process::get_log_path("miner")
        .ok_or_else(|| anyhow::anyhow!("cannot determine log path"))?;

    if !log_path.exists() {
        ui::print_warn(&format!("Log file not found: {}", log_path.display()));
        ui::print_info("The miner may not have been started yet.");
        return Ok(());
    }

    ui::print_row("Log file", &log_path.display().to_string());
    println!();

    if follow {
        follow_log("miner")?;
    } else {
        let n = lines.unwrap_or(50);
        show_log_tail(&log_path, n)?;
    }

    Ok(())
}

/// Show the last N lines of a log file.
fn show_log_tail(path: &std::path::Path, n: usize) -> Result<()> {
    let output = std::process::Command::new("tail")
        .args(["-n", &n.to_string()])
        .arg(path)
        .output();

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if stdout.is_empty() {
                ui::print_info("(log is empty)");
            } else {
                print!("{}", stdout);
            }
        }
        Err(_) => {
            // Fallback: read the whole file and show last N lines
            let content = std::fs::read_to_string(path)?;
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(n);
            for line in &lines[start..] {
                println!("{}", line);
            }
        }
    }
    println!();
    Ok(())
}

/// Follow a log file in real-time. Ctrl+C stops following but the process keeps running.
fn follow_log(name: &str) -> Result<()> {
    let log_path = process::get_log_path(name)
        .ok_or_else(|| anyhow::anyhow!("cannot determine log path"))?;

    if !log_path.exists() {
        ui::print_warn(&format!("Log file not found: {}", log_path.display()));
        return Ok(());
    }

    ui::print_info(&format!("Following {} log (Ctrl+C to stop, {} keeps running)...", name, name));
    println!();

    // Use `tail -f` on Unix; on Windows, we'd need a different approach.
    #[cfg(unix)]
    {
        let status = std::process::Command::new("tail")
            .args(["-n", "20", "-f"])
            .arg(&log_path)
            .status();

        match status {
            Ok(_) => {}
            Err(e) => ui::print_warn(&format!("Could not follow log: {}", e)),
        }
    }
    #[cfg(windows)]
    {
        // On Windows, just show the last 50 lines (no real-time tail).
        ui::print_info("(Real-time log follow is not available on Windows. Showing last 50 lines.)");
        show_log_tail(&log_path, 50)?;
    }

    println!();
    ui::print_ok(&format!("Stopped following. {} is still running in the background.", name));
    Ok(())
}
