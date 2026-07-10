//! Health check — pings all public endpoints (node RPC, pool, AI, website).
//!
//! Gives the user a quick overview of what's up and what's down.

use anyhow::Result;
use reqwest::Client;
use std::time::Duration;

use crate::config::Config;
use crate::ui;

pub async fn run(cfg: &Config) -> Result<()> {
    ui::print_header("ZION Network Status");
    println!();

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    // ── Node RPC (TCP JSON-RPC) ──────────────────────────────────────
    ui::print_section("Node RPC");
    let node_client = zion_sdk::node::NodeClient::builder(&cfg.node.rpc_host, cfg.node.rpc_port)
        .connect_timeout(Duration::from_secs(5))
        .build();

    match node_client.chain_info().await {
        Ok(chain) => {
            ui::print_ok(&format!("Online — height {}, {} peers in mempool", chain.chain_height, chain.mempool_transactions));
            ui::print_row("Network", &chain.network);
            ui::print_row("Tip", &chain.tip_hash_hex);
        }
        Err(e) => ui::print_err(&format!("Offline — {}", e)),
    }

    // ── Pool ─────────────────────────────────────────────────────────
    // Pool uses stratum protocol (TCP), not HTTP — do a TCP connect check.
    ui::print_section("Mining Pool");
    let pool_addr = format!("{}:{}", cfg.pool.host, cfg.pool.port);
    match tokio::net::TcpStream::connect(&pool_addr).await {
        Ok(_) => ui::print_ok(&format!("Online — {} (stratum/TCP)", pool_addr)),
        Err(e) => ui::print_err(&format!("Offline — {} ({})", pool_addr, e)),
    }

    // ── AI (Hiran) ───────────────────────────────────────────────────
    ui::print_section("Hiran AI");
    if cfg.ai.url.is_empty() {
        ui::print_info("Not configured (optional). Set with: zion config set ai.url <endpoint>");
    } else {
        let ai_health = format!("{}/health", cfg.ai.url.trim_end_matches('/'));
        match client.get(&ai_health).send().await {
            Ok(r) if r.status().is_success() => {
                ui::print_ok(&format!("Online — {}", cfg.ai.url));
            }
            Ok(r) => {
                ui::print_warn(&format!("Responded HTTP {} — {}", r.status(), cfg.ai.url));
            }
            Err(e) => ui::print_warn(&format!("Offline — {} (AI is optional)", e)),
        }
    }

    // ── Website ──────────────────────────────────────────────────────
    ui::print_section("Website");
    match client.get("https://zionterranova.com").send().await {
        Ok(r) if r.status().is_success() => {
            ui::print_ok("Online — https://zionterranova.com");
        }
        Ok(r) => {
            ui::print_warn(&format!("Responded HTTP {}", r.status()));
        }
        Err(e) => ui::print_err(&format!("Offline — {}", e)),
    }

    // ── Explorer ─────────────────────────────────────────────────────
    ui::print_section("Block Explorer");
    match client.get("https://zionterranova.com/explorer").send().await {
        Ok(r) if r.status().is_success() => {
            ui::print_ok("Online — https://zionterranova.com/explorer");
        }
        Ok(r) => {
            ui::print_warn(&format!("Responded HTTP {}", r.status()));
        }
        Err(e) => ui::print_err(&format!("Offline — {}", e)),
    }

    println!();
    Ok(())
}
