//! Challenges — AI challenges, quizzes, and meditation bonuses.
//!
//! Players earn XP by completing various challenges that promote
//! learning, consciousness growth, and humanitarian awareness.

use serde::{Deserialize, Serialize};

/// Challenge categories
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChallengeCategory {
    /// AI-generated knowledge quiz
    Quiz,
    /// Programming / crypto challenge
    Technical,
    /// Meditation / mindfulness exercise
    Meditation,
    /// Humanitarian awareness
    Humanitarian,
    /// Creative expression
    Creative,
    /// Community engagement
    Community,
}

/// Challenge difficulty
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Difficulty {
    Beginner,
    Intermediate,
    Advanced,
    Master,
}

impl Difficulty {
    pub fn xp_multiplier(&self) -> f64 {
        match self {
            Difficulty::Beginner => 1.0,
            Difficulty::Intermediate => 2.0,
            Difficulty::Advanced => 4.0,
            Difficulty::Master => 8.0,
        }
    }
}

/// A challenge in the OASIS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Challenge {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: ChallengeCategory,
    pub difficulty: Difficulty,
    /// Base XP reward (before multipliers)
    pub base_xp: u64,
    /// Optional ZION reward
    pub zion_reward: u64,
    /// Time limit in seconds (0 = no limit)
    pub time_limit: u64,
    /// Minimum consciousness level required
    pub min_level_xp: u64,
    /// Is this a daily challenge?
    pub is_daily: bool,
    /// Number of times this can be completed (0 = unlimited)
    pub max_completions: u32,
}

/// Quiz question for quiz-type challenges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuizQuestion {
    pub question: String,
    pub options: Vec<String>,
    pub correct_index: usize,
    pub explanation: String,
}

/// Meditation session for meditation-type challenges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeditationSession {
    /// Duration in minutes
    pub duration_minutes: u32,
    /// Guided meditation topic
    pub topic: String,
    /// Bonus XP for completing the full duration
    pub completion_bonus: u64,
    /// Streak multiplier (consecutive days)
    pub streak_multiplier: f64,
}

/// Challenge attempt result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeResult {
    pub challenge_id: String,
    pub player_address: String,
    pub score: f64, // 0.0 - 1.0
    pub xp_earned: u64,
    pub zion_earned: u64,
    pub completed_at: u64,
    pub time_taken: u64, // seconds
}

/// Challenge engine — manages available challenges
pub struct ChallengeEngine {
    challenges: Vec<Challenge>,
}

impl Default for ChallengeEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ChallengeEngine {
    pub fn new() -> Self {
        Self {
            challenges: Vec::new(),
        }
    }

    /// Add a challenge
    pub fn add_challenge(&mut self, challenge: Challenge) {
        self.challenges.push(challenge);
    }

    /// Get available challenges for a player's XP level
    pub fn available_for(&self, player_xp: u64) -> Vec<&Challenge> {
        self.challenges
            .iter()
            .filter(|c| c.min_level_xp <= player_xp)
            .collect()
    }

    /// Get daily challenges
    pub fn daily_challenges(&self) -> Vec<&Challenge> {
        self.challenges.iter().filter(|c| c.is_daily).collect()
    }

    /// Calculate XP reward for a challenge result
    pub fn calculate_reward(&self, challenge: &Challenge, score: f64) -> u64 {
        let base = challenge.base_xp as f64;
        let difficulty_mult = challenge.difficulty.xp_multiplier();
        let score_mult = score.clamp(0.0, 1.0);

        (base * difficulty_mult * score_mult) as u64
    }

    /// Generate genesis challenges
    pub fn genesis_challenges() -> Self {
        let mut engine = Self::new();

        engine.add_challenge(Challenge {
            id: "daily_meditation".into(),
            title: "Daily Meditation".into(),
            description: "Complete a 10-minute guided meditation".into(),
            category: ChallengeCategory::Meditation,
            difficulty: Difficulty::Beginner,
            base_xp: 50,
            zion_reward: 0,
            time_limit: 0,
            min_level_xp: 0,
            is_daily: true,
            max_completions: 1,
        });

        engine.add_challenge(Challenge {
            id: "crypto_quiz_beginner".into(),
            title: "Crypto Knowledge — Beginner".into(),
            description: "Test your blockchain knowledge".into(),
            category: ChallengeCategory::Quiz,
            difficulty: Difficulty::Beginner,
            base_xp: 30,
            zion_reward: 0,
            time_limit: 300, // 5 minutes
            min_level_xp: 0,
            is_daily: false,
            max_completions: 0,
        });

        engine.add_challenge(Challenge {
            id: "humanitarian_awareness".into(),
            title: "Humanitarian Impact Quiz".into(),
            description: "Learn about global humanitarian needs".into(),
            category: ChallengeCategory::Humanitarian,
            difficulty: Difficulty::Intermediate,
            base_xp: 80,
            zion_reward: 10,
            time_limit: 600,
            min_level_xp: 1_000, // Emotional level
            is_daily: false,
            max_completions: 0,
        });

        engine.add_challenge(Challenge {
            id: "ai_challenge_advanced".into(),
            title: "AI Reasoning Challenge".into(),
            description: "Solve an AI-generated reasoning puzzle".into(),
            category: ChallengeCategory::Technical,
            difficulty: Difficulty::Advanced,
            base_xp: 200,
            zion_reward: 50,
            time_limit: 1800,    // 30 minutes
            min_level_xp: 5_000, // Mental level
            is_daily: false,
            max_completions: 0,
        });

        engine
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_challenges() {
        let engine = ChallengeEngine::genesis_challenges();
        let daily = engine.daily_challenges();
        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0].id, "daily_meditation");
    }

    #[test]
    fn test_available_for_level() {
        let engine = ChallengeEngine::genesis_challenges();
        let beginner = engine.available_for(0);
        assert_eq!(beginner.len(), 2); // meditation + crypto quiz

        let emotional = engine.available_for(1_000);
        assert_eq!(emotional.len(), 3); // + humanitarian

        let mental = engine.available_for(5_000);
        assert_eq!(mental.len(), 4); // all
    }

    #[test]
    fn test_reward_calculation() {
        let engine = ChallengeEngine::genesis_challenges();
        let challenge = &engine.challenges[0]; // meditation, base 50, beginner (1.0×)
        let reward = engine.calculate_reward(challenge, 1.0);
        assert_eq!(reward, 50);

        let advanced = &engine.challenges[3]; // AI, base 200, advanced (4.0×)
        let reward = engine.calculate_reward(advanced, 0.5);
        assert_eq!(reward, 400); // 200 × 4.0 × 0.5
    }
}
