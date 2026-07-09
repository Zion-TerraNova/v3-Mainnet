//! RevenueJournal — append-only, crash-safe audit log for revenue events.
//!
//! Format: JSON Lines (`.jsonl`), one JSON object per line.
//! Rotation: daily files `revenue_YYYY-MM-DD.jsonl`, retention configurable.
//! Replay: on startup, reads all `.jsonl` files to reconstruct state.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// A single persisted revenue entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub ts: String, // RFC3339
    #[serde(flatten)]
    pub payload: JournalPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JournalPayload {
    ZionBlock {
        height: u64,
        subsidy: u64,
        pool_fee: u64,
        humanitarian: u64,
        issobella: u64,
        miner: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        tx_hash: Option<String>,
    },
    Event {
        source: String,
        value_usd: f64,
        qualifies: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        block_height: Option<u64>,
    },
    Payout {
        amount_usd: f64,
    },
    PayoutZion {
        amount: u64,
    },
}

#[derive(Debug)]
pub struct RevenueJournal {
    dir: PathBuf,
    retention_days: u64,
    /// Serializes appends across threads and tracks which daily file is
    /// currently active so we only prune retention on day rollover.
    current_file: Arc<Mutex<Option<PathBuf>>>,
}

impl RevenueJournal {
    pub fn new(dir: impl AsRef<Path>, retention_days: u64) -> Self {
        let dir = dir.as_ref().to_path_buf();
        let _ = fs::create_dir_all(&dir);
        Self {
            dir,
            retention_days,
            current_file: Arc::new(Mutex::new(None)),
        }
    }

    pub fn from_env_or_default() -> Self {
        let dir = std::env::var("ZION_REVENUE_JOURNAL_DIR")
            .unwrap_or_else(|_| "./data/revenue_journal".to_string());
        let retention = std::env::var("ZION_REVENUE_JOURNAL_DAYS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(90);
        Self::new(dir, retention)
    }

    pub fn append(&self, payload: JournalPayload) -> std::io::Result<()> {
        let ts = Utc::now().to_rfc3339();
        let entry = JournalEntry { ts, payload };
        let line = serde_json::to_string(&entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let file_path = self.current_file_path();
        // Hold the lock across the open/write/sync so concurrent appends can
        // never interleave a half-written line in the JSONL stream.
        let mut guard = self.current_file.lock().unwrap();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;
        writeln!(file, "{}", line)?;
        file.sync_all()?;

        let rotated = guard.as_deref() != Some(file_path.as_path());
        *guard = Some(file_path);
        drop(guard);

        // Only attempt retention pruning on day rollover to keep the hot path
        // cheap (one filesystem scan per day, not per append).
        if rotated && self.retention_days > 0 {
            if let Err(e) = self.prune_expired() {
                eprintln!("revenue_journal_prune_error: {}", e);
            }
        }
        Ok(())
    }

    /// Delete `revenue_*.jsonl` files older than `retention_days`.
    pub fn prune_expired(&self) -> std::io::Result<()> {
        let cutoff = Utc::now().date_naive() - chrono::Duration::days(self.retention_days as i64);
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let s = name.to_string_lossy();
            if !(s.starts_with("revenue_") && s.ends_with(".jsonl")) {
                continue;
            }
            // Filename pattern: revenue_YYYY-MM-DD.jsonl
            let date_part = &s["revenue_".len()..s.len() - ".jsonl".len()];
            if let Ok(date) = NaiveDate::parse_from_str(date_part, "%Y-%m-%d") {
                if date < cutoff {
                    let path = entry.path();
                    if let Err(e) = fs::remove_file(&path) {
                        eprintln!(
                            "revenue_journal_remove_failed path={} error={}",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }
        Ok(())
    }

    pub fn replay_zion_blocks(&self) -> std::io::Result<Vec<ReplayedZionBlock>> {
        let mut blocks = Vec::new();
        let mut seen = HashSet::new();

        let entries = self.read_all_entries()?;
        for entry in entries {
            if let JournalPayload::ZionBlock {
                height,
                subsidy,
                pool_fee,
                humanitarian,
                issobella,
                miner,
                tx_hash,
            } = entry.payload
            {
                if seen.insert(height) {
                    blocks.push(ReplayedZionBlock {
                        height,
                        subsidy,
                        pool_fee,
                        humanitarian,
                        issobella,
                        miner,
                        tx_hash,
                        ts: entry.ts,
                    });
                }
            }
        }
        Ok(blocks)
    }

    pub fn replay_events(&self) -> std::io::Result<Vec<ReplayedEvent>> {
        let mut events = Vec::new();
        let entries = self.read_all_entries()?;
        for entry in entries {
            if let JournalPayload::Event {
                source,
                value_usd,
                qualifies,
                block_height,
            } = entry.payload
            {
                events.push(ReplayedEvent {
                    source,
                    value_usd,
                    qualifies,
                    block_height,
                    ts: entry.ts,
                });
            }
        }
        Ok(events)
    }

    fn current_file_path(&self) -> PathBuf {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        self.dir.join(format!("revenue_{}.jsonl", today))
    }

    fn read_all_entries(&self) -> std::io::Result<Vec<JournalEntry>> {
        let mut entries = Vec::new();
        let mut files: Vec<_> = fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name();
                let s = name.to_string_lossy();
                s.starts_with("revenue_") && s.ends_with(".jsonl")
            })
            .map(|e| e.path())
            .collect();
        files.sort();

        for path in files {
            let file = fs::File::open(&path)?;
            let reader = BufReader::new(file);
            for line in reader.lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<JournalEntry>(&line) {
                    Ok(entry) => entries.push(entry),
                    Err(e) => {
                        eprintln!(
                            "revenue_journal: skipping corrupt line in {}: {}",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }
        Ok(entries)
    }
}

#[derive(Debug, Clone)]
pub struct ReplayedZionBlock {
    pub height: u64,
    pub subsidy: u64,
    pub pool_fee: u64,
    pub humanitarian: u64,
    pub issobella: u64,
    pub miner: u64,
    pub tx_hash: Option<String>,
    pub ts: String,
}

#[derive(Debug, Clone)]
pub struct ReplayedEvent {
    pub source: String,
    pub value_usd: f64,
    pub qualifies: bool,
    pub block_height: Option<u64>,
    pub ts: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zion_revenue_journal_test_{}_{}",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn append_and_replay_roundtrip_preserves_timestamp() {
        let dir = tmpdir("roundtrip");
        let journal = RevenueJournal::new(&dir, 90);
        journal
            .append(JournalPayload::ZionBlock {
                height: 42,
                subsidy: 1_000_000,
                pool_fee: 10_000,
                humanitarian: 50_000,
                issobella: 50_000,
                miner: 890_000,
                tx_hash: Some("abc".to_string()),
            })
            .unwrap();

        let blocks = journal.replay_zion_blocks().unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].height, 42);
        assert!(!blocks[0].ts.is_empty(), "ts must be populated for replay");
        // ts must be a valid RFC3339 stamp
        assert!(blocks[0]
            .ts
            .parse::<chrono::DateTime<chrono::Utc>>()
            .is_ok());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_appends_do_not_corrupt_lines() {
        use std::sync::Arc;
        use std::thread;

        let dir = tmpdir("concurrent");
        let journal = Arc::new(RevenueJournal::new(&dir, 90));
        let mut handles = Vec::new();
        for t in 0..8u64 {
            let j = journal.clone();
            handles.push(thread::spawn(move || {
                for i in 0..50u64 {
                    j.append(JournalPayload::PayoutZion {
                        amount: t * 1000 + i,
                    })
                    .unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        // Every line must parse as valid JSON.
        let path = journal.current_file_path();
        let content = fs::read_to_string(&path).unwrap();
        let mut count = 0;
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            serde_json::from_str::<JournalEntry>(line)
                .expect("every line must be valid JSON under concurrent append");
            count += 1;
        }
        assert_eq!(count, 8 * 50);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_expired_removes_old_files_only() {
        let dir = tmpdir("prune");
        let journal = RevenueJournal::new(&dir, 7);

        // Create one fresh file and one well-expired file.
        let today = Utc::now().date_naive();
        let old = today - chrono::Duration::days(30);
        let fresh_path = dir.join(format!("revenue_{}.jsonl", today.format("%Y-%m-%d")));
        let old_path = dir.join(format!("revenue_{}.jsonl", old.format("%Y-%m-%d")));
        fs::write(&fresh_path, "{}").unwrap();
        fs::write(&old_path, "{}").unwrap();

        journal.prune_expired().unwrap();
        assert!(fresh_path.exists(), "fresh file must be preserved");
        assert!(!old_path.exists(), "old file must be removed");

        let _ = fs::remove_dir_all(&dir);
    }
}
