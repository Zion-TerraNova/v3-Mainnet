//! OASIS Database — SQLite persistence for players, guilds, leaderboard.
//!
//! Players and guilds are stored as JSON blobs for schema flexibility.
//! Leaderboard and stats are stored in dedicated indexed columns.

use crate::consciousness::ConsciousnessLevel;
use crate::error::{OasisError, OasisResult};
use crate::guild::Guild;
use crate::player::Player;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Shared database handle (thread-safe).
#[derive(Clone)]
pub struct OasisDb {
    conn: Arc<Mutex<Connection>>,
}

impl OasisDb {
    /// Open (or create) the SQLite database at `path`.
    ///
    /// Creates tables if they don't exist.
    pub fn open<P: AsRef<Path>>(path: P) -> OasisResult<Self> {
        let conn = if path.as_ref() == Path::new(":memory:") {
            Connection::open_in_memory()
        } else {
            Connection::open(path)
        }
        .map_err(OasisError::Database)?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.create_tables()?;
        Ok(db)
    }

    /// Create an in-memory database (for tests).
    pub fn in_memory() -> OasisResult<Self> {
        Self::open(":memory:")
    }

    fn create_tables(&self) -> OasisResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS players (
                address     TEXT PRIMARY KEY,
                total_xp    INTEGER NOT NULL DEFAULT 0,
                level       INTEGER NOT NULL DEFAULT 1,
                guild_id    TEXT,
                data        TEXT NOT NULL,
                updated_at  INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_players_xp ON players(total_xp DESC);

            CREATE TABLE IF NOT EXISTS guilds (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                guild_xp    INTEGER NOT NULL DEFAULT 0,
                guild_level INTEGER NOT NULL DEFAULT 1,
                member_count INTEGER NOT NULL DEFAULT 1,
                data        TEXT NOT NULL,
                updated_at  INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_guilds_xp ON guilds(guild_xp DESC);

            CREATE TABLE IF NOT EXISTS oasis_state (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS quest_progress (
                player_address TEXT NOT NULL,
                quest_id       TEXT NOT NULL,
                completed      INTEGER NOT NULL DEFAULT 0,
                completed_at   INTEGER,
                PRIMARY KEY (player_address, quest_id)
            );

            CREATE TABLE IF NOT EXISTS raid_teams (
                id          TEXT PRIMARY KEY,
                leader      TEXT NOT NULL,
                name        TEXT NOT NULL,
                members     TEXT NOT NULL DEFAULT '[]',
                data        TEXT NOT NULL,
                created_at  INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS golden_egg_progress (
                player_address TEXT NOT NULL,
                clue_id        INTEGER NOT NULL,
                category       TEXT NOT NULL,
                discovered_at  INTEGER NOT NULL,
                PRIMARY KEY (player_address, clue_id)
            );

            CREATE INDEX IF NOT EXISTS idx_golden_egg_player ON golden_egg_progress(player_address);

            CREATE TABLE IF NOT EXISTS territory_state (
                territory_id   INTEGER PRIMARY KEY,
                controller      TEXT,
                guild_id        TEXT,
                contested       INTEGER NOT NULL DEFAULT 0,
                data            TEXT NOT NULL,
                updated_at      INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS tithe_records (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                player_address TEXT NOT NULL,
                amount_flowers INTEGER NOT NULL,
                timestamp    INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_tithe_player ON tithe_records(player_address);

            CREATE TABLE IF NOT EXISTS challenge_submissions (
                id           TEXT PRIMARY KEY,
                player_address TEXT NOT NULL,
                challenge_id  TEXT NOT NULL,
                score         INTEGER NOT NULL,
                data          TEXT NOT NULL,
                submitted_at  INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_challenge_player ON challenge_submissions(player_address);
            ",
        )
        .map_err(OasisError::Database)?;
        Ok(())
    }

    // ── Players ─────────────────────────────────────────────────────────────

    /// Save (insert or update) a player.
    pub fn save_player(&self, player: &Player) -> OasisResult<()> {
        let data =
            serde_json::to_string(player).map_err(|e| OasisError::Serialization(e.to_string()))?;
        let level = player.level as i64;
        let now = now_secs();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO players (address, total_xp, level, guild_id, data, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(address) DO UPDATE SET
               total_xp   = excluded.total_xp,
               level      = excluded.level,
               guild_id   = excluded.guild_id,
               data       = excluded.data,
               updated_at = excluded.updated_at",
            params![
                player.address,
                player.total_xp as i64,
                level,
                player.guild_id,
                data,
                now
            ],
        )
        .map_err(OasisError::Database)?;
        Ok(())
    }

    /// Get a player by wallet address.
    pub fn get_player(&self, address: &str) -> OasisResult<Option<Player>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT data FROM players WHERE address = ?1")
            .map_err(OasisError::Database)?;

        let mut rows = stmt.query(params![address]).map_err(OasisError::Database)?;

        if let Some(row) = rows.next().map_err(OasisError::Database)? {
            let data: String = row.get(0).map_err(OasisError::Database)?;
            let player: Player = serde_json::from_str(&data)
                .map_err(|e| OasisError::Serialization(e.to_string()))?;
            Ok(Some(player))
        } else {
            Ok(None)
        }
    }

    /// Get or create a player. If player doesn't exist, creates a new one and saves it.
    pub fn get_or_create_player(&self, address: &str) -> OasisResult<Player> {
        if let Some(player) = self.get_player(address)? {
            return Ok(player);
        }
        let player = Player::new(address.to_string());
        self.save_player(&player)?;
        Ok(player)
    }

    // ── Guilds ───────────────────────────────────────────────────────────────

    /// Save (insert or update) a guild.
    pub fn save_guild(&self, guild: &Guild) -> OasisResult<()> {
        let data =
            serde_json::to_string(guild).map_err(|e| OasisError::Serialization(e.to_string()))?;
        let now = now_secs();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO guilds (id, name, guild_xp, guild_level, member_count, data, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
               name         = excluded.name,
               guild_xp     = excluded.guild_xp,
               guild_level  = excluded.guild_level,
               member_count = excluded.member_count,
               data         = excluded.data,
               updated_at   = excluded.updated_at",
            params![
                guild.id,
                guild.name,
                guild.guild_xp as i64,
                guild.guild_level as i64,
                guild.members.len() as i64,
                data,
                now
            ],
        )
        .map_err(OasisError::Database)?;
        Ok(())
    }

    /// Get a guild by ID.
    pub fn get_guild(&self, id: &str) -> OasisResult<Option<Guild>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT data FROM guilds WHERE id = ?1")
            .map_err(OasisError::Database)?;

        let mut rows = stmt.query(params![id]).map_err(OasisError::Database)?;

        if let Some(row) = rows.next().map_err(OasisError::Database)? {
            let data: String = row.get(0).map_err(OasisError::Database)?;
            let guild: Guild = serde_json::from_str(&data)
                .map_err(|e| OasisError::Serialization(e.to_string()))?;
            Ok(Some(guild))
        } else {
            Ok(None)
        }
    }

    /// List all guilds, ordered by guild XP descending.
    pub fn list_guilds(&self, limit: u32) -> OasisResult<Vec<Guild>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT data FROM guilds ORDER BY guild_xp DESC LIMIT ?1")
            .map_err(OasisError::Database)?;

        let rows = stmt
            .query_map(params![limit as i64], |row| row.get::<_, String>(0))
            .map_err(OasisError::Database)?;

        let mut guilds = Vec::new();
        for row in rows {
            let data = row.map_err(OasisError::Database)?;
            let guild: Guild = serde_json::from_str(&data)
                .map_err(|e| OasisError::Serialization(e.to_string()))?;
            guilds.push(guild);
        }
        Ok(guilds)
    }

    // ── Leaderboard ──────────────────────────────────────────────────────────

    /// Get top N players by XP (leaderboard).
    pub fn top_players(&self, limit: u32) -> OasisResult<Vec<LeaderboardEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT address, total_xp, level FROM players
                 ORDER BY total_xp DESC LIMIT ?1",
            )
            .map_err(OasisError::Database)?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(OasisError::Database)?;

        let mut entries = Vec::new();
        for (rank, row) in rows.enumerate() {
            let (address, xp, level_int) = row.map_err(OasisError::Database)?;
            entries.push(LeaderboardEntry {
                rank: rank as u32 + 1,
                address,
                total_xp: xp as u64,
                level: level_from_int(level_int),
            });
        }
        Ok(entries)
    }

    /// Get a player's rank on the leaderboard.
    pub fn player_rank(&self, address: &str) -> OasisResult<Option<u64>> {
        let conn = self.conn.lock().unwrap();

        // Check existence and rank in a single lock scope to avoid deadlock
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM players WHERE address = ?1",
                params![address],
                |row| row.get(0),
            )
            .map_err(OasisError::Database)?;

        if exists == 0 {
            return Ok(None);
        }

        let rank: i64 = conn
            .query_row(
                "SELECT COUNT(*) + 1 FROM players
                 WHERE total_xp > (SELECT total_xp FROM players WHERE address = ?1)",
                params![address],
                |row| row.get(0),
            )
            .map_err(OasisError::Database)?;

        Ok(Some(rank as u64))
    }

    // ── Stats ────────────────────────────────────────────────────────────────

    /// Total registered players.
    pub fn player_count(&self) -> OasisResult<u64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM players", [], |row| row.get(0))
            .map_err(OasisError::Database)?;
        Ok(count as u64)
    }

    /// Total guilds.
    pub fn guild_count(&self) -> OasisResult<u64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM guilds", [], |row| row.get(0))
            .map_err(OasisError::Database)?;
        Ok(count as u64)
    }

    /// Reset `daily_xp` to 0 for all players. Called at UTC midnight.
    /// The player data is stored as JSON in the `data` column, so we use
    /// SQLite's `json_set()` to update the nested field.
    pub fn reset_all_daily_xp(&self) -> OasisResult<()> {
        let conn = self.conn.lock().unwrap();
        // json_set works on the JSON text in the data column
        conn.execute(
            "UPDATE players SET data = json_set(data, '$.daily_xp', 0)",
            [],
        )
        .map_err(OasisError::Database)?;
        Ok(())
    }

    // ── Quest Progress ─────────────────────────────────────────────────────

    /// Save (insert or update) quest progress.
    pub fn save_quest_progress(&self, progress: &crate::quests::QuestProgress) -> OasisResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO quest_progress (player_address, quest_id, completed, completed_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(player_address, quest_id) DO UPDATE SET
               completed    = excluded.completed,
               completed_at = excluded.completed_at",
            params![
                progress.player_address,
                progress.quest_id,
                progress.completed,
                progress.completed_at.map(|t| t as i64),
            ],
        )
        .map_err(OasisError::Database)?;
        Ok(())
    }

    /// Get a single quest progress entry.
    pub fn get_quest_progress(
        &self,
        address: &str,
        quest_id: &str,
    ) -> OasisResult<Option<crate::quests::QuestProgress>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT completed, completed_at FROM quest_progress WHERE player_address = ?1 AND quest_id = ?2")
            .map_err(OasisError::Database)?;
        let mut rows = stmt
            .query(params![address, quest_id])
            .map_err(OasisError::Database)?;
        if let Some(row) = rows.next().map_err(OasisError::Database)? {
            let completed: bool = row.get(0).map_err(OasisError::Database)?;
            let completed_at: Option<i64> = row.get(1).map_err(OasisError::Database)?;
            Ok(Some(crate::quests::QuestProgress {
                player_address: address.to_string(),
                quest_id: quest_id.to_string(),
                completed,
                completed_at: completed_at.map(|t| t as u64),
            }))
        } else {
            Ok(None)
        }
    }

    /// List all quest progress for a player.
    pub fn list_quest_progress(
        &self,
        address: &str,
    ) -> OasisResult<Vec<crate::quests::QuestProgress>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT quest_id, completed, completed_at FROM quest_progress WHERE player_address = ?1")
            .map_err(OasisError::Database)?;
        let rows = stmt
            .query_map(params![address], |row| {
                Ok(crate::quests::QuestProgress {
                    player_address: address.to_string(),
                    quest_id: row.get(0)?,
                    completed: row.get(1)?,
                    completed_at: row.get::<_, Option<i64>>(2)?.map(|t| t as u64),
                })
            })
            .map_err(OasisError::Database)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(OasisError::Database)?);
        }
        Ok(out)
    }

    /// Count completed quests for a player.
    pub fn count_completed_quests(&self, address: &str) -> OasisResult<u32> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM quest_progress WHERE player_address = ?1 AND completed = 1",
                params![address],
                |row| row.get(0),
            )
            .map_err(OasisError::Database)?;
        Ok(count as u32)
    }

    // ── Raid Teams ──────────────────────────────────────────────────────────

    pub fn save_raid_team(
        &self,
        id: &str,
        leader: &str,
        name: &str,
        members: &str,
        data: &str,
    ) -> OasisResult<()> {
        let now = now_secs();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO raid_teams (id, leader, name, members, data, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET members = excluded.members, data = excluded.data",
            params![id, leader, name, members, data, now],
        )
        .map_err(OasisError::Database)?;
        Ok(())
    }

    pub fn get_raid_team(&self, id: &str) -> OasisResult<Option<(String, String, String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT leader, name, members, data FROM raid_teams WHERE id = ?1")
            .map_err(OasisError::Database)?;
        let mut rows = stmt.query(params![id]).map_err(OasisError::Database)?;
        if let Some(row) = rows.next().map_err(OasisError::Database)? {
            return Ok(Some((
                row.get(0).map_err(OasisError::Database)?,
                row.get(1).map_err(OasisError::Database)?,
                row.get(2).map_err(OasisError::Database)?,
                row.get(3).map_err(OasisError::Database)?,
            )));
        }
        Ok(None)
    }

    // ── Golden Egg Progress ──────────────────────────────────────────────────

    pub fn save_clue_discovery(
        &self,
        address: &str,
        clue_id: u32,
        category: &str,
    ) -> OasisResult<()> {
        let now = now_secs();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO golden_egg_progress (player_address, clue_id, category, discovered_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![address, clue_id as i64, category, now],
        ).map_err(OasisError::Database)?;
        Ok(())
    }

    pub fn get_clue_count(&self, address: &str) -> OasisResult<u32> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM golden_egg_progress WHERE player_address = ?1",
                params![address],
                |row| row.get(0),
            )
            .map_err(OasisError::Database)?;
        Ok(count as u32)
    }

    // ── Territory State ──────────────────────────────────────────────────────

    pub fn save_territory(
        &self,
        territory_id: u32,
        controller: &str,
        guild_id: Option<&str>,
        data: &str,
    ) -> OasisResult<()> {
        let now = now_secs();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO territory_state (territory_id, controller, guild_id, contested, data, updated_at)
             VALUES (?1, ?2, ?3, 0, ?4, ?5)
             ON CONFLICT(territory_id) DO UPDATE SET controller = excluded.controller, guild_id = excluded.guild_id, data = excluded.data, updated_at = excluded.updated_at",
            params![territory_id as i64, controller, guild_id, data, now],
        ).map_err(OasisError::Database)?;
        Ok(())
    }

    // ── Tithe Records ────────────────────────────────────────────────────────

    pub fn save_tithe(&self, address: &str, amount_flowers: u64) -> OasisResult<()> {
        let now = now_secs();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tithe_records (player_address, amount_flowers, timestamp) VALUES (?1, ?2, ?3)",
            params![address, amount_flowers as i64, now],
        ).map_err(OasisError::Database)?;
        Ok(())
    }

    pub fn get_total_tithe(&self, address: &str) -> OasisResult<u64> {
        let conn = self.conn.lock().unwrap();
        let total: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(amount_flowers), 0) FROM tithe_records WHERE player_address = ?1",
                params![address],
                |row| row.get(0),
            )
            .map_err(OasisError::Database)?;
        Ok(total as u64)
    }

    // ── Challenge Submissions ────────────────────────────────────────────────

    pub fn save_challenge(
        &self,
        id: &str,
        address: &str,
        challenge_id: &str,
        score: u32,
        data: &str,
    ) -> OasisResult<()> {
        let now = now_secs();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO challenge_submissions (id, player_address, challenge_id, score, data, submitted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, address, challenge_id, score as i64, data, now],
        ).map_err(OasisError::Database)?;
        Ok(())
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn level_from_int(v: i64) -> ConsciousnessLevel {
    match v {
        2 => ConsciousnessLevel::Emotional,
        3 => ConsciousnessLevel::Mental,
        4 => ConsciousnessLevel::Intuitional,
        5 => ConsciousnessLevel::Spiritual,
        6 => ConsciousnessLevel::Cosmic,
        7 => ConsciousnessLevel::Divine,
        8 => ConsciousnessLevel::Unity,
        9 => ConsciousnessLevel::OnTheStar,
        _ => ConsciousnessLevel::Physical,
    }
}

/// A single leaderboard entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LeaderboardEntry {
    pub rank: u32,
    pub address: String,
    pub total_xp: u64,
    pub level: ConsciousnessLevel,
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guild::Guild;
    use crate::player::Player;
    use crate::xp::{XpSource, XpSystem};

    fn test_db() -> OasisDb {
        OasisDb::in_memory().expect("in-memory db")
    }

    #[test]
    fn test_save_and_get_player() {
        let db = test_db();
        let player = Player::new("zion1test000".to_string());
        db.save_player(&player).unwrap();
        let loaded = db.get_player("zion1test000").unwrap().unwrap();
        assert_eq!(loaded.address, "zion1test000");
        assert_eq!(loaded.total_xp, 0);
    }

    #[test]
    fn test_get_nonexistent_player_returns_none() {
        let db = test_db();
        let result = db.get_player("zion1doesnotexist").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_or_create_player() {
        let db = test_db();
        let p = db.get_or_create_player("zion1new").unwrap();
        assert_eq!(p.address, "zion1new");
        // Second call should return same player
        let p2 = db.get_or_create_player("zion1new").unwrap();
        assert_eq!(p2.address, p.address);
    }

    #[test]
    fn test_update_player_xp() {
        let db = test_db();
        let mut player = Player::new("zion1xp".to_string());
        db.save_player(&player).unwrap();

        let xp_sys = XpSystem::new();
        let source = XpSource::BlockMined {
            block_height: 100,
            shares: 10,
        };
        let award = xp_sys.award(player.total_xp, player.level, &source, player.daily_xp);
        player.total_xp = award.new_total_xp;
        player.level = award.new_level;
        db.save_player(&player).unwrap();

        let loaded = db.get_player("zion1xp").unwrap().unwrap();
        assert!(loaded.total_xp > 0);
    }

    #[test]
    fn test_save_and_get_guild() {
        let db = test_db();
        let guild = Guild::new(
            "guild-1".to_string(),
            "Zion Knights".to_string(),
            "zion1founder".to_string(),
        );
        db.save_guild(&guild).unwrap();
        let loaded = db.get_guild("guild-1").unwrap().unwrap();
        assert_eq!(loaded.name, "Zion Knights");
        assert_eq!(loaded.members.len(), 1);
    }

    #[test]
    fn test_list_guilds() {
        let db = test_db();
        for i in 0..3 {
            let mut g = Guild::new(
                format!("g{}", i),
                format!("Guild {}", i),
                "zion1x".to_string(),
            );
            g.guild_xp = (i as u64) * 1000;
            db.save_guild(&g).unwrap();
        }
        let guilds = db.list_guilds(10).unwrap();
        assert_eq!(guilds.len(), 3);
        // Should be ordered by guild_xp DESC
        assert!(guilds[0].guild_xp >= guilds[1].guild_xp);
    }

    #[test]
    fn test_leaderboard_top_players() {
        let db = test_db();
        for i in 0..5 {
            let mut p = Player::new(format!("zion1player{}", i));
            p.total_xp = i * 1000;
            p.level = ConsciousnessLevel::from_xp(p.total_xp);
            db.save_player(&p).unwrap();
        }
        let top = db.top_players(3).unwrap();
        assert_eq!(top.len(), 3);
        assert_eq!(top[0].rank, 1);
        assert!(top[0].total_xp >= top[1].total_xp);
    }

    #[test]
    fn test_player_count() {
        let db = test_db();
        assert_eq!(db.player_count().unwrap(), 0);
        db.save_player(&Player::new("zion1a".to_string())).unwrap();
        db.save_player(&Player::new("zion1b".to_string())).unwrap();
        assert_eq!(db.player_count().unwrap(), 2);
    }

    #[test]
    fn test_player_rank() {
        let db = test_db();
        let mut p1 = Player::new("top".to_string());
        p1.total_xp = 5000;
        let p2 = Player::new("bottom".to_string());
        db.save_player(&p1).unwrap();
        db.save_player(&p2).unwrap();
        assert_eq!(db.player_rank("top").unwrap(), Some(1));
        assert_eq!(db.player_rank("bottom").unwrap(), Some(2));
        assert_eq!(db.player_rank("nonexistent").unwrap(), None);
    }
}
