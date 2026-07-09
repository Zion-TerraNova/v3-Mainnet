//! XP System — experience points tracking and awards.

use crate::consciousness::ConsciousnessLevel;
use serde::{Deserialize, Serialize};

/// XP award types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum XpSource {
    /// Mining a block
    BlockMined { block_height: u64, shares: u64 },
    /// Completing an AI challenge
    AiChallenge { challenge_id: String, score: f64 },
    /// Passing a knowledge quiz
    Quiz {
        topic: String,
        correct: u32,
        total: u32,
    },
    /// Meditation bonus (daily check-in)
    Meditation { duration_minutes: u32 },
    /// Humanitarian tithe contribution
    Tithe { category: String, amount: u64 },
    /// Guild quest completion
    GuildQuest { quest_id: String },
    /// Avatar quest completion
    AvatarQuest { quest_id: String, avatar_id: u16 },
    /// Referral bonus
    Referral { referred_address: String },
}

impl XpSource {
    /// Calculate XP award for this source
    pub fn xp_amount(&self) -> u64 {
        match self {
            XpSource::BlockMined { shares, .. } => shares.min(&100) * 10,
            XpSource::AiChallenge { score, .. } => (*score * 100.0) as u64,
            XpSource::Quiz { correct, total, .. } => {
                if *total == 0 {
                    return 0;
                }
                let pct = (*correct as f64 / *total as f64) * 100.0;
                pct as u64
            }
            XpSource::Meditation { duration_minutes } => *duration_minutes.min(&60) as u64 * 5,
            XpSource::Tithe { amount, .. } => (*amount / 1_000_000).min(500), // max 500 XP per tithe
            XpSource::GuildQuest { .. } => 200,
            XpSource::AvatarQuest { .. } => 500,
            XpSource::Referral { .. } => 50,
        }
    }
}

/// XP System — manages player XP
pub struct XpSystem {
    /// Daily XP cap (prevent farming)
    pub daily_cap: u64,
}

impl Default for XpSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl XpSystem {
    pub fn new() -> Self {
        Self {
            daily_cap: 10_000, // max 10K XP per day
        }
    }

    /// Award XP and check for level-up
    pub fn award(
        &self,
        current_xp: u64,
        current_level: ConsciousnessLevel,
        source: &XpSource,
        daily_earned: u64,
    ) -> XpAward {
        let base_amount = source.xp_amount();

        // Apply level multiplier (higher levels earn faster)
        let multiplied = (base_amount as f64 * current_level.multiplier()) as u64;

        // Check daily cap
        let capped = if daily_earned + multiplied > self.daily_cap {
            self.daily_cap.saturating_sub(daily_earned)
        } else {
            multiplied
        };

        let new_xp = current_xp + capped;
        let new_level = ConsciousnessLevel::from_xp(new_xp);
        let leveled_up = new_level > current_level;

        XpAward {
            base_amount,
            multiplied_amount: multiplied,
            actual_amount: capped,
            new_total_xp: new_xp,
            new_level,
            leveled_up,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XpAward {
    pub base_amount: u64,
    pub multiplied_amount: u64,
    pub actual_amount: u64,
    pub new_total_xp: u64,
    pub new_level: ConsciousnessLevel,
    pub leveled_up: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mining_xp() {
        let source = XpSource::BlockMined {
            block_height: 1000,
            shares: 50,
        };
        assert_eq!(source.xp_amount(), 500); // 50 shares × 10
    }

    #[test]
    fn test_level_up() {
        let system = XpSystem::new();
        let award = system.award(
            4_900, // close to Mental (5000)
            ConsciousnessLevel::Emotional,
            &XpSource::BlockMined {
                block_height: 1,
                shares: 10,
            },
            0,
        );

        // 10 shares × 10 = 100 base, × 1.2 (Emotional) = 120
        assert_eq!(award.base_amount, 100);
        assert_eq!(award.new_total_xp, 4_900 + 120);
        assert_eq!(award.new_level, ConsciousnessLevel::Mental);
        assert!(award.leveled_up);
    }

    #[test]
    fn test_daily_cap() {
        let system = XpSystem::new();
        let award = system.award(
            0,
            ConsciousnessLevel::Physical,
            &XpSource::BlockMined {
                block_height: 1,
                shares: 100,
            },
            9_900, // already earned 9900 today
        );

        // Would be 1000 but capped at 10000 - 9900 = 100
        assert_eq!(award.actual_amount, 100);
    }
}
