//! Bridge configuration — L1 RPC, EVM chains, validator keys, thresholds.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level bridge configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    /// Bridge identity
    pub bridge: BridgeIdentity,

    /// ZION L1 connection
    pub l1: L1Config,

    /// Ankr multi-chain RPC configuration.
    ///
    /// When `ankr.enabled = true`, EVM chain connections are made through
    /// Ankr's unified endpoint (`https://rpc.ankr.com/{chain}`), eliminating
    /// the need for per-chain WebSocket URLs.
    #[serde(default)]
    pub ankr: AnkrConfig,

    /// EVM chain connections (can bridge to multiple chains)
    pub evm_chains: Vec<EvmChainConfig>,

    /// Validator / multisig settings
    pub validator: ValidatorConfig,

    /// Security settings
    pub security: SecurityConfig,

    /// Database settings
    pub database: DatabaseConfig,

    /// Monitoring
    pub metrics: MetricsConfig,
}

/// Ankr multi-chain RPC settings.
///
/// Ankr provides HTTP JSON-RPC for all major EVM chains under a single API key:
/// `https://rpc.ankr.com/{chain}/{api_key}`
///
/// Advantages over per-chain WebSocket config:
/// - One API key handles Base, Arbitrum, BSC, Polygon, etc.
/// - HTTP polling (no WebSocket reconnect logic needed)
/// - No per-chain RPC URL configuration
/// - Free tier available for development / testnet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnkrConfig {
    /// Enable Ankr as the EVM RPC backend.
    ///
    /// When `true`, `rpc_url` in each `EvmChainConfig` is ignored;
    /// the URL is auto-derived from `https://rpc.ankr.com/{chain_id}/{api_key}`.
    pub enabled: bool,

    /// Optional Ankr API key (premium tier).
    ///
    /// If not set, uses the free public endpoint (rate-limited).
    /// Can also be provided via the `ANKR_API_KEY` environment variable.
    pub api_key: Option<String>,
}

impl Default for AnkrConfig {
    fn default() -> Self {
        Self {
            enabled: true, // Ankr is the preferred default
            api_key: None, // free tier
        }
    }
}

impl AnkrConfig {
    /// Resolve the effective API key: config file first, then `ANKR_API_KEY` env var.
    pub fn effective_api_key(&self) -> Option<String> {
        self.api_key
            .clone()
            .or_else(|| std::env::var("ANKR_API_KEY").ok().filter(|k| !k.is_empty()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeIdentity {
    /// Human-readable bridge name
    pub name: String,

    /// Bridge version
    pub version: String,

    /// Network (testnet / mainnet)
    pub network: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L1Config {
    /// ZION L1 raw TCP JSON-RPC address (e.g., "127.0.0.1:8443")
    pub rpc_url: String,

    /// Optional backup L1 raw TCP JSON-RPC address
    pub rpc_url_backup: Option<String>,

    /// Bridge lock address on L1 (ZION locked here → wZION minted)
    pub bridge_address: String,

    /// L1 finality requirement (blocks to wait before processing lock)
    pub finality_blocks: u64,

    /// Poll interval for new blocks (seconds)
    pub poll_interval_secs: u64,

    /// Last processed L1 block height (persisted in DB)
    pub start_block_height: Option<u64>,

    /// Reserved for future authenticated bridge RPC gateways.
    pub l1_rpc_token: Option<String>,

    /// Default EVM recipient address used when a lock UTXO has no memo.
    /// Enables recovery of locks sent without `BRIDGE:chain:0xaddr` memo.
    /// When set, locks without memo mint wZION to this address on "base".
    #[serde(default)]
    pub default_evm_recipient: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmChainConfig {
    /// Chain identifier (e.g., "base", "arbitrum", "bsc", "polygon")
    pub chain_id: String,

    /// Human-readable chain name
    pub name: String,

    /// EVM chain ID (8453 = Base, 42161 = Arbitrum, 56 = BSC, 137 = Polygon)
    pub evm_chain_id: u64,

    /// EVM RPC URL override.
    ///
    /// When `BridgeConfig.ankr.enabled = true`, this field is **optional** —
    /// the Ankr endpoint for this chain is auto-derived from `chain_id`.
    ///
    /// Set this to a custom URL to override Ankr (e.g., a private node).
    #[serde(default)]
    pub rpc_url: Option<String>,

    /// Backup RPC URL (used if both Ankr and primary `rpc_url` fail).
    #[serde(default)]
    pub rpc_url_backup: Option<String>,

    /// Deployed wZION contract address
    pub wzion_address: String,

    /// Deployed ZIONBridge contract address
    pub bridge_contract_address: String,

    /// EVM finality blocks (varies by chain)
    pub finality_blocks: u64,

    /// Whether this chain is active
    pub enabled: bool,

    /// Gas price strategy: "legacy", "eip1559"
    pub gas_strategy: String,

    /// Max gas price (in gwei) — safety limit
    pub max_gas_gwei: u64,

    /// Starting EVM block for event scanning (avoids scanning from genesis).
    /// Set to approximate block number at bridge deployment time.
    #[serde(default)]
    pub start_block: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorConfig {
    /// This node's validator private key file (for signing EVM TX)
    pub private_key_file: PathBuf,

    /// This node's validator identifier (for L1 RPC requests)
    #[serde(default)]
    pub validator_id: String,

    /// Required confirmations (e.g., 3 out of 5)
    pub threshold: u8,

    /// Total validator count
    pub total_validators: u8,

    /// List of all validator EVM addresses (for verification)
    pub validator_addresses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Maximum amount per single bridge operation (wZION, 18 decimals)
    pub max_single_amount: String,

    /// Daily throughput limit (wZION, 18 decimals)
    pub daily_limit: String,

    /// Minimum bridge amount (anti-dust)
    pub min_bridge_amount: String,

    /// Timelock threshold (amounts above this get 24h delay)
    pub timelock_threshold: String,

    /// Rate limit: max bridge operations per hour
    pub max_ops_per_hour: u32,

    /// Watchdog: alert if no L1 blocks for N seconds
    pub l1_block_timeout_secs: u64,

    /// Auto-pause bridge if anomaly detected
    pub auto_pause_on_anomaly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// SQLite database path for bridge state
    pub path: PathBuf,

    /// Backup interval (seconds)
    pub backup_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Enable Prometheus metrics endpoint
    pub enabled: bool,

    /// Metrics HTTP port
    pub port: u16,

    /// Log level (trace, debug, info, warn, error)
    pub log_level: String,
}

impl BridgeConfig {
    /// Load configuration from a TOML file.
    /// The validator key file can be overridden via `ZION_BRIDGE_VALIDATOR_KEY_FILE`.
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut config: BridgeConfig = toml::from_str(&content)?;
        if let Ok(key_file) = std::env::var("ZION_BRIDGE_VALIDATOR_KEY_FILE") {
            if !key_file.trim().is_empty() {
                config.validator.private_key_file = std::path::PathBuf::from(key_file);
            }
        }
        Ok(config)
    }

    /// Get active EVM chains.
    pub fn active_chains(&self) -> Vec<&EvmChainConfig> {
        self.evm_chains.iter().filter(|c| c.enabled).collect()
    }

    /// Validate runtime safety constraints before starting bridge workers.
    pub fn validate_runtime(&self) -> anyhow::Result<()> {
        if self.validator.threshold < 2 {
            anyhow::bail!("validator.threshold must be at least 2");
        }
        if self.validator.threshold > self.validator.total_validators {
            anyhow::bail!(
                "validator.threshold ({}) exceeds validator.total_validators ({})",
                self.validator.threshold,
                self.validator.total_validators
            );
        }

        let mainnet = self.bridge.network.eq_ignore_ascii_case("mainnet");
        if mainnet {
            if self.l1.rpc_url.trim().is_empty() {
                anyhow::bail!("mainnet requires non-empty l1.rpc_url");
            }
            if self.validator.validator_addresses.len() < usize::from(self.validator.threshold) {
                anyhow::bail!(
                    "mainnet requires validator_addresses >= threshold ({} < {})",
                    self.validator.validator_addresses.len(),
                    self.validator.threshold
                );
            }
            // H3 security patch: mainnet must use 5-of-5. An operator
            // must not be able to silently lower the threshold in config.
            if self.validator.total_validators != 5 || self.validator.threshold != 5 {
                anyhow::bail!(
                    "mainnet requires total_validators=5 AND threshold=5 (got {}-of-{}); \
                     lowering the multisig threshold is a governance decision",
                    self.validator.threshold,
                    self.validator.total_validators
                );
            }
            // L1 fail-fast: parse security limits at startup so a malformed
            // config cannot silently disable limits at runtime.
            self.parse_security_limits()?;
        }

        for chain in self.active_chains() {
            if mainnet {
                if chain
                    .wzion_address
                    .eq_ignore_ascii_case("0x0000000000000000000000000000000000000000")
                {
                    anyhow::bail!(
                        "mainnet chain '{}' has zero wzion_address; deploy contract first",
                        chain.chain_id
                    );
                }
                if chain
                    .bridge_contract_address
                    .eq_ignore_ascii_case("0x0000000000000000000000000000000000000000")
                {
                    anyhow::bail!(
                        "mainnet chain '{}' has zero bridge_contract_address; deploy contract first",
                        chain.chain_id
                    );
                }
                if chain.start_block.is_none() {
                    anyhow::bail!(
                        "mainnet chain '{}' must set start_block to deployment height",
                        chain.chain_id
                    );
                }
            }
        }

        Ok(())
    }

    /// Parse the security limit strings once at startup so that a malformed
    /// config fails fast instead of falling back to unsafe defaults at
    /// runtime (L1 audit finding). Returns the parsed limits for the relayer
    /// to reuse without re-parsing.
    fn parse_security_limits(&self) -> anyhow::Result<()> {
        for field in [
            (
                "max_single_amount",
                self.security.max_single_amount.as_str(),
            ),
            ("daily_limit", self.security.daily_limit.as_str()),
            (
                "min_bridge_amount",
                self.security.min_bridge_amount.as_str(),
            ),
            (
                "timelock_threshold",
                self.security.timelock_threshold.as_str(),
            ),
        ] {
            if field.1.parse::<u128>().is_err() {
                anyhow::bail!(
                    "security.{} = {:?} is not a valid u128 integer — fix config before mainnet",
                    field.0,
                    field.1
                );
            }
        }
        Ok(())
    }
}

impl EvmChainConfig {
    /// Resolve the effective RPC URL for this chain.
    ///
    /// Priority:
    /// 1. `self.rpc_url` — if explicitly set
    /// 2. Ankr URL derived from `chain_id` + optional `api_key`
    pub fn effective_rpc_url(&self, ankr: &AnkrConfig) -> String {
        if let Some(url) = &self.rpc_url {
            return url.clone();
        }
        // Build Ankr URL
        let key = ankr.effective_api_key();
        match key {
            Some(k) => format!("https://rpc.ankr.com/{}/{}", self.chain_id, k),
            None => format!("https://rpc.ankr.com/{}", self.chain_id),
        }
    }
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            bridge: BridgeIdentity {
                name: "ZION Bridge Relay".into(),
                version: "0.1.0".into(),
                network: "testnet".into(),
            },
            l1: L1Config {
                rpc_url: "127.0.0.1:8443".into(),
                rpc_url_backup: None,
                bridge_address: "zion1j53677g5k83030x3s2z2z644e7h07792q0u02t7".into(),
                finality_blocks: 60,
                poll_interval_secs: 15,
                start_block_height: None,
                l1_rpc_token: None,
                default_evm_recipient: None,
            },
            ankr: AnkrConfig::default(),
            evm_chains: vec![],
            validator: ValidatorConfig {
                private_key_file: PathBuf::from("keys/validator.key"),
                validator_id: "validator-1".into(),
                threshold: 3,
                total_validators: 5,
                validator_addresses: vec![],
            },
            security: SecurityConfig {
                max_single_amount: "5000000000000000000000000".into(), // 5M wZION
                daily_limit: "10000000000000000000000000".into(),      // 10M wZION
                min_bridge_amount: "100000000000000000000".into(),     // 100 wZION
                timelock_threshold: "1000000000000000000000000".into(), // 1M wZION
                max_ops_per_hour: 100,
                l1_block_timeout_secs: 300,
                auto_pause_on_anomaly: true,
            },
            database: DatabaseConfig {
                path: PathBuf::from("data/bridge.db"),
                backup_interval_secs: 3600,
            },
            metrics: MetricsConfig {
                enabled: true,
                port: 9100,
                log_level: "info".into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mutex to serialize tests that mutate the process environment.
    /// Without this, parallel tests racing on ANKR_API_KEY cause flaky failures.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_default_config() {
        let cfg = BridgeConfig::default();
        assert_eq!(cfg.bridge.name, "ZION Bridge Relay");
        assert_eq!(cfg.bridge.network, "testnet");
        assert_eq!(cfg.l1.rpc_url, "127.0.0.1:8443");
        assert_eq!(
            cfg.l1.bridge_address,
            "zion1j53677g5k83030x3s2z2z644e7h07792q0u02t7"
        );
        assert_eq!(cfg.l1.finality_blocks, 60);
        assert_eq!(cfg.l1.poll_interval_secs, 15);
        assert_eq!(cfg.validator.threshold, 3);
        assert_eq!(cfg.validator.total_validators, 5);
        assert!(cfg.evm_chains.is_empty());
        assert!(cfg.security.auto_pause_on_anomaly);
        assert_eq!(cfg.metrics.port, 9100);
    }

    #[test]
    fn test_active_chains_empty() {
        let cfg = BridgeConfig::default();
        assert_eq!(cfg.active_chains().len(), 0);
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn test_active_chains_filter() {
        let mut cfg = BridgeConfig::default();
        cfg.evm_chains = vec![
            EvmChainConfig {
                chain_id: "base".into(),
                name: "Base".into(),
                evm_chain_id: 8453,
                rpc_url: None, // auto-derived from Ankr
                rpc_url_backup: None,
                wzion_address: "0xWZION".into(),
                bridge_contract_address: "0xBRIDGE".into(),
                finality_blocks: 15,
                enabled: true,
                gas_strategy: "eip1559".into(),
                max_gas_gwei: 50,
                start_block: None,
            },
            EvmChainConfig {
                chain_id: "arbitrum".into(),
                name: "Arbitrum".into(),
                evm_chain_id: 42161,
                rpc_url: None, // auto-derived from Ankr
                rpc_url_backup: None,
                wzion_address: "0xWZION_ARB".into(),
                bridge_contract_address: "0xBRIDGE_ARB".into(),
                finality_blocks: 12,
                enabled: false, // disabled
                gas_strategy: "eip1559".into(),
                max_gas_gwei: 30,
                start_block: None,
            },
        ];

        let active = cfg.active_chains();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].chain_id, "base");
    }

    #[test]
    fn test_config_load_from_toml() {
        let toml_str = r#"
[bridge]
name = "Test Bridge"
version = "0.1.0"
network = "testnet"

[l1]
rpc_url = "127.0.0.1:8443"
bridge_address = "zion1j53677g5k83030x3s2z2z644e7h07792q0u02t7"
finality_blocks = 60
poll_interval_secs = 15

[[evm_chains]]
chain_id = "base"
name = "Base Sepolia"
evm_chain_id = 84532
# rpc_url = "wss://base-sepolia.rpc"  # optional: leave unset to use Ankr auto-URL
wzion_address = "0xWZION_TEST"
bridge_contract_address = "0xBRIDGE_TEST"
finality_blocks = 15
enabled = true
gas_strategy = "eip1559"
max_gas_gwei = 50

[validator]
private_key_file = "keys/test.key"
threshold = 3
total_validators = 5
validator_addresses = ["0xAAA", "0xBBB", "0xCCC", "0xDDD", "0xEEE"]

[security]
max_single_amount = "5000000000000000000000000"
daily_limit = "10000000000000000000000000"
min_bridge_amount = "100000000000000000000"
timelock_threshold = "1000000000000000000000000"
max_ops_per_hour = 100
l1_block_timeout_secs = 300
auto_pause_on_anomaly = true

[database]
path = "data/bridge.db"
backup_interval_secs = 3600

[metrics]
enabled = true
port = 9100
log_level = "info"
"#;
        let config: BridgeConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.bridge.name, "Test Bridge");
        assert_eq!(config.l1.rpc_url, "127.0.0.1:8443");
        assert_eq!(config.evm_chains.len(), 1);
        assert_eq!(config.evm_chains[0].chain_id, "base");
        assert_eq!(config.evm_chains[0].evm_chain_id, 84532);
        assert_eq!(config.validator.threshold, 3);
        assert_eq!(config.validator.validator_addresses.len(), 5);
        assert!(config.security.auto_pause_on_anomaly);
    }

    // ── AnkrConfig tests ────────────────────────────────────────────────────

    #[test]
    fn test_ankr_config_default() {
        let ankr = AnkrConfig::default();
        assert!(ankr.enabled, "Ankr should be enabled by default");
        assert!(ankr.api_key.is_none(), "Default should have no API key");
    }

    #[test]
    fn test_ankr_effective_api_key_from_config() {
        let ankr = AnkrConfig {
            enabled: true,
            api_key: Some("config_key".into()),
        };
        assert_eq!(ankr.effective_api_key(), Some("config_key".into()));
    }

    #[test]
    fn test_ankr_effective_api_key_from_env() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("ANKR_API_KEY", "env_key_test");
        let ankr = AnkrConfig {
            enabled: true,
            api_key: None,
        };
        assert_eq!(ankr.effective_api_key(), Some("env_key_test".into()));
        std::env::remove_var("ANKR_API_KEY");
    }

    #[test]
    fn test_ankr_effective_api_key_config_takes_priority_over_env() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("ANKR_API_KEY", "env_key");
        let ankr = AnkrConfig {
            enabled: true,
            api_key: Some("config_key".into()),
        };
        // Config key takes priority
        assert_eq!(ankr.effective_api_key(), Some("config_key".into()));
        std::env::remove_var("ANKR_API_KEY");
    }

    // ── effective_rpc_url tests ─────────────────────────────────────────────

    fn make_chain_cfg(chain_id: &str, rpc_url: Option<&str>) -> EvmChainConfig {
        EvmChainConfig {
            chain_id: chain_id.into(),
            name: chain_id.into(),
            evm_chain_id: 1,
            rpc_url: rpc_url.map(|s| s.into()),
            rpc_url_backup: None,
            wzion_address: "0xWZION".into(),
            bridge_contract_address: "0xBRIDGE".into(),
            finality_blocks: 12,
            enabled: true,
            gas_strategy: "eip1559".into(),
            max_gas_gwei: 100,
            start_block: None,
        }
    }

    #[test]
    fn test_effective_rpc_url_no_key_no_override() {
        let chain = make_chain_cfg("base", None);
        let ankr = AnkrConfig {
            enabled: true,
            api_key: None,
        };
        assert_eq!(chain.effective_rpc_url(&ankr), "https://rpc.ankr.com/base");
    }

    #[test]
    fn test_effective_rpc_url_with_api_key() {
        let chain = make_chain_cfg("arbitrum", None);
        let ankr = AnkrConfig {
            enabled: true,
            api_key: Some("mykey123".into()),
        };
        assert_eq!(
            chain.effective_rpc_url(&ankr),
            "https://rpc.ankr.com/arbitrum/mykey123"
        );
    }

    #[test]
    fn test_effective_rpc_url_explicit_override() {
        let chain = make_chain_cfg("base", Some("wss://my-private-node.example.com"));
        let ankr = AnkrConfig {
            enabled: true,
            api_key: Some("should_be_ignored".into()),
        };
        // Explicit rpc_url overrides Ankr
        assert_eq!(
            chain.effective_rpc_url(&ankr),
            "wss://my-private-node.example.com"
        );
    }

    #[test]
    fn test_default_config_has_ankr() {
        let cfg = BridgeConfig::default();
        assert!(cfg.ankr.enabled);
        assert!(cfg.ankr.api_key.is_none());
    }

    #[test]
    fn test_security_limits_parsing() {
        let cfg = BridgeConfig::default();
        // Verify we can parse the string amounts
        let daily: u128 = cfg.security.daily_limit.parse().unwrap();
        let min: u128 = cfg.security.min_bridge_amount.parse().unwrap();
        let max_single: u128 = cfg.security.max_single_amount.parse().unwrap();

        assert!(daily > max_single, "Daily limit must exceed single max");
        assert!(max_single > min, "Max single must exceed minimum");
        assert!(min > 0, "Minimum must be positive");
    }

    #[test]
    fn test_validate_runtime_mainnet_rejects_zero_contracts() {
        let mut cfg = BridgeConfig::default();
        cfg.bridge.network = "mainnet".into();
        cfg.validator.threshold = 5;
        cfg.validator.total_validators = 5;
        cfg.validator.validator_addresses = vec![
            "0x1111111111111111111111111111111111111111".into(),
            "0x2222222222222222222222222222222222222222".into(),
            "0x3333333333333333333333333333333333333333".into(),
            "0x4444444444444444444444444444444444444444".into(),
            "0x5555555555555555555555555555555555555555".into(),
        ];
        cfg.evm_chains = vec![EvmChainConfig {
            chain_id: "base".into(),
            name: "Base".into(),
            evm_chain_id: 8453,
            rpc_url: Some("https://mainnet.base.org".into()),
            rpc_url_backup: None,
            wzion_address: "0x0000000000000000000000000000000000000000".into(),
            bridge_contract_address: "0x0000000000000000000000000000000000000000".into(),
            finality_blocks: 64,
            enabled: true,
            gas_strategy: "eip1559".into(),
            max_gas_gwei: 5,
            start_block: Some(1),
        }];

        let err = cfg.validate_runtime().unwrap_err().to_string();
        assert!(err.contains("zero wzion_address"));
    }

    #[test]
    fn test_validate_runtime_mainnet_requires_start_block() {
        let mut cfg = BridgeConfig::default();
        cfg.bridge.network = "mainnet".into();
        cfg.validator.threshold = 5;
        cfg.validator.total_validators = 5;
        cfg.validator.validator_addresses = vec![
            "0x1111111111111111111111111111111111111111".into(),
            "0x2222222222222222222222222222222222222222".into(),
            "0x3333333333333333333333333333333333333333".into(),
            "0x4444444444444444444444444444444444444444".into(),
            "0x5555555555555555555555555555555555555555".into(),
        ];
        cfg.evm_chains = vec![EvmChainConfig {
            chain_id: "base".into(),
            name: "Base".into(),
            evm_chain_id: 8453,
            rpc_url: Some("https://mainnet.base.org".into()),
            rpc_url_backup: None,
            wzion_address: "0x1111111111111111111111111111111111111111".into(),
            bridge_contract_address: "0x2222222222222222222222222222222222222222".into(),
            finality_blocks: 64,
            enabled: true,
            gas_strategy: "eip1559".into(),
            max_gas_gwei: 5,
            start_block: None,
        }];

        let err = cfg.validate_runtime().unwrap_err().to_string();
        assert!(err.contains("must set start_block"));
    }

    #[test]
    fn test_validate_runtime_mainnet_ok_when_guardrails_satisfied() {
        let mut cfg = BridgeConfig::default();
        cfg.bridge.network = "mainnet".into();
        cfg.validator.threshold = 5;
        cfg.validator.total_validators = 5;
        cfg.validator.validator_addresses = vec![
            "0x1111111111111111111111111111111111111111".into(),
            "0x2222222222222222222222222222222222222222".into(),
            "0x3333333333333333333333333333333333333333".into(),
            "0x4444444444444444444444444444444444444444".into(),
            "0x5555555555555555555555555555555555555555".into(),
        ];
        cfg.evm_chains = vec![EvmChainConfig {
            chain_id: "base".into(),
            name: "Base".into(),
            evm_chain_id: 8453,
            rpc_url: Some("https://mainnet.base.org".into()),
            rpc_url_backup: None,
            wzion_address: "0x1111111111111111111111111111111111111111".into(),
            bridge_contract_address: "0x2222222222222222222222222222222222222222".into(),
            finality_blocks: 64,
            enabled: true,
            gas_strategy: "eip1559".into(),
            max_gas_gwei: 5,
            start_block: Some(12_345_678),
        }];

        assert!(cfg.validate_runtime().is_ok());
    }

    #[test]
    fn test_validate_runtime_mainnet_rejects_lowered_threshold() {
        // H3: mainnet must reject threshold < 5 even if everything else is valid.
        let mut cfg = BridgeConfig::default();
        cfg.bridge.network = "mainnet".into();
        cfg.validator.threshold = 3;
        cfg.validator.total_validators = 5;
        cfg.validator.validator_addresses = vec![
            "0x1111111111111111111111111111111111111111".into(),
            "0x2222222222222222222222222222222222222222".into(),
            "0x3333333333333333333333333333333333333333".into(),
            "0x4444444444444444444444444444444444444444".into(),
            "0x5555555555555555555555555555555555555555".into(),
        ];
        let err = cfg.validate_runtime().unwrap_err().to_string();
        assert!(err.contains("5-of-5") || err.contains("threshold=5"));
    }
}
