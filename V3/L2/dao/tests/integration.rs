//! DAO Integration Tests — E2E flow
//!
//! Tests the full governance lifecycle:
//!   propose → vote → quorum/majority → timelock → execute
//!
//! These tests use in-memory SQLite and real DAO logic.
//! No network calls, no L1 RPC required.

use chrono::{Duration, Utc};
use zion_dao::config::DaoConfig;
use zion_dao::db::DaoDb;
use zion_dao::executor::{apply_parameter_change, execute_emergency_action, execute_proposal};
use zion_dao::proposal::{Proposal, ProposalStatus, ProposalType};
use zion_dao::quorum::check_quorum;
use zion_dao::timelock::Timelock;
use zion_dao::treasury::Treasury;
use zion_dao::types::{Guardian, VoteChoice};
use zion_dao::voting::VotingEngine;

// ─────────────────────────────────────────────────────────────────────────────
// Test helpers
// ─────────────────────────────────────────────────────────────────────────────

fn make_guardian(i: u8) -> Guardian {
    Guardian {
        address: format!("zion1guardian{:02}", i),
        public_key: format!("pubkey{:02}", i),
        name: format!("Guardian {}", i),
        is_active: true,
    }
}

fn make_guardians() -> Vec<Guardian> {
    (1..=7).map(make_guardian).collect()
}

fn make_treasury(balance: u128) -> Treasury {
    Treasury::new(make_guardians(), balance)
}

/// Create a Timelock that ended in the past (ready for execution).
fn expired_timelock(proposal_id: u64) -> Timelock {
    let past = Utc::now() - Duration::hours(50); // 50h ago, past 48h lock
    Timelock {
        proposal_id,
        started_at: past,
        ends_at: past + Duration::hours(48),
        executed: false,
    }
}

/// Create a Timelock that is still active.
fn active_timelock(proposal_id: u64) -> Timelock {
    Timelock::new(proposal_id) // 48h from now
}

fn make_proposal_treasury(id: u64) -> Proposal {
    Proposal::new(
        id,
        format!("Treasury Spend #{}", id),
        "Fund a habitat restoration project".into(),
        ProposalType::Treasury {
            recipient: "zion1beneficiary000000000000000000000001".into(),
            amount: 10_000_000_000_000, // 10M ZION
            purpose: "Habitat restoration".into(),
        },
        "zion1proposer000000000000000000000000001".into(),
        2_000_000_000_000, // 2M ZION proposer balance
        100_000,           // snapshot block
    )
}

fn make_proposal_parameter(id: u64, param: &str, value: &str) -> Proposal {
    Proposal::new(
        id,
        format!("Change {}", param),
        format!("Update {} to {}", param, value),
        ProposalType::Parameter {
            parameter_name: param.into(),
            current_value: "old".into(),
            proposed_value: value.into(),
        },
        "zion1proposer000000000000000000000000001".into(),
        2_000_000_000_000,
        100_001,
    )
}

fn make_proposal_emergency() -> Proposal {
    Proposal::new(
        99,
        "Emergency: Pause Bridge".into(),
        "Critical relay bug — pausing to prevent funds loss".into(),
        ProposalType::Emergency {
            action: "pause_bridge".into(),
            justification: "Critical bug discovered in relay".into(),
        },
        "zion1guardian01".into(),
        2_000_000_000_000,
        100_002,
    )
}

fn make_proposal_humanitarian(id: u64) -> Proposal {
    Proposal::new(
        id,
        "Humanitarian: Water".into(),
        "Fund clean water project in Sub-Saharan Africa".into(),
        ProposalType::Humanitarian {
            category: "Water".into(),
            amount: 5_000_000_000_000, // 5M ZION
            region: "Sub-Saharan Africa".into(),
            description: "100 wells".into(),
        },
        "zion1humanitarian0000000000000000000001".into(),
        2_000_000_000_000,
        100_003,
    )
}

/// Vote N voters for given choice (each with weight), return proposal with totals applied.
fn vote_n(
    engine: &mut VotingEngine,
    proposal: &mut Proposal,
    n: usize,
    choice: VoteChoice,
    weight: u64,
    prefix: &str,
) {
    for i in 1..=n {
        engine
            .cast_vote(
                proposal,
                format!("{}{:04}", prefix, i),
                choice,
                weight,
                None,
            )
            .unwrap();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DB persistence tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_db_proposal_roundtrip() {
    let db = DaoDb::in_memory().unwrap();
    let proposal = make_proposal_treasury(1);
    db.insert_proposal(&proposal).unwrap();

    let loaded = db.get_proposal(1).unwrap().expect("Proposal should exist");
    assert_eq!(loaded.id, 1);
    assert_eq!(loaded.status, "Active");
    assert!(loaded.title.contains("Treasury Spend"));
}

#[test]
fn test_db_vote_deduplication() {
    let db = DaoDb::in_memory().unwrap();
    let proposal = make_proposal_treasury(2);
    db.insert_proposal(&proposal).unwrap();

    let r1 = db
        .record_vote(2, "zion1voter01", VoteChoice::Yes, 1_000_000, None)
        .unwrap();
    assert!(r1, "First vote should be recorded");

    let r2 = db
        .record_vote(2, "zion1voter01", VoteChoice::No, 1_000_000, None)
        .unwrap();
    assert!(!r2, "Duplicate vote should be ignored");

    let r3 = db
        .record_vote(2, "zion1voter02", VoteChoice::No, 2_000_000, None)
        .unwrap();
    assert!(r3, "Different voter should succeed");
}

#[test]
fn test_db_vote_totals() {
    let db = DaoDb::in_memory().unwrap();
    let proposal = make_proposal_treasury(3);
    db.insert_proposal(&proposal).unwrap();

    db.record_vote(3, "zion1v1", VoteChoice::Yes, 5_000_000, None)
        .unwrap();
    db.record_vote(3, "zion1v2", VoteChoice::Yes, 3_000_000, None)
        .unwrap();
    db.record_vote(3, "zion1v3", VoteChoice::No, 1_000_000, None)
        .unwrap();
    db.record_vote(3, "zion1v4", VoteChoice::Abstain, 500_000, None)
        .unwrap();

    let (yes, no, abstain) = db.vote_totals(3).unwrap();
    assert_eq!(yes, 8_000_000);
    assert_eq!(no, 1_000_000);
    assert_eq!(abstain, 500_000);
}

#[test]
fn test_db_scan_state_cursor() {
    let db = DaoDb::in_memory().unwrap();
    assert_eq!(db.last_scanned_block().unwrap(), 0);
    db.set_last_scanned_block(42_000).unwrap();
    assert_eq!(db.last_scanned_block().unwrap(), 42_000);
    db.set_last_scanned_block(42_500).unwrap();
    assert_eq!(db.last_scanned_block().unwrap(), 42_500);
}

#[test]
fn test_db_proposal_status_update() {
    let db = DaoDb::in_memory().unwrap();
    let proposal = make_proposal_treasury(4);
    db.insert_proposal(&proposal).unwrap();

    let mut row = db.get_proposal(4).unwrap().unwrap();
    assert_eq!(row.status, "Active");
    row.status = "Passed".into();
    db.update_proposal_row(&row).unwrap();

    let row2 = db.get_proposal(4).unwrap().unwrap();
    assert_eq!(row2.status, "Passed");
}

// ─────────────────────────────────────────────────────────────────────────────
// Voting engine tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_voting_engine_add_votes() {
    let mut proposal = make_proposal_treasury(10);
    let mut engine = VotingEngine::new();

    engine
        .cast_vote(
            &mut proposal,
            "zion1v1".into(),
            VoteChoice::Yes,
            6_000_000,
            None,
        )
        .unwrap();
    engine
        .cast_vote(
            &mut proposal,
            "zion1v2".into(),
            VoteChoice::No,
            3_000_000,
            None,
        )
        .unwrap();
    engine
        .cast_vote(
            &mut proposal,
            "zion1v3".into(),
            VoteChoice::Abstain,
            1_000_000,
            None,
        )
        .unwrap();

    assert_eq!(proposal.votes_for, 6_000_000);
    assert_eq!(proposal.votes_against, 3_000_000);
    assert_eq!(proposal.votes_abstain, 1_000_000);
    assert_eq!(proposal.voter_count, 3);
    assert!(proposal.has_passed());
}

#[test]
fn test_voting_engine_deduplication() {
    let mut proposal = make_proposal_treasury(11);
    let mut engine = VotingEngine::new();

    engine
        .cast_vote(
            &mut proposal,
            "zion1v1".into(),
            VoteChoice::Yes,
            1_000_000,
            None,
        )
        .unwrap();
    let r = engine.cast_vote(
        &mut proposal,
        "zion1v1".into(),
        VoteChoice::No,
        1_000_000,
        None,
    );
    assert!(r.is_err(), "Duplicate vote should fail");
    assert_eq!(proposal.voter_count, 1);
}

#[test]
fn test_voting_engine_rejected_if_closed() {
    let mut proposal = make_proposal_treasury(12);
    proposal.status = ProposalStatus::Passed;
    let mut engine = VotingEngine::new();

    let r = engine.cast_vote(
        &mut proposal,
        "zion1v1".into(),
        VoteChoice::Yes,
        1_000_000,
        None,
    );
    assert!(r.is_err(), "Cannot vote on non-Active proposal");
}

// ─────────────────────────────────────────────────────────────────────────────
// Quorum tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_quorum_treasury_met() {
    // Treasury quorum = 15% of circulating supply
    let circulating = 100_000_000_000_000u64; // 100M ZION atomic
    let mut proposal = make_proposal_treasury(20);
    let mut engine = VotingEngine::new();

    // 20 voters × 1M ZION = 20M = 20% → above 15% treasury quorum
    vote_n(
        &mut engine,
        &mut proposal,
        20,
        VoteChoice::Yes,
        1_000_000_000_000,
        "zion1t",
    );

    assert!(check_quorum(&proposal, circulating).is_ok());
}

#[test]
fn test_quorum_not_met() {
    let circulating = 100_000_000_000_000u64;
    let mut proposal = make_proposal_treasury(21);
    let mut engine = VotingEngine::new();

    // Only 5 voters × 1M = 5M = 5% → below 15% treasury quorum
    vote_n(
        &mut engine,
        &mut proposal,
        5,
        VoteChoice::Yes,
        1_000_000_000_000,
        "zion1q",
    );

    assert!(
        check_quorum(&proposal, circulating).is_err(),
        "5% < 15% quorum"
    );
}

#[test]
fn test_quorum_parameter_lower() {
    // Parameter quorum = 10%
    let circulating = 100_000_000_000_000u64;
    let mut proposal = make_proposal_parameter(22, "quorum_percent", "12.0");
    let mut engine = VotingEngine::new();

    // 11 voters × 1M = 11M = 11% → above 10%
    vote_n(
        &mut engine,
        &mut proposal,
        11,
        VoteChoice::Yes,
        1_000_000_000_000,
        "zion1r",
    );

    assert!(check_quorum(&proposal, circulating).is_ok());
}

#[test]
fn test_quorum_emergency_higher() {
    // Emergency quorum = 20%
    let circulating = 100_000_000_000_000u64;
    let mut proposal = make_proposal_emergency();
    let mut engine = VotingEngine::new();

    // 15 voters × 1M = 15M = 15% → below 20% emergency quorum
    vote_n(
        &mut engine,
        &mut proposal,
        15,
        VoteChoice::Yes,
        1_000_000_000_000,
        "zion1e",
    );

    assert!(
        check_quorum(&proposal, circulating).is_err(),
        "Emergency needs 20%, got 15%"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Executor tests — D-05
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_executor_treasury_spend() {
    let mut proposal = make_proposal_treasury(30);
    proposal.status = ProposalStatus::Timelocked;
    let mut timelock = expired_timelock(30);
    let mut treasury = make_treasury(100_000_000_000_000);
    let mut config = DaoConfig::default();

    let result = execute_proposal(
        &mut proposal,
        &mut timelock,
        &mut treasury,
        &mut config,
        "zion1guardian01",
    )
    .unwrap();
    assert!(result.contains("Treasury spend submitted"));
    assert_eq!(proposal.status, ProposalStatus::Executed);
    assert!(proposal.executed_at.is_some());
}

#[test]
fn test_executor_humanitarian_grant() {
    let mut proposal = make_proposal_humanitarian(31);
    proposal.status = ProposalStatus::Timelocked;
    let mut timelock = expired_timelock(31);
    let mut treasury = make_treasury(100_000_000_000_000);
    let mut config = DaoConfig::default();

    let result = execute_proposal(
        &mut proposal,
        &mut timelock,
        &mut treasury,
        &mut config,
        "zion1guardian01",
    )
    .unwrap();
    assert!(result.contains("Humanitarian grant submitted"));
    assert_eq!(proposal.status, ProposalStatus::Executed);
}

#[test]
fn test_executor_parameter_change() {
    let mut proposal = make_proposal_parameter(32, "quorum_percent", "15.0");
    proposal.status = ProposalStatus::Timelocked;
    let mut timelock = expired_timelock(32);
    let mut treasury = make_treasury(0);
    let mut config = DaoConfig::default();
    assert_eq!(config.quorum_percent, 10.0);

    let result = execute_proposal(
        &mut proposal,
        &mut timelock,
        &mut treasury,
        &mut config,
        "zion1guardian01",
    )
    .unwrap();
    assert!(result.contains("15"));
    assert_eq!(config.quorum_percent, 15.0);
    assert_eq!(proposal.status, ProposalStatus::Executed);
}

#[test]
fn test_executor_parameter_invalid_value() {
    let mut cfg = DaoConfig::default();
    assert!(apply_parameter_change(&mut cfg, "quorum_percent", "abc").is_err());
    assert!(apply_parameter_change(&mut cfg, "quorum_percent", "99.0").is_err());
    assert!(apply_parameter_change(&mut cfg, "nonexistent_param", "1").is_err());
}

#[test]
fn test_executor_parameter_all_fields() {
    let mut cfg = DaoConfig::default();
    apply_parameter_change(&mut cfg, "voting_period_days", "14").unwrap();
    assert_eq!(cfg.voting_period_days, 14);
    apply_parameter_change(&mut cfg, "timelock_hours", "72").unwrap();
    assert_eq!(cfg.timelock_hours, 72);
    apply_parameter_change(&mut cfg, "multisig_threshold", "4").unwrap();
    assert_eq!(cfg.multisig_threshold, 4);
}

#[test]
fn test_executor_emergency_valid() {
    let r = execute_emergency_action("pause_bridge", "Critical vulnerability").unwrap();
    assert!(r.contains("pause_bridge"));
    assert!(r.contains("DAO:emergency:pause_bridge"));
}

#[test]
fn test_executor_emergency_all_valid_actions() {
    for action in &[
        "pause_bridge",
        "unpause_bridge",
        "freeze_treasury",
        "halt_validator",
    ] {
        assert!(
            execute_emergency_action(action, "test").is_ok(),
            "Action {} should work",
            action
        );
    }
}

#[test]
fn test_executor_emergency_unknown() {
    assert!(execute_emergency_action("drain_treasury", "evil").is_err());
    assert!(execute_emergency_action("", "test").is_err());
}

#[test]
fn test_executor_rejects_non_timelocked() {
    let mut proposal = make_proposal_treasury(33); // status = Active
    let mut timelock = expired_timelock(33);
    let mut treasury = make_treasury(100_000_000_000_000);
    let mut config = DaoConfig::default();

    let r = execute_proposal(
        &mut proposal,
        &mut timelock,
        &mut treasury,
        &mut config,
        "zion1guardian01",
    );
    assert!(r.is_err(), "Non-Timelocked proposal should be rejected");
}

#[test]
fn test_executor_rejects_active_timelock() {
    let mut proposal = make_proposal_treasury(34);
    proposal.status = ProposalStatus::Timelocked;
    let mut timelock = active_timelock(34); // still 48h to go
    let mut treasury = make_treasury(100_000_000_000_000);
    let mut config = DaoConfig::default();

    let r = execute_proposal(
        &mut proposal,
        &mut timelock,
        &mut treasury,
        &mut config,
        "zion1guardian01",
    );
    assert!(r.is_err(), "Active timelock should block execution");
}

// ─────────────────────────────────────────────────────────────────────────────
// Full E2E lifecycle
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_full_e2e_treasury_proposal() {
    // --- 1. Setup ---
    let db = DaoDb::in_memory().unwrap();
    let mut proposal = make_proposal_treasury(100);
    let mut engine = VotingEngine::new();
    let circulating = 100_000_000_000_000u64; // 100M ZION
    let mut treasury = make_treasury(100_000_000_000_000);
    let mut config = DaoConfig::default();

    // --- 2. Persist ---
    db.insert_proposal(&proposal).unwrap();

    // --- 3. Vote: 20 voters × 1M ZION = 20% > 15% treasury quorum ---
    for i in 1..=20 {
        let choice = if i <= 15 {
            VoteChoice::Yes
        } else {
            VoteChoice::No
        };
        let voter = format!("zion1voter{:03}", i);
        engine
            .cast_vote(
                &mut proposal,
                voter.clone(),
                choice,
                1_000_000_000_000,
                None,
            )
            .unwrap();
        db.record_vote(100, &voter, choice, 1_000_000, None)
            .unwrap();
    }

    // --- 4. Quorum check ---
    assert!(check_quorum(&proposal, circulating).is_ok());
    assert!(proposal.has_passed());

    // --- 5. Status → Passed → Timelocked ---
    proposal.status = ProposalStatus::Timelocked;
    let mut row = db.get_proposal(100).unwrap().unwrap();
    row.status = "Timelocked".into();
    db.update_proposal_row(&row).unwrap();

    // --- 6. Execute (timelock expired) ---
    let mut timelock = expired_timelock(100);
    let result = execute_proposal(
        &mut proposal,
        &mut timelock,
        &mut treasury,
        &mut config,
        "zion1guardian01",
    )
    .unwrap();

    assert!(result.contains("Treasury spend submitted"));
    assert_eq!(proposal.status, ProposalStatus::Executed);

    // --- 7. Persist final status ---
    let mut row2 = db.get_proposal(100).unwrap().unwrap();
    row2.status = "Executed".into();
    db.update_proposal_row(&row2).unwrap();

    let final_row = db.get_proposal(100).unwrap().unwrap();
    assert_eq!(final_row.status, "Executed");
}

#[test]
fn test_full_e2e_parameter_proposal() {
    let mut proposal = make_proposal_parameter(101, "voting_period_days", "14");
    let mut engine = VotingEngine::new();
    let circulating = 100_000_000_000_000u64;
    let mut treasury = make_treasury(0);
    let mut config = DaoConfig::default();

    // 12% → above 10% standard quorum
    vote_n(
        &mut engine,
        &mut proposal,
        12,
        VoteChoice::Yes,
        1_000_000_000_000,
        "zion1p",
    );
    assert!(check_quorum(&proposal, circulating).is_ok());

    proposal.status = ProposalStatus::Timelocked;
    let mut timelock = expired_timelock(101);

    let result = execute_proposal(
        &mut proposal,
        &mut timelock,
        &mut treasury,
        &mut config,
        "zion1guardian01",
    )
    .unwrap();
    assert!(result.contains("14"));
    assert_eq!(config.voting_period_days, 14);
    assert_eq!(proposal.status, ProposalStatus::Executed);
}

#[test]
fn test_full_e2e_emergency_proposal() {
    let mut proposal = make_proposal_emergency();
    let mut engine = VotingEngine::new();
    let circulating = 100_000_000_000_000u64;
    let mut treasury = make_treasury(0);
    let mut config = DaoConfig::default();

    // 21% → above 20% emergency quorum
    vote_n(
        &mut engine,
        &mut proposal,
        21,
        VoteChoice::Yes,
        1_000_000_000_000,
        "zion1em",
    );
    assert!(check_quorum(&proposal, circulating).is_ok());

    proposal.status = ProposalStatus::Timelocked;
    let mut timelock = expired_timelock(99);

    let result = execute_proposal(
        &mut proposal,
        &mut timelock,
        &mut treasury,
        &mut config,
        "zion1guardian01",
    )
    .unwrap();
    assert!(result.contains("pause_bridge"));
    assert_eq!(proposal.status, ProposalStatus::Executed);
}
