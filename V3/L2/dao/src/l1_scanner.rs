//! L1 Memo Scanner — watches the ZION L1 blockchain for DAO governance events.
//!
//! ## How the DAO uses L1
//!
//! ZION DAO works **without a smart contract on L1**. Instead, holders signal
//! governance intent by sending a tiny self-transfer TX with a structured memo:
//!
//! | Action                   | Memo format                           |
//! |--------------------------|---------------------------------------|
//! | Vote yes on proposal 42  | `DAO:vote:42:yes`                     |
//! | Vote no on proposal 42   | `DAO:vote:42:no`                      |
//! | Abstain on proposal 42   | `DAO:vote:42:abstain`                 |
//! | Register as candidate    | `DAO:guardian:register:<pubkey>`      |
//!
//! The scanner polls the L1 RPC, parses memos, verifies balance at TX block,
//! and records votes in the SQLite DB.
//!
//! ## Security Model
//!
//! - Weight = ZION balance of the sending address **at the TX block** (snapshot)
//! - TX must come from the same address that intends to vote (no proxy)
//! - Duplicate memos from the same address for the same proposal are ignored
//! - Minimum vote weight: 1 ZION (avoids dust spam)

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::db::DaoDb;
use crate::error::{DaoError, DaoResult};
use crate::types::{parse_dao_memo, DaoMemo};

// ─────────────────────────────────────────────────────────────────────────────
// L1 RPC types (minimal — only what we need)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct BlockInfo {
    height: u64,
    #[serde(default)]
    utxo_transactions: Vec<UtxoTransaction>,
    #[serde(default)]
    account_transactions: Vec<AccountTransaction>,
}

#[derive(Debug, Deserialize)]
struct UtxoTransaction {
    id: Vec<u8>,
    #[serde(default)]
    inputs: Vec<UtxoInput>,
    #[serde(default)]
    outputs: Vec<TxOutput>,
}

#[derive(Debug, Deserialize)]
struct AccountTransaction {
    tx_id: String,
    from: String,
    #[serde(default)]
    #[allow(dead_code)]
    to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    memo: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UtxoInput {
    #[serde(default)]
    public_key: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TxOutput {
    address: String,
    #[serde(default)]
    memo: Option<String>,
    amount: u64,
}

#[derive(Debug, Deserialize)]
struct BalanceAtHeightInfo {
    #[serde(default)]
    balance_flowers: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Scanner Config
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ScannerConfig {
    /// L1 RPC address, e.g. `127.0.0.1:8443`
    pub rpc_url: String,
    /// Poll interval (how often to ask for new blocks)
    pub poll_interval: Duration,
    /// Minimum ZION balance to count a vote (1 ZION = 1_000_000 flowers)
    pub min_vote_weight: u64,
    /// Number of blocks to confirm before accepting TX (finality)
    pub finality_blocks: u64,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            rpc_url: "127.0.0.1:8443".to_string(),
            poll_interval: Duration::from_secs(30),
            min_vote_weight: 1_000_000, // 1 ZION in flowers (6-decimal)
            finality_blocks: 6,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Scanner
// ─────────────────────────────────────────────────────────────────────────────

pub struct L1Scanner {
    config: ScannerConfig,
    db: Arc<Mutex<DaoDb>>,
    /// How many DAO events were processed this session
    events_processed: Arc<std::sync::atomic::AtomicU64>,
    /// Shared metrics counters (optional — None in tests)
    metrics: Option<Arc<crate::metrics::DaoMetrics>>,
}

impl L1Scanner {
    pub fn new(config: ScannerConfig, db: Arc<Mutex<DaoDb>>) -> Self {
        Self {
            config,
            db,
            events_processed: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            metrics: None,
        }
    }

    /// Attach shared metrics counters for Prometheus reporting.
    pub fn with_metrics(mut self, metrics: Arc<crate::metrics::DaoMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Number of events processed since startup.
    pub fn events_processed(&self) -> u64 {
        self.events_processed
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    // ── Main loop ─────────────────────────────────────────────────────────────

    /// Run forever — call this in a tokio::spawn task.
    pub async fn run(&self) {
        info!(
            "[DAO-SCANNER] Starting L1 scanner → {}",
            self.config.rpc_url
        );

        loop {
            match self.scan_new_blocks().await {
                Ok(found) => {
                    if found > 0 {
                        info!("[DAO-SCANNER] Processed {} DAO event(s)", found);
                    } else {
                        debug!("[DAO-SCANNER] No new DAO events");
                    }
                }
                Err(e) => {
                    warn!("[DAO-SCANNER] Scan error: {}", e);
                }
            }
            sleep(self.config.poll_interval).await;
        }
    }

    async fn scan_new_blocks(&self) -> DaoResult<u64> {
        // Get current L1 tip
        let tip_height = self.get_chain_height().await?;

        // Read cursor from DB
        let cursor = {
            let db = self.db.lock().await;
            db.last_scanned_block()?
        };

        // Adjust for finality
        let safe_height = tip_height.saturating_sub(self.config.finality_blocks);
        if cursor >= safe_height {
            return Ok(0); // nothing new
        }

        let mut events_found = 0u64;

        // Scan each new block
        for height in (cursor + 1)..=safe_height {
            let block = match self.get_block(height).await {
                Ok(b) => b,
                Err(e) => {
                    warn!("[DAO-SCANNER] Failed to fetch block {}: {}", height, e);
                    break;
                }
            };

            let mut processed: std::collections::HashSet<String> = std::collections::HashSet::new();

            for tx in &block.utxo_transactions {
                let txid = bytes_to_hex(&tx.id);
                if !processed.insert(txid.clone()) {
                    continue;
                }
                match self.process_tx(tx, height).await {
                    Ok(true) => {
                        events_found += 1;
                        self.events_processed
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    Ok(false) => {}
                    Err(e) => {
                        debug!("[DAO-SCANNER] TX {} error: {}", txid, e);
                    }
                }
            }

            for tx in &block.account_transactions {
                if !processed.insert(tx.tx_id.clone()) {
                    continue;
                }
                match self.process_account_tx(tx, height).await {
                    Ok(true) => {
                        events_found += 1;
                        self.events_processed
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    Ok(false) => {}
                    Err(e) => {
                        debug!("[DAO-SCANNER] Account TX {} error: {}", tx.tx_id, e);
                    }
                }
            }

            // Update cursor after each block (so crash mid-range doesn't restart)
            let db = self.db.lock().await;
            db.set_last_scanned_block(height)?;
            drop(db);

            // Increment Prometheus counter
            if let Some(m) = &self.metrics {
                m.l1_blocks_scanned
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }

        Ok(events_found)
    }

    /// Returns `true` if this TX contained a valid DAO memo that was processed.
    async fn process_tx(&self, tx: &UtxoTransaction, block_height: u64) -> DaoResult<bool> {
        let sender = tx
            .inputs
            .first()
            .filter(|input| input.public_key.len() == 32)
            .map(|input| zion_address_from_public_key(&input.public_key))
            .ok_or_else(|| {
                DaoError::Internal("DAO memo tx missing valid sender public key".into())
            })?;

        let txid = bytes_to_hex(&tx.id);

        self.process_dao_memo(
            &sender,
            &txid,
            tx.outputs.iter().filter_map(|o| o.memo.as_deref()),
            block_height,
        )
        .await
    }

    /// Returns `true` if this account TX contained a valid DAO memo that was processed.
    async fn process_account_tx(
        &self,
        tx: &AccountTransaction,
        block_height: u64,
    ) -> DaoResult<bool> {
        let sender = &tx.from;
        let txid = &tx.tx_id;
        let memo = tx.memo.as_deref();
        self.process_dao_memo(sender, txid, memo.into_iter(), block_height)
            .await
    }

    /// Process DAO memos for a sender/txid pair.
    async fn process_dao_memo<'a>(
        &self,
        sender: &str,
        txid: &str,
        memos: impl Iterator<Item = &'a str>,
        block_height: u64,
    ) -> DaoResult<bool> {
        let mut any_processed = false;

        for memo in memos {
            if memo.is_empty() {
                continue;
            }
            // L3: reject oversized memos before parsing to bound CPU/alloc.
            // L1 consensus already caps memo at 256 bytes (lib.rs:1937-1943),
            // so anything larger is either a bug or a non-DAO payload.
            if memo.len() > 256 {
                debug!(
                    "[DAO-SCANNER] Skipping oversized memo ({} bytes > 256) in tx {}",
                    memo.len(),
                    txid
                );
                continue;
            }

            let parsed = match parse_dao_memo(memo) {
                Some(p) => p,
                None => continue,
            };

            match parsed {
                DaoMemo::Vote {
                    proposal_id,
                    choice,
                } => {
                    let pid: u64 = proposal_id.parse().map_err(|_| {
                        DaoError::Internal(format!("Invalid proposal id: {}", proposal_id))
                    })?;

                    let weight = self.get_balance_at(sender, block_height).await?;
                    if weight < self.config.min_vote_weight {
                        debug!(
                            "[DAO-SCANNER] Skipping dust vote from {} (weight {})",
                            sender, weight
                        );
                        continue;
                    }

                    let recorded = {
                        let db = self.db.lock().await;

                        let row = db.get_proposal(pid)?;
                        match row {
                            None => {
                                debug!("[DAO-SCANNER] Proposal {} not found, ignoring vote", pid);
                                continue;
                            }
                            Some(ref r) if r.status != "Active" => {
                                debug!(
                                    "[DAO-SCANNER] Proposal {} not active ({}), ignoring",
                                    pid, r.status
                                );
                                continue;
                            }
                            _ => {}
                        }

                        db.record_vote(pid, sender, choice, weight, Some(txid))?
                    };

                    if recorded {
                        info!(
                            "[DAO-SCANNER] Vote recorded: proposal={} voter={} weight={}",
                            pid, sender, weight
                        );
                        any_processed = true;
                    } else {
                        debug!(
                            "[DAO-SCANNER] Duplicate vote ignored: proposal={} voter={}",
                            pid, sender
                        );
                    }
                }

                DaoMemo::Propose { proposal_type } => {
                    info!(
                        "[DAO-SCANNER] Propose memo from {} (type {}), ignored for now",
                        sender, proposal_type
                    );
                    continue;
                }

                DaoMemo::Execute { proposal_id } => {
                    info!(
                        "[DAO-SCANNER] Execute memo for proposal {}, ignored for now",
                        proposal_id
                    );
                    continue;
                }
            }
        }

        Ok(any_processed)
    }

    // ── L1 RPC helpers ────────────────────────────────────────────────────────

    async fn rpc<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> DaoResult<T> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        })
        .to_string();

        let mut stream = TcpStream::connect(normalize_rpc_addr(&self.config.rpc_url))
            .await
            .map_err(|e| DaoError::Internal(format!("RPC connect failed: {}", e)))?;
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| DaoError::Internal(format!("RPC write failed: {}", e)))?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|e| DaoError::Internal(format!("RPC newline write failed: {}", e)))?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| DaoError::Internal(format!("RPC read failed: {}", e)))?;

        let rpc_resp: RpcResponse<T> = serde_json::from_str(line.trim())
            .map_err(|e| DaoError::Internal(format!("RPC parse error: {}", e)))?;

        if let Some(err) = rpc_resp.error {
            return Err(DaoError::Internal(format!("RPC error: {}", err)));
        }

        rpc_resp
            .result
            .ok_or_else(|| DaoError::Internal("RPC returned null result".to_string()))
    }

    async fn get_chain_height(&self) -> DaoResult<u64> {
        #[derive(Deserialize)]
        struct Info {
            chain_height: u64,
        }
        let info: Info = self.rpc("getChainInfo", serde_json::json!({})).await?;
        Ok(info.chain_height)
    }

    async fn get_block(&self, height: u64) -> DaoResult<BlockInfo> {
        self.rpc("getBlockByHeight", serde_json::json!({ "height": height }))
            .await
    }

    async fn get_balance_at(&self, address: &str, block: u64) -> DaoResult<u64> {
        let bal: BalanceAtHeightInfo = self
            .rpc(
                "getBalanceAtHeight",
                serde_json::json!({ "address": address, "height": block }),
            )
            .await?;
        Ok(bal.balance_flowers)
    }
}

fn normalize_rpc_addr(raw: &str) -> String {
    raw.trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_start_matches("tcp://")
        .split('/')
        .next()
        .unwrap_or(raw)
        .to_string()
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}

fn zion_address_from_public_key(public_key: &[u8]) -> String {
    use ripemd::Ripemd160;
    use sha2::{Digest, Sha256};

    const ALPHABET: &[u8; 32] = b"023456789acdefghjklmnpqrstuvwxyz";

    let sha = Sha256::digest(public_key);
    let key_hash = Ripemd160::digest(sha);

    let mut body = String::with_capacity(40);
    for &byte in key_hash.as_slice() {
        body.push(ALPHABET[(byte % 32) as usize] as char);
        body.push(ALPHABET[((byte / 32) % 32) as usize] as char);
    }
    body.truncate(35);

    let mut hasher = Sha256::new();
    hasher.update(b"zion1");
    hasher.update(body.as_bytes());
    let hash = hasher.finalize();
    let mut checksum = String::with_capacity(4);
    for &byte in &hash[..2] {
        checksum.push(ALPHABET[(byte % 32) as usize] as char);
        checksum.push(ALPHABET[((byte / 32) % 32) as usize] as char);
    }
    format!("zion1{body}{checksum}")
}

// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::parse_dao_memo;

    #[test]
    fn test_memo_parsing() {
        // Valid vote memos
        assert!(parse_dao_memo("DAO:vote:42:yes").is_some());
        assert!(parse_dao_memo("DAO:vote:1:no").is_some());
        assert!(parse_dao_memo("DAO:vote:99:abstain").is_some());

        // Invalid memos (not DAO)
        assert!(parse_dao_memo("BRIDGE:base:0xabc").is_none());
        assert!(parse_dao_memo("hello world").is_none());
        assert!(parse_dao_memo("DAO:vote:42").is_none()); // missing choice
    }

    #[test]
    fn test_config_defaults() {
        let cfg = ScannerConfig::default();
        assert_eq!(cfg.finality_blocks, 6);
        assert_eq!(cfg.min_vote_weight, 1_000_000); // 1 ZION in flowers (6-decimal)
    }
}
