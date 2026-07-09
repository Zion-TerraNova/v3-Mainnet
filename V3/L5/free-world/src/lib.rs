//! # ZION Free World — V3 L5 Humanitarian Layer
//!
//! > *"Freedom is not given — it is built, block by block."*
//!
//! ## Architecture
//!
//! ```text
//!                 ┌──────────────────────────────────────┐
//!                 │      ZION FREE WORLD (L5)           │
//!                 │                                      │
//!                 │  ┌────────────┐  ┌──────────────┐  │
//!                 │  │  Grants    │  │   Projects   │  │
//!                 │  │  Engine    │  │  (Humanitarian│  │
//!                 │  └─────┬──────┘  │   Energy,    │  │
//!                 │  ┌─────┴──────┐  │   Education) │  │
//!                 │  │ Communities│  └──────┬───────┘  │
//!                 │  │  Registry  │         │          │
//!                 │  └────────────┘  ┌───────┴───────┐  │
//!                 │                  │  Fund Balance │  │
//!                 │                  │  (L1 scanner) │  │
//!                 └──────────────────┴───────────────┘  │
//!                                    │
//!                 ┌─────────────────┴──────────────────┐
//!                 │  L1 BLOCKCHAIN (READ ONLY)          │
//!                 │  5% block reward → Humanitarian Fund│
//!                 └─────────────────────────────────────┘
//! ```
//!
//! ## ⚠️ Layer Boundary
//!
//! This is a **V3 L5 crate**. It MUST NOT modify L1 state directly.
//! Communication with L1 is through:
//! - L1 RPC polling for block rewards
//! - Read-only queries to track humanitarian tithe accumulation
//! - L2 DAO for governance proposals on fund allocation

pub mod api;
pub mod config;
pub mod dao_client;
pub mod db;
pub mod error;
pub mod hiran_bridge;
pub mod l1_scanner;
pub mod metrics;

// Re-exports
pub use config::FreeWorldConfig;
pub use db::{FreeWorldDb, FundBalance, GrantRecord, ProjectRecord};
pub use error::{FreeWorldError, FreeWorldResult};
pub use l1_scanner::{L1Scanner, ScannerConfig};
