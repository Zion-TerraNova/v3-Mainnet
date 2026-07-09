//! L1 blockchain scanner for humanitarian fund accumulation.
//!
//! Watches L1 coinbase transactions for the 5% humanitarian tithe output
//! and accumulates the running total in the local database.

use crate::db::FreeWorldDb;
use crate::error::FreeWorldResult;
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::sleep;
use tracing::{debug, info, warn};

#[derive(Clone)]
pub struct ScannerConfig {
    pub rpc_url: String,
    pub poll_interval: Duration,
    pub fund_address: String,
    pub finality_blocks: u64,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            rpc_url: "127.0.0.1:8443".to_string(),
            poll_interval: Duration::from_secs(30),
            fund_address: "zion1humanitarian0000000000000000000000".to_string(),
            finality_blocks: 6,
        }
    }
}

pub struct L1Scanner {
    config: ScannerConfig,
    db: Arc<Mutex<FreeWorldDb>>,
    blocks_scanned: Arc<std::sync::atomic::AtomicU64>,
}

impl L1Scanner {
    pub fn new(config: ScannerConfig, db: Arc<Mutex<FreeWorldDb>>) -> Self {
        Self {
            config,
            db,
            blocks_scanned: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub async fn run(&self) {
        info!(
            "[FW-SCANNER] Starting L1 scanner → {} (fund={})",
            self.config.rpc_url, self.config.fund_address
        );

        loop {
            match self.scan_new_blocks().await {
                Ok(found) => {
                    if found > 0 {
                        info!("[FW-SCANNER] Accumulated {} tithe(s)", found);
                    } else {
                        debug!("[FW-SCANNER] No new blocks");
                    }
                }
                Err(e) => {
                    warn!("[FW-SCANNER] Scan error: {}", e);
                }
            }
            sleep(self.config.poll_interval).await;
        }
    }

    async fn scan_new_blocks(&self) -> FreeWorldResult<u64> {
        let tip_height = self.get_chain_height().await?;

        let cursor = {
            let db = self.db.lock().unwrap();
            db.get_fund_balance()?.last_block_height
        };

        let safe_height = tip_height.saturating_sub(self.config.finality_blocks);
        if cursor >= safe_height {
            return Ok(0);
        }

        let mut tithes_found = 0u64;

        for height in (cursor + 1)..=safe_height {
            let block = match self.get_block(height).await {
                Ok(b) => b,
                Err(e) => {
                    warn!("[FW-SCANNER] Failed to fetch block {}: {}", height, e);
                    break;
                }
            };

            // Coinbase is the first transaction in every block
            if let Some(coinbase) = block.utxo_transactions.first() {
                for output in &coinbase.outputs {
                    if output.address == self.config.fund_address {
                        let db = self.db.lock().unwrap();
                        let mut balance = db.get_fund_balance()?;
                        balance.total_accumulated += output.amount;
                        balance.last_block_height = height;
                        balance.updated_at = chrono::Utc::now().to_rfc3339();
                        db.update_fund_balance(&balance)?;
                        tithes_found += 1;
                        info!(
                            "[FW-SCANNER] Tithe @ block {}: {} flowers → total {} flowers",
                            height, output.amount, balance.total_accumulated
                        );
                    }
                }
            }

            self.blocks_scanned
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        Ok(tithes_found)
    }

    // ── L1 RPC helpers ──

    async fn rpc<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> FreeWorldResult<T> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        })
        .to_string();

        let addr = normalize_rpc_addr(&self.config.rpc_url);
        let mut stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| crate::error::FreeWorldError::L1Rpc(format!("connect: {}", e)))?;
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| crate::error::FreeWorldError::L1Rpc(format!("write: {}", e)))?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|e| crate::error::FreeWorldError::L1Rpc(format!("newline: {}", e)))?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| crate::error::FreeWorldError::L1Rpc(format!("read: {}", e)))?;

        #[derive(Deserialize)]
        struct RpcResponse<T> {
            result: Option<T>,
            error: Option<serde_json::Value>,
        }

        let rpc_resp: RpcResponse<T> = serde_json::from_str(line.trim())
            .map_err(|e| crate::error::FreeWorldError::L1Rpc(format!("parse: {}", e)))?;

        if let Some(err) = rpc_resp.error {
            return Err(crate::error::FreeWorldError::L1Rpc(format!(
                "rpc error: {}",
                err
            )));
        }

        rpc_resp
            .result
            .ok_or_else(|| crate::error::FreeWorldError::L1Rpc("null result".to_string()))
    }

    async fn get_chain_height(&self) -> FreeWorldResult<u64> {
        #[derive(Deserialize)]
        struct Info {
            chain_height: u64,
        }
        let info: Info = self.rpc("getChainInfo", serde_json::json!({})).await?;
        Ok(info.chain_height)
    }

    async fn get_block(&self, height: u64) -> FreeWorldResult<BlockInfo> {
        self.rpc("getBlockByHeight", serde_json::json!({ "height": height }))
            .await
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

#[derive(Debug, Deserialize)]
struct BlockInfo {
    #[serde(default)]
    utxo_transactions: Vec<UtxoTransaction>,
}

#[derive(Debug, Deserialize)]
struct UtxoTransaction {
    #[serde(default)]
    outputs: Vec<TxOutput>,
}

#[derive(Debug, Deserialize)]
struct TxOutput {
    address: String,
    amount: u64,
}
