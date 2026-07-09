//! OASIS Error types

use thiserror::Error;

#[derive(Error, Debug)]
pub enum OasisError {
    #[error("Player not found: {0}")]
    PlayerNotFound(String),

    #[error("Guild not found: {0}")]
    GuildNotFound(String),

    #[error("Territory not found: {0}")]
    TerritoryNotFound(String),

    #[error("Challenge not found: {0}")]
    ChallengeNotFound(String),

    #[error("Insufficient XP: need {needed}, have {have}")]
    InsufficientXp { needed: u64, have: u64 },

    #[error("Already in guild: {0}")]
    AlreadyInGuild(String),

    #[error("Guild full: max {max} members")]
    GuildFull { max: u32 },

    #[error("Level requirement not met: need level {needed}")]
    LevelRequirement { needed: u32 },

    #[error("Challenge already completed")]
    ChallengeAlreadyCompleted,

    #[error("Cooldown active: {remaining_secs}s remaining")]
    CooldownActive { remaining_secs: u64 },

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type OasisResult<T> = Result<T, OasisError>;
