use anyhow::{Context, Result};
use serde_json::{json, Value};
use zion_sdk::NodeClient;

/// Send one JSON-RPC request over raw TCP to the V3 core node (port 8443).
/// The wire format is: JSON object + newline → JSON object + newline.
///
/// Implemented via [`zion_sdk::NodeClient`] so the same RPC stack ships in the SDK library.
pub async fn call(host: &str, port: u16, method: &str, params: Value) -> Result<Value> {
    NodeClient::new(host, port)
        .call(method, params)
        .await
        .map_err(anyhow::Error::from)
        .with_context(|| format!("Cannot reach node at {}:{}", host, port))
}

/// Convenience: call with empty params
pub async fn call0(host: &str, port: u16, method: &str) -> Result<Value> {
    call(host, port, method, json!({})).await
}
