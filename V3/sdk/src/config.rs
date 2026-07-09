//! Configuration from environment — suitable for containers and systemd services.
//!
//! | Variable | Meaning | Default |
//! |----------|---------|---------|
//! | `ZION_RPC_ADDR` | `host:port` or socket address | `127.0.0.1:8443` |
//! | `ZION_RPC_HOST` | host (used with `ZION_RPC_PORT` if `ZION_RPC_ADDR` is not set) | — |
//! | `ZION_RPC_PORT` | port | `8443` |
//! | `ZION_RPC_CONNECT_TIMEOUT_MS` | TCP connect timeout | `15000` |
//! | `ZION_RPC_REQUEST_TIMEOUT_MS` | write + read response | `90000` |
//! | `ZION_RPC_MAX_LINE_BYTES` | max response line length | `16777216` (16 MiB) |
//! | `ZION_RPC_MAX_RETRIES` | retries after **transient** error (0 = disabled) | `2` |
//! | `ZION_RPC_RETRY_BACKOFF_MS` | initial backoff before the first retry | `100` |

use std::time::Duration;

use crate::error::{Result, ZionSdkError};

/// Parse `host:port` or [`std::net::SocketAddr`] (e.g. `127.0.0.1:8443`).
pub fn parse_rpc_addr(raw: &str) -> Result<(String, u16)> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(ZionSdkError::InvalidRpcAddr(raw.to_string()));
    }
    if let Ok(addr) = s.parse::<std::net::SocketAddr>() {
        return Ok((addr.ip().to_string(), addr.port()));
    }
    let (host, port_s) = s
        .rsplit_once(':')
        .ok_or_else(|| ZionSdkError::InvalidRpcAddr(s.to_string()))?;
    if host.is_empty() {
        return Err(ZionSdkError::InvalidRpcAddr(s.to_string()));
    }
    let port: u16 = port_s
        .parse()
        .map_err(|_| ZionSdkError::InvalidRpcAddr(s.to_string()))?;
    Ok((host.to_string(), port))
}

/// Complete node client configuration from env.
#[derive(Debug, Clone)]
pub struct NodeClientConfig {
    pub host: String,
    pub port: u16,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub max_line_bytes: usize,
    pub max_retries: u32,
    pub retry_initial_backoff: Duration,
}

fn parse_u64_env(key: &'static str, default: u64) -> Result<u64> {
    match std::env::var(key) {
        Ok(s) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(default);
            }
            t.parse::<u64>().map_err(|e| ZionSdkError::InvalidEnv {
                key,
                reason: format!("{e}"),
            })
        }
        Err(_) => Ok(default),
    }
}

fn parse_usize_env(key: &'static str, default: usize) -> Result<usize> {
    let v = parse_u64_env(key, default as u64)?;
    Ok(v as usize)
}

impl NodeClientConfig {
    /// Loads configuration from the process environment (see table in module).
    pub fn from_env() -> Result<Self> {
        let (host, port) = if let Ok(addr) = std::env::var("ZION_RPC_ADDR") {
            parse_rpc_addr(&addr)?
        } else {
            let host = std::env::var("ZION_RPC_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
            let port = parse_u64_env("ZION_RPC_PORT", 8443)? as u16;
            (host, port)
        };

        let connect_timeout =
            Duration::from_millis(parse_u64_env("ZION_RPC_CONNECT_TIMEOUT_MS", 15_000)?);
        let request_timeout =
            Duration::from_millis(parse_u64_env("ZION_RPC_REQUEST_TIMEOUT_MS", 90_000)?);
        let max_line_bytes = parse_usize_env("ZION_RPC_MAX_LINE_BYTES", 16 * 1024 * 1024)?;
        let max_retries = parse_u64_env("ZION_RPC_MAX_RETRIES", 2)? as u32;
        let retry_initial_backoff =
            Duration::from_millis(parse_u64_env("ZION_RPC_RETRY_BACKOFF_MS", 100)?);

        Ok(Self {
            host,
            port,
            connect_timeout,
            request_timeout,
            max_line_bytes,
            max_retries,
            retry_initial_backoff,
        })
    }
}
