//! Proposal Engine — create, manage, and track governance proposals.
//!
//! ## Proposal Types
//!
//! | Type       | Description                          | Quorum | Voting |
//! |------------|--------------------------------------|--------|--------|
//! | Parameter  | Change DAO parameters (fees, limits) | 10%    | 7 days |
//! | Treasury   | Spend from DAO treasury              | 15%    | 7 days |
//! | Emergency  | Emergency action (pause, upgrade)    | 20%    | 3 days |
//! | Grant      | Fund a project / team                | 10%    | 7 days |
//! | Humanitarian | Allocate humanitarian funds        | 10%    | 7 days |

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::VoteChoice;

// ---------------------------------------------------------------------------
// Proposal Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalType {
    /// Change a DAO parameter (fees, limits, thresholds)
    Parameter {
        parameter_name: String,
        current_value: String,
        proposed_value: String,
    },
    /// Spend from DAO treasury
    Treasury {
        recipient: String,
        amount: u64,
        purpose: String,
    },
    /// Emergency action (shorter voting period, higher quorum)
    Emergency {
        action: String,
        justification: String,
    },
    /// Fund a project / team
    Grant {
        recipient: String,
        amount: u64,
        milestones: Vec<String>,
        duration_days: u32,
    },
    /// Allocate funds to humanitarian category
    Humanitarian {
        category: String,
        amount: u64,
        region: String,
        description: String,
    },
    /// L5: Consciousness admission proposal (Gate 4 DAO confirmation)
    Admission {
        candidate_id: String,
        gate_scores_hash: String,
        sponsoring_guardians: Vec<String>,
        community: String,
    },
    /// L5: Bodhisattva vow confirmation (distributed witnessing)
    Bodhisattva {
        candidate_id: String,
        ceremony_date: String,
        ceremony_location: String,
        vow_text_hash: String,
        physical_symbol: String,
    },
    /// L5: Guardian expulsion (quadratic consent, 75% threshold)
    Expulsion {
        accused_id: String,
        offense_category: String,
        investigation_hash: String,
        defense_hash: Option<String>,
        tier: u8, // 1–4
    },
    /// Cross-layer proposal requiring multi-layer consent
    CrossLayer {
        target_layers: Vec<u8>,
        inner_proposal_id: u64,
        description: String,
    },
}

impl ProposalType {
    /// Required quorum percentage for this proposal type
    pub fn required_quorum_percent(&self) -> f64 {
        match self {
            ProposalType::Parameter { .. } => 10.0,
            ProposalType::Treasury { .. } => 15.0,
            ProposalType::Emergency { .. } => 20.0,
            ProposalType::Grant { .. } => 10.0,
            ProposalType::Humanitarian { .. } => 10.0,
            // L5 governance uses consent model, not token-weighted quorum;
            // these values are used for hybrid proposals that combine both.
            ProposalType::Admission { .. } => 60.0, // 60% of Guardians
            ProposalType::Bodhisattva { .. } => 60.0, // 60% of Guardians
            ProposalType::Expulsion { .. } => 75.0, // 75% quadratic consent
            ProposalType::CrossLayer { .. } => 15.0, // Standard cross-layer
        }
    }

    /// Voting period in seconds
    pub fn voting_period_secs(&self) -> u64 {
        match self {
            ProposalType::Emergency { .. } => 3 * 24 * 60 * 60, // 3 days
            ProposalType::Expulsion { .. } => 7 * 24 * 60 * 60, // 7 days
            ProposalType::Bodhisattva { .. } => 7 * 24 * 60 * 60, // 7 days
            ProposalType::CrossLayer { .. } => 7 * 24 * 60 * 60, // 7 days
            _ => 7 * 24 * 60 * 60,                              // 7 days
        }
    }

    /// Human-readable type name
    pub fn type_name(&self) -> &str {
        match self {
            ProposalType::Parameter { .. } => "parameter",
            ProposalType::Treasury { .. } => "treasury",
            ProposalType::Emergency { .. } => "emergency",
            ProposalType::Grant { .. } => "grant",
            ProposalType::Humanitarian { .. } => "humanitarian",
            ProposalType::Admission { .. } => "admission",
            ProposalType::Bodhisattva { .. } => "bodhisattva",
            ProposalType::Expulsion { .. } => "expulsion",
            ProposalType::CrossLayer { .. } => "cross_layer",
        }
    }

    /// Whether this proposal type uses the consent model (not token-weighted).
    pub fn uses_consent(&self) -> bool {
        matches!(
            self,
            ProposalType::Admission { .. }
                | ProposalType::Bodhisattva { .. }
                | ProposalType::Expulsion { .. }
        )
    }

    /// Which layer primarily governs this proposal type.
    pub fn governing_layer(&self) -> u8 {
        match self {
            ProposalType::Parameter { .. }
            | ProposalType::Treasury { .. }
            | ProposalType::Emergency { .. }
            | ProposalType::Grant { .. }
            | ProposalType::Humanitarian { .. } => 2,
            ProposalType::Admission { .. }
            | ProposalType::Bodhisattva { .. }
            | ProposalType::Expulsion { .. } => 5,
            ProposalType::CrossLayer { target_layers, .. } => {
                target_layers.first().copied().unwrap_or(2)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Proposal Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalStatus {
    /// Draft — not yet submitted
    Draft,
    /// Active — voting in progress
    Active,
    /// Passed — voting ended, quorum met, majority yes
    Passed,
    /// Failed — voting ended, quorum not met or majority no
    Failed,
    /// Timelocked — passed, waiting for timelock (48h)
    Timelocked,
    /// Executed — timelock elapsed, action performed
    Executed,
    /// Cancelled — proposer cancelled before end of voting
    Cancelled,
    /// Expired — timelock expired without execution
    Expired,
}

// ---------------------------------------------------------------------------
// Proposal
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    /// Unique proposal ID (sequential)
    pub id: u64,
    /// UUID for cross-reference
    pub uuid: String,
    /// Title
    pub title: String,
    /// Detailed description (markdown)
    pub description: String,
    /// Proposal type with specific data
    pub proposal_type: ProposalType,
    /// Current status
    pub status: ProposalStatus,
    /// Proposer L1 address
    pub proposer: String,
    /// Proposer's balance at snapshot (must be >= threshold)
    pub proposer_balance: u64,
    /// Block height at which voting power is snapshotted
    pub snapshot_block: u64,
    /// Total votes for
    pub votes_for: u64,
    /// Total votes against
    pub votes_against: u64,
    /// Total abstentions
    pub votes_abstain: u64,
    /// Number of unique voters
    pub voter_count: u32,
    /// When the proposal was created
    pub created_at: DateTime<Utc>,
    /// When voting ends
    pub voting_ends_at: DateTime<Utc>,
    /// When timelock ends (if passed)
    pub timelock_ends_at: Option<DateTime<Utc>>,
    /// When the proposal was executed (if executed)
    pub executed_at: Option<DateTime<Utc>>,
    /// Execution TX hash on L1
    pub execution_tx: Option<String>,
}

impl Proposal {
    /// Create a new proposal
    pub fn new(
        id: u64,
        title: String,
        description: String,
        proposal_type: ProposalType,
        proposer: String,
        proposer_balance: u64,
        snapshot_block: u64,
    ) -> Self {
        let now = Utc::now();
        let voting_period = chrono::Duration::seconds(proposal_type.voting_period_secs() as i64);
        Self {
            id,
            uuid: Uuid::new_v4().to_string(),
            title,
            description,
            proposal_type,
            status: ProposalStatus::Active,
            proposer,
            proposer_balance,
            snapshot_block,
            votes_for: 0,
            votes_against: 0,
            votes_abstain: 0,
            voter_count: 0,
            created_at: now,
            voting_ends_at: now + voting_period,
            timelock_ends_at: None,
            executed_at: None,
            execution_tx: None,
        }
    }

    /// Is voting still open?
    pub fn is_voting_open(&self) -> bool {
        self.status == ProposalStatus::Active && Utc::now() < self.voting_ends_at
    }

    /// Total votes cast (for + against + abstain)
    pub fn total_votes(&self) -> u64 {
        self.votes_for + self.votes_against + self.votes_abstain
    }

    /// Add a vote
    pub fn add_vote(&mut self, choice: VoteChoice, weight: u64) {
        match choice {
            VoteChoice::Yes => self.votes_for += weight,
            VoteChoice::No => self.votes_against += weight,
            VoteChoice::Abstain => self.votes_abstain += weight,
        }
        self.voter_count += 1;
    }

    /// Check if proposal passed (majority yes)
    pub fn has_passed(&self) -> bool {
        self.votes_for > self.votes_against
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_proposal() -> Proposal {
        Proposal::new(
            1,
            "Test Proposal".to_string(),
            "This is a test".to_string(),
            ProposalType::Parameter {
                parameter_name: "fee_percent".to_string(),
                current_value: "0.1".to_string(),
                proposed_value: "0.05".to_string(),
            },
            "zion1proposer".to_string(),
            2_000_000_000_000, // 2M ZION
            1000,
        )
    }

    #[test]
    fn test_create_proposal() {
        let p = sample_proposal();
        assert_eq!(p.id, 1);
        assert_eq!(p.status, ProposalStatus::Active);
        assert_eq!(p.votes_for, 0);
        assert_eq!(p.voter_count, 0);
        assert!(p.is_voting_open());
    }

    #[test]
    fn test_add_votes() {
        let mut p = sample_proposal();
        p.add_vote(VoteChoice::Yes, 1_000_000_000_000); // 1M ZION (6-dec: 1e6 ZION × 1e6 flowers)
        p.add_vote(VoteChoice::No, 500_000_000_000); // 500K ZION
        p.add_vote(VoteChoice::Abstain, 100_000_000_000); // 100K ZION

        assert_eq!(p.votes_for, 1_000_000_000_000);
        assert_eq!(p.votes_against, 500_000_000_000);
        assert_eq!(p.total_votes(), 1_600_000_000_000);
        assert_eq!(p.voter_count, 3);
        assert!(p.has_passed()); // more yes than no
    }

    #[test]
    fn test_proposal_type_quorum() {
        assert_eq!(
            ProposalType::Parameter {
                parameter_name: "x".into(),
                current_value: "1".into(),
                proposed_value: "2".into(),
            }
            .required_quorum_percent(),
            10.0
        );
        assert_eq!(
            ProposalType::Emergency {
                action: "pause".into(),
                justification: "critical".into(),
            }
            .required_quorum_percent(),
            20.0
        );
    }

    #[test]
    fn test_emergency_shorter_voting() {
        let emergency = ProposalType::Emergency {
            action: "pause".into(),
            justification: "urgent".into(),
        };
        // Emergency = 3 days
        assert_eq!(emergency.voting_period_secs(), 3 * 24 * 60 * 60);

        let standard = ProposalType::Grant {
            recipient: "team".into(),
            amount: 1000,
            milestones: vec![],
            duration_days: 30,
        };
        // Standard = 7 days
        assert_eq!(standard.voting_period_secs(), 7 * 24 * 60 * 60);
    }
}
