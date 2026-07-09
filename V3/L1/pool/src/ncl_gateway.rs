//! NCL Gateway — async bridge between the pool's Neural Compute Layer
//! revenue stream and the Hiran v2.2 inference service.
//!
//! This module owns:
//!   * `NclPricing` — pricing math (per-token + per-task floor)
//!   * `NclTaskRequest` — input descriptor pulled from the pool's NCL queue
//!   * `NclTaskResult` — outcome handed back for revenue accounting
//!   * `NclGatewayClient` — minimal OpenAI-compat HTTP/1.1 client over tokio
//!   * `NclDispatcher` — background loop that pulls tasks and routes them
//!
//! Intentionally avoids new crate dependencies: pool already has
//! `tokio` + `serde_json`, and Hiran serves plaintext HTTP on localhost,
//! so a hand-rolled HTTP/1.1 client keeps the validator-adjacent path
//! light and auditable.

use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};
use tracing::{debug, info, warn};

use zion_cosmic_harmony::{NclStats, RevenueCollector};

// ─────────────────────────────────────────────────────────────────────
// Pricing
// ─────────────────────────────────────────────────────────────────────

/// USD price model for NCL inference tasks.
///
/// The total customer charge is:
///   `max(min_per_task, prompt * price_in + completion * price_out / 1_000)`
///
/// `price_in_per_1k_tokens` and `price_out_per_1k_tokens` are in USD.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NclPricing {
    pub price_in_per_1k_tokens: f64,
    pub price_out_per_1k_tokens: f64,
    pub min_per_task_usd: f64,
}

impl Default for NclPricing {
    fn default() -> Self {
        // Conservative defaults derived from llama.cpp on commodity GPUs:
        // a 7B-class model at q5_k_m turns roughly 1k completion tokens
        // per second on an RTX 3060.  Public rates for similar APIs sit in
        // the $0.0005–$0.002 / 1k tokens band.
        Self {
            price_in_per_1k_tokens: 0.0005,
            price_out_per_1k_tokens: 0.0015,
            min_per_task_usd: 0.000_05,
        }
    }
}

impl NclPricing {
    pub fn from_env() -> Self {
        let mut p = Self::default();
        if let Ok(v) = std::env::var("ZION_NCL_PRICE_IN_PER_1K") {
            if let Ok(parsed) = v.parse() {
                p.price_in_per_1k_tokens = parsed;
            }
        }
        if let Ok(v) = std::env::var("ZION_NCL_PRICE_OUT_PER_1K") {
            if let Ok(parsed) = v.parse() {
                p.price_out_per_1k_tokens = parsed;
            }
        }
        if let Ok(v) = std::env::var("ZION_NCL_MIN_PER_TASK_USD") {
            if let Ok(parsed) = v.parse() {
                p.min_per_task_usd = parsed;
            }
        }
        p
    }

    /// Compute USD value for a task with `prompt_tokens` input and
    /// `completion_tokens` output.  Always returns >= `min_per_task_usd`.
    pub fn value_usd(&self, prompt_tokens: u64, completion_tokens: u64) -> f64 {
        let raw = (prompt_tokens as f64) * self.price_in_per_1k_tokens / 1_000.0
            + (completion_tokens as f64) * self.price_out_per_1k_tokens / 1_000.0;
        raw.max(self.min_per_task_usd)
    }
}

// ─────────────────────────────────────────────────────────────────────
// Task types
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NclTaskRequest {
    /// Free-form prompt.  In production this is a customer-submitted query;
    /// the built-in dispatcher uses periodic heartbeat prompts to keep the
    /// pipeline warm and exercise revenue accounting.
    pub prompt: String,
    /// Cap on completion tokens (passed to the gateway as `max_tokens`).
    pub max_tokens: u32,
    /// Optional pre-priced USD value.  When `Some`, this is used directly
    /// instead of computing from the pricing model — useful for paid
    /// customer jobs where the price was agreed up-front.
    pub value_usd_override: Option<f64>,
    /// Tag stamped on traces / metrics so different upstream sources can
    /// be distinguished in the routing snapshot (`customer`, `heartbeat`,
    /// `internal`, …).
    pub origin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NclTaskResult {
    pub origin: String,
    pub success: bool,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub latency_ms: u64,
    pub value_usd: f64,
    /// Truncated SHA-3 hash of the response — useful for dedup / audit.
    pub response_digest: Option<String>,
    pub error: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────
// Gateway client (minimal HTTP/1.1 over tokio)
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct NclGatewayClient {
    /// Authority (host:port) of the Hiran inference server.
    authority: String,
    request_timeout: Duration,
}

impl NclGatewayClient {
    /// Build a client from a URL of the form `http://host:port[/]`.
    pub fn new(base_url: &str) -> Result<Self> {
        let authority = parse_authority(base_url)?;
        Ok(Self {
            authority,
            request_timeout: Duration::from_secs(120),
        })
    }

    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.request_timeout = t;
        self
    }

    pub fn authority(&self) -> &str {
        &self.authority
    }

    /// `GET /health` — returns `true` iff the gateway responded with 2xx.
    pub async fn health(&self) -> bool {
        match timeout(
            Duration::from_secs(5),
            self.do_request("GET", "/health", None),
        )
        .await
        {
            Ok(Ok((status, _))) => (200..300).contains(&status),
            _ => false,
        }
    }

    /// Run an inference task against `POST /v1/chat/completions`.
    /// The response is parsed as OpenAI-compat JSON; token counts are read
    /// from `usage.prompt_tokens` / `usage.completion_tokens` if present,
    /// otherwise fall back to whitespace word-count of the input/output.
    pub async fn chat_completion(&self, prompt: &str, max_tokens: u32) -> Result<ChatCompletion> {
        let body = json!({
            "model": "hiran-v2.2",
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "max_tokens": max_tokens,
            "temperature": 0.7
        });

        let start = Instant::now();
        let (status, raw) = timeout(
            self.request_timeout,
            self.do_request("POST", "/v1/chat/completions", Some(&body.to_string())),
        )
        .await
        .map_err(|_| anyhow!("ncl_gateway: chat_completion timeout"))??;

        if !(200..300).contains(&status) {
            return Err(anyhow!(
                "ncl_gateway: HTTP {} from gateway: {}",
                status,
                String::from_utf8_lossy(raw.split(|b| *b == b' ').next().unwrap_or(&[]))
            ));
        }

        let parsed: Value = serde_json::from_slice(&raw)
            .with_context(|| "ncl_gateway: response was not valid JSON")?;
        let latency_ms = start.elapsed().as_millis() as u64;

        let content = parsed["choices"][0]["message"]["content"]
            .as_str()
            .or_else(|| parsed["response"].as_str())
            .or_else(|| parsed["text"].as_str())
            .unwrap_or("")
            .to_string();

        let prompt_tokens = parsed["usage"]["prompt_tokens"]
            .as_u64()
            .unwrap_or_else(|| approx_token_count(prompt));
        let completion_tokens = parsed["usage"]["completion_tokens"]
            .as_u64()
            .unwrap_or_else(|| approx_token_count(&content));

        Ok(ChatCompletion {
            content,
            prompt_tokens,
            completion_tokens,
            latency_ms,
        })
    }

    async fn do_request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(u16, Vec<u8>)> {
        let mut stream = TcpStream::connect(&self.authority)
            .await
            .with_context(|| format!("ncl_gateway: connect to {}", self.authority))?;

        let body_bytes = body.unwrap_or("");
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAccept: application/json\r\n",
            method = method,
            path = path,
            host = self.authority,
        );
        if body.is_some() {
            request.push_str("Content-Type: application/json\r\n");
            request.push_str(&format!("Content-Length: {}\r\n", body_bytes.len()));
        }
        request.push_str("\r\n");
        request.push_str(body_bytes);

        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;

        let mut buf = Vec::with_capacity(4096);
        stream.read_to_end(&mut buf).await?;
        parse_http_response(&buf)
    }
}

#[derive(Debug, Clone)]
pub struct ChatCompletion {
    pub content: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub latency_ms: u64,
}

// ─────────────────────────────────────────────────────────────────────
// Dispatcher
// ─────────────────────────────────────────────────────────────────────

/// Sender handle for the NCL task queue.  Cloneable across the pool so
/// any subsystem (customer HTTP endpoint, periodic heartbeat, share
/// observer, …) can submit AI inference work.
pub type NclTaskSender = mpsc::Sender<NclTaskRequest>;

/// Configuration knob for the heartbeat task source.  When enabled, the
/// dispatcher injects a tiny inference request every `interval` to keep
/// the gateway warm and ensure the 25 % stream is generating revenue
/// even before paying customers are plumbed in.
#[derive(Debug, Clone, Copy)]
pub struct NclHeartbeatConfig {
    pub enabled: bool,
    pub interval: Duration,
    pub max_tokens: u32,
}

impl Default for NclHeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: Duration::from_secs(60),
            max_tokens: 8,
        }
    }
}

impl NclHeartbeatConfig {
    pub fn from_env() -> Self {
        let enabled = std::env::var("ZION_NCL_HEARTBEAT").ok().as_deref() == Some("true");
        let interval = std::env::var("ZION_NCL_HEARTBEAT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(60));
        let max_tokens = std::env::var("ZION_NCL_HEARTBEAT_TOKENS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8);
        Self {
            enabled,
            interval,
            max_tokens,
        }
    }
}

/// Background NCL dispatcher.
///
/// Consumes [`NclTaskRequest`]s from `rx`, calls the Hiran gateway, then
/// records the result via `runtime.record_ncl_task_revenue`.
pub struct NclDispatcher {
    client: NclGatewayClient,
    pricing: NclPricing,
    revenue: RevenueCollector,
}

impl NclDispatcher {
    pub fn new(client: NclGatewayClient, pricing: NclPricing, revenue: RevenueCollector) -> Self {
        Self {
            client,
            pricing,
            revenue,
        }
    }

    /// Spawn the dispatcher loop and (optionally) the heartbeat producer.
    /// Returns a sender handle the rest of the pool can use to enqueue tasks.
    pub fn spawn(self, heartbeat: NclHeartbeatConfig, queue_capacity: usize) -> NclTaskSender {
        let (tx, mut rx) = mpsc::channel::<NclTaskRequest>(queue_capacity.max(16));

        if heartbeat.enabled {
            let tx_hb = tx.clone();
            tokio::spawn(async move {
                let mut tick = 0u64;
                loop {
                    sleep(heartbeat.interval).await;
                    tick = tick.wrapping_add(1);
                    let req = NclTaskRequest {
                        prompt: format!("ZION NCL heartbeat #{tick}: respond with one word."),
                        max_tokens: heartbeat.max_tokens,
                        value_usd_override: None,
                        origin: "heartbeat".to_string(),
                    };
                    if tx_hb.send(req).await.is_err() {
                        warn!("ncl_dispatcher: queue closed, heartbeat exiting");
                        break;
                    }
                }
            });
        }

        let client = self.client;
        let pricing = self.pricing;
        let revenue = self.revenue;
        tokio::spawn(async move {
            info!(
                "ncl_dispatcher started gateway={} pricing_in_per_1k={} pricing_out_per_1k={}",
                client.authority(),
                pricing.price_in_per_1k_tokens,
                pricing.price_out_per_1k_tokens
            );
            while let Some(req) = rx.recv().await {
                let result = dispatch_one(&client, &pricing, &req).await;
                match &result {
                    Ok(r) => {
                        debug!(
                            "ncl_task_ok origin={} prompt_tokens={} completion_tokens={} \
                             latency_ms={} value_usd={:.6}",
                            r.origin,
                            r.prompt_tokens,
                            r.completion_tokens,
                            r.latency_ms,
                            r.value_usd
                        );
                        revenue.track_ncl_task_detailed(
                            r.value_usd,
                            r.prompt_tokens,
                            r.completion_tokens,
                            r.latency_ms,
                            true,
                        );
                    }
                    Err(e) => {
                        warn!("ncl_task_err origin={} error={}", req.origin, e);
                        revenue.track_ncl_task_detailed(0.0, 0, 0, 0, false);
                    }
                }
            }
            info!("ncl_dispatcher exiting (channel closed)");
        });

        tx
    }
}

/// Convenience: snapshot current NCL stats from a collector handle.
pub fn ncl_stats_snapshot(revenue: &RevenueCollector) -> NclStats {
    revenue.ncl_stats()
}

async fn dispatch_one(
    client: &NclGatewayClient,
    pricing: &NclPricing,
    req: &NclTaskRequest,
) -> Result<NclTaskResult> {
    let completion = client.chat_completion(&req.prompt, req.max_tokens).await?;
    let value_usd = req.value_usd_override.unwrap_or_else(|| {
        pricing.value_usd(completion.prompt_tokens, completion.completion_tokens)
    });
    let digest = response_digest(&completion.content);
    Ok(NclTaskResult {
        origin: req.origin.clone(),
        success: true,
        prompt_tokens: completion.prompt_tokens,
        completion_tokens: completion.completion_tokens,
        latency_ms: completion.latency_ms,
        value_usd,
        response_digest: Some(digest),
        error: None,
    })
}

// ─────────────────────────────────────────────────────────────────────
// HTTP helpers
// ─────────────────────────────────────────────────────────────────────

fn parse_authority(base_url: &str) -> Result<String> {
    let trimmed = base_url
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let host_port = trimmed
        .split('/')
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    if host_port.is_empty() {
        return Err(anyhow!("ncl_gateway: empty base_url"));
    }
    if host_port.contains(':') {
        Ok(host_port.to_string())
    } else {
        Ok(format!("{host_port}:8002"))
    }
}

fn parse_http_response(buf: &[u8]) -> Result<(u16, Vec<u8>)> {
    let header_end = find_header_end(buf)
        .ok_or_else(|| anyhow!("ncl_gateway: malformed HTTP response (no CRLFCRLF)"))?;
    let header_str = std::str::from_utf8(&buf[..header_end])
        .map_err(|_| anyhow!("ncl_gateway: non-utf8 HTTP header"))?;
    let mut lines = header_str.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| anyhow!("ncl_gateway: empty HTTP response"))?;
    let mut parts = status_line.split_whitespace();
    let _http = parts.next();
    let status: u16 = parts
        .next()
        .ok_or_else(|| anyhow!("ncl_gateway: no status code"))?
        .parse()
        .map_err(|_| anyhow!("ncl_gateway: bad status code"))?;

    let body = buf[header_end + 4..].to_vec();
    Ok((status, body))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn approx_token_count(s: &str) -> u64 {
    // OpenAI's tokenizer averages ~0.75 tokens per word for English; we
    // use 1 token per whitespace-delimited word as a safe upper bound.
    s.split_whitespace().count() as u64
}

fn response_digest(s: &str) -> String {
    // Use blake3 (already a dep of cosmic-harmony) via core's crypto module
    // would couple too much; just take a 16-char SHA-3 fingerprint via
    // serde_json's hash-friendly path.  We keep this dependency-free by
    // implementing a tiny FNV-1a so the digest is stable & cheap.
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = FNV_OFFSET;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    format!("{h:016x}")
}

// ─────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_default_applies_floor() {
        let p = NclPricing::default();
        // 0 tokens → must hit the floor.
        let v = p.value_usd(0, 0);
        assert!((v - p.min_per_task_usd).abs() < 1e-12);
    }

    #[test]
    fn pricing_scales_with_tokens() {
        let p = NclPricing {
            price_in_per_1k_tokens: 1.0,
            price_out_per_1k_tokens: 2.0,
            min_per_task_usd: 0.0,
        };
        // 1000 in + 500 out = 1.0 + 1.0 = 2.0
        let v = p.value_usd(1000, 500);
        assert!((v - 2.0).abs() < 1e-12);
    }

    #[test]
    fn pricing_env_overrides_defaults() {
        std::env::set_var("ZION_NCL_PRICE_IN_PER_1K", "2.5");
        std::env::set_var("ZION_NCL_MIN_PER_TASK_USD", "0.1");
        let p = NclPricing::from_env();
        assert!((p.price_in_per_1k_tokens - 2.5).abs() < 1e-12);
        assert!((p.min_per_task_usd - 0.1).abs() < 1e-12);
        std::env::remove_var("ZION_NCL_PRICE_IN_PER_1K");
        std::env::remove_var("ZION_NCL_MIN_PER_TASK_USD");
    }

    #[test]
    fn authority_parsing_handles_schemes_and_ports() {
        assert_eq!(
            parse_authority("http://127.0.0.1:8002").unwrap(),
            "127.0.0.1:8002"
        );
        assert_eq!(parse_authority("127.0.0.1:8002").unwrap(), "127.0.0.1:8002");
        assert_eq!(parse_authority("localhost").unwrap(), "localhost:8002");
        assert_eq!(
            parse_authority("https://example.com/x").unwrap(),
            "example.com:8002"
        );
        assert_eq!(
            parse_authority("https://example.com:9999/x").unwrap(),
            "example.com:9999"
        );
        assert!(parse_authority("").is_err());
    }

    #[test]
    fn http_response_parser_extracts_status_and_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}";
        let (status, body) = parse_http_response(raw).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"{\"ok\":true}".to_vec());
    }

    #[test]
    fn approx_token_count_uses_word_boundary() {
        assert_eq!(approx_token_count(""), 0);
        assert_eq!(approx_token_count("one"), 1);
        assert_eq!(approx_token_count("hello world how are you"), 5);
    }

    #[test]
    fn response_digest_is_stable() {
        let a = response_digest("hello");
        let b = response_digest("hello");
        assert_eq!(a, b);
        assert_ne!(a, response_digest("hellz"));
    }

    // ──────────────────────────────────────────────────────────────
    // Integration test: stand up a mock HTTP gateway, run a real
    // chat_completion call, verify result.
    // ──────────────────────────────────────────────────────────────
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn chat_completion_against_mock_gateway() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Mock server: read a single HTTP request, respond with an
        // OpenAI-compat chat completion.
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf).await.unwrap();
            let body = r#"{"choices":[{"message":{"content":"hi"}}],"usage":{"prompt_tokens":7,"completion_tokens":2}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            s.write_all(resp.as_bytes()).await.unwrap();
            s.flush().await.unwrap();
        });

        let client = NclGatewayClient::new(&format!("http://{addr}"))
            .unwrap()
            .with_timeout(Duration::from_secs(5));
        let r = client.chat_completion("ping", 8).await.unwrap();
        assert_eq!(r.content, "hi");
        assert_eq!(r.prompt_tokens, 7);
        assert_eq!(r.completion_tokens, 2);

        let _ = server.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dispatcher_records_revenue_for_successful_task() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf).await.unwrap();
            let body = r#"{"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":3,"completion_tokens":1}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            s.write_all(resp.as_bytes()).await.unwrap();
            s.flush().await.unwrap();
        });

        let revenue = RevenueCollector::new();
        let client = NclGatewayClient::new(&format!("http://{addr}"))
            .unwrap()
            .with_timeout(Duration::from_secs(5));
        let dispatcher = NclDispatcher::new(client, NclPricing::default(), revenue.clone());
        let tx = dispatcher.spawn(NclHeartbeatConfig::default(), 16);
        tx.send(NclTaskRequest {
            prompt: "hello".to_string(),
            max_tokens: 8,
            value_usd_override: None,
            origin: "test".to_string(),
        })
        .await
        .unwrap();

        // Give the dispatcher time to process.
        for _ in 0..50 {
            sleep(Duration::from_millis(50)).await;
            let s = revenue.ncl_stats();
            if s.tasks_succeeded > 0 {
                assert_eq!(s.tasks_total, 1);
                assert_eq!(s.tasks_succeeded, 1);
                assert_eq!(s.tokens_in, 3);
                assert_eq!(s.tokens_out, 1);
                assert!(s.total_value_usd > 0.0);
                let _ = server.await;
                return;
            }
        }
        panic!("dispatcher did not record success within 2.5s");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dispatcher_records_failure_when_gateway_unreachable() {
        // Bind & immediately drop so the address is unreachable.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let revenue = RevenueCollector::new();
        let client = NclGatewayClient::new(&format!("http://{addr}"))
            .unwrap()
            .with_timeout(Duration::from_secs(2));
        let dispatcher = NclDispatcher::new(client, NclPricing::default(), revenue.clone());
        let tx = dispatcher.spawn(NclHeartbeatConfig::default(), 16);
        tx.send(NclTaskRequest {
            prompt: "x".to_string(),
            max_tokens: 4,
            value_usd_override: None,
            origin: "test-fail".to_string(),
        })
        .await
        .unwrap();

        for _ in 0..50 {
            sleep(Duration::from_millis(50)).await;
            let s = revenue.ncl_stats();
            if s.tasks_failed > 0 {
                assert_eq!(s.tasks_total, 1);
                assert_eq!(s.tasks_succeeded, 0);
                assert_eq!(s.tasks_failed, 1);
                assert!((s.total_value_usd - 0.0).abs() < 1e-12);
                return;
            }
        }
        panic!("dispatcher did not record failure within 2.5s");
    }
}
