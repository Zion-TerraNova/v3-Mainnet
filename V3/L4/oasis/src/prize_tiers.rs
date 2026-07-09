//! Prize Tiers — Golden Egg prize distribution and ranking system.
//!
//! 10 tiers from Hiranyagarbha Sovereign (1B ZION) down to Physical Initiate (100K ZION).

use serde::{Deserialize, Serialize};

/// A single prize tier
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrizeTier {
    pub rank: u32,
    pub title: String,
    pub zion: u64,
    pub flowers: u128,
    pub percentage: f64,
    pub nft_reward: String,
    pub unlock_condition: String,
}

/// Prize tier configuration (loaded from JSON)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrizeConfig {
    #[serde(rename = "_comment")]
    pub comment: String,
    pub total_pool_zion: u64,
    pub total_pool_flowers: u128,
    pub tiers: Vec<PrizeTier>,
    pub dao_approval_required: bool,
    pub donation_requirement: String,
}

impl Default for PrizeConfig {
    fn default() -> Self {
        Self {
            comment: "Golden Egg Prize Distribution".to_string(),
            total_pool_zion: 1_000_000_000,
            total_pool_flowers: 1_000_000_000_000_000,
            tiers: Self::default_tiers(),
            dao_approval_required: true,
            donation_requirement: "100% of 1st place must be donated".to_string(),
        }
    }
}

impl PrizeConfig {
    pub fn default_tiers() -> Vec<PrizeTier> {
        vec![
            PrizeTier {
                rank: 1,
                title: "Hiranyagarbha Sovereign".into(),
                zion: 1_000_000_000,
                flowers: 1_000_000_000_000_000,
                percentage: 100.0,
                nft_reward: "Legendary Golden Egg Solver".into(),
                unlock_condition: "All 3 Master Keys + CL 9 + DAO 67%".into(),
            },
            PrizeTier {
                rank: 2,
                title: "Cosmic Guardian".into(),
                zion: 500_000_000,
                flowers: 500_000_000_000_000,
                percentage: 50.0,
                nft_reward: "Epic Cosmic Guardian".into(),
                unlock_condition: "2 Master Keys + CL 8 + Top 2 raid".into(),
            },
            PrizeTier {
                rank: 3,
                title: "Divine Strategist".into(),
                zion: 250_000_000,
                flowers: 250_000_000_000_000,
                percentage: 25.0,
                nft_reward: "Epic Divine Strategist".into(),
                unlock_condition: "2 Master Keys + CL 7 + Top 3 raid".into(),
            },
            PrizeTier {
                rank: 4,
                title: "Star Mystic".into(),
                zion: 100_000_000,
                flowers: 100_000_000_000_000,
                percentage: 10.0,
                nft_reward: "Rare Star Mystic".into(),
                unlock_condition: "1 Master Key + CL 6 + Top 10 raid".into(),
            },
            PrizeTier {
                rank: 5,
                title: "Ascended Sage".into(),
                zion: 50_000_000,
                flowers: 50_000_000_000_000,
                percentage: 5.0,
                nft_reward: "Rare Ascended Sage".into(),
                unlock_condition: "1 Master Key + CL 5 + Top 20 raid".into(),
            },
            PrizeTier {
                rank: 6,
                title: "Spiritual Warrior".into(),
                zion: 25_000_000,
                flowers: 25_000_000_000_000,
                percentage: 2.5,
                nft_reward: "Uncommon Spiritual Warrior".into(),
                unlock_condition: "CL 4 + Top 50 raid".into(),
            },
            PrizeTier {
                rank: 7,
                title: "Intuitional Seeker".into(),
                zion: 10_000_000,
                flowers: 10_000_000_000_000,
                percentage: 1.0,
                nft_reward: "Uncommon Intuitional Seeker".into(),
                unlock_condition: "CL 3 + Top 100 raid".into(),
            },
            PrizeTier {
                rank: 8,
                title: "Mental Adept".into(),
                zion: 5_000_000,
                flowers: 5_000_000_000_000,
                percentage: 0.5,
                nft_reward: "Common Mental Adept".into(),
                unlock_condition: "CL 2 + Top 500 raid".into(),
            },
            PrizeTier {
                rank: 9,
                title: "Emotional Healer".into(),
                zion: 1_000_000,
                flowers: 1_000_000_000_000,
                percentage: 0.1,
                nft_reward: "Common Emotional Healer".into(),
                unlock_condition: "CL 1 + Top 1000 raid".into(),
            },
            PrizeTier {
                rank: 10,
                title: "Physical Initiate".into(),
                zion: 100_000,
                flowers: 100_000_000_000,
                percentage: 0.01,
                nft_reward: "Common Physical Initiate".into(),
                unlock_condition: "CL 1 + Participation".into(),
            },
        ]
    }

    pub fn from_json_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: PrizeConfig = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn from_embedded_json() -> Self {
        let json = include_str!("../data/prize_tiers.json");
        serde_json::from_str(json).unwrap_or_default()
    }

    /// Get tier by rank
    pub fn tier(&self, rank: u32) -> Option<&PrizeTier> {
        self.tiers.iter().find(|t| t.rank == rank)
    }

    /// Get tier for a given raid position
    pub fn tier_for_position(&self, position: u32) -> Option<&PrizeTier> {
        match position {
            1 => self.tier(1),
            2 => self.tier(2),
            3 => self.tier(3),
            4..=10 => self.tier(4),
            11..=20 => self.tier(5),
            21..=50 => self.tier(6),
            51..=100 => self.tier(7),
            101..=500 => self.tier(8),
            501..=1000 => self.tier(9),
            _ => self.tier(10),
        }
    }

    /// Total ZION distributed across all tiers (sanity check)
    pub fn total_distributed_zion(&self) -> u64 {
        self.tiers.iter().map(|t| t.zion).sum()
    }
}

/// Prize tracking per player
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerPrize {
    pub address: String,
    pub raid_position: u32,
    pub tier_rank: u32,
    pub zion_awarded: u64,
    pub nft_reward: String,
    pub awarded_at: u64,
    pub dao_approved: bool,
}

/// Prize ledger — tracks all Golden Egg prize awards
pub struct PrizeLedger {
    awards: Vec<PlayerPrize>,
}

impl Default for PrizeLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl PrizeLedger {
    pub fn new() -> Self {
        Self { awards: Vec::new() }
    }

    pub fn award(&mut self, prize: PlayerPrize) {
        self.awards.push(prize);
    }

    pub fn by_address(&self, address: &str) -> Vec<&PlayerPrize> {
        self.awards
            .iter()
            .filter(|a| a.address == address)
            .collect()
    }

    pub fn total_awarded_zion(&self) -> u64 {
        self.awards.iter().map(|a| a.zion_awarded).sum()
    }

    pub fn top_awards(&self, limit: usize) -> Vec<&PlayerPrize> {
        let mut sorted: Vec<&PlayerPrize> = self.awards.iter().collect();
        sorted.sort_by(|a, b| b.zion_awarded.cmp(&a.zion_awarded));
        sorted.truncate(limit);
        sorted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_tiers_count() {
        let config = PrizeConfig::default();
        assert_eq!(config.tiers.len(), 10);
    }

    #[test]
    fn test_tier_for_position() {
        let config = PrizeConfig::default();
        assert_eq!(config.tier_for_position(1).unwrap().rank, 1);
        assert_eq!(config.tier_for_position(2).unwrap().rank, 2);
        assert_eq!(config.tier_for_position(5).unwrap().rank, 4);
        assert_eq!(config.tier_for_position(25).unwrap().rank, 6);
        assert_eq!(config.tier_for_position(75).unwrap().rank, 7);
        assert_eq!(config.tier_for_position(999).unwrap().rank, 9);
    }

    #[test]
    fn test_total_pool() {
        let config = PrizeConfig::default();
        assert_eq!(config.total_pool_zion, 1_000_000_000);
    }

    #[test]
    fn test_prize_ledger() {
        let mut ledger = PrizeLedger::new();
        ledger.award(PlayerPrize {
            address: "zion1winner".into(),
            raid_position: 1,
            tier_rank: 1,
            zion_awarded: 1_000_000_000,
            nft_reward: "Legendary".into(),
            awarded_at: 0,
            dao_approved: true,
        });
        assert_eq!(ledger.total_awarded_zion(), 1_000_000_000);
    }
}
