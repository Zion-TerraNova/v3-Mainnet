//! Timelock — 48h delay between proposal passing and execution.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{DaoError, DaoResult};
use crate::types::TIMELOCK_SECS;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timelock {
    pub proposal_id: u64,
    pub started_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub executed: bool,
}

impl Timelock {
    pub fn new(proposal_id: u64) -> Self {
        let now = Utc::now();
        Self {
            proposal_id,
            started_at: now,
            ends_at: now + Duration::seconds(TIMELOCK_SECS as i64),
            executed: false,
        }
    }

    /// Is the timelock still active (can't execute yet)?
    pub fn is_active(&self) -> bool {
        !self.executed && Utc::now() < self.ends_at
    }

    /// Is ready for execution?
    pub fn is_ready(&self) -> bool {
        !self.executed && Utc::now() >= self.ends_at
    }

    /// Remaining hours
    pub fn remaining_hours(&self) -> u64 {
        let remaining = self.ends_at - Utc::now();
        remaining.num_hours().max(0) as u64
    }

    /// Mark as executed
    pub fn mark_executed(&mut self) -> DaoResult<()> {
        if self.is_active() {
            return Err(DaoError::TimelockActive {
                remaining_hours: self.remaining_hours(),
            });
        }
        self.executed = true;
        Ok(())
    }
}
