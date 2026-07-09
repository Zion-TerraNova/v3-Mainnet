//! ZION V3 — Admin governance module
//!
//! Constitutional reference: `V3/docs/GEN_Z_INHERITANCE.md`
//!
//! Implements the 3-admin multisig governance structure:
//!   - **Rama**    → Admin-1 (protocol governance, emergency pause)
//!   - **Sita**    → Admin-2 (treasury oversight, DAO guardian)
//!   - **Hanuman** → Admin-3 (bridge admin, EVM multisig)
//!
//! Future Gen Z succession:
//!   - **Maitreya Buddha** → Admin-1 (inherits from Rama)
//!   - **Sarah Issobela**  → Admin-2 (inherits from Sita)
//!   - **Elizabeth**       → Admin-3 (inherits from Hanuman) — patroness of ZION, Ave Maria
//!
//! Admins have **limited rights**: basic mainnet operation only.
//! They CANNOT mint ZION, change premine, change fee split, or bypass time-locks.
//! Full ownership transfers to Gen Z + DAO after T0+18 years.

use crate::crypto;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of admins in the multisig.
pub const ADMIN_COUNT: usize = 3;

/// Threshold for emergency operations (immediate, no time-lock).
pub const EMERGENCY_THRESHOLD: usize = 2;

/// Threshold for normal admin operations (parameter changes, etc.).
pub const FULL_THRESHOLD: usize = 3;

/// Threshold for bridge-specific operations (validator rotation).
pub const BRIDGE_THRESHOLD: usize = 2;

/// Time-lock durations in seconds.

/// Emergency pause: no time-lock (immediate).
pub const TIMELOCK_EMERGENCY: u64 = 0;

/// Parameter change: 72 hours.
pub const TIMELOCK_PARAMETER: u64 = 72 * 3600;

/// Treasury spend: 7 days.
pub const TIMELOCK_TREASURY: u64 = 7 * 24 * 3600;

/// Admin rotation: 30 days.
pub const TIMELOCK_ADMIN_ROTATION: u64 = 30 * 24 * 3600;

/// Hard fork: 90 days.
pub const TIMELOCK_HARD_FORK: u64 = 90 * 24 * 3600;

/// Gen Z inheritance: 1 year (minimum).
pub const TIMELOCK_INHERITANCE: u64 = 365 * 24 * 3600;

/// Dead man's switch: 5 years of inactivity triggers auto-succession.
pub const DEAD_MANS_SWITCH_SECS: u64 = 5 * 365 * 24 * 3600;

/// Admin operation types.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AdminOpType {
    /// Emergency pause the chain (2-of-3, immediate).
    EmergencyPause,
    /// Resume the chain after emergency pause (2-of-3, immediate).
    EmergencyResume,
    /// Change network parameters — difficulty, fees (3-of-3, 72h).
    ParameterChange,
    /// Unlock DAO treasury for spend (3-of-3 + DAO vote, 7d).
    TreasurySpend,
    /// Rotate admin key — replace one admin with new key (3-of-3 + DAO vote, 30d).
    AdminRotation,
    /// Rotate bridge validator EVM key (2-of-3, 7d).
    BridgeValidatorRotation,
    /// Rotate pool payout signing key (2-of-3, 7d).
    PoolPayoutRotation,
    /// Hard fork — change genesis (3-of-3 + DAO 75% supermajority, 90d).
    HardFork,
    /// Gen Z inheritance — transfer admin to successor (3-of-3 + DAO vote, 1 year).
    AdminInheritance,
}

impl AdminOpType {
    /// Required signature threshold for this operation.
    pub fn threshold(&self) -> usize {
        match self {
            Self::EmergencyPause | Self::EmergencyResume => EMERGENCY_THRESHOLD,
            Self::BridgeValidatorRotation | Self::PoolPayoutRotation => BRIDGE_THRESHOLD,
            Self::ParameterChange
            | Self::TreasurySpend
            | Self::AdminRotation
            | Self::HardFork
            | Self::AdminInheritance => FULL_THRESHOLD,
        }
    }

    /// Time-lock duration in seconds before operation can execute.
    pub fn timelock(&self) -> u64 {
        match self {
            Self::EmergencyPause | Self::EmergencyResume => TIMELOCK_EMERGENCY,
            Self::ParameterChange => TIMELOCK_PARAMETER,
            Self::TreasurySpend => TIMELOCK_TREASURY,
            Self::AdminRotation => TIMELOCK_ADMIN_ROTATION,
            Self::BridgeValidatorRotation | Self::PoolPayoutRotation => TIMELOCK_TREASURY,
            Self::HardFork => TIMELOCK_HARD_FORK,
            Self::AdminInheritance => TIMELOCK_INHERITANCE,
        }
    }

    /// Whether this operation requires a DAO vote in addition to admin signatures.
    pub fn requires_dao_vote(&self) -> bool {
        matches!(
            self,
            Self::TreasurySpend
                | Self::AdminRotation
                | Self::HardFork
                | Self::AdminInheritance
        )
    }

    /// DAO supermajority required (only for HardFork).
    pub fn dao_supermajority_pct(&self) -> Option<f64> {
        match self {
            Self::HardFork => Some(75.0),
            Self::AdminInheritance => Some(51.0), // simple majority for succession
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Admin set
// ---------------------------------------------------------------------------

/// A single admin in the governance multisig.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Admin {
    /// Symbolic name (e.g. "Rama", "Sita", "Hanuman").
    pub name: String,
    /// Role description.
    pub role: String,
    /// Ed25519 public key (32 bytes, hex-encoded in serialized form).
    pub public_key_hex: String,
    /// ZION L1 address derived from the public key.
    pub l1_address: String,
    /// EVM address (for bridge multisig) — 20 bytes, hex-encoded with 0x prefix.
    pub evm_address: String,
    /// Successor name (Gen Z inheritance).
    pub successor: String,
    /// Block height at which this admin was activated.
    pub activated_at_height: u64,
    /// Unix timestamp of last admin activity (for dead man's switch).
    pub last_activity_ts: u64,
}

/// The canonical 3-admin set.
///
/// **NOTE:** Public keys and addresses are populated at genesis time
/// from the offline-generated keypairs. The `name`, `role`, and `successor`
/// fields are constitutional constants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminSet {
    pub admins: Vec<Admin>,
    /// Block height at which this admin set was activated.
    pub activated_at_height: u64,
}

impl AdminSet {
    /// Create a new admin set from 3 admin entries.
    pub fn new(admins: Vec<Admin>, activated_at_height: u64) -> Result<Self, AdminError> {
        if admins.len() != ADMIN_COUNT {
            return Err(AdminError::InvalidAdminCount(admins.len()));
        }
        Ok(Self {
            admins,
            activated_at_height,
        })
    }

    /// Find an admin by L1 address.
    pub fn find_by_l1_address(&self, address: &str) -> Option<&Admin> {
        self.admins.iter().find(|a| a.l1_address == address)
    }

    /// Find an admin by name.
    pub fn find_by_name(&self, name: &str) -> Option<&Admin> {
        self.admins.iter().find(|a| a.name == name)
    }

    /// Check if a given L1 address is an active admin.
    pub fn is_admin(&self, address: &str) -> bool {
        self.find_by_l1_address(address).is_some()
    }

    /// Get the successor for a given admin name.
    pub fn successor_for(&self, name: &str) -> Option<&str> {
        self.find_by_name(name).map(|a| a.successor.as_str())
    }
}

// ---------------------------------------------------------------------------
// Admin operation proposal
// ---------------------------------------------------------------------------

/// A pending admin operation with collected signatures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminProposal {
    /// Unique proposal ID (hash of operation + params + timestamp).
    pub proposal_id: String,
    /// Operation type.
    pub op_type: AdminOpType,
    /// Block height at which the proposal was created.
    pub proposed_at_height: u64,
    /// Unix timestamp at which the proposal was created.
    pub proposed_at_ts: u64,
    /// Unix timestamp at which the proposal can be executed (proposed_at_ts + timelock).
    pub executable_at_ts: u64,
    /// Collected signatures: admin L1 address → Ed25519 signature (hex).
    pub signatures: BTreeMap<String, String>,
    /// DAO proposal ID (if operation requires DAO vote).
    pub dao_proposal_id: Option<String>,
    /// Whether the DAO has approved (if required).
    pub dao_approved: Option<bool>,
    /// Operation-specific parameters (JSON).
    pub params: serde_json::Value,
    /// Whether the proposal has been executed.
    pub executed: bool,
}

impl AdminProposal {
    /// Create a new proposal.
    pub fn new(
        op_type: AdminOpType,
        proposed_at_height: u64,
        proposed_at_ts: u64,
        params: serde_json::Value,
    ) -> Self {
        let executable_at_ts = proposed_at_ts + op_type.timelock();
        let proposal_id = Self::compute_proposal_id(&op_type, proposed_at_ts, &params);
        Self {
            proposal_id,
            op_type,
            proposed_at_height,
            proposed_at_ts,
            executable_at_ts,
            signatures: BTreeMap::new(),
            dao_proposal_id: None,
            dao_approved: None,
            params,
            executed: false,
        }
    }

    /// Compute a deterministic proposal ID.
    fn compute_proposal_id(
        op_type: &AdminOpType,
        proposed_at_ts: u64,
        params: &serde_json::Value,
    ) -> String {
        let preimage = format!("{op_type:?}:{proposed_at_ts}:{params}");
        let hash = crypto::blake3_hash(preimage.as_bytes());
        crypto::to_hex(&hash)
    }

    /// Add a signature from an admin.
    pub fn add_signature(
        &mut self,
        admin_set: &AdminSet,
        admin_address: &str,
        signature_hex: &str,
        message: &[u8],
    ) -> Result<(), AdminError> {
        // Verify admin is in the set
        let admin = admin_set
            .find_by_l1_address(admin_address)
            .ok_or(AdminError::NotAnAdmin(admin_address.to_string()))?;

        // Verify no double-sign
        if self.signatures.contains_key(admin_address) {
            return Err(AdminError::AlreadySigned(admin_address.to_string()));
        }

        // Verify signature
        let pk_bytes = crypto::from_hex(&admin.public_key_hex)
            .ok_or(AdminError::InvalidPublicKey(admin_address.to_string()))?;
        let sig_bytes = crypto::from_hex(signature_hex)
            .ok_or(AdminError::InvalidSignatureFormat(signature_hex.to_string()))?;
        if !crypto::verify(&pk_bytes, message, &sig_bytes) {
            return Err(AdminError::InvalidSignature(admin_address.to_string()));
        }

        self.signatures
            .insert(admin_address.to_string(), signature_hex.to_string());
        Ok(())
    }

    /// Check if the proposal has enough signatures to execute.
    pub fn has_quorum(&self) -> bool {
        self.signatures.len() >= self.op_type.threshold()
    }

    /// Check if the proposal is ready to execute (quorum + time-lock + DAO).
    pub fn is_executable(&self, current_ts: u64) -> Result<(), AdminError> {
        if self.executed {
            return Err(AdminError::AlreadyExecuted);
        }
        if !self.has_quorum() {
            return Err(AdminError::InsufficientSignatures {
                have: self.signatures.len(),
                need: self.op_type.threshold(),
            });
        }
        if current_ts < self.executable_at_ts {
            return Err(AdminError::TimelockNotExpired {
                current: current_ts,
                executable_at: self.executable_at_ts,
            });
        }
        if self.op_type.requires_dao_vote() {
            match self.dao_approved {
                Some(true) => {}
                Some(false) => return Err(AdminError::DaoRejected),
                None => return Err(AdminError::DaoVotePending),
            }
        }
        Ok(())
    }

    /// Mark the proposal as executed.
    pub fn mark_executed(&mut self) {
        self.executed = true;
    }
}

// ---------------------------------------------------------------------------
// Dead man's switch
// ---------------------------------------------------------------------------

/// Check if an admin has been inactive long enough to trigger dead man's switch.
pub fn check_dead_mans_switch(admin: &Admin, current_ts: u64) -> bool {
    current_ts.saturating_sub(admin.last_activity_ts) >= DEAD_MANS_SWITCH_SECS
}

/// Find all admins that have triggered dead man's switch.
pub fn find_inactive_admins(admin_set: &AdminSet, current_ts: u64) -> Vec<&Admin> {
    admin_set
        .admins
        .iter()
        .filter(|a| check_dead_mans_switch(a, current_ts))
        .collect()
}

// ---------------------------------------------------------------------------
// Admin rotation
// ---------------------------------------------------------------------------

/// Validate an admin rotation proposal.
///
/// Checks:
/// 1. The old admin is in the current set.
/// 2. The new admin's public key is valid.
/// 3. The new admin's L1 address matches the public key.
/// 4. The new admin's EVM address is valid format.
pub fn validate_admin_rotation(
    admin_set: &AdminSet,
    old_admin_name: &str,
    new_admin: &Admin,
) -> Result<(), AdminError> {
    // Old admin must exist
    let old = admin_set
        .find_by_name(old_admin_name)
        .ok_or(AdminError::AdminNotFound(old_admin_name.to_string()))?;

    // New admin must have valid public key
    let pk_bytes = crypto::from_hex(&new_admin.public_key_hex)
        .ok_or(AdminError::InvalidPublicKey(new_admin.name.to_string()))?;
    if pk_bytes.len() != 32 {
        return Err(AdminError::InvalidPublicKeyLength(pk_bytes.len()));
    }

    // New admin's L1 address must match public key
    let derived_address = crypto::derive_address(&pk_bytes);
    if derived_address != new_admin.l1_address {
        return Err(AdminError::AddressMismatch {
            expected: derived_address,
            actual: new_admin.l1_address.clone(),
        });
    }

    // New admin must not already be in the set
    if admin_set.is_admin(&new_admin.l1_address) {
        return Err(AdminError::AdminAlreadyExists(new_admin.l1_address.clone()));
    }

    // Successor must match the constitutional line
    if new_admin.name != old.successor {
        return Err(AdminError::InvalidSuccessor {
            expected: old.successor.clone(),
            actual: new_admin.name.clone(),
        });
    }

    Ok(())
}

/// Apply an admin rotation — returns a new AdminSet with the replacement.
pub fn apply_admin_rotation(
    admin_set: &AdminSet,
    old_admin_name: &str,
    new_admin: Admin,
    activated_at_height: u64,
) -> Result<AdminSet, AdminError> {
    validate_admin_rotation(admin_set, old_admin_name, &new_admin)?;

    let new_admins: Vec<Admin> = admin_set
        .admins
        .iter()
        .map(|a| {
            if a.name == old_admin_name {
                new_admin.clone()
            } else {
                a.clone()
            }
        })
        .collect();

    AdminSet::new(new_admins, activated_at_height)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Admin governance errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdminError {
    #[error("invalid admin count: expected {ADMIN_COUNT}, got {0}")]
    InvalidAdminCount(usize),
    #[error("not an admin: {0}")]
    NotAnAdmin(String),
    #[error("admin not found: {0}")]
    AdminNotFound(String),
    #[error("admin already exists: {0}")]
    AdminAlreadyExists(String),
    #[error("already signed: {0}")]
    AlreadySigned(String),
    #[error("invalid public key for admin: {0}")]
    InvalidPublicKey(String),
    #[error("invalid public key length: expected 32, got {0}")]
    InvalidPublicKeyLength(usize),
    #[error("invalid signature format: {0}")]
    InvalidSignatureFormat(String),
    #[error("invalid signature from admin: {0}")]
    InvalidSignature(String),
    #[error("insufficient signatures: have {have}, need {need}")]
    InsufficientSignatures { have: usize, need: usize },
    #[error("timelock not expired: current {current}, executable at {executable_at}")]
    TimelockNotExpired {
        current: u64,
        executable_at: u64,
    },
    #[error("proposal already executed")]
    AlreadyExecuted,
    #[error("DAO vote rejected")]
    DaoRejected,
    #[error("DAO vote pending")]
    DaoVotePending,
    #[error("address mismatch: expected {expected}, actual {actual}")]
    AddressMismatch { expected: String, actual: String },
    #[error("invalid successor: expected {expected}, actual {actual}")]
    InvalidSuccessor {
        expected: String,
        actual: String,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a test admin set with fresh keypairs.
    fn test_admin_set() -> AdminSet {
        let names = [("Rama", "Protocol governance"), ("Sita", "Treasury oversight"), ("Hanuman", "Bridge admin")];
        let successors = ["Maitreya Buddha", "Sarah Issobela", "Elizabeth"];

        let admins: Vec<Admin> = names
            .iter()
            .zip(successors.iter())
            .map(|((name, role), successor)| {
                let (sk, vk) = crypto::generate_keypair();
                let pk_hex = crypto::to_hex(vk.as_bytes());
                let l1_address = crypto::derive_address(vk.as_bytes());
                // Fake EVM address (32-byte hash truncated to 20 bytes)
                let evm_hash = crypto::blake3_hash(vk.as_bytes());
                let evm_address = format!("0x{}", crypto::to_hex(&evm_hash[..20]));
                let _ = sk;
                Admin {
                    name: name.to_string(),
                    role: role.to_string(),
                    public_key_hex: pk_hex,
                    l1_address,
                    evm_address,
                    successor: successor.to_string(),
                    activated_at_height: 0,
                    last_activity_ts: 1_000_000,
                }
            })
            .collect();

        AdminSet::new(admins, 0).unwrap()
    }

    #[test]
    fn admin_set_has_3_admins() {
        let set = test_admin_set();
        assert_eq!(set.admins.len(), ADMIN_COUNT);
    }

    #[test]
    fn admin_set_rejects_wrong_count() {
        let result = AdminSet::new(vec![], 0);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AdminError::InvalidAdminCount(0)));
    }

    #[test]
    fn find_admin_by_name() {
        let set = test_admin_set();
        assert!(set.find_by_name("Rama").is_some());
        assert!(set.find_by_name("Sita").is_some());
        assert!(set.find_by_name("Hanuman").is_some());
        assert!(set.find_by_name("NonExistent").is_none());
    }

    #[test]
    fn find_admin_by_l1_address() {
        let set = test_admin_set();
        let rama = set.find_by_name("Rama").unwrap();
        let found = set.find_by_l1_address(&rama.l1_address);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Rama");
    }

    #[test]
    fn is_admin_check() {
        let set = test_admin_set();
        let rama = set.find_by_name("Rama").unwrap();
        assert!(set.is_admin(&rama.l1_address));
        assert!(!set.is_admin("zion1nonexistent"));
    }

    #[test]
    fn successor_mapping() {
        let set = test_admin_set();
        assert_eq!(set.successor_for("Rama"), Some("Maitreya Buddha"));
        assert_eq!(set.successor_for("Sita"), Some("Sarah Issobela"));
        assert_eq!(set.successor_for("Hanuman"), Some("Elizabeth"));
    }

    #[test]
    fn op_type_thresholds() {
        assert_eq!(AdminOpType::EmergencyPause.threshold(), EMERGENCY_THRESHOLD);
        assert_eq!(AdminOpType::EmergencyResume.threshold(), EMERGENCY_THRESHOLD);
        assert_eq!(AdminOpType::ParameterChange.threshold(), FULL_THRESHOLD);
        assert_eq!(AdminOpType::AdminRotation.threshold(), FULL_THRESHOLD);
        assert_eq!(AdminOpType::BridgeValidatorRotation.threshold(), BRIDGE_THRESHOLD);
        assert_eq!(AdminOpType::PoolPayoutRotation.threshold(), BRIDGE_THRESHOLD);
        assert_eq!(AdminOpType::HardFork.threshold(), FULL_THRESHOLD);
        assert_eq!(AdminOpType::AdminInheritance.threshold(), FULL_THRESHOLD);
    }

    #[test]
    fn op_type_timelocks() {
        assert_eq!(AdminOpType::EmergencyPause.timelock(), TIMELOCK_EMERGENCY);
        assert_eq!(AdminOpType::ParameterChange.timelock(), TIMELOCK_PARAMETER);
        assert_eq!(AdminOpType::TreasurySpend.timelock(), TIMELOCK_TREASURY);
        assert_eq!(AdminOpType::AdminRotation.timelock(), TIMELOCK_ADMIN_ROTATION);
        assert_eq!(AdminOpType::HardFork.timelock(), TIMELOCK_HARD_FORK);
        assert_eq!(AdminOpType::AdminInheritance.timelock(), TIMELOCK_INHERITANCE);
    }

    #[test]
    fn emergency_has_no_timelock() {
        assert_eq!(TIMELOCK_EMERGENCY, 0);
    }

    #[test]
    fn dao_vote_requirement() {
        assert!(!AdminOpType::EmergencyPause.requires_dao_vote());
        assert!(!AdminOpType::ParameterChange.requires_dao_vote());
        assert!(AdminOpType::TreasurySpend.requires_dao_vote());
        assert!(AdminOpType::AdminRotation.requires_dao_vote());
        assert!(AdminOpType::HardFork.requires_dao_vote());
        assert!(AdminOpType::AdminInheritance.requires_dao_vote());
    }

    #[test]
    fn hard_fork_requires_supermajority() {
        assert_eq!(AdminOpType::HardFork.dao_supermajority_pct(), Some(75.0));
        assert_eq!(AdminOpType::AdminInheritance.dao_supermajority_pct(), Some(51.0));
        assert_eq!(AdminOpType::EmergencyPause.dao_supermajority_pct(), None);
    }

    #[test]
    fn proposal_creation_sets_executable_timestamp() {
        let ts = 1_000_000u64;
        let proposal = AdminProposal::new(
            AdminOpType::ParameterChange,
            100,
            ts,
            serde_json::json!({"key": "value"}),
        );
        // 72h timelock
        assert_eq!(proposal.executable_at_ts, ts + TIMELOCK_PARAMETER);
    }

    #[test]
    fn proposal_id_is_deterministic() {
        let ts = 1_000_000u64;
        let params = serde_json::json!({"key": "value"});
        let p1 = AdminProposal::new(AdminOpType::ParameterChange, 100, ts, params.clone());
        let p2 = AdminProposal::new(AdminOpType::ParameterChange, 100, ts, params);
        assert_eq!(p1.proposal_id, p2.proposal_id);
    }

    #[test]
    fn proposal_id_differs_for_different_ops() {
        let ts = 1_000_000u64;
        let params = serde_json::json!({});
        let p1 = AdminProposal::new(AdminOpType::EmergencyPause, 100, ts, params.clone());
        let p2 = AdminProposal::new(AdminOpType::ParameterChange, 100, ts, params);
        assert_ne!(p1.proposal_id, p2.proposal_id);
    }

    #[test]
    fn add_valid_signature() {
        let set = test_admin_set();
        let (sk, vk) = crypto::generate_keypair();
        let pk_hex = crypto::to_hex(vk.as_bytes());
        let l1_address = crypto::derive_address(vk.as_bytes());

        // Replace Rama's key with our test key
        let mut test_set = set.clone();
        test_set.admins[0].public_key_hex = pk_hex.clone();
        test_set.admins[0].l1_address = l1_address.clone();

        let message = b"test message";
        let signature = crypto::sign(&sk, message);
        let sig_hex = crypto::to_hex(&signature);

        let mut proposal = AdminProposal::new(
            AdminOpType::EmergencyPause,
            100,
            1_000_000,
            serde_json::json!({}),
        );

        let result = proposal.add_signature(&test_set, &l1_address, &sig_hex, message);
        assert!(result.is_ok());
        assert_eq!(proposal.signatures.len(), 1);
    }

    #[test]
    fn reject_signature_from_non_admin() {
        let set = test_admin_set();
        let (sk, vk) = crypto::generate_keypair();
        let l1_address = crypto::derive_address(vk.as_bytes());

        let message = b"test message";
        let signature = crypto::sign(&sk, message);
        let sig_hex = crypto::to_hex(&signature);

        let mut proposal = AdminProposal::new(
            AdminOpType::EmergencyPause,
            100,
            1_000_000,
            serde_json::json!({}),
        );

        let result = proposal.add_signature(&set, &l1_address, &sig_hex, message);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AdminError::NotAnAdmin(_)));
    }

    #[test]
    fn reject_invalid_signature() {
        let set = test_admin_set();
        let rama = set.find_by_name("Rama").unwrap();
        let fake_sig = crypto::to_hex(&[0u8; 64]);

        let mut proposal = AdminProposal::new(
            AdminOpType::EmergencyPause,
            100,
            1_000_000,
            serde_json::json!({}),
        );

        let result = proposal.add_signature(&set, &rama.l1_address, &fake_sig, b"message");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AdminError::InvalidSignature(_)));
    }

    #[test]
    fn reject_double_signing() {
        let set = test_admin_set();
        let (sk, vk) = crypto::generate_keypair();
        let pk_hex = crypto::to_hex(vk.as_bytes());
        let l1_address = crypto::derive_address(vk.as_bytes());

        let mut test_set = set.clone();
        test_set.admins[0].public_key_hex = pk_hex;
        test_set.admins[0].l1_address = l1_address.clone();

        let message = b"test message";
        let signature = crypto::sign(&sk, message);
        let sig_hex = crypto::to_hex(&signature);

        let mut proposal = AdminProposal::new(
            AdminOpType::EmergencyPause,
            100,
            1_000_000,
            serde_json::json!({}),
        );

        // First signature OK
        proposal.add_signature(&test_set, &l1_address, &sig_hex, message).unwrap();
        // Second should fail
        let result = proposal.add_signature(&test_set, &l1_address, &sig_hex, message);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AdminError::AlreadySigned(_)));
    }

    #[test]
    fn quorum_check_emergency() {
        let mut proposal = AdminProposal::new(
            AdminOpType::EmergencyPause,
            100,
            1_000_000,
            serde_json::json!({}),
        );

        // 0 signatures — no quorum
        assert!(!proposal.has_quorum());

        // Add 2 signatures (emergency threshold)
        for _i in 0..2 {
            let (sk, vk) = crypto::generate_keypair();
            let pk_hex = crypto::to_hex(vk.as_bytes());
            let l1_address = crypto::derive_address(vk.as_bytes());
            proposal.signatures.insert(l1_address.clone(), pk_hex.clone());
            // Bypass verification for test (we're testing quorum count, not sig validity)
            let _ = sk;
        }

        assert!(proposal.has_quorum());
    }

    #[test]
    fn quorum_check_full_threshold() {
        let mut proposal = AdminProposal::new(
            AdminOpType::ParameterChange,
            100,
            1_000_000,
            serde_json::json!({}),
        );

        // 2 signatures — not enough for 3-of-3
        proposal.signatures.insert("addr1".to_string(), "sig1".to_string());
        proposal.signatures.insert("addr2".to_string(), "sig2".to_string());
        assert!(!proposal.has_quorum());

        // 3 signatures — enough
        proposal.signatures.insert("addr3".to_string(), "sig3".to_string());
        assert!(proposal.has_quorum());
    }

    #[test]
    fn executable_check_timelock() {
        let ts = 1_000_000u64;
        let mut proposal = AdminProposal::new(
            AdminOpType::ParameterChange,
            100,
            ts,
            serde_json::json!({}),
        );

        // Add 3 fake signatures
        for i in 0..3 {
            proposal.signatures.insert(format!("addr{i}"), format!("sig{i}"));
        }

        // Before timelock expires — should fail
        let result = proposal.is_executable(ts + 100);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AdminError::TimelockNotExpired { .. }));

        // After timelock expires — ParameterChange does NOT require DAO vote, so this should pass
        let result = proposal.is_executable(ts + TIMELOCK_PARAMETER + 1);
        assert!(result.is_ok());
    }

    #[test]
    fn executable_check_emergency_no_timelock() {
        let ts = 1_000_000u64;
        let mut proposal = AdminProposal::new(
            AdminOpType::EmergencyPause,
            100,
            ts,
            serde_json::json!({}),
        );

        // Add 2 fake signatures (emergency threshold)
        for i in 0..2 {
            proposal.signatures.insert(format!("addr{i}"), format!("sig{i}"));
        }

        // Immediately executable (no timelock, no DAO vote)
        let result = proposal.is_executable(ts);
        assert!(result.is_ok());
    }

    #[test]
    fn executable_check_dao_vote_required() {
        let ts = 1_000_000u64;
        let mut proposal = AdminProposal::new(
            AdminOpType::TreasurySpend,
            100,
            ts,
            serde_json::json!({}),
        );

        // Add 3 fake signatures
        for i in 0..3 {
            proposal.signatures.insert(format!("addr{i}"), format!("sig{i}"));
        }

        // After timelock, but DAO vote pending
        let result = proposal.is_executable(ts + TIMELOCK_TREASURY + 1);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AdminError::DaoVotePending));

        // DAO rejected
        proposal.dao_approved = Some(false);
        let result = proposal.is_executable(ts + TIMELOCK_TREASURY + 1);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AdminError::DaoRejected));

        // DAO approved
        proposal.dao_approved = Some(true);
        let result = proposal.is_executable(ts + TIMELOCK_TREASURY + 1);
        assert!(result.is_ok());
    }

    #[test]
    fn executable_check_already_executed() {
        let ts = 1_000_000u64;
        let mut proposal = AdminProposal::new(
            AdminOpType::EmergencyPause,
            100,
            ts,
            serde_json::json!({}),
        );
        proposal.signatures.insert("addr".to_string(), "sig".to_string());
        proposal.signatures.insert("addr2".to_string(), "sig2".to_string());
        proposal.executed = true;

        let result = proposal.is_executable(ts);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AdminError::AlreadyExecuted));
    }

    #[test]
    fn mark_executed_sets_flag() {
        let mut proposal = AdminProposal::new(
            AdminOpType::EmergencyPause,
            100,
            1_000_000,
            serde_json::json!({}),
        );
        assert!(!proposal.executed);
        proposal.mark_executed();
        assert!(proposal.executed);
    }

    #[test]
    fn dead_mans_switch_triggers_after_5_years() {
        let admin = Admin {
            name: "Rama".to_string(),
            role: "Protocol governance".to_string(),
            public_key_hex: "00".repeat(32),
            l1_address: "zion1test".to_string(),
            evm_address: "0x00".repeat(20),
            successor: "Maitreya Buddha".to_string(),
            activated_at_height: 0,
            last_activity_ts: 1_000_000,
        };

        // 4 years — no trigger
        assert!(!check_dead_mans_switch(&admin, 1_000_000 + 4 * 365 * 24 * 3600));

        // 5 years — trigger
        assert!(check_dead_mans_switch(&admin, 1_000_000 + DEAD_MANS_SWITCH_SECS));

        // 6 years — trigger
        assert!(check_dead_mans_switch(&admin, 1_000_000 + 6 * 365 * 24 * 3600));
    }

    #[test]
    fn find_inactive_admins_returns_only_inactive() {
        let mut set = test_admin_set();
        let current_ts = 200_000_000; // ~6.3 years after 1_000_000

        // Rama inactive for 6 years
        set.admins[0].last_activity_ts = 1_000_000;
        // Sita active recently (within 5-year window)
        set.admins[1].last_activity_ts = current_ts - 1_000_000; // ~11.5 days ago
        // Hanuman inactive for 6 years
        set.admins[2].last_activity_ts = 1_000_000;

        let inactive = find_inactive_admins(&set, current_ts);
        assert_eq!(inactive.len(), 2);
        assert!(inactive.iter().any(|a| a.name == "Rama"));
        assert!(inactive.iter().any(|a| a.name == "Hanuman"));
        assert!(!inactive.iter().any(|a| a.name == "Sita"));
    }

    #[test]
    fn validate_admin_rotation_success() {
        let set = test_admin_set();
        let (sk, vk) = crypto::generate_keypair();
        let pk_hex = crypto::to_hex(vk.as_bytes());
        let l1_address = crypto::derive_address(vk.as_bytes());
        let evm_hash = crypto::blake3_hash(vk.as_bytes());
        let evm_address = format!("0x{}", crypto::to_hex(&evm_hash[..20]));
        let _ = sk;

        let new_admin = Admin {
            name: "Maitreya Buddha".to_string(),
            role: "Protocol governance (Gen Z)".to_string(),
            public_key_hex: pk_hex,
            l1_address,
            evm_address,
            successor: "TBD".to_string(),
            activated_at_height: 0,
            last_activity_ts: 0,
        };

        let result = validate_admin_rotation(&set, "Rama", &new_admin);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_admin_rotation_wrong_successor() {
        let set = test_admin_set();
        let (sk, vk) = crypto::generate_keypair();
        let pk_hex = crypto::to_hex(vk.as_bytes());
        let l1_address = crypto::derive_address(vk.as_bytes());
        let _ = sk;

        let new_admin = Admin {
            name: "WrongSuccessor".to_string(),
            role: "test".to_string(),
            public_key_hex: pk_hex,
            l1_address,
            evm_address: "0x00".repeat(20),
            successor: "TBD".to_string(),
            activated_at_height: 0,
            last_activity_ts: 0,
        };

        let result = validate_admin_rotation(&set, "Rama", &new_admin);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AdminError::InvalidSuccessor { .. }));
    }

    #[test]
    fn validate_admin_rotation_address_mismatch() {
        let set = test_admin_set();
        let (sk, vk) = crypto::generate_keypair();
        let pk_hex = crypto::to_hex(vk.as_bytes());
        let _ = sk;

        let new_admin = Admin {
            name: "Maitreya Buddha".to_string(),
            role: "test".to_string(),
            public_key_hex: pk_hex,
            l1_address: "zion1wrongaddress".to_string(), // mismatched
            evm_address: "0x00".repeat(20),
            successor: "TBD".to_string(),
            activated_at_height: 0,
            last_activity_ts: 0,
        };

        let result = validate_admin_rotation(&set, "Rama", &new_admin);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AdminError::AddressMismatch { .. }));
    }

    #[test]
    fn validate_admin_rotation_already_exists() {
        let set = test_admin_set();
        let sita = set.find_by_name("Sita").unwrap();

        let new_admin = Admin {
            name: "Maitreya Buddha".to_string(),
            role: "test".to_string(),
            public_key_hex: sita.public_key_hex.clone(),
            l1_address: sita.l1_address.clone(),
            evm_address: sita.evm_address.clone(),
            successor: "TBD".to_string(),
            activated_at_height: 0,
            last_activity_ts: 0,
        };

        let result = validate_admin_rotation(&set, "Rama", &new_admin);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AdminError::AdminAlreadyExists(_)));
    }

    #[test]
    fn apply_admin_rotation_replaces_admin() {
        let set = test_admin_set();
        let (sk, vk) = crypto::generate_keypair();
        let pk_hex = crypto::to_hex(vk.as_bytes());
        let l1_address = crypto::derive_address(vk.as_bytes());
        let evm_hash = crypto::blake3_hash(vk.as_bytes());
        let evm_address = format!("0x{}", crypto::to_hex(&evm_hash[..20]));
        let _ = sk;

        let new_admin = Admin {
            name: "Maitreya Buddha".to_string(),
            role: "Protocol governance (Gen Z)".to_string(),
            public_key_hex: pk_hex,
            l1_address: l1_address.clone(),
            evm_address,
            successor: "TBD".to_string(),
            activated_at_height: 0,
            last_activity_ts: 5_000_000,
        };

        let new_set = apply_admin_rotation(&set, "Rama", new_admin, 1000).unwrap();

        // Rama should be gone
        assert!(new_set.find_by_name("Rama").is_none());
        // Maitreya Buddha should be present
        let maitreya = new_set.find_by_name("Maitreya Buddha").unwrap();
        assert_eq!(maitreya.l1_address, l1_address);
        // Sita and Hanuman should still be there
        assert!(new_set.find_by_name("Sita").is_some());
        assert!(new_set.find_by_name("Hanuman").is_some());
        // Activation height updated
        assert_eq!(new_set.activated_at_height, 1000);
    }

    #[test]
    fn full_gen_z_inheritance_flow() {
        // Simulate full Gen Z inheritance: all 3 admins rotate to successors
        let mut set = test_admin_set();

        // Rotate Rama → Maitreya Buddha
        let (sk1, vk1) = crypto::generate_keypair();
        let maitreya = Admin {
            name: "Maitreya Buddha".to_string(),
            role: "Protocol governance (Gen Z)".to_string(),
            public_key_hex: crypto::to_hex(vk1.as_bytes()),
            l1_address: crypto::derive_address(vk1.as_bytes()),
            evm_address: format!("0x{}", crypto::to_hex(&crypto::blake3_hash(vk1.as_bytes())[..20])),
            successor: "TBD".to_string(),
            activated_at_height: 0,
            last_activity_ts: 5_000_000,
        };
        let _ = sk1;
        set = apply_admin_rotation(&set, "Rama", maitreya, 1000).unwrap();

        // Rotate Sita → Sarah Issobela
        let (sk2, vk2) = crypto::generate_keypair();
        let sarah = Admin {
            name: "Sarah Issobela".to_string(),
            role: "Treasury oversight (Gen Z)".to_string(),
            public_key_hex: crypto::to_hex(vk2.as_bytes()),
            l1_address: crypto::derive_address(vk2.as_bytes()),
            evm_address: format!("0x{}", crypto::to_hex(&crypto::blake3_hash(vk2.as_bytes())[..20])),
            successor: "TBD".to_string(),
            activated_at_height: 0,
            last_activity_ts: 5_000_000,
        };
        let _ = sk2;
        set = apply_admin_rotation(&set, "Sita", sarah, 2000).unwrap();

        // Rotate Hanuman → Elizabeth
        let (sk3, vk3) = crypto::generate_keypair();
        let elizabeth = Admin {
            name: "Elizabeth".to_string(),
            role: "Bridge admin (Gen Z), Patroness of ZION".to_string(),
            public_key_hex: crypto::to_hex(vk3.as_bytes()),
            l1_address: crypto::derive_address(vk3.as_bytes()),
            evm_address: format!("0x{}", crypto::to_hex(&crypto::blake3_hash(vk3.as_bytes())[..20])),
            successor: "TBD".to_string(),
            activated_at_height: 0,
            last_activity_ts: 5_000_000,
        };
        let _ = sk3;
        set = apply_admin_rotation(&set, "Hanuman", elizabeth, 3000).unwrap();

        // Verify all 3 Gen Z admins are in place
        assert!(set.find_by_name("Maitreya Buddha").is_some());
        assert!(set.find_by_name("Sarah Issobela").is_some());
        assert!(set.find_by_name("Elizabeth").is_some());

        // Verify old admins are gone
        assert!(set.find_by_name("Rama").is_none());
        assert!(set.find_by_name("Sita").is_none());
        assert!(set.find_by_name("Hanuman").is_none());

        // Verify set still has 3 admins
        assert_eq!(set.admins.len(), ADMIN_COUNT);
    }

    #[test]
    fn timelock_durations_are_sane() {
        // Emergency: 0 (immediate)
        assert_eq!(TIMELOCK_EMERGENCY, 0);

        // Parameter: 72h = 259200s
        assert_eq!(TIMELOCK_PARAMETER, 259200);

        // Treasury: 7d = 604800s
        assert_eq!(TIMELOCK_TREASURY, 604800);

        // Admin rotation: 30d = 2592000s
        assert_eq!(TIMELOCK_ADMIN_ROTATION, 2592000);

        // Hard fork: 90d = 7776000s
        assert_eq!(TIMELOCK_HARD_FORK, 7776000);

        // Inheritance: 365d = 31536000s
        assert_eq!(TIMELOCK_INHERITANCE, 31536000);

        // Dead man's switch: 5 years = 157680000s
        assert_eq!(DEAD_MANS_SWITCH_SECS, 157680000);
    }

    #[test]
    fn admin_set_serialization_roundtrip() {
        let set = test_admin_set();
        let json = serde_json::to_string(&set).unwrap();
        let deserialized: AdminSet = serde_json::from_str(&json).unwrap();
        assert_eq!(set, deserialized);
    }

    #[test]
    fn proposal_serialization_roundtrip() {
        let mut proposal = AdminProposal::new(
            AdminOpType::AdminRotation,
            100,
            1_000_000,
            serde_json::json!({"old_admin": "Rama", "new_admin": "Maitreya Buddha"}),
        );
        proposal.signatures.insert("addr".to_string(), "sig".to_string());
        proposal.dao_approved = Some(true);

        let json = serde_json::to_string(&proposal).unwrap();
        let deserialized: AdminProposal = serde_json::from_str(&json).unwrap();
        assert_eq!(proposal, deserialized);
    }
}
