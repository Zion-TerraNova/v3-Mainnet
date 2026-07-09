//! SQLite persistence layer for HTLC records.
//!
//! Schema (single table `htlc_locks`):
//!
//! ```sql
//! CREATE TABLE htlc_locks (
//!   hash_hex           TEXT PRIMARY KEY,
//!   locker_address     TEXT NOT NULL,
//!   amount_flowers      INTEGER NOT NULL,
//!   lock_tx_id         TEXT NOT NULL,
//!   lock_block_height  INTEGER NOT NULL,
//!   expires_at         INTEGER NOT NULL,
//!   counterparty_chain TEXT NOT NULL,
//!   counterparty_addr  TEXT NOT NULL,
//!   state              TEXT NOT NULL DEFAULT 'pending',
//!   release_tx_id      TEXT,
//!   release_recipient  TEXT,
//!   preimage_hex       TEXT,
//!   created_at         TEXT NOT NULL,
//!   updated_at         TEXT NOT NULL
//! );
//! ```

use crate::error::{SwapError, SwapResult};
use crate::types::{HtlcRecord, SwapState};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

// ─── SwapDb ──────────────────────────────────────────────────────────────────

/// Thread-safe HTLC database handle.
#[derive(Clone)]
pub struct SwapDb {
    conn: Arc<Mutex<Connection>>,
}

impl SwapDb {
    /// Acquire the connection guard. If a previous holder panicked the
    /// mutex is poisoned — we still recover the inner state because
    /// SQLite is independently thread-safe and our SQL operations are
    /// stateless across calls. Without this, every subsequent DB access
    /// in the daemon would panic on `lock().unwrap()`, taking the
    /// process down. (Audit finding F5.)
    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|poisoned| {
            eprintln!(
                "warning: SwapDb mutex was poisoned by a panicking holder; \
                 recovering inner connection state"
            );
            poisoned.into_inner()
        })
    }

    // ── Constructor ───────────────────────────────────────────────────────

    /// Open (or create) a SQLite database at `path` and run migrations.
    pub fn open(path: &str) -> SwapResult<Self> {
        // Ensure parent directory exists
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| SwapError::Internal(format!("Cannot create DB dir: {e}")))?;
            }
        }
        let conn = Connection::open(path)?;
        // Enable WAL mode for concurrent reads
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    /// In-memory database (for tests).
    pub fn in_memory() -> SwapResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    // ── Migration ─────────────────────────────────────────────────────────

    fn migrate(&self) -> SwapResult<()> {
        let conn = self.conn();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS htlc_locks (
                hash_hex           TEXT    PRIMARY KEY,
                locker_address     TEXT    NOT NULL,
                amount_flowers      INTEGER NOT NULL,
                lock_tx_id         TEXT    NOT NULL,
                lock_block_height  INTEGER NOT NULL,
                expires_at         INTEGER NOT NULL,
                counterparty_chain TEXT    NOT NULL,
                counterparty_addr  TEXT    NOT NULL,
                state              TEXT    NOT NULL DEFAULT 'pending',
                release_tx_id      TEXT,
                release_recipient  TEXT,
                preimage_hex       TEXT,
                created_at         TEXT    NOT NULL,
                updated_at         TEXT    NOT NULL
            );

            CREATE TABLE IF NOT EXISTS watcher_state (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            "#,
        )?;
        // Additive migration: claimant_address column (L2 security patch).
        // Older DBs created without the column get it added; new DBs already
        // have it via the CREATE above only if we add it there too. We use
        // the additive ALTER for both paths to keep one source of truth.
        let has_col: bool = conn
            .prepare("PRAGMA table_info(htlc_locks)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .any(|name| name == "claimant_address");
        if !has_col {
            conn.execute_batch("ALTER TABLE htlc_locks ADD COLUMN claimant_address TEXT;")?;
        }
        Ok(())
    }

    // ── HTLC CRUD ─────────────────────────────────────────────────────────

    /// Insert a new HTLC record.  Returns `Err` if `hash_hex` already exists.
    pub fn insert_htlc(&self, rec: &HtlcRecord) -> SwapResult<()> {
        let conn = self.conn();
        conn.execute(
            r#"INSERT INTO htlc_locks
               (hash_hex, locker_address, amount_flowers, lock_tx_id, lock_block_height,
                expires_at, counterparty_chain, counterparty_addr, claimant_address, state,
                release_tx_id, release_recipient, preimage_hex, created_at, updated_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)"#,
            params![
                rec.hash_hex,
                rec.locker_address,
                rec.amount_flowers as i64,
                rec.lock_tx_id,
                rec.lock_block_height as i64,
                rec.expires_at,
                rec.counterparty_chain,
                rec.counterparty_addr,
                rec.claimant_address,
                state_to_str(&rec.state),
                rec.release_tx_id,
                rec.release_recipient,
                rec.preimage_hex,
                rec.created_at.to_rfc3339(),
                rec.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Fetch a single HTLC by hash.
    pub fn get_htlc(&self, hash_hex: &str) -> SwapResult<Option<HtlcRecord>> {
        let conn = self.conn();
        let res = conn
            .query_row(
                r#"SELECT hash_hex, locker_address, amount_flowers, lock_tx_id,
                          lock_block_height, expires_at, counterparty_chain,
                          counterparty_addr, claimant_address, state, release_tx_id,
                          release_recipient, preimage_hex, created_at, updated_at
                   FROM htlc_locks WHERE hash_hex = ?1"#,
                params![hash_hex],
                row_to_record,
            )
            .optional()?;
        Ok(res)
    }

    /// Mark an HTLC as claimed.
    pub fn mark_claimed(
        &self,
        hash_hex: &str,
        release_tx_id: &str,
        release_recipient: &str,
        preimage_hex: &str,
    ) -> SwapResult<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            r#"UPDATE htlc_locks
               SET state = 'claimed', release_tx_id = ?1, release_recipient = ?2,
                   preimage_hex = ?3, updated_at = ?4
               WHERE hash_hex = ?5"#,
            params![
                release_tx_id,
                release_recipient,
                preimage_hex,
                now,
                hash_hex
            ],
        )?;
        Ok(())
    }

    /// Mark an HTLC as refunded.
    pub fn mark_refunded(&self, hash_hex: &str, release_tx_id: &str) -> SwapResult<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            r#"UPDATE htlc_locks
               SET state = 'refunded', release_tx_id = ?1, updated_at = ?2
               WHERE hash_hex = ?3"#,
            params![release_tx_id, now, hash_hex],
        )?;
        Ok(())
    }

    /// Mark an HTLC with an internal error string.
    pub fn mark_error(&self, hash_hex: &str, msg: &str) -> SwapResult<()> {
        let now = Utc::now().to_rfc3339();
        let state_str = format!("error:{msg}");
        let conn = self.conn();
        conn.execute(
            "UPDATE htlc_locks SET state = ?1, updated_at = ?2 WHERE hash_hex = ?3",
            params![state_str, now, hash_hex],
        )?;
        Ok(())
    }

    /// Return all pending HTLCs whose timelock has expired.
    pub fn get_expired_pending(&self) -> SwapResult<Vec<HtlcRecord>> {
        let now = Utc::now().timestamp();
        let conn = self.conn();
        let mut stmt = conn.prepare(
            r#"SELECT hash_hex, locker_address, amount_flowers, lock_tx_id,
                      lock_block_height, expires_at, counterparty_chain,
                      counterparty_addr, claimant_address, state, release_tx_id,
                      release_recipient, preimage_hex, created_at, updated_at
               FROM htlc_locks
               WHERE state = 'pending' AND expires_at <= ?1"#,
        )?;
        let rows = stmt.query_map(params![now], row_to_record)?;
        let mut result = Vec::new();
        for r in rows {
            result.push(r?);
        }
        Ok(result)
    }

    /// Return all pending HTLCs (for dashboard / admin).
    pub fn list_pending(&self, limit: i64) -> SwapResult<Vec<HtlcRecord>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            r#"SELECT hash_hex, locker_address, amount_flowers, lock_tx_id,
                      lock_block_height, expires_at, counterparty_chain,
                      counterparty_addr, claimant_address, state, release_tx_id,
                      release_recipient, preimage_hex, created_at, updated_at
               FROM htlc_locks WHERE state = 'pending'
               ORDER BY created_at DESC LIMIT ?1"#,
        )?;
        let rows = stmt.query_map(params![limit], row_to_record)?;
        let mut result = Vec::new();
        for r in rows {
            result.push(r?);
        }
        Ok(result)
    }

    // ── Watcher state ─────────────────────────────────────────────────────

    /// Read the last L1 block height scanned by the watcher.
    pub fn get_scan_height(&self) -> SwapResult<u64> {
        let conn = self.conn();
        let val: Option<String> = conn
            .query_row(
                "SELECT value FROM watcher_state WHERE key = 'scan_height'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(val.and_then(|s| s.parse().ok()).unwrap_or(0))
    }

    /// Persist the last scanned block height.
    pub fn set_scan_height(&self, height: u64) -> SwapResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR REPLACE INTO watcher_state (key, value) VALUES ('scan_height', ?1)",
            params![height.to_string()],
        )?;
        Ok(())
    }

    // ── EVM watcher helpers ───────────────────────────────────────────────

    /// Return a cloned connection handle for the EVM watcher task and
    /// ensure the EVM watcher tables exist.
    pub fn conn_for_evm_watcher(&self) -> std::sync::Arc<std::sync::Mutex<rusqlite::Connection>> {
        {
            let conn = self.conn();
            crate::evm_watcher::migrate(&conn).expect("EVM watcher migration failed");
        }
        Arc::clone(&self.conn)
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn state_to_str(s: &SwapState) -> String {
    match s {
        SwapState::Pending => "pending".into(),
        SwapState::Claimed => "claimed".into(),
        SwapState::Refunded => "refunded".into(),
        SwapState::Error(msg) => format!("error:{msg}"),
    }
}

fn str_to_state(s: &str) -> SwapState {
    match s {
        "pending" => SwapState::Pending,
        "claimed" => SwapState::Claimed,
        "refunded" => SwapState::Refunded,
        other => {
            let msg = other.strip_prefix("error:").unwrap_or(other).to_string();
            SwapState::Error(msg)
        }
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<HtlcRecord> {
    let state_str: String = row.get(9)?;
    let created_at: String = row.get(13)?;
    let updated_at: String = row.get(14)?;
    Ok(HtlcRecord {
        hash_hex: row.get(0)?,
        locker_address: row.get(1)?,
        amount_flowers: row.get::<_, i64>(2)? as u64,
        lock_tx_id: row.get(3)?,
        lock_block_height: row.get::<_, i64>(4)? as u64,
        expires_at: row.get(5)?,
        counterparty_chain: row.get(6)?,
        counterparty_addr: row.get(7)?,
        claimant_address: row.get(8)?,
        state: str_to_state(&state_str),
        release_tx_id: row.get(10)?,
        release_recipient: row.get(11)?,
        preimage_hex: row.get(12)?,
        created_at: created_at
            .parse()
            .unwrap_or_else(|_| chrono::DateTime::default()),
        updated_at: updated_at
            .parse()
            .unwrap_or_else(|_| chrono::DateTime::default()),
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SwapState;
    use chrono::Utc;

    fn make_record(hash: &str) -> HtlcRecord {
        let now = Utc::now();
        HtlcRecord {
            hash_hex: hash.to_string(),
            locker_address: "zion1alice".into(),
            amount_flowers: 5_000_000,
            lock_tx_id: "txabc123".into(),
            lock_block_height: 100,
            expires_at: now.timestamp() + 7200,
            counterparty_chain: "btc".into(),
            counterparty_addr: "bc1qtest".into(),
            claimant_address: None,
            state: SwapState::Pending,
            release_tx_id: None,
            release_recipient: None,
            preimage_hex: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn insert_and_get() {
        let db = SwapDb::in_memory().unwrap();
        let rec = make_record(&"a".repeat(64));
        db.insert_htlc(&rec).unwrap();
        let fetched = db.get_htlc(&"a".repeat(64)).unwrap().unwrap();
        assert_eq!(fetched.locker_address, "zion1alice");
        assert_eq!(fetched.amount_flowers, 5_000_000);
    }

    #[test]
    fn mark_claimed() {
        let db = SwapDb::in_memory().unwrap();
        let hash = "b".repeat(64);
        db.insert_htlc(&make_record(&hash)).unwrap();
        db.mark_claimed(&hash, "tx_release", "zion1bob", &"c".repeat(64))
            .unwrap();
        let rec = db.get_htlc(&hash).unwrap().unwrap();
        assert_eq!(rec.state, SwapState::Claimed);
        assert_eq!(rec.release_tx_id.as_deref(), Some("tx_release"));
    }

    #[test]
    fn watcher_height_persistence() {
        let db = SwapDb::in_memory().unwrap();
        assert_eq!(db.get_scan_height().unwrap(), 0);
        db.set_scan_height(42).unwrap();
        assert_eq!(db.get_scan_height().unwrap(), 42);
    }
}
