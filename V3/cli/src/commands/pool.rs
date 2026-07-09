use anyhow::Result;
use clap::Subcommand;
use serde_json::json;
use std::net::ToSocketAddrs;

use crate::config::Config;
use crate::rpc::node_rpc;
use crate::ui;

#[derive(Subcommand)]
pub enum PoolCmd {
    /// Pool stats: connected miners, hashrate, shares
    Stats {
        /// Target pool: local | core | edge | host:port (default: local)
        #[arg(default_value = "local")]
        target: String,
    },
    /// List active workers and hashrate
    Miners {
        /// Target pool: local | core | edge | host:port (default: local)
        #[arg(default_value = "local")]
        target: String,
    },
    /// Show pool config
    Config {
        /// Target pool: local | core | edge | host:port (default: local)
        #[arg(default_value = "local")]
        target: String,
    },
    /// PPLNS earnings for an address
    Earnings {
        #[arg(long)]
        address: Option<String>,
        /// Target pool: local | core | edge | host:port (default: local)
        #[arg(default_value = "local")]
        target: String,
    },
}

pub async fn run(cfg: &Config, cmd: PoolCmd) -> Result<()> {
    match cmd {
        PoolCmd::Stats { target } => pool_stats(cfg, &target).await,
        PoolCmd::Miners { target } => pool_miners(cfg, &target).await,
        PoolCmd::Config { target } => {
            let (host, port) = cfg.target_pool(&target);
            ui::print_header("Pool Config");
            ui::print_row("Host", host);
            ui::print_row("Port", &port.to_string());
            ui::print_row(
                "Algorithm",
                "multi-algo (deeksha_lite_v1, deeksha_lite_fire, cosmic_harmony_ekam_deeksha_v2)",
            );
            println!();
            Ok(())
        }
        PoolCmd::Earnings { address, target } => {
            let (rpc_host, rpc_port) = cfg.target_rpc(&target);
            ui::print_header("PPLNS Earnings");
            let addr = address.unwrap_or_else(|| cfg.miner.wallet.clone());
            if addr.is_empty() {
                ui::print_warn("No address specified. Use --address <addr>");
                return Ok(());
            }
            // Query pool stats endpoint on node RPC for now
            let result = node_rpc::call(
                rpc_host,
                rpc_port,
                "get_miner_stats",
                json!({ "address": addr }),
            )
            .await;
            match result {
                Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
                Err(e) => ui::print_warn(&format!("Pool earnings not available: {}", e)),
            }
            println!();
            Ok(())
        }
    }
}

async fn pool_stats(cfg: &Config, target: &str) -> Result<()> {
    let (host, port) = cfg.target_pool(target);
    ui::print_header("Pool Stats");

    // The V3 pool is a pure TCP stratum server — no HTTP stats API.
    // Probe liveness via TCP, then show node-side mempool context.
    let alive = tcp_probe(host, port, std::time::Duration::from_secs(3));

    ui::print_row("Pool host", &format!("{}:{}", host, port));
    ui::print_row("Algorithm", "multi-algo (session-based: deeksha_lite_v1, deeksha_lite_fire, cosmic_harmony_ekam_deeksha_v2)");
    ui::print_row("Protocol", "ZION stratum v3 (TCP)");

    if alive {
        ui::print_ok(&format!("Pool stratum reachable ({}:{})", host, port));
    } else {
        ui::print_warn(&format!(
            "Pool stratum not reachable ({}:{}) — start with: zion start pool",
            host, port
        ));
    }

    // Pull chain context from node to show block template info
    let (rpc_host, rpc_port) = cfg.target_rpc(target);
    let node_result = node_rpc::call0(rpc_host, rpc_port, "getMempoolInfo").await;
    if let Ok(v) = node_result {
        let size = v["size"].as_u64().unwrap_or(0);
        let tmpl_txs = v["template_transactions"].as_u64().unwrap_or(0);
        ui::print_row("Mempool txs", &format!("{} pending", size));
        ui::print_row("Template txs", &tmpl_txs.to_string());
    }

    println!();
    Ok(())
}

async fn pool_miners(cfg: &Config, target: &str) -> Result<()> {
    let (host, port) = cfg.target_pool(target);
    let (rpc_host, rpc_port) = cfg.target_rpc(target);
    ui::print_header("Active Workers");

    // Pool is stratum-only; get template info from node as proxy indicator
    ui::print_info(&format!("Pool stratum: {}:{}", host, port));

    let alive = tcp_probe(host, port, std::time::Duration::from_secs(3));
    if !alive {
        ui::print_warn("Pool stratum not reachable — cannot query worker sessions");
        println!();
        return Ok(());
    }

    // Pool does not expose an HTTP session API; worker list would require
    // pool internal state. Show node block template as proxy for current work.
    let tmpl = node_rpc::call0(rpc_host, rpc_port, "getBlockTemplate").await;
    match tmpl {
        Ok(v) => {
            let height = v["height"].as_u64().unwrap_or(0);
            let difficulty = v["difficulty"].as_u64().unwrap_or(0);
            ui::print_ok("Pool stratum is accepting connections");
            ui::print_row("Current template height", &height.to_string());
            ui::print_row("Current difficulty", &difficulty.to_string());
            ui::print_info(
                "Live per-worker session info requires pool metrics endpoint (Phase 4).",
            );
        }
        Err(e) => ui::print_warn(&format!("Node block template unavailable: {}", e)),
    }

    println!();
    Ok(())
}

/// Non-async TCP probe: returns true if a TCP connection can be established.
fn tcp_probe(host: &str, port: u16, timeout: std::time::Duration) -> bool {
    let addr = format!("{}:{}", host, port);
    match addr.to_socket_addrs() {
        Ok(mut addrs) => addrs.any(|a| std::net::TcpStream::connect_timeout(&a, timeout).is_ok()),
        Err(_) => false,
    }
}
