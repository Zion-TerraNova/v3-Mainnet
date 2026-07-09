//! SQLite persistence for bridge state.
//!
//! Stores processed lock/burn events, validator confirmations,
//! timelocked operations, and bridge statistics for crash recovery.
//!
//! ## Schema additions (robustness)
//!
//! - `l1_locks.retry_count` â€” how many times relay has attempted this lock
//! - `l1_locks.last_error`  â€” last error message (for ops triage)
//! - `evm_burns.retry_count` / `evm_burns.last_error` â€” same for burns
//! - `timelocked_ops` â€” tracks pending `executeTimelockedMint` calls with
//!   the EVM TX hash, amounts, and expiry timestamp

use crate::types::{BridgeStats, BridgeStatus, EvmBurnEvent, L1LockEvent};
use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;
use tracing::info;

/// Thread-safe SQLite wrapper.
///
/// `rusqlite::Connection` uses interior `RefCell` which is not `Sync`.
/// Wrapping it in a `Mutex` makes `BridgeDb` both `Send` and `Sync` so it
/// can be shared across Tokio tasks with `Arc<BridgeDb>`.
pub struct BridgeDb {
    conn: Mutex<Connection>,
}

impl BridgeDb {
    /// Open or create the bridge database.
    pub fn open(path: &Path) -> Result<Self> {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_tables()?;
        Ok(db)
    }

    fn init_tables(&self) -> Result<()> {
        let conn = self.conn.lock().expect("db lock poisoned");
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS l1_locks (
                l1_tx_hash       TEXT PRIMARY KEY,
                l1_block_height  INTEGER NOT NULL,
                l1_sender        TEXT NOT NULL,
                amount_flowers    TEXT NOT NULL,
                amount_wzion_wei     TEXT NOT NULL,
                target_chain     TEXT NOT NULL,
                evm_recipient    TEXT NOT NULL,
                status           TEXT NOT NULL DEFAULT 'pending',
                confirmations    INTEGER NOT NULL DEFAULT 0,
                detected_at      TEXT NOT NULL,
                completed_at     TEXT,
                evm_tx_hash      TEXT,
                retry_count      INTEGER NOT NULL DEFAULT 0,
                last_error       TEXT
            );

            CREATE TABLE IF NOT EXISTS evm_burns (
                burn_id          TEXT PRIMARY KEY,
                evm_tx_hash      TEXT NOT NULL,
                evm_block_number INTEGER NOT NULL,
                evm_chain        TEXT NOT NULL,
                evm_burner       TEXT NOT NULL,
                amount_wzion_wei     TEXT NOT NULL,
                amount_flowers   TEXT NOT NULL,
                l1_recipient     TEXT NOT NULL,
                status           TEXT NOT NULL DEFAULT 'pending',
                confirmations    INTEGER NOT NULL DEFAULT 0,
                detected_at      TEXT NOT NULL,
                completed_at     TEXT,
                l1_unlock_tx     TEXT,
                retry_count      INTEGER NOT NULL DEFAULT 0,
                last_error       TEXT
            );

            CREATE TABLE IF NOT EXISTS validator_confirmations (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                operation_type   TEXT NOT NULL,  -- 'lock' or 'burn'
                operation_id     TEXT NOT NULL,
                validator_addr   TEXT NOT NULL,
                confirmed_at     TEXT NOT NULL,
                UNIQUE(operation_type, operation_id, validator_addr)
            );

            -- Tracks large-amount locks that require executeTimelockedMint()
            -- after the 24-hour delay expires on-chain.
            CREATE TABLE IF NOT EXISTS timelocked_ops (
                l1_tx_hash       TEXT PRIMARY KEY,
                evm_chain        TEXT NOT NULL,
                bridge_contract  TEXT NOT NULL,
                amount_wzion_wei TEXT NOT NULL,
                evm_recipient    TEXT NOT NULL,
                timelock_expires_at TEXT NOT NULL,   -- ISO-8601 UTC
                status           TEXT NOT NULL DEFAULT 'pending',  -- pending | executed | failed
                evm_execute_tx   TEXT,               -- executeTimelockedMint tx hash
                last_error       TEXT,
                created_at       TEXT NOT NULL,
                executed_at      TEXT
            );

            CREATE TABLE IF NOT EXISTS bridge_state (
                key              TEXT PRIMARY KEY,
                value            TEXT NOT NULL,
                updated_at       TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_locks_status ON l1_locks(status);
            CREATE INDEX IF NOT EXISTS idx_burns_status ON evm_burns(status);
            CREATE INDEX IF NOT EXISTS idx_confirmations_op ON validator_confirmations(operation_type, operation_id);
            CREATE INDEX IF NOT EXISTS idx_timelocked_status ON timelocked_ops(status);
            CREATE INDEX IF NOT EXISTS idx_timelocked_expires ON timelocked_ops(timelock_expires_at);

            -- Migration: add new columns to existing tables if they don't exist yet.
            -- SQLite does not support IF NOT EXISTS on ALTER TABLE, so we use a
            -- try-and-ignore approach by running each in its own statement and
            -- catching the error at the Rust level.
            ",
        )?;

        // Add retry_count / last_error to existing l1_locks and evm_burns tables
        // (safe to run on both fresh and already-existing databases).
        for alter in &[
            "ALTER TABLE l1_locks ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE l1_locks ADD COLUMN last_error TEXT",
            "ALTER TABLE evm_burns ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE evm_burns ADD COLUMN last_error TEXT",
        ] {
            // Ignore "duplicate column" errors from already-migrated DBs.
            let _ = conn.execute(alter, []);
        }

        // Migration: convert amount_flowers from INTEGER to TEXT for both tables.
        // Pre-fork premine locks have amount_flowers up to 1.67e19 which overflows
        // SQLite's INTEGER (i64, max 9.2e18). Store as TEXT instead.
        // SQLite doesn't support ALTER COLUMN, so we use the table-rebuild approach.
        for (table, cols) in &[
            ("l1_locks", "l1_tx_hash TEXT PRIMARY KEY, l1_block_height INTEGER NOT NULL, l1_sender TEXT NOT NULL, amount_flowers TEXT NOT NULL, amount_wzion_wei TEXT NOT NULL, target_chain TEXT NOT NULL, evm_recipient TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', confirmations INTEGER NOT NULL DEFAULT 0, detected_at TEXT NOT NULL, completed_at TEXT, evm_tx_hash TEXT, retry_count INTEGER NOT NULL DEFAULT 0, last_error TEXT"),
            ("evm_burns", "burn_id TEXT PRIMARY KEY, evm_tx_hash TEXT NOT NULL, evm_block_number INTEGER NOT NULL, evm_chain TEXT NOT NULL, evm_burner TEXT NOT NULL, amount_wzion_wei TEXT NOT NULL, amount_flowers TEXT NOT NULL, l1_recipient TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', confirmations INTEGER NOT NULL DEFAULT 0, detected_at TEXT NOT NULL, completed_at TEXT, l1_unlock_tx TEXT, retry_count INTEGER NOT NULL DEFAULT 0, last_error TEXT"),
        ] {
            // Check if amount_flowers column is INTEGER (needs migration)
            let needs_migration: bool = {
                let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
                let rows = stmt.query_map([], |row| {
                    let name: String = row.get(1)?;
                    let col_type: String = row.get(2)?;
                    Ok((name, col_type))
                })?;
                let mut found = false;
                for (name, col_type) in rows.flatten() {
                    if name == "amount_flowers" && col_type == "INTEGER" {
                        found = true;
                    }
                }
                found
            };
            if needs_migration {
                info!("Migrating {}.amount_flowers from INTEGER to TEXT", table);
                conn.execute_batch(&format!(
                    "CREATE TABLE _{table}_new ({cols});
                     INSERT INTO _{table}_new SELECT * FROM {table};
                     DROP TABLE {table};
                     ALTER TABLE _{table}_new RENAME TO {table};",
                    table = table, cols = cols
                ))?;
                // Recreate indexes
                let idx = if *table == "l1_locks" { "idx_locks_status" } else { "idx_burns_status" };
                let status_col = "status"; // both tables use "status" column
                conn.execute(&format!("CREATE INDEX IF NOT EXISTS {} ON {}({})", idx, table, status_col), [])?;
                info!("Migration of {} complete", table);
            }
        }

        info!("đź“¦ Bridge database initialized");
        Ok(())
    }

    /// Insert a new L1 lock event.
    /// Uses INSERT OR IGNORE so that duplicate TX hashes are silently skipped â€”
    /// prevents replay attacks where an attacker resends a completed lock to reset
    /// its status back to Pending and trigger a second mint.
    pub fn insert_lock(&self, lock: &L1LockEvent) -> Result<()> {
        self.conn.lock().expect("db lock poisoned").execute(
            "INSERT OR IGNORE INTO l1_locks
             (l1_tx_hash, l1_block_height, l1_sender, amount_flowers, amount_wzion_wei,
              target_chain, evm_recipient, status, confirmations, detected_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                lock.l1_tx_hash,
                lock.l1_block_height,
                lock.l1_sender,
                lock.amount_flowers.to_string(),
                lock.amount_wzion_wei,
                lock.target_chain,
                lock.evm_recipient,
                format!("{:?}", lock.status),
                lock.confirmations,
                lock.detected_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Insert a new EVM burn event.
    /// Uses INSERT OR IGNORE so that duplicate burn IDs are silently skipped â€”
    /// prevents processing the same burn event twice if the watcher re-scans old blocks.
    pub fn insert_burn(&self, burn: &EvmBurnEvent) -> Result<()> {
        self.conn.lock().expect("db lock poisoned").execute(
            "INSERT OR IGNORE INTO evm_burns
             (burn_id, evm_tx_hash, evm_block_number, evm_chain, evm_burner,
              amount_wzion_wei, amount_flowers, l1_recipient, status, confirmations, detected_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                burn.burn_id,
                burn.evm_tx_hash,
                burn.evm_block_number,
                burn.evm_chain,
                burn.evm_burner,
                burn.amount_wzion_wei,
                burn.amount_flowers.to_string(),
                burn.l1_recipient,
                format!("{:?}", burn.status),
                burn.confirmations,
                burn.detected_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Update lock status.
    pub fn update_lock_status(&self, l1_tx_hash: &str, status: BridgeStatus) -> Result<()> {
        self.conn.lock().expect("db lock poisoned").execute(
            "UPDATE l1_locks SET status = ?1 WHERE l1_tx_hash = ?2",
            params![format!("{:?}", status), l1_tx_hash],
        )?;
        Ok(())
    }

    /// Update burn status.
    pub fn update_burn_status(&self, burn_id: &str, status: BridgeStatus) -> Result<()> {
        self.conn.lock().expect("db lock poisoned").execute(
            "UPDATE evm_burns SET status = ?1 WHERE burn_id = ?2",
            params![format!("{:?}", status), burn_id],
        )?;
        Ok(())
    }

    /// Get or set a bridge state key-value pair.
    pub fn get_state(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("db lock poisoned");
        let mut stmt = conn.prepare("SELECT value FROM bridge_state WHERE key = ?1")?;
        let result = stmt.query_row(params![key], |row| row.get(0)).ok();
        Ok(result)
    }

    pub fn set_state(&self, key: &str, value: &str) -> Result<()> {
        self.conn.lock().expect("db lock poisoned").execute(
            "INSERT OR REPLACE INTO bridge_state (key, value, updated_at) VALUES (?1, ?2, datetime('now'))",
            params![key, value],
        )?;
        Ok(())
    }

    /// Get last processed L1 block height.
    pub fn get_last_l1_height(&self) -> Result<u64> {
        self.get_state("last_l1_height")?
            .map(|s| s.parse().unwrap_or(0))
            .ok_or_else(|| anyhow::anyhow!("No last L1 height"))
            .or(Ok(0))
    }

    /// Set last processed L1 block height.
    pub fn set_last_l1_height(&self, height: u64) -> Result<()> {
        self.set_state("last_l1_height", &height.to_string())
    }

    /// Get bridge statistics.
    pub fn get_stats(&self) -> Result<BridgeStats> {
        let total_locks: u64 = self.conn.lock().expect("db lock poisoned").query_row(
            "SELECT COUNT(*) FROM l1_locks WHERE status = 'Completed'",
            [],
            |r| r.get(0),
        )?;

        let total_burns: u64 = self.conn.lock().expect("db lock poisoned").query_row(
            "SELECT COUNT(*) FROM evm_burns WHERE status = 'Completed'",
            [],
            |r| r.get(0),
        )?;

        Ok(BridgeStats {
            total_operations: total_locks + total_burns,
            ..Default::default()
        })
    }

    /// Get pending L1 lock events.
    ///
    /// Includes `Executing` status so that locks stuck mid-processing after a
    /// crash or restart are re-queued. The relayer's `submitLockProof()` is
    /// idempotent on-chain (increments confirmations), so re-submission is safe.
    pub fn get_pending_locks(&self) -> Result<Vec<L1LockEvent>> {
        let conn = self.conn.lock().expect("db lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT l1_tx_hash, l1_block_height, l1_sender, amount_flowers, amount_wzion_wei,
                    target_chain, evm_recipient, status, confirmations, detected_at
             FROM l1_locks WHERE status IN ('Pending', 'Confirmed', 'Executing')",
        )?;
        let rows = stmt.query_map([], |row| {
            let flowers_str: String = row.get(3)?;
            Ok(L1LockEvent {
                l1_tx_hash: row.get(0)?,
                l1_block_height: row.get(1)?,
                l1_sender: row.get(2)?,
                amount_flowers: flowers_str.parse().unwrap_or(0),
                amount_wzion_wei: row.get(4)?,
                target_chain: row.get(5)?,
                evm_recipient: row.get(6)?,
                status: BridgeStatus::Pending, // simplified
                confirmations: row.get(8)?,
                detected_at: chrono::Utc::now(), // simplified
            })
        })?;
        let mut locks = Vec::new();
        for row in rows {
            locks.push(row?);
        }
        Ok(locks)
    }

    /// Get pending EVM burn events.
    ///
    /// Includes `Executing` status so that burns stuck mid-processing after a
    /// crash or restart are re-queued.
    pub fn get_pending_burns(&self) -> Result<Vec<EvmBurnEvent>> {
        let conn = self.conn.lock().expect("db lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT burn_id, evm_tx_hash, evm_block_number, evm_chain, evm_burner,
                    amount_wzion_wei, amount_flowers, l1_recipient, status, confirmations, detected_at
             FROM evm_burns WHERE status IN ('Pending', 'Confirmed', 'Executing')",
        )?;
        let rows = stmt.query_map([], |row| {
            let flowers_str: String = row.get(6)?;
            Ok(EvmBurnEvent {
                burn_id: row.get(0)?,
                evm_tx_hash: row.get(1)?,
                evm_block_number: row.get(2)?,
                evm_chain: row.get(3)?,
                evm_burner: row.get(4)?,
                amount_wzion_wei: row.get(5)?,
                amount_flowers: flowers_str.parse().unwrap_or(0),
                l1_recipient: row.get(7)?,
                status: BridgeStatus::Pending,
                confirmations: row.get(9)?,
                detected_at: chrono::Utc::now(),
            })
        })?;
        let mut burns = Vec::new();
        for row in rows {
            burns.push(row?);
        }
        Ok(burns)
    }

    /// Add a validator confirmation.
    pub fn add_confirmation(&self, op_type: &str, op_id: &str, validator: &str) -> Result<bool> {
        let result = self.conn.lock().expect("db lock poisoned").execute(
            "INSERT OR IGNORE INTO validator_confirmations (operation_type, operation_id, validator_addr, confirmed_at)
             VALUES (?1, ?2, ?3, datetime('now'))",
            params![op_type, op_id, validator],
        )?;
        Ok(result > 0) // true if inserted (not duplicate)
    }

    /// Get confirmation count for an operation.
    pub fn get_confirmation_count(&self, op_type: &str, op_id: &str) -> Result<u32> {
        let count: u32 = self.conn.lock().expect("db lock poisoned").query_row(
            "SELECT COUNT(*) FROM validator_confirmations WHERE operation_type = ?1 AND operation_id = ?2",
            params![op_type, op_id],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    /// Count total locks/burns by status.
    pub fn count_by_status(&self, table: &str, status: &str) -> Result<u64> {
        // Defense-in-depth: `table` is interpolated directly into SQL, so restrict
        // it to the known bridge tables to prevent SQL injection if a future caller
        // ever forwards untrusted input.
        if !matches!(table, "l1_locks" | "evm_burns") {
            anyhow::bail!("count_by_status: invalid table name: {table}");
        }
        let query = format!("SELECT COUNT(*) FROM {} WHERE status = ?1", table);
        let count: u64 = self.conn.lock().expect("db lock poisoned").query_row(
            &query,
            params![status],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Retry management
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Increment retry_count and store last_error for a lock.
    /// Returns the new retry_count so callers can decide whether to give up.
    pub fn increment_lock_retry(&self, l1_tx_hash: &str, error: &str) -> Result<u32> {
        let conn = self.conn.lock().expect("db lock poisoned");
        conn.execute(
            "UPDATE l1_locks SET retry_count = retry_count + 1, last_error = ?1 WHERE l1_tx_hash = ?2",
            params![error, l1_tx_hash],
        )?;
        let count: u32 = conn.query_row(
            "SELECT retry_count FROM l1_locks WHERE l1_tx_hash = ?1",
            params![l1_tx_hash],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    /// Increment retry_count and store last_error for a burn.
    pub fn increment_burn_retry(&self, burn_id: &str, error: &str) -> Result<u32> {
        let conn = self.conn.lock().expect("db lock poisoned");
        conn.execute(
            "UPDATE evm_burns SET retry_count = retry_count + 1, last_error = ?1 WHERE burn_id = ?2",
            params![error, burn_id],
        )?;
        let count: u32 = conn.query_row(
            "SELECT retry_count FROM evm_burns WHERE burn_id = ?1",
            params![burn_id],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    /// Get failed locks that still have retry_count < max_retries.
    pub fn get_retryable_locks(&self, max_retries: u32) -> Result<Vec<L1LockEvent>> {
        let conn = self.conn.lock().expect("db lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT l1_tx_hash, l1_block_height, l1_sender, amount_flowers, amount_wzion_wei,
                    target_chain, evm_recipient, status, confirmations, detected_at
             FROM l1_locks WHERE status = 'Failed' AND retry_count < ?1",
        )?;
        let rows = stmt.query_map(params![max_retries], |row| {
            let flowers_str: String = row.get(3)?;
            Ok(L1LockEvent {
                l1_tx_hash: row.get(0)?,
                l1_block_height: row.get(1)?,
                l1_sender: row.get(2)?,
                amount_flowers: flowers_str.parse().unwrap_or(0),
                amount_wzion_wei: row.get(4)?,
                target_chain: row.get(5)?,
                evm_recipient: row.get(6)?,
                status: BridgeStatus::Failed,
                confirmations: row.get(8)?,
                detected_at: chrono::Utc::now(),
            })
        })?;
        let mut locks = Vec::new();
        for row in rows {
            locks.push(row?);
        }
        Ok(locks)
    }

    /// Get failed burns that still have retry_count < max_retries.
    pub fn get_retryable_burns(&self, max_retries: u32) -> Result<Vec<EvmBurnEvent>> {
        let conn = self.conn.lock().expect("db lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT burn_id, evm_tx_hash, evm_block_number, evm_chain, evm_burner,
                    amount_wzion_wei, amount_flowers, l1_recipient, status, confirmations, detected_at
             FROM evm_burns WHERE status = 'Failed' AND retry_count < ?1",
        )?;
        let rows = stmt.query_map(params![max_retries], |row| {
            let flowers_str: String = row.get(6)?;
            Ok(EvmBurnEvent {
                burn_id: row.get(0)?,
                evm_tx_hash: row.get(1)?,
                evm_block_number: row.get(2)?,
                evm_chain: row.get(3)?,
                evm_burner: row.get(4)?,
                amount_wzion_wei: row.get(5)?,
                amount_flowers: flowers_str.parse().unwrap_or(0),
                l1_recipient: row.get(7)?,
                status: BridgeStatus::Failed,
                confirmations: row.get(9)?,
                detected_at: chrono::Utc::now(),
            })
        })?;
        let mut burns = Vec::new();
        for row in rows {
            burns.push(row?);
        }
        Ok(burns)
    }

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Timelocked operations
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Insert a new timelocked operation (large lock awaiting executeTimelockedMint).
    pub fn insert_timelocked_op(
        &self,
        l1_tx_hash: &str,
        evm_chain: &str,
        bridge_contract: &str,
        amount_wzion_wei: &str,
        evm_recipient: &str,
        timelock_expires_at: &str,
    ) -> Result<()> {
        self.conn.lock().expect("db lock poisoned").execute(
            "INSERT OR IGNORE INTO timelocked_ops
             (l1_tx_hash, evm_chain, bridge_contract, amount_wzion_wei, evm_recipient,
              timelock_expires_at, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', datetime('now'))",
            params![
                l1_tx_hash,
                evm_chain,
                bridge_contract,
                amount_wzion_wei,
                evm_recipient,
                timelock_expires_at,
            ],
        )?;
        Ok(())
    }

    /// Get all pending timelocked ops whose expiry has passed.
    /// `now_iso` should be an ISO-8601 UTC string like `"2026-06-24T16:52:00Z"`.
    pub fn get_expired_timelocked_ops(&self, now_iso: &str) -> Result<Vec<TimelockRecord>> {
        let conn = self.conn.lock().expect("db lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT l1_tx_hash, evm_chain, bridge_contract, amount_wzion_wei, evm_recipient,
                    timelock_expires_at
             FROM timelocked_ops
             WHERE status = 'pending' AND timelock_expires_at <= ?1",
        )?;
        let rows = stmt.query_map(params![now_iso], |row| {
            Ok(TimelockRecord {
                l1_tx_hash: row.get(0)?,
                evm_chain: row.get(1)?,
                bridge_contract: row.get(2)?,
                amount_wzion_wei: row.get(3)?,
                evm_recipient: row.get(4)?,
                timelock_expires_at: row.get(5)?,
            })
        })?;
        let mut ops = Vec::new();
        for row in rows {
            ops.push(row?);
        }
        Ok(ops)
    }

    /// Mark a timelocked op as executed.
    pub fn mark_timelocked_executed(&self, l1_tx_hash: &str, evm_tx: &str) -> Result<()> {
        self.conn.lock().expect("db lock poisoned").execute(
            "UPDATE timelocked_ops SET status = 'executed', evm_execute_tx = ?1,
             executed_at = datetime('now') WHERE l1_tx_hash = ?2",
            params![evm_tx, l1_tx_hash],
        )?;
        Ok(())
    }

    /// Mark a timelocked op as failed.
    pub fn mark_timelocked_failed(&self, l1_tx_hash: &str, error: &str) -> Result<()> {
        self.conn.lock().expect("db lock poisoned").execute(
            "UPDATE timelocked_ops SET status = 'failed', last_error = ?1 WHERE l1_tx_hash = ?2",
            params![error, l1_tx_hash],
        )?;
        Ok(())
    }
}

/// A row from the `timelocked_ops` table (for executeTimelockedMint polling).
#[derive(Debug, Clone)]
pub struct TimelockRecord {
    pub l1_tx_hash: String,
    pub evm_chain: String,
    pub bridge_contract: String,
    pub amount_wzion_wei: String,
    pub evm_recipient: String,
    pub timelock_expires_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use chrono::Utc;
    use tempfile::TempDir;

    fn test_db() -> (BridgeDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test_bridge.db");
        let db = BridgeDb::open(&db_path).unwrap();
        (db, dir)
    }

    fn sample_lock(tx_hash: &str) -> L1LockEvent {
        L1LockEvent {
            l1_tx_hash: tx_hash.into(),
            l1_block_height: 1000,
            l1_sender: "zion1qsender123".into(),
            amount_flowers: 5_000_000_000_000_000, // 5000 ZION (V3: 12-dec flowers)
            amount_wzion_wei: "5000000000000000000000".into(),
            target_chain: "base".into(),
            evm_recipient: "0x1234567890abcdef1234567890abcdef12345678".into(),
            detected_at: Utc::now(),
            status: BridgeStatus::Pending,
            confirmations: 0,
        }
    }

    fn sample_burn(burn_id: &str) -> EvmBurnEvent {
        EvmBurnEvent {
            evm_tx_hash: "0xdeadbeef".into(),
            evm_block_number: 50000,
            evm_chain: "base".into(),
            evm_burner: "0xaaabbbccc".into(),
            amount_wzion_wei: "1000000000000000000000".into(), // 1000 wZION
            amount_flowers: 1_000_000_000,                     // 1000 ZION × 1e6 (post-3.0.3)
            l1_recipient: "zion1qrecipient".into(),
            burn_id: burn_id.into(),
            detected_at: Utc::now(),
            status: BridgeStatus::Pending,
            confirmations: 0,
        }
    }

    #[test]
    fn test_db_open_and_init() {
        let (db, _dir) = test_db();
        // Tables should be created
        let stats = db.get_stats().unwrap();
        assert_eq!(stats.total_operations, 0);
    }

    #[test]
    fn test_insert_and_query_lock() {
        let (db, _dir) = test_db();
        let lock = sample_lock("tx001");
        db.insert_lock(&lock).unwrap();

        let pending = db.get_pending_locks().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].l1_tx_hash, "tx001");
        assert_eq!(pending[0].amount_flowers, 5_000_000_000_000_000);
        assert_eq!(pending[0].target_chain, "base");
    }

    #[test]
    fn test_insert_and_query_burn() {
        let (db, _dir) = test_db();
        let burn = sample_burn("burn001");
        db.insert_burn(&burn).unwrap();

        let pending = db.get_pending_burns().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].burn_id, "burn001");
        assert_eq!(pending[0].amount_flowers, 1_000_000_000); // 1000 ZION × 1e6 (post-3.0.3)
    }

    #[test]
    fn test_update_lock_status_to_completed() {
        let (db, _dir) = test_db();
        db.insert_lock(&sample_lock("tx002")).unwrap();

        // Status update to Completed
        db.update_lock_status("tx002", BridgeStatus::Completed)
            .unwrap();

        // Should no longer appear in pending
        let pending = db.get_pending_locks().unwrap();
        assert_eq!(pending.len(), 0);

        // Stats should reflect completed
        let stats = db.get_stats().unwrap();
        assert_eq!(stats.total_operations, 1);
    }

    #[test]
    fn test_update_burn_status_to_completed() {
        let (db, _dir) = test_db();
        db.insert_burn(&sample_burn("burn002")).unwrap();

        db.update_burn_status("burn002", BridgeStatus::Completed)
            .unwrap();

        let pending = db.get_pending_burns().unwrap();
        assert_eq!(pending.len(), 0);

        let stats = db.get_stats().unwrap();
        assert_eq!(stats.total_operations, 1);
    }

    #[test]
    fn test_state_get_set() {
        let (db, _dir) = test_db();

        // Initially no state
        assert!(db.get_state("test_key").unwrap().is_none());

        // Set and get
        db.set_state("test_key", "test_value").unwrap();
        assert_eq!(db.get_state("test_key").unwrap().unwrap(), "test_value");

        // Overwrite
        db.set_state("test_key", "new_value").unwrap();
        assert_eq!(db.get_state("test_key").unwrap().unwrap(), "new_value");
    }

    #[test]
    fn test_last_l1_height() {
        let (db, _dir) = test_db();

        // Default 0
        assert_eq!(db.get_last_l1_height().unwrap(), 0);

        db.set_last_l1_height(12345).unwrap();
        assert_eq!(db.get_last_l1_height().unwrap(), 12345);

        db.set_last_l1_height(99999).unwrap();
        assert_eq!(db.get_last_l1_height().unwrap(), 99999);
    }

    #[test]
    fn test_validator_confirmations() {
        let (db, _dir) = test_db();

        // Add confirmations
        assert!(db.add_confirmation("lock", "tx001", "0xAAA").unwrap());
        assert!(db.add_confirmation("lock", "tx001", "0xBBB").unwrap());
        assert!(db.add_confirmation("lock", "tx001", "0xCCC").unwrap());

        // Duplicate should return false
        assert!(!db.add_confirmation("lock", "tx001", "0xAAA").unwrap());

        // Count
        assert_eq!(db.get_confirmation_count("lock", "tx001").unwrap(), 3);

        // Different operation
        assert_eq!(db.get_confirmation_count("lock", "tx002").unwrap(), 0);
    }

    #[test]
    fn test_multiple_locks_and_burns() {
        let (db, _dir) = test_db();

        // Insert 5 locks, 3 burns
        for i in 0..5 {
            db.insert_lock(&sample_lock(&format!("lock_{}", i)))
                .unwrap();
        }
        for i in 0..3 {
            db.insert_burn(&sample_burn(&format!("burn_{}", i)))
                .unwrap();
        }

        assert_eq!(db.get_pending_locks().unwrap().len(), 5);
        assert_eq!(db.get_pending_burns().unwrap().len(), 3);

        // Complete some
        db.update_lock_status("lock_0", BridgeStatus::Completed)
            .unwrap();
        db.update_lock_status("lock_1", BridgeStatus::Completed)
            .unwrap();
        db.update_burn_status("burn_0", BridgeStatus::Completed)
            .unwrap();

        assert_eq!(db.get_pending_locks().unwrap().len(), 3);
        assert_eq!(db.get_pending_burns().unwrap().len(), 2);

        let stats = db.get_stats().unwrap();
        assert_eq!(stats.total_operations, 3); // 2 locks + 1 burn completed
    }

    #[test]
    fn test_insert_lock_ignore_duplicate() {
        // INSERT OR IGNORE: second insert of same TX hash is silently skipped.
        // The original row (with original confirmations) is preserved.
        // This protects against replay attacks where an attacker resends a completed
        // lock to reset its status or trigger a second mint.
        let (db, _dir) = test_db();
        let mut lock = sample_lock("tx_dup_ignore");
        lock.confirmations = 0;
        lock.status = BridgeStatus::Pending;
        db.insert_lock(&lock).unwrap();

        // Mark as Completed
        db.update_lock_status("tx_dup_ignore", BridgeStatus::Completed)
            .unwrap();

        // Attacker tries to replay: insert same TX with Pending status
        lock.confirmations = 0;
        lock.status = BridgeStatus::Pending;
        db.insert_lock(&lock).unwrap(); // INSERT OR IGNORE â†’ no-op

        // Completed status must be preserved (not reset to Pending)
        let pending = db.get_pending_locks().unwrap();
        assert_eq!(
            pending.len(),
            0,
            "Replay attack must not reset Completed â†’ Pending"
        );
    }

    #[test]
    fn test_count_by_status() {
        let (db, _dir) = test_db();

        db.insert_lock(&sample_lock("a")).unwrap();
        db.insert_lock(&sample_lock("b")).unwrap();
        db.insert_lock(&sample_lock("c")).unwrap();
        db.update_lock_status("a", BridgeStatus::Completed).unwrap();

        assert_eq!(db.count_by_status("l1_locks", "Completed").unwrap(), 1);
        assert_eq!(db.count_by_status("l1_locks", "Pending").unwrap(), 2);
    }

    #[test]
    fn test_failed_status() {
        let (db, _dir) = test_db();
        db.insert_lock(&sample_lock("fail_tx")).unwrap();
        db.update_lock_status("fail_tx", BridgeStatus::Failed)
            .unwrap();

        // Failed should not be in pending
        assert_eq!(db.get_pending_locks().unwrap().len(), 0);
        assert_eq!(db.count_by_status("l1_locks", "Failed").unwrap(), 1);
    }

    #[test]
    fn test_executing_status_recovered() {
        // After a crash, locks in 'Executing' status should be recovered
        // by get_pending_locks() so they can be re-processed on restart.
        let (db, _dir) = test_db();
        db.insert_lock(&sample_lock("exec_tx")).unwrap();
        db.update_lock_status("exec_tx", BridgeStatus::Executing)
            .unwrap();

        // Executing should appear in pending (for crash recovery)
        let pending = db.get_pending_locks().unwrap();
        assert_eq!(pending.len(), 1, "Executing locks must be recoverable");
        assert_eq!(pending[0].l1_tx_hash, "exec_tx");
    }

    // ── New robustness tests ─────────────────────────────────────────────────

    #[test]
    fn test_increment_lock_retry() {
        let (db, _dir) = test_db();
        db.insert_lock(&sample_lock("retry_tx")).unwrap();

        let count1 = db.increment_lock_retry("retry_tx", "RPC timeout").unwrap();
        assert_eq!(count1, 1);

        let count2 = db
            .increment_lock_retry("retry_tx", "RPC timeout again")
            .unwrap();
        assert_eq!(count2, 2);
    }

    #[test]
    fn test_increment_burn_retry() {
        let (db, _dir) = test_db();
        db.insert_burn(&sample_burn("retry_burn")).unwrap();

        let count = db
            .increment_burn_retry("retry_burn", "gas too low")
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_get_retryable_locks() {
        let (db, _dir) = test_db();
        db.insert_lock(&sample_lock("r1")).unwrap();
        db.insert_lock(&sample_lock("r2")).unwrap();
        db.insert_lock(&sample_lock("r3")).unwrap();

        // Mark all as Failed
        db.update_lock_status("r1", BridgeStatus::Failed).unwrap();
        db.update_lock_status("r2", BridgeStatus::Failed).unwrap();
        db.update_lock_status("r3", BridgeStatus::Failed).unwrap();

        // r3 has been retried 5 times already — should be excluded at max 5
        for _ in 0..5 {
            db.increment_lock_retry("r3", "err").unwrap();
        }

        let retryable = db.get_retryable_locks(5).unwrap();
        // r1 and r2 have 0 retries (< 5), r3 has 5 retries (not < 5)
        assert_eq!(retryable.len(), 2);
        let hashes: Vec<_> = retryable.iter().map(|l| l.l1_tx_hash.as_str()).collect();
        assert!(hashes.contains(&"r1"));
        assert!(hashes.contains(&"r2"));
        assert!(!hashes.contains(&"r3"));
    }

    #[test]
    fn test_get_retryable_burns() {
        let (db, _dir) = test_db();
        db.insert_burn(&sample_burn("b_retry")).unwrap();
        db.update_burn_status("b_retry", BridgeStatus::Failed)
            .unwrap();

        let retryable = db.get_retryable_burns(5).unwrap();
        assert_eq!(retryable.len(), 1);
        assert_eq!(retryable[0].burn_id, "b_retry");
    }

    #[test]
    fn test_timelocked_ops_full_flow() {
        let (db, _dir) = test_db();

        // Insert
        db.insert_timelocked_op(
            "tl_tx_001",
            "base",
            "0xBridgeContract",
            "1000000000000000000000000",
            "0xRecipient",
            "2026-01-01T00:00:00Z",
        )
        .unwrap();

        // Not expired yet (far future)
        let not_expired = db
            .get_expired_timelocked_ops("2025-01-01T00:00:00Z")
            .unwrap();
        assert_eq!(not_expired.len(), 0);

        // Expired now
        let expired = db
            .get_expired_timelocked_ops("2026-06-01T00:00:00Z")
            .unwrap();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].l1_tx_hash, "tl_tx_001");
        assert_eq!(expired[0].evm_chain, "base");

        // Mark as executed
        db.mark_timelocked_executed("tl_tx_001", "0xExecTxHash")
            .unwrap();

        // Should no longer appear in expired (status = executed)
        let after_exec = db
            .get_expired_timelocked_ops("2026-06-01T00:00:00Z")
            .unwrap();
        assert_eq!(after_exec.len(), 0);
    }

    #[test]
    fn test_timelocked_ops_ignore_duplicate() {
        let (db, _dir) = test_db();

        // Insert same hash twice — INSERT OR IGNORE must not fail or double-count
        db.insert_timelocked_op(
            "dup_tl",
            "base",
            "0xBridge",
            "100",
            "0xRec",
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
        db.insert_timelocked_op(
            "dup_tl",
            "base",
            "0xBridge",
            "100",
            "0xRec",
            "2026-01-01T00:00:00Z",
        )
        .unwrap();

        let expired = db
            .get_expired_timelocked_ops("2026-06-01T00:00:00Z")
            .unwrap();
        assert_eq!(expired.len(), 1, "Duplicate insert must be ignored");
    }

    #[test]
    fn test_timelocked_ops_mark_failed() {
        let (db, _dir) = test_db();

        db.insert_timelocked_op(
            "fail_tl",
            "base",
            "0xBridge",
            "100",
            "0xRec",
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
        db.mark_timelocked_failed("fail_tl", "gas price too high")
            .unwrap();

        // Status is now 'failed' — should not appear in expired pending list
        let expired = db
            .get_expired_timelocked_ops("2026-06-01T00:00:00Z")
            .unwrap();
        assert_eq!(expired.len(), 0);
    }
}
