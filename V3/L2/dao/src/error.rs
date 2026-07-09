//! DAO Error types

use thiserror::Error;

#[derive(Error, Debug)]
pub enum DaoError {
    // Proposal errors
    #[error("Proposal not found: {0}")]
    ProposalNotFound(String),

    #[error("Proposal already exists: {0}")]
    ProposalAlreadyExists(String),

    #[error("Proposal not in votable state: {0}")]
    ProposalNotVotable(String),

    #[error("Proposal expired: {0}")]
    ProposalExpired(String),

    // Voting errors
    #[error("Insufficient balance for proposal creation: need {needed} ZION, have {have}")]
    InsufficientProposalBalance { needed: u64, have: u64 },

    #[error("Already voted on proposal {0}")]
    AlreadyVoted(String),

    #[error("Voting period ended for proposal {0}")]
    VotingPeriodEnded(String),

    #[error("Voting period not yet ended for proposal {0}")]
    VotingPeriodNotEnded(String),

    // Quorum errors
    #[error("Quorum not reached: need {needed}%, got {got}%")]
    QuorumNotReached { needed: f64, got: f64 },

    // Treasury errors
    #[error("Insufficient treasury balance: need {needed}, have {available}")]
    InsufficientTreasuryBalance { needed: u64, available: u64 },

    #[error("Treasury operation requires {needed}-of-{total} signatures, have {have}")]
    InsufficientSignatures { needed: u32, have: u32, total: u32 },

    #[error("Treasury daily limit exceeded: {limit} ZION/day")]
    DailyLimitExceeded { limit: u64 },

    #[error("Invalid Golden Egg prize place: {0}")]
    InvalidPrizePlace(u8),

    // Timelock errors
    #[error("Timelock active: {remaining_hours}h remaining")]
    TimelockActive { remaining_hours: u64 },

    #[error("Timelock expired: proposal must be re-submitted")]
    TimelockExpired,

    // Authorization
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Invalid signature: {0}")]
    InvalidSignature(String),

    // Infrastructure
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type DaoResult<T> = Result<T, DaoError>;
