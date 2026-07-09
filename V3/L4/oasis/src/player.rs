//! Player Profile — identity, achievements, and statistics.

use crate::consciousness::ConsciousnessLevel;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Player profile in the OASIS world
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    /// ZION wallet address (L1 identity)
    pub address: String,
    /// Display name (optional)
    pub display_name: Option<String>,
    /// Current XP
    pub total_xp: u64,
    /// Current consciousness level
    pub level: ConsciousnessLevel,
    /// Guild membership (guild ID)
    pub guild_id: Option<String>,
    /// Total blocks mined
    pub blocks_mined: u64,
    /// Total ZION earned through OASIS
    pub zion_earned: u64,
    /// Achievements unlocked
    pub achievements: Vec<Achievement>,
    /// Humanitarian tithe given (total)
    pub tithe_total: u64,
    /// AI challenges completed
    pub challenges_completed: u32,
    /// Active streak (consecutive days)
    pub daily_streak: u32,
    /// Best daily streak ever
    pub best_streak: u32,
    /// Referrals count
    pub referrals: u32,
    /// Today's earned XP (for daily cap)
    pub daily_xp: u64,
    /// Last active timestamp (Unix seconds)
    pub last_active: u64,
    /// Registration timestamp (Unix seconds)
    pub created_at: u64,
    /// Custom stats / metadata
    pub stats: HashMap<String, u64>,
}

impl Player {
    /// Create a new player
    pub fn new(address: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            address,
            display_name: None,
            total_xp: 0,
            level: ConsciousnessLevel::Physical,
            guild_id: None,
            blocks_mined: 0,
            zion_earned: 0,
            achievements: Vec::new(),
            tithe_total: 0,
            challenges_completed: 0,
            daily_streak: 0,
            best_streak: 0,
            referrals: 0,
            daily_xp: 0,
            last_active: now,
            created_at: now,
            stats: HashMap::new(),
        }
    }

    /// Add XP to player, return true if leveled up
    pub fn add_xp(&mut self, amount: u64) -> bool {
        let old_level = self.level;
        self.total_xp += amount;
        self.daily_xp += amount;
        self.level = ConsciousnessLevel::from_xp(self.total_xp);
        self.level > old_level
    }

    /// Mark the player as active now. Updates the daily streak based on
    /// the time elapsed since `last_active`:
    ///   - < 24h since last activity → consecutive (streak +1 once per day)
    ///   - 24h–48h → streak continues (same day boundary)
    ///   - > 48h → streak reset to 1
    ///     > Also updates `last_active` to the current timestamp.
    pub fn touch(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.touch_at(now);
    }

    /// Same as `touch()` but with an explicit timestamp (for testing).
    pub fn touch_at(&mut self, now: u64) {
        let elapsed = now.saturating_sub(self.last_active);
        if self.last_active == 0 {
            // First ever activity
            self.daily_streak = 1;
            self.best_streak = self.best_streak.max(self.daily_streak);
        } else if elapsed < 86_400 {
            // Same day — no streak change
        } else if elapsed < 172_800 {
            // 1-2 days: consecutive day
            self.daily_streak += 1;
            self.best_streak = self.best_streak.max(self.daily_streak);
        } else {
            // > 2 days: streak broken
            self.daily_streak = 1;
        }
        self.last_active = now;
        self.check_achievement(AchievementType::Streak);
    }

    /// Record a mined block
    pub fn record_block(&mut self) {
        self.blocks_mined += 1;
        self.check_achievement(AchievementType::BlocksMined);
    }

    /// Check and award achievements
    fn check_achievement(&mut self, achievement_type: AchievementType) {
        let milestones = match achievement_type {
            AchievementType::BlocksMined => vec![1, 10, 100, 1000, 10_000, 100_000],
            AchievementType::Streak => vec![7, 30, 90, 365],
            AchievementType::Tithe => vec![1, 100, 1000, 10_000],
            AchievementType::Challenges => vec![1, 10, 50, 100, 500],
            AchievementType::Referrals => vec![1, 5, 25, 100],
        };

        let current = match achievement_type {
            AchievementType::BlocksMined => self.blocks_mined,
            AchievementType::Streak => self.daily_streak as u64,
            AchievementType::Tithe => self.tithe_total,
            AchievementType::Challenges => self.challenges_completed as u64,
            AchievementType::Referrals => self.referrals as u64,
        };

        for &milestone in &milestones {
            let id = format!("{:?}_{}", achievement_type, milestone);
            if current >= milestone && !self.achievements.iter().any(|a| a.id == id) {
                self.achievements.push(Achievement {
                    id,
                    achievement_type: achievement_type.clone(),
                    milestone,
                    earned_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                });
            }
        }
    }

    /// Reset daily XP (called at midnight UTC)
    pub fn reset_daily(&mut self) {
        self.daily_xp = 0;
    }

    /// Update streak
    pub fn update_streak(&mut self, is_consecutive: bool) {
        if is_consecutive {
            self.daily_streak += 1;
            if self.daily_streak > self.best_streak {
                self.best_streak = self.daily_streak;
            }
        } else {
            self.daily_streak = 1;
        }
        self.check_achievement(AchievementType::Streak);
    }
}

/// Achievement types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AchievementType {
    BlocksMined,
    Streak,
    Tithe,
    Challenges,
    Referrals,
}

/// Achievement instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Achievement {
    pub id: String,
    pub achievement_type: AchievementType,
    pub milestone: u64,
    pub earned_at: u64,
}

/// Player store — registry of all players
pub struct PlayerStore {
    players: HashMap<String, Player>,
}

impl Default for PlayerStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerStore {
    pub fn new() -> Self {
        Self {
            players: HashMap::new(),
        }
    }

    /// Register or get existing player
    pub fn get_or_create(&mut self, address: &str) -> &mut Player {
        self.players
            .entry(address.to_string())
            .or_insert_with(|| Player::new(address.to_string()))
    }

    /// Get player (read-only)
    pub fn get(&self, address: &str) -> Option<&Player> {
        self.players.get(address)
    }

    /// Total registered players
    pub fn total_players(&self) -> usize {
        self.players.len()
    }

    /// Top players by XP
    pub fn top_by_xp(&self, limit: usize) -> Vec<&Player> {
        let mut sorted: Vec<&Player> = self.players.values().collect();
        sorted.sort_by(|a, b| b.total_xp.cmp(&a.total_xp));
        sorted.truncate(limit);
        sorted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_player() {
        let player = Player::new("ZIONtest1234".to_string());
        assert_eq!(player.level, ConsciousnessLevel::Physical);
        assert_eq!(player.total_xp, 0);
    }

    #[test]
    fn test_add_xp_level_up() {
        let mut player = Player::new("ZIONtest1234".to_string());
        let leveled = player.add_xp(2000); // → Emotional (1000 threshold)
        assert!(leveled);
        assert_eq!(player.level, ConsciousnessLevel::Emotional);
    }

    #[test]
    fn test_achievement_on_first_block() {
        let mut player = Player::new("ZIONtest1234".to_string());
        player.record_block();
        assert_eq!(player.blocks_mined, 1);
        assert!(player.achievements.iter().any(|a| a.id == "BlocksMined_1"));
    }

    #[test]
    fn test_player_store() {
        let mut store = PlayerStore::new();
        store.get_or_create("addr1");
        store.get_or_create("addr2");
        assert_eq!(store.total_players(), 2);
    }

    #[test]
    fn test_streak_first_touch() {
        let mut player = Player::new("zion1streak".to_string());
        player.last_active = 0;
        player.touch_at(1_000_000);
        assert_eq!(player.daily_streak, 1);
        assert_eq!(player.best_streak, 1);
    }

    #[test]
    fn test_streak_same_day_no_change() {
        let mut player = Player::new("zion1streak".to_string());
        player.last_active = 1_000_000;
        player.daily_streak = 5;
        player.touch_at(1_000_000 + 3600); // 1 hour later
        assert_eq!(player.daily_streak, 5); // unchanged
    }

    #[test]
    fn test_streak_next_day_increments() {
        let mut player = Player::new("zion1streak".to_string());
        player.last_active = 1_000_000;
        player.daily_streak = 5;
        player.touch_at(1_000_000 + 86_400 + 1); // ~24h+1s later
        assert_eq!(player.daily_streak, 6);
        assert_eq!(player.best_streak, 6);
    }

    #[test]
    fn test_streak_breaks_after_48h() {
        let mut player = Player::new("zion1streak".to_string());
        player.last_active = 1_000_000;
        player.daily_streak = 10;
        player.best_streak = 10;
        player.touch_at(1_000_000 + 172_800 + 1); // ~48h+1s later
        assert_eq!(player.daily_streak, 1); // reset
        assert_eq!(player.best_streak, 10); // best unchanged
    }
}
