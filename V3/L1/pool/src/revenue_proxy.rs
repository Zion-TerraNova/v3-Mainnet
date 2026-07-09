//! Revenue Proxy — Stratum client for external multi-algo pools.
//!
//! Forwards ZION backend shares to external pools (2miners, MoneroOcean,
//! ZPool, etc.) so the 25% multi-algo revenue stream is actually live.
//!
//! Each `ExternalPoolClient` maintains a long-lived TCP connection to one
//! external pool, subscribes for jobs, and submits shares produced by ZION
//! miners that have been routed to the `Revenue` or `Auto` session groups.

use anyhow::{Context, Result};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

use zion_cosmic_harmony::profit_router::CoinProfile;

/// Stats for one external pool connection.
#[derive(Debug, Default)]
pub struct ExternalPoolStats {
    pub jobs_received: AtomicU64,
    pub shares_submitted: AtomicU64,
    pub shares_accepted: AtomicU64,
    pub shares_rejected: AtomicU64,
    pub connected: AtomicU64, // 1 = connected, 0 = disconnected
}

/// A request to submit a share to the external pool.
#[derive(Debug, Clone)]
pub struct ShareSubmission {
    pub job_id: String,
    pub nonce: String,
    pub worker: String,
    /// Result hash (32 bytes hex) — required for CryptoNote/RandomX pools.
    pub result: String,
}

/// Async Stratum client for a single external coin/pool.
pub struct ExternalPoolClient {
    name: String,
    pool_addr: String,
    wallet: String,
    worker: String,
    /// Channel for inbound share submissions from ZION miners.
    submit_tx: mpsc::Sender<ShareSubmission>,
    submit_rx: tokio::sync::Mutex<mpsc::Receiver<ShareSubmission>>,
    stats: Arc<ExternalPoolStats>,
}

impl ExternalPoolClient {
    pub fn new(
        name: &str,
        pool_addr: &str,
        wallet: &str,
        worker: &str,
        stats: Arc<ExternalPoolStats>,
    ) -> Arc<Self> {
        let (submit_tx, submit_rx) = mpsc::channel(256);
        Arc::new(Self {
            name: name.to_string(),
            pool_addr: pool_addr.to_string(),
            wallet: wallet.to_string(),
            worker: worker.to_string(),
            submit_tx,
            submit_rx: tokio::sync::Mutex::new(submit_rx),
            stats,
        })
    }

    /// Queue a share for submission to the external pool.
    pub async fn queue_submit(&self, submission: ShareSubmission) {
        if let Err(e) = self.submit_tx.send(submission).await {
            warn!("[{}] Failed to queue share: {}", self.name, e);
        }
    }

    /// Main reconnect loop with exponential backoff.
    pub async fn run_loop(self: Arc<Self>) {
        let mut consecutive_failures: u32 = 0;
        loop {
            info!("[{}] Connecting to {} ...", self.name, self.pool_addr);
            match self.connect_and_session().await {
                Ok(_) => {
                    consecutive_failures = 0;
                    warn!("[{}] Connection finished, reconnecting in 5s...", self.name);
                    sleep(Duration::from_secs(5)).await;
                }
                Err(e) => {
                    consecutive_failures += 1;
                    let err_str = e.to_string();
                    let delay_secs = if err_str.contains("IP ban")
                        || err_str.contains("temporarily suspended")
                    {
                        warn!("[{}] IP ban detected — backing off 10 min", self.name);
                        600
                    } else {
                        let exp = 10u64 * (1u64 << consecutive_failures.min(5));
                        exp.min(300)
                    };
                    error!(
                        "[{}] Connection error: {}. Retrying in {}s (attempt #{})",
                        self.name, e, delay_secs, consecutive_failures
                    );
                    sleep(Duration::from_secs(delay_secs)).await;
                }
            }
        }
    }

    /// Handshake + job consumer + share forwarder.
    async fn connect_and_session(self: &Arc<Self>) -> Result<()> {
        let stream = TcpStream::connect(&self.pool_addr)
            .await
            .with_context(|| format!("failed to connect to {}", self.pool_addr))?;

        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();

        // Mark connected
        self.stats.connected.store(1, Ordering::Relaxed);

        // --- Stratum v1 handshake ---
        let subscribe_req = json!({
            "id": 1,
            "method": "mining.subscribe",
            "params": [format!("zion_proxy/{}", self.name), null]
        });
        Self::send_line(&mut writer, &subscribe_req).await?;

        // Expect subscribe response
        let _subscribe_resp = Self::recv_line(&mut lines).await?;
        debug!("[{}] subscribe response received", self.name);

        let auth_req = json!({
            "id": 2,
            "method": "mining.authorize",
            "params": [format!("{}.{}", self.wallet, self.worker)]
        });
        Self::send_line(&mut writer, &auth_req).await?;

        // Expect authorize response
        let _auth_resp = Self::recv_line(&mut lines).await?;
        info!(
            "[{}] Authorized as {}.{}",
            self.name, self.wallet, self.worker
        );

        // --- Main loop: read jobs, forward shares ---
        let mut submit_rx = self.submit_rx.lock().await;
        loop {
            tokio::select! {
                line = lines.next_line() => {
                    match line? {
                        Some(text) => {
                            self.handle_pool_message(&text).await?;
                        }
                        None => {
                            warn!("[{}] Upstream closed connection", self.name);
                            break;
                        }
                    }
                }
                Some(submission) = submit_rx.recv() => {
                    let submit_req = json!({
                        "id": 3,
                        "method": "mining.submit",
                        "params": [
                            format!("{}.{}", self.wallet, submission.worker),
                            submission.job_id,
                            submission.nonce,
                        ]
                    });
                    Self::send_line(&mut writer, &submit_req).await?;
                    self.stats.shares_submitted.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        self.stats.connected.store(0, Ordering::Relaxed);
        Ok(())
    }

    async fn handle_pool_message(&self, text: &str) -> Result<()> {
        let msg: serde_json::Value = serde_json::from_str(text)
            .with_context(|| format!("invalid JSON from pool: {text}"))?;

        if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
            match method {
                "mining.notify" => {
                    self.stats.jobs_received.fetch_add(1, Ordering::Relaxed);
                    debug!("[{}] mining.notify received", self.name);
                }
                "mining.set_difficulty" => {
                    debug!("[{}] difficulty updated", self.name);
                }
                _ => {
                    debug!("[{}]Unhandled method: {}", self.name, method);
                }
            }
        }

        if let Some(result) = msg.get("result") {
            if result.get("status").and_then(|s| s.as_str()) == Some("ok") {
                self.stats.shares_accepted.fetch_add(1, Ordering::Relaxed);
            }
        }
        if let Some(error) = msg.get("error") {
            if !error.is_null() {
                self.stats.shares_rejected.fetch_add(1, Ordering::Relaxed);
                warn!("[{}] Share rejected: {}", self.name, error);
            }
        }

        Ok(())
    }

    async fn send_line(
        writer: &mut tokio::net::tcp::OwnedWriteHalf,
        value: &serde_json::Value,
    ) -> Result<()> {
        let mut line = serde_json::to_string(value)?;
        line.push('\n');
        writer.write_all(line.as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }

    async fn recv_line(
        lines: &mut tokio::io::Lines<tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>>,
    ) -> Result<serde_json::Value> {
        let line = lines
            .next_line()
            .await?
            .context("expected JSON line from pool, got EOF")?;
        let val =
            serde_json::from_str(&line).with_context(|| format!("invalid JSON line: {line}"))?;
        Ok(val)
    }
}

/// Transparent Stratum proxy listener.
/// Accepts connections from GPU miners and forwards JSON-RPC lines to an
/// external pool, substituting the pool wallet in authorize / login
/// messages so payouts go to the operator.
pub struct ProxyListener {
    listen_addr: String,
    pool_addr: String,
    wallet: String,
    worker: String,
    stats: Arc<ExternalPoolStats>,
}

impl ProxyListener {
    pub fn new(
        listen_addr: impl Into<String>,
        pool_addr: impl Into<String>,
        wallet: impl Into<String>,
        worker: impl Into<String>,
        stats: Arc<ExternalPoolStats>,
    ) -> Self {
        Self {
            listen_addr: listen_addr.into(),
            pool_addr: pool_addr.into(),
            wallet: wallet.into(),
            worker: worker.into(),
            stats,
        }
    }

    /// Start listening and forwarding miner connections.
    pub async fn run(self: Arc<Self>) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(&self.listen_addr)
            .await
            .with_context(|| format!("failed to bind proxy on {}", self.listen_addr))?;
        info!(
            "Proxy listening on {} → {}",
            self.listen_addr, self.pool_addr
        );

        loop {
            let (miner_stream, peer) = listener.accept().await?;
            let proxy = Arc::clone(&self);
            tokio::spawn(async move {
                if let Err(e) = proxy.forward_session(miner_stream, peer).await {
                    warn!("[{}] Session from {} ended: {}", proxy.pool_addr, peer, e);
                }
            });
        }
    }

    /// Forward a single miner session: connect upstream, swap wallet, pipe lines.
    async fn forward_session(
        &self,
        miner_stream: tokio::net::TcpStream,
        peer: std::net::SocketAddr,
    ) -> Result<()> {
        let upstream = TcpStream::connect(&self.pool_addr)
            .await
            .with_context(|| format!("failed to connect to pool {}", self.pool_addr))?;
        info!("[{}] ↔ Session from {} started", self.pool_addr, peer);

        let (mut upstream_r, mut upstream_w) = upstream.into_split();
        let (mut downstream_r, mut downstream_w) = miner_stream.into_split();

        // Spawn two tasks: downstream → upstream (with wallet substitution on first messages)
        // and upstream → downstream (transparent).
        let wallet = self.wallet.clone();
        let worker = self.worker.clone();
        let pool_addr_d2u = self.pool_addr.clone();
        let pool_addr_u2d = self.pool_addr.clone();
        let stats = Arc::clone(&self.stats);

        let d2u = tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(&mut downstream_r);
            let mut buf = String::new();
            let mut authorized = false;
            loop {
                buf.clear();
                match reader.read_line(&mut buf).await {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) => {
                        warn!("[{}] Error reading from miner: {}", pool_addr_d2u, e);
                        break;
                    }
                }
                let mut line = buf.trim_end().to_string();
                if line.is_empty() {
                    continue;
                }

                // Wallet substitution on first authorize / login / subscribe messages.
                if !authorized {
                    if let Ok(mut msg) = serde_json::from_str::<serde_json::Value>(&line) {
                        if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
                            match method {
                                "mining.authorize" => {
                                    if let Some(params) =
                                        msg.get_mut("params").and_then(|p| p.as_array_mut())
                                    {
                                        if !params.is_empty() {
                                            let user = format!("{}.{}", wallet, worker);
                                            params[0] = serde_json::Value::String(user);
                                        }
                                    }
                                    authorized = true;
                                }
                                "mining.subscribe" => {
                                    // Replace user-agent with ours.
                                    if let Some(params) =
                                        msg.get_mut("params").and_then(|p| p.as_array_mut())
                                    {
                                        if !params.is_empty() {
                                            params[0] = serde_json::Value::String(
                                                "zion_revenue_proxy/1.0".to_string(),
                                            );
                                        }
                                    }
                                }
                                "login" => {
                                    // CryptoNote stratum login.
                                    if let Some(params) =
                                        msg.get_mut("params").and_then(|p| p.as_object_mut())
                                    {
                                        params.insert(
                                            "login".to_string(),
                                            serde_json::Value::String(wallet.clone()),
                                        );
                                        params.insert(
                                            "pass".to_string(),
                                            serde_json::Value::String(worker.clone()),
                                        );
                                    }
                                    authorized = true;
                                }
                                _ => {}
                            }
                        }
                        line = serde_json::to_string(&msg).unwrap_or(line);
                    }
                }

                line.push('\n');
                if let Err(e) = upstream_w.write_all(line.as_bytes()).await {
                    warn!("[{}] Error writing to pool: {}", pool_addr_d2u, e);
                    break;
                }
                if let Err(e) = upstream_w.flush().await {
                    warn!("[{}] Error flushing to pool: {}", pool_addr_d2u, e);
                    break;
                }
            }
        });

        let u2d = tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(&mut upstream_r);
            let mut buf = String::new();
            loop {
                buf.clear();
                match reader.read_line(&mut buf).await {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) => {
                        warn!("[{}] Error reading from pool: {}", pool_addr_u2d, e);
                        break;
                    }
                }
                if buf.trim().is_empty() {
                    continue;
                }
                // Track stats from pool responses.
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(buf.trim()) {
                    if let Some(error) = msg.get("error") {
                        if !error.is_null() {
                            stats.shares_rejected.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    if let Some(result) = msg.get("result") {
                        if result.get("status").and_then(|s| s.as_str()) == Some("ok") {
                            stats.shares_accepted.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
                        if method == "mining.notify" {
                            stats.jobs_received.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                if let Err(e) = downstream_w.write_all(buf.as_bytes()).await {
                    warn!("[{}] Error writing to miner: {}", pool_addr_u2d, e);
                    break;
                }
                if let Err(e) = downstream_w.flush().await {
                    warn!("[{}] Error flushing to miner: {}", pool_addr_u2d, e);
                    break;
                }
            }
        });

        // Wait for either direction to finish.
        tokio::select! {
            _ = d2u => {},
            _ = u2d => {},
        }

        info!("[{}] Session from {} ended", self.pool_addr, peer);
        Ok(())
    }
}

/// Build a client from a `CoinProfile` and wallet string.
pub fn client_from_profile(
    profile: &CoinProfile,
    wallet: &str,
    worker: &str,
) -> Arc<ExternalPoolClient> {
    let stats = Arc::new(ExternalPoolStats::default());
    ExternalPoolClient::new(
        &profile.ticker,
        &profile.pool_address(),
        wallet,
        worker,
        stats,
    )
}
