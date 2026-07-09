//! **Zion SDK** — async library for working with ZION L1 node over TCP JSON-RPC (one JSON object per line, default port **8443**).
//!
//! ## Basic usage
//!
//! ```no_run
//! use zion_sdk::node::NodeClient;
//!
//! # async fn demo() -> zion_sdk::Result<()> {
//! let client = NodeClient::builder("127.0.0.1", 8443)
//!     .connect_timeout(std::time::Duration::from_secs(5))
//!     .request_timeout(std::time::Duration::from_secs(30))
//!     .build();
//!
//! let chain = client.chain_info().await?;
//! println!("height={}", chain.chain_height);
//! # Ok(())
//! # }
//! ```
//!
//! Behavior is aligned with `zion-cli` (`V3/cli`): same wire protocol and same method names (`getChainInfo`, …).
//!
//! ## Production configuration
//!
//! [`NodeClient::from_env`] / [`NodeClientBuilder::from_env`] load [`NodeClientConfig`] from `ZION_RPC_*` variables (see [`config`]).
//!
//! Optionally enable the **`tracing`** feature for structured RPC logs (retry, success).

pub mod config;
pub mod error;
pub mod node;
pub mod rpc_codes;
pub mod types;
pub mod wallet;

/// Crate version (`CARGO_PKG_VERSION`).
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

pub use config::{parse_rpc_addr, NodeClientConfig};
pub use error::{Result, RpcErrorBody, ZionSdkError};
pub use node::{NodeClient, NodeClientBuilder};
pub use types::{
    ChainInfo, MempoolInfo, NodeInfo, PeerEndpoint, PeerInfo, SubmitAccepted, SubmitBlockParams,
    SubmitCandidateResult, SupplyInfo,
};
pub use wallet::{BalanceBreakdown, KeyPair, SendResult, TxModel, WalletClient};

/// Common imports for application code.
pub mod prelude {
    pub use crate::config::{parse_rpc_addr, NodeClientConfig};
    pub use crate::error::{Result, RpcErrorBody, ZionSdkError};
    pub use crate::node::{NodeClient, NodeClientBuilder};
    pub use crate::types::{
        ChainInfo, MempoolInfo, NodeInfo, PeerEndpoint, PeerInfo, SubmitAccepted,
        SubmitBlockParams, SubmitCandidateResult, SupplyInfo,
    };
    pub use crate::wallet::{BalanceBreakdown, KeyPair, SendResult, TxModel, WalletClient};
}
