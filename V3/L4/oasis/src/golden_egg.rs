//! Golden Egg Engine — Treasure Hunt: 108 clues, 3 Master Keys, Hiranyagarbha.
//!
//! Tracks player progress through the Golden Egg quest:
//! - Clue discovery (108 total)
//! - Master Key assembly (Ramayana, Mahabharata, Unity)
//! - Consciousness level gating
//! - DAO approval for final prize

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Total clues in the Golden Egg hunt
pub const TOTAL_CLUES: u32 = 108;
/// Clues needed for Ramayana Key
pub const RAMAYANA_CLUES: u32 = 30;
/// Clues needed for Mahabharata Key
pub const MAHABHARATA_CLUES: u32 = 35;
/// Clues needed for Unity Key
pub const UNITY_CLUES: u32 = 43;

/// Master Key types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MasterKey {
    Ramayana,    // Dharma Path - 30 clues
    Mahabharata, // Karma Path - 35 clues
    Unity,       // Moksha Path - 43 clues (requires both previous)
}

impl MasterKey {
    pub fn clues_required(&self) -> u32 {
        match self {
            Self::Ramayana => RAMAYANA_CLUES,
            Self::Mahabharata => MAHABHARATA_CLUES,
            Self::Unity => UNITY_CLUES,
        }
    }

    pub fn min_consciousness_level(&self) -> u8 {
        match self {
            Self::Ramayana => 4,
            Self::Mahabharata => 6,
            Self::Unity => 7,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Ramayana => "Ramayana Key",
            Self::Mahabharata => "Mahabharata Key",
            Self::Unity => "Unity Key",
        }
    }

    pub fn sanskrit_name(&self) -> &'static str {
        match self {
            Self::Ramayana => "Rama Kuñjī",
            Self::Mahabharata => "Karma Kuñjī",
            Self::Unity => "Mokṣa Kuñjī",
        }
    }
}

/// Individual clue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clue {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub category: ClueCategory,
    pub source: String,
    pub min_cl: u8,
    pub solved: bool,
    pub discovered_at: Option<u64>,
    pub solution_hash: Option<String>, // Hash of correct answer (anti-spoiler)
}

/// Clue categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClueCategory {
    SacredTrinity,
    SacredKnowledge,
    Whitepaper,
    SourceCode,
    Blockchain,
    Community,
    EkamTemple,
}

/// Player's Golden Egg progress
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoldenEggProgress {
    pub address: String,
    pub clues_discovered: HashSet<u32>,
    pub keys_unlocked: HashSet<MasterKey>,
    pub current_cl: u8,
    pub ramayana_progress: u32,
    pub mahabharata_progress: u32,
    pub unity_progress: u32,
    pub final_test_submitted: bool,
    pub final_test_passed: bool,
    pub dao_voted: bool,
    pub dao_vote_percent: f64,
    pub started_at: u64,
    pub last_activity_at: u64,
}

impl GoldenEggProgress {
    pub fn new(address: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            address,
            clues_discovered: HashSet::new(),
            keys_unlocked: HashSet::new(),
            current_cl: 1,
            ramayana_progress: 0,
            mahabharata_progress: 0,
            unity_progress: 0,
            final_test_submitted: false,
            final_test_passed: false,
            dao_voted: false,
            dao_vote_percent: 0.0,
            started_at: now,
            last_activity_at: now,
        }
    }

    /// Discover a new clue
    pub fn discover_clue(&mut self, clue_id: u32, clue_min_cl: u8) -> Result<(), GoldenEggError> {
        if self.current_cl < clue_min_cl {
            return Err(GoldenEggError::InsufficientConsciousnessLevel {
                required: clue_min_cl,
                current: self.current_cl,
            });
        }
        self.clues_discovered.insert(clue_id);
        self.last_activity_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Auto-progress key assembly
        self.update_key_progress();
        Ok(())
    }

    /// Update key progress based on discovered clues
    fn update_key_progress(&mut self) {
        let ramayana_ids: HashSet<u32> = (1..=30).collect();
        let mahabharata_ids: HashSet<u32> = (31..=65).collect();
        let unity_ids: HashSet<u32> = (66..=108).collect();

        self.ramayana_progress = self.clues_discovered.intersection(&ramayana_ids).count() as u32;
        self.mahabharata_progress =
            self.clues_discovered.intersection(&mahabharata_ids).count() as u32;
        self.unity_progress = self.clues_discovered.intersection(&unity_ids).count() as u32;

        // Auto-unlock keys if requirements met
        if self.ramayana_progress >= RAMAYANA_CLUES && self.current_cl >= 4 {
            self.keys_unlocked.insert(MasterKey::Ramayana);
        }
        if self.mahabharata_progress >= MAHABHARATA_CLUES && self.current_cl >= 6 {
            self.keys_unlocked.insert(MasterKey::Mahabharata);
        }
        if self.unity_progress >= UNITY_CLUES
            && self.current_cl >= 7
            && self.keys_unlocked.contains(&MasterKey::Ramayana)
            && self.keys_unlocked.contains(&MasterKey::Mahabharata)
        {
            self.keys_unlocked.insert(MasterKey::Unity);
        }
    }

    /// Check if all 3 keys are unlocked
    pub fn has_all_keys(&self) -> bool {
        self.keys_unlocked.contains(&MasterKey::Ramayana)
            && self.keys_unlocked.contains(&MasterKey::Mahabharata)
            && self.keys_unlocked.contains(&MasterKey::Unity)
    }

    /// Total clues discovered
    pub fn total_clues(&self) -> usize {
        self.clues_discovered.len()
    }

    /// Percentage complete
    pub fn completion_percent(&self) -> f64 {
        (self.total_clues() as f64 / TOTAL_CLUES as f64) * 100.0
    }

    /// Can attempt the final EKAM raid?
    pub fn can_enter_ekam(&self) -> bool {
        self.has_all_keys() && self.current_cl >= 9
    }

    /// Submit the final consciousness test answer
    pub fn submit_final_test(&mut self, donate_100_percent: bool) -> Result<bool, GoldenEggError> {
        if !self.can_enter_ekam() {
            return Err(GoldenEggError::NotReadyForFinalTest);
        }
        self.final_test_submitted = true;
        // Only passes if they choose to donate 100%
        self.final_test_passed = donate_100_percent;
        Ok(self.final_test_passed)
    }
}

/// Golden Egg errors
#[derive(Debug, Clone, PartialEq)]
pub enum GoldenEggError {
    InsufficientConsciousnessLevel { required: u8, current: u8 },
    ClueAlreadyDiscovered,
    KeyAlreadyUnlocked,
    NotReadyForFinalTest,
    FinalTestFailed,
    DaoApprovalPending,
    InvalidClueId,
}

impl std::fmt::Display for GoldenEggError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientConsciousnessLevel { required, current } => {
                write!(
                    f,
                    "Need CL {} to discover this clue (current: {})",
                    required, current
                )
            }
            Self::ClueAlreadyDiscovered => write!(f, "Clue already discovered"),
            Self::KeyAlreadyUnlocked => write!(f, "Key already unlocked"),
            Self::NotReadyForFinalTest => {
                write!(f, "Not ready for final test (need all 3 keys + CL 9)")
            }
            Self::FinalTestFailed => {
                write!(f, "Final test failed - the answer requires 100% donation")
            }
            Self::DaoApprovalPending => write!(f, "DAO approval still pending"),
            Self::InvalidClueId => write!(f, "Invalid clue ID"),
        }
    }
}

/// Global Golden Egg tracker
pub struct GoldenEggTracker {
    players: HashMap<String, GoldenEggProgress>,
    pub total_solvers: u64,
    pub first_solver_address: Option<String>,
}

impl Default for GoldenEggTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl GoldenEggTracker {
    pub fn new() -> Self {
        Self {
            players: HashMap::new(),
            total_solvers: 0,
            first_solver_address: None,
        }
    }

    pub fn get_or_create(&mut self, address: &str) -> &mut GoldenEggProgress {
        self.players
            .entry(address.to_string())
            .or_insert_with(|| GoldenEggProgress::new(address.to_string()))
    }

    pub fn get(&self, address: &str) -> Option<&GoldenEggProgress> {
        self.players.get(address)
    }

    /// Global leaderboard by most clues discovered
    pub fn leaderboard_by_clues(&self, limit: usize) -> Vec<&GoldenEggProgress> {
        let mut sorted: Vec<&GoldenEggProgress> = self.players.values().collect();
        sorted.sort_by_key(|b| std::cmp::Reverse(b.total_clues()));
        sorted.truncate(limit);
        sorted
    }

    /// Global leaderboard by completion percentage
    pub fn leaderboard_by_completion(&self, limit: usize) -> Vec<&GoldenEggProgress> {
        let mut sorted: Vec<&GoldenEggProgress> = self.players.values().collect();
        sorted.sort_by(|a, b| {
            b.completion_percent()
                .partial_cmp(&a.completion_percent())
                .unwrap()
        });
        sorted.truncate(limit);
        sorted
    }

    /// Players who have all 3 keys (potential solvers)
    pub fn potential_solvers(&self) -> Vec<&GoldenEggProgress> {
        self.players.values().filter(|p| p.has_all_keys()).collect()
    }

    /// Total players participating
    pub fn total_participants(&self) -> usize {
        self.players.len()
    }

    /// Average completion across all players
    pub fn avg_completion_percent(&self) -> f64 {
        if self.players.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.players.values().map(|p| p.completion_percent()).sum();
        sum / self.players.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_progress() {
        let p = GoldenEggProgress::new("zion1test".into());
        assert_eq!(p.total_clues(), 0);
        assert_eq!(p.completion_percent(), 0.0);
    }

    #[test]
    fn test_discover_clue() {
        let mut p = GoldenEggProgress::new("zion1test".into());
        p.current_cl = 5;
        p.discover_clue(1, 1).unwrap();
        assert_eq!(p.total_clues(), 1);
        assert_eq!(p.ramayana_progress, 1);
    }

    #[test]
    fn test_cl_gating() {
        let mut p = GoldenEggProgress::new("zion1test".into());
        p.current_cl = 2;
        // Clue requiring CL 4 should fail
        assert!(p.discover_clue(1, 4).is_err());
    }

    #[test]
    fn test_key_unlock() {
        let mut p = GoldenEggProgress::new("zion1test".into());
        p.current_cl = 9;

        // Discover all Ramayana clues (1-30)
        for i in 1..=30 {
            p.discover_clue(i, 1).unwrap();
        }
        assert!(p.keys_unlocked.contains(&MasterKey::Ramayana));

        // Discover all Mahabharata clues (31-65)
        for i in 31..=65 {
            p.discover_clue(i, 1).unwrap();
        }
        assert!(p.keys_unlocked.contains(&MasterKey::Mahabharata));

        // Discover all Unity clues (66-108)
        for i in 66..=108 {
            p.discover_clue(i, 1).unwrap();
        }
        assert!(p.keys_unlocked.contains(&MasterKey::Unity));
        assert!(p.has_all_keys());
    }

    #[test]
    fn test_final_test() {
        let mut p = GoldenEggProgress::new("zion1test".into());
        p.current_cl = 9;
        for i in 1..=108 {
            p.discover_clue(i, 1).unwrap();
        }

        // Must donate 100% to pass
        assert!(!p.submit_final_test(false).unwrap());
        assert!(p.submit_final_test(true).unwrap());
    }

    #[test]
    fn test_tracker_leaderboard() {
        let mut tracker = GoldenEggTracker::new();

        let p1 = tracker.get_or_create("addr1");
        p1.current_cl = 5;
        for i in 1..=50 {
            p1.discover_clue(i, 1).unwrap();
        }

        let p2 = tracker.get_or_create("addr2");
        p2.current_cl = 5;
        for i in 1..=20 {
            p2.discover_clue(i, 1).unwrap();
        }

        let lb = tracker.leaderboard_by_clues(10);
        assert_eq!(lb[0].address, "addr1");
        assert_eq!(lb[0].total_clues(), 50);
    }
}
