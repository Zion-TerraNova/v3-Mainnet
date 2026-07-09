use anyhow::Result;
use std::net::ToSocketAddrs;

use crate::config::Config;
use crate::rpc::node_rpc;
use crate::ui;

/// `zion status` — health check for node and pool
pub async fn run(cfg: &Config) -> Result<()> {
    ui::print_banner();
    ui::print_header("Network Status");

    // ── Node ───────────────────────────────────────────────────────
    let (host, port) = cfg.rpc();
    let result = node_rpc::call0(host, port, "getChainInfo").await;
    match &result {
        Ok(v) => {
            let height = v["chain_height"].as_u64().unwrap_or(0);
            let hash = v["tip_hash"].as_str().unwrap_or("?");
            let network = v["network"].as_str().unwrap_or("mainnet");
            let proto = v["protocol_version"].as_u64().unwrap_or(0);
            let mempool = v["mempool_transactions"].as_u64().unwrap_or(0);

            let peers_v = node_rpc::call0(host, port, "getNodeInfo").await;
            let peers = peers_v
                .as_ref()
                .ok()
                .and_then(|p| p["known_peers"].as_u64())
                .unwrap_or(0);

            let short = if hash.len() > 12 { &hash[..12] } else { hash };
            ui::print_ok(&format!(
                "Node    {}:{}  network={} proto=v{}  height={} peers={} tip={}...",
                host, port, network, proto, height, peers, short
            ));
            ui::print_row("Mempool", &format!("{} pending txs", mempool));
        }
        Err(e) => ui::print_err(&format!(
            "Node    {}:{} — {}",
            host, port, e
        )),
    }

    // ── Pool ───────────────────────────────────────────────────────
    let (pool_host, pool_port) = cfg.pool_endpoint();
    let pool_alive = tcp_probe(
        pool_host,
        pool_port,
        std::time::Duration::from_secs(3),
    );
    if pool_alive {
        ui::print_ok(&format!(
            "Pool    {}:{} accepting connections",
            pool_host, pool_port
        ));
    } else {
        ui::print_warn(&format!(
            "Pool    {}:{} not reachable",
            pool_host, pool_port
        ));
    }

    println!();
    ui::print_info(&format!(
        "Config: {}  (zion config show)",
        "~/.zion/zion.toml"
    ));
    println!();
    Ok(())
}

fn tcp_probe(host: &str, port: u16, timeout: std::time::Duration) -> bool {
    let addr = format!("{}:{}", host, port);
    match addr.to_socket_addrs() {
        Ok(mut addrs) => addrs.any(|a| std::net::TcpStream::connect_timeout(&a, timeout).is_ok()),
        Err(_) => false,
    }
}
