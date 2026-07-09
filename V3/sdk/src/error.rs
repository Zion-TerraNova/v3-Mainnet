use serde_json::Value;
use thiserror::Error;

/// JSON-RPC `error` object from the node response.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RpcErrorBody {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Debug, Error)]
pub enum ZionSdkError {
    #[error("invalid RPC address: {0}")]
    InvalidRpcAddr(String),
    #[error("invalid environment variable {key}: {reason}")]
    InvalidEnv { key: &'static str, reason: String },
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("RPC `{method}` failed [{code}]: {message}")]
    NodeRpc {
        method: String,
        code: i64,
        message: String,
        data: Option<Value>,
    },
    #[error("RPC `{method}` returned non-object error: {raw}")]
    NodeRpcMalformed { method: String, raw: Value },
    #[error("RPC line exceeded max length ({max} bytes)")]
    ResponseLineTooLong { max: usize },
    #[error("unexpected empty RPC response from node")]
    EmptyResponseLine,
    #[error("invalid JSON-RPC response: {reason}")]
    InvalidRpcEnvelope { reason: String },
    #[error("JSON-RPC id mismatch: expected {expected}, got {got:?}")]
    RpcIdMismatch { expected: u64, got: Option<Value> },
    #[error("operation timed out during {phase} (limit {limit_ms} ms)")]
    Timeout { phase: &'static str, limit_ms: u64 },
    #[error("typed decode failed for {context}: {source}")]
    TypeDecode {
        context: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ZionSdkError>;

impl ZionSdkError {
    /// `true` for errors suitable for automatic retry (network outages).
    pub fn is_transient(&self) -> bool {
        match self {
            ZionSdkError::Timeout { .. } => true,
            ZionSdkError::Io(io) => matches!(
                io.kind(),
                std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::Interrupted
            ),
            _ => false,
        }
    }
}
