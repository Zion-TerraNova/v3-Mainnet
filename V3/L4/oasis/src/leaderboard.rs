//! Leaderboard — global and guild rankings.

use serde::{Deserialize, Serialize};

/// Leaderboard entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub rank: u32,
    pub address: String,
    pub display_name: Option<String>,
    pub value: u64,
    pub level: String,
    pub guild_name: Option<String>,
}

/// Leaderboard types
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum LeaderboardType {
    /// Top players by XP
    GlobalXp,
    /// Top players by blocks mined
    BlocksMined,
    /// Top players by humanitarian tithe
    TopTithers,
    /// Top guilds by XP
    GuildXp,
    /// Top guilds by territory count
    GuildTerritories,
    /// Most challenges completed
    Challenges,
    /// Longest daily streak
    LongestStreak,
}

/// Leaderboard data (cached snapshot)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Leaderboard {
    pub leaderboard_type: LeaderboardType,
    pub entries: Vec<LeaderboardEntry>,
    pub total_participants: u64,
    /// Snapshot timestamp
    pub updated_at: u64,
}

impl Leaderboard {
    pub fn new(leaderboard_type: LeaderboardType) -> Self {
        Self {
            leaderboard_type,
            entries: Vec::new(),
            total_participants: 0,
            updated_at: 0,
        }
    }

    /// Update leaderboard with sorted entries
    pub fn update(&mut self, mut entries: Vec<LeaderboardEntry>) {
        // Assign ranks
        for (i, entry) in entries.iter_mut().enumerate() {
            entry.rank = (i + 1) as u32;
        }
        self.entries = entries;
        self.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }

    /// Get top N entries
    pub fn top(&self, n: usize) -> &[LeaderboardEntry] {
        let end = n.min(self.entries.len());
        &self.entries[..end]
    }

    /// Find a player's rank
    pub fn find_rank(&self, address: &str) -> Option<u32> {
        self.entries
            .iter()
            .find(|e| e.address == address)
            .map(|e| e.rank)
    }
}

/// Leaderboard manager
pub struct LeaderboardManager {
    leaderboards: Vec<Leaderboard>,
}

impl Default for LeaderboardManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LeaderboardManager {
    pub fn new() -> Self {
        let types = vec![
            LeaderboardType::GlobalXp,
            LeaderboardType::BlocksMined,
            LeaderboardType::TopTithers,
            LeaderboardType::GuildXp,
            LeaderboardType::GuildTerritories,
            LeaderboardType::Challenges,
            LeaderboardType::LongestStreak,
        ];

        Self {
            leaderboards: types.into_iter().map(Leaderboard::new).collect(),
        }
    }

    /// Get a specific leaderboard
    pub fn get(&self, lb_type: LeaderboardType) -> Option<&Leaderboard> {
        self.leaderboards
            .iter()
            .find(|lb| lb.leaderboard_type == lb_type)
    }

    /// Get a mutable leaderboard for updating
    pub fn get_mut(&mut self, lb_type: LeaderboardType) -> Option<&mut Leaderboard> {
        self.leaderboards
            .iter_mut()
            .find(|lb| lb.leaderboard_type == lb_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leaderboard_update() {
        let mut lb = Leaderboard::new(LeaderboardType::GlobalXp);
        let entries = vec![
            LeaderboardEntry {
                rank: 0,
                address: "addr1".into(),
                display_name: None,
                value: 5000,
                level: "Mental".into(),
                guild_name: None,
            },
            LeaderboardEntry {
                rank: 0,
                address: "addr2".into(),
                display_name: None,
                value: 3000,
                level: "Emotional".into(),
                guild_name: None,
            },
        ];
        lb.update(entries);
        assert_eq!(lb.entries[0].rank, 1);
        assert_eq!(lb.entries[1].rank, 2);
    }

    #[test]
    fn test_find_rank() {
        let mut lb = Leaderboard::new(LeaderboardType::GlobalXp);
        let entries = vec![LeaderboardEntry {
            rank: 0,
            address: "alice".into(),
            display_name: None,
            value: 100,
            level: "Physical".into(),
            guild_name: None,
        }];
        lb.update(entries);
        assert_eq!(lb.find_rank("alice"), Some(1));
        assert_eq!(lb.find_rank("bob"), None);
    }

    #[test]
    fn test_manager_has_all_types() {
        let manager = LeaderboardManager::new();
        assert!(manager.get(LeaderboardType::GlobalXp).is_some());
        assert!(manager.get(LeaderboardType::LongestStreak).is_some());
    }
}
