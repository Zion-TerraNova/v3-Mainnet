//! Preflight diagnostics — checks local environment, config, connectivity,
//! and wallet validity before the user starts mining or sending transactions.

use anyhow::Result;
use std::path::PathBuf;

use crate::bundle;
use crate::config::{self, Config};
use crate::ui;

pub async fn run(cfg: &Config) -> Result<()> {
    ui::print_header("ZION Doctor — Preflight Diagnostics");
    println!();

    let mut warnings = 0u32;
    let mut errors = 0u32;

    // ── 1. Config file ───────────────────────────────────────────────
    ui::print_section("Config");
    match config::config_path() {
        Ok(path) => {
            if path.exists() {
                ui::print_ok(&format!("Config found: {}", path.display()));
            } else {
                ui::print_warn(&format!("No config file at {} — using defaults", path.display()));
                warnings += 1;
            }
        }
        Err(e) => {
            ui::print_err(&format!("Cannot resolve config path: {}", e));
            errors += 1;
        }
    }

    // ── 2. Node connectivity ─────────────────────────────────────────
    ui::print_section("Node Connectivity");
    ui::print_row("RPC endpoint", &format!("{}:{}", cfg.node.rpc_host, cfg.node.rpc_port));

    let node_client = zion_sdk::node::NodeClient::builder(&cfg.node.rpc_host, cfg.node.rpc_port)
        .connect_timeout(std::time::Duration::from_secs(5))
        .build();

    match node_client.chain_info().await {
        Ok(chain) => {
            ui::print_ok(&format!("Node reachable — height {}", chain.chain_height));
            ui::print_row("Network", &chain.network);
            ui::print_row("Consensus", &chain.consensus_profile);
            ui::print_row("Tip", &chain.tip_hash_hex);
            ui::print_row("Mempool txs", &chain.mempool_transactions.to_string());
        }
        Err(e) => {
            ui::print_err(&format!("Node unreachable: {}", e));
            errors += 1;
        }
    }

    // Peers
    match node_client.peer_info().await {
        Ok(peers) => {
            ui::print_row("Connected peers", &peers.count.to_string());
        }
        Err(_) => {}
    }

    // Supply
    match node_client.supply_info().await {
        Ok(supply) => {
            ui::print_row("Total supply", &format!("{} ZION", supply.total_supply_zion));
            ui::print_row("Mined so far", &format!("{} ZION", supply.mined_so_far_zion));
            ui::print_row("Mined %", &format!("{}%", supply.supply_mined_percent));
            ui::print_row("Block reward", &format!("{:.6} ZION", supply.block_reward_zion));
        }
        Err(_) => {}
    }

    // ── 3. Wallet ────────────────────────────────────────────────────
    ui::print_section("Wallet");
    if cfg.miner.wallet.is_empty() {
        ui::print_err("No wallet address configured.");
        ui::print_info("Run: zion wallet new --mnemonic --set-default");
        errors += 1;
    } else if zion_core::crypto::is_valid_address(&cfg.miner.wallet) {
        ui::print_ok(&format!("Wallet address valid: {}", cfg.miner.wallet));

        // Check balance
        let wallet_client = zion_sdk::wallet::WalletClient::new(node_client.clone());
        match wallet_client.balance_breakdown(&cfg.miner.wallet).await {
            Ok(bal) => {
                ui::print_row("Balance", &format!("{:.6} ZION", bal.total_zion));
                if bal.total_zion == 0.0 {
                    ui::print_warn("Balance is 0 — you won't be able to send transactions yet.");
                    warnings += 1;
                }
            }
            Err(e) => {
                ui::print_warn(&format!("Cannot fetch balance: {}", e));
                warnings += 1;
            }
        }
    } else {
        ui::print_err(&format!("Invalid wallet address: {}", cfg.miner.wallet));
        errors += 1;
    }

    // ── 4. Bundled binaries ───────────────────────────────────────────
    ui::print_section("Bundled Binaries");
    match bundle::extract_all() {
        Ok(binaries) => {
            for (name, path) in binaries {
                ui::print_ok(&format!("{} extracted: {}", name, path.display()));
            }
        }
        Err(e) => {
            ui::print_warn(&format!("Could not extract all bundled binaries: {}", e));
            warnings += 1;
        }
    }

    // ── 5. Miner binary availability ──────────────────────────────────
    ui::print_section("Miner Binary");
    let mut miner_found = false;

    let miner_names: &[&str] = if cfg!(windows) {
        &["zion-miner.exe", "miner.exe"]
    } else {
        &["zion-miner", "miner"]
    };

    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(if cfg!(windows) { ';' } else { ':' }) {
            for name in miner_names {
                let candidate = PathBuf::from(dir).join(name);
                if candidate.exists() {
                    ui::print_ok(&format!("Miner found: {}", candidate.display()));
                    miner_found = true;
                    break;
                }
            }
            if miner_found {
                break;
            }
        }
    }

    if !miner_found {
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            let home = PathBuf::from(home);
            for name in miner_names {
                let candidate = home.join(".zion").join("bin").join(name);
                if candidate.exists() {
                    ui::print_ok(&format!("Miner found: {}", candidate.display()));
                    miner_found = true;
                    break;
                }
            }
        }
    }

    if !miner_found {
        ui::print_warn("zion-miner binary not found in PATH or ~/.zion/bin/");
        ui::print_info("The bundled miner will be extracted on first `zion mine start`.");
        warnings += 1;
    }

    // ── 6. Pool connectivity ─────────────────────────────────────────
    ui::print_section("Pool Connectivity");
    ui::print_row("Pool endpoint", &format!("{}:{}", cfg.pool.host, cfg.pool.port));

    let pool_addr = format!("{}:{}", cfg.pool.host, cfg.pool.port);
    match tokio::net::TcpStream::connect(&pool_addr).await {
        Ok(_) => ui::print_ok("Pool TCP port reachable."),
        Err(e) => {
            ui::print_warn(&format!("Pool TCP connect failed: {}", e));
            warnings += 1;
        }
    }

    // ── 7. AI endpoint ───────────────────────────────────────────────
    ui::print_section("Hiran AI");
    if cfg.ai.url.is_empty() {
        ui::print_info("Not configured (optional). Set with: zion config set ai.url <endpoint>");
    } else {
        ui::print_row("Endpoint", &cfg.ai.url);
        let health_url = format!("{}/health", cfg.ai.url.trim_end_matches('/'));
        match reqwest::Client::new()
            .get(&health_url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => ui::print_ok("Hiran AI reachable."),
            Ok(r) => {
                ui::print_warn(&format!("Hiran AI responded HTTP {}", r.status()));
                warnings += 1;
            }
            Err(e) => {
                ui::print_warn(&format!("Hiran AI unreachable: {}", e));
                warnings += 1;
            }
        }
    }

    // ── Summary ──────────────────────────────────────────────────────
    println!();
    ui::print_section("Summary");
    if errors == 0 && warnings == 0 {
        ui::print_ok("All checks passed. You're ready to mine and transact!");
    } else {
        if errors > 0 {
            ui::print_err(&format!("{} error(s) must be fixed before mining.", errors));
        }
        if warnings > 0 {
            ui::print_warn(&format!("{} warning(s) — not blocking but worth checking.", warnings));
        }
    }
    println!();

    Ok(())
}
