//! Voting Engine — token-weighted voting (1 ZION = 1 vote).
//!
//! Voters lock their balance at proposal snapshot block.
//! Each address can vote once per proposal.
//! Weight = balance at snapshot block (in atomic units).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{DaoError, DaoResult};
use crate::proposal::{Proposal, ProposalStatus};
use crate::types::VoteChoice;

// ---------------------------------------------------------------------------
// Vote Record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    /// Proposal ID
    pub proposal_id: u64,
    /// Voter L1 address
    pub voter: String,
    /// Vote choice
    pub choice: VoteChoice,
    /// Vote weight (balance at snapshot)
    pub weight: u64,
    /// L1 TX hash containing the vote memo
    pub tx_hash: Option<String>,
    /// Timestamp
    pub voted_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Voting Engine
// ---------------------------------------------------------------------------

pub struct VotingEngine {
    /// proposal_id → (voter_address → Vote)
    votes: HashMap<u64, HashMap<String, Vote>>,
}

impl Default for VotingEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl VotingEngine {
    pub fn new() -> Self {
        Self {
            votes: HashMap::new(),
        }
    }

    /// Cast a vote on a proposal
    pub fn cast_vote(
        &mut self,
        proposal: &mut Proposal,
        voter: String,
        choice: VoteChoice,
        weight: u64,
        tx_hash: Option<String>,
    ) -> DaoResult<Vote> {
        // Check proposal is votable
        if proposal.status != ProposalStatus::Active {
            return Err(DaoError::ProposalNotVotable(proposal.id.to_string()));
        }

        if !proposal.is_voting_open() {
            return Err(DaoError::VotingPeriodEnded(proposal.id.to_string()));
        }

        // Check not already voted
        let proposal_votes = self.votes.entry(proposal.id).or_default();
        if proposal_votes.contains_key(&voter) {
            return Err(DaoError::AlreadyVoted(proposal.id.to_string()));
        }

        // Record vote
        let vote = Vote {
            proposal_id: proposal.id,
            voter: voter.clone(),
            choice,
            weight,
            tx_hash,
            voted_at: Utc::now(),
        };

        // Update proposal totals
        proposal.add_vote(choice, weight);

        // Store
        proposal_votes.insert(voter, vote.clone());

        Ok(vote)
    }

    /// Get all votes for a proposal
    pub fn get_votes(&self, proposal_id: u64) -> Vec<&Vote> {
        self.votes
            .get(&proposal_id)
            .map(|v| v.values().collect())
            .unwrap_or_default()
    }

    /// Check if an address has already voted
    pub fn has_voted(&self, proposal_id: u64, voter: &str) -> bool {
        self.votes
            .get(&proposal_id)
            .map(|v| v.contains_key(voter))
            .unwrap_or(false)
    }

    /// Get voter count for a proposal
    pub fn voter_count(&self, proposal_id: u64) -> u32 {
        self.votes
            .get(&proposal_id)
            .map(|v| v.len() as u32)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proposal::ProposalType;

    fn test_proposal() -> Proposal {
        Proposal::new(
            1,
            "Test".into(),
            "Desc".into(),
            ProposalType::Parameter {
                parameter_name: "fee".into(),
                current_value: "0.1".into(),
                proposed_value: "0.05".into(),
            },
            "zion1proposer".into(),
            2_000_000_000_000,
            100,
        )
    }

    #[test]
    fn test_cast_vote() {
        let mut engine = VotingEngine::new();
        let mut proposal = test_proposal();

        let vote = engine
            .cast_vote(
                &mut proposal,
                "zion1voter1".to_string(),
                VoteChoice::Yes,
                1_000_000_000_000, // 1M ZION
                None,
            )
            .unwrap();

        assert_eq!(vote.weight, 1_000_000_000_000);
        assert_eq!(vote.choice, VoteChoice::Yes);
        assert_eq!(proposal.votes_for, 1_000_000_000_000);
        assert_eq!(proposal.voter_count, 1);
    }

    #[test]
    fn test_double_vote_rejected() {
        let mut engine = VotingEngine::new();
        let mut proposal = test_proposal();

        engine
            .cast_vote(
                &mut proposal,
                "zion1voter1".to_string(),
                VoteChoice::Yes,
                1_000_000_000_000,
                None,
            )
            .unwrap();

        let result = engine.cast_vote(
            &mut proposal,
            "zion1voter1".to_string(),
            VoteChoice::No,
            1_000_000_000_000,
            None,
        );

        assert!(result.is_err());
        assert!(matches!(result, Err(DaoError::AlreadyVoted(_))));
    }

    #[test]
    fn test_has_voted() {
        let mut engine = VotingEngine::new();
        let mut proposal = test_proposal();

        assert!(!engine.has_voted(1, "zion1voter1"));

        engine
            .cast_vote(
                &mut proposal,
                "zion1voter1".to_string(),
                VoteChoice::Yes,
                1_000_000,
                None,
            )
            .unwrap();

        assert!(engine.has_voted(1, "zion1voter1"));
        assert!(!engine.has_voted(1, "zion1voter2"));
    }
}
