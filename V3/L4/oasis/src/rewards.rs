//! Rewards — 8.25B OASIS pool distribution system.
//!
//! OASIS premine allocation: 8.25B ZION (5 slots × 1.65B)
//! Distributed over 10 years via mining rewards, challenges, and bonuses.
//!
//! ┌──────────────────────────────────────────┐
//! │          OASIS REWARD POOL               │
//! │         8,250,000,000 ZION               │
//! │                                          │
//! │  Slot 1: Mining Rewards     1.65B (20%)  │
//! │  Slot 2: Challenge Rewards  1.65B (20%)  │
//! │  Slot 3: Guild & Territory  1.65B (20%)  │
//! │  Slot 4: Level-Up Bonuses   1.65B (20%)  │
//! │  Slot 5: Reserve / Future   1.65B (20%)  │
//! └──────────────────────────────────────────┘

use serde::{Deserialize, Serialize};

/// Total OASIS premine allocation (ZION)
pub const OASIS_TOTAL: u64 = 8_250_000_000;
/// Per-slot allocation (ZION)
pub const SLOT_ALLOCATION: u64 = 1_650_000_000;
/// Distribution period (10 years in seconds)
pub const DISTRIBUTION_PERIOD: u64 = 10 * 365 * 24 * 3600;

/// Reward pool slots
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum RewardSlot {
    /// Slot 1: Mining rewards (XP-boosted mining payouts)
    MiningRewards,
    /// Slot 2: Challenge completion rewards
    ChallengeRewards,
    /// Slot 3: Guild and territory bonuses
    GuildTerritory,
    /// Slot 4: Level-up bonus payouts
    LevelUpBonuses,
    /// Slot 5: Reserve for future features
    Reserve,
}

impl RewardSlot {
    pub fn allocation(&self) -> u64 {
        SLOT_ALLOCATION
    }

    pub fn all() -> Vec<RewardSlot> {
        vec![
            RewardSlot::MiningRewards,
            RewardSlot::ChallengeRewards,
            RewardSlot::GuildTerritory,
            RewardSlot::LevelUpBonuses,
            RewardSlot::Reserve,
        ]
    }
}

/// Tracks reward distribution from each slot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardPool {
    pub slot: RewardSlot,
    pub total: u64,
    pub distributed: u64,
    pub locked: bool,
}

impl RewardPool {
    pub fn new(slot: RewardSlot) -> Self {
        Self {
            slot,
            total: SLOT_ALLOCATION,
            distributed: 0,
            locked: false,
        }
    }

    /// Remaining balance in this pool
    pub fn remaining(&self) -> u64 {
        self.total.saturating_sub(self.distributed)
    }

    /// Distribute rewards (returns actual amount distributed)
    pub fn distribute(&mut self, amount: u64) -> Result<u64, RewardError> {
        if self.locked {
            return Err(RewardError::PoolLocked);
        }
        let actual = amount.min(self.remaining());
        if actual == 0 {
            return Err(RewardError::PoolExhausted);
        }
        self.distributed += actual;
        Ok(actual)
    }

    /// Percentage distributed
    pub fn distribution_percentage(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        (self.distributed as f64 / self.total as f64) * 100.0
    }
}

/// Reward distribution errors
#[derive(Debug, Clone)]
pub enum RewardError {
    PoolExhausted,
    PoolLocked,
    InvalidAmount,
    SlotNotFound,
}

impl std::fmt::Display for RewardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PoolExhausted => write!(f, "Reward pool exhausted"),
            Self::PoolLocked => write!(f, "Reward pool is locked"),
            Self::InvalidAmount => write!(f, "Invalid reward amount"),
            Self::SlotNotFound => write!(f, "Reward slot not found"),
        }
    }
}

/// Master reward manager — manages all 5 slots
pub struct RewardManager {
    pools: Vec<RewardPool>,
}

impl Default for RewardManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RewardManager {
    pub fn new() -> Self {
        Self {
            pools: RewardSlot::all().into_iter().map(RewardPool::new).collect(),
        }
    }

    /// Get pool by slot
    pub fn pool(&self, slot: RewardSlot) -> Option<&RewardPool> {
        self.pools.iter().find(|p| p.slot == slot)
    }

    /// Get mutable pool by slot
    pub fn pool_mut(&mut self, slot: RewardSlot) -> Option<&mut RewardPool> {
        self.pools.iter_mut().find(|p| p.slot == slot)
    }

    /// Distribute from a specific slot
    pub fn distribute(&mut self, slot: RewardSlot, amount: u64) -> Result<u64, RewardError> {
        self.pool_mut(slot)
            .ok_or(RewardError::SlotNotFound)?
            .distribute(amount)
    }

    /// Total remaining across all pools
    pub fn total_remaining(&self) -> u64 {
        self.pools.iter().map(|p| p.remaining()).sum()
    }

    /// Total distributed across all pools
    pub fn total_distributed(&self) -> u64 {
        self.pools.iter().map(|p| p.distributed).sum()
    }

    /// Summary of all pools
    pub fn summary(&self) -> Vec<PoolSummary> {
        self.pools
            .iter()
            .map(|p| PoolSummary {
                slot: p.slot,
                total: p.total,
                distributed: p.distributed,
                remaining: p.remaining(),
                percentage: p.distribution_percentage(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolSummary {
    pub slot: RewardSlot,
    pub total: u64,
    pub distributed: u64,
    pub remaining: u64,
    pub percentage: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_total_allocation() {
        let manager = RewardManager::new();
        assert_eq!(manager.total_remaining(), OASIS_TOTAL);
    }

    #[test]
    fn test_distribute() {
        let mut manager = RewardManager::new();
        let amount = manager.distribute(RewardSlot::MiningRewards, 1000).unwrap();
        assert_eq!(amount, 1000);
        assert_eq!(manager.total_distributed(), 1000);
        assert_eq!(manager.total_remaining(), OASIS_TOTAL - 1000);
    }

    #[test]
    fn test_pool_exhaustion() {
        let mut pool = RewardPool::new(RewardSlot::Reserve);
        pool.distributed = SLOT_ALLOCATION; // exhaust it
        assert!(pool.distribute(1).is_err());
    }

    #[test]
    fn test_locked_pool() {
        let mut pool = RewardPool::new(RewardSlot::Reserve);
        pool.locked = true;
        assert!(pool.distribute(1).is_err());
    }

    #[test]
    fn test_summary() {
        let manager = RewardManager::new();
        let summary = manager.summary();
        assert_eq!(summary.len(), 5);
        assert!(summary.iter().all(|s| s.percentage == 0.0));
    }
}
