//! EVM Chain Watcher — polls for wZION BridgeBurn events.
//!
//! Uses the chain’s configured `rpc_url` (e.g. `https://base-sepolia.publicnode.com`)
//! as the **primary** RPC and falls back to Ankr if the primary is unavailable.
//! `wss://` URLs are automatically converted to `https://`.
//!
//! ## Auto-reconnect
//!
//! The watcher retries on any RPC failure using exponential backoff:
//! 5 s → 10 s → 20 s → 40 s → 80 s (max 5 retries, then returns Err).
//! On a successful poll, the retry counter resets.

use crate::ankr::{AnkrClient, AnkrLog, LogFilter, BRIDGE_BURN_TOPIC};
use crate::config::{AnkrConfig, EvmChainConfig};
use crate::evm_rpc::EvmHttpClient;
use crate::types::{BridgeStatus, EvmBurnEvent};
use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Maximum reconnect attempts before giving up.
const MAX_RETRIES: u32 = 5;

/// Base backoff in seconds — doubles each attempt (5→10→20→40→80 s).
const BACKOFF_BASE_SECS: u64 = 5;

/// Poll interval between `eth_getLogs` calls.
const POLL_INTERVAL_SECS: u64 = 12;

/// Maximum block range per `eth_getLogs` chunk.
/// Base public RPC limit is ~2000 blocks but returns errors at exactly 2000;
/// Ankr free-tier is 3500. We use 1500 to stay safely under all known limits.
const MAX_BLOCK_RANGE: u64 = 1_500;

// ─────────────────────────────────────────────────────────────────────────────
// EvmWatcher
// ─────────────────────────────────────────────────────────────────────────────

/// EVM watcher for a single chain.
///
/// Polls `eth_getLogs` via direct HTTP RPC (primary) with Ankr as fallback.
pub struct EvmWatcher {
    config: EvmChainConfig,
    ankr_config: AnkrConfig,
    last_processed_block: u64,
    /// Direct HTTP client built from `config.rpc_url` (wss:// auto-converted to https://).
    direct_rpc: EvmHttpClient,
}

impl EvmWatcher {
    /// Create a new watcher.
    ///
    /// - `start_block`: Resume scanning from this block (from persisted state or chain config).
    pub fn new(config: EvmChainConfig, ankr_config: AnkrConfig, start_block: Option<u64>) -> Self {
        let last = start_block.or(config.start_block).unwrap_or(0);
        // Build direct HTTP client from chain's configured rpc_url.
        // Falls back to the Ankr URL pattern if rpc_url is not set.
        let direct_rpc_url = config
            .rpc_url
            .clone()
            .unwrap_or_else(|| format!("https://rpc.ankr.com/{}", config.chain_id));
        let direct_rpc = EvmHttpClient::from_rpc_url(&direct_rpc_url);
        Self {
            config,
            ankr_config,
            last_processed_block: last,
            direct_rpc,
        }
    }

    /// Start the EVM watcher loop with auto-reconnect.
    ///
    /// Sends confirmed burn events to `burn_tx`.  Returns `Err` only after
    /// exhausting all retries (the bridge daemon should then restart this task).
    pub async fn run(
        &mut self,
        burn_tx: mpsc::Sender<EvmBurnEvent>,
        metrics: Arc<crate::metrics::BridgeMetrics>,
    ) -> Result<()> {
        info!(
            "👁️  EVM Watcher started — chain: {} (EVM ID {}), wZION: {}, finality: {} blocks",
            self.config.name,
            self.config.evm_chain_id,
            self.config.wzion_address,
            self.config.finality_blocks,
        );

        let direct_rpc_url = self.config.rpc_url.as_deref().unwrap_or("(ankr fallback)");
        info!(
            "   Primary RPC: {} | Ankr fallback: {}",
            direct_rpc_url, self.ankr_config.enabled
        );

        let ankr = AnkrClient::new(self.ankr_config.effective_api_key());
        let mut retry_count = 0u32;

        loop {
            match self.poll_loop(&ankr, &burn_tx, &metrics).await {
                Ok(()) => {
                    info!("[{}] Watcher loop exited cleanly", self.config.name);
                    return Ok(());
                }
                Err(e) => {
                    retry_count += 1;
                    if retry_count > MAX_RETRIES {
                        error!(
                            "[{}] EVM Watcher exceeded {} retries. Last error: {}",
                            self.config.name, MAX_RETRIES, e
                        );
                        return Err(e);
                    }

                    let backoff_secs = BACKOFF_BASE_SECS * (1 << (retry_count - 1).min(6));
                    warn!(
                        "[{}] EVM Watcher error (attempt {}/{}): {} — retry in {}s",
                        self.config.name, retry_count, MAX_RETRIES, e, backoff_secs
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                }
            }
        }
    }

    /// Inner polling loop — returns Err on connectivity issues (triggers retry).
    async fn poll_loop(
        &mut self,
        ankr: &AnkrClient,
        burn_tx: &mpsc::Sender<EvmBurnEvent>,
        metrics: &Arc<crate::metrics::BridgeMetrics>,
    ) -> Result<()> {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(POLL_INTERVAL_SECS));
        let mut consecutive_errors = 0u32;

        loop {
            interval.tick().await;

            match self.poll_burns(ankr, burn_tx).await {
                Ok(count) => {
                    consecutive_errors = 0;
                    self.update_metrics(metrics);
                    if count > 0 {
                        info!("[{}] Processed {} burn event(s)", self.config.name, count);
                    }
                }
                Err(e) => {
                    consecutive_errors += 1;
                    warn!(
                        "[{}] Poll error #{}: {}",
                        self.config.name, consecutive_errors, e
                    );
                    if consecutive_errors >= 3 {
                        return Err(anyhow::anyhow!(
                            "3 consecutive poll errors on {}: {}",
                            self.config.name,
                            e
                        ));
                    }
                }
            }
        }
    }

    /// Fetch new burn logs from `last_processed_block + 1` to `finalized_block`.
    ///
    /// Tries `self.direct_rpc` (chain's configured `rpc_url`) first.
    /// Falls back to Ankr on any error.
    /// Processes in chunks of `MAX_BLOCK_RANGE` to respect RPC limits (Ankr ≤ 3500).
    async fn poll_burns(
        &mut self,
        ankr: &AnkrClient,
        burn_tx: &mpsc::Sender<EvmBurnEvent>,
    ) -> Result<usize> {
        // ── Block number: direct RPC → Ankr fallback ──────────────────────
        let current_block = match self.direct_rpc.block_number().await {
            Ok(n) => n,
            Err(e) => {
                debug!(
                    "[{}] Direct RPC block_number failed ({}) — trying Ankr",
                    self.config.name, e
                );
                ankr.block_number(&self.config.chain_id).await?
            }
        };
        let finalized_block = current_block.saturating_sub(self.config.finality_blocks);

        if finalized_block <= self.last_processed_block {
            debug!(
                "[{}] No new finalized blocks (last: {}, finalized: {})",
                self.config.name, self.last_processed_block, finalized_block
            );
            return Ok(0);
        }

        let mut from_block = self.last_processed_block + 1;
        let mut total_events = 0usize;

        // Chunk scan to stay within RPC block-range limits.
        while from_block <= finalized_block {
            let to_block = finalized_block.min(from_block + MAX_BLOCK_RANGE - 1);

            debug!(
                "[{}] Scanning blocks {} → {} (current: {}, finalized: {})",
                self.config.name, from_block, to_block, current_block, finalized_block
            );

            // ── eth_getLogs: direct RPC → Ankr fallback ───────────────────────
            let topics_ref: Vec<Option<&str>> = vec![Some(BRIDGE_BURN_TOPIC)];
            let logs: Vec<AnkrLog> = match self
                .direct_rpc
                .get_logs(
                    &self.config.wzion_address,
                    from_block,
                    to_block,
                    &topics_ref,
                )
                .await
            {
                Ok(l) => l,
                Err(e) => {
                    debug!(
                        "[{}] Direct RPC get_logs failed ({}) — trying Ankr",
                        self.config.name, e
                    );
                    let filter = LogFilter {
                        from_block,
                        to_block,
                        address: self.config.wzion_address.clone(),
                        topics: vec![Some(BRIDGE_BURN_TOPIC.to_string())],
                    };
                    ankr.get_logs(&self.config.chain_id, &filter).await?
                }
            };

            let count = logs.len();
            total_events += count;

            // M1: track reorgs. If any removed log is seen in this chunk,
            // do NOT advance the cursor past it — stop and re-scan next
            // iteration so the relayer does not build on inconsistent state.
            let mut saw_reorg = false;

            for log in logs {
                // Skip removed logs (chain reorg)
                if log.removed == Some(true) {
                    saw_reorg = true;
                    error!(
                        "[{}] Reorg detected: removed log in block {} — pausing cursor advance (M1)",
                        self.config.name,
                        log.block_number_u64()
                    );
                    continue;
                }

                match parse_bridge_burn_log(&log, &self.config.chain_id) {
                    Ok(burn) => {
                        info!(
                            "🔥 BridgeBurn on {}: {} wZION → {} (burn_id: {})",
                            self.config.name,
                            burn.amount_wzion_wei,
                            burn.l1_recipient,
                            burn.burn_id,
                        );
                        if let Err(e) = burn_tx.send(burn).await {
                            error!(
                                "[{}] Failed to send burn event to channel: {:?}",
                                self.config.name, e
                            );
                        }
                    }
                    Err(e) => {
                        warn!(
                            "[{}] Failed to parse BridgeBurn log: {}",
                            self.config.name, e
                        );
                    }
                }
            }

            // M1: only advance the cursor when no reorg was observed in
            // this chunk. If a reorg happened, keep last_processed_block
            // unchanged so the next poll re-scans the same range after the
            // chain has settled.
            if !saw_reorg {
                self.last_processed_block = to_block;
                from_block = to_block + 1;
            } else {
                // Stop scanning further chunks this iteration; the next
                // poll will re-fetch from the pre-reorg cursor.
                break;
            }
        }

        Ok(total_events)
    }

    /// Update the shared metrics gauge with the latest processed EVM block.
    pub fn update_metrics(&self, metrics: &crate::metrics::BridgeMetrics) {
        metrics.last_evm_block.store(
            self.last_processed_block,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Log parsing helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Parse an `AnkrLog` into an `EvmBurnEvent`.
///
/// ABI encoding for `BridgeBurn(address indexed from, uint256 amount,
/// string l1Recipient, bytes32 indexed burnId, uint256 timestamp)`:
///
/// - `topics[0]` = event signature hash (always BRIDGE_BURN_TOPIC)
/// - `topics[1]` = `from` address (zero-padded to 32 bytes)
/// - `topics[2]` = `burnId` (bytes32)
/// - `data[0..32]`  = `amount` (uint256, big-endian)
/// - `data[32..64]` = offset of `l1Recipient` string (= 96 = 0x60)
/// - `data[64..96]` = `timestamp` (uint256, unused)
/// - `data[96..128]` = `l1Recipient` string length
/// - `data[128..128+len]` = `l1Recipient` UTF-8 bytes
fn parse_bridge_burn_log(log: &AnkrLog, chain_id: &str) -> Result<EvmBurnEvent> {
    use anyhow::anyhow;

    let from_topic = log
        .topics
        .get(1)
        .ok_or_else(|| anyhow!("BridgeBurn: missing topics[1] (from address)"))?;

    let burn_id_topic = log
        .topics
        .get(2)
        .ok_or_else(|| anyhow!("BridgeBurn: missing topics[2] (burnId)"))?;

    // Address = last 20 bytes of the 32-byte zero-padded topic (skip first 24 hex chars = 12 bytes)
    let evm_burner = format!(
        "0x{}",
        from_topic
            .trim_start_matches("0x")
            .get(24..)
            .unwrap_or(from_topic.trim_start_matches("0x"))
    );
    let burn_id = burn_id_topic.clone();

    let data = log.data_bytes()?;

    if data.len() < 96 {
        return Err(anyhow!(
            "BridgeBurn: data too short ({} bytes, need ≥ 96)",
            data.len()
        ));
    }

    let amount_str = u256_be_to_decimal(&data[0..32]);
    let string_offset = usize_from_be32(&data[32..64]);

    if data.len() < string_offset + 32 {
        return Err(anyhow!(
            "BridgeBurn: data truncated at string offset {} (have {} bytes)",
            string_offset,
            data.len()
        ));
    }

    let string_len = usize_from_be32(&data[string_offset..string_offset + 32]);
    let string_start = string_offset + 32;
    let string_end = string_start + string_len;

    if data.len() < string_end {
        return Err(anyhow!(
            "BridgeBurn: data truncated reading l1Recipient (need {} bytes, have {})",
            string_end,
            data.len()
        ));
    }

    let l1_recipient = String::from_utf8(data[string_start..string_end].to_vec())
        .map_err(|e| anyhow!("BridgeBurn: invalid UTF-8 in l1Recipient: {}", e))?;

    if !l1_recipient.starts_with("zion1") {
        warn!(
            "BridgeBurn: l1_recipient '{}' does not start with 'zion1'",
            l1_recipient
        );
    }

    let amount_flowers = crate::types::conversion::wzion_wei_to_flowers(&amount_str).unwrap_or(0);

    Ok(EvmBurnEvent {
        evm_tx_hash: log.transaction_hash.clone().unwrap_or_default(),
        evm_block_number: log.block_number_u64(),
        evm_chain: chain_id.to_string(),
        evm_burner,
        amount_wzion_wei: amount_str,
        amount_flowers,
        l1_recipient,
        burn_id,
        detected_at: Utc::now(),
        status: BridgeStatus::Confirmed,
        confirmations: 0,
    })
}

/// Read a usize from the lower bytes of a big-endian slice (≥8 bytes).
fn usize_from_be32(bytes: &[u8]) -> usize {
    if bytes.len() < 8 {
        return 0;
    }
    let offset = bytes.len().saturating_sub(8);
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_be_bytes(arr) as usize
}

/// Convert big-endian uint256 bytes (32 bytes) to decimal string.
///
/// ZION amounts always fit in u128, so we only use the lower 16 bytes.
fn u256_be_to_decimal(bytes: &[u8]) -> String {
    if bytes.len() < 32 {
        return "0".into();
    }
    let hi = &bytes[0..16];
    let lo_bytes: [u8; 16] = bytes[16..32].try_into().unwrap_or([0u8; 16]);
    if hi == [0u8; 16] {
        u128::from_be_bytes(lo_bytes).to_string()
    } else {
        format!("0x{}{}", hex::encode(hi), hex::encode(&lo_bytes as &[u8]))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AnkrConfig;

    fn make_chain() -> EvmChainConfig {
        EvmChainConfig {
            chain_id: "base".into(),
            name: "Base".into(),
            evm_chain_id: 8453,
            rpc_url: None,
            rpc_url_backup: None,
            wzion_address: "0x742d35Cc6634C0532925a3b8D4C9C5B2C39b8F2".into(),
            bridge_contract_address: "0xBridgeContract".into(),
            finality_blocks: 12,
            enabled: true,
            gas_strategy: "eip1559".into(),
            max_gas_gwei: 100,
            start_block: None,
        }
    }

    // ── Constants ────────────────────────────────────────────────────────────

    #[test]
    fn test_max_retries_constant() {
        assert_eq!(MAX_RETRIES, 5);
    }

    #[test]
    fn test_backoff_sequence() {
        let seq: Vec<u64> = (1u32..=5)
            .map(|a| BACKOFF_BASE_SECS * (1 << (a - 1).min(6)))
            .collect();
        assert_eq!(seq, vec![5, 10, 20, 40, 80]);
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_max_block_range_within_ankr_limit() {
        assert!(
            MAX_BLOCK_RANGE <= 3_500,
            "MAX_BLOCK_RANGE {} exceeds Ankr free-tier limit",
            MAX_BLOCK_RANGE
        );
    }

    // ── EvmWatcher::new ──────────────────────────────────────────────────────

    #[test]
    fn test_watcher_new_with_start_block() {
        let watcher = EvmWatcher::new(make_chain(), AnkrConfig::default(), Some(1_234_567));
        assert_eq!(watcher.last_processed_block, 1_234_567);
    }

    #[test]
    fn test_watcher_new_without_start_block() {
        let watcher = EvmWatcher::new(make_chain(), AnkrConfig::default(), None);
        assert_eq!(watcher.last_processed_block, 0);
    }

    #[test]
    fn test_watcher_uses_config_start_block() {
        let mut chain = make_chain();
        chain.start_block = Some(5_000_000);
        let watcher = EvmWatcher::new(chain, AnkrConfig::default(), None);
        assert_eq!(watcher.last_processed_block, 5_000_000);
    }

    // ── u256_be_to_decimal ───────────────────────────────────────────────────

    #[test]
    fn test_u256_be_zero() {
        assert_eq!(u256_be_to_decimal(&[0u8; 32]), "0");
    }

    #[test]
    fn test_u256_be_one() {
        let mut b = [0u8; 32];
        b[31] = 1;
        assert_eq!(u256_be_to_decimal(&b), "1");
    }

    #[test]
    fn test_u256_be_5000_wzion() {
        let amount: u128 = 5_000 * 1_000_000_000_000_000_000u128;
        let mut bytes = [0u8; 32];
        bytes[16..32].copy_from_slice(&amount.to_be_bytes());
        assert_eq!(u256_be_to_decimal(&bytes), amount.to_string());
    }

    // ── usize_from_be32 ──────────────────────────────────────────────────────

    #[test]
    fn test_usize_from_be32_zero() {
        assert_eq!(usize_from_be32(&[0u8; 32]), 0);
    }

    #[test]
    fn test_usize_from_be32_96() {
        let mut b = [0u8; 32];
        b[31] = 96;
        assert_eq!(usize_from_be32(&b), 96);
    }

    // ── parse_bridge_burn_log ────────────────────────────────────────────────

    fn encode_burn_data(amount: u128, l1_recipient: &str, timestamp: u64) -> String {
        let recipient_bytes = l1_recipient.as_bytes();
        let recipient_len = recipient_bytes.len();
        let padded_len = recipient_len.div_ceil(32) * 32;
        let mut data = Vec::<u8>::new();
        // [0..32]: amount
        data.extend_from_slice(&[0u8; 16]);
        data.extend_from_slice(&amount.to_be_bytes());
        // [32..64]: string offset = 96
        data.extend_from_slice(&[0u8; 24]);
        data.extend_from_slice(&96u64.to_be_bytes());
        // [64..96]: timestamp
        data.extend_from_slice(&[0u8; 24]);
        data.extend_from_slice(&timestamp.to_be_bytes());
        // [96..128]: string length
        data.extend_from_slice(&[0u8; 24]);
        data.extend_from_slice(&(recipient_len as u64).to_be_bytes());
        // [128..128+padded]: string bytes
        data.extend_from_slice(recipient_bytes);
        data.extend((0..(padded_len - recipient_len)).map(|_| 0u8));
        format!("0x{}", hex::encode(&data))
    }

    fn make_burn_log(
        from: &str,
        burn_id: &str,
        amount: u128,
        recipient: &str,
        block: u64,
    ) -> AnkrLog {
        AnkrLog {
            address: "0x742d35Cc6634C0532925a3b8D4C9C5B2C39b8F2".into(),
            topics: vec![
                BRIDGE_BURN_TOPIC.to_string(),
                format!("0x{:0>64}", from.trim_start_matches("0x")),
                format!("0x{:0>64}", burn_id.trim_start_matches("0x")),
            ],
            data: encode_burn_data(amount, recipient, 1_700_000_000),
            block_number: format!("0x{:x}", block),
            transaction_hash: Some(format!("0xtxhash{}", block)),
            block_hash: None,
            log_index: None,
            removed: None,
        }
    }

    #[test]
    fn test_parse_burn_log_basic() {
        let amount: u128 = 1_000 * 1_000_000_000_000_000_000u128;
        let log = make_burn_log(
            "DeAdBeEf00000000000000000000000000000001",
            "abcdef00000000000000000000000000000000000000000000000000abcdef01",
            amount,
            "zion1qrecipient000000000000000000000000001",
            123456,
        );
        let burn = parse_bridge_burn_log(&log, "base").unwrap();
        assert_eq!(burn.evm_chain, "base");
        assert_eq!(
            burn.l1_recipient,
            "zion1qrecipient000000000000000000000000001"
        );
        assert_eq!(burn.amount_wzion_wei, amount.to_string());
        assert_eq!(burn.evm_block_number, 123456);
        assert_eq!(burn.status, BridgeStatus::Confirmed);
    }

    #[test]
    fn test_parse_burn_log_short_data_errors() {
        let log = AnkrLog {
            address: "0x...".into(),
            topics: vec![
                BRIDGE_BURN_TOPIC.to_string(),
                "0x0000000000000000000000001234".to_string(),
                "0x0000000000000000000000000000000000000000000000000000000000000001".to_string(),
            ],
            data: "0xdeadbeef".into(),
            block_number: "0x1".into(),
            transaction_hash: None,
            block_hash: None,
            log_index: None,
            removed: None,
        };
        assert!(parse_bridge_burn_log(&log, "base").is_err());
    }

    #[test]
    fn test_parse_burn_log_missing_topics_errors() {
        let log = AnkrLog {
            address: "0x...".into(),
            topics: vec![BRIDGE_BURN_TOPIC.to_string()],
            data: format!("0x{}", "00".repeat(128)),
            block_number: "0x1".into(),
            transaction_hash: None,
            block_hash: None,
            log_index: None,
            removed: None,
        };
        assert!(parse_bridge_burn_log(&log, "base").is_err());
    }
}
