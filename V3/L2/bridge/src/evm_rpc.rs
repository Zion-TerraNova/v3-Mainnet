//! Minimal HTTP JSON-RPC client for EVM chains.
//!
//! Used for direct transaction submission (nonce, gas, sendRawTransaction).
//! Bypasses the Ankr integration (which is for event watching, not TX sending).
//! Converts WebSocket URLs (wss://) to HTTP (https://) automatically.

use crate::ankr::AnkrLog;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use tracing::debug;

// ─────────────────────────────────────────────────────────────────────────────
// EvmHttpClient
// ─────────────────────────────────────────────────────────────────────────────

/// Simple HTTP JSON-RPC client for a single EVM chain.
pub struct EvmHttpClient {
    client: reqwest::Client,
    http_url: String,
}

impl EvmHttpClient {
    /// Create from any URL. Converts wss:// → https:// and ws:// → http://.
    pub fn from_rpc_url(rpc_url: &str) -> Self {
        let http_url = rpc_url
            .replace("wss://", "https://")
            .replace("ws://", "http://");
        Self {
            client: reqwest::Client::new(),
            http_url,
        }
    }

    /// Send a raw JSON-RPC call and return the `result` field.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        debug!("EVM RPC → {} {}", self.http_url, method);

        let response = self
            .client
            .post(&self.http_url)
            .json(&payload)
            .send()
            .await
            .with_context(|| format!("HTTP request to {} failed", self.http_url))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, body));
        }

        let body: Value = response.json().await.context("JSON parse failed")?;

        if let Some(err) = body.get("error") {
            return Err(anyhow!("JSON-RPC error: {}", err));
        }

        Ok(body["result"].clone())
    }

    /// Get transaction count (nonce) for `address`.
    pub async fn get_nonce(&self, address: &str) -> Result<u64> {
        let result = self
            .call("eth_getTransactionCount", json!([address, "pending"]))
            .await?;
        parse_hex_u64(&result)
    }

    /// Get base fee (via eth_gasPrice), in wei.
    pub async fn get_gas_price(&self) -> Result<u64> {
        let result = self.call("eth_gasPrice", json!([])).await?;
        parse_hex_u64(&result)
    }

    /// Get max priority fee (EIP-1559 tip), in wei.
    /// Falls back to 1.5 gwei if the method is not supported.
    pub async fn get_max_priority_fee(&self) -> Result<u64> {
        match self.call("eth_maxPriorityFeePerGas", json!([])).await {
            Ok(result) => Ok(parse_hex_u64(&result).unwrap_or(1_500_000_000)),
            Err(_) => Ok(1_500_000_000), // 1.5 gwei default
        }
    }

    /// Estimate gas for a call.
    pub async fn estimate_gas(&self, from: &str, to: &str, calldata_hex: &str) -> Result<u64> {
        let result = self
            .call(
                "eth_estimateGas",
                json!([{
                    "from": from,
                    "to": to,
                    "data": calldata_hex,
                }]),
            )
            .await?;
        parse_hex_u64(&result)
    }

    /// Get current block number.
    pub async fn block_number(&self) -> Result<u64> {
        let result = self.call("eth_blockNumber", json!([])).await?;
        parse_hex_u64(&result)
    }

    /// Get logs via direct `eth_getLogs` (no Ankr dependency).
    ///
    /// Returns a `Vec<AnkrLog>` — same struct as the Ankr variant since both
    /// are standard Ethereum JSON-RPC log objects.
    pub async fn get_logs(
        &self,
        address: &str,
        from_block: u64,
        to_block: u64,
        topics: &[Option<&str>],
    ) -> Result<Vec<AnkrLog>> {
        let topics_json: Vec<Value> = topics
            .iter()
            .map(|t| match t {
                Some(s) => Value::String(s.to_string()),
                None => Value::Null,
            })
            .collect();

        let result = self
            .call(
                "eth_getLogs",
                json!([{
                    "address": address,
                    "fromBlock": format!("0x{:x}", from_block),
                    "toBlock":   format!("0x{:x}", to_block),
                    "topics":    topics_json,
                }]),
            )
            .await?;

        serde_json::from_value::<Vec<AnkrLog>>(result)
            .context("Failed to parse eth_getLogs response into Vec<AnkrLog>")
    }

    /// Broadcast a signed raw transaction, returning the tx hash.
    pub async fn send_raw_transaction(&self, raw_tx: &str) -> Result<String> {
        let result = self.call("eth_sendRawTransaction", json!([raw_tx])).await?;
        result
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Expected tx hash string, got {:?}", result))
    }

    /// Get transaction receipt (returns None if not mined yet).
    pub async fn get_receipt(&self, tx_hash: &str) -> Result<Option<Value>> {
        let result = self
            .call("eth_getTransactionReceipt", json!([tx_hash]))
            .await?;
        if result.is_null() {
            Ok(None)
        } else {
            Ok(Some(result))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn parse_hex_u64(v: &Value) -> Result<u64> {
    let hex = v
        .as_str()
        .ok_or_else(|| anyhow!("Expected hex string, got {:?}", v))?;
    u64::from_str_radix(hex.trim_start_matches("0x"), 16)
        .with_context(|| format!("Failed to parse hex u64: {}", hex))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wss_to_https() {
        let c = EvmHttpClient::from_rpc_url("wss://base-sepolia.publicnode.com");
        assert_eq!(c.http_url, "https://base-sepolia.publicnode.com");
    }

    #[test]
    fn test_ws_to_http() {
        let c = EvmHttpClient::from_rpc_url("ws://localhost:8545");
        assert_eq!(c.http_url, "http://localhost:8545");
    }

    #[test]
    fn test_https_unchanged() {
        let c = EvmHttpClient::from_rpc_url("https://base-sepolia.publicnode.com");
        assert_eq!(c.http_url, "https://base-sepolia.publicnode.com");
    }
}
