//! Level Progression System — unlocks, perks, and multiplier management.
//!
//! Each consciousness level unlocks new features in the OASIS world.

use crate::consciousness::ConsciousnessLevel;
use serde::{Deserialize, Serialize};

/// Feature unlock at a specific level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelUnlock {
    pub level: ConsciousnessLevel,
    pub features: Vec<Feature>,
}

/// Features unlockable through level progression
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Feature {
    /// Basic mining (always available)
    BasicMining,
    /// Guild membership
    JoinGuild,
    /// AI challenge participation
    AiChallenges,
    /// Create own guild
    CreateGuild,
    /// Territory claiming
    ClaimTerritory,
    /// Meditation bonus multiplier
    MeditationBonus,
    /// DAO voting rights
    DaoVoting,
    /// Humanitarian tithe proposals
    TitheProposals,
    /// AI agent creation
    CreateAiAgent,
    /// Guild wars participation
    GuildWars,
    /// Territory expansion
    ExpandTerritory,
    /// Mentor new players
    Mentorship,
    /// Cross-chain OASIS portals
    WarpPortals,
    /// Custom challenge creation
    CreateChallenges,
    /// Consciousness beacon (bonus to nearby miners)
    ConsciousnessBeacon,
}

/// Get features unlocked at a given level
pub fn unlocked_features(level: ConsciousnessLevel) -> Vec<Feature> {
    let mut features = vec![Feature::BasicMining]; // always available

    if level >= ConsciousnessLevel::Emotional {
        features.extend_from_slice(&[Feature::JoinGuild, Feature::AiChallenges]);
    }
    if level >= ConsciousnessLevel::Mental {
        features.extend_from_slice(&[Feature::CreateGuild, Feature::ClaimTerritory]);
    }
    if level >= ConsciousnessLevel::Intuitional {
        features.extend_from_slice(&[Feature::MeditationBonus, Feature::DaoVoting]);
    }
    if level >= ConsciousnessLevel::Spiritual {
        features.extend_from_slice(&[Feature::TitheProposals, Feature::CreateAiAgent]);
    }
    if level >= ConsciousnessLevel::Cosmic {
        features.extend_from_slice(&[Feature::GuildWars, Feature::ExpandTerritory]);
    }
    if level >= ConsciousnessLevel::Divine {
        features.extend_from_slice(&[Feature::Mentorship, Feature::WarpPortals]);
    }
    if level >= ConsciousnessLevel::Unity {
        features.extend_from_slice(&[Feature::CreateChallenges]);
    }
    if level >= ConsciousnessLevel::OnTheStar {
        features.push(Feature::ConsciousnessBeacon);
    }

    features
}

/// Check if a player at a given level has a specific feature
pub fn has_feature(level: ConsciousnessLevel, feature: &Feature) -> bool {
    unlocked_features(level).contains(feature)
}

/// Level-up reward information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelUpReward {
    pub new_level: ConsciousnessLevel,
    pub zion_bonus: u64,
    pub new_features: Vec<Feature>,
    pub title: String,
}

/// Calculate level-up reward
pub fn level_up_reward(
    old_level: ConsciousnessLevel,
    new_level: ConsciousnessLevel,
) -> LevelUpReward {
    let old_features = unlocked_features(old_level);
    let new_features: Vec<Feature> = unlocked_features(new_level)
        .into_iter()
        .filter(|f| !old_features.contains(f))
        .collect();

    let zion_bonus = match new_level {
        ConsciousnessLevel::Physical => 0,
        ConsciousnessLevel::Emotional => 100,
        ConsciousnessLevel::Mental => 500,
        ConsciousnessLevel::Intuitional => 2_000,
        ConsciousnessLevel::Spiritual => 10_000,
        ConsciousnessLevel::Cosmic => 50_000,
        ConsciousnessLevel::Divine => 200_000,
        ConsciousnessLevel::Unity => 1_000_000,
        ConsciousnessLevel::OnTheStar => 5_000_000,
    };

    let title = format!(
        "{} Awakened — {:?} ({})",
        new_level.sefira(),
        new_level,
        new_level.multiplier()
    );

    LevelUpReward {
        new_level,
        zion_bonus,
        new_features,
        title,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_physical_has_basic_mining() {
        assert!(has_feature(
            ConsciousnessLevel::Physical,
            &Feature::BasicMining
        ));
    }

    #[test]
    fn test_physical_no_guild() {
        assert!(!has_feature(
            ConsciousnessLevel::Physical,
            &Feature::JoinGuild
        ));
    }

    #[test]
    fn test_cosmic_has_all() {
        let features = unlocked_features(ConsciousnessLevel::Cosmic);
        assert!(features.contains(&Feature::GuildWars));
        assert!(features.contains(&Feature::BasicMining));
    }

    #[test]
    fn test_on_the_star_has_beacon() {
        let features = unlocked_features(ConsciousnessLevel::OnTheStar);
        assert!(features.contains(&Feature::ConsciousnessBeacon));
        assert!(features.contains(&Feature::BasicMining));
    }

    #[test]
    fn test_level_up_reward() {
        let reward = level_up_reward(ConsciousnessLevel::Physical, ConsciousnessLevel::Emotional);
        assert_eq!(reward.zion_bonus, 100);
        assert!(reward.new_features.contains(&Feature::JoinGuild));
        assert!(reward.new_features.contains(&Feature::AiChallenges));
    }
}
