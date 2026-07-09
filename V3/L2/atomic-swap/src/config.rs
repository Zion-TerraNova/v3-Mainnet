//! Atomic-swap daemon configuration.

use crate::evm_watcher::EvmWatcherConfig;
use serde::{Deserialize, Serialize};

/// Top-level daemon configuration (loaded from TOML).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapConfig {
    /// Daemon identity and network
    pub swap: SwapIdentity,

    /// ZION L1 node connection
    pub l1: L1Config,

    /// SQLite database settings
    pub database: DatabaseConfig,

    /// HTTP API settings
    pub api: ApiConfig,

    /// Refund automation settings
    pub refund: RefundConfig,

    /// EVM watcher for cross-chain HTLC events (Base, etc.)
    #[serde(default)]
    pub evm_watcher: Option<EvmWatcherConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapIdentity {
    /// Human-readable name (for logs / metrics)
    pub name: String,

    /// Network identifier: "mainnet" | "testnet" | "devnet"
    pub network: String,

    /// Minimum lock amount (atomic units).  Rejects tiny locks.
    pub min_lock_flowers: u64,

    /// Maximum lock amount (atomic units = 10_000 ZION).
    pub max_lock_atomic: u64,

    /// Flat fee in atomic units deducted from each release to cover
    /// the escrow account's L1 TX fee.
    pub release_fee_atomic: u64,
}

impl Default for SwapIdentity {
    fn default() -> Self {
        Self {
            name: "ZION Atomic Swap Service".into(),
            network: "mainnet".into(),
            min_lock_flowers: 1_000_000,     // 1 ZION (1e6 scale)
            max_lock_atomic: 10_000_000_000, // 10 000 ZION (1e6 scale)
            release_fee_atomic: 2_000,       // 0.002 ZION
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L1Config {
    /// L1 raw TCP JSON-RPC address, e.g. `127.0.0.1:8443`
    pub rpc_url: String,

    /// Bearer token for L1 RPC (value of `ZION_RPC_TOKEN` on node).
    /// Can be overridden by `ZION_RPC_TOKEN` env var.
    #[serde(default)]
    pub rpc_token: Option<String>,

    /// Escrow keypair (hex-encoded 32-byte Ed25519 secret).
    /// Prefer the `ZION_SWAP_ESCROW_KEY` env var over TOML for prod.
    #[serde(default)]
    pub escrow_key_hex: Option<String>,

    /// How many L1 blocks to scan per watcher iteration.
    pub scan_batch_size: u64,

    /// Poll interval (seconds) for the L1 block watcher.
    pub poll_interval_secs: u64,
}

impl Default for L1Config {
    fn default() -> Self {
        Self {
            rpc_url: "127.0.0.1:8443".into(),
            rpc_token: None,
            escrow_key_hex: None,
            scan_batch_size: 10,
            poll_interval_secs: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub path: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: "/data/atomic-swap.db".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    /// Bind address for the HTTP API server.
    pub bind: String,

    /// Secret bearer token for protected endpoints (claim / refund).
    /// If `None` — the API is open (only suitable for local/dev runs).
    #[serde(default)]
    pub bearer_token: Option<String>,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:8888".into(),
            bearer_token: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefundConfig {
    /// Enable automatic refund of expired HTLCs (background loop).
    pub auto_refund: bool,

    /// How often (seconds) the refund loop checks for expired locks.
    pub check_interval_secs: u64,

    /// Grace period (seconds) added on top of the HTLC timelock before
    /// the daemon issues an auto-refund.  Allows late claims to succeed.
    pub grace_period_secs: u64,
}

impl Default for RefundConfig {
    fn default() -> Self {
        Self {
            auto_refund: true,
            check_interval_secs: 30,
            grace_period_secs: 120,
        }
    }
}

impl SwapConfig {
    /// Load from a TOML file path.
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let cfg: Self = toml::from_str(&raw)?;
        Ok(cfg)
    }

    /// Validate runtime safety constraints. Call after loading.
    /// On mainnet, the HTTP API bearer_token MUST be set — otherwise the
    /// `/swap/claim` and `/swap/refund` endpoints are open-access, which
    /// combined with legacy (no-claimant) HTLC locks enables front-running
    /// (C1 security patch).
    pub fn validate_runtime(&self) -> anyhow::Result<()> {
        let mainnet = self.swap.network.eq_ignore_ascii_case("mainnet");
        if mainnet {
            if self.api_bearer_token().is_none() {
                anyhow::bail!(
                    "mainnet requires api.bearer_token or ZION_SWAP_BEARER_TOKEN env var \
                     (open-access claim/refund endpoints are unsafe — C1)"
                );
            }
            if self.escrow_key_hex().is_none() {
                anyhow::bail!("mainnet requires ZION_SWAP_ESCROW_KEY env var (escrow signing key)");
            }
        }
        Ok(())
    }

    /// Resolve the escrow key: env var takes priority over TOML field.
    pub fn escrow_key_hex(&self) -> Option<String> {
        std::env::var("ZION_SWAP_ESCROW_KEY")
            .ok()
            .or_else(|| self.l1.escrow_key_hex.clone())
    }

    /// Resolve the L1 RPC bearer token: env var takes priority.
    pub fn rpc_token(&self) -> Option<String> {
        std::env::var("ZION_RPC_TOKEN")
            .ok()
            .or_else(|| self.l1.rpc_token.clone())
    }

    /// Resolve the API bearer token: `ZION_SWAP_BEARER_TOKEN` env var
    /// takes priority over the `api.bearer_token` TOML field.
    pub fn api_bearer_token(&self) -> Option<String> {
        std::env::var("ZION_SWAP_BEARER_TOKEN")
            .ok()
            .or_else(|| self.api.bearer_token.clone())
    }
}
