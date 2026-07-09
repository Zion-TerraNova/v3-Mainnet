//! Guild System — social mining cooperatives.
//!
//! Guilds allow miners to cooperate, share bonuses, and compete.
//! Minimum level: Emotional (L2) to join, Mental (L3) to create.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Minimum level to join a guild
pub const MIN_LEVEL_JOIN: u64 = 1_000; // Emotional XP threshold
/// Minimum level to create a guild
pub const MIN_LEVEL_CREATE: u64 = 5_000; // Mental XP threshold
/// Maximum guild members
pub const MAX_GUILD_SIZE: usize = 100;
/// Minimum members for guild quests
pub const MIN_QUEST_MEMBERS: usize = 5;

/// Guild structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guild {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Founder wallet address
    pub founder: String,
    /// Officer addresses (can manage members)
    pub officers: Vec<String>,
    /// Member addresses
    pub members: Vec<String>,
    /// Total guild XP (sum of member contributions)
    pub guild_xp: u64,
    /// Guild level (based on guild XP)
    pub guild_level: u32,
    /// Active guild quests
    pub active_quests: Vec<GuildQuest>,
    /// Completed quests count
    pub quests_completed: u32,
    /// Territories controlled by this guild
    pub territories: Vec<String>,
    /// Guild creation timestamp
    pub created_at: u64,
}

impl Guild {
    pub fn new(id: String, name: String, founder: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            id,
            name,
            description: String::new(),
            founder: founder.clone(),
            officers: vec![founder.clone()],
            members: vec![founder],
            guild_xp: 0,
            guild_level: 1,
            active_quests: Vec::new(),
            quests_completed: 0,
            territories: Vec::new(),
            created_at: now,
        }
    }

    /// Add a member to the guild
    pub fn add_member(&mut self, address: &str) -> Result<(), GuildError> {
        if self.members.len() >= MAX_GUILD_SIZE {
            return Err(GuildError::GuildFull);
        }
        if self.members.contains(&address.to_string()) {
            return Err(GuildError::AlreadyMember);
        }
        self.members.push(address.to_string());
        Ok(())
    }

    /// Remove a member from the guild
    pub fn remove_member(&mut self, address: &str) -> Result<(), GuildError> {
        if address == self.founder {
            return Err(GuildError::CannotRemoveFounder);
        }
        if let Some(pos) = self.members.iter().position(|m| m == address) {
            self.members.remove(pos);
            self.officers.retain(|o| o != address);
            Ok(())
        } else {
            Err(GuildError::NotMember)
        }
    }

    /// Promote a member to officer
    pub fn promote(&mut self, address: &str) -> Result<(), GuildError> {
        if !self.members.contains(&address.to_string()) {
            return Err(GuildError::NotMember);
        }
        if self.officers.contains(&address.to_string()) {
            return Err(GuildError::AlreadyOfficer);
        }
        self.officers.push(address.to_string());
        Ok(())
    }

    /// Contribute XP to the guild
    pub fn contribute_xp(&mut self, amount: u64) {
        self.guild_xp += amount;
        // Guild level every 10K XP
        self.guild_level = (self.guild_xp / 10_000 + 1).min(50) as u32;
    }

    /// Guild member count
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Can start a guild quest?
    pub fn can_quest(&self) -> bool {
        self.members.len() >= MIN_QUEST_MEMBERS
    }
}

/// Guild quest — cooperative challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildQuest {
    pub id: String,
    pub quest_type: QuestType,
    pub target: u64,
    pub progress: u64,
    pub reward_xp: u64,
    pub reward_zion: u64,
    pub participants: Vec<String>,
    pub deadline: u64,
    pub completed: bool,
}

/// Quest types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuestType {
    /// Mine X blocks collectively
    CollectiveMining { target_blocks: u64 },
    /// Complete X AI challenges as a guild
    AiChallengeSprint { target_challenges: u32 },
    /// Humanitarian tithe goal
    TitheGoal { target_amount: u64 },
    /// Hold a territory for X hours
    TerritoryDefense { territory_id: String, hours: u32 },
    /// Reach collective XP milestone
    XpMilestone { target_xp: u64 },
}

/// Guild errors
#[derive(Debug, Clone)]
pub enum GuildError {
    GuildFull,
    AlreadyMember,
    NotMember,
    CannotRemoveFounder,
    AlreadyOfficer,
    InsufficientLevel,
    QuestInProgress,
}

impl std::fmt::Display for GuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GuildFull => write!(f, "Guild is full (max {})", MAX_GUILD_SIZE),
            Self::AlreadyMember => write!(f, "Already a guild member"),
            Self::NotMember => write!(f, "Not a guild member"),
            Self::CannotRemoveFounder => write!(f, "Cannot remove guild founder"),
            Self::AlreadyOfficer => write!(f, "Already an officer"),
            Self::InsufficientLevel => write!(f, "Insufficient level"),
            Self::QuestInProgress => write!(f, "Quest already in progress"),
        }
    }
}

/// Guild registry
pub struct GuildRegistry {
    guilds: HashMap<String, Guild>,
}

impl Default for GuildRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl GuildRegistry {
    pub fn new() -> Self {
        Self {
            guilds: HashMap::new(),
        }
    }

    pub fn create_guild(
        &mut self,
        id: String,
        name: String,
        founder: String,
    ) -> Result<(), GuildError> {
        if self.guilds.contains_key(&id) {
            return Err(GuildError::AlreadyMember); // guild ID exists
        }
        self.guilds
            .insert(id.clone(), Guild::new(id, name, founder));
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Guild> {
        self.guilds.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Guild> {
        self.guilds.get_mut(id)
    }

    pub fn total_guilds(&self) -> usize {
        self.guilds.len()
    }

    /// Top guilds by XP
    pub fn top_guilds(&self, limit: usize) -> Vec<&Guild> {
        let mut sorted: Vec<&Guild> = self.guilds.values().collect();
        sorted.sort_by(|a, b| b.guild_xp.cmp(&a.guild_xp));
        sorted.truncate(limit);
        sorted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_guild() {
        let guild = Guild::new("g1".into(), "TestGuild".into(), "founder1".into());
        assert_eq!(guild.member_count(), 1);
        assert_eq!(guild.guild_level, 1);
    }

    #[test]
    fn test_add_remove_member() {
        let mut guild = Guild::new("g1".into(), "TestGuild".into(), "founder1".into());
        guild.add_member("member2").unwrap();
        assert_eq!(guild.member_count(), 2);
        guild.remove_member("member2").unwrap();
        assert_eq!(guild.member_count(), 1);
    }

    #[test]
    fn test_cannot_remove_founder() {
        let mut guild = Guild::new("g1".into(), "TestGuild".into(), "founder1".into());
        assert!(guild.remove_member("founder1").is_err());
    }

    #[test]
    fn test_guild_level_progression() {
        let mut guild = Guild::new("g1".into(), "TestGuild".into(), "founder1".into());
        guild.contribute_xp(30_000);
        assert_eq!(guild.guild_level, 4); // 30000 / 10000 + 1 = 4
    }
}
