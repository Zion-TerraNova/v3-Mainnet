//! EVM watcher — polls Base (L2) EVM-side ZIONAtomicSwap contract
//!
//! Listens for `Locked`, `Claimed`, `Refunded` events on the EVM HTLC contract
//! and persists them into a local SQLite table `evm_htlc_locks`.
//!
//! The watcher is intentionally stateless between calls: it reads the last
//! processed block from the DB, issues an `eth_getLogs` JSON-RPC call for the
//! new range, and upserts every event it finds.
//!
//! # Configuration (section `[evm_watcher]` in config TOML)
//! ```toml
//! [evm_watcher]
//! enabled        = true
//! rpc_url        = "https://sepolia.base.org"
//! contract_addr  = "0x..."     # ZIONAtomicSwap address
//! poll_interval_secs = 12      # ~1 Base block
//! start_block    = 0           # scan from this block (0 = chain genesis)
//! ```
//!
//! # Event topics (keccak256 of signature)
//! Computed once at startup via `sha3::Keccak256`.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

// ─── Config ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmWatcherConfig {
    #[serde(default)]
    pub enabled: bool,
    /// HTTP(S) JSON-RPC endpoint (e.g. Base Sepolia)
    pub rpc_url: String,
    /// Backup/fallback RPC endpoint (e.g. Ankr: https://rpc.ankr.com/base)
    #[serde(default)]
    pub rpc_url_backup: Option<String>,
    /// Deployed ZIONAtomicSwap contract address (0x…)
    pub contract_addr: String,
    /// Polling interval in seconds
    #[serde(default = "default_poll")]
    pub poll_interval_secs: u64,
    /// Block to start scanning from (0 = genesis)
    #[serde(default)]
    pub start_block: u64,
}

fn default_poll() -> u64 {
    12
}

// ─── DB schema ───────────────────────────────────────────────────────────────

/// All columns we track for an EVM-side lock.
#[derive(Debug, Clone, Serialize)]
pub struct EvmLock {
    pub lock_id: String,   // bytes32 hex (0x…)
    pub initiator: String, // address
    pub recipient: String, // address (may be ZeroAddress)
    pub token: String,     // address  (ZeroAddress = native ETH)
    pub amount: String,    // uint256 as decimal string
    pub hashlock: String,  // bytes32 hex
    pub timelock: u64,     // unix timestamp
    pub counterparty_chain: String,
    pub counterparty_addr: String,
    pub state: String, // "pending" | "claimed" | "refunded"
    pub preimage: Option<String>,
    pub claimed_by: Option<String>,
    pub block_number: u64,
    pub tx_hash: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Apply the EVM watcher migration to an open Connection.
pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS evm_htlc_locks (
            lock_id            TEXT PRIMARY KEY,
            initiator          TEXT NOT NULL,
            recipient          TEXT NOT NULL,
            token              TEXT NOT NULL,
            amount             TEXT NOT NULL,
            hashlock           TEXT NOT NULL,
            timelock           INTEGER NOT NULL,
            counterparty_chain TEXT NOT NULL,
            counterparty_addr  TEXT NOT NULL,
            state              TEXT NOT NULL DEFAULT 'pending',
            preimage           TEXT,
            claimed_by         TEXT,
            block_number       INTEGER NOT NULL,
            tx_hash            TEXT NOT NULL,
            created_at         TEXT NOT NULL,
            updated_at         TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS evm_watcher_state (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    )?;
    Ok(())
}

// ─── Topic helpers ────────────────────────────────────────────────────────────

/// Acquire the connection guard, recovering from poisoning rather than
/// panicking. SQLite is independently thread-safe and our SQL operations
/// here are stateless across calls — without this, a panic in any
/// previous holder would take down the whole atomic-swap daemon on the
/// next DB access. (Audit finding F5.)
fn lock_conn_recover(m: &Arc<Mutex<Connection>>) -> MutexGuard<'_, Connection> {
    m.lock().unwrap_or_else(|poisoned| {
        eprintln!(
            "warning: EVM-watcher db mutex was poisoned by a panicking holder; \
             recovering inner connection state"
        );
        poisoned.into_inner()
    })
}

fn keccak256_topic(sig: &str) -> String {
    let mut hasher = Keccak256::new();
    hasher.update(sig.as_bytes());
    format!("0x{}", hex::encode(hasher.finalize()))
}

/// Pre-compute the three topics we care about.
pub struct Topics {
    pub locked: String, // Locked(bytes32,address,address,address,uint256,bytes32,uint256,string,string)
    pub claimed: String, // Claimed(bytes32,address,bytes32)
    pub refunded: String, // Refunded(bytes32,address)
}

impl Default for Topics {
    fn default() -> Self {
        Self::new()
    }
}

impl Topics {
    pub fn new() -> Self {
        Self {
            locked: keccak256_topic(
                "Locked(bytes32,address,address,address,uint256,bytes32,uint256,string,string)",
            ),
            claimed: keccak256_topic("Claimed(bytes32,address,bytes32)"),
            refunded: keccak256_topic("Refunded(bytes32,address)"),
        }
    }
}

// ─── JSON-RPC types ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

/// A single log entry returned by eth_getLogs.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct EthLog {
    address: String,
    topics: Vec<String>,
    data: String,
    #[serde(rename = "blockNumber")]
    block_number: String, // hex
    #[serde(rename = "transactionHash")]
    transaction_hash: String,
    removed: Option<bool>,
}

// ─── RPC helpers ─────────────────────────────────────────────────────────────

async fn eth_block_number(client: &reqwest::Client, rpc: &str) -> Result<u64> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "eth_blockNumber", "params": []
    });
    let resp: RpcResponse<String> = client.post(rpc).json(&body).send().await?.json().await?;
    if let Some(e) = resp.error {
        return Err(anyhow!("eth_blockNumber error: {}", e.message));
    }
    let hex = resp
        .result
        .ok_or_else(|| anyhow!("eth_blockNumber: no result"))?;
    Ok(u64::from_str_radix(hex.trim_start_matches("0x"), 16)?)
}

async fn eth_get_logs(
    client: &reqwest::Client,
    rpc: &str,
    contract: &str,
    topics: &[&str],
    from_block: u64,
    to_block: u64,
) -> Result<Vec<EthLog>> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 2,
        "method":  "eth_getLogs",
        "params":  [{
            "address":   contract,
            "topics":    [topics],       // OR-match: any of these topics at position 0
            "fromBlock": format!("0x{:x}", from_block),
            "toBlock":   format!("0x{:x}", to_block),
        }]
    });
    let resp: RpcResponse<Vec<EthLog>> = client.post(rpc).json(&body).send().await?.json().await?;
    if let Some(e) = resp.error {
        return Err(anyhow!("eth_getLogs error {}: {}", e.code, e.message));
    }
    Ok(resp.result.unwrap_or_default())
}

// ─── Decoding helpers ─────────────────────────────────────────────────────────

fn hex_to_u64(h: &str) -> u64 {
    let s = h.trim_start_matches("0x");
    u64::from_str_radix(s, 16).unwrap_or(0)
}

/// Decode ABI-encoded `(address, address, uint256, bytes32, uint256, string, string)` tail data.
///
/// EVM ABI encoding for Locked event (indexed: lock_id, initiator → in topics[1], topics[2]):
///   data = abi.encode(recipient, token, amount, hashlock, timelock, counterpartyChain, counterpartyAddr)
///
/// Layout (each slot = 32 bytes):
/// slot 0  — recipient  (address, right-padded lower 20 bytes)
/// slot 1  — token      (address)
/// slot 2  — amount     (uint256)
/// slot 3  — hashlock   (bytes32)
/// slot 4  — timelock   (uint256)
/// slot 5  — offset of counterpartyChain string
/// slot 6  — offset of counterpartyAddr  string
/// slot 7  — length of counterpartyChain
/// slot 8… — counterpartyChain bytes
/// …
fn decode_locked_data(
    data_hex: &str,
) -> Option<(String, String, String, String, u64, String, String)> {
    let raw = hex::decode(data_hex.trim_start_matches("0x")).ok()?;
    if raw.len() < 7 * 32 {
        return None;
    }

    let slot = |i: usize| &raw[i * 32..(i + 1) * 32];

    // recipient: last 20 bytes of slot 0
    let recipient = format!("0x{}", hex::encode(&slot(0)[12..]));
    // token: last 20 bytes of slot 1
    let token = format!("0x{}", hex::encode(&slot(1)[12..]));
    // amount: full slot 2 as decimal
    let amount_bytes: [u8; 32] = slot(2).try_into().ok()?;
    let amount = {
        let n = u128::from_be_bytes(amount_bytes[16..].try_into().ok()?);
        n.to_string()
    };
    // hashlock: slot 3
    let hashlock = format!("0x{}", hex::encode(slot(3)));
    // timelock: slot 4
    let timelock_bytes: [u8; 8] = slot(4)[24..].try_into().ok()?;
    let timelock = u64::from_be_bytes(timelock_bytes);

    // Dynamic strings: offsets at slot 5 and slot 6
    let chain_offset = usize::try_from(u64::from_be_bytes(slot(5)[24..].try_into().ok()?)).ok()?;
    let addr_offset = usize::try_from(u64::from_be_bytes(slot(6)[24..].try_into().ok()?)).ok()?;

    let read_string = |offset: usize| -> Option<String> {
        if raw.len() < offset + 32 {
            return None;
        }
        let len = usize::try_from(u64::from_be_bytes(
            raw[offset..offset + 32].get(24..32)?.try_into().ok()?,
        ))
        .ok()?;
        let start = offset + 32;
        let bytes = raw.get(start..start + len)?;
        String::from_utf8(bytes.to_vec()).ok()
    };

    let counterparty_chain = read_string(chain_offset)?;
    let counterparty_addr = read_string(addr_offset)?;

    Some((
        recipient,
        token,
        amount,
        hashlock,
        timelock,
        counterparty_chain,
        counterparty_addr,
    ))
}

// ─── Event processing ─────────────────────────────────────────────────────────

fn process_locked(conn: &Connection, log: &EthLog, _topics: &Topics) -> Result<()> {
    let lock_id = log.topics.get(1).cloned().unwrap_or_default();
    let initiator = {
        let raw = log.topics.get(2).cloned().unwrap_or_default();
        format!(
            "0x{}",
            raw.get(raw.len().saturating_sub(40)..)
                .unwrap_or("")
                .to_lowercase()
        )
    };

    let (recipient, token, amount, hashlock, timelock, chain, addr) = decode_locked_data(&log.data)
        .ok_or_else(|| anyhow!("decode_locked_data failed for tx {}", log.transaction_hash))?;

    let block = hex_to_u64(&log.block_number);
    let now = Utc::now().to_rfc3339();

    conn.execute(
        r#"INSERT OR IGNORE INTO evm_htlc_locks
           (lock_id, initiator, recipient, token, amount, hashlock, timelock,
            counterparty_chain, counterparty_addr, state, block_number, tx_hash, created_at, updated_at)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'pending',?10,?11,?12,?12)"#,
        params![lock_id, initiator, recipient, token, amount, hashlock, timelock,
                chain, addr, block as i64, log.transaction_hash, now],
    )?;
    info!(
        "EVM Locked  txn={} lock_id={} amount={} chain={}",
        log.transaction_hash, lock_id, amount, chain
    );
    Ok(())
}

fn process_claimed(conn: &Connection, log: &EthLog) -> Result<()> {
    let lock_id = log.topics.get(1).cloned().unwrap_or_default();
    let claimed_by = {
        let raw = log.topics.get(2).cloned().unwrap_or_default();
        format!(
            "0x{}",
            raw.get(raw.len().saturating_sub(40)..)
                .unwrap_or("")
                .to_lowercase()
        )
    };
    // preimage is in topics[3] (indexed bytes32)
    let preimage = log.topics.get(3).cloned().unwrap_or_default();
    let now = Utc::now().to_rfc3339();

    conn.execute(
        r#"UPDATE evm_htlc_locks
           SET state='claimed', preimage=?2, claimed_by=?3, updated_at=?4
           WHERE lock_id=?1"#,
        params![lock_id, preimage, claimed_by, now],
    )?;
    info!(
        "EVM Claimed  txn={} lock_id={}",
        log.transaction_hash, lock_id
    );
    Ok(())
}

fn process_refunded(conn: &Connection, log: &EthLog) -> Result<()> {
    let lock_id = log.topics.get(1).cloned().unwrap_or_default();
    let now = Utc::now().to_rfc3339();

    conn.execute(
        r#"UPDATE evm_htlc_locks SET state='refunded', updated_at=?2 WHERE lock_id=?1"#,
        params![lock_id, now],
    )?;
    info!(
        "EVM Refunded txn={} lock_id={}",
        log.transaction_hash, lock_id
    );
    Ok(())
}

// ─── Last-block persistence ───────────────────────────────────────────────────

fn get_last_block(conn: &Connection, start: u64) -> u64 {
    conn.query_row(
        "SELECT value FROM evm_watcher_state WHERE key='last_block'",
        [],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .and_then(|v| v.parse::<u64>().ok())
    .unwrap_or(start)
}

fn set_last_block(conn: &Connection, block: u64) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO evm_watcher_state (key, value) VALUES ('last_block', ?1)",
        params![block.to_string()],
    )?;
    Ok(())
}

// ─── Main watcher loop ────────────────────────────────────────────────────────

/// Spawn the EVM watcher as a long-running Tokio task.
///
/// db_conn must already have `migrate()` applied.
pub async fn run(cfg: EvmWatcherConfig, db_conn: Arc<std::sync::Mutex<Connection>>) -> Result<()> {
    if !cfg.enabled {
        info!("EVM watcher disabled — skipping");
        return Ok(());
    }

    let topics = Topics::new();
    info!("EVM watcher starting");
    info!("  RPC:        {}", cfg.rpc_url);
    info!(
        "  RPC backup: {}",
        cfg.rpc_url_backup.as_deref().unwrap_or("(none)")
    );
    info!("  Contract: {}", cfg.contract_addr);
    info!("  Locked topic:   {}", topics.locked);
    info!("  Claimed topic:  {}", topics.claimed);
    info!("  Refunded topic: {}", topics.refunded);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;

    loop {
        match poll_once(&cfg, &client, &topics, &db_conn).await {
            Ok(processed) => {
                if processed > 0 {
                    info!("EVM watcher processed {} logs", processed);
                }
            }
            Err(e) => {
                warn!("EVM watcher poll error: {}", e);
            }
        }
        sleep(Duration::from_secs(cfg.poll_interval_secs)).await;
    }
}

async fn poll_once(
    cfg: &EvmWatcherConfig,
    client: &reqwest::Client,
    topics: &Topics,
    db_conn: &Arc<std::sync::Mutex<Connection>>,
) -> Result<usize> {
    // ── Block number: primary RPC → backup fallback ───────────────────────
    let latest = match eth_block_number(client, &cfg.rpc_url).await {
        Ok(n) => n,
        Err(e) => {
            if let Some(backup) = &cfg.rpc_url_backup {
                debug!("Primary RPC block_number failed ({}) — trying backup", e);
                eth_block_number(client, backup)
                    .await
                    .context("eth_blockNumber (primary + backup both failed)")?
            } else {
                return Err(e).context("eth_blockNumber");
            }
        }
    };

    let from_block = {
        let conn = lock_conn_recover(db_conn);
        get_last_block(&conn, cfg.start_block)
    };

    if from_block >= latest {
        debug!("EVM watcher up-to-date at block {}", latest);
        return Ok(0);
    }

    let to_block = (from_block + 1_000 - 1).min(latest);

    // ── eth_getLogs: primary RPC → backup fallback ────────────────────────
    let logs = match eth_get_logs(
        client,
        &cfg.rpc_url,
        &cfg.contract_addr,
        &[&topics.locked, &topics.claimed, &topics.refunded],
        from_block + 1,
        to_block,
    )
    .await
    {
        Ok(l) => l,
        Err(e) => {
            if let Some(backup) = &cfg.rpc_url_backup {
                debug!("Primary RPC get_logs failed ({}) — trying backup", e);
                eth_get_logs(
                    client,
                    backup,
                    &cfg.contract_addr,
                    &[&topics.locked, &topics.claimed, &topics.refunded],
                    from_block + 1,
                    to_block,
                )
                .await
                .context("eth_getLogs (primary + backup both failed)")?
            } else {
                return Err(e).context("eth_getLogs");
            }
        }
    };

    let count = logs.len();
    if count > 0 {
        debug!(
            "EVM watcher: {} logs in blocks {}..{}",
            count,
            from_block + 1,
            to_block
        );
    }

    {
        let conn = lock_conn_recover(db_conn);
        for log in &logs {
            // Skip removed (re-org) logs
            if log.removed.unwrap_or(false) {
                continue;
            }

            let topic0 = log.topics.first().map(String::as_str).unwrap_or("");
            let result = if topic0 == topics.locked {
                process_locked(&conn, log, topics)
            } else if topic0 == topics.claimed {
                process_claimed(&conn, log)
            } else if topic0 == topics.refunded {
                process_refunded(&conn, log)
            } else {
                debug!("EVM watcher: unknown topic {}", topic0);
                Ok(())
            };

            if let Err(e) = result {
                error!(
                    "EVM watcher: error processing log {}: {}",
                    log.transaction_hash, e
                );
            }
        }
        set_last_block(&conn, to_block)?;
    }

    Ok(count)
}

// ─── Query helpers (exposed to HTTP handlers) ─────────────────────────────────

/// Return all EVM HTLCs with the given state.
pub fn get_evm_locks_by_state(conn: &Connection, state: &str) -> Result<Vec<EvmLock>> {
    let mut stmt = conn.prepare(
        "SELECT lock_id, initiator, recipient, token, amount, hashlock, timelock,
                counterparty_chain, counterparty_addr, state, preimage, claimed_by,
                block_number, tx_hash, created_at, updated_at
         FROM   evm_htlc_locks WHERE state = ?1 ORDER BY block_number DESC",
    )?;
    let rows = stmt.query_map(params![state], |row| {
        Ok(EvmLock {
            lock_id: row.get(0)?,
            initiator: row.get(1)?,
            recipient: row.get(2)?,
            token: row.get(3)?,
            amount: row.get(4)?,
            hashlock: row.get(5)?,
            timelock: row.get::<_, i64>(6)? as u64,
            counterparty_chain: row.get(7)?,
            counterparty_addr: row.get(8)?,
            state: row.get(9)?,
            preimage: row.get(10)?,
            claimed_by: row.get(11)?,
            block_number: row.get::<_, i64>(12)? as u64,
            tx_hash: row.get(13)?,
            created_at: row.get(14)?,
            updated_at: row.get(15)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Look up a single EVM HTLC by lock_id.
pub fn get_evm_lock(conn: &Connection, lock_id: &str) -> Result<Option<EvmLock>> {
    conn.query_row(
        "SELECT lock_id, initiator, recipient, token, amount, hashlock, timelock,
                counterparty_chain, counterparty_addr, state, preimage, claimed_by,
                block_number, tx_hash, created_at, updated_at
         FROM   evm_htlc_locks WHERE lock_id = ?1",
        params![lock_id],
        |row| {
            Ok(EvmLock {
                lock_id: row.get(0)?,
                initiator: row.get(1)?,
                recipient: row.get(2)?,
                token: row.get(3)?,
                amount: row.get(4)?,
                hashlock: row.get(5)?,
                timelock: row.get::<_, i64>(6)? as u64,
                counterparty_chain: row.get(7)?,
                counterparty_addr: row.get(8)?,
                state: row.get(9)?,
                preimage: row.get(10)?,
                claimed_by: row.get(11)?,
                block_number: row.get::<_, i64>(12)? as u64,
                tx_hash: row.get(13)?,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn test_topics_deterministic() {
        let t1 = Topics::new();
        let t2 = Topics::new();
        assert_eq!(t1.locked, t2.locked);
        assert_eq!(t1.claimed, t2.claimed);
        assert_eq!(t1.refunded, t2.refunded);
        // Must be valid 0x…66hex chars
        assert!(t1.locked.starts_with("0x"));
        assert_eq!(t1.locked.len(), 66);
    }

    #[test]
    fn test_migrate_idempotent() {
        let conn = in_memory_db();
        // second migration must not fail
        migrate(&conn).unwrap();
    }

    #[test]
    fn test_last_block_defaults_to_start() {
        let conn = in_memory_db();
        assert_eq!(get_last_block(&conn, 1234), 1234);
    }

    #[test]
    fn test_set_get_last_block() {
        let conn = in_memory_db();
        set_last_block(&conn, 42_000).unwrap();
        assert_eq!(get_last_block(&conn, 0), 42_000);
        // update
        set_last_block(&conn, 43_000).unwrap();
        assert_eq!(get_last_block(&conn, 0), 43_000);
    }

    #[test]
    fn test_get_evm_lock_not_found() {
        let conn = in_memory_db();
        let result = get_evm_lock(&conn, "0xdeadbeef").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_insert_and_query_lock() {
        let conn = in_memory_db();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            r#"INSERT INTO evm_htlc_locks
               (lock_id, initiator, recipient, token, amount, hashlock, timelock,
                counterparty_chain, counterparty_addr, state, block_number, tx_hash, created_at, updated_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'pending',?10,?11,?12,?12)"#,
            params!["0xabc", "0xalice", "0xbob", "0x0", "1000000", "0xhash", 9999i64,
                    "zion", "zion1test", 100i64, "0xtx", now],
        ).unwrap();

        let lock = get_evm_lock(&conn, "0xabc").unwrap().unwrap();
        assert_eq!(lock.state, "pending");
        assert_eq!(lock.amount, "1000000");
        assert_eq!(lock.counterparty_chain, "zion");

        let pending = get_evm_locks_by_state(&conn, "pending").unwrap();
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn test_update_to_claimed() {
        let conn = in_memory_db();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            r#"INSERT INTO evm_htlc_locks
               (lock_id, initiator, recipient, token, amount, hashlock, timelock,
                counterparty_chain, counterparty_addr, state, block_number, tx_hash, created_at, updated_at)
               VALUES ('0xid','0xa','0xb','0x0','100','0xh',9000,'btc','bc1q','pending',1,'0xtx',?1,?1)"#,
            params![now],
        ).unwrap();

        conn.execute(
            "UPDATE evm_htlc_locks SET state='claimed', preimage='0xpre', claimed_by='0xb', updated_at=?2 WHERE lock_id=?1",
            params!["0xid", now],
        ).unwrap();

        let lock = get_evm_lock(&conn, "0xid").unwrap().unwrap();
        assert_eq!(lock.state, "claimed");
        assert_eq!(lock.preimage, Some("0xpre".into()));
    }
}
