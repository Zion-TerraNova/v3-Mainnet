//! Configuration for zion-free-world daemon.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FreeWorldConfig {
    pub name: String,
    pub bind: String,
    pub port: u16,
    pub db_path: String,
    pub l1_rpc_url: String,
    pub scan_interval_secs: u64,
    pub api_key: String,
    pub humanitarian_fund_address: String,
    pub min_grant_amount_zion: u64,
    pub max_grant_amount_zion: u64,
    pub hiran_endpoint: Option<String>,
    pub hiran_enabled: bool,
}

impl Default for FreeWorldConfig {
    fn default() -> Self {
        Self {
            name: "zion-free-world".to_string(),
            bind: "0.0.0.0".to_string(),
            port: 8095,
            db_path: "./free_world.db".to_string(),
            l1_rpc_url: "http://127.0.0.1:8443/jsonrpc".to_string(),
            scan_interval_secs: 60,
            api_key: std::env::var("FREE_WORLD_API_KEY").unwrap_or_default(),
            humanitarian_fund_address: "zion1humanitarian0000000000000000000000".to_string(),
            min_grant_amount_zion: 1_000,
            max_grant_amount_zion: 10_000_000,
            hiran_endpoint: Some("http://localhost:8002".to_string()),
            hiran_enabled: false,
        }
    }
}

impl FreeWorldConfig {
    pub fn load(path: Option<&str>) -> Self {
        let mut cfg = Self::default();

        if let Ok(port) = std::env::var("FREE_WORLD_PORT") {
            cfg.port = port.parse().unwrap_or(cfg.port);
        }
        if let Ok(bind) = std::env::var("FREE_WORLD_BIND") {
            cfg.bind = bind;
        }
        if let Ok(db) = std::env::var("FREE_WORLD_DB") {
            cfg.db_path = db;
        }
        if let Ok(rpc) = std::env::var("FREE_WORLD_L1_RPC") {
            cfg.l1_rpc_url = rpc;
        }
        if let Ok(key) = std::env::var("FREE_WORLD_API_KEY") {
            cfg.api_key = key;
        }
        if let Ok(url) = std::env::var("FREE_WORLD_HIRAN_URL") {
            cfg.hiran_endpoint = Some(url);
        }
        if let Ok(enabled) = std::env::var("FREE_WORLD_HIRAN_ENABLED") {
            cfg.hiran_enabled = enabled.eq_ignore_ascii_case("true");
        }

        if let Some(p) = path {
            if let Ok(text) = std::fs::read_to_string(p) {
                if let Ok(loaded) = toml::from_str::<FreeWorldConfig>(&text) {
                    cfg = loaded;
                }
            }
        }

        cfg
    }
}
