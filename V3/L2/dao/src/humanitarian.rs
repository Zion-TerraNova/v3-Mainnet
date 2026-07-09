//! Humanitarian Fund — DAO-governed categories for global good.
//!
//! ## 7 Categories (from WP2.9.5 / humanitarian_dao.py)
//!
//! 1. 💧 Water — Clean water access projects
//! 2. 🍞 Food — Food security & nutrition
//! 3. 🏠 Shelter — Housing & infrastructure
//! 4. 🌍 Environment — Reforestation, cleanup, sustainability
//! 5. 🏥 Medical — Healthcare access, medicine
//! 6. 📚 Education — Schools, training, literacy
//! 7. 🚨 Emergency — Disaster relief, crisis response
//!
//! ## Funding
//! - 1,440,000,000 ZION (genesis humanitarian fund)
//! - Ongoing: humanitarian DAO proposals from 4B DAO treasury

use serde::{Deserialize, Serialize};

use crate::types::FLOWERS_PER_ZION;

// ---------------------------------------------------------------------------
// Categories
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HumanitarianCategory {
    Water,
    Food,
    Shelter,
    Environment,
    Medical,
    Education,
    Emergency,
}

impl HumanitarianCategory {
    pub fn all() -> &'static [HumanitarianCategory] {
        &[
            HumanitarianCategory::Water,
            HumanitarianCategory::Food,
            HumanitarianCategory::Shelter,
            HumanitarianCategory::Environment,
            HumanitarianCategory::Medical,
            HumanitarianCategory::Education,
            HumanitarianCategory::Emergency,
        ]
    }

    pub fn emoji(&self) -> &str {
        match self {
            Self::Water => "💧",
            Self::Food => "🍞",
            Self::Shelter => "🏠",
            Self::Environment => "🌍",
            Self::Medical => "🏥",
            Self::Education => "📚",
            Self::Emergency => "🚨",
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Water => "water",
            Self::Food => "food",
            Self::Shelter => "shelter",
            Self::Environment => "environment",
            Self::Medical => "medical",
            Self::Education => "education",
            Self::Emergency => "emergency",
        }
    }
}

// ---------------------------------------------------------------------------
// Humanitarian Fund
// ---------------------------------------------------------------------------

/// Genesis humanitarian allocation: 1.44B ZION (u128 required at 6-decimal)
pub const HUMANITARIAN_GENESIS: u128 = 1_440_000_000_u128 * FLOWERS_PER_ZION as u128;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanitarianFund {
    /// Total allocated per category
    pub allocations: std::collections::HashMap<HumanitarianCategory, u64>,
    /// Total distributed per category
    pub distributed: std::collections::HashMap<HumanitarianCategory, u64>,
}

impl Default for HumanitarianFund {
    fn default() -> Self {
        Self::new()
    }
}

impl HumanitarianFund {
    pub fn new() -> Self {
        Self {
            allocations: std::collections::HashMap::new(),
            distributed: std::collections::HashMap::new(),
        }
    }

    /// Allocate funds to a category (from DAO proposal)
    pub fn allocate(&mut self, category: HumanitarianCategory, amount: u64) {
        *self.allocations.entry(category).or_insert(0) += amount;
    }

    /// Record a distribution
    pub fn distribute(&mut self, category: HumanitarianCategory, amount: u64) -> bool {
        let allocated = self.allocations.get(&category).copied().unwrap_or(0);
        let already_distributed = self.distributed.get(&category).copied().unwrap_or(0);

        if already_distributed + amount > allocated {
            return false; // exceeds allocation
        }

        *self.distributed.entry(category).or_insert(0) += amount;
        true
    }

    /// Remaining budget for a category
    pub fn remaining(&self, category: &HumanitarianCategory) -> u64 {
        let allocated = self.allocations.get(category).copied().unwrap_or(0);
        let distributed = self.distributed.get(category).copied().unwrap_or(0);
        allocated.saturating_sub(distributed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_categories() {
        assert_eq!(HumanitarianCategory::all().len(), 7);
        assert_eq!(HumanitarianCategory::Water.emoji(), "💧");
        assert_eq!(HumanitarianCategory::Emergency.name(), "emergency");
    }

    #[test]
    fn test_fund_allocation() {
        let mut fund = HumanitarianFund::new();
        fund.allocate(HumanitarianCategory::Water, 1_000_000);
        assert_eq!(fund.remaining(&HumanitarianCategory::Water), 1_000_000);

        assert!(fund.distribute(HumanitarianCategory::Water, 400_000));
        assert_eq!(fund.remaining(&HumanitarianCategory::Water), 600_000);

        // Can't distribute more than allocated
        assert!(!fund.distribute(HumanitarianCategory::Water, 700_000));
    }
}
