//! Golden Egg Prize Distribution
//!
//! Links the OASIS L4 game server with the DAO treasury.
//! When a player claims the Golden Egg, the DAO creates a treasury
//! operation that requires 5-of-7 guardian signatures before payout.
//!
//! Prize pool (on-chain via DAO treasury):
//!   1st — 1,000,000,000 ZION  (CEO)
//!   2nd —   500,000,000 ZION  (CCO)
//!   3rd —   250,000,000 ZION  (CAO)

use crate::error::{DaoError, DaoResult};
use crate::treasury::{Treasury, TreasuryOperation};
use serde::{Deserialize, Serialize};

/// Prize tiers for the Golden Egg hunt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrizeTier {
    pub place: u8,
    pub title: &'static str,
    pub amount_zion: u64,
    pub dao_voting_boost_pct: u8,
}

/// Static prize configuration
pub const PRIZE_TIERS: &[PrizeTier] = &[
    PrizeTier {
        place: 1,
        title: "CEO — Hiranyagarbha Sovereign",
        amount_zion: 1_000_000_000,
        dao_voting_boost_pct: 15,
    },
    PrizeTier {
        place: 2,
        title: "CCO — Cosmic Architect",
        amount_zion: 500_000_000,
        dao_voting_boost_pct: 10,
    },
    PrizeTier {
        place: 3,
        title: "CAO — Divine Strategist",
        amount_zion: 250_000_000,
        dao_voting_boost_pct: 5,
    },
];

/// A pending Golden Egg prize awaiting DAO guardian signatures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPrize {
    pub place: u8,
    pub recipient: String,
    pub amount: u64,
    pub claimed_at: u64, // unix seconds
    pub proposal_id: u64,
}

/// Prize distribution engine
pub struct PrizeDistributor;

impl PrizeDistributor {
    /// Total prize pool (1.75B ZION)
    pub fn total_pool() -> u64 {
        PRIZE_TIERS.iter().map(|t| t.amount_zion).sum()
    }

    /// Create a treasury operation for a prize payout.
    /// Returns the operation ID.
    pub fn request_payout(
        treasury: &mut Treasury,
        place: u8,
        recipient: &str,
        submitter: &str,
    ) -> DaoResult<String> {
        let tier = PRIZE_TIERS
            .iter()
            .find(|t| t.place == place)
            .ok_or_else(|| DaoError::InvalidPrizePlace(place))?;

        let op = TreasuryOperation::GoldenEggPrize {
            place,
            recipient: recipient.to_string(),
            amount: tier.amount_zion,
            proposal_id: 0,
        };

        let op_id = format!("prize_{}_{}", place, recipient);
        treasury.submit_operation(op_id.clone(), op, submitter)?;
        Ok(op_id)
    }

    /// Check whether a prize for a given place has already been requested.
    pub fn place_filled(treasury: &Treasury, _place: u8) -> bool {
        // Check pending or executed — simplified: scan pending for now
        treasury.pending_count() > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_total_pool() {
        assert_eq!(PrizeDistributor::total_pool(), 1_750_000_000);
    }

    #[test]
    fn test_prize_tiers() {
        assert_eq!(PRIZE_TIERS[0].amount_zion, 1_000_000_000);
        assert_eq!(PRIZE_TIERS[1].dao_voting_boost_pct, 10);
        assert_eq!(PRIZE_TIERS[2].place, 3);
    }
}
