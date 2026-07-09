//! Raid Team System — 40-player raid groups for Golden Egg endgame.
//!
//! EKAM Dimension raid requires coordinated teams of up to 40 players
//! to defeat 108 mini-bosses and the final Hiranyagarbha boss.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Maximum players in a raid team
pub const MAX_RAID_SIZE: usize = 40;
/// Minimum players to start a raid
pub const MIN_RAID_SIZE: usize = 10;
/// Number of pillars (mini-bosses) in EKAM Dimension
pub const EKAM_PILLARS: u32 = 108;

/// Raid team composition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaidTeam {
    pub id: String,
    pub name: String,
    pub leader_address: String,
    pub members: Vec<RaidMember>,
    pub status: RaidStatus,
    pub current_pillar: u32,
    pub pillars_defeated: u32,
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    pub total_attempts: u32,
    pub fastest_pillar_clear_secs: Option<u64>,
    pub created_at: u64,
}

/// Individual raid member
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaidMember {
    pub address: String,
    pub display_name: Option<String>,
    pub consciousness_level: u8,
    pub role: RaidRole,
    pub joined_at: u64,
    pub damage_dealt: u64,
    pub healing_done: u64,
    pub deaths: u32,
}

/// Raid role classes
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum RaidRole {
    Tank,
    Healer,
    Dps,
    Support,
}

impl std::fmt::Display for RaidRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RaidRole::Tank => write!(f, "Tank"),
            RaidRole::Healer => write!(f, "Healer"),
            RaidRole::Dps => write!(f, "DPS"),
            RaidRole::Support => write!(f, "Support"),
        }
    }
}

/// Raid lifecycle status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum RaidStatus {
    Recruiting,
    InProgress,
    Completed,
    Failed,
    Disbanded,
}

impl RaidTeam {
    pub fn new(id: String, name: String, leader_address: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            id,
            name,
            leader_address,
            members: Vec::new(),
            status: RaidStatus::Recruiting,
            current_pillar: 0,
            pillars_defeated: 0,
            start_time: None,
            end_time: None,
            total_attempts: 0,
            fastest_pillar_clear_secs: None,
            created_at: now,
        }
    }

    /// Add a member to the raid
    pub fn add_member(&mut self, member: RaidMember) -> Result<(), RaidError> {
        if self.members.len() >= MAX_RAID_SIZE {
            return Err(RaidError::RaidFull);
        }
        if self.members.iter().any(|m| m.address == member.address) {
            return Err(RaidError::AlreadyMember);
        }
        if self.status != RaidStatus::Recruiting {
            return Err(RaidError::NotRecruiting);
        }
        self.members.push(member);
        Ok(())
    }

    /// Remove a member from the raid
    pub fn remove_member(&mut self, address: &str) -> Result<(), RaidError> {
        if address == self.leader_address {
            return Err(RaidError::CannotRemoveLeader);
        }
        let pos = self.members.iter().position(|m| m.address == address);
        match pos {
            Some(i) => {
                self.members.remove(i);
                Ok(())
            }
            None => Err(RaidError::NotMember),
        }
    }

    /// Start the raid
    pub fn start(&mut self) -> Result<(), RaidError> {
        if self.members.len() < MIN_RAID_SIZE {
            return Err(RaidError::InsufficientMembers);
        }
        if self.status != RaidStatus::Recruiting {
            return Err(RaidError::AlreadyStarted);
        }
        self.status = RaidStatus::InProgress;
        self.start_time = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
        self.current_pillar = 1;
        Ok(())
    }

    /// Defeat a pillar (mini-boss)
    pub fn defeat_pillar(&mut self, clear_time_secs: u64) -> Result<bool, RaidError> {
        if self.status != RaidStatus::InProgress {
            return Err(RaidError::NotInProgress);
        }
        self.pillars_defeated += 1;
        self.current_pillar += 1;

        // Track fastest clear
        if let Some(fastest) = self.fastest_pillar_clear_secs {
            if clear_time_secs < fastest {
                self.fastest_pillar_clear_secs = Some(clear_time_secs);
            }
        } else {
            self.fastest_pillar_clear_secs = Some(clear_time_secs);
        }

        // Check if all 108 pillars defeated
        if self.pillars_defeated >= EKAM_PILLARS {
            self.complete_raid();
            return Ok(true); // Raid complete!
        }
        Ok(false)
    }

    /// Complete the raid (all 108 pillars defeated)
    fn complete_raid(&mut self) {
        self.status = RaidStatus::Completed;
        self.end_time = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
    }

    /// Calculate total raid duration in seconds
    pub fn duration_secs(&self) -> Option<u64> {
        match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => Some(end - start),
            (Some(start), None) => Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    - start,
            ),
            _ => None,
        }
    }

    /// Average consciousness level of the team
    pub fn avg_consciousness_level(&self) -> f64 {
        if self.members.is_empty() {
            return 0.0;
        }
        let sum: u32 = self
            .members
            .iter()
            .map(|m| m.consciousness_level as u32)
            .sum();
        sum as f64 / self.members.len() as f64
    }

    /// Total damage dealt by the team
    pub fn total_damage(&self) -> u64 {
        self.members.iter().map(|m| m.damage_dealt).sum()
    }
}

/// Raid error types
#[derive(Debug, Clone, PartialEq)]
pub enum RaidError {
    RaidFull,
    AlreadyMember,
    NotMember,
    CannotRemoveLeader,
    NotRecruiting,
    AlreadyStarted,
    NotInProgress,
    InsufficientMembers,
    InvalidRole,
}

impl std::fmt::Display for RaidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RaidFull => write!(f, "Raid is full (max {} members)", MAX_RAID_SIZE),
            Self::AlreadyMember => write!(f, "Already a member of this raid"),
            Self::NotMember => write!(f, "Not a member of this raid"),
            Self::CannotRemoveLeader => write!(f, "Cannot remove raid leader"),
            Self::NotRecruiting => write!(f, "Raid is not recruiting"),
            Self::AlreadyStarted => write!(f, "Raid has already started"),
            Self::NotInProgress => write!(f, "Raid is not in progress"),
            Self::InsufficientMembers => {
                write!(f, "Need at least {} members to start", MIN_RAID_SIZE)
            }
            Self::InvalidRole => write!(f, "Invalid raid role"),
        }
    }
}

/// Raid registry — global tracking of all raids
pub struct RaidRegistry {
    raids: HashMap<String, RaidTeam>,
}

impl Default for RaidRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RaidRegistry {
    pub fn new() -> Self {
        Self {
            raids: HashMap::new(),
        }
    }

    pub fn create(&mut self, id: String, name: String, leader: String) -> &mut RaidTeam {
        self.raids
            .insert(id.clone(), RaidTeam::new(id.clone(), name, leader));
        self.raids.get_mut(&id).unwrap()
    }

    pub fn get(&self, id: &str) -> Option<&RaidTeam> {
        self.raids.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut RaidTeam> {
        self.raids.get_mut(id)
    }

    pub fn list_active(&self) -> Vec<&RaidTeam> {
        self.raids
            .values()
            .filter(|r| r.status == RaidStatus::Recruiting || r.status == RaidStatus::InProgress)
            .collect()
    }

    pub fn list_completed(&self) -> Vec<&RaidTeam> {
        self.raids
            .values()
            .filter(|r| r.status == RaidStatus::Completed)
            .collect()
    }

    /// Global leaderboard by fastest raid completion (tie-breaker: total damage desc)
    pub fn leaderboard_by_time(&self, limit: usize) -> Vec<&RaidTeam> {
        let mut completed: Vec<&RaidTeam> = self.list_completed();
        completed.sort_by(|a, b| {
            let a_time = a.duration_secs().unwrap_or(u64::MAX);
            let b_time = b.duration_secs().unwrap_or(u64::MAX);
            a_time
                .cmp(&b_time)
                .then_with(|| b.total_damage().cmp(&a.total_damage()))
        });
        completed.truncate(limit);
        completed
    }

    /// Global leaderboard by total damage
    pub fn leaderboard_by_damage(&self, limit: usize) -> Vec<&RaidTeam> {
        let mut completed: Vec<&RaidTeam> = self.list_completed();
        completed.sort_by_key(|b| std::cmp::Reverse(b.total_damage()));
        completed.truncate(limit);
        completed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_member(addr: &str, level: u8, role: RaidRole) -> RaidMember {
        RaidMember {
            address: addr.into(),
            display_name: None,
            consciousness_level: level,
            role,
            joined_at: 0,
            damage_dealt: 0,
            healing_done: 0,
            deaths: 0,
        }
    }

    #[test]
    fn test_create_raid() {
        let raid = RaidTeam::new("r1".into(), "Golden Egg Raiders".into(), "leader1".into());
        assert_eq!(raid.status, RaidStatus::Recruiting);
        assert_eq!(raid.members.len(), 0);
    }

    #[test]
    fn test_add_member() {
        let mut raid = RaidTeam::new("r1".into(), "Test".into(), "leader1".into());
        raid.add_member(test_member("m1", 5, RaidRole::Tank))
            .unwrap();
        assert_eq!(raid.members.len(), 1);
    }

    #[test]
    fn test_cannot_start_without_min_members() {
        let mut raid = RaidTeam::new("r1".into(), "Test".into(), "leader1".into());
        assert!(raid.start().is_err());
    }

    #[test]
    fn test_start_raid() {
        let mut raid = RaidTeam::new("r1".into(), "Test".into(), "leader1".into());
        for i in 0..MIN_RAID_SIZE {
            raid.add_member(test_member(&format!("m{}", i), 5, RaidRole::Dps))
                .unwrap();
        }
        assert!(raid.start().is_ok());
        assert_eq!(raid.status, RaidStatus::InProgress);
        assert_eq!(raid.current_pillar, 1);
    }

    #[test]
    fn test_defeat_pillars() {
        let mut raid = RaidTeam::new("r1".into(), "Test".into(), "leader1".into());
        for i in 0..MIN_RAID_SIZE {
            raid.add_member(test_member(&format!("m{}", i), 5, RaidRole::Dps))
                .unwrap();
        }
        raid.start().unwrap();

        // Defeat 107 pillars
        for _ in 0..107 {
            raid.defeat_pillar(120).unwrap();
        }
        assert_eq!(raid.status, RaidStatus::InProgress);

        // 108th pillar - raid completes
        let completed = raid.defeat_pillar(120).unwrap();
        assert!(completed);
        assert_eq!(raid.status, RaidStatus::Completed);
    }

    #[test]
    fn test_raid_registry_leaderboard() {
        let mut reg = RaidRegistry::new();
        reg.create("r1".into(), "Fast".into(), "l1".into());
        reg.create("r2".into(), "Slow".into(), "l2".into());

        // Complete raids with different damages
        {
            let r1 = reg.get_mut("r1").unwrap();
            for i in 0..MIN_RAID_SIZE {
                let mut m = test_member(&format!("m{}", i), 5, RaidRole::Dps);
                m.damage_dealt = 1000;
                r1.add_member(m).unwrap();
            }
            r1.start().unwrap();
            for _ in 0..EKAM_PILLARS {
                r1.defeat_pillar(60).unwrap();
            }
        }
        {
            let r2 = reg.get_mut("r2").unwrap();
            for i in 0..MIN_RAID_SIZE {
                let mut m = test_member(&format!("m{}", i), 5, RaidRole::Dps);
                m.damage_dealt = 500;
                r2.add_member(m).unwrap();
            }
            r2.start().unwrap();
            for _ in 0..EKAM_PILLARS {
                r2.defeat_pillar(120).unwrap();
            }
        }

        let by_time = reg.leaderboard_by_time(10);
        assert_eq!(by_time[0].id, "r1"); // faster

        let by_damage = reg.leaderboard_by_damage(10);
        assert_eq!(by_damage[0].id, "r1"); // more damage
    }
}
