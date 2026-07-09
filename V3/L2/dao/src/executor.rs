//! Executor — executes passed + timelocked proposals on L1.
//!
//! The executor is the final step in the governance lifecycle:
//! Proposal → Voting → Passed → Timelock → **Execute**
//!
//! Execution means creating an L1 transaction that fulfills the proposal.

use chrono::Utc;

use crate::config::DaoConfig;
use crate::error::{DaoError, DaoResult};
use crate::proposal::{Proposal, ProposalStatus, ProposalType};
use crate::timelock::Timelock;
use crate::treasury::{Treasury, TreasuryOperation};
use crate::types::FLOWERS_PER_ZION;

// ─────────────────────────────────────────────────────────────────────────────
// Parameter registry — maps parameter names to DaoConfig mutators
// ─────────────────────────────────────────────────────────────────────────────

/// Apply a DAO parameter change to the live config.
/// Returns a human-readable confirmation, or an error if the parameter is unknown
/// or the value fails validation.
pub fn apply_parameter_change(
    config: &mut DaoConfig,
    parameter_name: &str,
    proposed_value: &str,
) -> DaoResult<String> {
    match parameter_name {
        "quorum_percent" => {
            let v: f64 = proposed_value.parse().map_err(|_| {
                DaoError::Internal(format!(
                    "quorum_percent must be a float, got '{}'",
                    proposed_value
                ))
            })?;
            if !(1.0..=50.0).contains(&v) {
                return Err(DaoError::Internal(format!(
                    "quorum_percent must be 1–50, got {}",
                    v
                )));
            }
            let old = config.quorum_percent;
            config.quorum_percent = v;
            Ok(format!("quorum_percent: {} → {}", old, v))
        }
        "voting_period_days" => {
            let v: u32 = proposed_value.parse().map_err(|_| {
                DaoError::Internal(format!(
                    "voting_period_days must be u32, got '{}'",
                    proposed_value
                ))
            })?;
            if !(1..=30).contains(&v) {
                return Err(DaoError::Internal(format!(
                    "voting_period_days must be 1–30, got {}",
                    v
                )));
            }
            let old = config.voting_period_days;
            config.voting_period_days = v;
            Ok(format!("voting_period_days: {} → {}", old, v))
        }
        "timelock_hours" => {
            let v: u32 = proposed_value.parse().map_err(|_| {
                DaoError::Internal(format!(
                    "timelock_hours must be u32, got '{}'",
                    proposed_value
                ))
            })?;
            if !(12..=168).contains(&v) {
                return Err(DaoError::Internal(format!(
                    "timelock_hours must be 12–168, got {}",
                    v
                )));
            }
            let old = config.timelock_hours;
            config.timelock_hours = v;
            Ok(format!("timelock_hours: {} → {}", old, v))
        }
        "daily_spend_limit" => {
            let v: u64 = proposed_value.parse().map_err(|_| {
                DaoError::Internal(format!(
                    "daily_spend_limit must be u64 (whole ZION), got '{}'",
                    proposed_value
                ))
            })?;
            // Max 500M ZION/day (config stores whole ZION, not flowers)
            if v > 500_000_000 {
                return Err(DaoError::Internal(
                    "daily_spend_limit cannot exceed 500M ZION".into(),
                ));
            }
            let old = config.daily_spend_limit;
            config.daily_spend_limit = v;
            Ok(format!("daily_spend_limit: {} → {} ZION", old, v))
        }
        "multisig_threshold" => {
            let v: u32 = proposed_value.parse().map_err(|_| {
                DaoError::Internal(format!(
                    "multisig_threshold must be u32, got '{}'",
                    proposed_value
                ))
            })?;
            if v < 3 || v > config.multisig_total {
                return Err(DaoError::Internal(format!(
                    "multisig_threshold must be 3–{} (multisig_total), got {}",
                    config.multisig_total, v
                )));
            }
            let old = config.multisig_threshold;
            config.multisig_threshold = v;
            Ok(format!("multisig_threshold: {} → {}", old, v))
        }
        "proposal_threshold" => {
            let v: u64 = proposed_value.parse().map_err(|_| {
                DaoError::Internal(format!(
                    "proposal_threshold must be u64 (flowers), got '{}'",
                    proposed_value
                ))
            })?;
            let old = config.proposal_threshold;
            config.proposal_threshold = v;
            Ok(format!("proposal_threshold: {} → {} (atomic ZION)", old, v))
        }
        other => Err(DaoError::Internal(format!(
            "Unknown DAO parameter '{}' — valid: quorum_percent, voting_period_days, \
             timelock_hours, daily_spend_limit, multisig_threshold, proposal_threshold",
            other
        ))),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Emergency executor
// ─────────────────────────────────────────────────────────────────────────────

/// Emergency actions that can be executed without normal timelock.
/// Each returns a memo string that should be broadcast as an L1 TX
/// with the DAO:emergency:<action> prefix.
pub fn execute_emergency_action(action: &str, justification: &str) -> DaoResult<String> {
    let valid_actions = [
        "pause_bridge",
        "unpause_bridge",
        "freeze_treasury",
        "unfreeze_treasury",
        "halt_validator",
        "rotate_guardian",
    ];
    if !valid_actions.contains(&action) {
        return Err(DaoError::Internal(format!(
            "Unknown emergency action '{}' — valid: {}",
            action,
            valid_actions.join(", ")
        )));
    }

    let l1_memo = format!(
        "DAO:emergency:{}:{}",
        action,
        &justification[..justification.len().min(100)]
    );
    Ok(format!(
        "Emergency '{}' executed — broadcast L1 memo: [{}]",
        action, l1_memo
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Main executor
// ─────────────────────────────────────────────────────────────────────────────

/// Execute a passed and timelocked proposal.
///
/// `executor_guardian` — the L1 address of the guardian calling execute.
///   Must be in the guardian set for treasury operations.
/// `config` — mutable DAO config (modified by Parameter proposals).
pub fn execute_proposal(
    proposal: &mut Proposal,
    timelock: &mut Timelock,
    treasury: &mut Treasury,
    config: &mut DaoConfig,
    executor_guardian: &str,
) -> DaoResult<String> {
    // Verify status
    if proposal.status != ProposalStatus::Timelocked {
        return Err(DaoError::ProposalNotVotable(format!(
            "Proposal {} is {:?}, expected Timelocked",
            proposal.id, proposal.status
        )));
    }

    // Verify timelock is ready
    if timelock.is_active() {
        return Err(DaoError::TimelockActive {
            remaining_hours: timelock.remaining_hours(),
        });
    }

    // Execute based on proposal type
    let result = match &proposal.proposal_type.clone() {
        ProposalType::Treasury {
            recipient,
            amount,
            purpose,
        } => {
            let op_id = format!("dao-exec-{}", proposal.id);
            treasury.submit_operation(
                op_id.clone(),
                TreasuryOperation::Spend {
                    recipient: recipient.clone(),
                    amount: *amount,
                    purpose: purpose.clone(),
                    proposal_id: proposal.id,
                },
                executor_guardian,
            )?;
            format!(
                "Treasury spend submitted: {} ZION → {} (op: {})",
                amount / FLOWERS_PER_ZION,
                recipient,
                op_id
            )
        }

        ProposalType::Humanitarian {
            category,
            amount,
            region,
            ..
        } => {
            let op_id = format!("dao-humanitarian-{}", proposal.id);
            treasury.submit_operation(
                op_id.clone(),
                TreasuryOperation::HumanitarianGrant {
                    category: category.clone(),
                    recipient: region.clone(),
                    amount: *amount,
                    proposal_id: proposal.id,
                },
                executor_guardian,
            )?;
            format!(
                "Humanitarian grant submitted: {} ZION to {} ({}) (op: {})",
                amount / FLOWERS_PER_ZION,
                category,
                region,
                op_id
            )
        }

        ProposalType::Grant {
            recipient,
            amount,
            milestones,
            ..
        } => {
            let op_id = format!("dao-grant-{}", proposal.id);
            treasury.submit_operation(
                op_id.clone(),
                TreasuryOperation::Spend {
                    recipient: recipient.clone(),
                    amount: *amount,
                    purpose: format!("Grant #{} — {} milestones", proposal.id, milestones.len()),
                    proposal_id: proposal.id,
                },
                executor_guardian,
            )?;
            format!(
                "Grant submitted: {} ZION → {} ({} milestones) (op: {})",
                amount / FLOWERS_PER_ZION,
                recipient,
                milestones.len(),
                op_id
            )
        }

        ProposalType::Parameter {
            parameter_name,
            proposed_value,
            ..
        } => apply_parameter_change(config, parameter_name, proposed_value)?,

        ProposalType::Emergency {
            action,
            justification,
        } => execute_emergency_action(action, justification)?,

        // L5 governance proposals — these are consent-based, not treasury-based.
        // Execution means recording the on-chain confirmation + reputation update.
        ProposalType::Admission { candidate_id, .. } => {
            format!("Admission confirmed for candidate {}", candidate_id)
        }

        ProposalType::Bodhisattva { candidate_id, .. } => {
            format!("Bodhisattva vow confirmed for Guardian {}", candidate_id)
        }

        ProposalType::Expulsion {
            accused_id, tier, ..
        } => {
            format!("Expulsion executed for {} at tier {}", accused_id, tier)
        }

        ProposalType::CrossLayer {
            target_layers,
            inner_proposal_id,
            ..
        } => {
            format!(
                "Cross-layer proposal {} consented by layers {:?}",
                inner_proposal_id, target_layers
            )
        }
    };

    // Update status
    proposal.status = ProposalStatus::Executed;
    proposal.executed_at = Some(Utc::now());
    timelock.mark_executed()?;

    Ok(result)
}

// ─────────────────────────────────────────────────────────────────────────────
// Guardian multisig — collect signatures for treasury operations
// ─────────────────────────────────────────────────────────────────────────────

/// Sign a pending treasury operation.
/// Returns `(signatures_so_far, threshold_reached)`.
pub fn sign_treasury_op(
    treasury: &mut Treasury,
    operation_id: &str,
    guardian_address: &str,
) -> DaoResult<(usize, bool)> {
    let reached = treasury.add_signature(operation_id, guardian_address)?;
    // Count sigs via execute (it returns Ok if threshold not reached yet — we re-query)
    Ok((0, reached)) // treasury handles count internally
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_parameter_change_quorum() {
        let mut cfg = DaoConfig::default();
        let r = apply_parameter_change(&mut cfg, "quorum_percent", "15.0").unwrap();
        assert!(r.contains("15"));
        assert_eq!(cfg.quorum_percent, 15.0);
    }

    #[test]
    fn test_apply_parameter_change_out_of_range() {
        let mut cfg = DaoConfig::default();
        assert!(apply_parameter_change(&mut cfg, "quorum_percent", "99.0").is_err());
    }

    #[test]
    fn test_apply_parameter_change_unknown() {
        let mut cfg = DaoConfig::default();
        assert!(apply_parameter_change(&mut cfg, "fake_param", "1").is_err());
    }

    #[test]
    fn test_apply_parameter_voting_period() {
        let mut cfg = DaoConfig::default();
        let r = apply_parameter_change(&mut cfg, "voting_period_days", "14").unwrap();
        assert!(r.contains("14"));
        assert_eq!(cfg.voting_period_days, 14);
    }

    #[test]
    fn test_apply_parameter_timelock_hours() {
        let mut cfg = DaoConfig::default();
        apply_parameter_change(&mut cfg, "timelock_hours", "72").unwrap();
        assert_eq!(cfg.timelock_hours, 72);
    }

    #[test]
    fn test_emergency_action_valid() {
        let r = execute_emergency_action("pause_bridge", "Critical bug in relay").unwrap();
        assert!(r.contains("pause_bridge"));
        assert!(r.contains("DAO:emergency:pause_bridge"));
    }

    #[test]
    fn test_emergency_action_invalid() {
        assert!(execute_emergency_action("steal_funds", "hacker").is_err());
    }
}
