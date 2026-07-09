//! Ankr multi-chain RPC client.
//!
//! Ankr provides a unified HTTP JSON-RPC endpoint for all major EVM chains.
//! Instead of maintaining per-chain WebSocket connections (and the heavy `ethers`
//! dependency), we use Ankr's simple endpoint pattern:
//!
//! ```text
//! https://rpc.ankr.com/{chain}             ← free tier (rate-limited)
//! https://rpc.ankr.com/{chain}/{api_key}   ← premium (higher limits)
//! ```
//!
//! ## Supported chains
//!
//! | Chain     | Ankr slug   | EVM Chain ID |
//! |-----------|-------------|-------------|
//! | Ethereum  | `eth`       | 1           |
//! | Base      | `base`      | 8453        |
//! | Arbitrum  | `arbitrum`  | 42161       |
//! | BSC       | `bsc`       | 56          |
//! | Polygon   | `polygon`   | 137         |
//!
//! ## Simplification vs old approach
//!
//! Before: each EVM chain needed its own WebSocket RPC URL + ethers WebSocket provider.
//! Now:    one `AnkrClient` handles all chains via HTTP polling — no WebSocket, no `abigen!`,
//!         no per-chain URL configuration.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use tracing::{debug, warn};

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Base URL for all Ankr RPC endpoints.
pub const ANKR_BASE_URL: &str = "https://rpc.ankr.com";

/// keccak256("BridgeBurn(address,uint256,string,bytes32,uint256)")
/// Pre-computed event topic hash for the wZION BridgeBurn log filter.
pub const BRIDGE_BURN_TOPIC: &str =
    "0x179dc3b748531271bc8b650b06312455d746350b674ae8d67d0f8b0ecf1212fb";

/// Maximum block range per eth_getLogs call (Ankr free tier limit).
pub const MAX_LOG_BLOCK_RANGE: u64 = 3_000;

// ─────────────────────────────────────────────────────────────────────────────
// Data types
// ─────────────────────────────────────────────────────────────────────────────

/// A log entry returned by `eth_getLogs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnkrLog {
    /// Contract address that emitted the log.
    pub address: String,

    /// Log topics (indexed fields). topic[0] = event signature hash.
    pub topics: Vec<String>,

    /// Non-indexed log data (ABI-encoded).
    pub data: String,

    /// Hex-encoded block number (e.g. `"0x1234"`).
    #[serde(rename = "blockNumber")]
    pub block_number: String,

    /// Transaction hash.
    #[serde(rename = "transactionHash")]
    pub transaction_hash: Option<String>,

    /// Block hash.
    #[serde(rename = "blockHash")]
    pub block_hash: Option<String>,

    /// Log index within the block.
    #[serde(rename = "logIndex")]
    pub log_index: Option<String>,

    /// Whether the log was removed due to chain reorganisation.
    pub removed: Option<bool>,
}

impl AnkrLog {
    /// Parse block number from hex string (e.g. `"0x1234"` → `4660`).
    pub fn block_number_u64(&self) -> u64 {
        u64::from_str_radix(self.block_number.trim_start_matches("0x"), 16).unwrap_or(0)
    }

    /// Decode the log's hex `data` field into bytes.
    pub fn data_bytes(&self) -> Result<Vec<u8>> {
        hex::decode(self.data.trim_start_matches("0x"))
            .context("Failed to decode AnkrLog data as hex")
    }
}

/// Parameters for an `eth_getLogs` request.
#[derive(Debug, Clone)]
pub struct LogFilter {
    /// Earliest block to scan (inclusive).
    pub from_block: u64,

    /// Latest block to scan (inclusive).
    pub to_block: u64,

    /// Contract address to filter by.
    pub address: String,

    /// Topics to filter by (topic[0] = event signature hash).
    pub topics: Vec<Option<String>>,
}

/// Transaction receipt returned by `eth_getTransactionReceipt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxReceipt {
    /// `"0x1"` = success, `"0x0"` = reverted.
    pub status: Option<String>,

    /// Transaction hash.
    #[serde(rename = "transactionHash")]
    pub transaction_hash: String,

    /// Gas used (hex).
    #[serde(rename = "gasUsed")]
    pub gas_used: Option<String>,

    /// Block number (hex).
    #[serde(rename = "blockNumber")]
    pub block_number: Option<String>,
}

impl TxReceipt {
    /// Returns `true` if the transaction succeeded (`status == "0x1"`).
    pub fn is_success(&self) -> bool {
        self.status.as_deref() == Some("0x1")
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AnkrClient
// ─────────────────────────────────────────────────────────────────────────────

/// HTTP JSON-RPC client for Ankr multi-chain endpoints.
///
/// One client handles all chains. All calls simply pass the chain name as part
/// of the URL — no per-chain connection management needed.
///
/// # Example
///
/// ```ignore
/// let client = AnkrClient::new(None);                       // free tier
/// let client = AnkrClient::new(Some("key".into()));         // premium
/// let client = AnkrClient::from_env();                      // reads ANKR_API_KEY
/// ```
pub struct AnkrClient {
    /// Optional Ankr API key (enables premium endpoints with higher rate limits).
    api_key: Option<String>,

    /// Underlying reqwest HTTP client (keep-alive pool).
    client: reqwest::Client,
}

impl AnkrClient {
    /// Create a new client with an optional API key.
    ///
    /// - `None` → free tier endpoints (`https://rpc.ankr.com/{chain}`)
    /// - `Some(key)` → premium endpoints (`https://rpc.ankr.com/{chain}/{key}`)
    pub fn new(api_key: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("zion-bridge/2.9.6")
            .build()
            .expect("Failed to build reqwest client for Ankr");

        Self { api_key, client }
    }

    /// Create a client by reading the `ANKR_API_KEY` environment variable.
    ///
    /// Falls back to the free tier if the variable is not set.
    pub fn from_env() -> Self {
        let api_key = std::env::var("ANKR_API_KEY").ok().filter(|k| !k.is_empty());
        if api_key.is_some() {
            debug!("AnkrClient: using API key from ANKR_API_KEY env var");
        } else {
            debug!("AnkrClient: no ANKR_API_KEY set — using free Ankr tier");
        }
        Self::new(api_key)
    }

    /// Build the JSON-RPC endpoint URL for a given chain.
    ///
    /// ```ignore
    /// // Free tier
    /// client.rpc_url("base")   // → "https://rpc.ankr.com/base"
    /// // Premium tier
    /// client.rpc_url("base")   // → "https://rpc.ankr.com/base/your-api-key"
    /// ```
    pub fn rpc_url(&self, chain: &str) -> String {
        match &self.api_key {
            Some(key) => format!("{}/{}/{}", ANKR_BASE_URL, chain, key),
            None => format!("{}/{}", ANKR_BASE_URL, chain),
        }
    }

    /// Send a raw JSON-RPC call to the given chain and return the `result` field.
    ///
    /// Returns `Err` on HTTP failure or JSON-RPC error response.
    pub async fn call_rpc(&self, chain: &str, method: &str, params: Value) -> Result<Value> {
        let url = self.rpc_url(chain);

        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        debug!("Ankr → {} {} {:?}", chain, method, params);

        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .with_context(|| format!("HTTP request to Ankr/{} failed", chain))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Ankr/{} HTTP {}: {}", chain, status, body));
        }

        let body: Value = response
            .json()
            .await
            .context("Failed to parse Ankr JSON-RPC response")?;

        // Check for JSON-RPC error
        if let Some(err) = body.get("error") {
            let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            return Err(anyhow!("Ankr/{} JSON-RPC error {}: {}", chain, code, msg));
        }

        body.get("result")
            .cloned()
            .ok_or_else(|| anyhow!("Ankr/{} response missing 'result' field", chain))
    }

    // ─────────────────────────────────────────────
    // Standard JSON-RPC convenience methods
    // ─────────────────────────────────────────────

    /// `eth_blockNumber` — returns the latest block number for the given chain.
    pub async fn block_number(&self, chain: &str) -> Result<u64> {
        let result = self.call_rpc(chain, "eth_blockNumber", json!([])).await?;

        let hex = result
            .as_str()
            .ok_or_else(|| anyhow!("eth_blockNumber: expected hex string, got {:?}", result))?;

        u64::from_str_radix(hex.trim_start_matches("0x"), 16)
            .with_context(|| format!("eth_blockNumber: invalid hex '{}'", hex))
    }

    /// `eth_getLogs` — fetch logs matching the given filter.
    ///
    /// Automatically chunks into `MAX_LOG_BLOCK_RANGE`-sized ranges to stay
    /// within Ankr's free-tier limit.
    pub async fn get_logs(&self, chain: &str, filter: &LogFilter) -> Result<Vec<AnkrLog>> {
        let mut all_logs: Vec<AnkrLog> = Vec::new();
        let mut chunk_from = filter.from_block;

        while chunk_from <= filter.to_block {
            let chunk_to = (chunk_from + MAX_LOG_BLOCK_RANGE - 1).min(filter.to_block);

            let filter_obj = json!({
                "fromBlock": format!("0x{:x}", chunk_from),
                "toBlock":   format!("0x{:x}", chunk_to),
                "address":   filter.address,
                "topics":    filter.topics,
            });

            debug!(
                "Ankr eth_getLogs {}: blocks {} → {} (chunk {}/{})",
                chain,
                chunk_from,
                chunk_to,
                chunk_from / MAX_LOG_BLOCK_RANGE + 1,
                (filter.to_block - filter.from_block) / MAX_LOG_BLOCK_RANGE + 1,
            );

            let result = self
                .call_rpc(chain, "eth_getLogs", json!([filter_obj]))
                .await
                .with_context(|| {
                    format!(
                        "eth_getLogs failed for {}:{}-{}",
                        chain, chunk_from, chunk_to
                    )
                })?;

            let chunk_logs: Vec<AnkrLog> = serde_json::from_value(result)
                .context("Failed to deserialize eth_getLogs result")?;

            all_logs.extend(chunk_logs);
            chunk_from = chunk_to + 1;
        }

        Ok(all_logs)
    }

    /// `eth_sendRawTransaction` — broadcast a signed raw transaction.
    ///
    /// Returns the transaction hash on success.
    pub async fn send_raw_transaction(&self, chain: &str, raw_tx: &str) -> Result<String> {
        let result = self
            .call_rpc(chain, "eth_sendRawTransaction", json!([raw_tx]))
            .await?;

        result.as_str().map(|s| s.to_string()).ok_or_else(|| {
            anyhow!(
                "eth_sendRawTransaction: expected hash string, got {:?}",
                result
            )
        })
    }

    /// `eth_getTransactionReceipt` — fetch a transaction receipt by hash.
    ///
    /// Returns `None` if the transaction is not yet mined.
    pub async fn get_transaction_receipt(
        &self,
        chain: &str,
        tx_hash: &str,
    ) -> Result<Option<TxReceipt>> {
        let result = self
            .call_rpc(chain, "eth_getTransactionReceipt", json!([tx_hash]))
            .await?;

        if result.is_null() {
            return Ok(None);
        }

        let receipt: TxReceipt =
            serde_json::from_value(result).context("Failed to deserialize TxReceipt")?;
        Ok(Some(receipt))
    }

    /// `eth_call` — call a contract read function (no transaction).
    ///
    /// `data` is the ABI-encoded calldata (hex, with or without `0x` prefix).
    /// Returns the raw hex result.
    pub async fn eth_call(&self, chain: &str, to: &str, data: &str) -> Result<String> {
        let call_obj = json!({
            "to":   to,
            "data": data,
        });

        let result = self
            .call_rpc(chain, "eth_call", json!([call_obj, "latest"]))
            .await?;

        result
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("eth_call: expected hex string, got {:?}", result))
    }

    /// `eth_getBalance` — get account ETH balance as a hex Wei value.
    pub async fn get_balance(&self, chain: &str, address: &str) -> Result<String> {
        let result = self
            .call_rpc(chain, "eth_getBalance", json!([address, "latest"]))
            .await?;

        result
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("eth_getBalance: expected hex string, got {:?}", result))
    }

    // ─────────────────────────────────────────────
    // Health / connectivity
    // ─────────────────────────────────────────────

    /// Check connectivity to a chain's Ankr endpoint.
    ///
    /// Returns `Ok(true)` if we can successfully call `eth_blockNumber`.
    pub async fn health_check(&self, chain: &str) -> Result<bool> {
        match self.block_number(chain).await {
            Ok(block) => {
                debug!("Ankr health OK — {} @ block {}", chain, block);
                Ok(true)
            }
            Err(e) => {
                warn!("Ankr health FAIL — {}: {}", chain, e);
                Ok(false)
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Compute keccak256 of a byte slice, returned as `0x`-prefixed hex string.
///
/// Used to compute canonical EVM event topic hashes from their signature string.
///
/// ```
/// use zion_bridge::ankr::keccak256_topic;
///
/// // BridgeBurn event topic
/// let topic = keccak256_topic(b"BridgeBurn(address,uint256,string,bytes32,uint256)");
/// assert!(topic.starts_with("0x"));
/// assert_eq!(topic.len(), 66); // "0x" + 32 bytes * 2
/// ```
pub fn keccak256_topic(input: &[u8]) -> String {
    let mut hasher = Keccak256::new();
    hasher.update(input);
    let result = hasher.finalize();
    format!("0x{}", hex::encode(result))
}

/// Parse a `0x`-prefixed hex block number string into `u64`.
pub fn parse_hex_u64(hex: &str) -> Result<u64> {
    u64::from_str_radix(hex.trim_start_matches("0x"), 16)
        .with_context(|| format!("Invalid hex number: '{}'", hex))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── URL building ────────────────────────────────────────────────────────

    #[test]
    fn test_ankr_url_free_tier() {
        let client = AnkrClient::new(None);
        assert_eq!(client.rpc_url("base"), "https://rpc.ankr.com/base");
        assert_eq!(client.rpc_url("arbitrum"), "https://rpc.ankr.com/arbitrum");
        assert_eq!(client.rpc_url("eth"), "https://rpc.ankr.com/eth");
        assert_eq!(client.rpc_url("bsc"), "https://rpc.ankr.com/bsc");
        assert_eq!(client.rpc_url("polygon"), "https://rpc.ankr.com/polygon");
    }

    #[test]
    fn test_ankr_url_premium_tier() {
        let client = AnkrClient::new(Some("test_api_key_123".into()));
        assert_eq!(
            client.rpc_url("base"),
            "https://rpc.ankr.com/base/test_api_key_123"
        );
        assert_eq!(
            client.rpc_url("arbitrum"),
            "https://rpc.ankr.com/arbitrum/test_api_key_123"
        );
    }

    #[test]
    fn test_ankr_url_empty_key_treated_as_premium() {
        // Empty string API key — still formats as premium URL pattern
        let client = AnkrClient::new(Some("".into()));
        assert_eq!(client.rpc_url("base"), "https://rpc.ankr.com/base/");
    }

    // Mutex for tests that mutate the ANKR_API_KEY environment variable.
    // Without this, parallel test execution causes race conditions.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_ankr_from_env_without_key() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        // Without ANKR_API_KEY set, should build free-tier client
        std::env::remove_var("ANKR_API_KEY");
        let client = AnkrClient::from_env();
        assert_eq!(client.rpc_url("base"), "https://rpc.ankr.com/base");
    }

    #[test]
    fn test_ankr_from_env_with_key() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("ANKR_API_KEY", "env_key_abc");
        let client = AnkrClient::from_env();
        assert_eq!(
            client.rpc_url("base"),
            "https://rpc.ankr.com/base/env_key_abc"
        );
        std::env::remove_var("ANKR_API_KEY");
    }

    // ── keccak256 topic helpers ─────────────────────────────────────────────

    #[test]
    fn test_keccak256_topic_bridge_burn() {
        let topic = keccak256_topic(b"BridgeBurn(address,uint256,string,bytes32,uint256)");
        assert!(topic.starts_with("0x"), "Topic must start with 0x");
        assert_eq!(
            topic.len(),
            66,
            "Topic must be 0x + 32 hex bytes = 66 chars"
        );
        // Verify against the known constant
        assert_eq!(
            topic, BRIDGE_BURN_TOPIC,
            "BridgeBurn topic mismatch — update BRIDGE_BURN_TOPIC constant!"
        );
    }

    #[test]
    fn test_keccak256_topic_transfer() {
        // ERC-20 Transfer event — well-known topic for cross-check
        let topic = keccak256_topic(b"Transfer(address,address,uint256)");
        assert_eq!(
            topic,
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
        );
    }

    #[test]
    fn test_keccak256_topic_approval() {
        // ERC-20 Approval event — another well-known topic
        let topic = keccak256_topic(b"Approval(address,address,uint256)");
        assert_eq!(
            topic,
            "0x8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925"
        );
    }

    // ── parse_hex_u64 ───────────────────────────────────────────────────────

    #[test]
    fn test_parse_hex_u64_basic() {
        assert_eq!(parse_hex_u64("0x0").unwrap(), 0);
        assert_eq!(parse_hex_u64("0x1").unwrap(), 1);
        assert_eq!(parse_hex_u64("0xff").unwrap(), 255);
        assert_eq!(parse_hex_u64("0x100").unwrap(), 256);
        assert_eq!(parse_hex_u64("0x1234").unwrap(), 0x1234);
    }

    #[test]
    fn test_parse_hex_u64_without_prefix() {
        // Works even without 0x
        assert_eq!(parse_hex_u64("ff").unwrap(), 255);
    }

    #[test]
    fn test_parse_hex_u64_invalid() {
        assert!(parse_hex_u64("0xzzzz").is_err());
        assert!(parse_hex_u64("not_hex").is_err());
    }

    // ── AnkrLog helpers ─────────────────────────────────────────────────────

    #[test]
    fn test_ankr_log_block_number_u64() {
        let log = AnkrLog {
            address: "0xabc".into(),
            topics: vec![],
            data: "0x".into(),
            block_number: "0x1234".into(),
            transaction_hash: None,
            block_hash: None,
            log_index: None,
            removed: None,
        };
        assert_eq!(log.block_number_u64(), 0x1234);
    }

    #[test]
    fn test_ankr_log_data_bytes_empty() {
        let log = AnkrLog {
            address: "0xabc".into(),
            topics: vec![],
            data: "0x".into(),
            block_number: "0x0".into(),
            transaction_hash: None,
            block_hash: None,
            log_index: None,
            removed: None,
        };
        let bytes = log.data_bytes().unwrap();
        assert!(bytes.is_empty());
    }

    #[test]
    fn test_ankr_log_data_bytes_decodes() {
        let log = AnkrLog {
            address: "0xabc".into(),
            topics: vec![],
            data: "0xdeadbeef".into(),
            block_number: "0x0".into(),
            transaction_hash: None,
            block_hash: None,
            log_index: None,
            removed: None,
        };
        let bytes = log.data_bytes().unwrap();
        assert_eq!(bytes, vec![0xde, 0xad, 0xbe, 0xef]);
    }

    // ── TxReceipt helpers ───────────────────────────────────────────────────

    #[test]
    fn test_tx_receipt_is_success_true() {
        let r = TxReceipt {
            status: Some("0x1".into()),
            transaction_hash: "0xabc".into(),
            gas_used: None,
            block_number: None,
        };
        assert!(r.is_success());
    }

    #[test]
    fn test_tx_receipt_is_success_false_reverted() {
        let r = TxReceipt {
            status: Some("0x0".into()),
            transaction_hash: "0xabc".into(),
            gas_used: None,
            block_number: None,
        };
        assert!(!r.is_success());
    }

    #[test]
    fn test_tx_receipt_is_success_false_missing() {
        let r = TxReceipt {
            status: None,
            transaction_hash: "0xabc".into(),
            gas_used: None,
            block_number: None,
        };
        assert!(!r.is_success());
    }

    // ── LogFilter / chunk behaviour ─────────────────────────────────────────

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_log_filter_chunk_size_constant() {
        // Ensure free-tier chunk is ≤ Ankr's stated limit of 3500 blocks
        assert!(
            MAX_LOG_BLOCK_RANGE <= 3_500,
            "Chunk size {} exceeds Ankr free-tier limit of 3500",
            MAX_LOG_BLOCK_RANGE
        );
    }
}
