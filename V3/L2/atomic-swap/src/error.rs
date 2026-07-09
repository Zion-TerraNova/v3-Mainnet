//! SwapError — structured errors for the atomic-swap daemon.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SwapError {
    // ── Database ──────────────────────────────────────────────────────────
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    // ── L1 RPC ───────────────────────────────────────────────────────────
    #[error("L1 RPC error: {0}")]
    L1Rpc(String),

    #[error("HTTP client error: {0}")]
    Http(#[from] reqwest::Error),

    // ── Crypto ───────────────────────────────────────────────────────────
    #[error("Invalid preimage: SHA-256(preimage) does not match hash")]
    PreimageMismatch,

    #[error("Invalid hex: {0}")]
    HexDecode(#[from] hex::FromHexError),

    #[error("Invalid escrow key: {msg}")]
    InvalidEscrowKey { msg: String },

    // ── HTLC logic ────────────────────────────────────────────────────────
    #[error("HTLC not found for hash {hash_hex}")]
    HtlcNotFound { hash_hex: String },

    #[error("HTLC {hash_hex} is already in terminal state: {state}")]
    AlreadySettled { hash_hex: String, state: String },

    #[error("HTLC {hash_hex} timelock has not expired yet (expires at {expires_at})")]
    TimelockActive { hash_hex: String, expires_at: i64 },

    #[error("HTLC {hash_hex} has expired — cannot claim after timelock")]
    TimelockExpired { hash_hex: String },

    #[error("Insufficient escrow balance: have {have} atomic, need {need} atomic")]
    InsufficientBalance { have: u64, need: u64 },

    // ── Config / setup ────────────────────────────────────────────────────
    #[error("Configuration error: {0}")]
    Config(String),

    // ── Generic ──────────────────────────────────────────────────────────
    #[error("Internal error: {0}")]
    Internal(String),
}

pub type SwapResult<T> = Result<T, SwapError>;
