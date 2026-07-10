//! Live monitor — shows local process health for node, pool, miner
//! plus the latest chain height from the configured node RPC.

use anyhow::Result;
use std::time::Duration;

use crate::config::Config;
use crate::commands::stats;
use crate::ui;

pub async fn run(cfg: &Config) -> Result<()> {
    ui::print_header("ZION Monitor — Local Stack");

    let s = stats::collect(cfg).await;

    // Node
    ui::print_section("Node");
    match s.node_process {
        Some(pid) => ui::print_ok(&format!("Running (PID {})", pid)),
        None => ui::print_warn("Not running"),
    }

    let client = zion_sdk::node::NodeClient::builder(&cfg.node.rpc_host, cfg.node.rpc_port)
        .connect_timeout(Duration::from_secs(5))
        .build();
    match client.chain_info().await {
        Ok(chain) => {
            ui::print_row("Height", &chain.chain_height.to_string());
            ui::print_row("Tip", &chain.tip_hash_hex);
            ui::print_row("Network", &chain.network);
        }
        Err(e) => ui::print_warn(&format!("RPC not reachable: {}", e)),
    }

    // Pool
    ui::print_section("Pool");
    match s.pool_process {
        Some(pid) => ui::print_ok(&format!("Running (PID {})", pid)),
        None => ui::print_info("Not running (public pool is used by default)"),
    }

    // Miner
    ui::print_section("Miner");
    match s.miner_process {
        Some(pid) => ui::print_ok(&format!("Running (PID {})", pid)),
        None => ui::print_warn("Not running"),
    }

    // Miner stats
    if let Some(ms) = &s.miner_stats {
        ui::print_row("Hashrate", &format!("{:.1} H/s", ms.hashrate_hps));
        ui::print_row("Accepted", &ms.accepted_shares.to_string());
        ui::print_row("Rejected", &ms.rejected_shares.to_string());
        if let Some(w) = &ms.worker_name {
            ui::print_row("Worker", w);
        }
        if let Some(a) = &ms.algorithm {
            ui::print_row("Algorithm", a);
        }
        if let Some(p) = &ms.pool_addr {
            ui::print_row("Pool", p);
        }
    }

    // Wallet
    if !s.wallet_address.is_empty() {
        ui::print_section("Wallet");
        ui::print_row("Address", &s.wallet_address);
        if let Some(bal) = s.wallet_balance {
            ui::print_row("Balance", &format!("{:.6} ZION", bal));
        }
    }

    println!();
    ui::print_info("Commands: zion node start | zion mine start | zion pool start");
    println!();
    Ok(())
}
