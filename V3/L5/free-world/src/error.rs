//! Error types for zion-free-world.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum FreeWorldError {
    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Invalid grant status transition: {from} -> {to}")]
    InvalidGrantTransition { from: String, to: String },

    #[error("Grant not found: {0}")]
    GrantNotFound(String),

    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("Insufficient funds: required {required}, available {available}")]
    InsufficientFunds { required: u64, available: u64 },

    #[error("L1 RPC error: {0}")]
    L1Rpc(String),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("{0}")]
    Other(String),
}

pub type FreeWorldResult<T> = Result<T, FreeWorldError>;
