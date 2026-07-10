//! Local mining pool control — start, stop, status.
//!
//! Manages the `zion-pool` `server` binary via PID file `~/.zion/pool.pid`.
//! A public user typically does NOT need to run a pool; this is for local
//! solo-mining or development.

use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;

use crate::config::{self, Config};
use crate::process;
use crate::ui;

#[derive(Subcommand)]
pub enum PoolCmd {
    /// Start a local mining pool
    Start {
        /// Override bind address (default: 0.0.0.0:8444)
        #[arg(long)]
        bind: Option<String>,
        /// Override node RPC address (default: config node.rpc_host:port)
        #[arg(long)]
        node_rpc: Option<String>,
        /// Path to pool wallet / fee address
        #[arg(long)]
        wallet: Option<String>,
        /// Run pool in a visible console window
        #[arg(long)]
        console: bool,
    },
    /// Stop the running local pool
    Stop,
    /// Show local pool process status
    Status,
}

pub async fn run(cfg: &Config, cmd: PoolCmd) -> Result<()> {
    match cmd {
        PoolCmd::Start { bind, node_rpc, wallet, console } => {
            start_pool(cfg, bind, node_rpc, wallet, console).await
        }
        PoolCmd::Stop => stop_pool(),
        PoolCmd::Status => pool_status(),
    }
}

async fn start_pool(
    cfg: &Config,
    bind: Option<String>,
    node_rpc: Option<String>,
    wallet: Option<String>,
    console: bool,
) -> Result<()> {
    ui::print_header("Start Pool");

    let bin = find_pool_binary()?;
    ui::print_row("Pool binary", &bin.display().to_string());

    let bind = bind.unwrap_or_else(|| cfg.pool.bind.clone());
    let node_rpc = node_rpc.unwrap_or_else(|| format!("{}:{}", cfg.node.rpc_host, cfg.node.rpc_port));
    let wallet = wallet.unwrap_or_else(|| cfg.pool.wallet.clone());

    ui::print_row("Pool bind", &bind);
    ui::print_row("Node RPC", &node_rpc);
    if !wallet.is_empty() {
        ui::print_row("Pool wallet", &wallet);
    }
    println!();

    let mut envs: Vec<(&str, String)> = Vec::new();
    envs.push(("ZION_POOL_BIND", bind));
    envs.push(("ZION_NODE_RPC_ADDR", node_rpc));
    envs.push(("ZION_POOL_LOOP_COUNT", "1000000".into()));
    envs.push(("ZION_NONCE_COUNT", "4096".into()));
    envs.push(("ZION_NONCE_COUNT_GPU", "262144".into()));
    if !wallet.is_empty() {
        envs.push(("ZION_POOL_WALLET", wallet));
    }
    if !cfg.miner.wallet.is_empty() {
        envs.push(("ZION_POOL_WALLET", cfg.miner.wallet.clone()));
    }

    let pid = process::start("pool", &bin, &[], &envs, console)?;

    ui::print_ok(&format!("Pool started (PID {})", pid));
    ui::print_info("Miners can connect to this pool's bind address.");
    ui::print_info("Stop: zion pool stop");
    println!();
    Ok(())
}

fn stop_pool() -> Result<()> {
    ui::print_header("Stop Pool");
    match process::stop("pool")? {
        true => ui::print_ok("Pool stopped."),
        false => ui::print_warn("Pool was not running."),
    }
    println!();
    Ok(())
}

fn pool_status() -> Result<()> {
    ui::print_header("Pool Status");
    match process::status("pool") {
        Some(pid) => ui::print_ok(&format!("Pool is running (PID {})", pid)),
        None => {
            ui::print_warn("Pool is not running.");
            ui::print_info("Start with: zion pool start");
        }
    }
    println!();
    Ok(())
}

fn find_pool_binary() -> Result<PathBuf> {
    if let Some(path) = config::load(None).ok().and_then(|c| c.binaries.pool) {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    for c in ["zion-pool-windows-x86_64", "zion-pool"] {
        if let Some(p) = process::find_binary(c) {
            return Ok(p);
        }
    }

    // Self-contained bundle.
    if let Ok(p) = crate::bundle::ensure_binary("pool") {
        if p.exists() {
            return Ok(p);
        }
    }

    // Bare `server` is too generic; search only safe locations.
    if let Some(p) = process::find_binary_safely("server") {
        return Ok(p);
    }

    Err(anyhow::anyhow!(
        "pool binary not found. Download zion-pool-windows-x86_64.exe from https://zionterranova.com/download or build: cargo build --release -p zion-pool --bin server"
    ))
}
