//! L1 block watcher — scans for SWAP:LOCK, SWAP:CLAIM, SWAP:REFUND memos.
//!
//! The watcher loops over new L1 blocks, inspects every transaction output
//! whose `address` equals the escrow address, parses the memo, and writes
//! HTLC records into the database.  Claim and refund memos trigger the
//! executor asynchronously.

use crate::config::SwapConfig;
use crate::db::SwapDb;
use crate::error::SwapResult;
use crate::executor::SwapExecutor;
use crate::types::{
    bytes_to_hex, normalize_rpc_addr, zion_address_from_public_key, HtlcRecord, L1Block,
    L1ChainInfo, RpcResponse, SwapMemo, SwapState,
};
use chrono::Utc;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

// ─── L1Watcher ───────────────────────────────────────────────────────────────

pub struct L1Watcher {
    cfg: Arc<SwapConfig>,
    db: Arc<SwapDb>,
    executor: Arc<SwapExecutor>,
    escrow_address: String,
}

impl L1Watcher {
    pub fn new(
        cfg: Arc<SwapConfig>,
        db: Arc<SwapDb>,
        executor: Arc<SwapExecutor>,
        escrow_address: String,
    ) -> Self {
        Self {
            cfg,
            db,
            executor,
            escrow_address,
        }
    }

    /// Run forever — call once from `tokio::spawn`.
    pub async fn run(&self) {
        let interval = Duration::from_secs(self.cfg.l1.poll_interval_secs);
        info!(
            "🔍 L1 watcher started — escrow={} poll={}s",
            self.escrow_address, self.cfg.l1.poll_interval_secs
        );
        loop {
            if let Err(e) = self.tick().await {
                error!("Watcher tick error: {e}");
            }
            sleep(interval).await;
        }
    }

    // ── Single poll iteration ─────────────────────────────────────────────

    async fn tick(&self) -> SwapResult<()> {
        let current_height = match self.fetch_chain_height().await {
            Ok(h) => h,
            Err(e) => {
                warn!("Cannot fetch chain height: {e}");
                return Ok(());
            }
        };

        let scan_from = self.db.get_scan_height()?;
        if scan_from >= current_height {
            debug!("Watcher up-to-date at height {current_height}");
            return Ok(());
        }

        let scan_to = (scan_from + self.cfg.l1.scan_batch_size).min(current_height);
        debug!("Scanning L1 blocks {scan_from}..{scan_to}");

        for height in (scan_from + 1)..=scan_to {
            if let Err(e) = self.scan_block(height).await {
                warn!("Error scanning block {height}: {e}");
                // Don't advance cursor past the failed block
                break;
            }
            self.db.set_scan_height(height)?;
        }
        Ok(())
    }

    // ── Block scanning ────────────────────────────────────────────────────

    async fn scan_block(&self, height: u64) -> SwapResult<()> {
        let block = self.fetch_block(height).await?;
        let mut processed: std::collections::HashSet<String> = std::collections::HashSet::new();

        for tx in &block.utxo_transactions {
            let tx_id = bytes_to_hex(&tx.id);
            if !processed.insert(tx_id.clone()) {
                continue;
            }
            let sender = tx
                .inputs
                .first()
                .and_then(|input| zion_address_from_public_key(&input.public_key))
                .unwrap_or_else(|| "unknown".into());
            for output in &tx.outputs {
                // Only consider outputs sent to the escrow address
                if output.address != self.escrow_address {
                    continue;
                }
                let memo = match &output.memo {
                    Some(m) => m.as_str(),
                    None => continue,
                };
                let parsed = match SwapMemo::parse(memo) {
                    Some(m) => m,
                    None => continue,
                };
                if let Err(e) = self
                    .handle_memo(parsed, tx_id.clone(), sender.clone(), output.amount, height)
                    .await
                {
                    error!("Error handling memo '{}' in tx {}: {e}", memo, tx_id);
                }
            }
        }

        for tx in &block.account_transactions {
            if tx.to != self.escrow_address {
                continue;
            }
            if !processed.insert(tx.tx_id.clone()) {
                continue;
            }
            let memo = match &tx.memo {
                Some(m) => m.as_str(),
                None => continue,
            };
            let parsed = match SwapMemo::parse(memo) {
                Some(m) => m,
                None => continue,
            };
            let amount = match u64::try_from(tx.amount_zion) {
                Ok(a) => a,
                Err(_) => {
                    warn!(
                        "account tx {} amount {} exceeds u64 — skipping (H1 guard)",
                        tx.tx_id, tx.amount_zion
                    );
                    continue;
                }
            };
            if let Err(e) = self
                .handle_memo(parsed, tx.tx_id.clone(), tx.from.clone(), amount, height)
                .await
            {
                error!(
                    "Error handling memo '{}' in account tx {}: {e}",
                    memo, tx.tx_id
                );
            }
        }

        Ok(())
    }

    async fn handle_memo(
        &self,
        memo: SwapMemo,
        tx_id: String,
        sender: String,
        amount: u64,
        block_height: u64,
    ) -> SwapResult<()> {
        match memo {
            // ── LOCK ──────────────────────────────────────────────────────
            SwapMemo::Lock {
                hash_hex,
                timeout_minutes,
                counterparty_chain,
                counterparty_addr,
                claimant_address,
            } => {
                // De-duplicate: if we already have this hash, skip
                if self.db.get_htlc(&hash_hex)?.is_some() {
                    debug!("LOCK {hash_hex} already registered — skipping");
                    return Ok(());
                }

                let expires_at = Utc::now().timestamp() + (timeout_minutes as i64 * 60);

                // Sanity: enforce config bounds
                let min = self.cfg.swap.min_lock_flowers;
                let max = self.cfg.swap.max_lock_atomic;
                if amount < min || amount > max {
                    warn!("LOCK {hash_hex} amount {amount} outside [{min},{max}] — rejected");
                    return Ok(());
                }

                let now = Utc::now();
                let rec = HtlcRecord {
                    hash_hex: hash_hex.clone(),
                    locker_address: sender,
                    amount_flowers: amount,
                    lock_tx_id: tx_id.clone(),
                    lock_block_height: block_height,
                    expires_at,
                    counterparty_chain,
                    counterparty_addr,
                    claimant_address,
                    state: SwapState::Pending,
                    release_tx_id: None,
                    release_recipient: None,
                    preimage_hex: None,
                    created_at: now,
                    updated_at: now,
                };

                self.db.insert_htlc(&rec)?;
                info!("🔒 HTLC locked  hash={hash_hex} amount={amount} tx={tx_id}");
            }

            // ── CLAIM ─────────────────────────────────────────────────────
            SwapMemo::Claim {
                hash_hex,
                preimage_hex,
            } => {
                info!("🔑 CLAIM memo   hash={hash_hex} tx={tx_id}");
                let executor = Arc::clone(&self.executor);
                let db = Arc::clone(&self.db);
                tokio::spawn(async move {
                    if let Err(e) = executor
                        .execute_claim(&db, &hash_hex, &preimage_hex, &sender)
                        .await
                    {
                        error!("Claim execution failed for {hash_hex}: {e}");
                        let _ = db.mark_error(&hash_hex, &e.to_string());
                    }
                });
            }

            // ── REFUND ────────────────────────────────────────────────────
            SwapMemo::Refund { hash_hex } => {
                info!("↩️  REFUND memo  hash={hash_hex} tx={tx_id}");
                let executor = Arc::clone(&self.executor);
                let db = Arc::clone(&self.db);
                tokio::spawn(async move {
                    if let Err(e) = executor.execute_refund(&db, &hash_hex).await {
                        error!("Refund execution failed for {hash_hex}: {e}");
                        let _ = db.mark_error(&hash_hex, &e.to_string());
                    }
                });
            }
        }
        Ok(())
    }

    // ── L1 RPC helpers ────────────────────────────────────────────────────

    async fn fetch_chain_height(&self) -> SwapResult<u64> {
        let info: L1ChainInfo = self.rpc("getChainInfo", json!({})).await?;
        Ok(info.chain_height)
    }

    async fn fetch_block(&self, height: u64) -> SwapResult<L1Block> {
        self.rpc("getBlockByHeight", json!({ "height": height }))
            .await
    }

    async fn rpc<T: DeserializeOwned>(&self, method: &str, params: Value) -> SwapResult<T> {
        let address = normalize_rpc_addr(&self.cfg.l1.rpc_url);
        let mut stream = TcpStream::connect(&address)
            .await
            .map_err(|e| crate::error::SwapError::L1Rpc(format!("RPC connect failed: {e}")))?;

        let request = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1,
        }))
        .map_err(|e| crate::error::SwapError::L1Rpc(format!("RPC encode failed: {e}")))?;

        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| crate::error::SwapError::L1Rpc(format!("RPC write failed: {e}")))?;
        stream.write_all(b"\n").await.map_err(|e| {
            crate::error::SwapError::L1Rpc(format!("RPC newline write failed: {e}"))
        })?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| crate::error::SwapError::L1Rpc(format!("RPC read failed: {e}")))?;

        let rpc_resp: RpcResponse<T> = serde_json::from_str(line.trim())
            .map_err(|e| crate::error::SwapError::L1Rpc(format!("RPC parse failed: {e}")))?;

        if let Some(err) = rpc_resp.error {
            return Err(crate::error::SwapError::L1Rpc(format!("RPC error: {err}")));
        }

        rpc_resp
            .result
            .ok_or_else(|| crate::error::SwapError::L1Rpc("RPC returned null result".into()))
    }
}

// ─── Auto-refund background loop ─────────────────────────────────────────────

pub struct RefundLoop {
    cfg: Arc<SwapConfig>,
    db: Arc<SwapDb>,
    executor: Arc<SwapExecutor>,
}

impl RefundLoop {
    pub fn new(cfg: Arc<SwapConfig>, db: Arc<SwapDb>, executor: Arc<SwapExecutor>) -> Self {
        Self { cfg, db, executor }
    }

    pub async fn run(&self) {
        if !self.cfg.refund.auto_refund {
            info!("Auto-refund loop disabled by config");
            return;
        }
        let interval = Duration::from_secs(self.cfg.refund.check_interval_secs);
        info!(
            "♻️  Refund loop started — check every {}s grace={}s",
            self.cfg.refund.check_interval_secs, self.cfg.refund.grace_period_secs
        );
        loop {
            if let Err(e) = self.tick().await {
                error!("Refund loop tick error: {e}");
            }
            sleep(interval).await;
        }
    }

    async fn tick(&self) -> SwapResult<()> {
        let expired = self.db.get_expired_pending()?;
        for rec in expired {
            // Apply extra grace period
            let grace_end = rec.expires_at + self.cfg.refund.grace_period_secs as i64;
            if Utc::now().timestamp() < grace_end {
                continue;
            }
            info!("♻️  Auto-refunding expired HTLC {}", rec.hash_hex);
            let executor = Arc::clone(&self.executor);
            let db = Arc::clone(&self.db);
            let hash = rec.hash_hex.clone();
            tokio::spawn(async move {
                if let Err(e) = executor.execute_refund(&db, &hash).await {
                    error!("Auto-refund failed for {hash}: {e}");
                    let _ = db.mark_error(&hash, &e.to_string());
                }
            });
        }
        Ok(())
    }
}
