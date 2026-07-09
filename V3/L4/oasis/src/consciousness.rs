//! Consciousness Levels — 9 stages of awareness (Kabbalah Tree of Life inspired).
//!
//! Maps to 10 Sefirot: Malkuth → Keter
//! Each level unlocks new capabilities and higher mining multipliers.

use serde::{Deserialize, Serialize};

/// 9 consciousness levels — the soul's journey
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ConsciousnessLevel {
    /// Malkuth — foundation, basic awareness
    Physical = 1,
    /// Yesod — emotional intelligence, empathy
    Emotional = 2,
    /// Hod + Netzach — mental clarity, logic + intuition
    Mental = 3,
    /// Tiferet — heart center, creative insight
    Intuitional = 4,
    /// Gevurah + Chesed — spiritual discipline + compassion
    Spiritual = 5,
    /// Binah — cosmic understanding, synthesis
    Cosmic = 6,
    /// Chokmah — divine wisdom, transformation
    Divine = 7,
    /// Da'at — unity consciousness, seamless flow
    Unity = 8,
    /// Keter — crown, transcendence
    OnTheStar = 9,
}

impl ConsciousnessLevel {
    /// XP threshold to reach this level
    pub fn xp_threshold(&self) -> u64 {
        match self {
            Self::Physical => 0,
            Self::Emotional => 1_000,
            Self::Mental => 5_000,
            Self::Intuitional => 15_000,
            Self::Spiritual => 50_000,
            Self::Cosmic => 150_000,
            Self::Divine => 500_000,
            Self::Unity => 2_000_000,
            Self::OnTheStar => 10_000_000,
        }
    }

    /// Mining reward multiplier
    pub fn multiplier(&self) -> f64 {
        match self {
            Self::Physical => 1.0,
            Self::Emotional => 1.2,
            Self::Mental => 1.5,
            Self::Intuitional => 2.0,
            Self::Spiritual => 3.0,
            Self::Cosmic => 5.0,
            Self::Divine => 8.0,
            Self::Unity => 12.0,
            Self::OnTheStar => 15.0,
        }
    }

    /// Determine level from total XP
    pub fn from_xp(xp: u64) -> Self {
        if xp >= 10_000_000 {
            Self::OnTheStar
        } else if xp >= 2_000_000 {
            Self::Unity
        } else if xp >= 500_000 {
            Self::Divine
        } else if xp >= 150_000 {
            Self::Cosmic
        } else if xp >= 50_000 {
            Self::Spiritual
        } else if xp >= 15_000 {
            Self::Intuitional
        } else if xp >= 5_000 {
            Self::Mental
        } else if xp >= 1_000 {
            Self::Emotional
        } else {
            Self::Physical
        }
    }

    /// Human-readable name
    pub fn name(&self) -> &str {
        match self {
            Self::Physical => "Physical",
            Self::Emotional => "Emotional",
            Self::Mental => "Mental",
            Self::Intuitional => "Intuitional",
            Self::Spiritual => "Spiritual",
            Self::Cosmic => "Cosmic",
            Self::Divine => "Divine",
            Self::Unity => "Unity",
            Self::OnTheStar => "On The Star",
        }
    }

    /// Sefira name (Kabbalah)
    pub fn sefira(&self) -> &str {
        match self {
            Self::Physical => "Malkuth",
            Self::Emotional => "Yesod",
            Self::Mental => "Hod/Netzach",
            Self::Intuitional => "Tiferet",
            Self::Spiritual => "Gevurah/Chesed",
            Self::Cosmic => "Binah",
            Self::Divine => "Chokmah",
            Self::Unity => "Da'at",
            Self::OnTheStar => "Keter",
        }
    }

    /// All levels in order
    pub fn all() -> &'static [ConsciousnessLevel] {
        &[
            Self::Physical,
            Self::Emotional,
            Self::Mental,
            Self::Intuitional,
            Self::Spiritual,
            Self::Cosmic,
            Self::Divine,
            Self::Unity,
            Self::OnTheStar,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_levels() {
        assert_eq!(ConsciousnessLevel::all().len(), 9);
    }

    #[test]
    fn test_xp_thresholds_ascending() {
        let levels = ConsciousnessLevel::all();
        for i in 1..levels.len() {
            assert!(levels[i].xp_threshold() > levels[i - 1].xp_threshold());
        }
    }

    #[test]
    fn test_multipliers_ascending() {
        let levels = ConsciousnessLevel::all();
        for i in 1..levels.len() {
            assert!(levels[i].multiplier() > levels[i - 1].multiplier());
        }
    }

    #[test]
    fn test_from_xp() {
        assert_eq!(ConsciousnessLevel::from_xp(0), ConsciousnessLevel::Physical);
        assert_eq!(
            ConsciousnessLevel::from_xp(1_000),
            ConsciousnessLevel::Emotional
        );
        assert_eq!(
            ConsciousnessLevel::from_xp(4_999),
            ConsciousnessLevel::Emotional
        );
        assert_eq!(
            ConsciousnessLevel::from_xp(5_000),
            ConsciousnessLevel::Mental
        );
        assert_eq!(
            ConsciousnessLevel::from_xp(10_000_000),
            ConsciousnessLevel::OnTheStar
        );
    }

    #[test]
    fn test_sefira_mapping() {
        assert_eq!(ConsciousnessLevel::Physical.sefira(), "Malkuth");
        assert_eq!(ConsciousnessLevel::OnTheStar.sefira(), "Keter");
    }
}
