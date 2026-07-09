//! ZION Atomic Swap Daemon — HTLC (Hash Time-Lock Contract) engine
//!
//! # Protocol
//!
//! HTLC swaps are coordinated via L1 transaction memos on a shared **escrow address**:
//!
//! | Step | Actor  | Memo format                                          |
//! |------|--------|------------------------------------------------------|
//! | LOCK | Alice  | `SWAP:LOCK:<hash_hex>:<timeout_min>:<chain>:<addr>`  |
//! | CLAIM| Bob    | `SWAP:CLAIM:<hash_hex>:<preimage_hex>`               |
//! | REFUND| Alice | `SWAP:REFUND:<hash_hex>`                             |
//!
//! # Cross-chain flow (ZION ↔ BTC example)
//! ```text
//! 1. Alice generates secret S, H = SHA-256(S)
//! 2. Alice → escrow: N ZION, memo SWAP:LOCK:<H>:120:btc:<bob_btc_addr>
//! 3. Bob witnesses LOCK, creates BTC HTLC with same H, 1h timelock
//! 4. Alice claims BTC by revealing S on Bitcoin
//! 5. Bob sends ZION TX or calls POST /swap/claim {hash, preimage, recipient}
//!    → daemon verifies SHA256(preimage)==H, releases ZION to Bob
//! 6. If Bob never claims: after 2h daemon refunds ZION to Alice
//! ```
//!
//! # Modules
//! - [`types`]    — core HTLC types, memo parser  (S-01)
//! - [`error`]    — SwapError enum
//! - [`config`]   — TOML configuration
//! - [`db`]       — SQLite persistence             (S-02)
//! - [`watcher`]  — L1 block scanner               (S-03)
//! - [`executor`] — sign + submit L1 release TX    (S-04)
//! - [`handlers`] — axum HTTP handlers             (S-05)

pub mod config;
pub mod db;
pub mod error;
pub mod evm_watcher;
pub mod executor;
pub mod handlers;
pub mod types;
pub mod watcher;
