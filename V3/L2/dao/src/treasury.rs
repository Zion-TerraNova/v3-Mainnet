//! Treasury — multi-sig treasury management.
//!
//! ## Treasury Model
//!
//! - Total: 4,000,000,000 ZION (genesis premine)
//! - Multi-sig: 5-of-7 guardians required for any spend
//! - Daily limit: 100M ZION/day
//! - All spends require passed + timelocked DAO proposal
//!
//! ## Revenue Inflows
//!
//! The treasury receives ongoing revenue from:
//! - 25% of WARP bridge fees (via warp/src/fees.rs)
//! - 25% of L2 bridge fees (via bridge/src/)
//! - 100% of BTC buyback (via cosmic-harmony revenue)

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{DaoError, DaoResult};
use crate::types::{
    Guardian, DAILY_SPEND_LIMIT, FLOWERS_PER_ZION, MULTISIG_THRESHOLD, MULTISIG_TOTAL,
};

// ---------------------------------------------------------------------------
// Treasury Operation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TreasuryOperation {
    /// Send ZION to an address
    Spend {
        recipient: String,
        amount: u64,
        purpose: String,
        proposal_id: u64,
    },
    /// Allocate funds to humanitarian category
    HumanitarianGrant {
        category: String,
        recipient: String,
        amount: u64,
        proposal_id: u64,
    },
    /// Internal rebalance between treasury addresses
    Rebalance {
        from: String,
        to: String,
        amount: u64,
    },
    /// Golden Egg treasure hunt prize payout
    GoldenEggPrize {
        place: u8,         // 1, 2, 3
        recipient: String, // wallet address
        amount: u64,       // ZION amount
        proposal_id: u64,
    },
}

// ---------------------------------------------------------------------------
// Treasury State
// ---------------------------------------------------------------------------

pub struct Treasury {
    /// Guardian set
    guardians: Vec<Guardian>,
    /// Multi-sig threshold
    threshold: u32,
    /// Total balance in flowers (u128 — treasury exceeds u64 at 12-decimal)
    balance: u128,
    /// Pending operations awaiting signatures
    pending: HashMap<String, PendingOperation>,
    /// Daily spend tracking (flowers, u128)
    daily_spent: u128,
    /// Date of last reset
    last_reset: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingOperation {
    pub id: String,
    pub operation: TreasuryOperation,
    pub signatures: Vec<String>, // guardian addresses that signed
    pub created_at: DateTime<Utc>,
}

impl Treasury {
    pub fn new(guardians: Vec<Guardian>, balance: u128) -> Self {
        Self {
            guardians,
            threshold: MULTISIG_THRESHOLD,
            balance,
            pending: HashMap::new(),
            daily_spent: 0,
            last_reset: Utc::now(),
        }
    }

    /// Submit a treasury operation (needs multi-sig approval)
    pub fn submit_operation(
        &mut self,
        id: String,
        operation: TreasuryOperation,
        submitter: &str,
    ) -> DaoResult<()> {
        // Verify submitter is a guardian
        if !self.is_guardian(submitter) {
            return Err(DaoError::Unauthorized(
                "Only guardians can submit treasury operations".into(),
            ));
        }

        // Check balance
        let amount = self.operation_amount(&operation) as u128;
        if amount > self.balance {
            return Err(DaoError::InsufficientTreasuryBalance {
                needed: amount as u64,
                available: self.balance as u64,
            });
        }

        // Check daily limit
        self.maybe_reset_daily();
        if self.daily_spent + amount > DAILY_SPEND_LIMIT {
            return Err(DaoError::DailyLimitExceeded {
                limit: (DAILY_SPEND_LIMIT / FLOWERS_PER_ZION as u128) as u64,
            });
        }

        let pending = PendingOperation {
            id: id.clone(),
            operation,
            signatures: vec![submitter.to_string()],
            created_at: Utc::now(),
        };

        self.pending.insert(id, pending);
        Ok(())
    }

    /// Add a guardian signature to a pending operation
    pub fn add_signature(&mut self, operation_id: &str, guardian: &str) -> DaoResult<bool> {
        if !self.is_guardian(guardian) {
            return Err(DaoError::Unauthorized("Not a guardian".into()));
        }

        let pending = self
            .pending
            .get_mut(operation_id)
            .ok_or_else(|| DaoError::Internal(format!("Operation {} not found", operation_id)))?;

        // Deduplicate
        if !pending.signatures.contains(&guardian.to_string()) {
            pending.signatures.push(guardian.to_string());
        }

        // Check if threshold reached
        Ok(pending.signatures.len() as u32 >= self.threshold)
    }

    /// Execute a fully-signed operation
    pub fn execute(&mut self, operation_id: &str) -> DaoResult<TreasuryOperation> {
        let pending = self
            .pending
            .get(operation_id)
            .ok_or_else(|| DaoError::Internal(format!("Operation {} not found", operation_id)))?;

        if (pending.signatures.len() as u32) < self.threshold {
            return Err(DaoError::InsufficientSignatures {
                needed: self.threshold,
                have: pending.signatures.len() as u32,
                total: MULTISIG_TOTAL,
            });
        }

        let amount = self.operation_amount(&pending.operation) as u128;
        self.balance -= amount;
        self.daily_spent += amount;

        let operation = pending.operation.clone();
        self.pending.remove(operation_id);

        Ok(operation)
    }

    /// Get current balance
    pub fn balance(&self) -> u128 {
        self.balance
    }

    /// Number of configured guardians.
    pub fn guardian_count(&self) -> usize {
        self.guardians.len()
    }

    /// Current multisig threshold.
    pub fn threshold(&self) -> u32 {
        self.threshold
    }

    /// Number of pending operations waiting for signatures/execution.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Signature count for a pending operation.
    pub fn pending_signatures(&self, operation_id: &str) -> Option<usize> {
        self.pending.get(operation_id).map(|p| p.signatures.len())
    }

    /// Public guardian membership check for API/auth layers.
    pub fn is_guardian_address(&self, address: &str) -> bool {
        self.is_guardian(address)
    }

    /// Is address a guardian?
    fn is_guardian(&self, address: &str) -> bool {
        self.guardians
            .iter()
            .any(|g| g.address == address && g.is_active)
    }

    /// Get amount from operation
    fn operation_amount(&self, op: &TreasuryOperation) -> u64 {
        match op {
            TreasuryOperation::Spend { amount, .. } => *amount,
            TreasuryOperation::HumanitarianGrant { amount, .. } => *amount,
            TreasuryOperation::Rebalance { amount, .. } => *amount,
            TreasuryOperation::GoldenEggPrize { amount, .. } => *amount,
        }
    }

    /// Reset daily counter if new day
    fn maybe_reset_daily(&mut self) {
        let now = Utc::now();
        if now.date_naive() != self.last_reset.date_naive() {
            self.daily_spent = 0;
            self.last_reset = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_guardians() -> Vec<Guardian> {
        (1..=7)
            .map(|i| Guardian {
                name: format!("Guardian {}", i),
                address: format!("zion1guardian{}", i),
                public_key: format!("key{}", i),
                is_active: true,
            })
            .collect()
    }

    #[test]
    fn test_submit_and_sign() {
        let mut treasury = Treasury::new(test_guardians(), 1_000_000_000_000u128);

        treasury
            .submit_operation(
                "op1".into(),
                TreasuryOperation::Spend {
                    recipient: "zion1recipient".into(),
                    amount: 100_000_000_000, // 100K ZION
                    purpose: "dev grant".into(),
                    proposal_id: 1,
                },
                "zion1guardian1",
            )
            .unwrap();

        // Need 5 signatures total (already have 1)
        for i in 2..=5 {
            let ready = treasury
                .add_signature("op1", &format!("zion1guardian{}", i))
                .unwrap();
            if i < 5 {
                assert!(!ready);
            } else {
                assert!(ready); // 5th signature = threshold
            }
        }

        // Execute
        let op = treasury.execute("op1").unwrap();
        assert!(matches!(op, TreasuryOperation::Spend { .. }));
    }

    #[test]
    fn test_insufficient_signatures() {
        let mut treasury = Treasury::new(test_guardians(), 1_000_000_000_000u128);

        treasury
            .submit_operation(
                "op1".into(),
                TreasuryOperation::Spend {
                    recipient: "zion1r".into(),
                    amount: 100_000,
                    purpose: "test".into(),
                    proposal_id: 1,
                },
                "zion1guardian1",
            )
            .unwrap();

        // Only 1 signature, need 5
        let result = treasury.execute("op1");
        assert!(matches!(
            result,
            Err(DaoError::InsufficientSignatures { .. })
        ));
    }

    #[test]
    fn test_non_guardian_rejected() {
        let mut treasury = Treasury::new(test_guardians(), 1_000_000_000_000u128);

        let result = treasury.submit_operation(
            "op1".into(),
            TreasuryOperation::Spend {
                recipient: "zion1r".into(),
                amount: 100_000,
                purpose: "test".into(),
                proposal_id: 1,
            },
            "zion1hacker",
        );
        assert!(matches!(result, Err(DaoError::Unauthorized(_))));
    }
}
