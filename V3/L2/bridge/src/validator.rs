//! Multisig validator logic for bridge operations.
//!
//! Each bridge relay operator runs as a validator node.
//! N-of-M consensus is required before mint/burn execution.

use serde::{Deserialize, Serialize};
use tracing::info;

/// Validator set configuration.
#[derive(Debug, Clone)]
pub struct ValidatorSet {
    /// Required confirmations
    pub threshold: u8,
    /// All validator addresses (EVM)
    pub validators: Vec<String>,
}

impl ValidatorSet {
    pub fn new(threshold: u8, validators: Vec<String>) -> Self {
        assert!(
            threshold as usize <= validators.len(),
            "Threshold ({}) exceeds validator count ({})",
            threshold,
            validators.len()
        );
        assert!(threshold >= 2, "Threshold must be at least 2");

        Self {
            threshold,
            validators,
        }
    }

    pub fn is_validator(&self, address: &str) -> bool {
        self.validators
            .iter()
            .any(|v| v.eq_ignore_ascii_case(address))
    }

    pub fn count(&self) -> usize {
        self.validators.len()
    }
}

/// Tracks confirmations for a single bridge operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusTracker {
    pub operation_id: String,
    pub confirmations: Vec<String>, // Validator addresses that confirmed
    pub threshold: u8,
    pub reached: bool,
}

impl ConsensusTracker {
    pub fn new(operation_id: String, threshold: u8) -> Self {
        Self {
            operation_id,
            confirmations: Vec::new(),
            threshold,
            reached: false,
        }
    }

    /// Add a validator confirmation. Returns true if threshold was just reached.
    pub fn add_confirmation(&mut self, validator_address: &str) -> bool {
        // Dedup
        if self
            .confirmations
            .iter()
            .any(|v| v.eq_ignore_ascii_case(validator_address))
        {
            return false;
        }

        self.confirmations.push(validator_address.to_string());

        if self.confirmations.len() as u8 >= self.threshold && !self.reached {
            self.reached = true;
            info!(
                "🎯 Consensus reached for {} ({}/{} validators)",
                self.operation_id,
                self.confirmations.len(),
                self.threshold,
            );
            return true;
        }

        false
    }

    pub fn confirmation_count(&self) -> u8 {
        self.confirmations.len() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validator_set() {
        let set = ValidatorSet::new(
            3,
            vec![
                "0xAAA".into(),
                "0xBBB".into(),
                "0xCCC".into(),
                "0xDDD".into(),
                "0xEEE".into(),
            ],
        );

        assert_eq!(set.count(), 5);
        assert!(set.is_validator("0xAAA"));
        assert!(set.is_validator("0xaaa")); // case-insensitive
        assert!(!set.is_validator("0xFFF"));
    }

    #[test]
    fn test_consensus_tracker() {
        let mut tracker = ConsensusTracker::new("lock_001".into(), 3);

        assert!(!tracker.add_confirmation("0xAAA")); // 1/3
        assert!(!tracker.add_confirmation("0xBBB")); // 2/3
        assert!(tracker.add_confirmation("0xCCC")); // 3/3 → reached!
        assert!(!tracker.add_confirmation("0xDDD")); // 4/3 → already reached
        assert!(!tracker.add_confirmation("0xAAA")); // duplicate

        assert_eq!(tracker.confirmation_count(), 4);
        assert!(tracker.reached);
    }

    #[test]
    #[should_panic(expected = "Threshold must be at least 2")]
    fn test_threshold_too_low() {
        ValidatorSet::new(1, vec!["0xAAA".into()]);
    }
}
