use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub struct ValidationReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationReport {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub node: NodeConfig,
    #[serde(default)]
    pub pool: PoolConfig,
    #[serde(default)]
    pub miner: MinerConfig,
    #[serde(default)]
    pub cli: CliConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    #[serde(default = "default_true")]
    pub auto_update_check: bool,
}

fn default_true() -> bool {
    true
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            auto_update_check: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub rpc_host: String,
    pub rpc_port: u16,
    pub p2p_port: u16,
    pub websocket_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinerConfig {
    pub wallet: String,
    pub btc_wallet: String,
    pub threads: String,
    pub backend: String,
    pub profile: String,
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
}

fn default_algorithm() -> String {
    "deeksha_lite_v1".into()
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            rpc_host: "127.0.0.1".into(),
            rpc_port: 8443,
            p2p_port: 8333,
            websocket_port: Some(8445),
        }
    }
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            host: "pool.zionterranova.com".into(),
            port: 8444,
        }
    }
}

impl Default for MinerConfig {
    fn default() -> Self {
        Self {
            wallet: String::new(),
            btc_wallet: String::new(),
            threads: "auto".into(),
            backend: "auto".into(),
            profile: "pool".into(),
            algorithm: default_algorithm(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            node: NodeConfig::default(),
            pool: PoolConfig::default(),
            miner: MinerConfig::default(),
            cli: CliConfig::default(),
        }
    }
}

impl Config {
    /// Resolve the active RPC endpoint.
    pub fn rpc(&self) -> (&str, u16) {
        (&self.node.rpc_host, self.node.rpc_port)
    }

    /// Resolve the active pool endpoint.
    pub fn pool_endpoint(&self) -> (&str, u16) {
        (&self.pool.host, self.pool.port)
    }

    /// Pick the appropriate RPC endpoint based on a target name.
    /// Recognised targets: `local`, `core`, `edge`, or `host:port`.
    pub fn target_rpc<'a>(&'a self, target: &'a str) -> (&'a str, u16) {
        match target.trim().to_ascii_lowercase().as_str() {
            "local" | "core" | "edge" | "mainnet" => self.rpc(),
            _ => {
                if target.contains(':') {
                    let parts: Vec<&'a str> = target.splitn(2, ':').collect();
                    if let Ok(port) = parts[1].parse() {
                        return (parts[0], port);
                    }
                }
                self.rpc()
            }
        }
    }

    /// Pick the appropriate pool endpoint based on a target name.
    pub fn target_pool<'a>(&'a self, target: &'a str) -> (&'a str, u16) {
        match target.trim().to_ascii_lowercase().as_str() {
            "local" | "core" | "edge" | "mainnet" => self.pool_endpoint(),
            _ => {
                if target.contains(':') {
                    let parts: Vec<&'a str> = target.splitn(2, ':').collect();
                    if let Ok(port) = parts[1].parse() {
                        return (parts[0], port);
                    }
                }
                self.pool_endpoint()
            }
        }
    }
}

pub fn config_path() -> Result<PathBuf> {
    let home = dirs_next().context("Cannot determine home directory")?;
    Ok(home.join(".zion").join("zion.toml"))
}

pub fn load(override_path: Option<&str>) -> Result<Config> {
    let path = match override_path {
        Some(p) => PathBuf::from(p),
        None => config_path()?,
    };

    if !path.exists() {
        return Ok(Config::default());
    }

    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("Cannot read config: {}", path.display()))?;
    let cfg: Config =
        toml::from_str(&text).with_context(|| format!("Invalid config: {}", path.display()))?;
    Ok(cfg)
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(cfg)?;
    std::fs::write(&path, text)?;
    Ok(())
}

pub fn set_value(key: &str, value: &str) -> Result<()> {
    let mut cfg = load(None)?;
    let parts: Vec<&str> = key.splitn(2, '.').collect();
    match parts.as_slice() {
        ["node", "rpc_host"] => cfg.node.rpc_host = value.into(),
        ["node", "rpc_port"] => cfg.node.rpc_port = value.parse()?,
        ["node", "p2p_port"] => cfg.node.p2p_port = value.parse()?,
        ["node", "websocket_port"] => cfg.node.websocket_port = Some(value.parse()?),
        ["pool", "host"] => cfg.pool.host = value.into(),
        ["pool", "port"] => cfg.pool.port = value.parse()?,
        ["miner", "wallet"] => cfg.miner.wallet = value.into(),
        ["miner", "btc_wallet"] => cfg.miner.btc_wallet = value.into(),
        ["miner", "threads"] => cfg.miner.threads = value.into(),
        ["miner", "backend"] => cfg.miner.backend = value.into(),
        ["miner", "profile"] => cfg.miner.profile = value.into(),
        ["miner", "algorithm"] => cfg.miner.algorithm = value.into(),
        ["cli", "auto_update_check"] => cfg.cli.auto_update_check = value.parse()?,
        _ => anyhow::bail!(
            "Unknown config key: {}. Valid keys: node.rpc_host, node.rpc_port, node.p2p_port, node.websocket_port, pool.host, pool.port, miner.wallet, miner.btc_wallet, miner.threads, miner.backend, miner.profile, miner.algorithm, cli.auto_update_check",
            key
        ),
    }
    save(&cfg)?;
    println!("✓ {} = {}", key, value);
    Ok(())
}

fn dirs_next() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

pub fn expand_path(p: &str) -> String {
    if p.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}{}", home, &p[1..]);
        }
    }
    p.to_string()
}

pub fn validate(cfg: &Config) -> ValidationReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if cfg.node.rpc_host.trim().is_empty() {
        errors.push("node.rpc_host must not be empty".to_string());
    }
    if cfg.node.rpc_port == 0 {
        errors.push("node.rpc_port must be greater than 0".to_string());
    }
    if cfg.node.p2p_port == 0 {
        errors.push("node.p2p_port must be greater than 0".to_string());
    }
    if cfg.pool.host.trim().is_empty() {
        errors.push("pool.host must not be empty".to_string());
    }
    if cfg.pool.port == 0 {
        errors.push("pool.port must be greater than 0".to_string());
    }

    match cfg.miner.backend.trim().to_ascii_lowercase().as_str() {
        "auto" | "cpu" | "gpu" | "metal" | "opencl" | "ocl" | "cuda" => {}
        other => errors.push(format!(
            "miner.backend has unsupported value '{}'. Supported: auto, cpu, gpu, metal, opencl, cuda",
            other
        )),
    }

    match cfg.miner.profile.trim().to_ascii_lowercase().as_str() {
        "pool" | "solo" | "benchmark" | "bench" | "dual" => {}
        other => errors.push(format!(
            "miner.profile has unsupported value '{}'. Supported: pool, solo, benchmark, dual",
            other
        )),
    }

    match cfg.miner.algorithm.trim().to_ascii_lowercase().as_str() {
        "deeksha_lite_v1" | "lite" | "dl" | "dlv1"
        | "deeksha_lite_fire" | "fire" | "dlfire"
        | "cosmic_harmony_ekam_deeksha_v2" | "ekam" | "ekam_v2" | "full" => {}
        other => errors.push(format!(
            "miner.algorithm has unsupported value '{}'. Supported: deeksha_lite_v1, deeksha_lite_fire, cosmic_harmony_ekam_deeksha_v2",
            other
        )),
    }

    if cfg.miner.profile.trim().eq_ignore_ascii_case("dual")
        && cfg.miner.btc_wallet.trim().is_empty()
    {
        warnings.push("miner.profile is dual but miner.btc_wallet is empty; DCR sidecar will rely on env or fallback BTC payout wallet".to_string());
    }

    ValidationReport { errors, warnings }
}
