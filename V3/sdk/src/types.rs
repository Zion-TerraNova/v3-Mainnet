//! Typed JSON-RPC responses matching shapes from `zion-core` (`V3/L1/core/src/rpc.rs`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainInfo {
    pub network: String,
    pub consensus_profile: String,
    pub chain_height: u64,
    #[serde(rename = "tip_hash")]
    pub tip_hash_hex: String,
    pub accepted_blocks: u64,
    pub mempool_transactions: u64,
    pub protocol_version: String,
    pub transaction_model: String,
    #[serde(default)]
    pub utxo_validation_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub node_id: String,
    pub protocol_version: String,
    pub network: String,
    pub chain_height: u64,
    pub p2p_bind: String,
    pub rpc_bind: String,
    pub pool_bind: String,
    pub known_peers: usize,
    pub accepted_blocks: u64,
    pub mempool_transactions: u64,
    pub transaction_model: String,
    #[serde(default)]
    pub balance_lookup: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolInfo {
    pub size: u64,
    pub template_transactions: u64,
    pub template_total_fees_zion: u64,
    pub transaction_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerEndpoint {
    pub host: String,
    pub port: u16,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peers: Vec<PeerEndpoint>,
    pub count: usize,
}

/// `getSupplyInfo` response (numbers as strings because of u128 on the wire).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplyInfo {
    pub total_supply_atomic: String,
    pub total_supply_zion: u64,
    pub premine_atomic: String,
    pub premine_zion: u64,
    pub mining_emission_atomic: String,
    pub mining_emission_zion: u64,
    pub mined_so_far_atomic: String,
    pub mined_so_far_zion: u64,
    pub supply_mined_percent: String,
    pub circulating_supply_atomic: String,
    pub circulating_supply_zion: u64,
    pub remaining_supply_atomic: String,
    pub remaining_supply_zion: u64,
    pub block_reward_atomic: u64,
    pub block_reward_zion: f64,
    pub height: u64,
}

/// Successful `submitTransaction` / `sendRawTransaction` / `submitAccountTransaction` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitAccepted {
    pub accepted: bool,
    #[serde(default)]
    pub tx_id: Option<String>,
}

/// RPC parameters for `submitBlock`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitBlockParams {
    pub template_id: u64,
    pub header_hex: String,
    pub nonce: u64,
    pub target_hex: String,
}

/// `submitBlock` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitCandidateResult {
    pub accepted: bool,
    pub template_id: u64,
    pub block_height: Option<u64>,
    pub hash_hex: String,
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supply_info_from_fixture() {
        let j = serde_json::json!({
            "total_supply_atomic": "210000000000000000",
            "total_supply_zion": 21000000,
            "premine_atomic": "0",
            "premine_zion": 0,
            "mining_emission_atomic": "210000000000000000",
            "mining_emission_zion": 21000000,
            "mined_so_far_atomic": "5000000000000",
            "mined_so_far_zion": 50,
            "supply_mined_percent": "0.000024",
            "circulating_supply_atomic": "5000000000000",
            "circulating_supply_zion": 50,
            "remaining_supply_atomic": "209999999950000000",
            "remaining_supply_zion": 20999950,
            "block_reward_atomic": 500000000000u64,
            "block_reward_zion": 5000.0,
            "height": 1
        });
        let s: SupplyInfo = serde_json::from_value(j).expect("decode");
        assert_eq!(s.height, 1);
        assert_eq!(s.mined_so_far_zion, 50);
    }
}
