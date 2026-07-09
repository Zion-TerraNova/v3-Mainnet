//! # ZION Bridge Relay
//!
//! Cross-chain bridge relay between ZION L1 and EVM chains (Base, Arbitrum, BSC, Polygon).
//!
//! ## Architecture
//!
//! ```text
//! ZION L1                    Bridge Relay (this crate)              EVM Chain
//! ┌──────────┐              ┌─────────────────────┐              ┌──────────┐
//! │ User     │  lock TX     │  L1 Watcher         │  submitProof │ wZION    │
//! │ sends    │─────────────▶│  (polls node RPC)   │─────────────▶│ .sol     │
//! │ ZION to  │              │                     │              │ mints    │
//! │ bridge   │              │  EVM Watcher        │  burn event  │ wZION    │
//! │ vault    │◀─────────────│  (listens to        │◀─────────────│ User     │
//! │ unlock   │  unlock TX   │   BridgeBurn)       │              │ burns    │
//! └──────────┘              └─────────────────────┘              └──────────┘
//! ```
//!
//! ## V3 Mainnet Decimal Convention
//!
//! - L1: 1 ZION = 1,000,000,000,000 flowers (12 decimals, u64)
//! - EVM: 1 wZION = 1e18 wei (18 decimals)
//! - Conversion factor: 1e6 (18 - 12 = 6)
//!
//! **WARNING**: Legacy L2 code used 6 decimals (1 ZION = 1,000,000 atomic).
//! V3 uses 12 decimals (1 ZION = 1e12 flowers). All conversion functions
//! have been updated accordingly.

pub mod ankr;
pub mod config;
pub mod db;
pub mod evm_rpc;
pub mod evm_tx;
pub mod evm_watcher;
pub mod l1_watcher;
pub mod metrics;
pub mod rate_limiter;
pub mod relayer;
pub mod types;
pub mod validator;

pub use config::BridgeConfig;
pub use types::*;
