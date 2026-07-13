//! Live stats collector — gathers node, miner, pool, wallet status in one shot.
//!
//! Used by the interactive menu dashboard and `zion monitor`.

use std::time::Duration;

use crate::config::Config;
use crate::process;

/// Snapshot of the local ZION stack at a point in time.
pub struct Stats {
    pub node_process: Option<u32>,
    pub pool_process: Option<u32>,
    pub miner_process: Option<u32>,
    pub node_height: Option<u64>,
    pub node_network: Option<String>,
    pub node_peers: Option<u64>,
    pub node_tip: Option<String>,
    pub wallet_address: String,
    pub wallet_balance: Option<f64>,
    pub miner_stats: Option<MinerStats>,
    pub node_rpc_ok: bool,
}

/// Miner stats read from `~/.zion/miner-stats.json` (written by the miner).
#[derive(Debug, Default, serde::Deserialize)]
pub struct MinerStats {
    #[serde(default)]
    pub hashrate_hps: f64,
    #[serde(default)]
    pub accepted_shares: u64,
    #[serde(default)]
    pub rejected_shares: u64,
    #[serde(default)]
    pub last_share_time: Option<String>,
    #[serde(default)]
    pub worker_name: Option<String>,
    #[serde(default)]
    pub algorithm: Option<String>,
    #[serde(default)]
    pub pool_addr: Option<String>,
}

pub async fn collect(cfg: &Config) -> Stats {
    let node_process = process::status("node");
    let pool_process = process::status("pool");
    let miner_process = process::status("miner");

    let client = zion_sdk::node::NodeClient::builder(&cfg.node.rpc_host, cfg.node.rpc_port)
        .connect_timeout(Duration::from_secs(3))
        .request_timeout(Duration::from_secs(5))
        .build();

    let (node_height, node_network, node_tip, node_rpc_ok) =
        match client.chain_info().await {
            Ok(chain) => (
                Some(chain.chain_height),
                Some(chain.network),
                Some(chain.tip_hash_hex),
                true,
            ),
            Err(_) => (None, None, None, false),
        };

    let node_peers = match client.peer_info().await {
        Ok(p) => Some(p.count as u64),
        Err(_) => None,
    };

    let wallet_balance = if !cfg.miner.wallet.is_empty() {
        match client.balance(&cfg.miner.wallet).await {
            Ok(v) => v
                .get("total_zion")
                .and_then(|t| t.as_f64())
                .or_else(|| v.get("total").and_then(|t| t.as_f64())),
            Err(_) => None,
        }
    } else {
        None
    };

    let miner_stats = read_miner_stats();

    Stats {
        node_process,
        pool_process,
        miner_process,
        node_height,
        node_network,
        node_peers,
        node_tip,
        wallet_address: cfg.miner.wallet.clone(),
        wallet_balance,
        miner_stats,
        node_rpc_ok,
    }
}

fn read_miner_stats() -> Option<MinerStats> {
    let path = dirs::home_dir()?.join(".zion").join("miner-stats.json");
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}
