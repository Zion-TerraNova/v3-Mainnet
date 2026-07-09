//! # ZION DAO — L2 Governance Layer
//!
//! On-chain governance for the ZION ecosystem.
//! Token-weighted voting: **1 ZION = 1 vote**.
//!
//! ## Architecture
//!
//! ```text
//!                    ┌──────────────────────────────────┐
//!                    │         DAO GOVERNANCE            │
//!                    │                                   │
//!                    │  ┌──────────┐  ┌──────────────┐  │
//!                    │  │ Proposal │  │   Treasury    │  │
//!                    │  │ Engine   │  │ Multi-sig 5/7 │  │
//!                    │  └────┬─────┘  └──────┬───────┘  │
//!                    │       │               │          │
//!                    │  ┌────┴─────┐  ┌──────┴───────┐  │
//!                    │  │ Voting   │  │ Humanitarian  │  │
//!                    │  │ 1Z = 1V  │  │ 7 Categories  │  │
//!                    │  └────┬─────┘  └──────────────┘  │
//!                    │       │                           │
//!                    │  ┌────┴─────┐  ┌──────────────┐  │
//!                    │  │ Timelock │  │   Executor    │  │
//!                    │  │   48h    │  │   On-chain    │  │
//!                    │  └──────────┘  └──────────────┘  │
//!                    └──────────────────────────────────┘
//!                                  │
//!                    ┌─────────────┴──────────────┐
//!                    │  L1 BLOCKCHAIN (READ ONLY) │
//!                    │  TX memo: "DAO:vote:42"    │
//!                    └────────────────────────────┘
//! ```
//!
//! ## ⚠️ Layer Boundary
//!
//! This is an **L2 crate**. It MUST NOT modify L1 state directly.
//! Communication with L1 is through:
//! - TX memo fields: `DAO:vote:<proposal_id>`
//! - RPC queries: balance checks for voting weight
//! - Block events: monitoring for governance TXs
//!
//! ## Key Parameters
//!
//! | Parameter        | Value              | Source                |
//! |------------------|--------------------|-----------------------|
//! | Treasury         | 4,000,000,000 ZION | Genesis premine       |
//! | Proposal thresh. | 1,000,000 ZION     | Min to create         |
//! | Quorum           | 10% participation  | Of circulating supply |
//! | Voting period    | 7 days             | Standard proposals    |
//! | Timelock         | 48 hours           | Before execution      |
//! | Multi-sig        | 5-of-7             | Treasury operations   |

pub mod co_admin;
pub mod config;
pub mod consent;
pub mod cross_layer;
pub mod error;
pub mod executor;
pub mod humanitarian;
pub mod prizes;
pub mod proposal;
pub mod quorum;
pub mod timelock;
pub mod treasury;
pub mod types;
pub mod voting;

// Persistence + daemon layer (added in v2.9.6)
pub mod api;
pub mod db;
pub mod l1_scanner;
pub mod metrics;

// Re-exports
pub use co_admin::CoAdminRegistry;
pub use config::DaoConfig;
pub use consent::{Attestation, ConsentEngine, ConsentRecord};
pub use cross_layer::{CrossLayerRegistry, CrossLayerState, LayerConsent};
pub use db::DaoDb;
pub use error::{DaoError, DaoResult};
pub use humanitarian::{HumanitarianCategory, HumanitarianFund};
pub use l1_scanner::{L1Scanner, ScannerConfig};
pub use proposal::{Proposal, ProposalStatus, ProposalType};
pub use treasury::{Treasury, TreasuryOperation};
pub use types::VoteChoice;
pub use voting::{Vote, VotingEngine};
