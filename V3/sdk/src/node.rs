//! TCP newline-delimited JSON-RPC to ZION core `node` (same wire format as `zion-cli`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::config::NodeClientConfig;
use crate::error::{Result, RpcErrorBody, ZionSdkError};
use crate::types::{
    ChainInfo, MempoolInfo, NodeInfo, PeerInfo, SubmitAccepted, SubmitBlockParams,
    SubmitCandidateResult, SupplyInfo,
};

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const DEFAULT_MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_RETRIES: u32 = 2;
const DEFAULT_RETRY_BACKOFF: Duration = Duration::from_millis(100);

/// Build a [`NodeClient`] with timeouts, limits, and retry policy.
#[derive(Debug, Clone)]
pub struct NodeClientBuilder {
    host: String,
    port: u16,
    connect_timeout: Duration,
    request_timeout: Duration,
    max_line_bytes: usize,
    max_retries: u32,
    retry_initial_backoff: Duration,
}

impl NodeClientBuilder {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_line_bytes: DEFAULT_MAX_LINE_BYTES,
            max_retries: DEFAULT_MAX_RETRIES,
            retry_initial_backoff: DEFAULT_RETRY_BACKOFF,
        }
    }

    /// Configuration from environment (see [`NodeClientConfig`]).
    pub fn from_env() -> Result<Self> {
        let c = NodeClientConfig::from_env()?;
        Ok(Self {
            host: c.host,
            port: c.port,
            connect_timeout: c.connect_timeout,
            request_timeout: c.request_timeout,
            max_line_bytes: c.max_line_bytes,
            max_retries: c.max_retries,
            retry_initial_backoff: c.retry_initial_backoff,
        })
    }

    pub fn connect_timeout(mut self, d: Duration) -> Self {
        self.connect_timeout = d;
        self
    }

    pub fn request_timeout(mut self, d: Duration) -> Self {
        self.request_timeout = d;
        self
    }

    pub fn max_line_bytes(mut self, n: usize) -> Self {
        self.max_line_bytes = n.max(1024);
        self
    }

    /// Number of **retries** after a transient error (0 = single attempt without retry).
    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    pub fn retry_initial_backoff(mut self, d: Duration) -> Self {
        self.retry_initial_backoff = d;
        self
    }

    pub fn build(self) -> NodeClient {
        NodeClient {
            host: self.host,
            port: self.port,
            connect_timeout: self.connect_timeout,
            request_timeout: self.request_timeout,
            max_line_bytes: self.max_line_bytes,
            max_retries: self.max_retries,
            retry_initial_backoff: self.retry_initial_backoff,
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }
}

/// Async JSON-RPC client for ZION core node.
#[derive(Debug, Clone)]
pub struct NodeClient {
    host: String,
    port: u16,
    connect_timeout: Duration,
    request_timeout: Duration,
    max_line_bytes: usize,
    max_retries: u32,
    retry_initial_backoff: Duration,
    next_id: Arc<AtomicU64>,
}

impl NodeClient {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        NodeClientBuilder::new(host, port).build()
    }

    pub fn builder(host: impl Into<String>, port: u16) -> NodeClientBuilder {
        NodeClientBuilder::new(host, port)
    }

    /// Same as [`NodeClientBuilder::from_env`] + [`NodeClientBuilder::build`].
    pub fn from_env() -> Result<Self> {
        NodeClientBuilder::from_env().map(|b| b.build())
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub fn max_retries(&self) -> u32 {
        self.max_retries
    }

    pub fn retry_initial_backoff(&self) -> Duration {
        self.retry_initial_backoff
    }

    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Generic JSON-RPC call (returns the `result` field). On transient errors performs up to `max_retries` retries.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let mut delay = self.retry_initial_backoff;
        let mut attempt: u32 = 0;
        loop {
            match self.call_once(method, &params).await {
                Ok(v) => {
                    #[cfg(feature = "tracing")]
                    tracing::debug!(target: "zion_sdk::rpc", method, attempt, "RPC ok");
                    return Ok(v);
                }
                Err(e) if attempt < self.max_retries && e.is_transient() => {
                    #[cfg(feature = "tracing")]
                    tracing::warn!(
                        target: "zion_sdk::rpc",
                        method,
                        attempt,
                        max_retries = self.max_retries,
                        error = %e,
                        "transient RPC failure, retrying"
                    );
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_secs(5));
                    attempt += 1;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn call_once(&self, method: &str, params: &Value) -> Result<Value> {
        let id = self.next_request_id();
        let addr = format!("{}:{}", self.host, self.port);

        let connect_fut = TcpStream::connect(&addr);
        let stream = tokio::time::timeout(self.connect_timeout, connect_fut)
            .await
            .map_err(|_| ZionSdkError::Timeout {
                phase: "tcp_connect",
                limit_ms: self.connect_timeout.as_millis() as u64,
            })?
            .map_err(ZionSdkError::Io)?;

        let work = self.rpc_roundtrip(stream, method, params, id);
        tokio::time::timeout(self.request_timeout, work)
            .await
            .map_err(|_| ZionSdkError::Timeout {
                phase: "rpc_request",
                limit_ms: self.request_timeout.as_millis() as u64,
            })?
    }

    async fn rpc_roundtrip(
        &self,
        stream: TcpStream,
        method: &str,
        params: &Value,
        id: u64,
    ) -> Result<Value> {
        let (reader, mut writer) = stream.into_split();

        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let mut line = serde_json::to_string(&request)?;
        line.push('\n');
        writer.write_all(line.as_bytes()).await?;
        writer.flush().await?;

        let mut buf_reader = BufReader::new(reader);
        let response_line = self.read_first_non_empty_line(&mut buf_reader).await?;
        Self::parse_envelope(&response_line, id, method)
    }

    async fn read_first_non_empty_line<R: tokio::io::AsyncBufRead + Unpin>(
        &self,
        reader: &mut R,
    ) -> Result<String> {
        let mut buf = Vec::new();
        for _ in 0..32 {
            buf.clear();
            reader.read_until(b'\n', &mut buf).await?;
            if buf.len() > self.max_line_bytes {
                return Err(ZionSdkError::ResponseLineTooLong {
                    max: self.max_line_bytes,
                });
            }
            let line = String::from_utf8_lossy(&buf).trim().to_string();
            if line.is_empty() {
                continue;
            }
            return Ok(line);
        }
        Err(ZionSdkError::EmptyResponseLine)
    }

    pub(crate) fn parse_envelope(line: &str, expected_id: u64, method: &str) -> Result<Value> {
        let v: Value = serde_json::from_str(line).map_err(ZionSdkError::Json)?;

        if let Some(ver) = v.get("jsonrpc").and_then(|x| x.as_str()) {
            if ver != "2.0" {
                return Err(ZionSdkError::InvalidRpcEnvelope {
                    reason: format!("unsupported jsonrpc version {:?}", ver),
                });
            }
        }

        if let Some(id_val) = v.get("id") {
            if !id_val.is_null() {
                let got_u = id_val.as_u64();
                let matches = got_u == Some(expected_id)
                    || id_val == &json!(expected_id)
                    || id_val.as_str().and_then(|s| s.parse::<u64>().ok()) == Some(expected_id);
                if !matches {
                    return Err(ZionSdkError::RpcIdMismatch {
                        expected: expected_id,
                        got: Some(id_val.clone()),
                    });
                }
            }
        }

        if let Some(err) = v.get("error") {
            if !err.is_null() {
                if let Ok(body) = serde_json::from_value::<RpcErrorBody>(err.clone()) {
                    return Err(ZionSdkError::NodeRpc {
                        method: method.to_string(),
                        code: body.code,
                        message: body.message,
                        data: body.data,
                    });
                }
                return Err(ZionSdkError::NodeRpcMalformed {
                    method: method.to_string(),
                    raw: err.clone(),
                });
            }
        }

        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }

    pub async fn call0(&self, method: &str) -> Result<Value> {
        self.call(method, json!({})).await
    }

    pub async fn chain_info(&self) -> Result<ChainInfo> {
        let v = self.call0("getChainInfo").await?;
        serde_json::from_value(v).map_err(|e| ZionSdkError::TypeDecode {
            context: "getChainInfo".to_string(),
            source: e,
        })
    }

    pub async fn node_info(&self) -> Result<NodeInfo> {
        let v = self.call0("getNodeInfo").await?;
        serde_json::from_value(v).map_err(|e| ZionSdkError::TypeDecode {
            context: "getNodeInfo".to_string(),
            source: e,
        })
    }

    pub async fn mempool_info(&self) -> Result<MempoolInfo> {
        let v = self.call0("getMempoolInfo").await?;
        serde_json::from_value(v).map_err(|e| ZionSdkError::TypeDecode {
            context: "getMempoolInfo".to_string(),
            source: e,
        })
    }

    pub async fn peer_info(&self) -> Result<PeerInfo> {
        let v = self.call0("getPeerInfo").await?;
        serde_json::from_value(v).map_err(|e| ZionSdkError::TypeDecode {
            context: "getPeerInfo".to_string(),
            source: e,
        })
    }

    pub async fn supply_info(&self) -> Result<SupplyInfo> {
        let v = self.call0("getSupplyInfo").await?;
        serde_json::from_value(v).map_err(|e| ZionSdkError::TypeDecode {
            context: "getSupplyInfo".to_string(),
            source: e,
        })
    }

    pub async fn block_by_height(&self, height: u64) -> Result<Value> {
        self.call("getBlockByHeight", json!({ "height": height }))
            .await
    }

    pub async fn block_by_hash(&self, hash: &str) -> Result<Value> {
        self.call("getBlock", json!({ "hash": hash })).await
    }

    pub async fn transaction(&self, txid: &str) -> Result<Value> {
        self.call("getTransaction", json!({ "txid": txid })).await
    }

    pub async fn account_transaction(&self, txid: &str) -> Result<Value> {
        self.call("getAccountTransaction", json!({ "txid": txid }))
            .await
    }

    pub async fn block_template(&self) -> Result<Value> {
        self.call0("getBlockTemplate").await
    }

    pub async fn balance(&self, address_or_account: &str) -> Result<Value> {
        self.call(
            "getBalance",
            json!({ "account": address_or_account, "address": address_or_account }),
        )
        .await
    }

    pub async fn balance_at_height(&self, address_or_account: &str, height: u64) -> Result<Value> {
        self.call(
            "getBalanceAtHeight",
            json!({ "account": address_or_account, "height": height }),
        )
        .await
    }

    pub async fn utxos(&self, zion1_address: &str) -> Result<Value> {
        self.call("getUtxos", json!({ "address": zion1_address }))
            .await
    }

    pub async fn bridge_locks(&self, from_height: u64, to_height: Option<u64>) -> Result<Value> {
        let mut p = serde_json::Map::new();
        p.insert("from_height".to_string(), json!(from_height));
        if let Some(h) = to_height {
            p.insert("to_height".to_string(), json!(h));
        }
        self.call("getBridgeLocks", Value::Object(p)).await
    }

    pub async fn bridge_vault_balance(&self) -> Result<Value> {
        self.call0("getBridgeVaultBalance").await
    }

    /// `submitTransaction`, `submitAccountTransaction`, nebo `sendRawTransaction`.
    pub async fn submit_transaction(
        &self,
        method: &str,
        transaction: Value,
    ) -> Result<SubmitAccepted> {
        let v = self
            .call(method, json!({ "transaction": transaction }))
            .await?;
        serde_json::from_value(v).map_err(|e| ZionSdkError::TypeDecode {
            context: method.to_string(),
            source: e,
        })
    }

    pub async fn submit_block(&self, params: &SubmitBlockParams) -> Result<SubmitCandidateResult> {
        let v = self
            .call(
                "submitBlock",
                json!({
                    "template_id": params.template_id,
                    "header_hex": params.header_hex,
                    "nonce": params.nonce,
                    "target_hex": params.target_hex,
                }),
            )
            .await?;
        serde_json::from_value(v).map_err(|e| ZionSdkError::TypeDecode {
            context: "submitBlock".to_string(),
            source: e,
        })
    }

    pub async fn submit_bridge_unlock(&self, params: Value) -> Result<Value> {
        self.call("submitBridgeUnlock", params).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_rpc_addr;

    #[test]
    fn parse_ipv4() {
        let (h, p) = parse_rpc_addr("127.0.0.1:8443").unwrap();
        assert_eq!(h, "127.0.0.1");
        assert_eq!(p, 8443);
    }

    #[test]
    fn parse_socket_addr() {
        let (h, p) = parse_rpc_addr("0.0.0.0:9999").unwrap();
        assert_eq!(h, "0.0.0.0");
        assert_eq!(p, 9999);
    }

    #[test]
    fn builder_defaults() {
        let c = NodeClient::builder("10.0.0.1", 1).build();
        assert_eq!(c.connect_timeout(), DEFAULT_CONNECT_TIMEOUT);
        assert_eq!(c.max_retries(), DEFAULT_MAX_RETRIES);
    }

    #[test]
    fn parse_envelope_rpc_error() {
        let line = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32001,"message":"nope"}}"#;
        let e = NodeClient::parse_envelope(line, 1, "getBlock").unwrap_err();
        match e {
            ZionSdkError::NodeRpc {
                method,
                code,
                message,
                ..
            } => {
                assert_eq!(method, "getBlock");
                assert_eq!(code, -32001);
                assert_eq!(message, "nope");
            }
            _ => panic!("unexpected {e:?}"),
        }
    }
}
