//! DAO Configuration
//!
//! Priority: TOML file → env var overrides → built-in defaults
//!
//! ```sh
//! # Load from TOML:
//! DAO_CONFIG=/etc/zion/dao.toml cargo run --bin zion-dao
//!
//! # Override individual keys without editing the file:
//! DAO_API_KEY=secret cargo run --bin zion-dao
//! ```

use std::env;
use std::path::Path;

use serde::{Deserialize, Serialize};

// ─── Top-level config ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoConfig {
    // ── Identity ──────────────────────────────────────────────────────────
    pub name: String,
    pub version: String,

    // ── HTTP server ───────────────────────────────────────────────────────
    /// Port for the HTTP API (default 8080). Env: DAO_API_PORT
    pub api_port: u16,
    /// API key for write endpoints (empty = disabled). Env: ZION_DAO_API_KEY
    pub api_key: String,

    // ── Database ──────────────────────────────────────────────────────────
    /// SQLite DB file path. Env: DAO_DB_PATH
    pub db_path: String,

    // ── L1 connection ─────────────────────────────────────────────────────
    pub l1_rpc_url: String,
    pub l1_rpc_backup: Option<String>,

    // ── L1 scanner ────────────────────────────────────────────────────────
    /// How often to poll L1 for new blocks (seconds, default 10)
    pub scan_interval_secs: u64,
    /// Minimum vote weight to register a vote (atomic)
    pub min_vote_weight: u64,
    /// Blocks to wait before treating a TX as final
    pub finality_blocks: u64,

    // ── Governance ────────────────────────────────────────────────────────
    pub proposal_threshold: u64,
    pub quorum_percent: f64,
    pub voting_period_days: u32,
    pub timelock_hours: u32,

    // ── Treasury ──────────────────────────────────────────────────────────
    pub treasury_addresses: Vec<String>,
    pub daily_spend_limit: u64,
    pub multisig_threshold: u32,
    pub multisig_total: u32,

    // ── Guardians ─────────────────────────────────────────────────────────
    #[serde(default)]
    pub guardians: Vec<GuardianConfig>,

    // ── Multi-Layer Co-Admin ────────────────────────────────────────────
    /// Co-Admins by layer (1–6). Each layer has its own Co-Admin set.
    #[serde(default)]
    pub co_admins: Vec<CoAdminConfig>,
    /// Enable cross-layer veto for proposals affecting multiple layers
    #[serde(default = "default_true")]
    pub cross_layer_veto_enabled: bool,
    /// Minimum layers that must consent for cross-layer proposals
    #[serde(default = "default_cross_layer_threshold")]
    pub cross_layer_consent_threshold: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianConfig {
    pub name: String,
    pub address: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoAdminConfig {
    pub layer: u8,    // 1–6
    pub role: String, // e.g. "validator", "guardian", "relayer", "curator", "community", "steward"
    pub name: String,
    pub address: String,
    pub public_key: String,
    pub bonded_amount: u64,
    pub reputation: u64,
    pub term_start: String, // ISO 8601
    pub term_end: Option<String>,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

fn default_true() -> bool {
    true
}

fn default_cross_layer_threshold() -> u8 {
    2 // minimum 2 layers must consent for cross-layer proposals
}

// ─── Defaults ─────────────────────────────────────────────────────────────────

impl Default for DaoConfig {
    fn default() -> Self {
        Self {
            name: "ZION DAO".into(),
            version: "1.0.0".into(),
            api_port: 8080,
            api_key: String::new(),
            db_path: "data/dao.db".into(),
            l1_rpc_url: "127.0.0.1:8443".into(),
            l1_rpc_backup: Some("127.0.0.1:8443".into()),
            scan_interval_secs: 10,
            min_vote_weight: 1_000_000, // 1 ZION in flowers (6-decimal)
            finality_blocks: 6,
            proposal_threshold: 1_000_000_000_000, // 1M ZION in flowers (1M × 1e6 = 1e12)
            quorum_percent: 10.0,
            voting_period_days: 7,
            timelock_hours: 48,
            treasury_addresses: vec![
                "zion1u5u7k43240d5l4d0x7q5m3c4a838z4k000cv3q0".into(), // Community Governance (main) — 2.5B ZION
                "zion1m8d235x268h8d887s036m8c3x7s356d3r37k6m6".into(), // Grants & Bounties — 1.0B ZION
                "zion102s8k4k0w783d657j255z865e47054s342u87v3".into(), // Ecosystem Bootstrap — 0.5B ZION
            ],
            daily_spend_limit: 100_000_000, // 100M ZION (whole coins, not flowers)
            multisig_threshold: 5,
            multisig_total: 7,
            guardians: vec![],
            co_admins: vec![],
            cross_layer_veto_enabled: true,
            cross_layer_consent_threshold: 2,
        }
    }
}

// ─── Loading ──────────────────────────────────────────────────────────────────

impl DaoConfig {
    /// Load config with three-level priority:
    /// 1. Built-in defaults
    /// 2. TOML file (`DAO_CONFIG` env var, or explicit `file_path`)
    /// 3. Individual env vars (highest priority)
    pub fn load(file_path: Option<&str>) -> Self {
        let mut cfg = Self::default();

        // Level 2: TOML file
        let toml_path = file_path
            .map(|s| s.to_string())
            .or_else(|| env::var("DAO_CONFIG").ok());

        if let Some(ref path) = toml_path {
            match Self::from_toml_file(path) {
                Ok(file_cfg) => {
                    tracing::info!("Config loaded from {}", path);
                    cfg = file_cfg;
                }
                Err(e) => {
                    tracing::warn!("Could not load DAO_CONFIG={}: {} — using defaults", path, e);
                }
            }
        }

        // Level 3: env var overrides (can patch individual keys without editing TOML)
        if let Ok(v) = env::var("DAO_API_PORT") {
            if let Ok(p) = v.parse::<u16>() {
                cfg.api_port = p;
            }
        }
        if let Ok(v) = env::var("ZION_DAO_API_KEY") {
            cfg.api_key = v;
        }
        if let Ok(v) = env::var("DAO_DB_PATH") {
            cfg.db_path = v;
        }
        if let Ok(v) = env::var("DAO_L1_RPC") {
            cfg.l1_rpc_url = v;
        }

        cfg
    }

    /// Parse a TOML file into DaoConfig (uses `serde` + `toml`).
    fn from_toml_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        if !Path::new(path).exists() {
            return Err(format!("file not found: {}", path).into());
        }
        let raw = std::fs::read_to_string(path)?;
        let cfg: Self = toml::from_str(&raw)?;
        Ok(cfg)
    }

    /// Serialize current config to TOML string (for diagnostics / `GET /api/config`).
    pub fn to_toml_string(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_else(|_| "# serialization error".into())
    }
}
