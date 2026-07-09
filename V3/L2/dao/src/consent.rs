//! Consent Engine — distributed witnessing for L5 governance.
//!
//! Unlike token-weighted voting (L2 DAO), consent voting gives each Co-Admin
//! one attestation (not weighted by stake). A proposal passes if:
//! 1. Quorum of eligible Co-Admins has attested, AND
//! 2. Zero "object" attestations exist (or objections are resolved).
//!
//! This is sociocracy translated to on-chain logic.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{DaoError, DaoResult};

// ---------------------------------------------------------------------------
// Attestation Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Attestation {
    Witness, // "I have no reasoned objection"
    Object,  // "I have an objection"
    Abstain, // "I choose not to participate"
}

// ---------------------------------------------------------------------------
// Consent Record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentRecord {
    pub proposal_id: u64,
    pub voter: String,
    pub attestation: Attestation,
    pub reason_hash: Option<String>, // hashed reason (for objections, private)
    pub attested_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Consent Engine
// ---------------------------------------------------------------------------

pub struct ConsentEngine {
    /// proposal_id → [ConsentRecord]
    records: HashMap<u64, Vec<ConsentRecord>>,
}

impl Default for ConsentEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsentEngine {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    /// Cast an attestation (witness/object/abstain) for a proposal.
    /// Each Co-Admin can attest only once per proposal.
    pub fn attest(
        &mut self,
        proposal_id: u64,
        voter: String,
        attestation: Attestation,
        reason_hash: Option<String>,
    ) -> DaoResult<ConsentRecord> {
        let records = self.records.entry(proposal_id).or_default();

        // Deduplicate
        if records.iter().any(|r| r.voter == voter) {
            return Err(DaoError::AlreadyVoted(format!(
                "CoAdmin {} already attested on proposal {}",
                voter, proposal_id
            )));
        }

        // Objections must have a reason hash
        if attestation == Attestation::Object && reason_hash.is_none() {
            return Err(DaoError::Internal(
                "Objection attestation requires a reason_hash".into(),
            ));
        }

        let record = ConsentRecord {
            proposal_id,
            voter: voter.clone(),
            attestation,
            reason_hash,
            attested_at: Utc::now(),
        };

        records.push(record.clone());
        Ok(record)
    }

    /// Check if a proposal has achieved consent.
    ///
    /// Returns (consent_achieved, witnesses, objections, abstentions, missing)
    pub fn check_consent(
        &self,
        proposal_id: u64,
        eligible_voters: &[String],
        required_quorum_percent: f64,
    ) -> (bool, usize, usize, usize, usize) {
        let records = self
            .records
            .get(&proposal_id)
            .map(|r| r.as_slice())
            .unwrap_or(&[]);

        let witnesses = records
            .iter()
            .filter(|r| r.attestation == Attestation::Witness)
            .count();
        let objections = records
            .iter()
            .filter(|r| r.attestation == Attestation::Object)
            .count();
        let abstentions = records
            .iter()
            .filter(|r| r.attestation == Attestation::Abstain)
            .count();

        let total_eligible = eligible_voters.len();
        let required =
            ((total_eligible as f64) * (required_quorum_percent / 100.0)).ceil() as usize;

        // Consent = quorum met (witnesses >= required) AND zero objections
        let quorum_met = witnesses >= required;
        let consent_achieved = quorum_met && objections == 0;

        let missing = total_eligible.saturating_sub(witnesses + objections + abstentions);

        (
            consent_achieved,
            witnesses,
            objections,
            abstentions,
            missing,
        )
    }

    /// Get all attestations for a proposal.
    pub fn get_attestations(&self, proposal_id: u64) -> Vec<&ConsentRecord> {
        self.records
            .get(&proposal_id)
            .map(|r| r.iter().collect())
            .unwrap_or_default()
    }

    /// Has this voter already attested?
    pub fn has_attested(&self, proposal_id: u64, voter: &str) -> bool {
        self.records
            .get(&proposal_id)
            .map(|r| r.iter().any(|rec| rec.voter == voter))
            .unwrap_or(false)
    }

    /// Get objection records (for review/pause handling).
    pub fn get_objections(&self, proposal_id: u64) -> Vec<&ConsentRecord> {
        self.records
            .get(&proposal_id)
            .map(|r| {
                r.iter()
                    .filter(|rec| rec.attestation == Attestation::Object)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Count unique attestors for a proposal.
    pub fn attestation_count(&self, proposal_id: u64) -> usize {
        self.records.get(&proposal_id).map(|r| r.len()).unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Quadratic Consent — for expulsion votes (weighted by conviction, not stake)
// ---------------------------------------------------------------------------

/// Quadratic weighting for consent votes.
/// Earlier attestations carry more weight (conviction signal).
pub fn quadratic_weight(attestation_index: usize, total_eligible: usize) -> f64 {
    let base = 1.0;
    let decay = 0.95f64.powi(attestation_index as i32);
    let scale = (total_eligible as f64).ln().max(1.0);
    base * decay * scale
}

/// Check if a proposal passes quadratic consent threshold.
pub fn check_quadratic_consent(
    witnesses: usize,
    objections: usize,
    eligible_count: usize,
    threshold_percent: f64,
) -> bool {
    if objections > 0 {
        // Any objection triggers pause in quadratic mode
        return false;
    }
    let required = ((eligible_count as f64) * (threshold_percent / 100.0)).ceil() as usize;
    witnesses >= required
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_voters() -> Vec<String> {
        vec![
            "v1".into(),
            "v2".into(),
            "v3".into(),
            "v4".into(),
            "v5".into(),
        ]
    }

    #[test]
    fn test_consent_achieved() {
        let mut engine = ConsentEngine::new();
        let voters = test_voters();

        // 5 voters, 60% quorum = need 3 witnesses
        for v in &voters[0..3] {
            engine
                .attest(1, v.clone(), Attestation::Witness, None)
                .unwrap();
        }

        let (ok, w, o, a, m) = engine.check_consent(1, &voters, 60.0);
        assert!(ok);
        assert_eq!(w, 3);
        assert_eq!(o, 0);
        assert_eq!(a, 0);
        assert_eq!(m, 2);
    }

    #[test]
    fn test_consent_blocked_by_objection() {
        let mut engine = ConsentEngine::new();
        let voters = test_voters();

        engine
            .attest(1, "v1".into(), Attestation::Witness, None)
            .unwrap();
        engine
            .attest(1, "v2".into(), Attestation::Witness, None)
            .unwrap();
        engine
            .attest(1, "v3".into(), Attestation::Object, Some("hash123".into()))
            .unwrap();

        let (ok, w, o, _a, _m) = engine.check_consent(1, &voters, 60.0);
        assert!(!ok); // blocked by objection
        assert_eq!(w, 2);
        assert_eq!(o, 1);
    }

    #[test]
    fn test_consent_quorum_not_met() {
        let mut engine = ConsentEngine::new();
        let voters = test_voters();

        engine
            .attest(1, "v1".into(), Attestation::Witness, None)
            .unwrap();

        let (ok, w, _o, _a, _m) = engine.check_consent(1, &voters, 60.0);
        assert!(!ok); // only 1 witness, need 3
        assert_eq!(w, 1);
    }

    #[test]
    fn test_double_attestation_rejected() {
        let mut engine = ConsentEngine::new();
        engine
            .attest(1, "v1".into(), Attestation::Witness, None)
            .unwrap();

        let result = engine.attest(1, "v1".into(), Attestation::Object, Some("h".into()));
        assert!(result.is_err());
    }

    #[test]
    fn test_objection_requires_reason() {
        let mut engine = ConsentEngine::new();
        let result = engine.attest(1, "v1".into(), Attestation::Object, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_quadratic_consent() {
        // 5 voters, 75% threshold = need 4 witnesses
        assert!(check_quadratic_consent(4, 0, 5, 75.0));
        assert!(!check_quadratic_consent(3, 0, 5, 75.0));
        assert!(!check_quadratic_consent(4, 1, 5, 75.0)); // objection blocks
    }

    #[test]
    fn test_quadratic_weight() {
        let w0 = quadratic_weight(0, 10);
        let w1 = quadratic_weight(1, 10);
        let w2 = quadratic_weight(2, 10);
        // Earlier = heavier
        assert!(w0 > w1);
        assert!(w1 > w2);
        assert!(w0 > 0.0);
    }
}
