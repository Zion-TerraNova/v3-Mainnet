//! Local miner control — start, stop, status.
//!
//! Manages the `zion-miner` process via PID file `~/.zion/miner.pid`. The
//! miner can connect to the public Edge pool (default) or to a local pool.
//!
//! Autonomous mode: when `miner.auto_start_node` is true in config, the CLI
//! will start the local node first if it's not running, then start the miner.

use anyhow::Result;
use clap::Subcommand;
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
    /// Show miner process status
    Status,
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
        Some(pid) => ui::print_ok(&format!("Miner is running (PID {})", pid)),
        None => {
            ui::print_warn("Miner is not running.");
            ui::print_info("Start with: zion mine start");
        }
    }
    println!();
    Ok(())
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
