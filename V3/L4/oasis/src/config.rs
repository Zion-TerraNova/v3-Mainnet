//! OASIS Configuration — game world settings and defaults.

use serde::{Deserialize, Serialize};

/// OASIS world configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OasisConfig {
    /// API server port
    pub port: u16,
    /// API bind address
    pub bind: String,

    // === XP Settings ===
    /// Daily XP cap per player
    pub daily_xp_cap: u64,
    /// XP for mining a block (base, before multiplier)
    pub xp_per_block_base: u64,
    /// Meditation bonus (% of base XP)
    pub meditation_bonus_pct: u32,

    // === Guild Settings ===
    /// Max guild members
    pub max_guild_size: usize,
    /// Min members for guild quests
    pub min_quest_members: usize,
    /// XP per guild level
    pub guild_xp_per_level: u64,
    /// Max guild level
    pub max_guild_level: u32,

    // === Territory Settings ===
    /// ZION cost to claim a territory
    pub territory_claim_cost: u64,
    /// Mining bonus in controlled territory (%)
    pub territory_mining_bonus_pct: u32,
    /// Max miners per territory
    pub territory_capacity: u32,

    // === Reward Settings ===
    /// Total OASIS reward pool (ZION)
    pub reward_pool_total: u64,
    /// Per-slot allocation (ZION)
    pub reward_slot_allocation: u64,

    // === L1 Connection ===
    /// L1 RPC endpoints for reading blockchain data
    pub l1_rpc_endpoints: Vec<String>,
    /// L3 NCL endpoint for AI challenges
    pub ncl_endpoint: Option<String>,
    /// L3 AI-Native endpoint for AI agents
    pub ai_native_endpoint: Option<String>,

    // === Hiran AI ===
    /// Hiran v2.2 inference endpoint (OpenAI-compatible)
    pub hiran_endpoint: Option<String>,
    /// Enable Hiran AI integration
    pub hiran_enabled: bool,

    // === UE5 Client ===
    /// WebSocket port for real-time UE5 updates
    pub ws_port: u16,
    /// Max concurrent UE5 connections
    pub max_ws_connections: u32,

    // === Metrics ===
    /// Prometheus metrics HTTP port
    pub metrics_port: u16,
}

impl Default for OasisConfig {
    fn default() -> Self {
        Self {
            port: 8094,
            bind: "0.0.0.0".to_string(),

            daily_xp_cap: 10_000,
            xp_per_block_base: 10,
            meditation_bonus_pct: 25,

            max_guild_size: 100,
            min_quest_members: 5,
            guild_xp_per_level: 10_000,
            max_guild_level: 50,

            territory_claim_cost: 10_000,
            territory_mining_bonus_pct: 10,
            territory_capacity: 50,

            reward_pool_total: 8_250_000_000,
            reward_slot_allocation: 1_650_000_000,

            l1_rpc_endpoints: Vec::new(),
            ncl_endpoint: Some("http://localhost:8090".to_string()),
            ai_native_endpoint: Some("http://localhost:8091".to_string()),

            hiran_endpoint: Some("http://localhost:8002".to_string()),
            hiran_enabled: false,

            ws_port: 8095,
            max_ws_connections: 1000,

            metrics_port: 9101,
        }
    }
}

impl OasisConfig {
    /// Load from a TOML file on disk. Falls back to `Self::default()` if the file
    /// does not exist, so a missing config is not a fatal error.
    pub fn load(path: &str) -> Result<Self, String> {
        use std::fs;

        if !std::path::Path::new(path).exists() {
            tracing::info!("OASIS config file not found at {}; using defaults", path);
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read OASIS config at {}: {}", path, e))?;

        let config: OasisConfig = toml::from_str(&contents)
            .map_err(|e| format!("Failed to parse OASIS config at {}: {}", path, e))?;

        tracing::info!("OASIS config loaded from {}", path);
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = OasisConfig::default();
        assert_eq!(config.port, 8094);
        assert_eq!(config.reward_pool_total, 8_250_000_000);
        assert!(config.l1_rpc_endpoints.is_empty());
    }

    #[test]
    fn test_load_from_toml() {
        use std::io::Write;

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        let toml = r#"
port = 9099
bind = "127.0.0.1"
daily_xp_cap = 5000
xp_per_block_base = 5
meditation_bonus_pct = 10
max_guild_size = 50
min_quest_members = 3
guild_xp_per_level = 5000
max_guild_level = 25
territory_claim_cost = 5000
territory_mining_bonus_pct = 5
territory_capacity = 25
reward_pool_total = 1000000
reward_slot_allocation = 200000
l1_rpc_endpoints = ["http://localhost:8443"]
ncl_endpoint = "http://localhost:8090"
ai_native_endpoint = "http://localhost:8091"
hiran_endpoint = "http://localhost:8002"
hiran_enabled = true
ws_port = 9095
max_ws_connections = 500
metrics_port = 9102
"#;
        tmp.write_all(toml.as_bytes()).unwrap();

        let config = OasisConfig::load(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(config.port, 9099);
        assert_eq!(config.bind, "127.0.0.1");
        assert_eq!(config.daily_xp_cap, 5000);
        assert_eq!(config.max_guild_size, 50);
        assert_eq!(config.territory_claim_cost, 5000);
        assert_eq!(config.reward_pool_total, 1_000_000);
        assert_eq!(config.l1_rpc_endpoints, vec!["http://localhost:8443"]);
        assert!(config.hiran_enabled);
        assert_eq!(config.metrics_port, 9102);
    }

    #[test]
    fn test_load_missing_file_uses_defaults() {
        let config = OasisConfig::load("/nonexistent/path/oasis.toml").unwrap();
        assert_eq!(config.port, 8094);
        assert_eq!(config.reward_pool_total, 8_250_000_000);
    }
}
