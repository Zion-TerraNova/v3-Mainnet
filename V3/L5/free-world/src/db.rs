//! SQLite persistence for zion-free-world.

use crate::error::FreeWorldResult;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub struct FreeWorldDb {
    conn: Connection,
}

impl FreeWorldDb {
    pub fn open(path: &str) -> FreeWorldResult<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> FreeWorldResult<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS grants (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                description TEXT,
                applicant_name TEXT,
                applicant_address TEXT,
                category TEXT NOT NULL,
                amount_zion INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL,
                reviewed_at TEXT,
                reviewer_notes TEXT
            );

            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                location TEXT,
                category TEXT NOT NULL,
                budget_zion INTEGER NOT NULL,
                spent_zion INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'planning',
                started_at TEXT,
                completed_at TEXT,
                impact_metrics TEXT
            );

            CREATE TABLE IF NOT EXISTS communities (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                location TEXT,
                population INTEGER,
                energy_source TEXT,
                zion_address TEXT,
                status TEXT NOT NULL DEFAULT 'forming',
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS fund_balance (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                total_accumulated INTEGER NOT NULL DEFAULT 0,
                total_disbursed INTEGER NOT NULL DEFAULT 0,
                last_block_height INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );

            INSERT OR IGNORE INTO fund_balance (id, total_accumulated, total_disbursed, last_block_height, updated_at)
            VALUES (1, 0, 0, 0, datetime('now'));"
        )?;
        Ok(())
    }

    // ── Grants ──

    pub fn insert_grant(&self, g: &GrantRecord) -> FreeWorldResult<()> {
        self.conn.execute(
            "INSERT INTO grants (id, title, description, applicant_name, applicant_address, category, amount_zion, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (&g.id, &g.title, &g.description, &g.applicant_name, &g.applicant_address,
             &g.category, &g.amount_zion, &g.status, &g.created_at.to_rfc3339()),
        )?;
        Ok(())
    }

    pub fn list_grants(&self, status: Option<&str>) -> FreeWorldResult<Vec<GrantRecord>> {
        let sql = match status {
            Some(_s) => "SELECT id, title, description, applicant_name, applicant_address, category, amount_zion, status, created_at, reviewed_at, reviewer_notes FROM grants WHERE status = ?1 ORDER BY created_at DESC",
            None => "SELECT id, title, description, applicant_name, applicant_address, category, amount_zion, status, created_at, reviewed_at, reviewer_notes FROM grants ORDER BY created_at DESC",
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = match status {
            Some(s) => stmt.query_map([s], row_to_grant)?,
            None => stmt.query_map([], row_to_grant)?,
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn update_grant_status(
        &self,
        id: &str,
        status: &str,
        notes: Option<&str>,
    ) -> FreeWorldResult<()> {
        let reviewed = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE grants SET status = ?1, reviewed_at = ?2, reviewer_notes = ?3 WHERE id = ?4",
            (status, &reviewed, notes.unwrap_or(""), id),
        )?;
        Ok(())
    }

    // ── Projects ──

    pub fn insert_project(&self, p: &ProjectRecord) -> FreeWorldResult<()> {
        self.conn.execute(
            "INSERT INTO projects (id, name, description, location, category, budget_zion, status, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (&p.id, &p.name, &p.description, &p.location, &p.category, &p.budget_zion, &p.status, &p.started_at.map(|t| t.to_rfc3339())),
        )?;
        Ok(())
    }

    pub fn list_projects(&self, status: Option<&str>) -> FreeWorldResult<Vec<ProjectRecord>> {
        let sql = match status {
            Some(_s) => "SELECT id, name, description, location, category, budget_zion, spent_zion, status, started_at, completed_at, impact_metrics FROM projects WHERE status = ?1 ORDER BY started_at DESC",
            None => "SELECT id, name, description, location, category, budget_zion, spent_zion, status, started_at, completed_at, impact_metrics FROM projects ORDER BY started_at DESC",
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = match status {
            Some(s) => stmt.query_map([s], row_to_project)?,
            None => stmt.query_map([], row_to_project)?,
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // ── Fund balance ──

    pub fn get_fund_balance(&self) -> FreeWorldResult<FundBalance> {
        let mut stmt = self.conn.prepare(
            "SELECT total_accumulated, total_disbursed, last_block_height, updated_at FROM fund_balance WHERE id = 1"
        )?;
        let row = stmt
            .query_row([], |row| {
                Ok(FundBalance {
                    total_accumulated: row.get(0)?,
                    total_disbursed: row.get(1)?,
                    last_block_height: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })
            .optional()?;
        Ok(row.unwrap_or_default())
    }

    pub fn update_fund_balance(&self, balance: &FundBalance) -> FreeWorldResult<()> {
        self.conn.execute(
            "UPDATE fund_balance SET total_accumulated = ?1, total_disbursed = ?2, last_block_height = ?3, updated_at = ?4 WHERE id = 1",
            (&balance.total_accumulated, &balance.total_disbursed, &balance.last_block_height, &balance.updated_at),
        )?;
        Ok(())
    }
}

fn row_to_grant(row: &rusqlite::Row) -> Result<GrantRecord, rusqlite::Error> {
    Ok(GrantRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        applicant_name: row.get(3)?,
        applicant_address: row.get(4)?,
        category: row.get(5)?,
        amount_zion: row.get(6)?,
        status: row.get(7)?,
        created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        reviewed_at: row
            .get::<_, Option<String>>(9)?
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc)),
        reviewer_notes: row.get(10)?,
    })
}

fn row_to_project(row: &rusqlite::Row) -> Result<ProjectRecord, rusqlite::Error> {
    Ok(ProjectRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        location: row.get(3)?,
        category: row.get(4)?,
        budget_zion: row.get(5)?,
        spent_zion: row.get(6)?,
        status: row.get(7)?,
        started_at: row
            .get::<_, Option<String>>(8)?
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc)),
        completed_at: row
            .get::<_, Option<String>>(9)?
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc)),
        impact_metrics: row.get(10)?,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantRecord {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub applicant_name: Option<String>,
    pub applicant_address: Option<String>,
    pub category: String, // humanitarian | energy | education | community
    pub amount_zion: u64,
    pub status: String, // pending | approved | rejected | disbursed
    pub created_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub reviewer_notes: Option<String>,
}

impl GrantRecord {
    pub fn new(title: &str, category: &str, amount: u64) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            title: title.to_string(),
            description: None,
            applicant_name: None,
            applicant_address: None,
            category: category.to_string(),
            amount_zion: amount,
            status: "pending".to_string(),
            created_at: Utc::now(),
            reviewed_at: None,
            reviewer_notes: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub category: String,
    pub budget_zion: u64,
    pub spent_zion: u64,
    pub status: String, // planning | active | completed | cancelled
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub impact_metrics: Option<String>,
}

impl ProjectRecord {
    pub fn new(name: &str, category: &str, budget: u64) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            description: None,
            location: None,
            category: category.to_string(),
            budget_zion: budget,
            spent_zion: 0,
            status: "planning".to_string(),
            started_at: None,
            completed_at: None,
            impact_metrics: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FundBalance {
    pub total_accumulated: u64,
    pub total_disbursed: u64,
    pub last_block_height: u64,
    pub updated_at: String,
}
