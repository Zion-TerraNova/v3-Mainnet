//! DAO Core Types
//!
//! Shared types used across all DAO modules.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// DAO Address Constants
// ---------------------------------------------------------------------------

/// DAO Treasury address on L1 (from genesis premine)
/// Total: 4,000,000,000 ZION across 3 addresses
pub const DAO_TREASURY_ADDRESSES: &[&str] = &[
    "zion1u5u7k43240d5l4d0x7q5m3c4a838z4k000cv3q0", // Community Governance (main) — 2.5B ZION
    "zion1m8d235x268h8d887s036m8c3x7s356d3r37k6m6", // Grants & Bounties — 1.0B ZION
    "zion102s8k4k0w783d657j255z865e47054s342u87v3", // Ecosystem Bootstrap — 0.5B ZION
];

/// Flowers per ZION — V3 canonical 6-decimal precision (post-3.0.3 fork).
pub const FLOWERS_PER_ZION: u64 = 1_000_000;

/// Total DAO treasury in flowers (4B ZION × 10⁶) — u128 required.
pub const DAO_TREASURY_TOTAL: u128 = 4_000_000_000_u128 * FLOWERS_PER_ZION as u128;

/// Minimum ZION balance to create a proposal (1M ZION in flowers)
pub const PROPOSAL_THRESHOLD: u64 = 1_000_000 * FLOWERS_PER_ZION;

/// Voting period in seconds (7 days)
pub const VOTING_PERIOD_SECS: u64 = 7 * 24 * 60 * 60;

/// Timelock duration in seconds (48 hours)
pub const TIMELOCK_SECS: u64 = 48 * 60 * 60;

/// Quorum percentage (10% of circulating supply must vote)
pub const QUORUM_PERCENT: f64 = 10.0;

/// Multi-sig threshold for treasury operations
pub const MULTISIG_THRESHOLD: u32 = 5;
pub const MULTISIG_TOTAL: u32 = 7;

/// Maximum treasury spend per day in flowers (100M ZION) — u128 required.
pub const DAILY_SPEND_LIMIT: u128 = 100_000_000_u128 * FLOWERS_PER_ZION as u128;

// ---------------------------------------------------------------------------
// DAO Memo Format
// ---------------------------------------------------------------------------

/// DAO memo prefix for L1 transactions
/// Format: "DAO:<action>:<data>"
/// Examples:
///   "DAO:vote:42:yes"       — Vote yes on proposal 42
///   "DAO:propose:treasury"  — Create treasury proposal
///   "DAO:execute:42"        — Execute approved proposal 42
pub const DAO_MEMO_PREFIX: &str = "DAO";

/// Parse a DAO memo from an L1 transaction
pub fn parse_dao_memo(memo: &str) -> Option<DaoMemo> {
    let parts: Vec<&str> = memo.split(':').collect();
    if parts.first() != Some(&DAO_MEMO_PREFIX) || parts.len() < 3 {
        return None;
    }
    match parts[1] {
        "vote" if parts.len() >= 4 => Some(DaoMemo::Vote {
            proposal_id: parts[2].to_string(),
            choice: match parts[3] {
                "yes" => VoteChoice::Yes,
                "no" => VoteChoice::No,
                "abstain" => VoteChoice::Abstain,
                _ => return None,
            },
        }),
        "propose" => Some(DaoMemo::Propose {
            proposal_type: parts[2].to_string(),
        }),
        "execute" => Some(DaoMemo::Execute {
            proposal_id: parts[2].to_string(),
        }),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DaoMemo {
    Vote {
        proposal_id: String,
        choice: VoteChoice,
    },
    Propose {
        proposal_type: String,
    },
    Execute {
        proposal_id: String,
    },
}

// ---------------------------------------------------------------------------
// Vote Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoteChoice {
    Yes,
    No,
    Abstain,
}

// ---------------------------------------------------------------------------
// Guardian (multi-sig signer)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guardian {
    pub name: String,
    pub address: String,
    pub public_key: String,
    pub is_active: bool,
}

// ---------------------------------------------------------------------------
// Co-Admin — multi-layer governance participant
// ---------------------------------------------------------------------------

/// Layer identifier for Co-Admin governance (1–6)
pub type LayerId = u8;

/// Role of a Co-Admin within a specific layer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoAdminRole {
    Validator, // L1: consensus node operator
    CoreDev,   // L1: protocol developer
    Security,  // L1: emergency response
    Treasury,  // L2: treasury guardian
    Bridge,    // L2/L3: bridge guardian
    Relayer,   // L3: WARP cross-chain relayer
    Auditor,   // L3: cross-chain auditor
    Curator,   // L4: OASIS quest/avatar curator
    Moderator, // L4: content moderator
    Community, // L5: physical community guardian
    Network,   // L5: L5 network council delegate
    Steward,   // L6: Issobella space steward
}

impl CoAdminRole {
    /// Which layer this role belongs to
    pub fn layer(&self) -> LayerId {
        match self {
            CoAdminRole::Validator | CoAdminRole::CoreDev | CoAdminRole::Security => 1,
            CoAdminRole::Treasury | CoAdminRole::Bridge => 2,
            CoAdminRole::Relayer | CoAdminRole::Auditor => 3,
            CoAdminRole::Curator | CoAdminRole::Moderator => 4,
            CoAdminRole::Community | CoAdminRole::Network => 5,
            CoAdminRole::Steward => 6,
        }
    }

    /// Human-readable role name
    pub fn role_name(&self) -> &'static str {
        match self {
            CoAdminRole::Validator => "validator",
            CoAdminRole::CoreDev => "core_dev",
            CoAdminRole::Security => "security",
            CoAdminRole::Treasury => "treasury",
            CoAdminRole::Bridge => "bridge",
            CoAdminRole::Relayer => "relayer",
            CoAdminRole::Auditor => "auditor",
            CoAdminRole::Curator => "curator",
            CoAdminRole::Moderator => "moderator",
            CoAdminRole::Community => "community",
            CoAdminRole::Network => "network",
            CoAdminRole::Steward => "steward",
        }
    }
}

/// A Co-Admin is a governance participant with bonded stake and reputation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoAdmin {
    pub name: String,
    pub address: String,
    pub public_key: String,
    pub role: CoAdminRole,
    pub layer: LayerId,
    pub bonded: u64,     // staked/bonded amount in flowers
    pub reputation: u64, // governance reputation score
    pub is_active: bool,
    pub appointed_at: u64,     // block height
    pub term_end: Option<u64>, // block height, None = indefinite
}

impl CoAdmin {
    /// Check if this Co-Admin holds a role in a given layer
    pub fn is_in_layer(&self, layer: LayerId) -> bool {
        self.layer == layer && self.is_active
    }

    /// Check if this Co-Admin can participate in cross-layer proposals
    pub fn can_cross_layer(&self) -> bool {
        // Only roles with explicit cross-layer permission
        matches!(
            self.role,
            CoAdminRole::Treasury
                | CoAdminRole::Bridge
                | CoAdminRole::Network
                | CoAdminRole::Steward
        )
    }
}

// ---------------------------------------------------------------------------
// Snapshot of voter balance
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoterSnapshot {
    /// Voter L1 address
    pub address: String,
    /// Balance in atomic units at snapshot block
    pub balance: u64,
    /// Block height at which balance was snapshotted
    pub snapshot_block: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vote_memo() {
        let memo = "DAO:vote:42:yes";
        let parsed = parse_dao_memo(memo).unwrap();
        match parsed {
            DaoMemo::Vote {
                proposal_id,
                choice,
            } => {
                assert_eq!(proposal_id, "42");
                assert_eq!(choice, VoteChoice::Yes);
            }
            _ => panic!("Expected Vote memo"),
        }
    }

    #[test]
    fn test_parse_proposal_memo() {
        let memo = "DAO:propose:treasury";
        let parsed = parse_dao_memo(memo).unwrap();
        match parsed {
            DaoMemo::Propose { proposal_type } => {
                assert_eq!(proposal_type, "treasury");
            }
            _ => panic!("Expected Propose memo"),
        }
    }

    #[test]
    fn test_parse_invalid_memo() {
        assert!(parse_dao_memo("BRIDGE:base:0x123").is_none());
        assert!(parse_dao_memo("DAO:invalid").is_none());
        assert!(parse_dao_memo("random text").is_none());
    }

    #[test]
    fn test_co_admin_role_layer() {
        assert_eq!(CoAdminRole::Validator.layer(), 1);
        assert_eq!(CoAdminRole::Treasury.layer(), 2);
        assert_eq!(CoAdminRole::Relayer.layer(), 3);
        assert_eq!(CoAdminRole::Curator.layer(), 4);
        assert_eq!(CoAdminRole::Community.layer(), 5);
        assert_eq!(CoAdminRole::Steward.layer(), 6);
    }

    #[test]
    fn test_co_admin_role_name() {
        assert_eq!(CoAdminRole::Validator.role_name(), "validator");
        assert_eq!(CoAdminRole::Steward.role_name(), "steward");
    }

    #[test]
    fn test_co_admin_cross_layer() {
        let treasury = CoAdmin {
            name: "Alice".into(),
            address: "zion1alice".into(),
            public_key: "pk1".into(),
            role: CoAdminRole::Treasury,
            layer: 2,
            bonded: 1_000_000,
            reputation: 500,
            is_active: true,
            appointed_at: 100,
            term_end: None,
        };
        assert!(treasury.can_cross_layer());
        assert!(treasury.is_in_layer(2));
        assert!(!treasury.is_in_layer(5));

        let validator = CoAdmin {
            name: "Bob".into(),
            address: "zion1bob".into(),
            public_key: "pk2".into(),
            role: CoAdminRole::Validator,
            layer: 1,
            bonded: 2_000_000,
            reputation: 1000,
            is_active: true,
            appointed_at: 200,
            term_end: None,
        };
        assert!(!validator.can_cross_layer());
    }

    #[test]
    fn test_constants() {
        // 1 ZION = 10⁶ flowers (post-3.0.3)
        assert_eq!(FLOWERS_PER_ZION, 1_000_000);
        // 4B ZION = 4_000_000_000 × 10⁶ flowers
        assert_eq!(DAO_TREASURY_TOTAL, 4_000_000_000_000_000_u128);
        // 1M ZION threshold
        assert_eq!(PROPOSAL_THRESHOLD, 1_000_000_000_000);
        // 100M ZION daily limit
        assert_eq!(DAILY_SPEND_LIMIT, 100_000_000_000_000_u128);
        // 7 days
        assert_eq!(VOTING_PERIOD_SECS, 604_800);
        // 48 hours
        assert_eq!(TIMELOCK_SECS, 172_800);
        // 5-of-7
        assert_eq!(MULTISIG_THRESHOLD, 5);
        assert_eq!(MULTISIG_TOTAL, 7);
    }
}
