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
    },
    /// Stop the running miner
    Stop,
    /// Show miner process status
    Status,
}

pub async fn run(cfg: &Config, cmd: MineCmd) -> Result<()> {
    match cmd {
        MineCmd::Start {
            pool, wallet, algorithm, backend, worker, auto_node, auto_pool, console,
        } => start_mining(cfg, pool, wallet, algorithm, backend, worker, auto_node, auto_pool, console).await,
        MineCmd::Stop => stop_mining(),
        MineCmd::Status => miner_status(),
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
    println!();

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
        "miner binary not found. Download zion-miner-windows-x86_64.exe from https://zionterranova.com/download or build: cargo build --release -p zion-miner"
    ))
}
