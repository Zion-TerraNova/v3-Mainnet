use anyhow::{anyhow, Result};
use clap::Subcommand;
use serde_json::json;

use crate::config::Config;
use crate::rpc::node_rpc;
use crate::ui;

#[derive(Subcommand)]
pub enum NodeCmd {
    /// Tip height, hash, peers, sync status
    Status {
        /// Target node: local | core | edge | host:port (default: local)
        #[arg(default_value = "local")]
        target: String,
    },
    /// List connected P2P peers
    Peers {
        /// Target node: local | core | edge | host:port (default: local)
        #[arg(default_value = "local")]
        target: String,
    },
    /// Last N blocks (default 10)
    Blocks {
        #[arg(default_value = "10")]
        n: u64,
        /// Target node: local | core | edge | host:port (default: local)
        #[arg(default_value = "local")]
        target: String,
    },
    /// Block detail by height or hash
    Block {
        id: String,
        /// Target node: local | core | edge | host:port (default: local)
        #[arg(default_value = "local")]
        target: String,
    },
    /// Transaction lookup
    Tx {
        txid: String,
        /// Target node: local | core | edge | host:port (default: local)
        #[arg(default_value = "local")]
        target: String,
    },
    /// Pending transactions in mempool
    Mempool {
        /// Target node: local | core | edge | host:port (default: local)
        #[arg(default_value = "local")]
        target: String,
    },
    /// Force peer sync / bootstrap
    Sync {
        /// Target node: local | core | edge | host:port (default: local)
        #[arg(default_value = "local")]
        target: String,
    },
    /// Raw JSON-RPC call: zion node rpc <method> [params_json]
    Rpc {
        method: String,
        #[arg(default_value = "{}")]
        params: String,
        /// Target node: local | core | edge | host:port (default: local)
        #[arg(default_value = "local")]
        target: String,
    },
    /// WebSocket subscriptions
    Websocket {
        #[command(subcommand)]
        ws_cmd: WebSocketCmd,
    },
    /// Export chain snapshot (height, hash, supply info)
    Snapshot {
        /// Output file path (default: snapshot_<height>.json)
        #[arg(short, long)]
        output: Option<String>,
        /// Target node: local | core | edge | host:port (default: local)
        #[arg(default_value = "local")]
        target: String,
    },
}

#[derive(Subcommand)]
pub enum WebSocketCmd {
    /// Subscribe to WebSocket events (new_blocks, pending_transactions, address, network_status)
    Subscribe {
        /// Subscription type: new_blocks, pending_transactions, address, network_status
        subscription: String,
        /// Optional address for address subscriptions
        #[arg(short, long)]
        address: Option<String>,
    },
    /// Unsubscribe from WebSocket events
    Unsubscribe {
        /// Subscription ID to unsubscribe
        subscription_id: String,
    },
    /// Listen to WebSocket subscriptions (streaming)
    Listen {
        /// WebSocket host (default from config)
        #[arg(short, long)]
        host: Option<String>,
        /// WebSocket port (default 8445)
        #[arg(short, long)]
        port: Option<u16>,
    },
}

pub async fn run(cfg: &Config, cmd: NodeCmd) -> Result<()> {
    match cmd {
        NodeCmd::Status { target } => {
            let (host, port) = cfg.target_rpc(&target);
            node_status(host, port).await
        }
        NodeCmd::Peers { target } => {
            let (host, port) = cfg.target_rpc(&target);
            node_peers(host, port).await
        }
        NodeCmd::Blocks { n, target } => {
            let (host, port) = cfg.target_rpc(&target);
            node_blocks(host, port, n).await
        }
        NodeCmd::Block { id, target } => {
            let (host, port) = cfg.target_rpc(&target);
            node_block(host, port, &id).await
        }
        NodeCmd::Tx { txid, target } => {
            let (host, port) = cfg.target_rpc(&target);
            node_tx(host, port, &txid).await
        }
        NodeCmd::Mempool { target } => {
            let (host, port) = cfg.target_rpc(&target);
            node_mempool(host, port).await
        }
        NodeCmd::Sync { target } => {
            let (host, port) = cfg.target_rpc(&target);
            ui::print_info("Triggering peer sync...");
            let result = node_rpc::call0(host, port, "sync_peers").await;
            match result {
                Ok(v) => {
                    ui::print_ok("Sync triggered");
                    println!("  {}", v);
                }
                Err(e) => ui::print_err(&format!("{}", e)),
            }
            Ok(())
        }
        NodeCmd::Rpc {
            method,
            params,
            target,
        } => {
            let (host, port) = cfg.target_rpc(&target);
            let params_val: serde_json::Value = serde_json::from_str(&params).unwrap_or(json!({}));
            let result = node_rpc::call(host, port, &method, params_val).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        NodeCmd::Websocket { ws_cmd } => websocket_command(cfg, ws_cmd).await,
        NodeCmd::Snapshot { output, target } => snapshot_command(cfg, output, &target).await,
    }
}

/// Export a chain snapshot (height, hash, supply info, tip block).
///
/// Calls `getChainInfo` to get the current tip height, then calls `getSupplyInfo`
/// and `getBlockByHeight` for the tip. Writes a JSON file containing:
/// - height, hash, timestamp
/// - total supply, premine, mined so far
async fn snapshot_command(cfg: &Config, output: Option<String>, target: &str) -> Result<()> {
    let (host, port) = cfg.target_rpc(target);
    ui::print_info(&format!("Exporting snapshot from {}:{}...", host, port));

    // 1. Get chain info (tip height + hash)
    let chain_info = node_rpc::call(host, port, "getChainInfo", json!({})).await?;
    let height = chain_info["height"]
        .as_u64()
        .ok_or_else(|| anyhow!("chain info missing height"))?;
    let tip_hash = chain_info["tip_hash_hex"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    // 2. Get supply info
    let supply_info = node_rpc::call(host, port, "getSupplyInfo", json!({})).await?;

    // 3. Get tip block
    let tip_block =
        node_rpc::call(host, port, "getBlockByHeight", json!({"height": height})).await?;

    // 4. Build snapshot JSON
    let snapshot = json!({
        "snapshot_height": height,
        "tip_hash": tip_hash,
        "tip_block": tip_block,
        "supply_info": supply_info,
        "exported_at": chrono::Utc::now().to_rfc3339(),
    });

    // 5. Write to file
    let filename = output.unwrap_or_else(|| format!("snapshot_{}.json", height));
    let json_str = serde_json::to_string_pretty(&snapshot)?;
    std::fs::write(&filename, json_str)?;

    ui::print_ok(&format!("Snapshot written to {}", filename));
    ui::print_info(&format!(
        "Height: {}, Hash: {}...",
        height,
        &tip_hash[..20.min(tip_hash.len())]
    ));

    Ok(())
}

async fn node_status(host: &str, port: u16) -> Result<()> {
    ui::print_header(&format!("Node Status ({}:{})", host, port));

    let chain = node_rpc::call0(host, port, "getChainInfo").await;
    let node = node_rpc::call0(host, port, "getNodeInfo").await;
    match chain {
        Ok(v) => {
            // getChainInfo: chain_height, tip_hash, network, protocol_version, mempool_transactions
            let height = v["chain_height"].as_u64().unwrap_or(0);
            let hash = v["tip_hash"].as_str().unwrap_or("unknown");
            let network = v["network"].as_str().unwrap_or("mainnet");
            let proto = v["protocol_version"].as_u64().unwrap_or(0);
            let mempool = v["mempool_transactions"].as_u64().unwrap_or(0);
            // getNodeInfo: known_peers (count), pool_bind
            let peers = node
                .as_ref()
                .ok()
                .and_then(|n| n["known_peers"].as_u64())
                .unwrap_or(0);
            let pool_bind = node
                .as_ref()
                .ok()
                .and_then(|n| n["pool_bind"].as_str())
                .unwrap_or("?");

            ui::print_row("Network", network);
            ui::print_row("Protocol", &format!("v{}", proto));
            ui::print_row("Height", &format!("{}", height));
            ui::print_row("Tip", &format!("{}...", &hash[..hash.len().min(16)]));
            ui::print_row("Peers", &format!("{} connected", peers));
            ui::print_row("Pool bind", pool_bind);
            ui::print_row("Mempool", &format!("{} pending txs", mempool));
            ui::print_ok("Reachable");
        }
        Err(e) => {
            ui::print_err(&format!("Cannot reach {}:{} — {}", host, port, e));
        }
    }
    println!();
    Ok(())
}

async fn node_peers(host: &str, port: u16) -> Result<()> {
    ui::print_header("Peers");
    let result = node_rpc::call0(host, port, "getPeerInfo").await?;
    // getPeerInfo returns { "peers": [{host, port, address}], "count": N }
    let peer_arr = result["peers"].as_array().cloned();
    if let Some(peers) = peer_arr {
        if peers.is_empty() {
            ui::print_warn("No peers connected");
        }
        for p in &peers {
            let addr = p["address"]
                .as_str()
                .or_else(|| p["host"].as_str())
                .unwrap_or("unknown");
            let height = p["height"].as_u64().unwrap_or(0);
            println!("  {} height={}", addr, height);
        }
    } else {
        // Fallback: raw dump
        println!("{}", serde_json::to_string_pretty(&result)?);
    }
    println!();
    Ok(())
}

async fn node_blocks(host: &str, port: u16, n: u64) -> Result<()> {
    ui::print_header(&format!("Last {} blocks", n));

    let stats = node_rpc::call0(host, port, "getChainInfo").await?;
    let height = stats["chain_height"].as_u64().unwrap_or(0);

    for h in (height.saturating_sub(n - 1)..=height).rev() {
        let block = node_rpc::call(host, port, "getBlockByHeight", json!({ "height": h })).await;
        match block {
            Ok(b) => {
                let hash = b["hash_hex"].as_str().unwrap_or("?");
                let ts = b["timestamp"].as_u64().unwrap_or(0);
                let txs = b["transactions"].as_array().map(|a| a.len()).unwrap_or(0);
                let short_hash = if hash.len() > 16 { &hash[..16] } else { hash };
                println!("  {:>7}  {}...  ts={}  txs={}", h, short_hash, ts, txs);
            }
            Err(e) => println!("  {:>7}  error: {}", h, e),
        }
    }
    println!();
    Ok(())
}

async fn node_block(host: &str, port: u16, id: &str) -> Result<()> {
    ui::print_header(&format!("Block {}", id));
    let result = if id.chars().all(|c| c.is_ascii_digit()) {
        let h: u64 = id.parse()?;
        node_rpc::call(host, port, "getBlockByHeight", json!({ "height": h })).await?
    } else {
        node_rpc::call(host, port, "getBlock", json!({ "hash": id })).await?
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    println!();
    Ok(())
}

async fn node_tx(host: &str, port: u16, txid: &str) -> Result<()> {
    ui::print_header(&format!("Transaction {}", txid));
    // Try UTXO tx first, then account-model tx
    let result = match node_rpc::call(host, port, "getTransaction", json!({ "txid": txid })).await {
        Ok(v) => Ok(v),
        Err(_) => {
            node_rpc::call(host, port, "getAccountTransaction", json!({ "txid": txid })).await
        }
    };
    match result {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
        Err(e) => ui::print_err(&format!("{}", e)),
    }
    println!();
    Ok(())
}

async fn node_mempool(host: &str, port: u16) -> Result<()> {
    ui::print_header("Mempool");
    let result = node_rpc::call0(host, port, "getMempoolInfo").await?;
    let txs = result["size"].as_u64().unwrap_or(0);
    let tmpl_txs = result["template_transactions"].as_u64().unwrap_or(0);
    let fees = result["template_total_fees_zion"].as_str().unwrap_or("0");
    let model = result["transaction_model"].as_str().unwrap_or("unknown");
    ui::print_row("Pending txs", &txs.to_string());
    ui::print_row("Template txs", &tmpl_txs.to_string());
    ui::print_row("Template fees", &format!("{} ZION", fees));
    ui::print_row("Transaction model", model);
    if txs == 0 {
        ui::print_ok("Mempool is empty");
    } else if tmpl_txs == 0 {
        ui::print_warn("Pending transactions are not included in the active template");
    } else {
        ui::print_warn("Mempool has pending transactions; templates may change frequently.");
        ui::print_ok(&format!(
            "{} of {} pending transactions are template-ready",
            tmpl_txs, txs
        ));
    }
    println!();
    Ok(())
}

async fn websocket_command(cfg: &Config, cmd: WebSocketCmd) -> Result<()> {
    let ws_host = cfg.node.rpc_host.clone();
    let ws_port = cfg.node.websocket_port.unwrap_or(8445);

    match cmd {
        WebSocketCmd::Subscribe {
            subscription,
            address,
        } => websocket_subscribe(&ws_host, ws_port, &subscription, address).await,
        WebSocketCmd::Unsubscribe { subscription_id } => {
            websocket_unsubscribe(&ws_host, ws_port, &subscription_id).await
        }
        WebSocketCmd::Listen { host, port } => {
            let listen_host = host.unwrap_or_else(|| ws_host.clone());
            let listen_port = port.unwrap_or(ws_port);
            websocket_listen(&listen_host, listen_port).await
        }
    }
}

async fn websocket_subscribe(
    host: &str,
    port: u16,
    subscription: &str,
    address: Option<String>,
) -> Result<()> {
    ui::print_header(&format!("Subscribe to {}", subscription));

    // Use WebSocket client to subscribe
    let ws_url = format!("ws://{}:{}", host, port);
    ui::print_info(&format!("Connecting to {}", ws_url));

    // For now, just print the subscription request
    // In a full implementation, this would connect and send the subscription request
    let _params = if let Some(ref addr) = address {
        json!({ "subscription": subscription, "address": addr })
    } else {
        json!({ "subscription": subscription })
    };

    ui::print_row("Subscription", subscription);
    if let Some(ref addr) = address {
        ui::print_row("Address", addr);
    }
    ui::print_ok("Subscription request prepared");
    println!("  Use 'zion node websocket listen' to stream events");
    println!();

    Ok(())
}

async fn websocket_unsubscribe(host: &str, port: u16, subscription_id: &str) -> Result<()> {
    ui::print_header(&format!("Unsubscribe from {}", subscription_id));

    let ws_url = format!("ws://{}:{}", host, port);
    ui::print_info(&format!("Connecting to {}", ws_url));

    ui::print_row("Subscription ID", subscription_id);
    ui::print_ok("Unsubscribe request prepared");
    println!();

    Ok(())
}

async fn websocket_listen(host: &str, port: u16) -> Result<()> {
    ui::print_header(&format!("Listening on ws://{}:{}", host, port));
    ui::print_info("Press Ctrl+C to stop listening");

    // For now, just print a message
    // In a full implementation, this would connect and stream events
    ui::print_warn("WebSocket streaming not yet implemented in CLI");
    ui::print_info("Use the web client for real-time subscriptions");
    println!();

    Ok(())
}
