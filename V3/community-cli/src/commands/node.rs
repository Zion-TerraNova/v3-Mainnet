//! Node operations — read-only queries and local service lifecycle.
//!
//! Read-only queries go through `zion_sdk::NodeClient` (TCP JSON-RPC). The
//! `start`/`stop`/`status` subcommands manage a local `node` binary via PID
//! file in `~/.zion/node.pid`.

use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;
use std::time::Duration;

use crate::config::{self, Config};
use crate::process;
use crate::ui;

#[derive(Subcommand)]
pub enum NodeCmd {
    /// Start a local ZION node
    Start {
        /// Override P2P bind address (default: 0.0.0.0:8333)
        #[arg(long)]
        p2p_bind: Option<String>,
        /// Override RPC bind address (default: 0.0.0.0:8443)
        #[arg(long)]
        rpc_bind: Option<String>,
        /// Override seed peers (comma-separated)
        #[arg(long)]
        seed_peers: Option<String>,
        /// Path to node state DB (default: ~/.zion/node-state.db)
        #[arg(long)]
        state_path: Option<String>,
        /// Run node in a visible console window (default: detached)
        #[arg(long)]
        console: bool,
    },
    /// Stop the running local node
    Stop,
    /// Show local node process status
    Status,
    /// Show node info (version, network, bind addresses, peer count)
    Info,
    /// Show chain info (height, tip hash, mempool size)
    Chain,
    /// Show connected peers
    Peers,
    /// Show supply info (total supply, mined, remaining, block reward)
    Supply,
    /// Show mempool info
    Mempool,
}

pub async fn run(cfg: &Config, cmd: NodeCmd) -> Result<()> {
    match cmd {
        NodeCmd::Start {
            p2p_bind,
            rpc_bind,
            seed_peers,
            state_path,
            console,
        } => start_node(cfg, p2p_bind, rpc_bind, seed_peers, state_path, console).await,
        NodeCmd::Stop => stop_node(),
        NodeCmd::Status => node_status(),
        NodeCmd::Info => node_info(cfg).await,
        NodeCmd::Chain => node_chain(cfg).await,
        NodeCmd::Peers => node_peers(cfg).await,
        NodeCmd::Supply => node_supply(cfg).await,
        NodeCmd::Mempool => node_mempool(cfg).await,
    }
}

async fn start_node(
    cfg: &Config,
    p2p_bind: Option<String>,
    rpc_bind: Option<String>,
    seed_peers: Option<String>,
    state_path: Option<String>,
    console: bool,
) -> Result<()> {
    ui::print_header("Start Node");

    let bin = find_node_binary()?;
    ui::print_row("Node binary", &bin.display().to_string());

    let p2p_bind = p2p_bind.unwrap_or_else(|| cfg.node.p2p_bind.clone());
    let rpc_bind = rpc_bind.unwrap_or_else(|| format!("{}:{}", cfg.node.rpc_host, cfg.node.rpc_port));
    let seed_peers = seed_peers.unwrap_or_else(|| cfg.node.seed_peers.clone());
    let state_path = state_path.unwrap_or_else(|| default_state_path());

    ui::print_row("P2P bind", &p2p_bind);
    ui::print_row("RPC bind", &rpc_bind);
    ui::print_row("Seed peers", &seed_peers);
    ui::print_row("State DB", &state_path);
    println!();

    let mut envs: Vec<(&str, String)> = Vec::new();
    envs.push(("ZION_NODE_ID", cfg.node.node_id.clone()));
    envs.push(("ZION_P2P_BIND", p2p_bind));
    envs.push(("ZION_RPC_BIND", rpc_bind));
    if !seed_peers.is_empty() {
        envs.push(("ZION_SEED_PEERS", seed_peers));
    }
    envs.push(("ZION_NODE_STATE_PATH", state_path));
    if !cfg.miner.wallet.is_empty() {
        envs.push(("ZION_MINER_ADDRESS", cfg.miner.wallet.clone()));
    }
    if !cfg.node.humanitarian_wallet.is_empty() {
        envs.push(("ZION_HUMANITARIAN_WALLET", cfg.node.humanitarian_wallet.clone()));
    }
    if !cfg.node.issobella_wallet.is_empty() {
        envs.push(("ZION_ISSOBELLA_WALLET", cfg.node.issobella_wallet.clone()));
    }

    let pid = process::start("node", &bin, &[], &envs, console)?;

    ui::print_ok(&format!("Node started (PID {})", pid));
    ui::print_info("It may take a few seconds to accept RPC connections.");
    ui::print_info("Check status: zion node status");
    ui::print_info("Stop: zion node stop");
    println!();
    Ok(())
}

fn stop_node() -> Result<()> {
    ui::print_header("Stop Node");
    match process::stop("node")? {
        true => {
            ui::print_ok("Node stopped.");
        }
        false => {
            ui::print_warn("Node was not running.");
        }
    }
    println!();
    Ok(())
}

fn node_status() -> Result<()> {
    ui::print_header("Node Status");
    match process::status("node") {
        Some(pid) => {
            ui::print_ok(&format!("Node is running (PID {})", pid));
        }
        None => {
            ui::print_warn("Node is not running.");
            ui::print_info("Start with: zion node start");
        }
    }
    println!();
    Ok(())
}

fn find_node_binary() -> Result<PathBuf> {
    // 1. Prefer configured path.
    if let Some(path) = config::load(None).ok().and_then(|c| c.binaries.node) {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    // 2. Try known public-CLI download names next to the CLI or in ~/.zion.
    for c in ["zion-node-windows-x86_64", "zion-node"] {
        if let Some(p) = process::find_binary(c) {
            return Ok(p);
        }
    }

    // 3. Self-contained bundle: extract from the zion binary itself.
    if let Ok(p) = crate::bundle::ensure_binary("node") {
        if p.exists() {
            return Ok(p);
        }
    }

    // 4. Last resort: look for the bare `node` binary only in safe locations
    // (current dir, exe dir, ~/.zion), never in PATH, to avoid picking up Node.js.
    if let Some(p) = process::find_binary_safely("node") {
        return Ok(p);
    }

    Err(anyhow::anyhow!(
        "node binary not found. Download zion-node-windows-x86_64.exe from https://zionterranova.com/download or build: cargo build --release -p zion-core --bin node"
    ))
}

fn default_state_path() -> String {
    dirs::home_dir()
        .map(|h| h.join(".zion").join("node-state.db").to_string_lossy().to_string())
        .unwrap_or_else(|| ".zion-node-state.db".to_string())
}

// ─── Read-only RPC helpers ───────────────────────────────────────────────────

async fn node_info(cfg: &Config) -> Result<()> {
    with_client(cfg, "Node Info", |client| async move {
        match client.node_info().await {
            Ok(info) => {
                ui::print_row("Node ID", &info.node_id);
                ui::print_row("Protocol", &info.protocol_version);
                ui::print_row("Network", &info.network);
                ui::print_row("Chain height", &info.chain_height.to_string());
                ui::print_row("P2P bind", &info.p2p_bind);
                ui::print_row("RPC bind", &info.rpc_bind);
                ui::print_row("Pool bind", &info.pool_bind);
                ui::print_row("Known peers", &info.known_peers.to_string());
                ui::print_row("Accepted blocks", &info.accepted_blocks.to_string());
                ui::print_row("Mempool txs", &info.mempool_transactions.to_string());
                ui::print_row("TX model", &info.transaction_model);
            }
            Err(e) => ui::print_err(&format!("Cannot reach node: {}", e)),
        }
    })
    .await
}

async fn node_chain(cfg: &Config) -> Result<()> {
    with_client(cfg, "Chain Info", |client| async move {
        match client.chain_info().await {
            Ok(chain) => {
                ui::print_row("Network", &chain.network);
                ui::print_row("Consensus", &chain.consensus_profile);
                ui::print_row("Height", &chain.chain_height.to_string());
                ui::print_row("Tip hash", &chain.tip_hash_hex);
                ui::print_row("Accepted blocks", &chain.accepted_blocks.to_string());
                ui::print_row("Mempool txs", &chain.mempool_transactions.to_string());
                ui::print_row("Protocol", &chain.protocol_version);
                ui::print_row("TX model", &chain.transaction_model);
            }
            Err(e) => ui::print_err(&format!("Cannot reach node: {}", e)),
        }
    })
    .await
}

async fn node_peers(cfg: &Config) -> Result<()> {
    with_client(cfg, "Connected Peers", |client| async move {
        match client.peer_info().await {
            Ok(peer_info) => {
                ui::print_row("Peer count", &peer_info.count.to_string());
                println!();
                for (i, peer) in peer_info.peers.iter().enumerate() {
                    println!("  {:>3}. {}:{} — {}", i + 1, peer.host, peer.port, peer.address);
                }
                if peer_info.peers.is_empty() {
                    ui::print_warn("No peers connected.");
                }
            }
            Err(e) => ui::print_err(&format!("Cannot reach node: {}", e)),
        }
    })
    .await
}

async fn node_supply(cfg: &Config) -> Result<()> {
    with_client(cfg, "Supply Info", |client| async move {
        match client.supply_info().await {
            Ok(supply) => {
                ui::print_row("Total supply", &format!("{} ZION", supply.total_supply_zion));
                ui::print_row("Premine", &format!("{} ZION", supply.premine_zion));
                ui::print_row("Mining emission", &format!("{} ZION", supply.mining_emission_zion));
                ui::print_row("Mined so far", &format!("{} ZION", supply.mined_so_far_zion));
                ui::print_row("Mined %", &supply.supply_mined_percent);
                ui::print_row("Circulating", &format!("{} ZION", supply.circulating_supply_zion));
                ui::print_row("Remaining", &format!("{} ZION", supply.remaining_supply_zion));
                ui::print_row("Block reward", &format!("{:.6} ZION", supply.block_reward_zion));
                ui::print_row("Height", &supply.height.to_string());
            }
            Err(e) => ui::print_err(&format!("Cannot reach node: {}", e)),
        }
    })
    .await
}

async fn node_mempool(cfg: &Config) -> Result<()> {
    with_client(cfg, "Mempool", |client| async move {
        match client.mempool_info().await {
            Ok(mp) => {
                ui::print_row("Size", &mp.size.to_string());
                ui::print_row("Template txs", &mp.template_transactions.to_string());
                ui::print_row("Template fees", &format!("{} ZION", mp.template_total_fees_zion));
                ui::print_row("TX model", &mp.transaction_model);
            }
            Err(e) => ui::print_err(&format!("Cannot reach node: {}", e)),
        }
    })
    .await
}

async fn with_client<F, Fut>(cfg: &Config, title: &str, f: F) -> Result<()>
where
    F: FnOnce(zion_sdk::node::NodeClient) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    ui::print_header(title);
    let client = zion_sdk::node::NodeClient::builder(&cfg.node.rpc_host, cfg.node.rpc_port)
        .connect_timeout(Duration::from_secs(5))
        .build();
    f(client).await;
    println!();
    Ok(())
}
