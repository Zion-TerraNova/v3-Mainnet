//! Humanitarian Tithe — OASIS player contributions to real-world causes.
//!
//! Mirrors the DAO humanitarian fund categories but from the player side.
//! Players voluntarily tithe a portion of their OASIS earnings.
//!
//! ⚠️ LAYER BOUNDARY: This module tracks player tithes.
//! Actual fund management happens in the DAO crate (L2).
//! Communication via L1 TX memo: "TITHE:category:amount:player_addr"

use serde::{Deserialize, Serialize};

/// Tithe categories (mirror dao/humanitarian.rs)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TitheCategory {
    /// Clean water projects
    Water,
    /// Food security
    Food,
    /// Shelter & housing
    Shelter,
    /// Environmental protection
    Environment,
    /// Medical aid
    Medical,
    /// Education access
    Education,
    /// Emergency relief
    Emergency,
}

impl TitheCategory {
    pub fn all() -> Vec<TitheCategory> {
        vec![
            TitheCategory::Water,
            TitheCategory::Food,
            TitheCategory::Shelter,
            TitheCategory::Environment,
            TitheCategory::Medical,
            TitheCategory::Education,
            TitheCategory::Emergency,
        ]
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            TitheCategory::Water => "💧",
            TitheCategory::Food => "🍞",
            TitheCategory::Shelter => "🏠",
            TitheCategory::Environment => "🌍",
            TitheCategory::Medical => "🏥",
            TitheCategory::Education => "📚",
            TitheCategory::Emergency => "🚨",
        }
    }
}

/// A player's tithe contribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitheContribution {
    pub player_address: String,
    pub category: TitheCategory,
    pub amount: u64,
    pub timestamp: u64,
    /// L1 TX hash confirming the tithe
    pub tx_hash: Option<String>,
}

/// Player tithe tracker
pub struct TitheTracker {
    contributions: Vec<TitheContribution>,
}

impl Default for TitheTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl TitheTracker {
    pub fn new() -> Self {
        Self {
            contributions: Vec::new(),
        }
    }

    /// Record a tithe contribution
    pub fn contribute(
        &mut self,
        player_address: &str,
        category: TitheCategory,
        amount: u64,
    ) -> TitheContribution {
        let contribution = TitheContribution {
            player_address: player_address.to_string(),
            category,
            amount,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            tx_hash: None,
        };
        self.contributions.push(contribution.clone());
        contribution
    }

    /// Build L1 TX memo for this tithe
    pub fn build_memo(category: TitheCategory, amount: u64, player: &str) -> String {
        format!("TITHE:{:?}:{}:{}", category, amount, player)
    }

    /// Total tithes by a player
    pub fn player_total(&self, address: &str) -> u64 {
        self.contributions
            .iter()
            .filter(|c| c.player_address == address)
            .map(|c| c.amount)
            .sum()
    }

    /// Total tithes by category
    pub fn category_total(&self, category: TitheCategory) -> u64 {
        self.contributions
            .iter()
            .filter(|c| c.category == category)
            .map(|c| c.amount)
            .sum()
    }

    /// Grand total of all tithes
    pub fn grand_total(&self) -> u64 {
        self.contributions.iter().map(|c| c.amount).sum()
    }

    /// Top tithers
    pub fn top_tithers(&self, limit: usize) -> Vec<(String, u64)> {
        use std::collections::HashMap;
        let mut totals: HashMap<String, u64> = HashMap::new();
        for c in &self.contributions {
            *totals.entry(c.player_address.clone()).or_default() += c.amount;
        }
        let mut sorted: Vec<(String, u64)> = totals.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        sorted.truncate(limit);
        sorted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contribute() {
        let mut tracker = TitheTracker::new();
        tracker.contribute("addr1", TitheCategory::Water, 1000);
        tracker.contribute("addr1", TitheCategory::Food, 500);
        tracker.contribute("addr2", TitheCategory::Water, 2000);

        assert_eq!(tracker.player_total("addr1"), 1500);
        assert_eq!(tracker.category_total(TitheCategory::Water), 3000);
        assert_eq!(tracker.grand_total(), 3500);
    }

    #[test]
    fn test_memo_format() {
        let memo = TitheTracker::build_memo(TitheCategory::Education, 500, "ZIONaddr1");
        assert!(memo.starts_with("TITHE:Education:500:"));
    }

    #[test]
    fn test_top_tithers() {
        let mut tracker = TitheTracker::new();
        tracker.contribute("alice", TitheCategory::Water, 5000);
        tracker.contribute("bob", TitheCategory::Food, 3000);
        tracker.contribute("alice", TitheCategory::Medical, 2000);

        let top = tracker.top_tithers(10);
        assert_eq!(top[0].0, "alice");
        assert_eq!(top[0].1, 7000);
    }
}
