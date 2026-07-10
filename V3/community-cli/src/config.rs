//! Public CLI configuration — loaded from `~/.zion/zion.toml` or env vars.
//!
//! Only public-facing fields: node RPC, pool, miner, AI endpoint.
//! No deploy, SSH, DAO, bridge, or topology config.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub node: NodeConfig,
    #[serde(default)]
    pub pool: PoolConfig,
    #[serde(default)]
    pub miner: MinerConfig,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub binaries: BinaryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub rpc_host: String,
    pub rpc_port: u16,
    #[serde(default = "default_p2p_bind")]
    pub p2p_bind: String,
    #[serde(default = "default_node_id")]
    pub node_id: String,
    #[serde(default = "default_seed_peers")]
    pub seed_peers: String,
    #[serde(default)]
    pub humanitarian_wallet: String,
    #[serde(default)]
    pub issobella_wallet: String,
}

fn default_p2p_bind() -> String {
    "0.0.0.0:8333".into()
}
fn default_node_id() -> String {
    "zion-public-node".into()
}
fn default_seed_peers() -> String {
    "62.171.141.136:8333".into()
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            rpc_host: "127.0.0.1".to_string(), // when running local node
            rpc_port: 8443,
            p2p_bind: default_p2p_bind(),
            node_id: default_node_id(),
            seed_peers: "62.171.141.136:8333".to_string(),
            humanitarian_wallet: String::new(),
            issobella_wallet: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    pub host: String,
    pub port: u16,
    #[serde(default = "default_pool_bind")]
    pub bind: String,
    #[serde(default)]
    pub wallet: String,
}

fn default_pool_bind() -> String {
    "0.0.0.0:8444".into()
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            host: "pool.zionterranova.com".to_string(), // public Edge pool
            port: 8444,
            bind: default_pool_bind(),
            wallet: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinerConfig {
    pub wallet: String,
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default = "default_worker")]
    pub worker_name: String,
    #[serde(default = "default_auto_node")]
    pub auto_start_node: bool,
    #[serde(default = "default_auto_pool")]
    pub auto_start_pool: bool,
}

fn default_auto_node() -> bool {
    true
}
fn default_auto_pool() -> bool {
    false
}

fn default_algorithm() -> String {
    "deeksha_lite_v1".into()
}
fn default_backend() -> String {
    "cpu".into()
}
fn default_worker() -> String {
    "worker-1".into()
}

impl Default for MinerConfig {
    fn default() -> Self {
        Self {
            wallet: String::new(),
            algorithm: default_algorithm(),
            backend: default_backend(),
            worker_name: default_worker(),
            auto_start_node: default_auto_node(),
            auto_start_pool: default_auto_pool(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// Hiran inference endpoint URL (OpenAI-compatible).
    pub url: String,
    /// Model name for chat completions.
    #[serde(default = "default_ai_model")]
    pub model: String,
}

fn default_ai_model() -> String {
    "hiran-v2.2".into()
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            model: default_ai_model(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BinaryConfig {
    #[serde(default)]
    pub node: Option<String>,
    #[serde(default)]
    pub pool: Option<String>,
    #[serde(default)]
    pub miner: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            node: NodeConfig::default(),
            pool: PoolConfig::default(),
            miner: MinerConfig::default(),
            ai: AiConfig::default(),
            binaries: BinaryConfig::default(),
        }
    }
}

/// Resolve config file path: `~/.zion/zion.toml`.
pub fn config_path() -> Result<PathBuf> {
    let home = dirs_home()?;
    Ok(home.join(".zion").join("zion.toml"))
}

fn dirs_home() -> Result<PathBuf> {
    if let Ok(h) = std::env::var("HOME") {
        return Ok(PathBuf::from(h));
    }
    if let Ok(h) = std::env::var("USERPROFILE") {
        return Ok(PathBuf::from(h));
    }
    anyhow::bail!("cannot determine home directory (HOME / USERPROFILE unset)")
}

/// Load config from the given path, or the default path if `None`.
/// Falls back to `Config::default()` if the file does not exist.
pub fn load(path: Option<&str>) -> Result<Config> {
    let path = match path {
        Some(p) => PathBuf::from(p),
        None => config_path()?,
    };

    if !path.exists() {
        return Ok(Config::default());
    }

    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read config {}", path.display()))?;
    let cfg: Config = toml::from_str(&raw)
        .with_context(|| format!("parse config {}", path.display()))?;
    Ok(cfg)
}

/// Set a single config value by dotted key (e.g. `miner.wallet`).
pub fn set_value(key: &str, value: &str) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut cfg = load(None).unwrap_or_default();

    match key {
        "node.rpc_host" => cfg.node.rpc_host = value.to_string(),
        "node.rpc_port" => cfg.node.rpc_port = value.parse().context("invalid port")?,
        "node.p2p_bind" => cfg.node.p2p_bind = value.to_string(),
        "node.node_id" => cfg.node.node_id = value.to_string(),
        "node.seed_peers" => cfg.node.seed_peers = value.to_string(),
        "pool.host" => cfg.pool.host = value.to_string(),
        "pool.port" => cfg.pool.port = value.parse().context("invalid port")?,
        "pool.bind" => cfg.pool.bind = value.to_string(),
        "miner.wallet" => cfg.miner.wallet = value.to_string(),
        "miner.algorithm" => cfg.miner.algorithm = value.to_string(),
        "miner.backend" => cfg.miner.backend = value.to_string(),
        "miner.worker_name" => cfg.miner.worker_name = value.to_string(),
        "miner.auto_start_node" => cfg.miner.auto_start_node = value.parse().context("invalid bool")?,
        "miner.auto_start_pool" => cfg.miner.auto_start_pool = value.parse().context("invalid bool")?,
        "ai.url" => cfg.ai.url = value.to_string(),
        "ai.model" => cfg.ai.model = value.to_string(),
        "binaries.node" => cfg.binaries.node = Some(value.to_string()),
        "binaries.pool" => cfg.binaries.pool = Some(value.to_string()),
        "binaries.miner" => cfg.binaries.miner = Some(value.to_string()),
        _ => anyhow::bail!("unknown config key: {} (valid: node.rpc_host, node.rpc_port, node.p2p_bind, node.node_id, node.seed_peers, pool.host, pool.port, pool.bind, miner.wallet, miner.algorithm, miner.backend, miner.worker_name, miner.auto_start_node, miner.auto_start_pool, ai.url, ai.model, binaries.node, binaries.pool, binaries.miner)", key),
    }

    let raw = toml::to_string_pretty(&cfg)?;
    std::fs::write(&path, raw)?;
    println!("✓ {} = {} → {}", key, value, path.display());
    Ok(())
}
