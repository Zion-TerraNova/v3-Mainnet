//! DAO SQLite Persistence Layer
//!
//! Stores all DAO state to disk — proposals, votes, treasury operations, scanner
//! cursor. Designed for single-writer, multi-reader access (Tokio + rusqlite).
//!
//! ## Schema
//!
//! ```text
//! proposals  — canonical proposal state (JSON blob for flexible ProposalType)
//! votes      — one row per (proposal_id, voter_address), deduplicated
//! treasury   — pending + executed treasury operations
//! scan_state — last scanned L1 block height (singleton row)
//! ```

use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json;
use std::path::Path;

use crate::error::{DaoError, DaoResult};
use crate::proposal::Proposal;
use crate::treasury::TreasuryOperation;
use crate::types::VoteChoice;

// ─────────────────────────────────────────────────────────────────────────────

pub struct DaoDb {
    conn: Connection,
}

impl DaoDb {
    /// Open (or create) the SQLite database at `path`.
    pub fn open<P: AsRef<Path>>(path: P) -> DaoResult<Self> {
        let conn = Connection::open(path).map_err(|e| DaoError::Internal(e.to_string()))?;

        // WAL mode — better concurrent read performance
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| DaoError::Internal(e.to_string()))?;

        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// In-memory database for tests.
    pub fn in_memory() -> DaoResult<Self> {
        let conn = Connection::open_in_memory().map_err(|e| DaoError::Internal(e.to_string()))?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    // ── Schema ────────────────────────────────────────────────────────────────

    fn init_schema(&self) -> DaoResult<()> {
        self.conn
            .execute_batch(
                r#"
            CREATE TABLE IF NOT EXISTS proposals (
                id             INTEGER PRIMARY KEY,
                uuid           TEXT    NOT NULL UNIQUE,
                status         TEXT    NOT NULL DEFAULT 'Draft',
                proposal_type  TEXT    NOT NULL,  -- JSON
                title          TEXT    NOT NULL,
                description    TEXT    NOT NULL,
                proposer       TEXT    NOT NULL,
                votes_yes      INTEGER NOT NULL DEFAULT 0,
                votes_no       INTEGER NOT NULL DEFAULT 0,
                votes_abstain  INTEGER NOT NULL DEFAULT 0,
                created_at     TEXT    NOT NULL,
                voting_ends_at TEXT    NOT NULL,
                executed_at    TEXT
            );

            CREATE TABLE IF NOT EXISTS votes (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                proposal_id INTEGER NOT NULL REFERENCES proposals(id),
                voter       TEXT    NOT NULL,
                choice      TEXT    NOT NULL,   -- 'yes' | 'no' | 'abstain'
                weight      INTEGER NOT NULL,   -- ZION balance at snapshot
                l1_tx_hash  TEXT,               -- TX hash on L1 (optional, from memo)
                voted_at    TEXT    NOT NULL,
                UNIQUE(proposal_id, voter)
            );

            CREATE TABLE IF NOT EXISTS treasury_ops (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                op_id         TEXT NOT NULL UNIQUE,
                proposal_id   INTEGER     REFERENCES proposals(id),
                operation     TEXT NOT NULL,  -- JSON (TreasuryOperation variant)
                submitted_by  TEXT NOT NULL,
                status        TEXT NOT NULL DEFAULT 'pending',  -- pending|signed|executed|rejected
                created_at    TEXT NOT NULL,
                executed_at   TEXT
            );

            CREATE TABLE IF NOT EXISTS scan_state (
                id            INTEGER PRIMARY KEY CHECK(id = 1),  -- singleton
                last_block    INTEGER NOT NULL DEFAULT 0,
                updated_at    TEXT    NOT NULL
            );

            INSERT OR IGNORE INTO scan_state(id, last_block, updated_at)
            VALUES (1, 0, datetime('now'));
            "#,
            )
            .map_err(|e| DaoError::Internal(e.to_string()))?;
        Ok(())
    }

    // ── Proposals ─────────────────────────────────────────────────────────────

    /// Insert a new proposal. Returns the auto-incremented row id.
    pub fn insert_proposal(&self, p: &Proposal) -> DaoResult<i64> {
        let type_json = serde_json::to_string(&p.proposal_type)
            .map_err(|e| DaoError::Internal(e.to_string()))?;
        let status = format!("{:?}", p.status);

        self.conn
            .execute(
                r#"INSERT INTO proposals
                    (id, uuid, status, proposal_type, title, description, proposer,
                     votes_yes, votes_no, votes_abstain, created_at, voting_ends_at)
                   VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"#,
                params![
                    p.id,
                    p.id.to_string(), // use numeric id as uuid for now
                    status,
                    type_json,
                    p.title,
                    p.description,
                    p.proposer,
                    p.votes_for,
                    p.votes_against,
                    p.votes_abstain,
                    p.created_at.to_rfc3339(),
                    p.voting_ends_at.to_rfc3339(),
                ],
            )
            .map_err(|e| DaoError::Internal(e.to_string()))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Update proposal status + vote counts from a live `Proposal` object.
    pub fn update_proposal_status(&self, p: &Proposal) -> DaoResult<()> {
        let status = format!("{:?}", p.status);
        let executed_at = p.executed_at.as_ref().map(|dt| dt.to_rfc3339());

        self.conn
            .execute(
                r#"UPDATE proposals SET status=?1, votes_yes=?2, votes_no=?3,
                       votes_abstain=?4, executed_at=?5 WHERE id=?6"#,
                params![
                    status,
                    p.votes_for,
                    p.votes_against,
                    p.votes_abstain,
                    executed_at,
                    p.id,
                ],
            )
            .map_err(|e| DaoError::Internal(e.to_string()))?;
        Ok(())
    }

    /// Update proposal status from a `ProposalRow` (used after loading from DB and modifying status).
    pub fn update_proposal_row(&self, row: &ProposalRow) -> DaoResult<()> {
        self.conn
            .execute(
                r#"UPDATE proposals SET status=?1, votes_yes=?2, votes_no=?3,
                       votes_abstain=?4, executed_at=?5 WHERE id=?6"#,
                params![
                    row.status,
                    row.votes_yes,
                    row.votes_no,
                    row.votes_abstain,
                    row.executed_at,
                    row.id,
                ],
            )
            .map_err(|e| DaoError::Internal(e.to_string()))?;
        Ok(())
    }

    /// Load all proposals (for in-memory reload at startup).
    pub fn load_all_proposals(&self) -> DaoResult<Vec<ProposalRow>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"SELECT id, status, proposal_type, title, description, proposer,
                          votes_yes, votes_no, votes_abstain, created_at, voting_ends_at, executed_at
                   FROM proposals ORDER BY id"#,
            )
            .map_err(|e| DaoError::Internal(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(ProposalRow {
                    id: row.get(0)?,
                    status: row.get(1)?,
                    proposal_type_json: row.get(2)?,
                    title: row.get(3)?,
                    description: row.get(4)?,
                    proposer: row.get(5)?,
                    votes_yes: row.get(6)?,
                    votes_no: row.get(7)?,
                    votes_abstain: row.get(8)?,
                    created_at: row.get(9)?,
                    voting_ends_at: row.get(10)?,
                    executed_at: row.get(11)?,
                })
            })
            .map_err(|e| DaoError::Internal(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| DaoError::Internal(e.to_string()))?;

        Ok(rows)
    }

    /// Get a single proposal by id (returns None if not found).
    pub fn get_proposal(&self, id: u64) -> DaoResult<Option<ProposalRow>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"SELECT id, status, proposal_type, title, description, proposer,
                          votes_yes, votes_no, votes_abstain, created_at, voting_ends_at, executed_at
                   FROM proposals WHERE id=?1"#,
            )
            .map_err(|e| DaoError::Internal(e.to_string()))?;

        let mut rows = stmt
            .query_map(params![id], |row| {
                Ok(ProposalRow {
                    id: row.get(0)?,
                    status: row.get(1)?,
                    proposal_type_json: row.get(2)?,
                    title: row.get(3)?,
                    description: row.get(4)?,
                    proposer: row.get(5)?,
                    votes_yes: row.get(6)?,
                    votes_no: row.get(7)?,
                    votes_abstain: row.get(8)?,
                    created_at: row.get(9)?,
                    voting_ends_at: row.get(10)?,
                    executed_at: row.get(11)?,
                })
            })
            .map_err(|e| DaoError::Internal(e.to_string()))?;

        match rows.next() {
            Some(Ok(row)) => Ok(Some(row)),
            Some(Err(e)) => Err(DaoError::Internal(e.to_string())),
            None => Ok(None),
        }
    }

    // ── Votes ─────────────────────────────────────────────────────────────────

    /// Record a vote. Returns `false` if duplicate (same voter, same proposal).
    pub fn record_vote(
        &self,
        proposal_id: u64,
        voter: &str,
        choice: VoteChoice,
        weight: u64,
        l1_tx_hash: Option<&str>,
    ) -> DaoResult<bool> {
        let choice_str = match choice {
            VoteChoice::Yes => "yes",
            VoteChoice::No => "no",
            VoteChoice::Abstain => "abstain",
        };

        let tx = self.conn.unchecked_transaction()?;
        let result = tx.execute(
            r#"INSERT OR IGNORE INTO votes
               (proposal_id, voter, choice, weight, l1_tx_hash, voted_at)
               VALUES (?1,?2,?3,?4,?5, datetime('now'))"#,
            params![proposal_id, voter, choice_str, weight, l1_tx_hash],
        );

        let inserted = match result {
            Ok(0) => false, // OR IGNORE hit — duplicate
            Ok(_) => true,
            Err(e) => return Err(DaoError::Internal(e.to_string())),
        };

        if inserted {
            let col = match choice {
                VoteChoice::Yes => "votes_yes",
                VoteChoice::No => "votes_no",
                VoteChoice::Abstain => "votes_abstain",
            };
            tx.execute(
                &format!("UPDATE proposals SET {col} = {col} + ?1 WHERE id = ?2"),
                params![weight, proposal_id],
            )
            .map_err(|e| DaoError::Internal(e.to_string()))?;
        }

        tx.commit().map_err(|e| DaoError::Internal(e.to_string()))?;
        Ok(inserted)
    }

    /// Count votes for a proposal (by choice).
    pub fn vote_totals(&self, proposal_id: u64) -> DaoResult<(u64, u64, u64)> {
        let mut stmt = self
            .conn
            .prepare(
                r#"SELECT
                    COALESCE(SUM(CASE WHEN choice='yes'     THEN weight ELSE 0 END), 0) AS yes_w,
                    COALESCE(SUM(CASE WHEN choice='no'      THEN weight ELSE 0 END), 0) AS no_w,
                    COALESCE(SUM(CASE WHEN choice='abstain' THEN weight ELSE 0 END), 0) AS abs_w
                   FROM votes WHERE proposal_id=?1"#,
            )
            .map_err(|e| DaoError::Internal(e.to_string()))?;

        let (yes, no, abstain): (i64, i64, i64) = stmt
            .query_row(params![proposal_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|e| DaoError::Internal(e.to_string()))?;

        Ok((yes as u64, no as u64, abstain as u64))
    }

    /// Check if a voter already voted on a proposal.
    pub fn has_voted(&self, proposal_id: u64, voter: &str) -> DaoResult<bool> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM votes WHERE proposal_id=?1 AND voter=?2",
                params![proposal_id, voter],
                |row| row.get(0),
            )
            .map_err(|e| DaoError::Internal(e.to_string()))?;
        Ok(count > 0)
    }

    // ── Treasury ──────────────────────────────────────────────────────────────

    pub fn insert_treasury_op(
        &self,
        op_id: &str,
        proposal_id: Option<u64>,
        operation: &TreasuryOperation,
        submitted_by: &str,
    ) -> DaoResult<()> {
        let op_json =
            serde_json::to_string(operation).map_err(|e| DaoError::Internal(e.to_string()))?;

        self.conn
            .execute(
                r#"INSERT OR IGNORE INTO treasury_ops
                   (op_id, proposal_id, operation, submitted_by, status, created_at)
                   VALUES (?1,?2,?3,?4,'pending',datetime('now'))"#,
                params![op_id, proposal_id, op_json, submitted_by],
            )
            .map_err(|e| DaoError::Internal(e.to_string()))?;
        Ok(())
    }

    pub fn update_treasury_op_status(&self, op_id: &str, status: &str) -> DaoResult<()> {
        let executed_at = if status == "executed" {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };
        self.conn
            .execute(
                "UPDATE treasury_ops SET status=?1, executed_at=?2 WHERE op_id=?3",
                params![status, executed_at, op_id],
            )
            .map_err(|e| DaoError::Internal(e.to_string()))?;
        Ok(())
    }

    // ── L1 Scan State ─────────────────────────────────────────────────────────

    /// Return last scanned L1 block height.
    pub fn last_scanned_block(&self) -> DaoResult<u64> {
        let h: i64 = self
            .conn
            .query_row("SELECT last_block FROM scan_state WHERE id=1", [], |row| {
                row.get(0)
            })
            .map_err(|e| DaoError::Internal(e.to_string()))?;
        Ok(h as u64)
    }

    /// Update the last scanned block cursor.
    pub fn set_last_scanned_block(&self, height: u64) -> DaoResult<()> {
        self.conn
            .execute(
                "UPDATE scan_state SET last_block=?1, updated_at=datetime('now') WHERE id=1",
                params![height as i64],
            )
            .map_err(|e| DaoError::Internal(e.to_string()))?;
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Row types (plain struct for easy JSON serialisation in API)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProposalRow {
    pub id: u64,
    pub status: String,
    pub proposal_type_json: String,
    pub title: String,
    pub description: String,
    pub proposer: String,
    pub votes_yes: i64,
    pub votes_no: i64,
    pub votes_abstain: i64,
    pub created_at: String,
    pub voting_ends_at: String,
    pub executed_at: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::VoteChoice;

    fn make_db() -> DaoDb {
        DaoDb::in_memory().unwrap()
    }

    #[test]
    fn test_schema_init() {
        let db = make_db();
        let h = db.last_scanned_block().unwrap();
        assert_eq!(h, 0);
    }

    #[test]
    fn test_scan_cursor() {
        let db = make_db();
        db.set_last_scanned_block(12345).unwrap();
        assert_eq!(db.last_scanned_block().unwrap(), 12345);
        db.set_last_scanned_block(99999).unwrap();
        assert_eq!(db.last_scanned_block().unwrap(), 99999);
    }

    #[test]
    fn test_vote_deduplication() {
        let db = make_db();
        // We need a proposal row first (minimal insert)
        db.conn.execute(
            r#"INSERT INTO proposals (id,uuid,status,proposal_type,title,description,
               proposer,votes_yes,votes_no,votes_abstain,created_at,voting_ends_at)
               VALUES (1,'1','Active','{}','Test','desc','zion1test',0,0,0,datetime('now'),datetime('now'))"#,
            [],
        ).unwrap();

        let first = db
            .record_vote(1, "zion1voter1", VoteChoice::Yes, 1_000_000, None)
            .unwrap();
        assert!(first, "First vote should succeed");

        let dup = db
            .record_vote(1, "zion1voter1", VoteChoice::No, 1_000_000, None)
            .unwrap();
        assert!(!dup, "Duplicate vote should be ignored");
    }

    #[test]
    fn test_vote_totals() {
        let db = make_db();
        db.conn.execute(
            r#"INSERT INTO proposals (id,uuid,status,proposal_type,title,description,
               proposer,votes_yes,votes_no,votes_abstain,created_at,voting_ends_at)
               VALUES (2,'2','Active','{}','Test2','d','zion1test',0,0,0,datetime('now'),datetime('now'))"#,
            [],
        ).unwrap();

        db.record_vote(2, "zion1a", VoteChoice::Yes, 5_000_000, None)
            .unwrap();
        db.record_vote(2, "zion1b", VoteChoice::No, 2_000_000, None)
            .unwrap();
        db.record_vote(2, "zion1c", VoteChoice::Abstain, 1_000_000, None)
            .unwrap();

        let (yes, no, abs) = db.vote_totals(2).unwrap();
        assert_eq!(yes, 5_000_000);
        assert_eq!(no, 2_000_000);
        assert_eq!(abs, 1_000_000);
    }

    #[test]
    fn test_has_voted() {
        let db = make_db();
        db.conn.execute(
            r#"INSERT INTO proposals (id,uuid,status,proposal_type,title,description,
               proposer,votes_yes,votes_no,votes_abstain,created_at,voting_ends_at)
               VALUES (3,'3','Active','{}','Test3','d','zion1test',0,0,0,datetime('now'),datetime('now'))"#,
            [],
        ).unwrap();

        assert!(!db.has_voted(3, "zion1voter").unwrap());
        db.record_vote(3, "zion1voter", VoteChoice::Yes, 100, None)
            .unwrap();
        assert!(db.has_voted(3, "zion1voter").unwrap());
    }
}
