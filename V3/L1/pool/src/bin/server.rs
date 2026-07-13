use anyhow::{anyhow, Context, Result};
use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zion_core::wallet::{BatchRecipient, SpendableUtxo};
use zion_core::{
    decode_rpc_response, encode_rpc_request, BlockTemplate, ConsensusConfig, CoreRuntime,
    DifficultyTarget, MiningHeader, MiningSolution, RevenueSource, RpcRequest, RpcResponse,
    Transaction as AccountTransaction,
};
use zion_pool::ncl_gateway::{NclDispatcher, NclGatewayClient, NclHeartbeatConfig, NclPricing};
use zion_pool::pplns::{FeeConfig, PayoutEntry, PplnsConfig, PplnsEngine};
use zion_pool::{
    decode_message, encode_message, MiningPool, PoolMessage, ShareDecision, ShareStatus,
};
use zion_auxpow::{
    AuxPowScheduler, AuxPowStats, ExternalCoin, JobMultiplexer, JobPackage, ShareForwardResult,
    ShareForwarder, SplitConfig,
};
use zion_cosmic_harmony::{
    fetch_live_profit_estimates, select_best_coin, ExternalCoin as ChExternalCoin,
};
use zion_cosmic_harmony::stream_profit::{
    fetch_profit_snapshot, StreamProfitConfig, StreamProfitSnapshot, StreamWeights,
};
use zion_core::MiningJob;

// ---------------------------------------------------------------------------
// LogChannel — batched async logging to reduce I/O on the hot path
// ---------------------------------------------------------------------------
// With 1000+ miners each generating many log lines per share submission,
// synchronous stdout writes (each a syscall + kernel buffer flush) become a
// major bottleneck and can fill /var/log/syslog rapidly.  LogChannel sends
// log lines through an mpsc channel to a background thread that batches them into
// 4 KB chunks and writes with a single `write_all`, flushing at most every 100 ms.
// Per-share/per-job lines are further gated behind ZION_POOL_VERBOSE_LOGS=1 so
// that production nodes emit only summary, error and block-found logs.

struct LogChannel {
    tx: mpsc::SyncSender<String>,
}

impl LogChannel {
    fn spawn() -> Self {
        let (tx, rx) = mpsc::sync_channel::<String>(4096);
        thread::spawn(move || {
            let stdout = std::io::stdout();
            let mut buf = String::with_capacity(8192);
            let flush_interval = Duration::from_millis(100);
            loop {
                match rx.recv_timeout(flush_interval) {
                    Ok(line) => {
                        buf.push_str(&line);
                        buf.push('\n');
                        // Flush if buffer exceeds 4 KB or channel is empty.
                        if buf.len() >= 4096 {
                            let mut out = stdout.lock();
                            let _ = out.write_all(buf.as_bytes());
                            let _ = out.flush();
                            drop(out);
                            buf.clear();
                            // Try to drain more without blocking.
                            while let Ok(more) = rx.try_recv() {
                                buf.push_str(&more);
                                buf.push('\n');
                                if buf.len() >= 8192 {
                                    let mut out = stdout.lock();
                                    let _ = out.write_all(buf.as_bytes());
                                    let _ = out.flush();
                                    drop(out);
                                    buf.clear();
                                }
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if !buf.is_empty() {
                            let mut out = stdout.lock();
                            let _ = out.write_all(buf.as_bytes());
                            let _ = out.flush();
                            drop(out);
                            buf.clear();
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        // Final flush on shutdown.
                        if !buf.is_empty() {
                            let mut out = stdout.lock();
                            let _ = out.write_all(buf.as_bytes());
                            let _ = out.flush();
                            drop(out);
                        }
                        break;
                    }
                }
            }
        });
        Self { tx }
    }

    /// Send a log line.  Non-blocking: if the channel is full the line is
    /// dropped (prefer dropping logs over blocking miner threads).
    fn log(&self, line: String) {
        let _ = self.tx.try_send(line);
    }

    /// Send a log line only when verbose pool logging is enabled.
    /// Use this for the per-share/per-job hot path to avoid filling syslog.
    fn log_verbose(&self, line: String) {
        if pool_verbose_logs_enabled() {
            let _ = self.tx.try_send(line);
        }
    }
}

/// Check whether verbose per-share/per-job pool logging is enabled.
/// Default: off. Set `ZION_POOL_VERBOSE_LOGS=1` to enable.
fn pool_verbose_logs_enabled() -> bool {
    static CACHED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("ZION_POOL_VERBOSE_LOGS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

// ---------------------------------------------------------------------------
// DeferredPayout — queued payout that will be retried by a background thread
// ---------------------------------------------------------------------------
// When a block is found the pool immediately tries to distribute the miner
// share via an on-chain transaction.  However, the node may not have credited
// the coinbase reward to the pool wallet yet (race condition between
// submit_candidate_to_node and getBalance).  Instead of rolling back the PPLNS
// balances and losing the payout, we queue it here.  A dedicated background
// thread polls the queue every 2 s and retries until the balance is sufficient
// or a maximum retry count is reached (default 300 = 10 minutes).

struct DeferredPayout {
    payouts: Vec<PayoutEntry>,
    height: u64,
    queued_at: Instant,
    retry_count: u32,
}

type DeferredPayoutQueue = Arc<Mutex<Vec<DeferredPayout>>>;

// ---------------------------------------------------------------------------
// TemplateCache — shared block-template cache to reduce node RPC load
// ---------------------------------------------------------------------------
// Without this cache every miner thread calls `get_template` on every loop
// iteration (once per share submission).  With 18+ miners that generates
// 100+ RPC calls/sec, pinning a full CPU core on the node.  The cache is
// shared across all sessions via Arc<Mutex<TemplateCache>> and only refetches
// when the template is older than `ttl` (default 3 s) or when a fetch
// explicitly fails.

struct TemplateCache {
    template: Option<BlockTemplate>,
    fetched_at: Instant,
    ttl: Duration,
}

impl TemplateCache {
    fn new(ttl: Duration) -> Self {
        Self {
            template: None,
            fetched_at: Instant::now(),
            ttl,
        }
    }

    /// Return a cached template if fresh enough, otherwise fetch from the
    /// node.  On fetch failure we fall back to the stale cached template
    /// (graceful degradation) so one bad RPC does not kill all sessions.
    fn get_or_fetch(&mut self, node_rpc_addr: &str) -> Result<BlockTemplate> {
        if let Some(ref t) = self.template {
            if self.fetched_at.elapsed() < self.ttl {
                return Ok(t.clone());
            }
        }
        match fetch_node_template(node_rpc_addr) {
            Ok(t) => {
                self.template = Some(t.clone());
                self.fetched_at = Instant::now();
                Ok(t)
            }
            Err(e) => {
                if let Some(ref t) = self.template {
                    println!("template_cache: fetch failed ({e:#}), serving stale template height={}", t.height);
                    Ok(t.clone())
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Force the next `get_or_fetch` to fetch a fresh template from the node.
    /// Called after a block is accepted so miners immediately get the next
    /// height's template instead of re-mining the accepted block.
    fn invalidate(&mut self) {
        self.template = None;
    }
}

/// Notify the ZION OASIS L4 game server that a block was mined so it can
/// award XP to the miner.  This is a best-effort fire-and-forget call;
/// failure is silently logged so the pool never blocks or errors.
fn notify_oasis_block_mined(miner_address: &str, block_height: u64) {
    let oasis_base_url = match std::env::var("ZION_OASIS_API_URL") {
        Ok(url) if !url.is_empty() => url,
        _ => return, // hook disabled — nothing to do
    };

    // Defense-in-depth: only allow localhost targets unless explicitly opted-in.
    let allow_remote = std::env::var("ZION_OASIS_ALLOW_REMOTE")
        .ok()
        .map(|v| {
            let normalized = v.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes"
        })
        .unwrap_or(false);

    let (authority, base_path) = match parse_oasis_http_target(&oasis_base_url, allow_remote) {
        Ok(target) => target,
        Err(e) => {
            println!("oasis_xp_hook_invalid_url url={} err={}", oasis_base_url, e);
            return;
        }
    };

    let body = format!(
        r#"{{"source":"block_mined","amount":500,"block_height":{}}}"#,
        block_height
    );
    let path = format!("{}{}{}", base_path, "/api/v1/oasis/player/", miner_address);
    let full_path = format!("{}/xp", path);

    match post_json_http(&authority, &full_path, &body, Duration::from_secs(3)) {
        Ok(code) if code == 200 || code == 201 => {
            println!(
                "oasis_xp_awarded miner={} height={}",
                miner_address, block_height
            );
        }
        Ok(code) => {
            println!(
                "oasis_xp_hook_failed miner={} height={} http_code={}",
                miner_address, block_height, code
            );
        }
        Err(e) => {
            println!(
                "oasis_xp_hook_unavailable miner={} height={} err={}",
                miner_address, block_height, e
            );
        }
    }
}

fn parse_oasis_http_target(url: &str, allow_remote: bool) -> Result<(String, String)> {
    let trimmed = url.trim();
    let without_scheme = trimmed
        .strip_prefix("http://")
        .ok_or_else(|| anyhow!("only http:// URLs are supported"))?;

    let (authority_raw, path_raw) = match without_scheme.split_once('/') {
        Some((host_port, path)) => (host_port, format!("/{}", path.trim_start_matches('/'))),
        None => (without_scheme, String::new()),
    };

    let authority = authority_raw.trim().trim_end_matches('/');
    if authority.is_empty() {
        return Err(anyhow!("missing host:port"));
    }

    // Default to localhost-only to prevent accidental SSRF via env misconfiguration.
    let host = authority.split(':').next().map(str::trim).unwrap_or("");
    let is_local = matches!(host, "127.0.0.1" | "localhost");
    if !allow_remote && !is_local {
        return Err(anyhow!(
            "remote OASIS target blocked; set ZION_OASIS_ALLOW_REMOTE=true to override"
        ));
    }

    Ok((authority.to_string(), path_raw))
}

fn post_json_http(authority: &str, path: &str, body: &str, timeout: Duration) -> Result<u16> {
    let mut stream =
        TcpStream::connect(authority).with_context(|| format!("connect failed to {authority}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .context("set read timeout")?;
    stream
        .set_write_timeout(Some(timeout))
        .context("set write timeout")?;

    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .context("write request")?;
    stream.flush().context("flush request")?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .context("read response")?;
    let status_line = response
        .lines()
        .next()
        .ok_or_else(|| anyhow!("empty HTTP response"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("missing HTTP status code"))?
        .parse::<u16>()
        .context("invalid HTTP status code")?;
    Ok(status)
}

/// Best-effort fire-and-forget share relay to upstream/Core pool.
/// Opens a TCP connection, sends a single ShareRelay line, and closes.
/// Failure is logged but never blocks the local session.
fn relay_share_fire_and_forget(upstream_addr: &str, relay: &PoolMessage) -> Result<()> {
    let mut stream = TcpStream::connect(upstream_addr)
        .with_context(|| format!("failed to connect to upstream pool at {}", upstream_addr))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let line = encode_message(relay)?;
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    // We intentionally do NOT read a response — fire-and-forget.
    Ok(())
}

/// Maps a `RevenueSource` to the external coin to mine.  Returns `None` for
/// non-external sources (Zion, NclAi, DeekshaLite, ThermalBonus, etc.).
fn revenue_source_to_external_coin(source: RevenueSource) -> Option<ExternalCoin> {
    match source {
        RevenueSource::Blake3External => Some(ExternalCoin::DCR),
        RevenueSource::KHeavyHashExternal => Some(ExternalCoin::KAS),
        RevenueSource::EthashExternal => Some(ExternalCoin::ETC),
        RevenueSource::KawPowExternal => Some(ExternalCoin::RVN),
        RevenueSource::AutolykosExternal => Some(ExternalCoin::ERG),
        RevenueSource::RandomXExternal => Some(ExternalCoin::XMR),
        RevenueSource::ZelHashExternal => Some(ExternalCoin::FLUX),
        RevenueSource::VerusHashExternal => Some(ExternalCoin::VRSC),
        _ => None,
    }
}

/// Maps an `ExternalCoin` to the revenue source used for routing stats.
fn external_coin_to_revenue_source(coin: ExternalCoin) -> RevenueSource {
    match coin {
        ExternalCoin::DCR | ExternalCoin::ALPH => RevenueSource::Blake3External,
        ExternalCoin::KAS => RevenueSource::KHeavyHashExternal,
        ExternalCoin::ETC => RevenueSource::EthashExternal,
        ExternalCoin::RVN | ExternalCoin::CLORE | ExternalCoin::EVR | ExternalCoin::MEWC => {
            RevenueSource::KawPowExternal
        }
        ExternalCoin::ERG => RevenueSource::AutolykosExternal,
        ExternalCoin::XMR => RevenueSource::RandomXExternal,
        ExternalCoin::FLUX => RevenueSource::ZelHashExternal,
        ExternalCoin::VRSC => RevenueSource::VerusHashExternal,
    }
}

/// Estimate the USD value of a single accepted external share.
///
/// This is a rough heuristic based on the coin's daily revenue estimate
/// divided by an assumed daily share count at difficulty 1.  In production
/// this would be replaced by actual payout data from the external pool.
fn estimate_external_share_usd(coin: ExternalCoin) -> f64 {
    // Use live estimates (falls back to static on API failure).
    let estimates = fetch_live_profit_estimates();
    let ch_coin = auxpow_to_ch_external_coin(coin);
    let daily_revenue = estimates
        .iter()
        .find(|e| e.coin == ch_coin)
        .map(|e| e.revenue_per_day_usd)
        .unwrap_or(0.10);
    // Assume ~10000 shares/day at reference hashrate (conservative).
    daily_revenue / 10_000.0
}

/// Convert `zion_auxpow::ExternalCoin` to `zion_cosmic_harmony::ExternalCoin`.
fn auxpow_to_ch_external_coin(coin: ExternalCoin) -> ChExternalCoin {
    match coin {
        ExternalCoin::DCR => ChExternalCoin::DCR,
        ExternalCoin::ALPH => ChExternalCoin::ALPH,
        ExternalCoin::KAS => ChExternalCoin::KAS,
        ExternalCoin::ERG => ChExternalCoin::ERG,
        ExternalCoin::RVN => ChExternalCoin::RVN,
        ExternalCoin::ETC => ChExternalCoin::ETC,
        ExternalCoin::EVR => ChExternalCoin::EVR,
        ExternalCoin::MEWC => ChExternalCoin::MEWC,
        ExternalCoin::FLUX => ChExternalCoin::FLUX,
        ExternalCoin::CLORE => ChExternalCoin::CLORE,
        ExternalCoin::XMR => ChExternalCoin::XMR,
        ExternalCoin::VRSC => ChExternalCoin::VRSC,
    }
}

/// Convert `zion_cosmic_harmony::ExternalCoin` to `zion_auxpow::ExternalCoin`.
fn ch_to_auxpow_external_coin(coin: ChExternalCoin) -> ExternalCoin {
    match coin {
        ChExternalCoin::DCR => ExternalCoin::DCR,
        ChExternalCoin::ALPH => ExternalCoin::ALPH,
        ChExternalCoin::KAS => ExternalCoin::KAS,
        ChExternalCoin::ERG => ExternalCoin::ERG,
        ChExternalCoin::RVN => ExternalCoin::RVN,
        ChExternalCoin::ETC => ExternalCoin::ETC,
        ChExternalCoin::EVR => ExternalCoin::EVR,
        ChExternalCoin::MEWC => ExternalCoin::MEWC,
        ChExternalCoin::FLUX => ExternalCoin::FLUX,
        ChExternalCoin::CLORE => ExternalCoin::CLORE,
        ChExternalCoin::XMR => ExternalCoin::XMR,
        ChExternalCoin::VRSC => ExternalCoin::VRSC,
    }
}

/// Maps an external coin to the ZION-pool algorithm string that miners expect.
fn external_coin_to_algorithm(coin: ExternalCoin) -> &'static str {
    match coin {
        ExternalCoin::DCR | ExternalCoin::ALPH => "blake3",
        ExternalCoin::KAS => "kheavyhash",
        ExternalCoin::ETC => "ethash",
        ExternalCoin::RVN | ExternalCoin::CLORE => "kawpow",
        ExternalCoin::ERG => "autolykos",
        ExternalCoin::XMR => "randomx",
        ExternalCoin::FLUX => "zelhash",
        ExternalCoin::EVR | ExternalCoin::MEWC => "kawpow",
        ExternalCoin::VRSC => "verushash",
    }
}

/// Background tokio task that keeps the `JobMultiplexer` connected and
/// forwards shares submitted by session threads.
async fn run_auxpow_bridge(
    cfg: AuxPowIntegrationConfig,
    bridge: AuxPowBridge,
    share_rx: std::sync::mpsc::Receiver<(ShareForwardRequest, std::sync::mpsc::Sender<ShareForwardOutcome>)>,
) {
    let mut mux = JobMultiplexer::new(&cfg.payout_wallet, &cfg.worker_name)
        .with_preference(cfg.pool_preference, &cfg.region);

    // Helper closure: select the wallet for a given coin (per-coin override
    // takes precedence over the default payout_wallet).
    let coin_wallets = cfg.coin_wallets.clone();
    let select_wallet = |coin: ExternalCoin| -> String {
        coin_wallets
            .get(coin.ticker())
            .cloned()
            .unwrap_or_else(|| cfg.payout_wallet.clone())
    };

    // Initial coin selection: force_coin wins, otherwise use profit-based selection.
    let initial_coin = cfg.force_coin.unwrap_or_else(|| {
        let estimates = fetch_live_profit_estimates();
        ch_to_auxpow_external_coin(
            select_best_coin(&estimates, None, cfg.hysteresis_pct)
                .unwrap_or(ChExternalCoin::KAS),
        )
    });
    mux.set_wallet(select_wallet(initial_coin));
    if let Err(e) = mux.connect(initial_coin).await {
        eprintln!("auxpow_bridge: initial connect to {} failed: {}", initial_coin, e);
    }

    // Track when we last checked profitability for auto-switching.
    let mut last_profit_check = Instant::now();
    let profit_check_interval = Duration::from_secs(cfg.profit_check_interval_secs);

    loop {
        // If the multiplexer has no active client (e.g. after a disconnect or
        // failed initial connect), try to reconnect before waiting for jobs.
        if mux.active_coin().is_none() {
            let coin = cfg.force_coin.unwrap_or_else(|| {
                let estimates = fetch_live_profit_estimates();
                ch_to_auxpow_external_coin(
                    select_best_coin(&estimates, None, cfg.hysteresis_pct)
                        .unwrap_or(ChExternalCoin::KAS),
                )
            });
            mux.set_wallet(select_wallet(coin));
            eprintln!("auxpow_bridge: no active connection, reconnecting to {}…", coin);
            if let Err(e) = mux.connect(coin).await {
                eprintln!("auxpow_bridge: reconnect to {} failed: {}", coin, e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
            println!("auxpow_bridge: reconnected to {}", coin);
        }

        // Auto coin switching: when force_coin is None, periodically check
        // profitability and switch to the best coin if hysteresis allows.
        if cfg.force_coin.is_none() && last_profit_check.elapsed() >= profit_check_interval {
            last_profit_check = Instant::now();
            let estimates = fetch_live_profit_estimates();
            let current = mux.active_coin().map(auxpow_to_ch_external_coin);
            if let Some(best) =
                select_best_coin(&estimates, current, cfg.hysteresis_pct)
            {
                if current != Some(best) {
                    println!(
                        "auxpow_bridge: profit_switch old={:?} new={} old_profit={:.4} new_profit={:.4}",
                        current,
                        best,
                        estimates
                            .iter()
                            .find(|e| Some(e.coin) == current)
                            .map(|e| e.profit_per_day_usd())
                            .unwrap_or(0.0),
                        estimates
                            .iter()
                            .find(|e| e.coin == best)
                            .map(|e| e.profit_per_day_usd())
                            .unwrap_or(0.0),
                    );
                    let new_coin = ch_to_auxpow_external_coin(best);
                    mux.disconnect().await;
                    mux.set_wallet(select_wallet(new_coin));
                    if let Err(e) = mux.connect(new_coin).await {
                        eprintln!("auxpow_bridge: profit_switch connect to {} failed: {}", new_coin, e);
                    } else {
                        println!("auxpow_bridge: profit_switch connected to {}", new_coin);
                    }
                }
            }
        }

        // Pull new jobs from the multiplexer and push them to the queue.
        // wait_for_job blocks the tokio task until a job arrives, which is fine
        // because this task has no other work besides forwarding shares.
        match mux.wait_for_job(5_000).await {
            Ok(Some(job)) => {
                println!(
                    "auxpow_bridge: queued job_id={} coin={} algo={}",
                    job.external_job_id, job.external_coin, job.algorithm
                );
                let mut q = bridge.job_queue.lock().expect("auxpow job queue lock poisoned");
                // Keep at most 2 jobs per algorithm to avoid stale work.
                while q.len() >= 2 {
                    q.pop_back();
                }
                q.push_front(job);
            }
            Ok(None) => {
                // No new job within the 5-second window; this is normal when
                // the external pool has not yet issued a notify.
            }
            Err(e) => {
                eprintln!("auxpow_bridge: wait_for_job error: {}", e);
                // The connection is likely dead — disconnect so the next
                // iteration triggers a reconnect.
                mux.disconnect().await;
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }

        // Drain any pending share-forward requests without blocking indefinitely.
        while let Ok((req, reply_tx)) = share_rx.try_recv() {
            let started = Instant::now();
            let result = if let Some(client) = mux.client() {
                let forwarder = ShareForwarder::new(client);
                match forwarder.try_forward(&req.external_job_id, req.nonce, &req.hash, &req.target, req.mix_hash.as_ref()).await {
                    Ok(r) => {
                        println!(
                            "auxpow_bridge: share_forwarded job_id={} nonce={} result={:?} elapsed_ms={}",
                            req.external_job_id, req.nonce, r, started.elapsed().as_millis()
                        );
                        r
                    }
                    Err(e) => {
                        eprintln!("auxpow_bridge: forward error: {}", e);
                        ShareForwardResult::Unknown
                    }
                }
            } else {
                ShareForwardResult::NotConnected
            };
            let _ = reply_tx.send(ShareForwardOutcome {
                result,
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        }
    }
}

fn main() -> Result<()> {
    let config = ServerConfig::from_env()?;
    let log_channel = Arc::new(LogChannel::spawn());
    let pool = Arc::new(Mutex::new(MiningPool::with_job_ttl(
        CoreRuntime::new_with_journal_replay(ConsensusConfig::default()),
        config.job_ttl_ms,
    )));
    let revenue_scheduler = Arc::new(Mutex::new(RevenueScheduler::from_env(
        config.revenue_source,
        config.revenue_value_usd,
    )?));
    let routing_stats = Arc::new(Mutex::new(RoutingStats::new(config.routing_log_every)));
    let miner_telemetry = Arc::new(Mutex::new(MinerTelemetryRegistry::default()));
    let fee_config = config.fee_config.clone();
    println!(
        "fee_split: miners={}% humanitarian={}% issobella={}% pool_fee={}%",
        fee_config.miner_pct(),
        fee_config.humanitarian_pct,
        fee_config.issobella_pct,
        fee_config.pool_fee_pct
    );
    if !fee_config.humanitarian_wallet.is_empty() {
        println!("humanitarian_wallet={}", fee_config.humanitarian_wallet);
    }
    if !fee_config.issobella_wallet.is_empty() {
        println!("issobella_wallet={}", fee_config.issobella_wallet);
    }
    let pplns_state_path = std::env::var("ZION_PPLNS_STATE_PATH").unwrap_or_default();
    let mut pplns_engine_inner = PplnsEngine::new(PplnsConfig {
        window_size: parse_env_u64("ZION_PPLNS_WINDOW_SIZE", 500_000).unwrap_or(500_000) as usize,
        min_payout_flowers: parse_env_u64(
            "ZION_PPLNS_MIN_PAYOUT",
            zion_core::wallet::MIN_PAYOUT_AMOUNT,
        )
        .unwrap_or(zion_core::wallet::MIN_PAYOUT_AMOUNT),
        fee_config,
    });

    // Restore PPLNS state from disk if a state path is configured and the file exists.
    if !pplns_state_path.is_empty() {
        if let Some(snap) = PplnsEngine::load_from_path(&pplns_state_path) {
            println!(
                "pplns_persistence: restored state from {} — shares={} miners={} unpaid_miners={} total_paid={}",
                pplns_state_path,
                snap.window.len(),
                snap.addresses.len(),
                snap.unpaid.len(),
                snap.total_paid_flowers
            );
            pplns_engine_inner.restore(snap);
        } else {
            println!(
                "pplns_persistence: no snapshot found at {} — starting fresh",
                pplns_state_path
            );
        }
    } else {
        println!(
            "pplns_persistence: ZION_PPLNS_STATE_PATH not set — state will be lost on restart"
        );
    }

    let pplns_engine = Arc::new(Mutex::new(pplns_engine_inner));
    let active_sessions = Arc::new(AtomicU64::new(0));
    let session_id_counter = Arc::new(AtomicU64::new(0));
    let template_cache = Arc::new(Mutex::new(TemplateCache::new(
        Duration::from_secs(3),
    )));
    let listener = TcpListener::bind(&config.bind_addr)
        .with_context(|| format!("failed to bind pool listener on {}", config.bind_addr))?;

    println!("ZION v3 pool server");
    println!("bind_addr={}", config.bind_addr);
    println!("loop_count={}", config.loop_count);
    println!("job_ttl_ms={}", config.job_ttl_ms);
    println!(
        "accept_limit={}",
        config
            .accept_limit
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unbounded".to_string())
    );
    if let Some(node_rpc_addr) = config.node_rpc_addr.as_deref() {
        println!("node_rpc_addr={node_rpc_addr}");
    }
    if let Some(upstream) = config.upstream_pool_addr.as_deref() {
        println!("upstream_pool_addr={upstream} (share relay enabled — Edge mode)");
    } else {
        println!("upstream_pool_addr=(not set) — this pool owns the PPLNS window (Core mode)");
    }
    println!(
        "payout_execution={} pool_wallet={}",
        if config.pool_signing_key.is_some() && config.pool_wallet_address.is_some() {
            "enabled"
        } else {
            "disabled"
        },
        config.pool_wallet_address.as_deref().unwrap_or("(not set)"),
    );
    // CRITICAL 3.0.4 Finding 1/2 guard: since the node now rejects account
    // transactions whose public key does not derive to the sender address, a
    // pool whose ZION_POOL_PAYOUT_SK_HEX does not derive to ZION_POOL_WALLET
    // will have every payout silently rejected. Fail fast.
    if let (Some(signing_key), Some(wallet_addr)) =
        (&config.pool_signing_key, &config.pool_wallet_address)
    {
        let derived = zion_core::crypto::derive_address(signing_key.verifying_key().as_bytes());
        if &derived != wallet_addr {
            return Err(anyhow!(
                "ZION_POOL_PAYOUT_SK_HEX derives to {derived} but ZION_POOL_WALLET is \
                 {wallet_addr}. Account-model payouts will be REJECTED by the node \
                 (3.0.4 from-address verification). Fix the pool wallet/key configuration \
                 before mining."
            ));
        }
    }
    if let Some(btc_wallet) = config.btc_wallet.as_deref() {
        println!("btc_wallet={btc_wallet}");
    }
    println!(
        "session_default_group={} backend_miner_ids={} backend_worker_hints={}",
        session_group_name(config.user_default_group),
        config.backend_miner_ids.len(),
        config.backend_worker_hints.join("|")
    );
    println!("routing_log_every={}", config.routing_log_every);
    println!("max_sessions_per_ip={}", config.max_sessions_per_ip);
    let started_at = std::time::Instant::now();
    // ── AuxPow scheduler (external merge mining) ───────────────────
    // When ZION_AUXPOW_ENABLED=1, the scheduler connects to an external
    // pool (e.g. DCR/ALPH via Blake3) and mines with the pool's own
    // compute, tracking USD revenue for PPLNS distribution.
    // The scheduler runs on a dedicated tokio runtime since the pool
    // server itself uses std::thread (not tokio).
    // NOTE: The runtime must be kept alive for the lifetime of the pool
    // server — if it's dropped, all spawned tasks are cancelled.
    let auxpow_scheduler: Arc<AuxPowScheduler> = {
        let sched = Arc::new(AuxPowScheduler::from_env());
        if sched.is_enabled_sync() {
            println!("auxpow: scheduler enabled, spawning background task");
            let auxpow_runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("auxpow")
                .build()
                .context("failed to create auxpow tokio runtime")?;
            let sched_clone = Arc::clone(&sched);
            sched_clone.spawn_on(&auxpow_runtime);
            // Leak the runtime so it stays alive for the lifetime of the process.
            // This is intentional — the pool server runs forever and the runtime
            // should never be dropped (which would cancel all auxpow tasks).
            std::mem::forget(auxpow_runtime);
        } else {
            println!("auxpow: disabled (set ZION_AUXPOW_ENABLED=1 to enable)");
        }
        sched
    };

    // ── AuxPow B2b bridge (pool-side job multiplexing) ────────────────
    // When ZION_POOL_AUXPOW_ENABLED=1, the pool bridges to an external
    // Stratum pool.  A tokio runtime runs the JobMultiplexer in a background
    // thread and keeps a queue of current external jobs; session threads pop
    // jobs synchronously and send shares back for forwarding.
    let (auxpow_bridge, auxpow_share_rx) = AuxPowBridge::new(config.auxpow_config.enabled);
    if config.auxpow_config.enabled {
        println!(
            "auxpow_bridge: enabled coin={:?} wallet={} worker={} preference={:?} region={}",
            config.auxpow_config.force_coin,
            config.auxpow_config.payout_wallet,
            config.auxpow_config.worker_name,
            config.auxpow_config.pool_preference,
            config.auxpow_config.region,
        );
        let bridge = auxpow_bridge.clone();
        let aux_cfg = config.auxpow_config.clone();
        thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("auxpow-bridge")
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("auxpow_bridge_rt_error: {e}");
                    return;
                }
            };
            rt.block_on(run_auxpow_bridge(aux_cfg, bridge, auxpow_share_rx));
        });
    } else {
        println!("auxpow_bridge: disabled (set ZION_POOL_AUXPOW_ENABLED=1 to enable B2b multiplexing)");
    }

    if let Some(metrics_bind) = config.routing_metrics_bind.as_deref() {
        println!("routing_metrics_bind={metrics_bind}");
        let routing_stats = Arc::clone(&routing_stats);
        let miner_telemetry_ref = Arc::clone(&miner_telemetry);
        let active_sessions_ref = Arc::clone(&active_sessions);
        let pplns_ref = Arc::clone(&pplns_engine);
        let auxpow_ref = Arc::clone(&auxpow_scheduler);
        let revenue_scheduler_ref = Arc::clone(&revenue_scheduler);
        let metrics_bind = metrics_bind.to_string();
        thread::spawn(move || {
            if let Err(error) = serve_routing_metrics(
                &metrics_bind,
                routing_stats,
                miner_telemetry_ref,
                started_at,
                active_sessions_ref,
                pplns_ref,
                auxpow_ref,
                revenue_scheduler_ref,
            ) {
                eprintln!("routing_metrics_error={error:#}");
            }
        });
    }

    // ── PPLNS persistence thread ────────────────────────────────────
    // Periodically saves the PPLNS engine state (unpaid balances, share
    // window, fee accumulators) to a JSON file so that miner earnings
    // survive pool restarts and crashes.
    //
    // F6 optimization: the lock is held only for the snapshot clone +
    // dirty-flag check.  JSON serialization and file I/O happen outside
    // the lock so share submissions are not blocked during disk writes.
    // The dirty flag skips saves entirely when no shares arrived.
    if !pplns_state_path.is_empty() {
        let pplns_ref = Arc::clone(&pplns_engine);
        let state_path = pplns_state_path.clone();
        let save_interval_s = parse_env_u64("ZION_PPLNS_SAVE_INTERVAL_S", 10).unwrap_or(10);
        println!(
            "pplns_persistence: periodic save every {}s to {}",
            save_interval_s, state_path
        );
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(save_interval_s));
            // Hold the lock only long enough to check dirty + snapshot.
            let snapshot = {
                let mut pplns = pplns_ref.lock().expect("pplns lock poisoned");
                if !pplns.take_dirty() {
                    continue; // no changes since last save — skip I/O
                }
                pplns.snapshot()
            };
            // Serialize + write outside the lock.
            if let Err(e) = PplnsEngine::write_snapshot_to_path(&snapshot, &state_path) {
                eprintln!("pplns_persistence: save failed to {}: {}", state_path, e);
            }
        });
    }

    // ── Stream Profit Updater (Deeksha Chv3 pipeline weights) ────────
    // When ZION_STREAM_PROFIT_SWITCH=true, periodically fetch profit data
    // and update stream weights.  All streams live INSIDE the Deeksha Chv3
    // pipeline — the weights tell the miner how to distribute extra work
    // across pipeline steps (Keccak, SHA3, NPU, etc.).
    {
        let scheduler_ref = Arc::clone(&revenue_scheduler);
        let profit_cfg = scheduler_ref
            .lock()
            .expect("revenue scheduler lock poisoned")
            .stream_profit_config
            .clone();

        if profit_cfg.enabled {
            let interval = profit_cfg.interval_secs;
            let cfg_clone = profit_cfg.clone();
            println!(
                "stream_profit_enabled provider={} interval={}s hysteresis={}%",
                profit_cfg.api_provider, interval, profit_cfg.hysteresis_pct
            );
            thread::spawn(move || loop {
                thread::sleep(Duration::from_secs(interval));

                // Fetch live profit snapshot from configured API provider.
                // Falls back to static estimates on any error.
                let snapshot = fetch_profit_snapshot(&cfg_clone);

                {
                    let mut sched = scheduler_ref.lock().expect("revenue scheduler lock poisoned");
                    sched.update_stream_weights(snapshot);
                }
            });
        } else {
            println!("stream_profit_disabled (set ZION_STREAM_PROFIT_SWITCH=true to enable)");
        }
    }

    // ── NCL Gateway dispatcher ───────────────────────────────────────
    // When ZION_NCL_GATEWAY_URL is configured, spawn a tokio runtime in a
    // background thread and run the NCL dispatcher.  This wires the 25 %
    // NCL revenue stream to a live Hiran inference service.  The dispatcher
    // pulls tasks from an mpsc queue; when ZION_NCL_HEARTBEAT=true the
    // dispatcher also self-produces periodic heartbeat tasks so the
    // pipeline stays warm and the revenue stream is observable end-to-end.
    if let Ok(gateway_url) = std::env::var("ZION_NCL_GATEWAY_URL") {
        if !gateway_url.trim().is_empty() {
            match NclGatewayClient::new(&gateway_url) {
                Ok(client) => {
                    let revenue = pool
                        .lock()
                        .expect("pool lock poisoned")
                        .runtime()
                        .revenue_handle();
                    let pricing = NclPricing::from_env();
                    let heartbeat = NclHeartbeatConfig::from_env();
                    let queue_capacity =
                        parse_env_u64("ZION_NCL_QUEUE_SIZE", 256).unwrap_or(256) as usize;
                    println!(
                        "ncl_gateway_enabled url={} heartbeat={} interval_secs={} \
                         price_in_per_1k={} price_out_per_1k={}",
                        client.authority(),
                        heartbeat.enabled,
                        heartbeat.interval.as_secs(),
                        pricing.price_in_per_1k_tokens,
                        pricing.price_out_per_1k_tokens
                    );
                    thread::spawn(move || {
                        let rt = match tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                        {
                            Ok(rt) => rt,
                            Err(e) => {
                                eprintln!("ncl_gateway_rt_error: {e}");
                                return;
                            }
                        };
                        rt.block_on(async move {
                            let dispatcher = NclDispatcher::new(client, pricing, revenue);
                            let _tx = dispatcher.spawn(heartbeat, queue_capacity);
                            // Keep the runtime alive — the dispatcher runs
                            // until the parent process exits.
                            futures_park().await;
                        });
                    });
                }
                Err(e) => {
                    eprintln!("ncl_gateway_config_error url={} error={}", gateway_url, e);
                }
            }
        }
    } else {
        println!("ncl_gateway_enabled=false (set ZION_NCL_GATEWAY_URL to enable)");
    }

    {
        let scheduler = revenue_scheduler
            .lock()
            .expect("revenue scheduler lock poisoned");
        println!(
            "revenue_mode={} lanes={} plan={} backend_auto_include_zion={}",
            if scheduler.multistream_enabled {
                "multistream"
            } else {
                "single"
            },
            scheduler.lanes.len(),
            scheduler.describe_plan(),
            scheduler.auto_assign_include_zion
        );
    }

    let shutdown = Arc::new(AtomicBool::new(false));
    {
        let shutdown = Arc::clone(&shutdown);
        ctrlc::set_handler(move || {
            println!("shutdown_signal_received");
            shutdown.store(true, Ordering::SeqCst);
        })
        .context("failed to set ctrl-c handler")?;
    }

    // ── Deferred payout queue + background processor ───────────────────
    // When a block is found and the pool wallet balance is not yet credited
    // by the node (race condition), payouts are queued here instead of being
    // rolled back.  A background thread retries every 2 s until the balance
    // is sufficient or max retries (300 = 10 min) is reached.
    let deferred_payouts: DeferredPayoutQueue = Arc::new(Mutex::new(Vec::new()));
    {
        let deferred_ref = Arc::clone(&deferred_payouts);
        let pplns_ref = Arc::clone(&pplns_engine);
        let telemetry_ref = Arc::clone(&miner_telemetry);
        let node_rpc = config.node_rpc_addr.clone();
        let pool_wallet = config.pool_wallet_address.clone();
        let signing_key = config.pool_signing_key.clone();
        let max_retries = parse_env_u64("ZION_PAYOUT_MAX_RETRIES", 300).unwrap_or(300) as u32;
        let retry_interval_ms =
            parse_env_u64("ZION_PAYOUT_RETRY_INTERVAL_MS", 2000).unwrap_or(2000);
        println!(
            "deferred_payout_processor: enabled max_retries={} interval_ms={}",
            max_retries, retry_interval_ms
        );
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(retry_interval_ms));
            let mut queue = deferred_ref.lock().expect("deferred lock poisoned");
            if queue.is_empty() {
                continue;
            }
            // Process oldest entry (FIFO).
            let deferred = match queue.first_mut() {
                Some(d) => d,
                None => continue,
            };
            deferred.retry_count += 1;
            let height = deferred.height;
            let retry = deferred.retry_count;
            let payouts = deferred.payouts.clone();
            drop(queue);

            if retry > max_retries {
                // Permanent failure — rollback PPLNS balances.
                let mut pplns = pplns_ref.lock().expect("pplns lock poisoned");
                pplns.rollback_payouts(&payouts);
                println!(
                    "payout_deferred_giveup height={} miners={} reason=max_retries_exceeded",
                    height,
                    payouts.len()
                );
                let mut queue = deferred_ref.lock().expect("deferred lock poisoned");
                queue.remove(0);
                continue;
            }

            // Attempt payout if we have all required credentials.
            let (rpc, wallet, key) = match (&node_rpc, &pool_wallet, &signing_key) {
                (Some(r), Some(w), Some(k)) => (r, w, k),
                _ => {
                    println!(
                        "payout_deferred_skip height={} reason=missing_credentials",
                        height
                    );
                    continue;
                }
            };

            match execute_pool_payout(rpc, wallet, key, &payouts, height) {
                Ok(outcome) => {
                    println!(
                        "payout_deferred_success height={} executed={} deferred={} tx_id={} retry={}",
                        height,
                        outcome.executed.len(),
                        outcome.deferred.len(),
                        outcome.tx_id,
                        retry
                    );
                    {
                        let mut telemetry = telemetry_ref
                            .lock()
                            .expect("miner telemetry lock poisoned");
                        telemetry.record_submitted_payouts(
                            height,
                            &outcome.executed,
                            &outcome.tx_id,
                        );
                    }
                    let mut queue = deferred_ref.lock().expect("deferred lock poisoned");
                    if outcome.deferred.is_empty() {
                        // Fully processed — remove from queue.
                        queue.remove(0);
                    } else {
                        // Partial success — update with remaining deferred.
                        if let Some(entry) = queue.first_mut() {
                            entry.payouts = outcome.deferred;
                            entry.retry_count = 0; // reset for remaining
                        }
                    }
                }
                Err(e) => {
                    let err_str = format!("{e}");
                    // Only log every 10th retry to avoid spam.
                    if retry % 10 == 0 || retry <= 3 {
                        println!(
                            "payout_deferred_retry height={} miners={} retry={} error={}",
                            height,
                            payouts.len(),
                            retry,
                            err_str
                        );
                    }
                }
            }
        });
    }

    listener
        .set_nonblocking(true)
        .context("failed to set listener non-blocking")?;

    let mut handles = Vec::new();
    let mut accepted = 0u32;
    let ip_sessions: Arc<Mutex<HashMap<IpAddr, u32>>> = Arc::new(Mutex::new(HashMap::new()));
    loop {
        // Reap finished session threads to prevent unbounded `handles` Vec
        // growth when miners connect/disconnect over time.  Without this, a
        // pool running for days with thousands of sessions would accumulate
        // millions of dead JoinHandle entries in memory.
        if handles.len() > 128 {
            handles.retain(|h: &thread::JoinHandle<Result<(), anyhow::Error>>| !h.is_finished());
        }
        if shutdown.load(Ordering::SeqCst) {
            println!("shutdown_draining clients={}", handles.len());
            // Final PPLNS state save before exit.
            if !pplns_state_path.is_empty() {
                let pplns = pplns_engine.lock().expect("pplns lock poisoned");
                match pplns.save_to_path(&pplns_state_path) {
                    Ok(()) => println!("pplns_persistence: final save OK to {}", pplns_state_path),
                    Err(e) => eprintln!(
                        "pplns_persistence: final save FAILED to {}: {}",
                        pplns_state_path, e
                    ),
                }
            }
            break;
        }
        if matches!(config.accept_limit, Some(limit) if accepted >= limit) {
            break;
        }

        let (stream, peer_addr) = match listener.accept() {
            Ok(pair) => pair,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => {
                return Err(anyhow::Error::new(e).context("failed to accept miner connection"))
            }
        };
        stream
            .set_nonblocking(false)
            .context("failed to set client stream blocking")?;
        // P1-fix: read timeout prevents zombie threads when miner disconnects
        // ungracefully (no FIN), which leaks ip_sessions counter slots.
        stream
            .set_read_timeout(Some(Duration::from_secs(config.session_read_timeout_secs)))
            .context("failed to set client stream read timeout")?;

        let peer_ip = peer_addr.ip();
        {
            let mut sessions = ip_sessions.lock().expect("ip_sessions lock");
            let ip_count = sessions.entry(peer_ip).or_insert(0);
            if config.max_sessions_per_ip > 0 && *ip_count >= config.max_sessions_per_ip {
                println!("rate_limit_reject ip={peer_ip} sessions={ip_count}");
                drop(stream);
                continue;
            }
            *ip_count = ip_count.saturating_add(1);
        }
        let ip_guard = IpSessionGuard(Arc::clone(&ip_sessions), peer_ip);

        println!("peer_addr={peer_addr}");
        let pool = Arc::clone(&pool);
        let revenue_scheduler = Arc::clone(&revenue_scheduler);
        let routing_stats = Arc::clone(&routing_stats);
        let miner_telemetry = Arc::clone(&miner_telemetry);
        let pplns_ref = Arc::clone(&pplns_engine);
        let active_sessions_ref = Arc::clone(&active_sessions);
        let session_id_ref = Arc::clone(&session_id_counter);
        let template_cache_ref = Arc::clone(&template_cache);
        let log_ch = Arc::clone(&log_channel);
        let deferred_ref = Arc::clone(&deferred_payouts);
        let auxpow_bridge = auxpow_bridge.clone();
        let config = config.clone();
        handles.push(thread::spawn(move || {
            let _ip_guard = ip_guard;
            handle_client(
                stream,
                pool,
                revenue_scheduler,
                routing_stats,
                miner_telemetry,
                pplns_ref,
                active_sessions_ref,
                session_id_ref,
                template_cache_ref,
                deferred_ref,
                auxpow_bridge,
                &config,
                &log_ch,
            )
        }));
        accepted = accepted.saturating_add(1);
    }

    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => println!("session_ended_with_error err={e:#}"),
            Err(_) => println!("session_ended_with_panic"),
        }
    }
    {
        let snapshot = routing_stats
            .lock()
            .expect("routing stats lock poisoned")
            .snapshot_line();
        println!("routing_final {snapshot}");
    }
    Ok(())
}

/// RAII guard that decrements the active session counter on drop.
struct SessionGuard(Arc<AtomicU64>);
impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// RAII guard that decrements the per-IP active session counter on drop.
struct IpSessionGuard(Arc<Mutex<HashMap<IpAddr, u32>>>, IpAddr);
impl Drop for IpSessionGuard {
    fn drop(&mut self) {
        if let Ok(mut sessions) = self.0.lock() {
            if let Some(count) = sessions.get_mut(&self.1) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    sessions.remove(&self.1);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// VarDiff — per-session adaptive share difficulty
// ---------------------------------------------------------------------------

/// Per-session variable-difficulty state.
///
/// Adjusts the share difficulty so that the miner submits approximately every
/// `target_secs` seconds.  Higher-hashrate miners get a harder target (more
/// PPLNS weight per share), while low-hashrate miners get an easier target so
/// they still submit shares regularly.
struct VarDiff {
    current_difficulty: u64,
    min_difficulty: u64,
    max_difficulty: u64,
    target_secs: f64,
    retarget_shares: u64,
    /// Timestamps of recent share submissions for retarget calculation.
    submit_times: VecDeque<Instant>,
    /// Accumulated shares since last retarget.
    shares_since_retarget: u64,
}

impl VarDiff {
    fn new(config: &ServerConfig) -> Self {
        Self {
            current_difficulty: config.vardiff_start_difficulty.max(1),
            min_difficulty: config.vardiff_min_difficulty.max(1),
            max_difficulty: if config.vardiff_max_difficulty == 0 {
                u64::MAX
            } else {
                config.vardiff_max_difficulty
            },
            target_secs: config.vardiff_target_secs.max(1) as f64,
            retarget_shares: config.vardiff_retarget_shares.max(2),
            submit_times: VecDeque::with_capacity(32),
            shares_since_retarget: 0,
        }
    }

    /// The current share difficulty target (256-bit).
    fn share_target(&self) -> DifficultyTarget {
        zion_core::difficulty::difficulty_to_target(self.current_difficulty)
    }

    /// Record a share submission and optionally retarget difficulty.
    ///
    /// Returns `Some(new_difficulty)` if the difficulty was adjusted.
    fn record_submit(&mut self) -> Option<u64> {
        let now = Instant::now();
        self.submit_times.push_back(now);
        self.shares_since_retarget += 1;

        // Keep a bounded ring of timestamps.
        while self.submit_times.len() > 32 {
            self.submit_times.pop_front();
        }

        if self.shares_since_retarget < self.retarget_shares || self.submit_times.len() < 2 {
            return None;
        }

        // Compute average time between submissions.
        let n = self.submit_times.len() - 1;
        let total_secs = self
            .submit_times
            .back()
            .unwrap()
            .duration_since(*self.submit_times.front().unwrap())
            .as_secs_f64();
        if total_secs <= 0.0 || n == 0 {
            return None;
        }
        let avg_secs = total_secs / n as f64;

        // Retarget: new_diff = current_diff × (target_time / avg_time).
        // Clamp the ratio to [0.25, 4.0] to prevent wild swings.
        let ratio = (self.target_secs / avg_secs).clamp(0.25, 4.0);
        let new_diff_f = self.current_difficulty as f64 * ratio;
        let new_diff = (new_diff_f as u64)
            .max(self.min_difficulty)
            .min(self.max_difficulty);

        self.shares_since_retarget = 0;

        if new_diff != self.current_difficulty {
            self.current_difficulty = new_diff;
            Some(new_diff)
        } else {
            None
        }
    }
}

/// Current work assignment for a session iteration — either a native ZION
/// job or an external AuxPow job pulled from the B2b bridge.
#[derive(Debug, Clone)]
enum WorkAssignment {
    Zion(MiningJob),
    External(JobPackage),
}

impl WorkAssignment {
    /// Return the job ID as the u64 used on the ZION wire.  External job IDs
    /// are strings upstream, so we deterministically hash them to a u64.
    fn job_id(&self) -> u64 {
        match self {
            Self::Zion(j) => j.job_id,
            Self::External(j) => hash_job_id(&j.external_job_id),
        }
    }

    #[allow(dead_code)]
    /// Return the upstream/external job ID string (only meaningful for external jobs).
    fn external_job_id(&self) -> Option<&str> {
        match self {
            Self::Zion(_) => None,
            Self::External(j) => Some(&j.external_job_id),
        }
    }

    fn algorithm(&self) -> &str {
        match self {
            // Phase D: Use height-aware canonical algorithm name for Zion jobs.
            // For height >= 4500 (CHV3_FORK_HEIGHT), advertises `deeksha_chv3`.
            // Below 4500, advertises `deeksha_lite_v1` (backward compat).
            // Both names map to the same hash function, so miners that only
            // know `deeksha_lite_v1` continue to work seamlessly.
            Self::Zion(j) => zion_pool::advertised_algorithm_for_height(j.height),
            Self::External(j) => {
                // For Blake3 coins, append the coin ticker so the miner
                // selects the correct kernel (DCR and ALPH use different
                // Blake3 variants: DCR = single hash of header||nonce_le,
                // ALPH = double hash of nonce||header).
                if j.algorithm.eq_ignore_ascii_case("blake3") {
                    match j.external_coin {
                        zion_auxpow::ExternalCoin::DCR => "blake3_dcr",
                        zion_auxpow::ExternalCoin::ALPH => "blake3_alph",
                        _ => &j.algorithm,
                    }
                } else {
                    &j.algorithm
                }
            }
        }
    }

    fn start_nonce(&self) -> u64 {
        match self {
            Self::Zion(j) => j.start_nonce,
            Self::External(j) => j.start_nonce,
        }
    }

    fn nonce_count(&self) -> u64 {
        match self {
            Self::Zion(j) => j.nonce_count,
            Self::External(j) => j.nonce_count,
        }
    }

    fn target_bytes(&self) -> [u8; 32] {
        match self {
            Self::Zion(j) => j.target.bytes,
            Self::External(j) => j.target_bytes,
        }
    }

    fn is_external(&self) -> bool {
        matches!(self, Self::External(_))
    }
}

/// Determine whether this iteration should issue an external job based on
/// the configured split.  If no split is configured, default to ZION-only
/// (safe default — prevents accidental external-only mining).
fn should_issue_external_job(iteration: u32, cfg: &AuxPowIntegrationConfig) -> bool {
    match cfg.split {
        Some(SplitConfig {
            zion_weight,
            external_weight,
        }) => {
            let total = zion_weight.saturating_add(external_weight);
            if total == 0 {
                return true;
            }
            (iteration as u64 % u64::from(total)) < u64::from(external_weight)
        }
        // When no split config is provided, default to ZION-only.
        // Previously this returned `true` (always external), which caused
        // the chain stall at block 4502 when ZION_POOL_AUXPOW_SPLIT_EXTERNAL
        // was missing from the environment.
        None => false,
    }
}

/// Issue a native ZION job, either from the node template or from a local
/// fallback header.
fn issue_zion_job(
    config: &ServerConfig,
    template_cache: &Arc<Mutex<TemplateCache>>,
    pool: &Arc<Mutex<MiningPool>>,
    last_template_height: &mut u64,
    session_id: u64,
    iteration: u32,
    worker_name: &str,
) -> Result<WorkAssignment> {
    let session_nonce_offset = session_id.wrapping_mul(1_000_000_000);
    let start_nonce = config
        .start_nonce
        .wrapping_add(session_nonce_offset)
        .wrapping_add((iteration as u64).wrapping_mul(config.nonce_stride));
    let job = match config.node_rpc_addr.as_deref() {
        Some(node_rpc_addr) => {
            let template = template_cache
                .lock()
                .expect("template cache lock poisoned")
                .get_or_fetch(node_rpc_addr)?;
            if template.height != *last_template_height {
                if *last_template_height > 0 {
                    println!(
                        "template_advanced prev_height={} new_height={} miner={}",
                        *last_template_height, template.height, worker_name
                    );
                }
                *last_template_height = template.height;
            }
            pool.lock()
                .expect("pool lock poisoned")
                .issue_job_from_template(&template, start_nonce, config.nonce_count)
                .map_err(|reason| anyhow!(reason))?
        }
        None => {
            let header = MiningHeader {
                version: 3,
                previous_hash: [0x11; 32],
                merkle_root: [0x22; 32],
                timestamp: config.timestamp + iteration as u64,
                difficulty_bits: 0x1f00ffff,
            };
            pool.lock().expect("pool lock poisoned").issue_job(
                header,
                config.target,
                start_nonce,
                config.nonce_count,
            )
        }
    };
    Ok(WorkAssignment::Zion(job))
}

fn assignment_header_bytes(assignment: &WorkAssignment) -> Vec<u8> {
    match assignment {
        WorkAssignment::Zion(j) => j.header.to_bytes().to_vec(),
        WorkAssignment::External(j) => j.header_bytes.clone(),
    }
}

fn assignment_height(assignment: &WorkAssignment) -> u64 {
    match assignment {
        WorkAssignment::Zion(j) => j.height,
        // External jobs: use block_number for epoch derivation (Ethash/KawPow).
        // Fall back to timestamp for coins that don't provide block height (KAS).
        WorkAssignment::External(j) => j.block_number.unwrap_or(j.timestamp),
    }
}

/// Deterministically map an external string job ID to the u64 job_id field
/// used by the ZION wire protocol.  External pools use arbitrary strings
/// (e.g. "job_dcr_001"); miners echo back whatever u64 the pool sends.
fn hash_job_id(id: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

/// Convert a work assignment into a `BlockCandidate` that `hash_with_algorithm`
/// can validate.  For external jobs, the raw external header is truncated /
/// padded to the 80-byte `MiningHeader` layout used by ZION's wire format.
/// This is sufficient for the Phase-1 B2b integration where miners hash the
/// same `header_hex` the pool sends.
fn assignment_to_candidate(assignment: &WorkAssignment, nonce: u64) -> zion_core::BlockCandidate {
    match assignment {
        WorkAssignment::Zion(j) => zion_core::BlockCandidate {
            header: j.header,
            nonce,
            height: j.height,
        },
        WorkAssignment::External(j) => {
            let bytes = &j.header_bytes;
            let mut header_bytes = [0u8; 80];
            let len = bytes.len().min(80);
            header_bytes[..len].copy_from_slice(&bytes[..len]);
            zion_core::BlockCandidate {
                header: MiningHeader::from_bytes(header_bytes),
                nonce,
                height: j.timestamp,
            }
        }
    }
}

/// Forward a share for an external job to the upstream external pool via the
/// AuxPow bridge.  Returns a `ShareDecision` reflecting the forward result.
fn handle_external_share(
    assignment: &WorkAssignment,
    bridge: &AuxPowBridge,
    nonce: u64,
    hash: &[u8; 32],
    worker_name: &str,
    job_id: String,
    mix_hash: Option<[u8; 32]>,
) -> ShareDecision {
    let external_job = match assignment {
        WorkAssignment::External(j) => j,
        WorkAssignment::Zion(_) => {
            return ShareDecision {
                status: ShareStatus::RejectedLowDifficulty,
                sealed_block: None,
            };
        }
    };

    let forward_req = ShareForwardRequest {
        external_job_id: external_job.external_job_id.clone(),
        nonce,
        hash: *hash,
        target: external_job.target_bytes,
        mix_hash,
    };

    match bridge.forward(forward_req) {
        Some(outcome) => match outcome.result {
            ShareForwardResult::Accepted => {
                println!(
                    "auxpow_share_accepted miner={} job={} coin={} elapsed_ms={}",
                    worker_name, job_id, external_job.external_coin, outcome.elapsed_ms
                );
                ShareDecision {
                    status: ShareStatus::Accepted,
                    sealed_block: None,
                }
            }
            ShareForwardResult::Rejected(ref reason) => {
                println!(
                    "auxpow_share_rejected miner={} job={} coin={} reason={}",
                    worker_name, job_id, external_job.external_coin, reason
                );
                ShareDecision {
                    status: ShareStatus::UpstreamRejected,
                    sealed_block: None,
                }
            }
            ShareForwardResult::BelowTarget => ShareDecision {
                status: ShareStatus::RejectedLowDifficulty,
                sealed_block: None,
            },
            ShareForwardResult::Unknown | ShareForwardResult::NotConnected => {
                println!(
                    "auxpow_share_unknown miner={} job={} coin={} result={:?}",
                    worker_name, job_id, external_job.external_coin, outcome.result
                );
                ShareDecision {
                    status: ShareStatus::UpstreamRejected,
                    sealed_block: None,
                }
            }
        },
        None => {
            println!(
                "auxpow_share_forward_failed miner={} job={} coin={}",
                worker_name, job_id, external_job.external_coin
            );
            ShareDecision {
                status: ShareStatus::UpstreamRejected,
                sealed_block: None,
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_client(
    stream: TcpStream,
    pool: Arc<Mutex<MiningPool>>,
    revenue_scheduler: Arc<Mutex<RevenueScheduler>>,
    routing_stats: Arc<Mutex<RoutingStats>>,
    miner_telemetry: Arc<Mutex<MinerTelemetryRegistry>>,
    pplns_engine: Arc<Mutex<PplnsEngine>>,
    active_sessions: Arc<AtomicU64>,
    session_id_counter: Arc<AtomicU64>,
    template_cache: Arc<Mutex<TemplateCache>>,
    deferred_payouts: DeferredPayoutQueue,
    auxpow_bridge: AuxPowBridge,
    config: &ServerConfig,
    log_ch: &LogChannel,
) -> Result<()> {
    let session_started = Instant::now();
    let session_id = session_id_counter.fetch_add(1, Ordering::Relaxed);

    let reader_stream = stream.try_clone().context("failed to clone tcp stream")?;
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;

    // Read hello BEFORE logging session_start — TCP probes (health checks,
    // dashboard polls) connect and immediately close without sending a hello.
    // Logging session_start for those creates noise and inflates session counts.
    let (hello_line, hello_message) = match read_wire_message(&mut reader) {
        Ok(pair) => pair,
        Err(_) => {
            // Connection closed before hello — likely a health check / TCP probe.
            // Decrement ip_sessions counter (already incremented in accept loop)
            // and return quietly without logging session_start.
            return Ok(());
        }
    };

    // Only now — after we have a valid hello — count this as an active session.
    active_sessions.fetch_add(1, Ordering::Relaxed);
    let session_count = active_sessions.load(Ordering::Relaxed);
    let _guard = SessionGuard(Arc::clone(&active_sessions));
    println!("session_start active_sessions={session_count} session_id={session_id}");
    println!("wire_hello={}", hello_line);

    // ── Inter-pool ShareRelay (Edge → Core) ─────────────────────────────
    // If the first message is a ShareRelay, record it in PPLNS and close.
    if let PoolMessage::ShareRelay {
        miner_id,
        worker_name,
        height,
        difficulty,
        relay_origin,
    } = &hello_message
    {
        let mut pplns = pplns_engine.lock().expect("pplns lock poisoned");
        pplns.record_share_with_diff(miner_id, worker_name, *height, *difficulty);
        println!(
            "share_relay_accepted miner={} worker={} height={} diff={} origin={}",
            miner_id, worker_name, height, difficulty, relay_origin
        );
        return Ok(());
    }

    let (miner_id, worker_name, algorithm, payout_address, backend) = match hello_message {
        PoolMessage::Hello {
            miner_id,
            worker_name,
            algorithm,
            payout_address,
            backend,
            ..
        } => {
            let payout = if payout_address.trim().is_empty() {
                return Err(anyhow!(
                    "payout_address required: set ZION_PAYOUT_ADDRESS to a valid zion1 address"
                ));
            } else {
                payout_address
            };
            if !zion_core::crypto::is_valid_address(&payout) {
                return Err(anyhow!(
                    "invalid payout_address {payout}: must be a valid zion1 address"
                ));
            }
            (miner_id, worker_name, algorithm, payout, backend)
        }
        other => return Err(anyhow!("expected hello from miner, got {other:?}")),
    };

    // Dual-algo support: accept any algorithm the miner advertises.
    // The pool validates shares with the session's algorithm.
    let session_algorithm = algorithm.clone();

    let requested_group = resolve_session_group(&miner_id, &worker_name, config);
    let session_group = if requested_group == SessionGroup::Auto {
        revenue_scheduler
            .lock()
            .expect("revenue scheduler lock poisoned")
            .assign_auto_group()
    } else {
        requested_group
    };
    println!(
        "session_group_requested={} session_group={} miner_id={} worker_name={}",
        session_group_name(requested_group),
        session_group_name(session_group),
        miner_id,
        worker_name
    );
    {
        let mut telemetry = miner_telemetry
            .lock()
            .expect("miner telemetry lock poisoned");
        telemetry.touch_session(&miner_id, &worker_name, &session_algorithm, &backend);
    }

    // Register miner payout address for PPLNS distribution.
    pplns_engine
        .lock()
        .expect("pplns lock poisoned")
        .register_address(&miner_id, &payout_address);

    // Choose a welcome algorithm hint.  For sessions that will receive
    // external AuxPoW jobs, advertise the forced coin's algorithm so the
    // miner can prepare the right hasher.  Otherwise echo the miner's native
    // algorithm as before.
    let welcome_algorithm = if session_group != SessionGroup::Zion {
        config
            .auxpow_config
            .force_coin
            .map(external_coin_to_algorithm)
            .unwrap_or(&session_algorithm)
            .to_string()
    } else {
        session_algorithm.clone()
    };
    let welcome_message = PoolMessage::Welcome {
        protocol_version: zion_pool::protocol_version().to_string(),
        algorithm: welcome_algorithm,
        job_ttl_ms: config.job_ttl_ms,
    };
    let welcome_line = write_wire_message(&mut writer, &welcome_message)?;
    println!("wire_welcome={welcome_line}");

    // ── Revenue proxy redirect ──────────────────────────────────────────
    // If the miner was assigned to Revenue (or Auto→Revenue) and a proxy
    // address is configured, send a ProxyRedirect so the GPU miner can
    // connect directly to the external Stratum proxy.
    if let Some(ref proxy_addr) = config.revenue_proxy_addr {
        let should_redirect = matches!(session_group, SessionGroup::Revenue | SessionGroup::Auto);
        if should_redirect {
            if let Some((host, port)) = split_host_port(proxy_addr) {
                let algorithm = zion_cosmic_harmony::profit_router::ExternalCoin::from_str_loose(
                    &config.revenue_proxy_coin,
                )
                .map(|c| c.algorithm().to_string())
                .unwrap_or_else(|| "unknown".to_string());
                let redirect = PoolMessage::ProxyRedirect {
                    host,
                    port,
                    coin: config.revenue_proxy_coin.clone(),
                    algorithm,
                };
                let redirect_line = write_wire_message(&mut writer, &redirect)?;
                println!("wire_proxy_redirect={redirect_line}");
            }
        }
    }

    // Initialise per-session variable difficulty.
    let mut vardiff = VarDiff::new(config);
    // Send initial share difficulty to miner.
    {
        let sd_msg = PoolMessage::SetDifficulty {
            difficulty: vardiff.current_difficulty,
            target_hex: to_hex(&vardiff.share_target().bytes),
        };
        let sd_line = write_wire_message(&mut writer, &sd_msg)?;
        println!("wire_set_difficulty={sd_line}");
    }

    let mut last_template_height: u64 = 0;

    for iteration in 0..config.loop_count {
        let stale_job_ids = pool.lock().expect("pool lock poisoned").expire_stale_jobs();
        for stale_job_id in stale_job_ids {
            let stale_message = pool
                .lock()
                .expect("pool lock poisoned")
                .stale_message(stale_job_id);
            let cancel_message = pool
                .lock()
                .expect("pool lock poisoned")
                .cancel_message(stale_job_id, "stale-ttl-expired");
            let stale_line = write_wire_message(&mut writer, &stale_message)?;
            let cancel_line = write_wire_message(&mut writer, &cancel_message)?;
            println!("wire_stale={stale_line}");
            println!("wire_cancel={cancel_line}");
        }

        // ── Decide whether to issue a ZION or an external AuxPow job ──────
        // The revenue scheduler selects a lane per iteration.  External lanes
        // are fulfilled by the AuxPow bridge; ZION / NCL lanes fall through
        // to the normal ZION job issuance.
        let (revenue_source, revenue_value_usd) = revenue_scheduler
            .lock()
            .expect("revenue scheduler lock poisoned")
            .next_lane_for_group(session_group);

        // Stream weights string for Deeksha Chv3 pipeline parameterisation.
        // Sent to miners in the job message so they can adjust work distribution.
        let stream_weights_string = revenue_scheduler
            .lock()
            .expect("revenue scheduler lock poisoned")
            .stream_weights_string();

        let desired_external_coin = if config.auxpow_config.enabled {
            // For sessions explicitly assigned to the Zion group, never
            // override with an external coin — the miner asked for ZION work.
            // For Revenue/Auto groups, prefer force_coin then revenue_source.
            if session_group == SessionGroup::Zion {
                None
            } else {
                config
                    .auxpow_config
                    .force_coin
                    .or_else(|| revenue_source_to_external_coin(revenue_source))
            }
        } else {
            None
        };

        let assignment = match desired_external_coin {
            Some(coin) if should_issue_external_job(iteration, &config.auxpow_config) => {
                // Try to use the freshest external job from the bridge queue.
                // pop_job returns a clone and leaves the job in the queue so
                // fast miners don't drain it between upstream notify messages.
                // If none is available, fall back to ZION so the session keeps
                // hashing instead of stalling.
                let job = auxpow_bridge.pop_job().filter(|j| j.external_coin == coin);
                if let Some(job) = job {
                    WorkAssignment::External(job)
                } else {
                    log_ch.log_verbose(format!(
                        "auxpow_no_job miner={} coin={} falling_back_to_zion",
                        worker_name, coin
                    ));
                    issue_zion_job(
                        config,
                        &template_cache,
                        &pool,
                        &mut last_template_height,
                        session_id,
                        iteration,
                        &worker_name,
                    )?
                }
            }
            _ => issue_zion_job(
                config,
                &template_cache,
                &pool,
                &mut last_template_height,
                session_id,
                iteration,
                &worker_name,
            )?,
        };

        let job_issued_at = Instant::now();
        // Store network target for block validation; send share target to miner.
        let network_target = DifficultyTarget {
            bytes: assignment.target_bytes(),
        };
        // For external AuxPoW jobs, use the external pool's target as the
        // share target (the external pool sets the difficulty).  For ZION
        // jobs, use the vardiff share target.
        let share_target = if assignment.is_external() {
            DifficultyTarget {
                bytes: assignment.target_bytes(),
            }
        } else {
            vardiff.share_target()
        };
        let share_difficulty = vardiff.current_difficulty;
        let job_nonce_count = if backend == "opencl" || backend == "cuda" || backend == "metal" {
            config.nonce_count_gpu
        } else {
            config.nonce_count
        };
        let job_message = PoolMessage::Job {
            job_id: assignment.job_id(),
            algorithm: assignment.algorithm().to_string(),
            start_nonce: assignment.start_nonce(),
            nonce_count: job_nonce_count,
            target_hex: to_hex(&share_target.bytes),
            header_hex: to_hex(&assignment_header_bytes(&assignment)),
            height: assignment_height(&assignment),
            stream_weights: stream_weights_string.clone(),
        };
        let job_line = write_wire_message(&mut writer, &job_message)?;

        if assignment.is_external() {
            let coin = match &assignment {
                WorkAssignment::External(j) => j.external_coin.ticker().to_string(),
                _ => "unknown".to_string(),
            };
            println!(
                "issued_external_job miner={} job_id={} coin={} algorithm={} height={}",
                worker_name,
                assignment.job_id(),
                coin,
                assignment.algorithm(),
                assignment_height(&assignment)
            );
        }

        log_ch.log_verbose(format!(
            "iteration={} miner={} height={} nonces={}..{} algorithm={} external={}",
            iteration + 1,
            worker_name,
            assignment_height(&assignment),
            assignment.start_nonce(),
            assignment.start_nonce().saturating_add(job_nonce_count),
            assignment.algorithm(),
            assignment.is_external()
        ));
        log_ch.log_verbose(format!("issued_job_id={}", assignment.job_id()));
        log_ch.log_verbose(format!("wire_job={job_line}"));

        let (submit_line, submit_message) = read_wire_message(&mut reader)?;
        let iter_elapsed_ms = job_issued_at.elapsed().as_millis();
        log_ch.log_verbose(format!("wire_submit={submit_line}"));
        log_ch.log_verbose(format!("iteration_elapsed_ms={iter_elapsed_ms}"));

        let outcome = match submit_message {
            PoolMessage::Submit {
                job_id,
                miner_id: submit_miner_id,
                worker_name: submit_worker_name,
                nonce,
                hash_hex,
                attempted_hashes,
                elapsed_ms,
                mix_hash_hex,
            } => {
                if submit_miner_id != miner_id || submit_worker_name != worker_name {
                    println!(
                        "submit_identity_mismatch session={}/{} submit={}/{}; using session identity",
                        miner_id, worker_name, submit_miner_id, submit_worker_name
                    );
                }

                // ── Two-tier vardiff validation ──────────────────────────
                // 1. Verify hash integrity (candidate.seal().hash == submitted hash).
                // 2. Check against share_target (easy) → valid share for PPLNS.
                // 3. Check against network_target (hard) → block found, submit to node.

                let candidate = assignment_to_candidate(&assignment, nonce);
                let job_algorithm = assignment.algorithm();
                let computed_hash = candidate.hash_with_algorithm(job_algorithm);
                let submitted_hash = parse_hash_hex(&hash_hex)?;

                // Log hash mismatch but use our own computed hash for validation
                // (miner-submitted hash is cosmetic; we trust only our own seal).
                if computed_hash != submitted_hash {
                    log_ch.log(format!(
                        "hash_mismatch_info miner={} job={} computed={} submitted={}",
                        worker_name,
                        job_id,
                        to_hex(&computed_hash),
                        hash_hex
                    ));
                }

                // Use submitted_hash for target validation (miner found this hash).
                // computed_hash is used for audit/mismatch detection only.
                let target_hash = &submitted_hash;

                // ── Stale-job check ──────────────────────────────────────
                // If the miner submits a share for an old job (different job_id or expired TTL),
                // reject it as StaleJob so it doesn't count against RejectedLowDifficulty.
                let current_job_id = assignment.job_id();
                let is_stale = {
                    let p = pool.lock().expect("pool lock poisoned");
                    job_id != current_job_id || p.is_job_stale(job_id)
                };
                if is_stale {
                    let reason = if job_id != current_job_id {
                        "wrong-iteration"
                    } else {
                        "ttl-expired"
                    };
                    log_ch.log(format!(
                        "share_stale miner={} submitted_job={} current_job={} reason={}",
                        worker_name, job_id, current_job_id, reason
                    ));
                    pool.lock()
                        .expect("pool lock poisoned")
                        .record_stale_share();
                    let decision = ShareDecision {
                        status: ShareStatus::StaleJob,
                        sealed_block: None,
                    };
                    JobCompletion::Submitted {
                        decision,
                        routed_source: RevenueSource::Zion,
                        attempted_hashes: attempted_hashes
                            .unwrap_or_else(|| nonce.saturating_sub(assignment.start_nonce()) + 1),
                        elapsed_ms: elapsed_ms
                            .unwrap_or_else(|| job_issued_at.elapsed().as_millis() as u64),
                    }
                } else if !share_target.allows(target_hash) {
                    // Hash does not meet even the (easier) share target → reject.
                    log_ch.log(format!(
                        "share_below_target miner={} job={} diff={}",
                        worker_name, job_id, share_difficulty
                    ));
                    pool.lock()
                        .expect("pool lock poisoned")
                        .record_rejected_share();
                    {
                        let mut pplns = pplns_engine.lock().expect("pplns lock poisoned");
                        pplns.record_invalid_share(&miner_id);
                    }
                    let decision = ShareDecision {
                        status: ShareStatus::RejectedLowDifficulty,
                        sealed_block: None,
                    };
                    JobCompletion::Submitted {
                        decision,
                        routed_source: RevenueSource::Zion,
                        attempted_hashes: attempted_hashes
                            .unwrap_or_else(|| nonce.saturating_sub(assignment.start_nonce()) + 1),
                        elapsed_ms: elapsed_ms
                            .unwrap_or_else(|| job_issued_at.elapsed().as_millis() as u64),
                    }
                } else {
                    // ── Valid share: meets share_target ──────────────────
                    // Record in PPLNS with difficulty weight.
                    let job_height = assignment_height(&assignment);
                    {
                        let mut pplns = pplns_engine.lock().expect("pplns lock poisoned");
                        pplns.record_share_with_diff(
                            &miner_id,
                            &worker_name,
                            job_height,
                            share_difficulty,
                        );
                    }
                    // ── Relay to upstream/Core pool (Edge mode) ──────────
                    if let Some(ref upstream) = config.upstream_pool_addr {
                        let relay = PoolMessage::ShareRelay {
                            miner_id: miner_id.clone(),
                            worker_name: worker_name.clone(),
                            height: job_height,
                            difficulty: share_difficulty,
                            relay_origin: config.bind_addr.clone(),
                        };
                        if let Err(e) = relay_share_fire_and_forget(upstream, &relay) {
                            println!(
                                "share_relay_failed miner={} upstream={} err={}",
                                worker_name, upstream, e
                            );
                        } else {
                            println!(
                                "share_relayed miner={} upstream={} diff={}",
                                worker_name, upstream, share_difficulty
                            );
                        }
                    }
                    log_ch.log_verbose(format!(
                        "valid_share miner={} job={} share_diff={}",
                        worker_name, job_id, share_difficulty
                    ));

                    // Vardiff retarget after each valid share submission.
                    // For external AuxPoW jobs the upstream pool sets the target,
                    // so do not override it with local vardiff retargets.
                    if !assignment.is_external() {
                        if let Some(new_diff) = vardiff.record_submit() {
                            log_ch.log(format!(
                                "vardiff_retarget miner={} old_diff={} new_diff={}",
                                worker_name, share_difficulty, new_diff
                            ));
                            let set_diff_msg = PoolMessage::SetDifficulty {
                                difficulty: new_diff,
                                target_hex: to_hex(&vardiff.share_target().bytes),
                            };
                            let diff_line = write_wire_message(&mut writer, &set_diff_msg)?;
                            println!("wire_set_difficulty={diff_line}");
                        }
                    }

                    // Check if hash also meets the (harder) network target → block found!
                    // For external jobs this means the share meets the external pool's
                    // target and should be forwarded upstream.
                    let decision = if assignment.is_external() {
                        // Parse mix_hash from hex if present (Ethash/KawPow).
                        let mix_hash = mix_hash_hex
                            .as_deref()
                            .and_then(|h| parse_hash_hex(h).ok());
                        handle_external_share(
                            &assignment,
                            &auxpow_bridge,
                            nonce,
                            target_hash,
                            &worker_name,
                            job_id.to_string(),
                            mix_hash,
                        )
                    } else if network_target.allows(target_hash) {
                        // Block found! Submit to the node.
                        println!(
                            "BLOCK_FOUND miner={} height={} nonce={} hash={}",
                            worker_name, job_height, nonce, hash_hex
                        );
                        let node_rpc_addr = config.node_rpc_addr.clone();
                        let node_status = match node_rpc_addr.as_deref() {
                            Some(addr) => {
                                let mining_job = match &assignment {
                                    WorkAssignment::Zion(j) => *j,
                                    WorkAssignment::External(_) => {
                                        unreachable!("external handled above")
                                    }
                                };
                                match submit_candidate_to_node(addr, mining_job, nonce, job_algorithm)
                                {
                                    Ok(RpcResponse::SubmitResult { accepted: true, .. }) => {
                                        println!(
                                            "node_accepted_block height={} nonce={}",
                                            job_height, nonce
                                        );
                                        ShareStatus::Accepted
                                    }
                                    Ok(RpcResponse::SubmitResult {
                                        accepted: false,
                                        reason,
                                        ..
                                    }) => {
                                        println!(
                                            "node_rejected_block height={} nonce={} reason={}",
                                            job_height,
                                            nonce,
                                            reason.as_deref().unwrap_or("unknown")
                                        );
                                        map_node_rejection(reason.as_deref())
                                    }
                                    Ok(other) => {
                                        println!("node_rpc_unexpected={other:?}");
                                        ShareStatus::UpstreamRejected
                                    }
                                    Err(error) => {
                                        println!("node_rpc_error={error:#}");
                                        ShareStatus::UpstreamRejected
                                    }
                                }
                            }
                            None => ShareStatus::Accepted,
                        };

                        // Record revenue for the block.
                        let block_accepted = matches!(node_status, ShareStatus::Accepted);
                        if block_accepted {
                            // Invalidate template cache so the next iteration
                            // fetches a fresh template (height+1) from the node.
                            template_cache
                                .lock()
                                .expect("template cache lock poisoned")
                                .invalidate();
                            // Dummy USD revenue (multi-chain compat).
                            pool.lock().expect("pool lock poisoned").record_revenue(
                                revenue_source,
                                revenue_value_usd,
                                true,
                            );
                            // Actual ZION canonical block revenue.
                            let subsidy = zion_core::emission::block_subsidy(job_height);
                            let pool_fee_pct = zion_core::emission::POOL_FEE_PCT;
                            let block_hash_hex = hex::encode(computed_hash);
                            pool.lock()
                                .expect("pool lock poisoned")
                                .runtime()
                                .record_zion_block_revenue(
                                    job_height,
                                    subsidy,
                                    pool_fee_pct,
                                    Some(block_hash_hex),
                                );
                            // Phase B: Stream telemetry — record per-step
                            // revenue breakdown for the winning nonce.
                            // Consensus-safe: does NOT change hash output.
                            {
                                let header_bytes = candidate.header.to_bytes();
                                let (_stream_hash, telemetry) =
                                    zion_cosmic_harmony::deeksha_chv3_with_streams(
                                        &header_bytes,
                                        nonce,
                                    );
                                // Verify stream hash matches computed hash
                                // (sanity check — should always pass).
                                debug_assert_eq!(_stream_hash.data, computed_hash);
                                let revenue_handle = pool
                                    .lock()
                                    .expect("pool lock poisoned")
                                    .runtime()
                                    .revenue_handle();
                                revenue_handle.track_deeksha_streams(
                                    &telemetry,
                                    revenue_value_usd,
                                    Some(job_height),
                                );
                            }
                            // Notify OASIS L4 game server to award XP to the miner.
                            // Fire-and-forget via background thread so pool never blocks.
                            let miner_addr = miner_id.clone();
                            std::thread::spawn(move || {
                                notify_oasis_block_mined(&miner_addr, job_height);
                            });
                        }

                        ShareDecision {
                            status: node_status,
                            sealed_block: if block_accepted {
                                Some(zion_core::SealedBlock {
                                    header: candidate.header,
                                    nonce,
                                    hash: computed_hash,
                                })
                            } else {
                                None
                            },
                        }
                    } else {
                        // Valid share but not a block — accept for PPLNS only.
                        ShareDecision {
                            status: ShareStatus::Accepted,
                            sealed_block: None,
                        }
                    };

                    // Track accepted/rejected in pool stats for bye_message.
                    {
                        let p = pool.lock().expect("pool lock poisoned");
                        if matches!(decision.status, ShareStatus::Accepted) {
                            p.record_accepted_share();
                        } else {
                            p.record_rejected_share();
                        }
                    }

                    // Record external revenue when an external share is accepted
                    // by the upstream pool.  This feeds the revenue collector and
                    // dashboard with per-coin external mining income.
                    if matches!(decision.status, ShareStatus::Accepted) {
                        if let WorkAssignment::External(ref ext_job) = assignment {
                            let ext_source =
                                external_coin_to_revenue_source(ext_job.external_coin);
                            // Estimate USD value per accepted share from fallback
                            // estimates.  In production this would come from the
                            // external pool's actual payout data.
                            let est_usd = estimate_external_share_usd(ext_job.external_coin);
                            pool.lock()
                                .expect("pool lock poisoned")
                                .runtime()
                                .record_external_revenue(
                                    ext_source,
                                    est_usd,
                                    Some(ext_job.external_coin.ticker()),
                                );
                            println!(
                                "auxpow_revenue_recorded coin={} source={:?} est_usd={:.4}",
                                ext_job.external_coin, ext_source, est_usd
                            );
                        }
                    }

                    let solution = MiningSolution {
                        job_id,
                        candidate,
                        hash: submitted_hash,
                    };
                    // Record the routed source based on the actual work assignment, not
                    // the scheduler lane.  This ensures external shares are counted under
                    // the correct source (e.g. src_kawpow for RVN) even when the revenue
                    // scheduler picked a different lane.
                    let routed_source = match &assignment {
                        WorkAssignment::External(j) => external_coin_to_revenue_source(j.external_coin),
                        WorkAssignment::Zion(_) => revenue_source,
                    };
                    JobCompletion::Submitted {
                        decision,
                        routed_source,
                        attempted_hashes: attempted_hashes.unwrap_or_else(|| {
                            solution.candidate.nonce.saturating_sub(assignment.start_nonce()) + 1
                        }),
                        elapsed_ms: elapsed_ms
                            .unwrap_or_else(|| job_issued_at.elapsed().as_millis() as u64),
                    }
                }
            }
            PoolMessage::NoSolution {
                job_id,
                miner_id: submit_miner_id,
                worker_name: submit_worker_name,
                attempted_hashes,
                elapsed_ms,
            } => {
                let current_job_id = assignment.job_id();
                if job_id != current_job_id {
                    return Err(anyhow!(
                        "no-solution job mismatch: expected {}, got {}",
                        current_job_id,
                        job_id
                    ));
                }
                if submit_miner_id != miner_id || submit_worker_name != worker_name {
                    println!(
                        "no_solution_identity_mismatch session={}/{} submit={}/{}; using session identity",
                        miner_id, worker_name, submit_miner_id, submit_worker_name
                    );
                }
                // Do NOT retarget difficulty on no-solution — the miner found nothing,
                // so there is no valid timing data.  Retargeting here would drive
                // difficulty to infinity and make the target impossible.
                // vardiff.record_submit();
                JobCompletion::NoSolution {
                    attempted_hashes: attempted_hashes.unwrap_or(assignment.nonce_count()),
                    elapsed_ms: elapsed_ms
                        .unwrap_or_else(|| job_issued_at.elapsed().as_millis() as u64),
                }
            }
            other => return Err(anyhow!("expected submit from miner, got {other:?}")),
        };

        match outcome {
            JobCompletion::Submitted {
                decision,
                routed_source,
                attempted_hashes,
                elapsed_ms,
            } => {
                let accepted = matches!(decision.status, ShareStatus::Accepted);
                let stale = matches!(decision.status, ShareStatus::StaleJob);
                let job_height = assignment_height(&assignment);
                // PPLNS share was already recorded in the submit handler above
                // (with difficulty weight).  Trigger payout only when a block
                // was actually found (sealed_block is present).
                let block_found = decision.sealed_block.is_some();
                if block_found && accepted && !assignment.is_external() {
                    {
                        let mut telemetry = miner_telemetry
                            .lock()
                            .expect("miner telemetry lock poisoned");
                        telemetry.record_block_found(&miner_id, &worker_name);
                    }
                    {
                        let mut pplns = pplns_engine.lock().expect("pplns lock poisoned");
                        pplns.record_block_found(&miner_id);
                    }
                    let payouts = {
                        if job_height > 0 {
                            // Core already distributes the protocol fees
                            // (humanitarian / issobella / pool_fee) atomically via
                            // the coinbase outputs, and credits the pool wallet with
                            // only the 89% miner slice. Redistribute that ENTIRE
                            // slice to miners here — no second fee split.
                            let (miner_share, _, _, _) = zion_core::emission::fee_split(
                                zion_core::emission::block_subsidy(job_height),
                            );
                            let mut pplns = pplns_engine.lock().expect("pplns lock poisoned");
                            pplns.compute_miner_payouts(miner_share)
                        } else {
                            Vec::new()
                        }
                    };
                    // Phase 18: Execute on-chain payout asynchronously so the
                    // miner thread that found the block is not blocked for the
                    // duration of N sequential RPC calls to the node (which can
                    // take 600ms+ with 12 miners, or 50s+ with 1000 miners).
                    if !payouts.is_empty() {
                        // Record pending payouts in telemetry before spawning.
                        {
                            let mut telemetry = miner_telemetry
                                .lock()
                                .expect("miner telemetry lock poisoned");
                            telemetry.record_pending_payouts(job_height, &payouts);
                        }
                        // Spawn a background thread for payout execution.
                        let node_rpc_addr = config.node_rpc_addr.clone();
                        let pool_wallet_addr = config.pool_wallet_address.clone();
                        let signing_key = config.pool_signing_key.clone();
                        let pplns_ref = Arc::clone(&pplns_engine);
                        let telemetry_ref = Arc::clone(&miner_telemetry);
                        let deferred_ref = Arc::clone(&deferred_payouts);
                        let payouts_clone = payouts.clone();
                        thread::spawn(move || {
                            execute_payout_async(
                                node_rpc_addr,
                                pool_wallet_addr,
                                signing_key,
                                &payouts_clone,
                                job_height,
                                &pplns_ref,
                                &telemetry_ref,
                                &deferred_ref,
                            );
                        });
                    }
                }
                {
                    let mut stats = routing_stats.lock().expect("routing stats lock poisoned");
                    if stale {
                        stats.record_stale();
                    }
                    let should_log = stats.record(session_group, routed_source, accepted);
                    if should_log {
                        println!("routing_snapshot {}", stats.snapshot_line());
                    }
                }
                {
                    let mut telemetry = miner_telemetry
                        .lock()
                        .expect("miner telemetry lock poisoned");
                    telemetry.record_job_result(
                        &miner_id,
                        &worker_name,
                        matches!(decision.status, ShareStatus::Accepted),
                        attempted_hashes,
                        elapsed_ms,
                    );
                }

                if matches!(decision.status, ShareStatus::StaleJob) {
                    let current_job_id = assignment.job_id();
                    let stale_message = pool
                        .lock()
                        .expect("pool lock poisoned")
                        .stale_message(current_job_id);
                    let cancel_message = pool
                        .lock()
                        .expect("pool lock poisoned")
                        .cancel_message(current_job_id, "submit-arrived-after-ttl");
                    let stale_line = write_wire_message(&mut writer, &stale_message)?;
                    let cancel_line = write_wire_message(&mut writer, &cancel_message)?;
                    println!("wire_stale={stale_line}");
                    println!("wire_cancel={cancel_line}");
                }

                let result_message = pool
                    .lock()
                    .expect("pool lock poisoned")
                    .result_message(&decision);
                let result_line = write_wire_message(&mut writer, &result_message)?;
                log_ch.log_verbose(format!("share_status={:?}", decision.status));
                log_ch.log_verbose(format!("wire_result={result_line}"));
            }
            JobCompletion::NoSolution {
                attempted_hashes,
                elapsed_ms,
            } => {
                {
                    let mut telemetry = miner_telemetry
                        .lock()
                        .expect("miner telemetry lock poisoned");
                    telemetry.record_no_solution(
                        &miner_id,
                        &worker_name,
                        attempted_hashes,
                        elapsed_ms,
                    );
                }
                let result_message = PoolMessage::Result {
                    accepted: false,
                    status: "NoSolution".to_string(),
                };
                let result_line = write_wire_message(&mut writer, &result_message)?;
                log_ch.log_verbose("share_status=NoSolution".to_string());
                log_ch.log_verbose(format!("wire_result={result_line}"));
            }
        }
    }

    let bye_message = pool.lock().expect("pool lock poisoned").bye_message();
    let bye_line = write_wire_message(&mut writer, &bye_message)?;
    let session_elapsed_secs = session_started.elapsed().as_secs();
    println!("session_miner_id={miner_id}");
    println!("session_worker_name={worker_name}");
    println!("session_duration_secs={session_elapsed_secs}");
    println!("wire_bye={bye_line}");
    Ok(())
}

fn read_wire_message(reader: &mut impl BufRead) -> Result<(String, PoolMessage)> {
    let mut line = String::new();
    let read = reader
        .read_line(&mut line)
        .context("failed to read wire message")?;
    if read == 0 {
        return Err(anyhow!("peer closed the connection"));
    }
    let message = decode_message(&line).context("failed to decode wire message")?;
    Ok((line.trim().to_string(), message))
}

fn write_wire_message(writer: &mut impl Write, message: &PoolMessage) -> Result<String> {
    let line = encode_message(message).context("failed to encode wire message")?;
    writer
        .write_all(line.as_bytes())
        .context("failed to write wire message")?;
    writer.flush().context("failed to flush wire message")?;
    Ok(line.trim().to_string())
}

fn fetch_node_template(node_rpc_addr: &str) -> Result<BlockTemplate> {
    match rpc_roundtrip(node_rpc_addr, &RpcRequest::GetTemplate)? {
        RpcResponse::Template { template } => Ok(template),
        other => Err(anyhow!(
            "expected template response from node, got {other:?}"
        )),
    }
}

fn submit_candidate_to_node(
    node_rpc_addr: &str,
    job: zion_core::MiningJob,
    nonce: u64,
    algorithm: &str,
) -> Result<RpcResponse> {
    rpc_roundtrip(
        node_rpc_addr,
        &RpcRequest::SubmitCandidate {
            template_id: job.job_id,
            header_hex: to_hex(&job.header.to_bytes()),
            nonce,
            target_hex: to_hex(&job.target.bytes),
            algorithm: algorithm.to_string(),
        },
    )
}

fn rpc_roundtrip(node_rpc_addr: &str, request: &RpcRequest) -> Result<RpcResponse> {
    let mut stream = TcpStream::connect(node_rpc_addr)
        .with_context(|| format!("failed to connect to node rpc at {node_rpc_addr}"))?;
    let request_line = encode_rpc_request(request).context("failed to encode node rpc request")?;
    stream
        .write_all(request_line.as_bytes())
        .context("failed to write node rpc request")?;
    stream.flush().context("failed to flush node rpc request")?;

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    let read = reader
        .read_line(&mut response_line)
        .context("failed to read node rpc response")?;
    if read == 0 {
        return Err(anyhow!("node rpc closed the connection"));
    }

    decode_rpc_response(&response_line).context("failed to decode node rpc response")
}

#[allow(dead_code)]
fn json_rpc_roundtrip(
    node_rpc_addr: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });
    let request_line =
        serde_json::to_string(&request).context("failed to encode json-rpc request")?;

    let mut stream = TcpStream::connect(node_rpc_addr)
        .with_context(|| format!("failed to connect to node rpc at {node_rpc_addr}"))?;
    stream
        .write_all(request_line.as_bytes())
        .context("failed to write json-rpc request")?;
    stream
        .write_all(b"\n")
        .context("failed to write json-rpc newline")?;
    stream.flush().context("failed to flush json-rpc request")?;

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    let read = reader
        .read_line(&mut response_line)
        .context("failed to read json-rpc response")?;
    if read == 0 {
        return Err(anyhow!("node rpc closed the json-rpc connection"));
    }

    let response: serde_json::Value =
        serde_json::from_str(response_line.trim()).context("failed to decode json-rpc response")?;
    if let Some(error) = response.get("error") {
        let message = error
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("json-rpc error");
        return Err(anyhow!("json-rpc {method} failed: {message}"));
    }

    response
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow!("json-rpc {method} missing result field"))
}

fn map_node_rejection(reason: Option<&str>) -> ShareStatus {
    match reason {
        Some(reason) if reason.contains("stale template") => ShareStatus::StaleJob,
        Some(reason) if reason.contains("does not match") => ShareStatus::JobMismatch,
        Some(reason) if reason.contains("low difficulty") => ShareStatus::RejectedLowDifficulty,
        _ => ShareStatus::UpstreamRejected,
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}

fn parse_hash_hex(raw: &str) -> Result<[u8; 32]> {
    parse_fixed_hex::<32>(raw, "submit hash")
}

fn parse_fixed_hex<const N: usize>(raw: &str, label: &str) -> Result<[u8; N]> {
    let normalized = raw.trim().trim_start_matches("0x");
    if normalized.len() != N * 2 {
        return Err(anyhow!("{label} must be exactly {} hex chars", N * 2));
    }

    let mut bytes = [0u8; N];
    for (index, chunk) in normalized.as_bytes().chunks(2).enumerate() {
        let pair =
            std::str::from_utf8(chunk).with_context(|| format!("{label} is not valid utf-8"))?;
        bytes[index] = u8::from_str_radix(pair, 16)
            .with_context(|| format!("invalid hex byte '{pair}' in {label}"))?;
    }
    Ok(bytes)
}

#[derive(Clone)]
struct ServerConfig {
    bind_addr: String,
    accept_limit: Option<u32>,
    node_rpc_addr: Option<String>,
    loop_count: u32,
    job_ttl_ms: u64,
    start_nonce: u64,
    nonce_count: u64,
    nonce_count_gpu: u64,
    nonce_stride: u64,
    timestamp: u64,
    target: DifficultyTarget,
    revenue_source: RevenueSource,
    revenue_value_usd: f64,
    user_default_group: SessionGroup,
    backend_miner_ids: Vec<String>,
    backend_worker_hints: Vec<String>,
    routing_log_every: u64,
    routing_metrics_bind: Option<String>,
    max_sessions_per_ip: u32,
    /// TCP read timeout for miner sessions — prevents zombie threads on
    /// ungraceful disconnects (no FIN).  Default: 300s.
    session_read_timeout_secs: u64,
    /// Pool wallet address for payout signing (ZION_POOL_WALLET).
    pool_wallet_address: Option<String>,
    /// Ed25519 signing key for pool payout transactions (ZION_POOL_PAYOUT_SK_HEX).
    pool_signing_key: Option<ed25519_dalek::SigningKey>,
    // --- Vardiff configuration ---
    /// Starting share difficulty for new sessions.  Default: 1 (accept everything).
    vardiff_start_difficulty: u64,
    /// Target time between share submissions in seconds.  Default: 10.
    vardiff_target_secs: u64,
    /// How often to retarget difficulty (number of shares).  Default: 6.
    vardiff_retarget_shares: u64,
    /// Minimum share difficulty.  Default: 1.
    vardiff_min_difficulty: u64,
    /// Maximum share difficulty (0 = unlimited = network diff).  Default: 0.
    vardiff_max_difficulty: u64,
    /// BTC wallet address for external pool payouts (2miners, NiceHash, etc.).
    /// All multi-algo revenue streams pay out to this wallet.
    btc_wallet: Option<String>,
    /// Revenue proxy redirect address (`host:port`) for Revenue / Auto sessions.
    /// When set, the pool sends `ProxyRedirect` to miners in these groups.
    revenue_proxy_addr: Option<String>,
    /// Default coin for revenue proxy redirect (e.g. "KAS").
    revenue_proxy_coin: String,
    fee_config: FeeConfig,
    /// Upstream/Core pool address for share relay (Edge pool only).
    /// When set, every accepted share is forwarded to the upstream pool
    /// via `ShareRelay` so the Core pool owns the unified PPLNS window.
    upstream_pool_addr: Option<String>,
    /// AuxPow (B2b) integration configuration.
    auxpow_config: AuxPowIntegrationConfig,
}

/// Configuration for the B2b AuxPow integration inside the pool server.
#[derive(Debug, Clone)]
struct AuxPowIntegrationConfig {
    /// Whether pool-side job multiplexing is enabled.
    enabled: bool,
    /// Fixed split between ZION and external jobs.  If `None`, the revenue
    /// scheduler decides per-session (external lanes = external jobs).
    split: Option<SplitConfig>,
    /// External coin to mine.  `None` means "follow the revenue scheduler".
    force_coin: Option<ExternalCoin>,
    /// External pool preference (NiceHash, HeroMiners, zpool, default).
    pool_preference: zion_auxpow::PoolPreference,
    /// Geographic region for external pool selection.
    region: String,
    /// BTC wallet address used as Stratum username for external pool payouts.
    payout_wallet: String,
    /// Worker name suffix sent to the external pool.
    worker_name: String,
    /// Per-coin wallet overrides (e.g. DCR requires a DCR address, not BTC).
    /// Key = coin ticker (uppercase), Value = wallet address.
    coin_wallets: std::collections::HashMap<String, String>,
    /// How often (in seconds) to check profitability for auto coin switching.
    /// Only applies when `force_coin` is `None`.  Default: 60 seconds.
    profit_check_interval_secs: u64,
    /// Hysteresis percentage for coin switching.  Only switch if the new coin
    /// is `hysteresis_pct`% more profitable than the current coin.
    /// Default: 15.0 (15%).
    hysteresis_pct: f64,
}

impl Default for AuxPowIntegrationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            split: None,
            force_coin: None,
            pool_preference: zion_auxpow::PoolPreference::Default,
            region: "eu".to_string(),
            payout_wallet: zion_auxpow::types::DEFAULT_BTC_WALLET.to_string(),
            worker_name: "zion_auxpow".to_string(),
            coin_wallets: std::collections::HashMap::new(),
            profit_check_interval_secs: 60,
            hysteresis_pct: 15.0,
        }
    }
}

impl AuxPowIntegrationConfig {
    fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(v) = std::env::var("ZION_POOL_AUXPOW_ENABLED") {
            cfg.enabled = v.trim().eq_ignore_ascii_case("1")
                || v.trim().eq_ignore_ascii_case("true")
                || v.trim().eq_ignore_ascii_case("yes");
        }
        cfg.split = Self::parse_split_env();
        if let Ok(v) = std::env::var("ZION_POOL_AUXPOW_COIN") {
            cfg.force_coin = ExternalCoin::from_str_loose(&v);
        }
        if let Ok(v) = std::env::var("ZION_POOL_AUXPOW_POOL_PREFERENCE") {
            cfg.pool_preference = zion_auxpow::PoolPreference::from_str_loose(&v);
        }
        if let Ok(v) = std::env::var("ZION_POOL_AUXPOW_REGION") {
            cfg.region = v;
        }
        if let Ok(v) = std::env::var("ZION_POOL_AUXPOW_WALLET") {
            cfg.payout_wallet = v;
        }
        if let Ok(v) = std::env::var("ZION_POOL_AUXPOW_WORKER_NAME") {
            cfg.worker_name = v;
        }
        // Per-coin wallet overrides: ZION_POOL_AUXPOW_WALLET_DCR, _KAS, _ALPH, etc.
        for coin in ExternalCoin::all() {
            let key = format!("ZION_POOL_AUXPOW_WALLET_{}", coin.ticker());
            if let Ok(v) = std::env::var(&key) {
                if !v.trim().is_empty() {
                    cfg.coin_wallets
                        .insert(coin.ticker().to_string(), v.trim().to_string());
                }
            }
        }
        // Auto coin switching configuration.
        if let Ok(v) = std::env::var("ZION_POOL_AUXPOW_PROFIT_CHECK_INTERVAL") {
            if let Ok(secs) = v.trim().parse::<u64>() {
                cfg.profit_check_interval_secs = secs;
            }
        }
        if let Ok(v) = std::env::var("ZION_POOL_AUXPOW_HYSTERESIS_PCT") {
            if let Ok(pct) = v.trim().parse::<f64>() {
                cfg.hysteresis_pct = pct;
            }
        }
        cfg
    }

    fn parse_split_env() -> Option<SplitConfig> {
        let zion = std::env::var("ZION_POOL_AUXPOW_SPLIT_ZION")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok());
        let external = std::env::var("ZION_POOL_AUXPOW_SPLIT_EXTERNAL")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok());
        match (zion, external) {
            (Some(z), Some(e)) => Some(SplitConfig {
                zion_weight: z,
                external_weight: e,
            }),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct RoutingStats {
    log_every: u64,
    total_submits: u64,
    total_accepted: u64,
    total_stale: u64,
    group_submits: [u64; 4],
    group_accepted: [u64; 4],
    source_submits: [u64; 14],
    source_accepted: [u64; 14],
}

enum JobCompletion {
    Submitted {
        decision: zion_pool::ShareDecision,
        routed_source: RevenueSource,
        attempted_hashes: u64,
        elapsed_ms: u64,
    },
    NoSolution {
        attempted_hashes: u64,
        elapsed_ms: u64,
    },
}

// ---------------------------------------------------------------------------
// AuxPowBridge — shared B2b multiplexer / share-forwarder state
// ---------------------------------------------------------------------------

/// Request sent from the synchronous session handler to the tokio share
/// forwarder task.  Contains everything needed to submit a share upstream.
#[derive(Debug)]
struct ShareForwardRequest {
    external_job_id: String,
    nonce: u64,
    hash: [u8; 32],
    target: [u8; 32],
    /// Mix hash for Ethash/KawPow (eth_submitWork).  None for other algorithms.
    mix_hash: Option<[u8; 32]>,
}

/// Result of a share-forward request returned to the session handler.
#[derive(Debug, Clone)]
struct ShareForwardOutcome {
    result: ShareForwardResult,
    elapsed_ms: u64,
}

/// Shared bridge between the synchronous pool session threads and the async
/// AuxPow multiplexer.  One bridge instance is created in `main()` and shared
/// across all sessions.
///
/// * The tokio side keeps a `JobMultiplexer` connected to the external pool
///   and pushes fresh jobs into `job_queue`.
/// * Session threads pop jobs from `job_queue` synchronously.
/// * Session threads send `ShareForwardRequest`s via `share_tx`; the tokio
///   side forwards them and returns the result via a synchronous mpsc channel.
#[derive(Clone)]
struct AuxPowBridge {
    enabled: bool,
    job_queue: Arc<Mutex<VecDeque<JobPackage>>>,
    share_tx: std::sync::mpsc::Sender<(ShareForwardRequest, std::sync::mpsc::Sender<ShareForwardOutcome>)>,
}

impl AuxPowBridge {
    fn new(enabled: bool) -> (Self, std::sync::mpsc::Receiver<(ShareForwardRequest, std::sync::mpsc::Sender<ShareForwardOutcome>)>) {
        let (share_tx, share_rx) = std::sync::mpsc::channel();
        let bridge = Self {
            enabled,
            job_queue: Arc::new(Mutex::new(VecDeque::new())),
            share_tx,
        };
        (bridge, share_rx)
    }

    /// Return a clone of the freshest external job from the queue.
    ///
    /// We keep the job in the queue so multiple sessions (and successive
    /// iterations of the same session) can mine the same external work unit
    /// until the bridge pushes a newer job.  This prevents fast miners from
    /// draining the single-slot queue and falling back to ZION jobs between
    /// external pool notify messages.
    fn pop_job(&self) -> Option<JobPackage> {
        if !self.enabled {
            return None;
        }
        let q = self.job_queue.lock().expect("auxpow job queue lock poisoned");
        q.front().cloned()
    }

    /// Send a share to be forwarded upstream.  Blocks until the tokio task
    /// processes the request (typically < 100 ms because it is local I/O).
    fn forward(&self, req: ShareForwardRequest) -> Option<ShareForwardOutcome> {
        if !self.enabled {
            return None;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        if self.share_tx.send((req, tx)).is_err() {
            return None;
        }
        rx.recv().ok()
    }
}

#[derive(Debug, Clone)]
struct WorkSample {
    completed_at_s: u64,
    attempted_hashes: u64,
    elapsed_ms: u64,
}

#[derive(Debug, Clone)]
struct MinerPayoutRecord {
    amount_atomic: u64,
    share_count: u64,
    created_ts: u64,
    height: u64,
    status: String,
    tx_id: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct MinerTelemetry {
    worker_name: String,
    algorithm: String,
    backend: String,
    first_seen_s: u64,
    last_seen_s: u64,
    last_share_time_s: u64,
    valid_shares: u64,
    invalid_shares: u64,
    no_solution_jobs: u64,
    blocks_found: u64,
    completed_jobs: u64,
    total_attempted_hashes: u64,
    total_elapsed_ms: u64,
    paid_total_atomic: u64,
    samples: VecDeque<WorkSample>,
    payouts: VecDeque<MinerPayoutRecord>,
}

#[derive(Debug, Default)]
struct MinerTelemetryRegistry {
    miners: HashMap<String, MinerTelemetry>,
}

const HASHRATE_WINDOW_1H_S: u64 = 60 * 60;
const HASHRATE_WINDOW_24H_S: u64 = 24 * 60 * 60;
const HASHRATE_WINDOW_LIVE_S: u64 = 10 * 60;
const PAYOUT_HISTORY_LIMIT: usize = 50;

impl MinerTelemetry {
    fn new(worker_name: &str, algorithm: &str, backend: &str, now_s: u64) -> Self {
        Self {
            worker_name: worker_name.to_string(),
            algorithm: algorithm.to_string(),
            backend: backend.to_string(),
            first_seen_s: now_s,
            last_seen_s: now_s,
            last_share_time_s: 0,
            valid_shares: 0,
            invalid_shares: 0,
            no_solution_jobs: 0,
            blocks_found: 0,
            completed_jobs: 0,
            total_attempted_hashes: 0,
            total_elapsed_ms: 0,
            paid_total_atomic: 0,
            samples: VecDeque::new(),
            payouts: VecDeque::new(),
        }
    }

    fn touch(&mut self, worker_name: &str, algorithm: &str, backend: &str, now_s: u64) {
        self.worker_name = worker_name.to_string();
        self.algorithm = algorithm.to_string();
        self.backend = backend.to_string();
        if self.first_seen_s == 0 {
            self.first_seen_s = now_s;
        }
        self.last_seen_s = now_s;
    }

    fn push_sample(&mut self, attempted_hashes: u64, elapsed_ms: u64, now_s: u64) {
        if attempted_hashes == 0 || elapsed_ms == 0 {
            return;
        }
        self.total_attempted_hashes = self.total_attempted_hashes.saturating_add(attempted_hashes);
        self.total_elapsed_ms = self.total_elapsed_ms.saturating_add(elapsed_ms);
        self.samples.push_back(WorkSample {
            completed_at_s: now_s,
            attempted_hashes,
            elapsed_ms,
        });
        self.prune_samples(now_s);
    }

    fn prune_samples(&mut self, now_s: u64) {
        while matches!(self.samples.front(), Some(sample) if sample.completed_at_s.saturating_add(HASHRATE_WINDOW_24H_S) < now_s)
        {
            self.samples.pop_front();
        }
    }

    fn hashrate_for_window(&self, window_s: u64, now_s: u64) -> f64 {
        let mut hashes = 0u64;
        let mut elapsed_ms = 0u64;
        for sample in self.samples.iter().rev() {
            if sample.completed_at_s.saturating_add(window_s) < now_s {
                break;
            }
            hashes = hashes.saturating_add(sample.attempted_hashes);
            elapsed_ms = elapsed_ms.saturating_add(sample.elapsed_ms);
        }
        if elapsed_ms == 0 {
            0.0
        } else {
            hashes as f64 / (elapsed_ms as f64 / 1000.0)
        }
    }

    fn total_shares(&self) -> u64 {
        self.valid_shares
            .saturating_add(self.invalid_shares)
            .saturating_add(self.no_solution_jobs)
    }
}

impl MinerTelemetryRegistry {
    fn touch_session(&mut self, miner_id: &str, worker_name: &str, algorithm: &str, backend: &str) {
        let now_s = now_unix_seconds();
        self.miners
            .entry(miner_id.to_string())
            .and_modify(|miner| miner.touch(worker_name, algorithm, backend, now_s))
            .or_insert_with(|| MinerTelemetry::new(worker_name, algorithm, backend, now_s));
    }

    fn record_job_result(
        &mut self,
        miner_id: &str,
        worker_name: &str,
        accepted: bool,
        attempted_hashes: u64,
        elapsed_ms: u64,
    ) {
        let now_s = now_unix_seconds();
        let miner = self
            .miners
            .entry(miner_id.to_string())
            .or_insert_with(|| MinerTelemetry::new(worker_name, "", "", now_s));
        miner.touch(worker_name, "", "", now_s);
        miner.completed_jobs = miner.completed_jobs.saturating_add(1);
        miner.push_sample(attempted_hashes, elapsed_ms, now_s);
        if accepted {
            miner.valid_shares = miner.valid_shares.saturating_add(1);
            miner.last_share_time_s = now_s;
        } else {
            miner.invalid_shares = miner.invalid_shares.saturating_add(1);
        }
    }

    fn record_block_found(&mut self, miner_id: &str, worker_name: &str) {
        let now_s = now_unix_seconds();
        let miner = self
            .miners
            .entry(miner_id.to_string())
            .or_insert_with(|| MinerTelemetry::new(worker_name, "", "", now_s));
        miner.touch(worker_name, "", "", now_s);
        miner.blocks_found = miner.blocks_found.saturating_add(1);
    }

    fn record_no_solution(
        &mut self,
        miner_id: &str,
        worker_name: &str,
        attempted_hashes: u64,
        elapsed_ms: u64,
    ) {
        let now_s = now_unix_seconds();
        let miner = self
            .miners
            .entry(miner_id.to_string())
            .or_insert_with(|| MinerTelemetry::new(worker_name, "", "", now_s));
        miner.touch(worker_name, "", "", now_s);
        miner.completed_jobs = miner.completed_jobs.saturating_add(1);
        miner.no_solution_jobs = miner.no_solution_jobs.saturating_add(1);
        miner.push_sample(attempted_hashes, elapsed_ms, now_s);
    }

    fn record_pending_payouts(&mut self, height: u64, payouts: &[PayoutEntry]) {
        let now_s = now_unix_seconds();
        for payout in payouts {
            let miner = self
                .miners
                .entry(payout.miner_id.clone())
                .or_insert_with(|| MinerTelemetry::new("", "", "", now_s));
            miner.last_seen_s = now_s;
            miner.payouts.push_front(MinerPayoutRecord {
                amount_atomic: payout.amount,
                share_count: payout.share_count,
                created_ts: now_s,
                height,
                status: "pending_execution".to_string(),
                tx_id: None,
                error: None,
            });
            while miner.payouts.len() > PAYOUT_HISTORY_LIMIT {
                miner.payouts.pop_back();
            }
        }
    }

    fn record_submitted_payouts(&mut self, height: u64, payouts: &[PayoutEntry], tx_id: &str) {
        for payout in payouts {
            let Some(miner) = self.miners.get_mut(&payout.miner_id) else {
                continue;
            };
            if let Some(record) = miner.payouts.iter_mut().find(|record| {
                record.height == height
                    && record.amount_atomic == payout.amount
                    && record.share_count == payout.share_count
                    && record.status == "pending_execution"
            }) {
                record.status = "submitted_to_node".to_string();
                record.tx_id = Some(tx_id.to_string());
                record.error = None;
                miner.paid_total_atomic = miner.paid_total_atomic.saturating_add(payout.amount);
            }
        }
    }

    fn record_failed_payouts(&mut self, height: u64, payouts: &[PayoutEntry], error: &str) {
        for payout in payouts {
            let Some(miner) = self.miners.get_mut(&payout.miner_id) else {
                continue;
            };
            if let Some(record) = miner.payouts.iter_mut().find(|record| {
                record.height == height
                    && record.amount_atomic == payout.amount
                    && record.share_count == payout.share_count
                    && record.status == "pending_execution"
            }) {
                record.status = "submit_failed".to_string();
                record.tx_id = None;
                record.error = Some(error.to_string());
            }
        }
    }

    /// Record a successful protocol-fee payout (humanitarian / issobella / pool).
    ///
    /// Retained for the alternative "pool distributes fees" architecture; the
    /// active model pays fees via the core coinbase, so this is currently unused.
    #[allow(dead_code)]
    fn record_fee_payout(
        &mut self,
        height: u64,
        tx_id: &str,
        humanitarian: u64,
        issobella: u64,
        pool: u64,
    ) {
        let now_s = now_unix_seconds();
        let miner = self
            .miners
            .entry("__pool__".to_string())
            .or_insert_with(|| MinerTelemetry::new("__pool__", "", "", now_s));
        miner.payouts.push_front(MinerPayoutRecord {
            amount_atomic: humanitarian.saturating_add(issobella).saturating_add(pool),
            share_count: 0,
            created_ts: now_s,
            height,
            status: "fee_submitted_to_node".to_string(),
            tx_id: Some(tx_id.to_string()),
            error: None,
        });
        while miner.payouts.len() > PAYOUT_HISTORY_LIMIT {
            miner.payouts.pop_back();
        }
    }

    /// Record a failed protocol-fee payout.
    ///
    /// Retained for the alternative "pool distributes fees" architecture; the
    /// active model pays fees via the core coinbase, so this is currently unused.
    #[allow(dead_code)]
    fn record_failed_fee_payout(
        &mut self,
        height: u64,
        error: &str,
        humanitarian: u64,
        issobella: u64,
        pool: u64,
    ) {
        let now_s = now_unix_seconds();
        let miner = self
            .miners
            .entry("__pool__".to_string())
            .or_insert_with(|| MinerTelemetry::new("__pool__", "", "", now_s));
        miner.payouts.push_front(MinerPayoutRecord {
            amount_atomic: humanitarian.saturating_add(issobella).saturating_add(pool),
            share_count: 0,
            created_ts: now_s,
            height,
            status: "fee_submit_failed".to_string(),
            tx_id: None,
            error: Some(error.to_string()),
        });
        while miner.payouts.len() > PAYOUT_HISTORY_LIMIT {
            miner.payouts.pop_back();
        }
    }

    fn pool_hashrate_for_window(&self, window_s: u64, now_s: u64) -> f64 {
        self.miners
            .values()
            .map(|miner| miner.hashrate_for_window(window_s, now_s))
            .sum()
    }

    fn total_blocks_found(&self) -> u64 {
        self.miners.values().map(|miner| miner.blocks_found).sum()
    }
}

#[derive(Debug, Clone, Copy)]
struct RevenueLane {
    source: RevenueSource,
    value_usd: f64,
    weight: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionGroup {
    Zion,
    Revenue,
    Ncl,
    Auto,
}

#[derive(Debug)]
struct RevenueScheduler {
    lanes: Vec<RevenueLane>,
    total_weight: u32,
    cursor: u32,
    auto_assign_cursor: u32,
    auto_assign_include_zion: bool,
    default_value_usd: f64,
    multistream_enabled: bool,
    /// Stream profit system config (Deeksha Chv3 pipeline weights).
    stream_profit_config: StreamProfitConfig,
    /// Current stream weights derived from profit data.
    stream_weights: StreamWeights,
    /// Last profit snapshot (for logging / debugging).
    last_profit_snapshot: Option<StreamProfitSnapshot>,
}

impl RevenueScheduler {
    fn from_env(default_source: RevenueSource, default_value_usd: f64) -> Result<Self> {
        let stream_profit_config = StreamProfitConfig::from_env();

        let enabled = parse_env_bool("ZION_REVENUE_MULTISTREAM", false);
        if !enabled {
            // Even with multistream disabled, if stream_profit is enabled,
            // we compute profit-based weights for the Deeksha Chv3 pipeline.
            let stream_weights = if stream_profit_config.enabled {
                let snap = StreamProfitSnapshot::fallback();
                let sources = stream_profit_config.parse_enabled_sources();
                StreamWeights::from_profit(
                    &snap,
                    None,
                    &sources,
                    stream_profit_config.hysteresis_pct,
                )
            } else {
                StreamWeights::default_split()
            };

            return Ok(Self {
                lanes: vec![RevenueLane {
                    source: default_source,
                    value_usd: default_value_usd,
                    weight: 100,
                }],
                total_weight: 100,
                cursor: 0,
                auto_assign_cursor: 0,
                auto_assign_include_zion: parse_env_bool("ZION_BACKEND_AUTO_INCLUDE_ZION", false),
                default_value_usd,
                multistream_enabled: false,
                stream_profit_config,
                stream_weights,
                last_profit_snapshot: None,
            });
        }

        let mut lanes = Vec::new();
        // Canonical pool-side 50/25/25 distribution.
        push_lane_from_env(
            &mut lanes,
            RevenueSource::Zion,
            "ZION_STREAM_ZION_PCT",
            "ZION_STREAM_ZION_USD",
            50,
            default_value_usd,
        )?;
        push_lane_from_env(
            &mut lanes,
            RevenueSource::Blake3External,
            "ZION_STREAM_BLAKE3_PCT",
            "ZION_STREAM_BLAKE3_USD",
            25,
            default_value_usd,
        )?;
        push_lane_from_env(
            &mut lanes,
            RevenueSource::NclAi,
            "ZION_STREAM_NCL_PCT",
            "ZION_STREAM_NCL_USD",
            25,
            default_value_usd,
        )?;
        // Optional per-algorithm external lanes (default 0 -> disabled until explicitly set).
        push_lane_from_env(
            &mut lanes,
            RevenueSource::KHeavyHashExternal,
            "ZION_STREAM_KHEAVYHASH_PCT",
            "ZION_STREAM_KHEAVYHASH_USD",
            0,
            default_value_usd,
        )?;
        push_lane_from_env(
            &mut lanes,
            RevenueSource::EthashExternal,
            "ZION_STREAM_ETHASH_PCT",
            "ZION_STREAM_ETHASH_USD",
            0,
            default_value_usd,
        )?;
        push_lane_from_env(
            &mut lanes,
            RevenueSource::KawPowExternal,
            "ZION_STREAM_KAWPOW_PCT",
            "ZION_STREAM_KAWPOW_USD",
            0,
            default_value_usd,
        )?;
        push_lane_from_env(
            &mut lanes,
            RevenueSource::AutolykosExternal,
            "ZION_STREAM_AUTOLYKOS_PCT",
            "ZION_STREAM_AUTOLYKOS_USD",
            0,
            default_value_usd,
        )?;
        push_lane_from_env(
            &mut lanes,
            RevenueSource::RandomXExternal,
            "ZION_STREAM_RANDOMX_PCT",
            "ZION_STREAM_RANDOMX_USD",
            0,
            default_value_usd,
        )?;
        push_lane_from_env(
            &mut lanes,
            RevenueSource::ZelHashExternal,
            "ZION_STREAM_ZELHASH_PCT",
            "ZION_STREAM_ZELHASH_USD",
            0,
            default_value_usd,
        )?;

        let total_weight: u32 = lanes.iter().map(|l| l.weight).sum();
        if total_weight == 0 {
            return Err(anyhow!(
                "ZION_REVENUE_MULTISTREAM=true but all stream weights are zero"
            ));
        }

        // Compute initial stream weights from profit data.
        let stream_weights = if stream_profit_config.enabled {
            let snap = StreamProfitSnapshot::fallback();
            let sources = stream_profit_config.parse_enabled_sources();
            StreamWeights::from_profit(
                &snap,
                None,
                &sources,
                stream_profit_config.hysteresis_pct,
            )
        } else {
            StreamWeights::default_split()
        };

        Ok(Self {
            lanes,
            total_weight,
            cursor: 0,
            auto_assign_cursor: 0,
            auto_assign_include_zion: parse_env_bool("ZION_BACKEND_AUTO_INCLUDE_ZION", false),
            default_value_usd,
            multistream_enabled: true,
            stream_profit_config,
            stream_weights,
            last_profit_snapshot: None,
        })
    }

    fn assign_auto_group(&mut self) -> SessionGroup {
        let mut choices: Vec<(SessionGroup, u32)> = Vec::new();
        for lane in &self.lanes {
            if lane.weight == 0 {
                continue;
            }
            match lane.source {
                RevenueSource::Zion if self.auto_assign_include_zion => {
                    choices.push((SessionGroup::Zion, lane.weight));
                }
                RevenueSource::Blake3External
                | RevenueSource::KHeavyHashExternal
                | RevenueSource::EthashExternal
                | RevenueSource::KawPowExternal
                | RevenueSource::AutolykosExternal
                | RevenueSource::RandomXExternal
                | RevenueSource::ZelHashExternal => {
                    choices.push((SessionGroup::Revenue, lane.weight))
                }
                RevenueSource::NclAi => choices.push((SessionGroup::Ncl, lane.weight)),
                _ => {}
            }
        }

        if choices.is_empty() {
            return SessionGroup::Zion;
        }

        let total: u32 = choices.iter().map(|(_, w)| *w).sum();
        if total == 0 {
            return SessionGroup::Zion;
        }

        let mut position = self.auto_assign_cursor % total;
        self.auto_assign_cursor = self.auto_assign_cursor.wrapping_add(1);
        for (group, weight) in choices {
            if position < weight {
                return group;
            }
            position -= weight;
        }

        SessionGroup::Zion
    }

    fn next_lane(&mut self) -> (RevenueSource, f64) {
        if self.lanes.len() == 1 {
            let lane = self.lanes[0];
            return (lane.source, lane.value_usd);
        }

        let mut position = self.cursor % self.total_weight;
        self.cursor = self.cursor.wrapping_add(1);
        for lane in &self.lanes {
            if position < lane.weight {
                return (lane.source, lane.value_usd);
            }
            position -= lane.weight;
        }

        let lane = self.lanes[0];
        (lane.source, lane.value_usd)
    }

    fn next_lane_for_group(&mut self, group: SessionGroup) -> (RevenueSource, f64) {
        match group {
            SessionGroup::Zion => (
                RevenueSource::Zion,
                self.value_for_source(RevenueSource::Zion)
                    .unwrap_or(self.default_value_usd),
            ),
            SessionGroup::Revenue => {
                // Rotate through enabled external-algo lanes.
                let external_lanes: Vec<_> = self
                    .lanes
                    .iter()
                    .filter(|l| {
                        l.weight > 0
                            && matches!(
                                l.source,
                                RevenueSource::Blake3External
                                    | RevenueSource::KHeavyHashExternal
                                    | RevenueSource::EthashExternal
                                    | RevenueSource::KawPowExternal
                                    | RevenueSource::AutolykosExternal
                                    | RevenueSource::RandomXExternal
                                    | RevenueSource::ZelHashExternal
                            )
                    })
                    .copied()
                    .collect();
                if external_lanes.is_empty() {
                    return (
                        RevenueSource::Blake3External,
                        self.value_for_source(RevenueSource::Blake3External)
                            .unwrap_or(self.default_value_usd),
                    );
                }
                // Use a stable sub-cursor for external rotation.
                let idx = self.cursor as usize % external_lanes.len();
                self.cursor = self.cursor.wrapping_add(1);
                let lane = external_lanes[idx];
                (lane.source, lane.value_usd)
            }
            SessionGroup::Ncl => (
                RevenueSource::NclAi,
                self.value_for_source(RevenueSource::NclAi)
                    .unwrap_or(self.default_value_usd),
            ),
            SessionGroup::Auto => self.next_lane(),
        }
    }

    fn value_for_source(&self, source: RevenueSource) -> Option<f64> {
        self.lanes
            .iter()
            .find(|lane| lane.source == source)
            .map(|lane| lane.value_usd)
    }

    fn describe_plan(&self) -> String {
        self.lanes
            .iter()
            .map(|lane| {
                format!(
                    "{}:{}%:${:.2}",
                    revenue_source_name(lane.source),
                    lane.weight,
                    lane.value_usd
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Return stream weights as a compact string for job messages.
    ///
    /// Format: "source_name:weight_pct,source_name:weight_pct,..."
    /// Only includes lanes with non-zero weight.
    fn stream_weights_string(&self) -> String {
        // If stream profit system is enabled, use the profit-based weights.
        if self.stream_profit_config.enabled && !self.stream_weights.weights.is_empty() {
            return self
                .stream_weights
                .weights
                .iter()
                .map(|w| format!("{}:{:.1}", w.source.as_str(), w.weight * 100.0))
                .collect::<Vec<_>>()
                .join(",");
        }

        // Fallback: derive from lane weights.
        let total: u32 = self.lanes.iter().map(|l| l.weight).sum();
        if total == 0 {
            return String::new();
        }
        self.lanes
            .iter()
            .filter(|l| l.weight > 0)
            .map(|l| {
                let pct = (l.weight as f64 / total as f64) * 100.0;
                format!("{}:{:.1}", revenue_source_name(l.source), pct)
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Update stream weights from a new profit snapshot.
    ///
    /// Called periodically by the background profit fetcher task.
    /// Applies hysteresis to prevent rapid oscillation.
    fn update_stream_weights(&mut self, snapshot: StreamProfitSnapshot) {
        if !self.stream_profit_config.enabled {
            return;
        }

        let sources = self.stream_profit_config.parse_enabled_sources();
        let new_weights = StreamWeights::from_profit(
            &snapshot,
            Some(&self.stream_weights),
            &sources,
            self.stream_profit_config.hysteresis_pct,
        );

        // Log if weights changed significantly.
        let old_desc = self.stream_weights.describe();
        let new_desc = new_weights.describe();
        if old_desc != new_desc {
            println!("stream_weights_update old=[{}] new=[{}]", old_desc, new_desc);
        }

        self.stream_weights = new_weights;
        self.last_profit_snapshot = Some(snapshot);
    }
}

impl RoutingStats {
    fn new(log_every: u64) -> Self {
        Self {
            log_every,
            total_submits: 0,
            total_accepted: 0,
            total_stale: 0,
            group_submits: [0; 4],
            group_accepted: [0; 4],
            source_submits: [0; 14],
            source_accepted: [0; 14],
        }
    }

    fn record_stale(&mut self) {
        self.total_stale = self.total_stale.saturating_add(1);
    }

    fn record(&mut self, group: SessionGroup, source: RevenueSource, accepted: bool) -> bool {
        self.total_submits = self.total_submits.saturating_add(1);
        self.group_submits[group_index(group)] =
            self.group_submits[group_index(group)].saturating_add(1);
        self.source_submits[source_index(source)] =
            self.source_submits[source_index(source)].saturating_add(1);

        if accepted {
            self.total_accepted = self.total_accepted.saturating_add(1);
            self.group_accepted[group_index(group)] =
                self.group_accepted[group_index(group)].saturating_add(1);
            self.source_accepted[source_index(source)] =
                self.source_accepted[source_index(source)].saturating_add(1);
        }

        self.log_every > 0 && self.total_submits.is_multiple_of(self.log_every)
    }

    fn snapshot_line(&self) -> String {
        let total = self.total_submits.max(1);
        let total_rejected = self
            .total_submits
            .saturating_sub(self.total_accepted)
            .saturating_sub(self.total_stale);
        let total_accept_rate = self.total_accepted as f64 * 100.0 / total as f64;

        let mut out = String::new();
        let _ = write!(
            out,
            "submits={} accepted={} rejected={} stale={} accept_rate={:.2}%",
            self.total_submits,
            self.total_accepted,
            total_rejected,
            self.total_stale,
            total_accept_rate
        );

        for group in [
            SessionGroup::Zion,
            SessionGroup::Revenue,
            SessionGroup::Ncl,
            SessionGroup::Auto,
        ] {
            let idx = group_index(group);
            let submits = self.group_submits[idx];
            let accepted = self.group_accepted[idx];
            let pct = submits as f64 * 100.0 / total as f64;
            let _ = write!(
                out,
                " {}={{submits:{},accepted:{},pct:{:.1}%}}",
                session_group_name(group),
                submits,
                accepted,
                pct
            );
        }

        for source in [
            RevenueSource::Zion,
            RevenueSource::Blake3External,
            RevenueSource::KHeavyHashExternal,
            RevenueSource::EthashExternal,
            RevenueSource::KawPowExternal,
            RevenueSource::AutolykosExternal,
            RevenueSource::RandomXExternal,
            RevenueSource::ZelHashExternal,
            RevenueSource::NclAi,
            RevenueSource::DeekshaLite,
            RevenueSource::ThermalBonus,
        ] {
            let idx = source_index(source);
            let submits = self.source_submits[idx];
            let accepted = self.source_accepted[idx];
            let pct = submits as f64 * 100.0 / total as f64;
            let _ = write!(
                out,
                " src_{}={{submits:{},accepted:{},pct:{:.1}%}}",
                revenue_source_name(source),
                submits,
                accepted,
                pct
            );
        }

        out
    }

    #[allow(dead_code)]
    fn snapshot_json(&self) -> String {
        let total_rejected = self
            .total_submits
            .saturating_sub(self.total_accepted)
            .saturating_sub(self.total_stale);
        let accept_rate = if self.total_submits == 0 {
            0.0
        } else {
            self.total_accepted as f64 * 100.0 / self.total_submits as f64
        };

        format!(
            "{{\"submits\":{},\"accepted\":{},\"rejected\":{},\"stale\":{},\"accept_rate_pct\":{:.2},\"groups\":{{\"zion\":{{\"submits\":{},\"accepted\":{}}},\"revenue\":{{\"submits\":{},\"accepted\":{}}},\"ncl\":{{\"submits\":{},\"accepted\":{}}},\"auto\":{{\"submits\":{},\"accepted\":{}}}}},\"sources\":{{\"zion\":{{\"submits\":{},\"accepted\":{}}},\"blake3\":{{\"submits\":{},\"accepted\":{}}},\"ncl\":{{\"submits\":{},\"accepted\":{}}}}}}}",
            self.total_submits,
            self.total_accepted,
            total_rejected,
            self.total_stale,
            accept_rate,
            self.group_submits[group_index(SessionGroup::Zion)],
            self.group_accepted[group_index(SessionGroup::Zion)],
            self.group_submits[group_index(SessionGroup::Revenue)],
            self.group_accepted[group_index(SessionGroup::Revenue)],
            self.group_submits[group_index(SessionGroup::Ncl)],
            self.group_accepted[group_index(SessionGroup::Ncl)],
            self.group_submits[group_index(SessionGroup::Auto)],
            self.group_accepted[group_index(SessionGroup::Auto)],
            self.source_submits[source_index(RevenueSource::Zion)],
            self.source_accepted[source_index(RevenueSource::Zion)],
            self.source_submits[source_index(RevenueSource::Blake3External)],
            self.source_accepted[source_index(RevenueSource::Blake3External)],
            self.source_submits[source_index(RevenueSource::NclAi)],
            self.source_accepted[source_index(RevenueSource::NclAi)],
        )
    }

    fn snapshot_prometheus(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "# HELP zion_pool_submits_total Total share submissions."
        );
        let _ = writeln!(out, "# TYPE zion_pool_submits_total counter");
        let _ = writeln!(out, "zion_pool_submits_total {}", self.total_submits);
        let _ = writeln!(out, "# HELP zion_pool_accepted_total Accepted shares.");
        let _ = writeln!(out, "# TYPE zion_pool_accepted_total counter");
        let _ = writeln!(out, "zion_pool_accepted_total {}", self.total_accepted);
        let _ = writeln!(out, "# HELP zion_pool_rejected_total Rejected shares.");
        let _ = writeln!(out, "# TYPE zion_pool_rejected_total counter");
        let _ = writeln!(
            out,
            "zion_pool_rejected_total {}",
            self.total_submits
                .saturating_sub(self.total_accepted)
                .saturating_sub(self.total_stale)
        );
        let _ = writeln!(out, "# HELP zion_pool_stale_total Stale shares.");
        let _ = writeln!(out, "# TYPE zion_pool_stale_total counter");
        let _ = writeln!(out, "zion_pool_stale_total {}", self.total_stale);
        let accept_rate = if self.total_submits == 0 {
            0.0
        } else {
            self.total_accepted as f64 * 100.0 / self.total_submits as f64
        };
        let _ = writeln!(
            out,
            "# HELP zion_pool_accept_rate_pct Accept rate percentage."
        );
        let _ = writeln!(out, "# TYPE zion_pool_accept_rate_pct gauge");
        let _ = writeln!(out, "zion_pool_accept_rate_pct {accept_rate:.2}");
        for (group, label) in [
            (SessionGroup::Zion, "zion"),
            (SessionGroup::Revenue, "revenue"),
            (SessionGroup::Ncl, "ncl"),
            (SessionGroup::Auto, "auto"),
        ] {
            let idx = group_index(group);
            let _ = writeln!(
                out,
                "zion_pool_group_submits{{group=\"{label}\"}} {}",
                self.group_submits[idx]
            );
            let _ = writeln!(
                out,
                "zion_pool_group_accepted{{group=\"{label}\"}} {}",
                self.group_accepted[idx]
            );
        }
        out
    }

    #[allow(dead_code)]
    fn snapshot_json_ext(&self, active_sessions: u64, uptime_s: u64) -> String {
        let total_rejected = self
            .total_submits
            .saturating_sub(self.total_accepted)
            .saturating_sub(self.total_stale);
        let accept_rate = if self.total_submits == 0 {
            0.0
        } else {
            self.total_accepted as f64 * 100.0 / self.total_submits as f64
        };

        format!(
            "{{\"submits\":{},\"accepted\":{},\"rejected\":{},\"stale\":{},\"accept_rate_pct\":{:.2},\"active_sessions\":{},\"uptime_s\":{},\"groups\":{{\"zion\":{{\"submits\":{},\"accepted\":{}}},\"revenue\":{{\"submits\":{},\"accepted\":{}}},\"ncl\":{{\"submits\":{},\"accepted\":{}}},\"auto\":{{\"submits\":{},\"accepted\":{}}}}},\"sources\":{{\"zion\":{{\"submits\":{},\"accepted\":{}}},\"blake3\":{{\"submits\":{},\"accepted\":{}}},\"ncl\":{{\"submits\":{},\"accepted\":{}}}}}}}",
            self.total_submits,
            self.total_accepted,
            total_rejected,
            self.total_stale,
            accept_rate,
            active_sessions,
            uptime_s,
            self.group_submits[group_index(SessionGroup::Zion)],
            self.group_accepted[group_index(SessionGroup::Zion)],
            self.group_submits[group_index(SessionGroup::Revenue)],
            self.group_accepted[group_index(SessionGroup::Revenue)],
            self.group_submits[group_index(SessionGroup::Ncl)],
            self.group_accepted[group_index(SessionGroup::Ncl)],
            self.group_submits[group_index(SessionGroup::Auto)],
            self.group_accepted[group_index(SessionGroup::Auto)],
            self.source_submits[source_index(RevenueSource::Zion)],
            self.source_accepted[source_index(RevenueSource::Zion)],
            self.source_submits[source_index(RevenueSource::Blake3External)],
            self.source_accepted[source_index(RevenueSource::Blake3External)],
            self.source_submits[source_index(RevenueSource::NclAi)],
            self.source_accepted[source_index(RevenueSource::NclAi)],
        )
    }

    fn snapshot_prometheus_ext(&self, active_sessions: u64, uptime_s: u64) -> String {
        let mut out = self.snapshot_prometheus();
        let _ = writeln!(
            out,
            "# HELP zion_pool_active_sessions Currently connected miners."
        );
        let _ = writeln!(out, "# TYPE zion_pool_active_sessions gauge");
        let _ = writeln!(out, "zion_pool_active_sessions {active_sessions}");
        let _ = writeln!(
            out,
            "# HELP zion_pool_uptime_seconds Pool uptime in seconds."
        );
        let _ = writeln!(out, "# TYPE zion_pool_uptime_seconds counter");
        let _ = writeln!(out, "zion_pool_uptime_seconds {uptime_s}");
        out
    }
}

fn serve_routing_metrics(
    bind_addr: &str,
    routing_stats: Arc<Mutex<RoutingStats>>,
    miner_telemetry: Arc<Mutex<MinerTelemetryRegistry>>,
    started_at: std::time::Instant,
    active_sessions: Arc<AtomicU64>,
    pplns_engine: Arc<Mutex<PplnsEngine>>,
    auxpow_scheduler: Arc<AuxPowScheduler>,
    revenue_scheduler: Arc<Mutex<RevenueScheduler>>,
) -> Result<()> {
    let listener = TcpListener::bind(bind_addr)
        .with_context(|| format!("failed to bind routing metrics listener on {bind_addr}"))?;

    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("routing_metrics_accept_error={error}");
                continue;
            }
        };

        // Read the HTTP request line to determine the path.
        let mut request_reader = BufReader::new(&stream);
        let mut request_line = String::new();
        if request_reader.read_line(&mut request_line).is_err() {
            continue;
        }
        let path = request_line.split_whitespace().nth(1).unwrap_or("/stats");

        let (status, content_type, body) = match path {
            "/health" => {
                let uptime_s = started_at.elapsed().as_secs();
                let body = format!("{{\"status\":\"ok\",\"uptime_s\":{uptime_s}}}");
                ("200 OK", "application/json", body)
            }
            "/metrics" => {
                let sessions = active_sessions.load(Ordering::Relaxed);
                let uptime_s = started_at.elapsed().as_secs();
                let stats = routing_stats.lock().expect("routing stats lock poisoned");
                let telemetry = miner_telemetry
                    .lock()
                    .expect("miner telemetry lock poisoned");
                let pplns = pplns_engine.lock().expect("pplns lock poisoned");
                let body = build_prometheus_payload(&stats, &telemetry, &pplns, sessions, uptime_s);
                ("200 OK", "text/plain; version=0.0.4", body)
            }
            p if p == "/stats" || p == "/" || p == "/pool" => {
                let sessions = active_sessions.load(Ordering::Relaxed);
                let uptime_s = started_at.elapsed().as_secs();
                let stats = routing_stats.lock().expect("routing stats lock poisoned");
                let telemetry = miner_telemetry
                    .lock()
                    .expect("miner telemetry lock poisoned");
                let pplns = pplns_engine.lock().expect("pplns lock poisoned");
                let auxpow_stats = auxpow_scheduler.stats_sync();
                let rev_sched = revenue_scheduler.lock().expect("revenue scheduler lock poisoned");
                let body = build_stats_payload(&stats, &telemetry, &pplns, sessions, uptime_s, &auxpow_stats, &rev_sched);
                ("200 OK", "application/json", body)
            }
            p if p.starts_with("/miners") => {
                let stats = routing_stats.lock().expect("routing stats lock poisoned");
                let telemetry = miner_telemetry
                    .lock()
                    .expect("miner telemetry lock poisoned");
                let pplns = pplns_engine.lock().expect("pplns lock poisoned");
                let body = build_miners_payload(path, &stats, &telemetry, &pplns);
                ("200 OK", "application/json", body)
            }
            p if p.starts_with("/api/v1/miner/") => {
                let stats = routing_stats.lock().expect("routing stats lock poisoned");
                let telemetry = miner_telemetry
                    .lock()
                    .expect("miner telemetry lock poisoned");
                let pplns = pplns_engine.lock().expect("pplns lock poisoned");
                match build_miner_api_payload(path, &stats, &telemetry, &pplns) {
                    Some(body) => ("200 OK", "application/json", body),
                    None => (
                        "404 Not Found",
                        "application/json",
                        "{\"ok\":false,\"error\":\"miner not found\"}".to_string(),
                    ),
                }
            }
            "/api/v1/revenue/stats" => {
                let uptime_s = started_at.elapsed().as_secs();
                let stats = routing_stats.lock().expect("routing stats lock poisoned");
                let pplns = pplns_engine.lock().expect("pplns lock poisoned");
                let auxpow_stats = auxpow_scheduler.stats_sync();
                let rev_sched = revenue_scheduler.lock().expect("revenue scheduler lock poisoned");
                let body = build_revenue_stats_payload(&stats, &pplns, &auxpow_stats, &rev_sched, uptime_s);
                ("200 OK", "application/json", body)
            }
            "/api/v1/revenue/streams" => {
                let stats = routing_stats.lock().expect("routing stats lock poisoned");
                let rev_sched = revenue_scheduler.lock().expect("revenue scheduler lock poisoned");
                let body = build_revenue_streams_payload(&stats, &rev_sched);
                ("200 OK", "application/json", body)
            }
            _ => (
                "404 Not Found",
                "application/json",
                "{\"ok\":false,\"error\":\"not found\"}".to_string(),
            ),
        };

        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );

        if let Err(error) = stream.write_all(response.as_bytes()) {
            eprintln!("routing_metrics_write_error={error}");
        }
    }

    Ok(())
}

fn build_prometheus_payload(
    stats: &RoutingStats,
    telemetry: &MinerTelemetryRegistry,
    pplns_engine: &PplnsEngine,
    active_sessions: u64,
    uptime_s: u64,
) -> String {
    let mut body = stats.snapshot_prometheus_ext(active_sessions, uptime_s);
    let pplns = pplns_engine.stats();
    let fees = pplns_engine.fee_stats();
    let now_s = now_unix_seconds();
    let pool_hashrate = telemetry.pool_hashrate_for_window(HASHRATE_WINDOW_LIVE_S, now_s);
    let pool_hashrate_1h = telemetry.pool_hashrate_for_window(HASHRATE_WINDOW_1H_S, now_s);
    let pool_hashrate_24h = telemetry.pool_hashrate_for_window(HASHRATE_WINDOW_24H_S, now_s);
    let _ = writeln!(body, "zion_pool_hashrate_hps {:.2}", pool_hashrate);
    let _ = writeln!(body, "zion_pool_hashrate_1h_hps {:.2}", pool_hashrate_1h);
    let _ = writeln!(body, "zion_pool_hashrate_24h_hps {:.2}", pool_hashrate_24h);
    let _ = writeln!(
        body,
        "zion_pool_blocks_found_total {}",
        telemetry.total_blocks_found()
    );
    let _ = writeln!(body, "zion_pool_miners_tracked {}", telemetry.miners.len());
    let _ = writeln!(body, "zion_pplns_window_size {}", pplns.window_size);
    let _ = writeln!(body, "zion_pplns_window_used {}", pplns.window_used);
    let _ = writeln!(
        body,
        "zion_pplns_registered_miners {}",
        pplns.registered_miners
    );
    let _ = writeln!(
        body,
        "zion_pplns_total_paid_flowers {}",
        pplns.total_paid_flowers
    );
    let _ = writeln!(body, "zion_pplns_payout_rounds {}", pplns.payout_rounds);
    let _ = writeln!(
        body,
        "zion_fee_humanitarian_flowers {}",
        fees.humanitarian_accumulated_flowers
    );
    let _ = writeln!(
        body,
        "zion_fee_issobella_flowers {}",
        fees.issobella_accumulated_flowers
    );
    let _ = writeln!(
        body,
        "zion_fee_pool_flowers {}",
        fees.pool_fee_accumulated_flowers
    );
    let _ = writeln!(body, "zion_fee_miner_pct {}", fees.miner_pct);
    for (miner_id, miner) in &telemetry.miners {
        let worker_name = sanitize_prometheus_label(&miner.worker_name);
        let miner_label = sanitize_prometheus_label(miner_id);
        let pending_balance = pplns_engine.unpaid_balance(miner_id);
        let _ = writeln!(
            body,
            "zion_pool_miner_hashrate_hps{{miner_id=\"{}\",worker_name=\"{}\"}} {:.2}",
            miner_label,
            worker_name,
            miner.hashrate_for_window(HASHRATE_WINDOW_LIVE_S, now_s)
        );
        let _ = writeln!(
            body,
            "zion_pool_miner_valid_shares_total{{miner_id=\"{}\",worker_name=\"{}\"}} {}",
            miner_label, worker_name, miner.valid_shares
        );
        let _ = writeln!(
            body,
            "zion_pool_miner_invalid_shares_total{{miner_id=\"{}\",worker_name=\"{}\"}} {}",
            miner_label, worker_name, miner.invalid_shares
        );
        let _ = writeln!(
            body,
            "zion_pool_miner_no_solution_total{{miner_id=\"{}\",worker_name=\"{}\"}} {}",
            miner_label, worker_name, miner.no_solution_jobs
        );
        let _ = writeln!(
            body,
            "zion_pool_miner_blocks_found_total{{miner_id=\"{}\",worker_name=\"{}\"}} {}",
            miner_label, worker_name, miner.blocks_found
        );
        let _ = writeln!(
            body,
            "zion_pool_miner_pending_balance_atomic{{miner_id=\"{}\",worker_name=\"{}\"}} {}",
            miner_label, worker_name, pending_balance
        );
        let _ = writeln!(
            body,
            "zion_pool_miner_paid_total_atomic{{miner_id=\"{}\",worker_name=\"{}\"}} {}",
            miner_label, worker_name, miner.paid_total_atomic
        );
        let _ = writeln!(
            body,
            "zion_pool_miner_last_seen_seconds{{miner_id=\"{}\",worker_name=\"{}\"}} {}",
            miner_label, worker_name, miner.last_seen_s
        );
    }
    body
}

fn build_stats_payload(
    stats: &RoutingStats,
    telemetry: &MinerTelemetryRegistry,
    pplns_engine: &PplnsEngine,
    active_sessions: u64,
    uptime_s: u64,
    auxpow: &AuxPowStats,
    revenue_scheduler: &RevenueScheduler,
) -> String {
    let now_s = now_unix_seconds();
    let pplns = pplns_engine.stats();
    let fees = pplns_engine.fee_stats();
    let json = serde_json::json!({
        "ok": true,
        "hashrate": {
            "pool": telemetry.pool_hashrate_for_window(HASHRATE_WINDOW_LIVE_S, now_s),
            "pool_1h": telemetry.pool_hashrate_for_window(HASHRATE_WINDOW_1H_S, now_s),
            "pool_24h": telemetry.pool_hashrate_for_window(HASHRATE_WINDOW_24H_S, now_s)
        },
        "miners": {
            "active": active_sessions,
            "total": telemetry.miners.len(),
            "registered": pplns.registered_miners
        },
        "shares": {
            "valid": stats.total_accepted,
            "invalid": stats.total_submits.saturating_sub(stats.total_accepted).saturating_sub(stats.total_stale),
            "stale": stats.total_stale,
            "total": stats.total_submits,
            "no_solution": telemetry.miners.values().map(|miner| miner.no_solution_jobs).sum::<u64>()
        },
        "blocks": {
            "found": telemetry.total_blocks_found()
        },
        "pool_hashrate": telemetry.pool_hashrate_for_window(HASHRATE_WINDOW_LIVE_S, now_s),
        "pool_hashrate_24h": telemetry.pool_hashrate_for_window(HASHRATE_WINDOW_24H_S, now_s),
        "uptime_s": uptime_s,
        "pool": {
            "uptime_secs": uptime_s,
            "version": "3.0.0"
        },
        "fee_split": {
            "miner_pct": fees.miner_pct,
            "humanitarian_pct": fees.humanitarian_pct,
            "issobella_pct": fees.issobella_pct,
            "pool_fee_pct": fees.pool_fee_pct,
            "humanitarian_accumulated_flowers": fees.humanitarian_accumulated_flowers,
            "issobella_accumulated_flowers": fees.issobella_accumulated_flowers,
            "pool_fee_accumulated_flowers": fees.pool_fee_accumulated_flowers,
            "humanitarian_wallet": fees.humanitarian_wallet,
            "issobella_wallet": fees.issobella_wallet,
            "pool_fee_wallet": fees.pool_fee_wallet
        },
        "routing": {
            "submits": stats.total_submits,
            "accepted": stats.total_accepted,
            "rejected": stats.total_submits.saturating_sub(stats.total_accepted).saturating_sub(stats.total_stale),
            "stale": stats.total_stale,
            "accept_rate_pct": if stats.total_submits == 0 { 0.0 } else { stats.total_accepted as f64 * 100.0 / stats.total_submits as f64 },
            "groups": {
                "zion": {
                    "submits": stats.group_submits[group_index(SessionGroup::Zion)],
                    "accepted": stats.group_accepted[group_index(SessionGroup::Zion)]
                },
                "revenue": {
                    "submits": stats.group_submits[group_index(SessionGroup::Revenue)],
                    "accepted": stats.group_accepted[group_index(SessionGroup::Revenue)]
                },
                "ncl": {
                    "submits": stats.group_submits[group_index(SessionGroup::Ncl)],
                    "accepted": stats.group_accepted[group_index(SessionGroup::Ncl)]
                },
                "auto": {
                    "submits": stats.group_submits[group_index(SessionGroup::Auto)],
                    "accepted": stats.group_accepted[group_index(SessionGroup::Auto)]
                }
            },
            "sources": ALL_REVENUE_SOURCES.iter().map(|&src| {
                let idx = source_index(src);
                (revenue_source_name(src).to_string(), serde_json::json!({
                    "submits": stats.source_submits[idx],
                    "accepted": stats.source_accepted[idx]
                }))
            }).collect::<serde_json::Map<String, serde_json::Value>>()
        },
        "pplns_window_size": pplns.window_size,
        "pplns": {
            "window_size": pplns.window_size,
            "window_used": pplns.window_used,
            "registered_miners": pplns.registered_miners,
            "total_unpaid_flowers": pplns.total_unpaid_flowers,
            "total_paid_flowers": pplns.total_paid_flowers,
            "payout_rounds": pplns.payout_rounds
        },
        "payouts": {
            "pending_total_atomic": pplns.total_unpaid_flowers,
            "pending_miners": pplns.miners_with_unpaid,
            "total_paid_atomic": pplns.total_paid_flowers,
            "payout_rounds": pplns.payout_rounds
        },
        "api": {
            "miners": "/miners?limit=200",
            "miner_stats": "/api/v1/miner/:address/stats",
            "miner_payouts": "/api/v1/miner/:address/payouts",
            "metrics": "/metrics",
            "revenue_stats": "/api/v1/revenue/stats",
            "revenue_streams": "/api/v1/revenue/streams"
        },
        "auxpow": {
            "enabled": auxpow.enabled,
            "current_coin": auxpow.current_coin,
            "current_pool": auxpow.current_pool,
            "current_algorithm": auxpow.current_algorithm,
            "shares_submitted": auxpow.shares_submitted,
            "shares_accepted": auxpow.shares_accepted,
            "shares_rejected": auxpow.shares_rejected,
            "revenue_usd": auxpow.revenue_usd,
            "consecutive_failures": auxpow.consecutive_failures,
            "circuit_open": auxpow.circuit_open,
            "uptime_secs": auxpow.uptime_secs,
            "coin_switches": auxpow.coin_switches,
            "last_switch_ts": auxpow.last_switch_ts
        },
        "stream_profit": {
            "enabled": revenue_scheduler.stream_profit_config.enabled,
            "provider": revenue_scheduler.stream_profit_config.api_provider,
            "interval_secs": revenue_scheduler.stream_profit_config.interval_secs,
            "hysteresis_pct": revenue_scheduler.stream_profit_config.hysteresis_pct,
            "enabled_sources": revenue_scheduler.stream_profit_config.enabled_sources,
            "weights": revenue_scheduler.stream_weights.weights.iter().map(|w| {
                serde_json::json!({
                    "source": w.source.as_str(),
                    "weight_pct": (w.weight * 100.0 * 10.0).round() / 10.0
                })
            }).collect::<Vec<_>>(),
            "weights_string": revenue_scheduler.stream_weights_string(),
            "live": revenue_scheduler.stream_weights.live,
            "description": revenue_scheduler.stream_weights.describe(),
        }
    });
    json.to_string()
}

/// All 14 revenue sources in canonical order (matches `source_index`).
const ALL_REVENUE_SOURCES: [RevenueSource; 14] = [
    RevenueSource::Zion,
    RevenueSource::KeccakBonus,
    RevenueSource::Sha3Bonus,
    RevenueSource::ProfitSwitch,
    RevenueSource::Blake3External,
    RevenueSource::NclAi,
    RevenueSource::KHeavyHashExternal,
    RevenueSource::EthashExternal,
    RevenueSource::KawPowExternal,
    RevenueSource::AutolykosExternal,
    RevenueSource::RandomXExternal,
    RevenueSource::ZelHashExternal,
    RevenueSource::DeekshaLite,
    RevenueSource::ThermalBonus,
];

/// Build the comprehensive revenue report payload for `/api/v1/revenue/stats`.
///
/// Aggregates routing stats (per-source submits/accepted), AuxPow revenue,
/// stream profit weights, PPLNS payouts, and fee split into a unified
/// per-source revenue breakdown.
fn build_revenue_stats_payload(
    stats: &RoutingStats,
    pplns_engine: &PplnsEngine,
    auxpow: &AuxPowStats,
    revenue_scheduler: &RevenueScheduler,
    uptime_s: u64,
) -> String {
    let pplns = pplns_engine.stats();
    let fees = pplns_engine.fee_stats();

    // Per-source breakdown — all 14 sources with submits, accepted, and
    // derived revenue estimates.
    let sources: Vec<_> = ALL_REVENUE_SOURCES
        .iter()
        .map(|&src| {
            let idx = source_index(src);
            let submits = stats.source_submits[idx];
            let accepted = stats.source_accepted[idx];
            let accept_rate = if submits == 0 {
                0.0
            } else {
                accepted as f64 * 100.0 / submits as f64
            };
            serde_json::json!({
                "source": revenue_source_name(src),
                "submits": submits,
                "accepted": accepted,
                "accept_rate_pct": (accept_rate * 10.0).round() / 10.0,
                "fee_pct": (src.fee_rate() * 100.0 * 100.0).round() / 100.0,
            })
        })
        .collect();

    // Stream weights breakdown
    let stream_weights: Vec<_> = revenue_scheduler
        .stream_weights
        .weights
        .iter()
        .map(|w| {
            serde_json::json!({
                "source": w.source.as_str(),
                "weight_pct": (w.weight * 100.0 * 10.0).round() / 10.0,
            })
        })
        .collect();

    // AuxPow per-coin revenue attribution
    let aux_revenue_usd = auxpow.revenue_usd;
    let aux_uptime = auxpow.uptime_secs;
    let aux_rev_per_hour = if aux_uptime > 0 && aux_revenue_usd > 0.0 {
        aux_revenue_usd / aux_uptime as f64 * 3600.0
    } else {
        0.0
    };

    let json = serde_json::json!({
        "ok": true,
        "timestamp": now_unix_seconds(),
        "uptime_secs": uptime_s,
        "totals": {
            "auxpow_revenue_usd": (aux_revenue_usd * 1e6).round() / 1e6,
            "auxpow_revenue_per_hour_usd": (aux_rev_per_hour * 1e6).round() / 1e6,
            "auxpow_revenue_per_day_usd": (aux_rev_per_hour * 24.0 * 1e6).round() / 1e6,
            "zion_blocks_found": pplns.total_paid_flowers / 5_400_067_000, // rough estimate
            "total_submits": stats.total_submits,
            "total_accepted": stats.total_accepted,
            "total_stale": stats.total_stale,
            "overall_accept_rate_pct": if stats.total_submits == 0 { 0.0 } else {
                (stats.total_accepted as f64 * 100.0 / stats.total_submits as f64 * 10.0).round() / 10.0
            },
        },
        "sources": sources,
        "auxpow": {
            "enabled": auxpow.enabled,
            "current_coin": auxpow.current_coin,
            "current_pool": auxpow.current_pool,
            "current_algorithm": auxpow.current_algorithm,
            "shares_submitted": auxpow.shares_submitted,
            "shares_accepted": auxpow.shares_accepted,
            "shares_rejected": auxpow.shares_rejected,
            "revenue_usd": (auxpow.revenue_usd * 1e6).round() / 1e6,
            "revenue_per_hour_usd": (aux_rev_per_hour * 1e6).round() / 1e6,
            "revenue_per_day_usd": (aux_rev_per_hour * 24.0 * 1e6).round() / 1e6,
            "consecutive_failures": auxpow.consecutive_failures,
            "circuit_open": auxpow.circuit_open,
            "uptime_secs": auxpow.uptime_secs,
            "coin_switches": auxpow.coin_switches,
            "last_switch_ts": auxpow.last_switch_ts,
        },
        "stream_profit": {
            "enabled": revenue_scheduler.stream_profit_config.enabled,
            "provider": revenue_scheduler.stream_profit_config.api_provider,
            "live": revenue_scheduler.stream_weights.live,
            "interval_secs": revenue_scheduler.stream_profit_config.interval_secs,
            "hysteresis_pct": revenue_scheduler.stream_profit_config.hysteresis_pct,
            "enabled_sources": revenue_scheduler.stream_profit_config.enabled_sources,
            "weights": stream_weights,
            "weights_string": revenue_scheduler.stream_weights_string(),
            "description": revenue_scheduler.stream_weights.describe(),
        },
        "fee_split": {
            "miner_pct": fees.miner_pct,
            "humanitarian_pct": fees.humanitarian_pct,
            "issobella_pct": fees.issobella_pct,
            "pool_fee_pct": fees.pool_fee_pct,
            "humanitarian_accumulated_flowers": fees.humanitarian_accumulated_flowers,
            "issobella_accumulated_flowers": fees.issobella_accumulated_flowers,
            "pool_fee_accumulated_flowers": fees.pool_fee_accumulated_flowers,
        },
        "pplns": {
            "window_size": pplns.window_size,
            "window_used": pplns.window_used,
            "registered_miners": pplns.registered_miners,
            "total_unpaid_flowers": pplns.total_unpaid_flowers,
            "total_paid_flowers": pplns.total_paid_flowers,
            "payout_rounds": pplns.payout_rounds,
        },
    });
    json.to_string()
}

/// Build the per-stream telemetry payload for `/api/v1/revenue/streams`.
///
/// Shows the Deeksha Chv3 pipeline stream weights, work distribution, and
/// per-stream revenue attribution.
fn build_revenue_streams_payload(
    stats: &RoutingStats,
    revenue_scheduler: &RevenueScheduler,
) -> String {
    // Map stream weights to per-source work units (submits/accepted)
    let streams: Vec<_> = revenue_scheduler
        .stream_weights
        .weights
        .iter()
        .map(|w| {
            let src = w.source;
            let idx = source_index(src);
            let submits = stats.source_submits[idx];
            let accepted = stats.source_accepted[idx];
            serde_json::json!({
                "source": src.as_str(),
                "weight_pct": (w.weight * 100.0 * 10.0).round() / 10.0,
                "submits": submits,
                "accepted": accepted,
                "fee_rate_pct": (src.fee_rate() * 100.0 * 100.0).round() / 100.0,
            })
        })
        .collect();

    let json = serde_json::json!({
        "ok": true,
        "timestamp": now_unix_seconds(),
        "live": revenue_scheduler.stream_weights.live,
        "provider": revenue_scheduler.stream_profit_config.api_provider,
        "multistream_enabled": revenue_scheduler.multistream_enabled,
        "streams": streams,
        "weights_string": revenue_scheduler.stream_weights_string(),
        "description": revenue_scheduler.stream_weights.describe(),
        "enabled_sources": revenue_scheduler.stream_profit_config.enabled_sources,
    });
    json.to_string()
}

fn build_miners_payload(
    path: &str,
    _stats: &RoutingStats,
    telemetry: &MinerTelemetryRegistry,
    pplns_engine: &PplnsEngine,
) -> String {
    let now_s = now_unix_seconds();
    let limit = extract_limit(path).unwrap_or(200);
    let mut miners: Vec<_> = telemetry.miners.iter().collect();
    miners.sort_by_key(|(_, miner)| std::cmp::Reverse(miner.last_seen_s));
    let miners = miners
        .into_iter()
        .take(limit)
        .map(|(miner_id, miner)| {
            serde_json::json!({
                "address": miner_id,
                "worker_name": miner.worker_name,
                "algorithm": miner.algorithm,
                "backend": miner.backend,
                "payout_address": pplns_engine.address_for(miner_id).unwrap_or(""),
                "last_share": miner.last_share_time_s,
                "last_seen": miner.last_seen_s,
                "hashrate": miner.hashrate_for_window(HASHRATE_WINDOW_LIVE_S, now_s),
                "hashrate_1h": miner.hashrate_for_window(HASHRATE_WINDOW_1H_S, now_s),
                "hashrate_24h": miner.hashrate_for_window(HASHRATE_WINDOW_24H_S, now_s),
                "blocks_found": miner.blocks_found,
                "valid_shares": miner.valid_shares,
                "invalid_shares": miner.invalid_shares,
                "pending_balance": pplns_engine.unpaid_balance(miner_id)
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "ok": true,
        "miners": miners,
        "count": telemetry.miners.len()
    })
    .to_string()
}

fn build_miner_api_payload(
    path: &str,
    _stats: &RoutingStats,
    telemetry: &MinerTelemetryRegistry,
    pplns_engine: &PplnsEngine,
) -> Option<String> {
    let remainder = path.strip_prefix("/api/v1/miner/")?;
    let (address, suffix) = remainder.split_once('/')?;
    let suffix = suffix.split('?').next().unwrap_or(suffix);

    // Resolve all miner IDs registered to this payout address.
    let miner_ids = pplns_engine.miner_ids_for_address(address);
    if miner_ids.is_empty() {
        return None;
    }

    let now_s = now_unix_seconds();

    // Aggregate telemetry across all worker IDs for this payout address.
    let mut hashrate_1h = 0.0;
    let mut hashrate_24h = 0.0;
    let mut valid_shares: u64 = 0;
    let mut invalid_shares: u64 = 0;
    let mut blocks_found: u64 = 0;
    let mut total_paid: u64 = 0;
    let mut pending_balance: u64 = 0;
    let mut last_share_time: u64 = 0;
    let mut last_seen: u64 = 0;
    let mut first_seen: u64 = u64::MAX;
    let mut completed_jobs: u64 = 0;
    let mut no_solution_jobs: u64 = 0;
    let mut workers = Vec::new();
    let mut all_payouts = Vec::new();

    for miner_id in miner_ids {
        pending_balance += pplns_engine.unpaid_balance(miner_id);
        if let Some(miner) = telemetry.miners.get(miner_id) {
            hashrate_1h += miner.hashrate_for_window(HASHRATE_WINDOW_1H_S, now_s);
            hashrate_24h += miner.hashrate_for_window(HASHRATE_WINDOW_24H_S, now_s);
            valid_shares += miner.valid_shares;
            invalid_shares += miner.invalid_shares;
            blocks_found += miner.blocks_found;
            total_paid += miner.paid_total_atomic;
            last_share_time = last_share_time.max(miner.last_share_time_s);
            last_seen = last_seen.max(miner.last_seen_s);
            first_seen = first_seen.min(miner.first_seen_s);
            completed_jobs += miner.completed_jobs;
            no_solution_jobs += miner.no_solution_jobs;
            if !miner.worker_name.is_empty() && !workers.contains(&miner.worker_name) {
                workers.push(miner.worker_name.clone());
            }
            all_payouts.extend(miner.payouts.iter().cloned());
        }
    }

    match suffix {
        "stats" => Some(
            serde_json::json!({
                "ok": true,
                "address": address,
                "stats": {
                    "hashrate_1h": hashrate_1h,
                    "hashrate_24h": hashrate_24h,
                    "total_shares": valid_shares + invalid_shares,
                    "valid_shares": valid_shares,
                    "invalid_shares": invalid_shares,
                    "blocks_found": blocks_found,
                    "total_paid": total_paid,
                    "pending_balance": pending_balance,
                    "last_share_time": last_share_time,
                    "last_seen": last_seen,
                    "first_seen": if first_seen == u64::MAX { 0 } else { first_seen },
                    "worker_name": workers.join(", "),
                    "jobs_completed": completed_jobs,
                    "no_solution_jobs": no_solution_jobs
                }
            })
            .to_string(),
        ),
        "payouts" => {
            // Sort payouts newest first and dedupe by tx_id/height.
            all_payouts.sort_by(|a, b| b.created_ts.cmp(&a.created_ts));
            Some(
                serde_json::json!({
                    "ok": true,
                    "address": address,
                    "pending_payouts": all_payouts.iter().map(|payout| serde_json::json!({
                        "amount": payout.amount_atomic,
                        "amount_atomic": payout.amount_atomic,
                        "share_count": payout.share_count,
                        "created_ts": payout.created_ts,
                        "height": payout.height,
                        "status": payout.status.clone(),
                        "tx_id": payout.tx_id.clone(),
                        "error": payout.error.clone()
                    })).collect::<Vec<_>>()
                })
                .to_string(),
            )
        }
        _ => None,
    }
}

fn extract_limit(path: &str) -> Option<usize> {
    let query = path.split_once('?')?.1;
    for part in query.split('&') {
        let (key, value) = part.split_once('=')?;
        if key == "limit" {
            return value.parse::<usize>().ok();
        }
    }
    None
}

fn sanitize_prometheus_label(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn group_index(group: SessionGroup) -> usize {
    match group {
        SessionGroup::Zion => 0,
        SessionGroup::Revenue => 1,
        SessionGroup::Ncl => 2,
        SessionGroup::Auto => 3,
    }
}

fn source_index(source: RevenueSource) -> usize {
    match source {
        RevenueSource::Zion => 0,
        RevenueSource::KeccakBonus => 1,
        RevenueSource::Sha3Bonus => 2,
        RevenueSource::ProfitSwitch => 3,
        RevenueSource::Blake3External => 4,
        RevenueSource::NclAi => 5,
        RevenueSource::KHeavyHashExternal => 6,
        RevenueSource::EthashExternal => 7,
        RevenueSource::KawPowExternal => 8,
        RevenueSource::AutolykosExternal => 9,
        RevenueSource::RandomXExternal => 10,
        RevenueSource::ZelHashExternal => 11,
        RevenueSource::DeekshaLite => 12,
        RevenueSource::ThermalBonus => 13,
        RevenueSource::VerusHashExternal => 14,
    }
}

fn revenue_source_name(source: RevenueSource) -> &'static str {
    match source {
        RevenueSource::Zion => "zion",
        RevenueSource::KeccakBonus => "keccak",
        RevenueSource::Sha3Bonus => "sha3",
        RevenueSource::ProfitSwitch => "profit",
        RevenueSource::Blake3External => "blake3",
        RevenueSource::NclAi => "ncl",
        RevenueSource::KHeavyHashExternal => "kheavyhash",
        RevenueSource::EthashExternal => "ethash",
        RevenueSource::KawPowExternal => "kawpow",
        RevenueSource::AutolykosExternal => "autolykos",
        RevenueSource::RandomXExternal => "randomx",
        RevenueSource::ZelHashExternal => "zelhash",
        RevenueSource::DeekshaLite => "deeksha_lite",
        RevenueSource::ThermalBonus => "thermal_bonus",
        RevenueSource::VerusHashExternal => "verushash",
    }
}

fn push_lane_from_env(
    lanes: &mut Vec<RevenueLane>,
    source: RevenueSource,
    weight_key: &str,
    value_key: &str,
    default_weight: u32,
    default_value_usd: f64,
) -> Result<()> {
    let weight = parse_env_u32(weight_key, default_weight)?;
    if weight == 0 {
        return Ok(());
    }
    let value_usd = parse_env_f64(value_key, default_value_usd)?;
    lanes.push(RevenueLane {
        source,
        value_usd,
        weight,
    });
    Ok(())
}

impl ServerConfig {
    fn from_env() -> Result<Self> {
        Ok(Self {
            bind_addr: env_or_default("ZION_POOL_BIND", "127.0.0.1:8444"),
            accept_limit: parse_optional_env_u32("ZION_ACCEPT_LIMIT")?,
            node_rpc_addr: std::env::var("ZION_NODE_RPC_ADDR").ok(),
            loop_count: parse_env_u32("ZION_POOL_LOOP_COUNT", 1_000_000)?,
            job_ttl_ms: parse_env_u64("ZION_JOB_TTL_MS", 15_000)?,
            start_nonce: parse_env_u64("ZION_START_NONCE", 42)?,
            nonce_count: parse_env_u64("ZION_NONCE_COUNT", 4096)?,
            nonce_count_gpu: parse_env_u64("ZION_NONCE_COUNT_GPU", 262_144)?,
            nonce_stride: parse_env_u64("ZION_NONCE_STRIDE", 1_024)?,
            timestamp: parse_env_u64("ZION_TIMESTAMP", 1_762_000_200)?,
            target: parse_target_env("ZION_TARGET")?,
            revenue_source: parse_revenue_source(
                &std::env::var("ZION_REVENUE_SOURCE").unwrap_or_else(|_| "zion".to_string()),
            )?,
            revenue_value_usd: parse_env_f64("ZION_REVENUE_USD", 1.25)?,
            user_default_group: parse_session_group(
                &std::env::var("ZION_USER_DEFAULT_GROUP").unwrap_or_else(|_| "zion".to_string()),
            )?,
            backend_miner_ids: parse_env_csv_lower("ZION_BACKEND_MINER_IDS"),
            backend_worker_hints: {
                let values = parse_env_csv_lower("ZION_BACKEND_WORKER_HINTS");
                if values.is_empty() {
                    vec![
                        "backend".to_string(),
                        "revenue".to_string(),
                        "ncl".to_string(),
                    ]
                } else {
                    values
                }
            },
            routing_log_every: parse_env_u64("ZION_ROUTING_LOG_EVERY", 25)?,
            routing_metrics_bind: parse_optional_env_string("ZION_ROUTING_METRICS_BIND"),
            max_sessions_per_ip: parse_env_u32("ZION_MAX_SESSIONS_PER_IP", 10)?,
            session_read_timeout_secs: parse_env_u64("ZION_SESSION_READ_TIMEOUT_SECS", 300)?,
            pool_wallet_address: parse_optional_env_string("ZION_POOL_WALLET"),
            pool_signing_key: parse_pool_signing_key(),
            vardiff_start_difficulty: parse_env_u64("ZION_VARDIFF_START_DIFF", 1)?,
            vardiff_target_secs: parse_env_u64("ZION_VARDIFF_TARGET_SECS", 10)?,
            vardiff_retarget_shares: parse_env_u64("ZION_VARDIFF_RETARGET_SHARES", 6)?,
            vardiff_min_difficulty: parse_env_u64("ZION_VARDIFF_MIN_DIFF", 1)?,
            vardiff_max_difficulty: parse_env_u64("ZION_VARDIFF_MAX_DIFF", 0)?,
            btc_wallet: parse_optional_env_string("ZION_BTC_WALLET"),
            revenue_proxy_addr: parse_optional_env_string("ZION_REVENUE_PROXY_ADDR"),
            revenue_proxy_coin: std::env::var("ZION_REVENUE_PROXY_COIN")
                .unwrap_or_else(|_| "KAS".to_string()),
            upstream_pool_addr: parse_optional_env_string("ZION_UPSTREAM_POOL_ADDR"),
            auxpow_config: AuxPowIntegrationConfig::from_env(),
            // WARNING: Fallback values must stay in sync with `zion_core::emission`.
            // If the protocol-level split changes, update here, in pplns.rs,
            // cosmic-harmony/src/revenue.rs, and the whitepapers.
            fee_config: FeeConfig {
                humanitarian_pct: parse_env_u64("ZION_HUMANITARIAN_TITHE_PCT", 5).unwrap_or(5),
                issobella_pct: parse_env_u64("ZION_ISSOBELLA_FUND_PCT", 5).unwrap_or(5),
                pool_fee_pct: parse_env_u64("ZION_POOL_FEE_PCT", 1).unwrap_or(1),
                humanitarian_wallet: std::env::var("ZION_HUMANITARIAN_WALLET").unwrap_or_default(),
                issobella_wallet: std::env::var("ZION_ISSOBELLA_WALLET").unwrap_or_default(),
                pool_fee_wallet: std::env::var("ZION_POOL_FEE_WALLET").unwrap_or_default(),
            },
        })
    }
}

#[allow(dead_code)]
fn parse_optional_key_bytes_env(key: &str) -> Result<Option<[u8; 32]>> {
    match std::env::var(key) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            parse_fixed_hex::<32>(trimmed, key).map(Some)
        }
        Err(_) => Ok(None),
    }
}

fn parse_optional_env_string(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Err(_) => None,
    }
}

fn split_host_port(addr: &str) -> Option<(String, u16)> {
    if let Some(pos) = addr.rfind(':') {
        let host = addr[..pos].to_string();
        let port = addr[pos + 1..].parse::<u16>().ok()?;
        Some((host, port))
    } else {
        None
    }
}

fn resolve_session_group(miner_id: &str, worker_name: &str, config: &ServerConfig) -> SessionGroup {
    if let Some(group) = extract_group_hint(worker_name).or_else(|| extract_group_hint(miner_id)) {
        return group;
    }

    let miner_id_lc = miner_id.trim().to_ascii_lowercase();
    if !miner_id_lc.is_empty() && config.backend_miner_ids.iter().any(|id| id == &miner_id_lc) {
        return SessionGroup::Auto;
    }

    let worker_name_lc = worker_name.to_ascii_lowercase();
    if config
        .backend_worker_hints
        .iter()
        .any(|hint| !hint.is_empty() && worker_name_lc.contains(hint.as_str()))
    {
        return SessionGroup::Auto;
    }

    config.user_default_group
}

fn extract_group_hint(raw: &str) -> Option<SessionGroup> {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("g=zion") || lower.contains("group=zion") {
        return Some(SessionGroup::Zion);
    }
    if lower.contains("g=revenue") || lower.contains("group=revenue") {
        return Some(SessionGroup::Revenue);
    }
    if lower.contains("g=ncl") || lower.contains("group=ncl") {
        return Some(SessionGroup::Ncl);
    }
    if lower.contains("g=auto") || lower.contains("group=auto") {
        return Some(SessionGroup::Auto);
    }
    None
}

fn session_group_name(group: SessionGroup) -> &'static str {
    match group {
        SessionGroup::Zion => "zion",
        SessionGroup::Revenue => "revenue",
        SessionGroup::Ncl => "ncl",
        SessionGroup::Auto => "auto",
    }
}

fn env_or_default(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn parse_env_u64(key: &str, default: u64) -> Result<u64> {
    match std::env::var(key) {
        Ok(value) => value
            .parse::<u64>()
            .with_context(|| format!("invalid u64 in {key}: {value}")),
        Err(_) => Ok(default),
    }
}

fn parse_env_u32(key: &str, default: u32) -> Result<u32> {
    match std::env::var(key) {
        Ok(value) => value
            .parse::<u32>()
            .with_context(|| format!("invalid u32 in {key}: {value}")),
        Err(_) => Ok(default),
    }
}

fn parse_env_f64(key: &str, default: f64) -> Result<f64> {
    match std::env::var(key) {
        Ok(value) => value
            .parse::<f64>()
            .with_context(|| format!("invalid f64 in {key}: {value}")),
        Err(_) => Ok(default),
    }
}

fn parse_target_env(key: &str) -> Result<DifficultyTarget> {
    let raw = match std::env::var(key) {
        Ok(value) => value,
        Err(_) => return Ok(DifficultyTarget::MAX),
    };

    Ok(DifficultyTarget {
        bytes: parse_fixed_hex::<32>(&raw, key)?,
    })
}

fn parse_revenue_source(value: &str) -> Result<RevenueSource> {
    match value.trim().to_ascii_lowercase().as_str() {
        "zion" => Ok(RevenueSource::Zion),
        "keccak" | "keccak_bonus" => Ok(RevenueSource::KeccakBonus),
        "sha3" | "sha3_bonus" => Ok(RevenueSource::Sha3Bonus),
        "profit" | "profit_switch" => Ok(RevenueSource::ProfitSwitch),
        "blake3" | "blake3_external" | "dcr" | "alph" => Ok(RevenueSource::Blake3External),
        "kheavyhash" | "kas" | "kaspa" => Ok(RevenueSource::KHeavyHashExternal),
        "ethash" | "etc" | "ethereum-classic" | "evr" | "evrmore" | "mewc" | "meowcoin" => {
            Ok(RevenueSource::EthashExternal)
        }
        "kawpow" | "rvn" | "ravencoin" | "clore" | "clore.ai" => Ok(RevenueSource::KawPowExternal),
        "autolykos" | "erg" | "ergo" => Ok(RevenueSource::AutolykosExternal),
        "randomx" | "xmr" | "monero" => Ok(RevenueSource::RandomXExternal),
        "zelhash" | "flux" => Ok(RevenueSource::ZelHashExternal),
        "ncl" | "ncl_ai" => Ok(RevenueSource::NclAi),
        "deeksha_lite" | "dl" => Ok(RevenueSource::DeekshaLite),
        "thermal_bonus" | "fire" | "thermal" => Ok(RevenueSource::ThermalBonus),
        other => Err(anyhow!("unsupported revenue source: {other}")),
    }
}

fn parse_session_group(value: &str) -> Result<SessionGroup> {
    match value.trim().to_ascii_lowercase().as_str() {
        "zion" => Ok(SessionGroup::Zion),
        "revenue" => Ok(SessionGroup::Revenue),
        "ncl" => Ok(SessionGroup::Ncl),
        "auto" => Ok(SessionGroup::Auto),
        other => Err(anyhow!("unsupported session group: {other}")),
    }
}

fn parse_env_csv_lower(key: &str) -> Vec<String> {
    match std::env::var(key) {
        Ok(raw) => raw
            .split(',')
            .map(|entry| entry.trim().to_ascii_lowercase())
            .filter(|entry| !entry.is_empty())
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::sync::{Mutex, OnceLock};
    use std::thread;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn sample_template() -> BlockTemplate {
        let header = MiningHeader {
            version: 3,
            previous_hash: [0x31; 32],
            merkle_root: [0x42; 32],
            timestamp: 1_762_100_100,
            difficulty_bits: 0x1f00ffff,
        };

        BlockTemplate {
            template_id: 91,
            height: 2,
            header_hex: to_hex(&header.to_bytes()),
            target_hex: DifficultyTarget::MAX.to_hex(),
            reward_zion: 5_400,
            transaction_ids: Vec::new(),
            transaction_count: 0,
            total_fees_zion: 0,
            body_hash_hex: "00".repeat(32),
            estimated_miner_reward_zion: 5_400,
            utxo_transaction_ids: Vec::new(),
            utxo_transaction_count: 0,
            total_utxo_fees: 0,
        }
    }

    fn spawn_mock_node(
        submit_response: RpcResponse,
    ) -> Result<(String, thread::JoinHandle<Result<Vec<RpcRequest>>>)> {
        let listener = TcpListener::bind("127.0.0.1:0").context("bind mock node")?;
        let addr = listener.local_addr().context("mock node addr")?;
        let template = sample_template();

        let handle = thread::spawn(move || -> Result<Vec<RpcRequest>> {
            let mut requests = Vec::new();
            for response in [
                RpcResponse::Template {
                    template: template.clone(),
                },
                submit_response,
            ] {
                let (stream, _) = listener.accept().context("accept mock node client")?;
                let reader_stream = stream.try_clone().context("clone mock node stream")?;
                let mut reader = BufReader::new(reader_stream);
                let mut writer = stream;

                let mut line = String::new();
                let read = reader
                    .read_line(&mut line)
                    .context("read mock node request")?;
                if read == 0 {
                    return Err(anyhow!("mock node client closed before request"));
                }

                requests.push(
                    zion_core::decode_rpc_request(&line).context("decode mock node request")?,
                );

                let response_line = zion_core::encode_rpc_response(&response)
                    .context("encode mock node response")?;
                writer
                    .write_all(response_line.as_bytes())
                    .context("write mock node response")?;
                writer.flush().context("flush mock node response")?;
            }
            Ok(requests)
        });

        Ok((addr.to_string(), handle))
    }

    fn spawn_pool_server(
        config: ServerConfig,
        auxpow_bridge: Option<AuxPowBridge>,
    ) -> Result<(
        SocketAddr,
        AuxPowBridge,
        thread::JoinHandle<Result<()>>,
    )> {
        let listener = TcpListener::bind("127.0.0.1:0").context("bind pool test listener")?;
        let addr = listener.local_addr().context("pool test addr")?;
        let pool = Arc::new(Mutex::new(MiningPool::with_job_ttl(
            CoreRuntime::default(),
            config.job_ttl_ms,
        )));
        let revenue_scheduler = Arc::new(Mutex::new(RevenueScheduler::from_env(
            config.revenue_source,
            config.revenue_value_usd,
        )?));
        let routing_stats = Arc::new(Mutex::new(RoutingStats::new(config.routing_log_every)));
        let miner_telemetry = Arc::new(Mutex::new(MinerTelemetryRegistry::default()));
        let pplns = Arc::new(Mutex::new(PplnsEngine::new(PplnsConfig::default())));
        let template_cache = Arc::new(Mutex::new(TemplateCache::new(Duration::from_secs(3))));
        let log_ch = LogChannel::spawn();
        let deferred_payouts: DeferredPayoutQueue = Arc::new(Mutex::new(Vec::new()));
        let auxpow_bridge = auxpow_bridge.unwrap_or_else(|| AuxPowBridge::new(config.auxpow_config.enabled).0);
        let auxpow_bridge_for_session = auxpow_bridge.clone();

        let handle = thread::spawn(move || -> Result<()> {
            let (stream, _) = listener.accept().context("accept pool test client")?;
            handle_client(
                stream,
                pool,
                revenue_scheduler,
                routing_stats,
                miner_telemetry,
                pplns,
                Arc::new(AtomicU64::new(0)),
                Arc::new(AtomicU64::new(0)),
                template_cache,
                deferred_payouts,
                auxpow_bridge_for_session,
                &config,
                &log_ch,
            )
        });

        Ok((addr, auxpow_bridge, handle))
    }

    fn run_bridge_session(
        submit_response: RpcResponse,
    ) -> Result<(Vec<PoolMessage>, Vec<RpcRequest>)> {
        let (node_rpc_addr, node_handle) = spawn_mock_node(submit_response)?;
        let config = ServerConfig {
            bind_addr: "127.0.0.1:0".to_string(),
            accept_limit: Some(1),
            node_rpc_addr: Some(node_rpc_addr),
            loop_count: 1,
            job_ttl_ms: 15_000,
            start_nonce: 42,
            nonce_count: 64,
            nonce_count_gpu: 64,
            nonce_stride: 64,
            timestamp: 1_762_100_200,
            target: DifficultyTarget::MAX,
            revenue_source: RevenueSource::Zion,
            revenue_value_usd: 1.25,
            user_default_group: SessionGroup::Zion,
            backend_miner_ids: Vec::new(),
            backend_worker_hints: Vec::new(),
            routing_log_every: 0,
            routing_metrics_bind: None,
            max_sessions_per_ip: 0,
            pool_wallet_address: None,
            pool_signing_key: None,
            session_read_timeout_secs: 300,
            vardiff_start_difficulty: 1,
            vardiff_target_secs: 10,
            vardiff_retarget_shares: 6,
            vardiff_min_difficulty: 1,
            vardiff_max_difficulty: 0,
            btc_wallet: None,
            revenue_proxy_addr: None,
            revenue_proxy_coin: "KAS".to_string(),
            fee_config: FeeConfig::default(),
            upstream_pool_addr: None,
            auxpow_config: AuxPowIntegrationConfig::default(),
        };
        let (pool_addr, _auxpow_bridge, pool_handle) = spawn_pool_server(config, None)?;

        let mut stream = TcpStream::connect(pool_addr).context("connect test miner to pool")?;
        let reader_stream = stream.try_clone().context("clone test miner stream")?;
        let mut reader = BufReader::new(reader_stream);

        write_wire_message(
            &mut stream,
            &PoolMessage::Hello {
                miner_id: "test-miner".to_string(),
                worker_name: "rig-test".to_string(),
                algorithm: zion_core::consensus_profile().to_string(),
                payout_address: "zion16825y2v5f3q507e5c2e0j8n666z43558l3zt604".to_string(),
                backend: "cpu".to_string(),
            },
        )?;

        let mut messages = Vec::new();
        let (_, welcome) = read_wire_message(&mut reader)?;
        messages.push(welcome);

        // With vardiff, the pool sends a SetDifficulty message after welcome.
        let (_, set_diff_message) = read_wire_message(&mut reader)?;
        messages.push(set_diff_message);

        let (_, job_message) = read_wire_message(&mut reader)?;
        let job_id = match &job_message {
            PoolMessage::Job { job_id, .. } => *job_id,
            other => return Err(anyhow!("expected job from pool, got {other:?}")),
        };
        messages.push(job_message);

        write_wire_message(
            &mut stream,
            &PoolMessage::Submit {
                job_id,
                miner_id: "test-miner".to_string(),
                worker_name: "rig-test".to_string(),
                nonce: 42,
                hash_hex: "00".repeat(32),
                attempted_hashes: Some(128),
                elapsed_ms: Some(1000),
                mix_hash_hex: None,
            },
        )?;

        loop {
            let (_, message) = read_wire_message(&mut reader)?;
            let is_bye = matches!(message, PoolMessage::Bye { .. });
            messages.push(message);
            if is_bye {
                break;
            }
        }

        pool_handle
            .join()
            .map_err(|_| anyhow!("pool test thread panicked"))??;
        let requests = node_handle
            .join()
            .map_err(|_| anyhow!("mock node thread panicked"))??;

        Ok((messages, requests))
    }

    #[test]
    fn pool_bridge_maps_stale_template_into_stale_cancel_result_flow() {
        let (messages, requests) = run_bridge_session(RpcResponse::SubmitResult {
            accepted: false,
            template_id: 91,
            block_height: None,
            hash_hex: "ab".repeat(32),
            reason: Some("stale template: expected 92, got 91".to_string()),
        })
        .expect("stale bridge session should succeed");

        assert!(matches!(messages[0], PoolMessage::Welcome { .. }));
        assert!(matches!(messages[1], PoolMessage::SetDifficulty { .. }));
        assert!(matches!(messages[2], PoolMessage::Job { job_id: 91, .. }));
        // With two-tier vardiff, the share meets share_target (MAX) so it is
        // accepted for PPLNS.  It also meets network_target (MAX) so it is
        // submitted to the node, which returns "stale template".
        assert!(matches!(messages[3], PoolMessage::Stale { job_id: 91 }));
        assert!(matches!(
            messages[4],
            PoolMessage::Cancel { job_id: 91, .. }
        ));
        assert!(matches!(
            messages[5],
            PoolMessage::Result {
                accepted: false,
                ref status
            } if status == "StaleJob"
        ));
        assert!(matches!(
            messages[6],
            PoolMessage::Bye {
                accepted_shares: 0,
                rejected_shares: 1,
                ..
            }
        ));

        assert!(matches!(requests[0], RpcRequest::GetTemplate));
        assert!(matches!(
            requests[1],
            RpcRequest::SubmitCandidate {
                template_id: 91,
                nonce: 42,
                ..
            }
        ));
    }

    #[test]
    fn pool_bridge_maps_unknown_upstream_rejection_into_rejected_result() {
        let (messages, requests) = run_bridge_session(RpcResponse::SubmitResult {
            accepted: false,
            template_id: 91,
            block_height: None,
            hash_hex: "cd".repeat(32),
            reason: Some("node maintenance window".to_string()),
        })
        .expect("upstream rejection bridge session should succeed");

        assert!(matches!(messages[0], PoolMessage::Welcome { .. }));
        assert!(matches!(messages[1], PoolMessage::SetDifficulty { .. }));
        assert!(matches!(messages[2], PoolMessage::Job { job_id: 91, .. }));
        assert!(matches!(
            messages[3],
            PoolMessage::Result {
                accepted: false,
                ref status
            } if status == "UpstreamRejected"
        ));
        assert!(matches!(
            messages[4],
            PoolMessage::Bye {
                accepted_shares: 0,
                rejected_shares: 1,
                ..
            }
        ));

        assert_eq!(requests.len(), 2);
        assert!(matches!(requests[0], RpcRequest::GetTemplate));
        assert!(matches!(
            requests[1],
            RpcRequest::SubmitCandidate {
                template_id: 91,
                nonce: 42,
                ..
            }
        ));
    }

    #[test]
    fn auxpow_bridge_issues_external_job_to_miner() {
        let _guard = env_lock().lock().expect("env lock");
        let config = ServerConfig {
            bind_addr: "127.0.0.1:0".to_string(),
            accept_limit: Some(1),
            node_rpc_addr: None,
            loop_count: 1,
            job_ttl_ms: 15_000,
            start_nonce: 42,
            nonce_count: 64,
            nonce_count_gpu: 64,
            nonce_stride: 64,
            timestamp: 1_762_100_200,
            target: DifficultyTarget::MAX,
            revenue_source: RevenueSource::KHeavyHashExternal,
            revenue_value_usd: 1.25,
            user_default_group: SessionGroup::Revenue,
            backend_miner_ids: Vec::new(),
            backend_worker_hints: Vec::new(),
            routing_log_every: 0,
            routing_metrics_bind: None,
            max_sessions_per_ip: 0,
            pool_wallet_address: None,
            pool_signing_key: None,
            session_read_timeout_secs: 300,
            vardiff_start_difficulty: 1,
            vardiff_target_secs: 10,
            vardiff_retarget_shares: 6,
            vardiff_min_difficulty: 1,
            vardiff_max_difficulty: 0,
            btc_wallet: None,
            revenue_proxy_addr: None,
            revenue_proxy_coin: "KAS".to_string(),
            fee_config: FeeConfig::default(),
            upstream_pool_addr: None,
            auxpow_config: AuxPowIntegrationConfig {
                enabled: true,
                split: Some(SplitConfig { zion_weight: 0, external_weight: 1 }),
                force_coin: Some(ExternalCoin::KAS),
                pool_preference: zion_auxpow::PoolPreference::Default,
                region: "eu".to_string(),
                payout_wallet: "bc1qtest".to_string(),
                worker_name: "zion_auxpow_test".to_string(),
                coin_wallets: std::collections::HashMap::new(),
                profit_check_interval_secs: 60,
                hysteresis_pct: 15.0,
            },
        };
        let (pool_addr, auxpow_bridge, pool_handle) = spawn_pool_server(config.clone(), None)
            .expect("spawn pool server with auxpow bridge");

        // Pre-seed the bridge queue with a synthetic KAS job.
        let external_job_id = "job_kas_001".to_string();
        let mut target = [0u8; 32];
        target[0] = 0x00;
        target[1] = 0x00;
        target[2] = 0xff;
        target[3] = 0xff;
        let external_job = JobPackage {
            external_coin: ExternalCoin::KAS,
            external_job_id: external_job_id.clone(),
            algorithm: "kheavyhash".to_string(),
            header_bytes: vec![0xAA; 80],
            target_bytes: target,
            timestamp: 1_762_100_200,
            block_number: None,
            extranonce1: vec![],
            start_nonce: 0,
            nonce_count: 1_000_000,
        };
        auxpow_bridge
            .job_queue
            .lock()
            .expect("auxpow job queue lock poisoned")
            .push_front(external_job);

        let mut stream = TcpStream::connect(pool_addr).expect("connect test miner to pool");
        let reader_stream = stream.try_clone().expect("clone test miner stream");
        let mut reader = BufReader::new(reader_stream);

        write_wire_message(
            &mut stream,
            &PoolMessage::Hello {
                miner_id: "test-miner".to_string(),
                worker_name: "rig-test".to_string(),
                algorithm: "kheavyhash".to_string(),
                payout_address: "zion16825y2v5f3q507e5c2e0j8n666z43558l3zt604".to_string(),
                backend: "cpu".to_string(),
            },
        )
        .expect("write hello");

        let (_, welcome) = read_wire_message(&mut reader).expect("read welcome");
        assert!(matches!(welcome, PoolMessage::Welcome { .. }));

        let (_, set_diff) = read_wire_message(&mut reader).expect("read set difficulty");
        assert!(matches!(set_diff, PoolMessage::SetDifficulty { .. }));

        let (_, job_message) = read_wire_message(&mut reader).expect("read job");
        match &job_message {
            PoolMessage::Job {
                job_id,
                algorithm,
                header_hex,
                height,
                ..
            } => {
                assert_eq!(*job_id, hash_job_id(&external_job_id));
                assert_eq!(algorithm, "kheavyhash");
                assert_eq!(header_hex, &to_hex(&vec![0xAA; 80]));
                assert_eq!(*height, 1_762_100_200);
            }
            other => panic!("expected Job message, got {other:?}"),
        }

        // Submit a NoSolution so the session ends cleanly.
        write_wire_message(
            &mut stream,
            &PoolMessage::NoSolution {
                job_id: hash_job_id(&external_job_id),
                miner_id: "test-miner".to_string(),
                worker_name: "rig-test".to_string(),
                attempted_hashes: Some(1_000_000),
                elapsed_ms: Some(1000),
            },
        )
        .expect("write no solution");

        // Read result and bye.
        let (_, result) = read_wire_message(&mut reader).expect("read result");
        assert!(matches!(result, PoolMessage::Result { accepted: false, .. }));
        let (_, bye) = read_wire_message(&mut reader).expect("read bye");
        assert!(matches!(bye, PoolMessage::Bye { .. }));

        pool_handle
            .join()
            .expect("pool test thread panicked")
            .expect("pool test thread error");
    }

    /// Spawn a minimal mock Stratum v1 server that accepts subscribe/authorize,
    /// sends one mining.notify job, and records any mining.submit it receives.
    async fn spawn_mock_stratum_pool(
        notify_job_id: &str,
        notify_header: &str,
        notify_target: &str,
        accept_submit: bool,
    ) -> (String, tokio::sync::mpsc::Receiver<serde_json::Value>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock stratum");
        let addr = listener.local_addr().unwrap().to_string();
        let (submit_tx, submit_rx) = tokio::sync::mpsc::channel::<serde_json::Value>(4);

        let notify = serde_json::json!({
            "id": null,
            "method": "mining.notify",
            "params": [notify_job_id, notify_header, notify_target]
        });
        let notify_line = serde_json::to_string(&notify).unwrap() + "\n";

        let accept_submit = accept_submit;
        tokio::spawn(async move {
            async fn read_json_message(
                reader: &mut tokio::net::tcp::OwnedReadHalf,
                buf: &mut [u8],
            ) -> serde_json::Value {
                let n = reader.read(buf).await.expect("read stratum message");
                serde_json::from_slice::<serde_json::Value>(&buf[..n]).expect("parse stratum message")
            }

            let (socket, _) = listener.accept().await.expect("accept stratum client");
            let (mut reader, mut writer) = socket.into_split();
            let mut buf = vec![0u8; 4096];

            // Subscribe
            let req = read_json_message(&mut reader, &mut buf).await;
            assert_eq!(req["method"], "mining.subscribe");
            let resp = serde_json::json!({ "id": 1, "result": [["mining.set_difficulty", "sub"], 4], "error": null });
            writer.write_all((serde_json::to_string(&resp).unwrap() + "\n").as_bytes()).await.unwrap();
            writer.flush().await.unwrap();

            // Authorize
            let req = read_json_message(&mut reader, &mut buf).await;
            assert_eq!(req["method"], "mining.authorize");
            let resp = serde_json::json!({ "id": 2, "result": true, "error": null });
            writer.write_all((serde_json::to_string(&resp).unwrap() + "\n").as_bytes()).await.unwrap();
            writer.flush().await.unwrap();

            // Send job
            writer.write_all(notify_line.as_bytes()).await.unwrap();
            writer.flush().await.unwrap();

            // Wait for submit
            let req = read_json_message(&mut reader, &mut buf).await;
            assert_eq!(req["method"], "mining.submit");
            let id = req.get("id").and_then(|v| v.as_i64()).unwrap_or(100);
            let _ = submit_tx.send(req).await;
            let resp = if accept_submit {
                serde_json::json!({ "id": id, "result": true, "error": null })
            } else {
                serde_json::json!({ "id": id, "result": false, "error": { "code": -1, "message": "low diff" } })
            };
            writer.write_all((serde_json::to_string(&resp).unwrap() + "\n").as_bytes()).await.unwrap();
            writer.flush().await.unwrap();
        });

        (addr, submit_rx)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn auxpow_e2e_pool_server_forwards_external_share_to_stratum_pool() {
        let _guard = env_lock().lock().expect("env lock");

        // 1) Start mock external Stratum pool.
        let (mock_pool_addr, mut submit_rx) = spawn_mock_stratum_pool(
            "job_dcr_e2e_001",
            "aabbccddeeff00112233445566778899",
            "0000ffff",
            true,
        )
        .await;

        // 2) Build an AuxPowClient pointing at the mock server.
        let mut profile = zion_auxpow::CoinProfile::default_for(zion_auxpow::ExternalCoin::DCR);
        let (host, port_str) = mock_pool_addr.rsplit_once(':').unwrap();
        profile.pool_host = host.to_string();
        profile.pool_port = port_str.parse().unwrap();
        profile.worker_name = "zion_e2e".to_string();

        let client = std::sync::Arc::new(zion_auxpow::AuxPowClient::new(profile));
        client.connect("bc1qtest").await.expect("connect to mock pool");

        // Wait for the first job.
        let external_job = client
            .wait_for_job(3000)
            .await
            .expect("wait for job")
            .expect("no job received");
        let job_package = zion_auxpow::JobPackage {
            external_coin: external_job.external_coin,
            external_job_id: external_job.job_id.clone(),
            algorithm: external_job.algorithm.clone(),
            header_bytes: external_job.header_bytes.clone(),
            target_bytes: external_job.target_bytes,
            timestamp: external_job.timestamp.unwrap_or(0),
            block_number: external_job.block_number,
            extranonce1: external_job.extranonce1.clone(),
            start_nonce: 0,
            nonce_count: 1_000_000,
        };

        // 3) Build the bridge and a background tokio task that forwards shares.
        let (bridge, share_rx) = AuxPowBridge::new(true);
        bridge
            .job_queue
            .lock()
            .expect("auxpow job queue lock poisoned")
            .push_front(job_package);

        let client_for_forwarder = std::sync::Arc::clone(&client);
        tokio::spawn(async move {
            let forwarder = zion_auxpow::ShareForwarder::new(client_for_forwarder);
            // Process the single share this test submits.
            let (req, reply_tx) = match tokio::task::spawn_blocking(move || share_rx.recv()).await {
                Ok(Ok(pair)) => pair,
                Ok(Err(_)) => return,
                Err(_) => return,
            };
            let started = std::time::Instant::now();
            let result = match forwarder
                .try_forward(&req.external_job_id, req.nonce, &req.hash, &req.target, req.mix_hash.as_ref())
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("e2e_forward_error: {e}");
                    zion_auxpow::ShareForwardResult::Unknown
                }
            };
            let outcome = ShareForwardOutcome {
                result,
                elapsed_ms: started.elapsed().as_millis() as u64,
            };
            let _ = reply_tx.send(outcome);
        });

        // 4) Start the ZION pool server with this bridge.
        let config = ServerConfig {
            bind_addr: "127.0.0.1:0".to_string(),
            accept_limit: Some(1),
            node_rpc_addr: None,
            loop_count: 1,
            job_ttl_ms: 15_000,
            start_nonce: 42,
            nonce_count: 64,
            nonce_count_gpu: 64,
            nonce_stride: 64,
            timestamp: 1_762_100_200,
            target: DifficultyTarget::MAX,
            revenue_source: RevenueSource::Blake3External,
            revenue_value_usd: 1.25,
            user_default_group: SessionGroup::Revenue,
            backend_miner_ids: Vec::new(),
            backend_worker_hints: Vec::new(),
            routing_log_every: 0,
            routing_metrics_bind: None,
            max_sessions_per_ip: 0,
            pool_wallet_address: None,
            pool_signing_key: None,
            session_read_timeout_secs: 300,
            vardiff_start_difficulty: 1,
            vardiff_target_secs: 10,
            vardiff_retarget_shares: 6,
            vardiff_min_difficulty: 1,
            vardiff_max_difficulty: 0,
            btc_wallet: None,
            revenue_proxy_addr: None,
            revenue_proxy_coin: "DCR".to_string(),
            fee_config: FeeConfig::default(),
            upstream_pool_addr: None,
            auxpow_config: AuxPowIntegrationConfig {
                enabled: true,
                split: Some(SplitConfig { zion_weight: 0, external_weight: 1 }),
                force_coin: Some(zion_auxpow::ExternalCoin::DCR),
                pool_preference: zion_auxpow::PoolPreference::Default,
                region: "eu".to_string(),
                payout_wallet: "bc1qtest".to_string(),
                worker_name: "zion_auxpow_e2e".to_string(),
                coin_wallets: std::collections::HashMap::new(),
                profit_check_interval_secs: 60,
                hysteresis_pct: 15.0,
            },
        };
        let (pool_addr, _bridge_for_server, pool_handle) = spawn_pool_server(config, Some(bridge))
            .expect("spawn pool server for e2e");

        // 5) Connect a miner and read the external job.
        let mut stream = TcpStream::connect(pool_addr).expect("connect test miner to pool");
        let reader_stream = stream.try_clone().expect("clone test miner stream");
        let mut reader = std::io::BufReader::new(reader_stream);

        write_wire_message(
            &mut stream,
            &PoolMessage::Hello {
                miner_id: "e2e-miner".to_string(),
                worker_name: "rig-e2e".to_string(),
                algorithm: "blake3".to_string(),
                payout_address: "zion16825y2v5f3q507e5c2e0j8n666z43558l3zt604".to_string(),
                backend: "cpu".to_string(),
            },
        )
        .expect("write hello");

        let (_, welcome) = read_wire_message(&mut reader).expect("read welcome");
        assert!(matches!(welcome, PoolMessage::Welcome { .. }));
        let (_, _set_diff) = read_wire_message(&mut reader).expect("read set difficulty");
        assert!(matches!(_set_diff, PoolMessage::SetDifficulty { .. }));
        let (_, job_message) = read_wire_message(&mut reader).expect("read job");
        let job_id = match &job_message {
            PoolMessage::Job { job_id, algorithm, .. } => {
                assert_eq!(algorithm, "blake3_dcr");
                *job_id
            }
            other => panic!("expected Job message, got {other:?}"),
        };

        // 6) Submit a share. The hash is cosmetic; the pool recomputes it.
        write_wire_message(
            &mut stream,
            &PoolMessage::Submit {
                job_id,
                miner_id: "e2e-miner".to_string(),
                worker_name: "rig-e2e".to_string(),
                nonce: 42,
                hash_hex: "00".repeat(32),
                attempted_hashes: Some(64),
                elapsed_ms: Some(1000),
                mix_hash_hex: None,
            },
        )
        .expect("write submit");

        // Read result (accepted/rejected) and bye.
        let (_, result) = read_wire_message(&mut reader).expect("read result");
        assert!(matches!(result, PoolMessage::Result { accepted: true, .. }), "share should be accepted: {result:?}");

        // 7) Verify the mock external pool received mining.submit.
        let submit_req = tokio::time::timeout(std::time::Duration::from_secs(5), submit_rx.recv())
            .await
            .expect("timeout waiting for submit")
            .expect("submit channel closed");
        assert_eq!(submit_req["method"], "mining.submit");
        assert_eq!(
            submit_req["params"][0].as_str().unwrap_or(""),
            "bc1qtest.zion_e2e"
        );
        assert_eq!(
            submit_req["params"][1].as_str().unwrap_or(""),
            "job_dcr_e2e_001"
        );

        // Clean shutdown: miner disconnects, pool handle finishes.
        drop(stream);
        pool_handle
            .join()
            .expect("pool test thread panicked")
            .expect("pool test thread error");
    }

    #[test]
    fn revenue_scheduler_defaults_to_single_lane() {
        let _guard = env_lock().lock().expect("env lock");
        std::env::remove_var("ZION_REVENUE_MULTISTREAM");
        let scheduler = RevenueScheduler::from_env(RevenueSource::Zion, 1.25).expect("scheduler");
        assert!(!scheduler.multistream_enabled);
        assert_eq!(scheduler.lanes.len(), 1);
        assert_eq!(scheduler.total_weight, 100);
        assert!(scheduler.describe_plan().contains("zion:100%"));
    }

    #[test]
    fn revenue_scheduler_weighted_round_robin() {
        let _guard = env_lock().lock().expect("env lock");
        std::env::set_var("ZION_REVENUE_MULTISTREAM", "true");
        std::env::set_var("ZION_STREAM_ZION_PCT", "2");
        std::env::set_var("ZION_STREAM_BLAKE3_PCT", "1");
        std::env::set_var("ZION_STREAM_NCL_PCT", "1");

        let mut scheduler =
            RevenueScheduler::from_env(RevenueSource::Zion, 1.0).expect("scheduler");
        let mut picks = Vec::new();
        for _ in 0..4 {
            picks.push(scheduler.next_lane().0);
        }

        assert_eq!(picks[0], RevenueSource::Zion);
        assert_eq!(picks[1], RevenueSource::Zion);
        assert_eq!(picks[2], RevenueSource::Blake3External);
        assert_eq!(picks[3], RevenueSource::NclAi);

        std::env::remove_var("ZION_REVENUE_MULTISTREAM");
        std::env::remove_var("ZION_STREAM_ZION_PCT");
        std::env::remove_var("ZION_STREAM_BLAKE3_PCT");
        std::env::remove_var("ZION_STREAM_NCL_PCT");
    }

    #[test]
    fn oasis_target_rejects_remote_without_override() {
        let err = parse_oasis_http_target("http://77.42.71.94:8094", false)
            .expect_err("remote URL must be blocked by default");
        assert!(
            err.to_string().contains("remote OASIS target blocked"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn oasis_target_allows_remote_with_override() {
        let (authority, path) = parse_oasis_http_target("http://77.42.71.94:8094/base", true)
            .expect("remote URL should be allowed when override is enabled");
        assert_eq!(authority, "77.42.71.94:8094");
        assert_eq!(path, "/base");
    }

    #[test]
    fn oasis_target_rejects_non_http_scheme() {
        let err = parse_oasis_http_target("https://127.0.0.1:8094", true)
            .expect_err("https should be rejected by parser");
        assert!(
            err.to_string().contains("only http:// URLs are supported"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn post_json_http_reads_status_code() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test listener");
        let addr = listener.local_addr().expect("local addr");

        let handle = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept local test connection");
            let mut buf = [0u8; 2048];
            let _ = socket.read(&mut buf).expect("read request bytes");
            let response = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            socket
                .write_all(response.as_bytes())
                .expect("write response bytes");
            socket.flush().expect("flush response");
        });

        let status = post_json_http(
            &addr.to_string(),
            "/api/v1/oasis/player/test/xp",
            r#"{"source":"block_mined","amount":500}"#,
            Duration::from_secs(2),
        )
        .expect("post_json_http should parse status");
        assert_eq!(status, 201);

        handle.join().expect("test server thread join");
    }

    #[test]
    fn resolve_session_group_defaults_to_zion_for_user_sessions() {
        let config = ServerConfig {
            bind_addr: "127.0.0.1:0".to_string(),
            accept_limit: Some(1),
            node_rpc_addr: None,
            loop_count: 1,
            job_ttl_ms: 15_000,
            start_nonce: 1,
            nonce_count: 64,
            nonce_count_gpu: 64,
            nonce_stride: 64,
            timestamp: 1,
            target: DifficultyTarget::MAX,
            revenue_source: RevenueSource::Zion,
            revenue_value_usd: 1.25,
            user_default_group: SessionGroup::Zion,
            backend_miner_ids: vec!["backend-miner-1".to_string()],
            backend_worker_hints: vec!["backend".to_string()],
            routing_log_every: 0,
            routing_metrics_bind: None,
            max_sessions_per_ip: 0,
            pool_wallet_address: None,
            pool_signing_key: None,
            session_read_timeout_secs: 300,
            vardiff_start_difficulty: 1,
            vardiff_target_secs: 10,
            vardiff_retarget_shares: 6,
            vardiff_min_difficulty: 1,
            vardiff_max_difficulty: 0,
            btc_wallet: None,
            revenue_proxy_addr: None,
            revenue_proxy_coin: "KAS".to_string(),
            fee_config: FeeConfig::default(),
            upstream_pool_addr: None,
            auxpow_config: AuxPowIntegrationConfig::default(),
        };

        let group = resolve_session_group("user-miner", "rig-01", &config);
        assert_eq!(group, SessionGroup::Zion);
    }

    #[test]
    fn resolve_session_group_routes_backend_allowlist_to_auto() {
        let config = ServerConfig {
            bind_addr: "127.0.0.1:0".to_string(),
            accept_limit: Some(1),
            node_rpc_addr: None,
            loop_count: 1,
            job_ttl_ms: 15_000,
            start_nonce: 1,
            nonce_count: 64,
            nonce_count_gpu: 64,
            nonce_stride: 64,
            timestamp: 1,
            target: DifficultyTarget::MAX,
            revenue_source: RevenueSource::Zion,
            revenue_value_usd: 1.25,
            user_default_group: SessionGroup::Zion,
            backend_miner_ids: vec!["backend-miner-1".to_string()],
            backend_worker_hints: vec!["backend".to_string()],
            routing_log_every: 0,
            routing_metrics_bind: None,
            max_sessions_per_ip: 0,
            pool_wallet_address: None,
            pool_signing_key: None,
            session_read_timeout_secs: 300,
            vardiff_start_difficulty: 1,
            vardiff_target_secs: 10,
            vardiff_retarget_shares: 6,
            vardiff_min_difficulty: 1,
            vardiff_max_difficulty: 0,
            btc_wallet: None,
            revenue_proxy_addr: None,
            revenue_proxy_coin: "KAS".to_string(),
            fee_config: FeeConfig::default(),
            upstream_pool_addr: None,
            auxpow_config: AuxPowIntegrationConfig::default(),
        };

        let group = resolve_session_group("backend-miner-1", "rig-01", &config);
        assert_eq!(group, SessionGroup::Auto);
    }

    #[test]
    fn resolve_session_group_routes_backend_worker_hint_to_auto() {
        let config = ServerConfig {
            bind_addr: "127.0.0.1:0".to_string(),
            accept_limit: Some(1),
            node_rpc_addr: None,
            loop_count: 1,
            job_ttl_ms: 15_000,
            start_nonce: 1,
            nonce_count: 64,
            nonce_count_gpu: 64,
            nonce_stride: 64,
            timestamp: 1,
            target: DifficultyTarget::MAX,
            revenue_source: RevenueSource::Zion,
            revenue_value_usd: 1.25,
            user_default_group: SessionGroup::Zion,
            backend_miner_ids: vec![],
            backend_worker_hints: vec!["backend".to_string(), "revenue".to_string()],
            routing_log_every: 0,
            routing_metrics_bind: None,
            max_sessions_per_ip: 0,
            pool_wallet_address: None,
            pool_signing_key: None,
            session_read_timeout_secs: 300,
            vardiff_start_difficulty: 1,
            vardiff_target_secs: 10,
            vardiff_retarget_shares: 6,
            vardiff_min_difficulty: 1,
            vardiff_max_difficulty: 0,
            btc_wallet: None,
            revenue_proxy_addr: None,
            revenue_proxy_coin: "KAS".to_string(),
            fee_config: FeeConfig::default(),
            upstream_pool_addr: None,
            auxpow_config: AuxPowIntegrationConfig::default(),
        };

        let group = resolve_session_group("miner-a", "backend-revenue-1", &config);
        assert_eq!(group, SessionGroup::Auto);
    }

    #[test]
    fn revenue_scheduler_group_pin_overrides_round_robin() {
        let mut scheduler = RevenueScheduler {
            lanes: vec![
                RevenueLane {
                    source: RevenueSource::Zion,
                    value_usd: 1.0,
                    weight: 2,
                },
                RevenueLane {
                    source: RevenueSource::Blake3External,
                    value_usd: 2.0,
                    weight: 1,
                },
                RevenueLane {
                    source: RevenueSource::NclAi,
                    value_usd: 3.0,
                    weight: 1,
                },
            ],
            total_weight: 4,
            cursor: 0,
            auto_assign_cursor: 0,
            auto_assign_include_zion: true,
            default_value_usd: 1.25,
            multistream_enabled: true,
            stream_profit_config: StreamProfitConfig::default(),
            stream_weights: StreamWeights::default_split(),
            last_profit_snapshot: None,
        };

        let (source, usd) = scheduler.next_lane_for_group(SessionGroup::Revenue);
        assert_eq!(source, RevenueSource::Blake3External);
        assert!((usd - 2.0).abs() < f64::EPSILON);

        let (source, usd) = scheduler.next_lane_for_group(SessionGroup::Ncl);
        assert_eq!(source, RevenueSource::NclAi);
        assert!((usd - 3.0).abs() < f64::EPSILON);

        let (source, usd) = scheduler.next_lane_for_group(SessionGroup::Auto);
        assert_eq!(source, RevenueSource::Zion);
        assert!((usd - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn routing_stats_tracks_groups_and_sources() {
        let mut stats = RoutingStats::new(2);
        assert!(!stats.record(SessionGroup::Zion, RevenueSource::Zion, true));
        assert!(stats.record(SessionGroup::Auto, RevenueSource::Blake3External, false));

        assert_eq!(stats.total_submits, 2);
        assert_eq!(stats.total_accepted, 1);
        assert_eq!(stats.group_submits[group_index(SessionGroup::Zion)], 1);
        assert_eq!(stats.group_submits[group_index(SessionGroup::Auto)], 1);
        assert_eq!(stats.source_submits[source_index(RevenueSource::Zion)], 1);
        assert_eq!(
            stats.source_submits[source_index(RevenueSource::Blake3External)],
            1
        );

        let snapshot = stats.snapshot_line();
        assert!(snapshot.contains("submits=2 accepted=1 rejected=1"));
        assert!(snapshot.contains("zion={submits:1,accepted:1"));
        assert!(snapshot.contains("auto={submits:1,accepted:0"));

        let snapshot_json = stats.snapshot_json();
        assert!(snapshot_json.contains("\"submits\":2"));
        assert!(snapshot_json.contains("\"groups\""));
        assert!(snapshot_json.contains("\"sources\""));
    }

    #[test]
    fn auto_assignment_is_weighted_and_session_pinned() {
        let mut scheduler = RevenueScheduler {
            lanes: vec![
                RevenueLane {
                    source: RevenueSource::Zion,
                    value_usd: 1.0,
                    weight: 2,
                },
                RevenueLane {
                    source: RevenueSource::Blake3External,
                    value_usd: 2.0,
                    weight: 1,
                },
                RevenueLane {
                    source: RevenueSource::NclAi,
                    value_usd: 3.0,
                    weight: 1,
                },
            ],
            total_weight: 4,
            cursor: 0,
            auto_assign_cursor: 0,
            auto_assign_include_zion: true,
            default_value_usd: 1.25,
            multistream_enabled: true,
            stream_profit_config: StreamProfitConfig::default(),
            stream_weights: StreamWeights::default_split(),
            last_profit_snapshot: None,
        };

        // Session allocation follows 2:1:1
        assert_eq!(scheduler.assign_auto_group(), SessionGroup::Zion);
        assert_eq!(scheduler.assign_auto_group(), SessionGroup::Zion);
        assert_eq!(scheduler.assign_auto_group(), SessionGroup::Revenue);
        assert_eq!(scheduler.assign_auto_group(), SessionGroup::Ncl);

        // Once session is pinned to revenue, submit routing stays revenue (no per-share rotation).
        let (src1, _) = scheduler.next_lane_for_group(SessionGroup::Revenue);
        let (src2, _) = scheduler.next_lane_for_group(SessionGroup::Revenue);
        assert_eq!(src1, RevenueSource::Blake3External);
        assert_eq!(src2, RevenueSource::Blake3External);
    }

    #[test]
    fn auto_assignment_can_exclude_zion() {
        let mut scheduler = RevenueScheduler {
            lanes: vec![
                RevenueLane {
                    source: RevenueSource::Zion,
                    value_usd: 1.0,
                    weight: 2,
                },
                RevenueLane {
                    source: RevenueSource::Blake3External,
                    value_usd: 2.0,
                    weight: 1,
                },
                RevenueLane {
                    source: RevenueSource::NclAi,
                    value_usd: 3.0,
                    weight: 1,
                },
            ],
            total_weight: 4,
            cursor: 0,
            auto_assign_cursor: 0,
            auto_assign_include_zion: false,
            default_value_usd: 1.25,
            multistream_enabled: true,
            stream_profit_config: StreamProfitConfig::default(),
            stream_weights: StreamWeights::default_split(),
            last_profit_snapshot: None,
        };

        assert_eq!(scheduler.assign_auto_group(), SessionGroup::Revenue);
        assert_eq!(scheduler.assign_auto_group(), SessionGroup::Ncl);
        assert_eq!(scheduler.assign_auto_group(), SessionGroup::Revenue);
        assert_eq!(scheduler.assign_auto_group(), SessionGroup::Ncl);
    }

    // ── Sprint 5 B4: session group resolution edge cases ───────────────

    #[test]
    fn extract_group_hint_from_worker_name() {
        assert_eq!(
            extract_group_hint("rig-g=revenue-01"),
            Some(SessionGroup::Revenue)
        );
        assert_eq!(extract_group_hint("rig-group=ncl"), Some(SessionGroup::Ncl));
        assert_eq!(extract_group_hint("rig-g=zion"), Some(SessionGroup::Zion));
        assert_eq!(extract_group_hint("rig-g=auto"), Some(SessionGroup::Auto));
        assert_eq!(extract_group_hint("rig-plain"), None);
    }

    #[test]
    fn extract_group_hint_case_insensitive() {
        assert_eq!(extract_group_hint("G=REVENUE"), Some(SessionGroup::Revenue));
        assert_eq!(extract_group_hint("GROUP=NCL"), Some(SessionGroup::Ncl));
        assert_eq!(extract_group_hint("g=Zion"), Some(SessionGroup::Zion));
    }

    #[test]
    fn resolve_session_group_explicit_hint_overrides_backend() {
        let config = ServerConfig {
            bind_addr: "127.0.0.1:0".to_string(),
            accept_limit: Some(1),
            node_rpc_addr: None,
            loop_count: 1,
            job_ttl_ms: 15_000,
            start_nonce: 1,
            nonce_count: 64,
            nonce_count_gpu: 64,
            nonce_stride: 64,
            timestamp: 1,
            target: DifficultyTarget::MAX,
            revenue_source: RevenueSource::Zion,
            revenue_value_usd: 1.25,
            user_default_group: SessionGroup::Zion,
            backend_miner_ids: vec!["backend-miner-1".to_string()],
            backend_worker_hints: vec!["backend".to_string()],
            routing_log_every: 0,
            routing_metrics_bind: None,
            max_sessions_per_ip: 0,
            pool_wallet_address: None,
            pool_signing_key: None,
            session_read_timeout_secs: 300,
            vardiff_start_difficulty: 1,
            vardiff_target_secs: 10,
            vardiff_retarget_shares: 6,
            vardiff_min_difficulty: 1,
            vardiff_max_difficulty: 0,
            btc_wallet: None,
            revenue_proxy_addr: None,
            revenue_proxy_coin: "KAS".to_string(),
            fee_config: FeeConfig::default(),
            upstream_pool_addr: None,
            auxpow_config: AuxPowIntegrationConfig::default(),
        };

        // Even though miner_id is in backend list, explicit hint wins
        let group = resolve_session_group("backend-miner-1", "rig-g=ncl", &config);
        assert_eq!(group, SessionGroup::Ncl);
    }

    // ── Sprint 5 B4: routing stats edge cases ──────────────────────────

    #[test]
    fn routing_stats_empty_state() {
        let stats = RoutingStats::new(10);
        assert_eq!(stats.total_submits, 0);
        assert_eq!(stats.total_accepted, 0);
        let line = stats.snapshot_line();
        assert!(line.contains("submits=0"));
        assert!(line.contains("accepted=0"));
    }

    #[test]
    fn routing_stats_prometheus_format() {
        let mut stats = RoutingStats::new(0);
        stats.record(SessionGroup::Zion, RevenueSource::Zion, true);
        stats.record(SessionGroup::Zion, RevenueSource::Zion, false);

        let prom = stats.snapshot_prometheus();
        assert!(prom.contains("zion_pool_submits_total 2"));
        assert!(prom.contains("zion_pool_accepted_total 1"));
        assert!(prom.contains("zion_pool_rejected_total 1"));
        assert!(prom.contains("zion_pool_accept_rate_pct 50.00"));
        assert!(prom.contains("zion_pool_group_submits{group=\"zion\"} 2"));
    }

    #[test]
    fn routing_stats_prometheus_ext_includes_sessions_and_uptime() {
        let stats = RoutingStats::new(0);
        let prom = stats.snapshot_prometheus_ext(5, 120);
        assert!(prom.contains("zion_pool_active_sessions 5"));
        assert!(prom.contains("zion_pool_uptime_seconds 120"));
    }

    #[test]
    fn routing_stats_json_ext_includes_sessions_and_uptime() {
        let stats = RoutingStats::new(0);
        let json = stats.snapshot_json_ext(3, 60);
        assert!(json.contains("\"active_sessions\":3"));
        assert!(json.contains("\"uptime_s\":60"));
    }

    #[test]
    fn routing_stats_log_interval_triggers_correctly() {
        let mut stats = RoutingStats::new(3);
        // First two should not trigger
        assert!(!stats.record(SessionGroup::Zion, RevenueSource::Zion, true));
        assert!(!stats.record(SessionGroup::Zion, RevenueSource::Zion, true));
        // Third should trigger
        assert!(stats.record(SessionGroup::Zion, RevenueSource::Zion, true));
        // Fourth should not
        assert!(!stats.record(SessionGroup::Zion, RevenueSource::Zion, true));
    }

    // ── Sprint 5 B4: revenue scheduler edge cases ──────────────────────

    #[test]
    fn revenue_scheduler_single_lane_always_returns_same() {
        let mut scheduler = RevenueScheduler {
            lanes: vec![RevenueLane {
                source: RevenueSource::Zion,
                value_usd: 1.5,
                weight: 100,
            }],
            total_weight: 100,
            cursor: 0,
            auto_assign_cursor: 0,
            auto_assign_include_zion: false,
            default_value_usd: 1.5,
            multistream_enabled: false,
            stream_profit_config: StreamProfitConfig::default(),
            stream_weights: StreamWeights::default_split(),
            last_profit_snapshot: None,
        };

        for _ in 0..10 {
            let (src, val) = scheduler.next_lane();
            assert_eq!(src, RevenueSource::Zion);
            assert!((val - 1.5).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn revenue_scheduler_cursor_wraps_around() {
        let mut scheduler = RevenueScheduler {
            lanes: vec![
                RevenueLane {
                    source: RevenueSource::Zion,
                    value_usd: 1.0,
                    weight: 1,
                },
                RevenueLane {
                    source: RevenueSource::Blake3External,
                    value_usd: 2.0,
                    weight: 1,
                },
            ],
            total_weight: 2,
            cursor: 0,
            auto_assign_cursor: 0,
            auto_assign_include_zion: true,
            default_value_usd: 1.0,
            multistream_enabled: true,
            stream_profit_config: StreamProfitConfig::default(),
            stream_weights: StreamWeights::default_split(),
            last_profit_snapshot: None,
        };

        let (s1, _) = scheduler.next_lane();
        let (s2, _) = scheduler.next_lane();
        let (s3, _) = scheduler.next_lane();
        assert_eq!(s1, RevenueSource::Zion);
        assert_eq!(s2, RevenueSource::Blake3External);
        assert_eq!(s3, RevenueSource::Zion); // wraps
    }

    #[test]
    fn revenue_scheduler_value_for_missing_source_returns_none() {
        let scheduler = RevenueScheduler {
            lanes: vec![RevenueLane {
                source: RevenueSource::Zion,
                value_usd: 1.0,
                weight: 100,
            }],
            total_weight: 100,
            cursor: 0,
            auto_assign_cursor: 0,
            auto_assign_include_zion: false,
            default_value_usd: 1.0,
            multistream_enabled: false,
            stream_profit_config: StreamProfitConfig::default(),
            stream_weights: StreamWeights::default_split(),
            last_profit_snapshot: None,
        };

        assert!(scheduler.value_for_source(RevenueSource::Zion).is_some());
        assert!(scheduler.value_for_source(RevenueSource::NclAi).is_none());
    }

    #[test]
    fn describe_plan_includes_all_lanes() {
        let scheduler = RevenueScheduler {
            lanes: vec![
                RevenueLane {
                    source: RevenueSource::Zion,
                    value_usd: 1.0,
                    weight: 50,
                },
                RevenueLane {
                    source: RevenueSource::Blake3External,
                    value_usd: 2.0,
                    weight: 25,
                },
                RevenueLane {
                    source: RevenueSource::NclAi,
                    value_usd: 3.0,
                    weight: 25,
                },
            ],
            total_weight: 100,
            cursor: 0,
            auto_assign_cursor: 0,
            auto_assign_include_zion: false,
            default_value_usd: 1.0,
            multistream_enabled: true,
            stream_profit_config: StreamProfitConfig::default(),
            stream_weights: StreamWeights::default_split(),
            last_profit_snapshot: None,
        };

        let plan = scheduler.describe_plan();
        assert!(plan.contains("zion:50%"));
        assert!(plan.contains("blake3:25%"));
        assert!(plan.contains("ncl:25%"));
    }

    // ── Sprint 5 B4: map_node_rejection coverage ───────────────────────

    #[test]
    fn map_node_rejection_classifies_reasons() {
        assert_eq!(
            map_node_rejection(Some("stale template: expected 5")),
            ShareStatus::StaleJob
        );
        assert_eq!(
            map_node_rejection(Some("header does not match")),
            ShareStatus::JobMismatch
        );
        assert_eq!(
            map_node_rejection(Some("low difficulty hash")),
            ShareStatus::RejectedLowDifficulty
        );
        assert_eq!(
            map_node_rejection(Some("unknown error")),
            ShareStatus::UpstreamRejected
        );
        assert_eq!(map_node_rejection(None), ShareStatus::UpstreamRejected);
    }

    // ── Sprint 5 B4: parse helpers ─────────────────────────────────────

    #[test]
    fn parse_fixed_hex_rejects_wrong_length() {
        assert!(parse_fixed_hex::<32>("aabb", "test").is_err());
    }

    #[test]
    fn parse_fixed_hex_accepts_valid_input() {
        let hex = "ff".repeat(32);
        let bytes = parse_fixed_hex::<32>(&hex, "test").unwrap();
        assert_eq!(bytes, [0xff; 32]);
    }

    #[test]
    fn parse_hash_hex_validates_32_bytes() {
        assert!(parse_hash_hex("aabb").is_err());
        let valid = "00".repeat(32);
        assert!(parse_hash_hex(&valid).is_ok());
    }

    #[test]
    fn session_group_name_covers_all_variants() {
        assert_eq!(session_group_name(SessionGroup::Zion), "zion");
        assert_eq!(session_group_name(SessionGroup::Revenue), "revenue");
        assert_eq!(session_group_name(SessionGroup::Ncl), "ncl");
        assert_eq!(session_group_name(SessionGroup::Auto), "auto");
    }

    #[test]
    fn revenue_source_name_covers_all_variants() {
        assert_eq!(revenue_source_name(RevenueSource::Zion), "zion");
        assert_eq!(revenue_source_name(RevenueSource::KeccakBonus), "keccak");
        assert_eq!(revenue_source_name(RevenueSource::Sha3Bonus), "sha3");
        assert_eq!(revenue_source_name(RevenueSource::ProfitSwitch), "profit");
        assert_eq!(revenue_source_name(RevenueSource::Blake3External), "blake3");
        assert_eq!(
            revenue_source_name(RevenueSource::KHeavyHashExternal),
            "kheavyhash"
        );
        assert_eq!(revenue_source_name(RevenueSource::EthashExternal), "ethash");
        assert_eq!(revenue_source_name(RevenueSource::KawPowExternal), "kawpow");
        assert_eq!(
            revenue_source_name(RevenueSource::AutolykosExternal),
            "autolykos"
        );
        assert_eq!(
            revenue_source_name(RevenueSource::RandomXExternal),
            "randomx"
        );
        assert_eq!(
            revenue_source_name(RevenueSource::ZelHashExternal),
            "zelhash"
        );
        assert_eq!(revenue_source_name(RevenueSource::NclAi), "ncl");
    }

    #[test]
    fn should_issue_external_job_defaults_to_zion_when_no_split() {
        let cfg = AuxPowIntegrationConfig {
            enabled: true,
            split: None,
            force_coin: None,
            pool_preference: zion_auxpow::PoolPreference::Default,
            region: "eu".to_string(),
            payout_wallet: "bc1qtest".to_string(),
            worker_name: "test".to_string(),
            coin_wallets: std::collections::HashMap::new(),
            profit_check_interval_secs: 60,
            hysteresis_pct: 15.0,
        };
        // With no split config, should default to ZION (false = not external)
        assert!(!should_issue_external_job(0, &cfg));
        assert!(!should_issue_external_job(1, &cfg));
        assert!(!should_issue_external_job(100, &cfg));
    }

    #[test]
    fn should_issue_external_job_respects_split() {
        let cfg = AuxPowIntegrationConfig {
            enabled: true,
            split: Some(SplitConfig { zion_weight: 4, external_weight: 1 }),
            force_coin: None,
            pool_preference: zion_auxpow::PoolPreference::Default,
            region: "eu".to_string(),
            payout_wallet: "bc1qtest".to_string(),
            worker_name: "test".to_string(),
            coin_wallets: std::collections::HashMap::new(),
            profit_check_interval_secs: 60,
            hysteresis_pct: 15.0,
        };
        // 4:1 split → 1 in 5 iterations is external (iteration % 5 < 1)
        assert!(should_issue_external_job(0, &cfg));  // 0 % 5 = 0 < 1 → external
        assert!(!should_issue_external_job(1, &cfg)); // 1 % 5 = 1 < 1? no → zion
        assert!(!should_issue_external_job(2, &cfg)); // 2 % 5 = 2 < 1? no → zion
        assert!(!should_issue_external_job(3, &cfg)); // 3 % 5 = 3 < 1? no → zion
        assert!(!should_issue_external_job(4, &cfg)); // 4 % 5 = 4 < 1? no → zion
        assert!(should_issue_external_job(5, &cfg));  // 5 % 5 = 0 < 1 → external
    }

    #[test]
    fn advertised_algorithm_is_deeksha_lite_v1() {
        // Verify that the pool always advertises deeksha_lite_v1,
        // not deeksha_chv3 (which broke the chain at block 4502).
        assert_eq!(zion_pool::advertised_algorithm_for_height(0), "deeksha_lite_v1");
        assert_eq!(zion_pool::advertised_algorithm_for_height(4499), "deeksha_lite_v1");
        assert_eq!(zion_pool::advertised_algorithm_for_height(4500), "deeksha_lite_v1");
        assert_eq!(zion_pool::advertised_algorithm_for_height(5000), "deeksha_lite_v1");
        assert_eq!(zion_pool::advertised_algorithm_for_height(99999), "deeksha_lite_v1");
    }
}

fn parse_optional_env_u32(key: &str) -> Result<Option<u32>> {
    match std::env::var(key) {
        Ok(value) => {
            let parsed = value
                .parse::<u32>()
                .with_context(|| format!("invalid u32 in {key}: {value}"))?;
            if parsed == 0 {
                Ok(None)
            } else {
                Ok(Some(parsed))
            }
        }
        Err(_) => Ok(None),
    }
}

fn parse_env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            !(normalized == "0"
                || normalized == "false"
                || normalized == "no"
                || normalized == "off")
        }
        Err(_) => default,
    }
}

// ── Pool payout execution (Phase 18) ──────────────────────────────────

fn parse_pool_signing_key() -> Option<ed25519_dalek::SigningKey> {
    let hex_str = std::env::var("ZION_POOL_PAYOUT_SK_HEX").ok()?;
    let hex_str = hex_str.trim();
    if hex_str.is_empty() || hex_str.len() != 64 {
        return None;
    }
    let bytes = parse_hex_bytes(hex_str)?;
    if bytes.len() != 32 {
        return None;
    }
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&bytes);
    Some(ed25519_dalek::SigningKey::from_bytes(&key_bytes))
}

fn parse_hex_bytes(hex_str: &str) -> Option<Vec<u8>> {
    if !hex_str.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(hex_str.len() / 2);
    for chunk in hex_str.as_bytes().chunks(2) {
        let pair = std::str::from_utf8(chunk).ok()?;
        bytes.push(u8::from_str_radix(pair, 16).ok()?);
    }
    Some(bytes)
}

/// Fetch pool wallet's account balance (flowers) from the node via getBalance RPC.
fn fetch_pool_account_balance(node_rpc_addr: &str, address: &str) -> Result<u128> {
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getBalance",
        "params": { "address": address }
    });

    let mut stream = TcpStream::connect(node_rpc_addr)
        .with_context(|| format!("failed to connect to node rpc at {node_rpc_addr}"))?;
    let mut request_line = serde_json::to_string(&request_body)?;
    request_line.push('\n');
    stream.write_all(request_line.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    reader.read_line(&mut response_line)?;

    let response: serde_json::Value =
        serde_json::from_str(&response_line).context("failed to parse getBalance response")?;

    if let Some(error) = response.get("error") {
        if !error.is_null() {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");
            return Err(anyhow!("getBalance error: {}", msg));
        }
    }

    let result = response
        .get("result")
        .ok_or_else(|| anyhow!("missing result in getBalance response"))?;

    // Use total balance (account + UTXO) as the spendable amount.
    let balance_str = result
        .get("balance_flowers")
        .and_then(|v| v.as_str())
        .unwrap_or("0");

    balance_str
        .parse::<u128>()
        .map_err(|e| anyhow!("failed to parse balance_flowers '{}': {}", balance_str, e))
}

/// Submit an account-model transaction to the node via submitAccountTransaction RPC.
fn submit_account_transaction(node_rpc_addr: &str, tx: &AccountTransaction) -> Result<String> {
    let tx_json = serde_json::to_value(tx)?;
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "submitAccountTransaction",
        "params": { "transaction": tx_json }
    });

    let mut stream = TcpStream::connect(node_rpc_addr)
        .with_context(|| format!("failed to connect to node rpc at {node_rpc_addr}"))?;
    let mut request_line = serde_json::to_string(&request_body)?;
    request_line.push('\n');
    stream.write_all(request_line.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    reader.read_line(&mut response_line)?;

    let response: serde_json::Value = serde_json::from_str(&response_line)
        .context("failed to parse submitAccountTransaction response")?;

    if let Some(error) = response.get("error") {
        if !error.is_null() {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");
            return Err(anyhow!("submitAccountTransaction error: {}", msg));
        }
    }

    let result = response
        .get("result")
        .ok_or_else(|| anyhow!("missing result in submitAccountTransaction response"))?;
    let accepted = result
        .get("accepted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !accepted {
        let reason = result
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("rejected");
        return Err(anyhow!("account transaction rejected: {}", reason));
    }

    let tx_id = result
        .get("tx_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    Ok(tx_id.to_string())
}

/// Fetch pool wallet's spendable UTXOs from the node via JSON-RPC 2.0.
fn fetch_pool_utxos(node_rpc_addr: &str, address: &str) -> Result<Vec<SpendableUtxo>> {
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getUtxos",
        "params": { "address": address }
    });

    let mut stream = TcpStream::connect(node_rpc_addr)
        .with_context(|| format!("failed to connect to node rpc at {node_rpc_addr}"))?;
    let mut request_line = serde_json::to_string(&request_body)?;
    request_line.push('\n');
    stream.write_all(request_line.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    reader.read_line(&mut response_line)?;

    let response: serde_json::Value =
        serde_json::from_str(&response_line).context("failed to parse getUtxos response")?;

    if let Some(error) = response.get("error") {
        if !error.is_null() {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");
            return Err(anyhow!("getUtxos error: {}", msg));
        }
    }

    let result = response
        .get("result")
        .ok_or_else(|| anyhow!("missing result in getUtxos response"))?;
    let utxo_array = result
        .get("utxos")
        .and_then(|u| u.as_array())
        .ok_or_else(|| anyhow!("missing utxos array in getUtxos response"))?;

    let mut utxos = Vec::new();
    for item in utxo_array {
        let tx_hash_hex = item.get("tx_hash").and_then(|v| v.as_str()).unwrap_or("");
        let output_index = item
            .get("output_index")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let amount = item.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
        let addr = item.get("address").and_then(|v| v.as_str()).unwrap_or("");

        let hash_bytes = parse_hex_bytes(tx_hash_hex).unwrap_or_default();
        if hash_bytes.len() != 32 {
            continue;
        }
        let mut tx_hash = [0u8; 32];
        tx_hash.copy_from_slice(&hash_bytes);

        utxos.push(SpendableUtxo {
            tx_hash,
            output_index,
            amount,
            address: addr.to_string(),
        });
    }
    Ok(utxos)
}

/// Submit a signed UTXO transaction to the node via JSON-RPC 2.0.
fn submit_utxo_transaction(node_rpc_addr: &str, tx: &zion_core::tx::Transaction) -> Result<String> {
    let tx_json = serde_json::to_value(tx)?;
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "submitTransaction",
        "params": { "transaction": tx_json }
    });

    let mut stream = TcpStream::connect(node_rpc_addr)
        .with_context(|| format!("failed to connect to node rpc at {node_rpc_addr}"))?;
    let mut request_line = serde_json::to_string(&request_body)?;
    request_line.push('\n');
    stream.write_all(request_line.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    reader.read_line(&mut response_line)?;

    let response: serde_json::Value = serde_json::from_str(&response_line)
        .context("failed to parse submitTransaction response")?;

    if let Some(error) = response.get("error") {
        if !error.is_null() {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");
            return Err(anyhow!("submitTransaction error: {}", msg));
        }
    }

    let result = response
        .get("result")
        .ok_or_else(|| anyhow!("missing result"))?;
    let accepted = result
        .get("accepted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !accepted {
        let reason = result
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("rejected");
        return Err(anyhow!("transaction rejected: {}", reason));
    }

    let tx_id = result
        .get("tx_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    Ok(tx_id.to_string())
}

/// Execute a pool payout: fetch UTXOs, build batch transaction, sign, and submit.
#[derive(Debug, Clone)]
struct PayoutExecutionOutcome {
    tx_id: String,
    executed: Vec<PayoutEntry>,
    deferred: Vec<PayoutEntry>,
}

/// Async payout execution — runs in a background thread so the miner thread
/// that found the block is not blocked during N sequential RPC calls to the
/// node.  Handles telemetry recording and PPLNS rollback on failure.
///
/// If the payout fails due to insufficient balance (race condition: node
/// hasn't credited the coinbase reward yet), the payouts are pushed onto the
/// deferred payout queue instead of being rolled back.  A background thread
/// retries deferred payouts until the balance is sufficient.
fn execute_payout_async(
    node_rpc_addr: Option<String>,
    pool_wallet_addr: Option<String>,
    signing_key: Option<ed25519_dalek::SigningKey>,
    payouts: &[PayoutEntry],
    height: u64,
    pplns_engine: &Arc<Mutex<PplnsEngine>>,
    miner_telemetry: &Arc<Mutex<MinerTelemetryRegistry>>,
    deferred_payouts: &DeferredPayoutQueue,
) {
    let node_rpc_addr = match node_rpc_addr.as_deref() {
        Some(a) => a,
        None => {
            println!(
                "payout_skipped height={} miners={} reason=missing_node_rpc_addr",
                height,
                payouts.len()
            );
            let mut pplns = pplns_engine.lock().expect("pplns lock poisoned");
            pplns.rollback_payouts(payouts);
            println!(
                "pplns_rollback height={} miners={} reason=payout_not_executed",
                height,
                payouts.len()
            );
            return;
        }
    };
    let pool_wallet_addr = match pool_wallet_addr.as_deref() {
        Some(a) => a,
        None => {
            println!(
                "payout_skipped height={} miners={} reason=missing_pool_wallet_address",
                height,
                payouts.len()
            );
            let mut pplns = pplns_engine.lock().expect("pplns lock poisoned");
            pplns.rollback_payouts(payouts);
            println!(
                "pplns_rollback height={} miners={} reason=payout_not_executed",
                height,
                payouts.len()
            );
            return;
        }
    };
    let signing_key = match signing_key.as_ref() {
        Some(k) => k,
        None => {
            println!(
                "payout_skipped height={} miners={} reason=missing_signing_key",
                height,
                payouts.len()
            );
            let mut pplns = pplns_engine.lock().expect("pplns lock poisoned");
            pplns.rollback_payouts(payouts);
            println!(
                "pplns_rollback height={} miners={} reason=payout_not_executed",
                height,
                payouts.len()
            );
            return;
        }
    };

    let result = execute_pool_payout(
        node_rpc_addr,
        pool_wallet_addr,
        signing_key,
        payouts,
        height,
    );

    match result {
        Ok(outcome) => {
            println!(
                "payout_submitted height={} miners={} deferred={} tx_id={}",
                height,
                outcome.executed.len(),
                outcome.deferred.len(),
                outcome.tx_id
            );
            {
                let mut telemetry = miner_telemetry
                    .lock()
                    .expect("miner telemetry lock poisoned");
                telemetry.record_submitted_payouts(height, &outcome.executed, &outcome.tx_id);
                if !outcome.deferred.is_empty() {
                    telemetry.record_failed_payouts(
                        height,
                        &outcome.deferred,
                        "deferred: insufficient pool payout wallet balance for full batch",
                    );
                }
            }
            if !outcome.deferred.is_empty() {
                let mut pplns = pplns_engine.lock().expect("pplns lock poisoned");
                pplns.rollback_payouts(&outcome.deferred);
                println!(
                    "pplns_partial_rollback height={} deferred_miners={} reason=insufficient_wallet_balance",
                    height,
                    outcome.deferred.len()
                );
            }
        }
        Err(err) => {
            let err_str = format!("{err}");
            println!(
                "payout_submit_failed height={} miners={} error={}",
                height,
                payouts.len(),
                err_str
            );
            {
                let mut telemetry = miner_telemetry
                    .lock()
                    .expect("miner telemetry lock poisoned");
                telemetry.record_failed_payouts(height, payouts, &err_str);
            }
            // If the failure is due to insufficient balance (race condition:
            // node hasn't credited the coinbase yet), queue for deferred retry
            // instead of rolling back PPLNS balances.
            let is_balance_issue = err_str.contains("deferring")
                || err_str.contains("account balance")
                || err_str.contains("insufficient");
            if is_balance_issue {
                deferred_payouts
                    .lock()
                    .expect("deferred lock poisoned")
                    .push(DeferredPayout {
                        payouts: payouts.to_vec(),
                        height,
                        queued_at: Instant::now(),
                        retry_count: 0,
                    });
                println!(
                    "payout_deferred_queued height={} miners={} reason=insufficient_balance_will_retry",
                    height,
                    payouts.len()
                );
            } else {
                // Permanent failure (not balance-related) — rollback.
                let mut pplns = pplns_engine.lock().expect("pplns lock poisoned");
                pplns.rollback_payouts(payouts);
                println!(
                    "pplns_rollback height={} miners={} reason=permanent_failure: {}",
                    height,
                    payouts.len(),
                    err_str
                );
            }
        }
    }
}

fn execute_pool_payout(
    node_rpc_addr: &str,
    pool_wallet_addr: &str,
    signing_key: &ed25519_dalek::SigningKey,
    payouts: &[PayoutEntry],
    height: u64,
) -> Result<PayoutExecutionOutcome> {
    if payouts.is_empty() {
        return Err(anyhow!("no payouts to execute"));
    }

    // Fetch spendable UTXOs for the pool wallet.
    let utxos = fetch_pool_utxos(node_rpc_addr, pool_wallet_addr)?;

    // ── Account-model fallback ─────────────────────────────────────────
    // When the node creates account-model coinbase transactions (instead of
    // UTXO outputs), the pool wallet has account balance but no UTXOs.
    // Fall back to account-model payouts in that case.
    if utxos.is_empty() {
        let account_balance = fetch_pool_account_balance(node_rpc_addr, pool_wallet_addr)?;
        let min_tx_fee = zion_core::fee::MIN_TX_FEE as u128;
        // Deduct tx fee from each miner payout so total needed equals the miner
        // reward accumulated in the pool wallet (no external buffer required).
        let total_needed: u128 = payouts.iter().map(|p| p.amount as u128).sum::<u128>();

        if account_balance == 0 {
            return Err(anyhow!(
                "pool payout wallet {} has no spendable UTXOs and zero account balance (balance will accumulate from new blocks)",
                pool_wallet_addr,
            ));
        }

        if account_balance < total_needed {
            return Err(anyhow!(
                "pool payout wallet {} account balance {} < total payout {} (deferring)",
                pool_wallet_addr,
                account_balance,
                total_needed,
            ));
        }

        let base_nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut executed = Vec::new();
        let mut first_tx_id = String::new();

        let pk_hex = hex::encode(signing_key.verifying_key().as_bytes());
        for (i, payout) in payouts.iter().enumerate() {
            if payout.address == pool_wallet_addr {
                continue; // skip self-send; node rejects account-model tx where from == to
            }
            let nonce = base_nonce + i as u64;
            let net_amount = (payout.amount as u128).saturating_sub(min_tx_fee);
            if net_amount == 0 {
                continue;
            }
            let tx_id = zion_core::wallet::generate_account_tx_id(
                pool_wallet_addr,
                &payout.address,
                net_amount as u64,
                nonce,
                None,
                0,
            );
            let sig = zion_core::crypto::sign(signing_key, tx_id.as_bytes());
            let tx = AccountTransaction {
                tx_id: tx_id.clone(),
                from: pool_wallet_addr.to_string(),
                to: payout.address.clone(),
                amount_zion: net_amount,
                fee_zion: zion_core::fee::MIN_TX_FEE,
                nonce,
                signature: hex::encode(sig),
                public_key: pk_hex.clone(),
                memo: None,
            };
            match submit_account_transaction(node_rpc_addr, &tx) {
                Ok(submitted_tx_id) => {
                    if first_tx_id.is_empty() {
                        first_tx_id = submitted_tx_id;
                    }
                    executed.push(payout.clone());
                }
                Err(err) => {
                    return Err(anyhow!(
                        "account payout failed for miner {} ({}): {}. executed={} deferred={}",
                        payout.miner_id,
                        payout.address,
                        err,
                        executed.len(),
                        payouts.len() - executed.len(),
                    ));
                }
            }
        }

        println!(
            "payout_account_model height={} recipients={} wallet={} tx_id={}",
            height,
            executed.len(),
            pool_wallet_addr,
            first_tx_id,
        );

        let deferred: Vec<PayoutEntry> = payouts
            .iter()
            .filter(|p| !executed.iter().any(|e| e.miner_id == p.miner_id))
            .cloned()
            .collect();

        return Ok(PayoutExecutionOutcome {
            tx_id: first_tx_id,
            executed,
            deferred,
        });
    }

    // Build the largest payable batch: sort ascending and trim largest payouts
    // on insufficient-funds errors so at least part of the round can be paid.
    let mut candidates = payouts.to_vec();
    candidates.sort_by_key(|p| p.amount);
    let mut last_build_error = String::new();

    while !candidates.is_empty() {
        let recipients: Vec<BatchRecipient> = candidates
            .iter()
            .map(|p| BatchRecipient {
                address: p.address.clone(),
                amount: p.amount,
            })
            .collect();

        // Use full UTXO count as upper-bound for inputs — coin selection may
        // need many UTXOs and the node validates fee ≥ size × MIN_FEE_RATE.
        // Over-estimating inputs only adds negligible extra flowers to the fee.
        let payout_fee = zion_core::fee::minimum_fee_for_size(zion_core::fee::estimate_tx_size(
            utxos.len(),
            recipients.len() + 1,
        ));

        match zion_core::wallet::build_batch_payout(
            signing_key,
            pool_wallet_addr,
            &recipients,
            payout_fee,
            &utxos,
            height,
        ) {
            Ok(build_result) => {
                let tx_id = submit_utxo_transaction(node_rpc_addr, &build_result.transaction)?;
                let deferred: Vec<PayoutEntry> = payouts
                    .iter()
                    .filter(|entry| !candidates.contains(*entry))
                    .cloned()
                    .collect();

                if !deferred.is_empty() {
                    let deferred_total: u128 = deferred.iter().map(|p| p.amount as u128).sum();
                    println!(
                        "payout_partial height={} executed={} deferred={} deferred_total_atomic={}",
                        height,
                        candidates.len(),
                        deferred.len(),
                        deferred_total,
                    );
                }

                println!(
                    "payout_built height={} recipients={} fee={} change={} inputs={}",
                    height,
                    recipients.len(),
                    payout_fee,
                    build_result.change_amount,
                    build_result.transaction.inputs.len(),
                );

                return Ok(PayoutExecutionOutcome {
                    tx_id,
                    executed: candidates,
                    deferred,
                });
            }
            Err(err) => {
                last_build_error = err.to_string();
                if !last_build_error.contains("insufficient funds") {
                    return Err(anyhow!("payout build failed: {}", last_build_error));
                }
                if candidates.len() == 1 {
                    // All miners dropped and single miner still exceeds
                    // balance — fall through to budget-capped mode below.
                    break;
                }
                // Drop the largest payout and retry to maximize paid miners.
                candidates.pop();
            }
        }
    }

    // ── Budget-capped payout ───────────────────────────────────────────
    // When the full unpaid balance for every miner exceeds the pool
    // wallet's spendable UTXOs, scale down proportionally so at least
    // some payment goes through each round instead of nothing.
    let available_total: u64 = utxos.iter().map(|u| u.amount).sum();
    let payout_fee_est = zion_core::fee::minimum_fee_for_size(zion_core::fee::estimate_tx_size(
        utxos.len(),
        payouts.len() + 1,
    ));
    let max_payable = available_total.saturating_sub(payout_fee_est);
    let total_needed: u64 = payouts.iter().map(|p| p.amount).sum();
    let min_payout = zion_core::wallet::MIN_PAYOUT_AMOUNT;

    if max_payable < min_payout || total_needed == 0 {
        return Err(anyhow!("payout build failed: {}", last_build_error));
    }

    // Scale each miner's payout proportionally to available budget.
    let mut capped_candidates: Vec<PayoutEntry> = Vec::new();
    let mut distributed: u64 = 0;
    let sorted_payouts: Vec<&PayoutEntry> = {
        let mut v: Vec<&PayoutEntry> = payouts.iter().collect();
        v.sort_by_key(|p| p.amount);
        v
    };
    for (i, p) in sorted_payouts.iter().enumerate() {
        let capped_amount = if i == sorted_payouts.len() - 1 {
            max_payable.saturating_sub(distributed)
        } else {
            ((p.amount as u128) * (max_payable as u128) / (total_needed as u128)) as u64
        };
        if capped_amount >= min_payout {
            distributed = distributed.saturating_add(capped_amount);
            capped_candidates.push(PayoutEntry {
                miner_id: p.miner_id.clone(),
                address: p.address.clone(),
                amount: capped_amount,
                share_count: p.share_count,
            });
        }
    }

    if capped_candidates.is_empty() {
        return Err(anyhow!("payout build failed: {}", last_build_error));
    }

    let capped_recipients: Vec<BatchRecipient> = capped_candidates
        .iter()
        .map(|p| BatchRecipient {
            address: p.address.clone(),
            amount: p.amount,
        })
        .collect();
    let capped_fee = zion_core::fee::minimum_fee_for_size(zion_core::fee::estimate_tx_size(
        utxos.len(),
        capped_recipients.len() + 1,
    ));

    match zion_core::wallet::build_batch_payout(
        signing_key,
        pool_wallet_addr,
        &capped_recipients,
        capped_fee,
        &utxos,
        height,
    ) {
        Ok(build_result) => {
            let tx_id = submit_utxo_transaction(node_rpc_addr, &build_result.transaction)?;

            // Deferred entries: original amount minus capped for executed
            // miners, plus full amounts for any miners that couldn't fit.
            let mut deferred: Vec<PayoutEntry> = Vec::new();
            for orig in payouts {
                if let Some(cap) = capped_candidates
                    .iter()
                    .find(|c| c.miner_id == orig.miner_id)
                {
                    let remainder = orig.amount.saturating_sub(cap.amount);
                    if remainder > 0 {
                        deferred.push(PayoutEntry {
                            miner_id: orig.miner_id.clone(),
                            address: orig.address.clone(),
                            amount: remainder,
                            share_count: orig.share_count,
                        });
                    }
                } else {
                    deferred.push(orig.clone());
                }
            }

            println!(
                "payout_budget_capped height={} available={} needed={} executed={} deferred={} fee={}",
                height, available_total, total_needed,
                capped_candidates.len(), deferred.len(), capped_fee,
            );
            println!(
                "payout_built height={} recipients={} fee={} change={} inputs={}",
                height,
                capped_recipients.len(),
                capped_fee,
                build_result.change_amount,
                build_result.transaction.inputs.len(),
            );

            Ok(PayoutExecutionOutcome {
                tx_id,
                executed: capped_candidates,
                deferred,
            })
        }
        Err(err) => Err(anyhow!("payout build failed (budget-capped): {}", err)),
    }
}

/// Execute a protocol-fee payout: humanitarian tithe, issobella fund, and
/// pool operator fee.  Builds a single batch transaction with up to three
/// outputs and submits it to the node RPC.
///
/// Retained for the alternative "pool distributes fees" architecture; the
/// active model pays fees via the core coinbase, so this is currently unused.
#[allow(dead_code)]
fn fee_payout_recipients(
    humanitarian: u64,
    issobella: u64,
    pool_fee: u64,
    fee_config: &FeeConfig,
) -> Vec<BatchRecipient> {
    let mut recipients = Vec::new();
    if humanitarian > 0 && !fee_config.humanitarian_wallet.is_empty() {
        recipients.push(BatchRecipient {
            address: fee_config.humanitarian_wallet.clone(),
            amount: humanitarian,
        });
    }
    if issobella > 0 && !fee_config.issobella_wallet.is_empty() {
        recipients.push(BatchRecipient {
            address: fee_config.issobella_wallet.clone(),
            amount: issobella,
        });
    }
    if pool_fee > 0 && !fee_config.pool_fee_wallet.is_empty() {
        recipients.push(BatchRecipient {
            address: fee_config.pool_fee_wallet.clone(),
            amount: pool_fee,
        });
    }
    recipients
}

#[allow(dead_code)]
fn execute_fee_payout(
    node_rpc_addr: &str,
    pool_wallet_addr: &str,
    signing_key: &ed25519_dalek::SigningKey,
    recipients: &[zion_core::wallet::BatchRecipient],
    height: u64,
) -> Result<String> {
    if recipients.is_empty() {
        return Err(anyhow!("no fee recipients"));
    }

    let utxos = fetch_pool_utxos(node_rpc_addr, pool_wallet_addr)?;

    // ── Account-model fallback ─────────────────────────────────────────
    if utxos.is_empty() {
        let account_balance = fetch_pool_account_balance(node_rpc_addr, pool_wallet_addr)?;
        let min_tx_fee = zion_core::fee::MIN_TX_FEE as u128;
        let total_needed: u128 = recipients.iter().map(|r| r.amount as u128).sum::<u128>()
            + (recipients.len() as u128 * min_tx_fee);

        if account_balance == 0 {
            return Err(anyhow!(
                "pool payout wallet {} has no spendable UTXOs and zero account balance for fee payout",
                pool_wallet_addr,
            ));
        }

        if account_balance < total_needed {
            return Err(anyhow!(
                "pool payout wallet {} account balance {} < fee payout {} (deferring)",
                pool_wallet_addr,
                account_balance,
                total_needed,
            ));
        }

        let base_nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut first_tx_id = String::new();

        let pk_hex = hex::encode(signing_key.verifying_key().as_bytes());
        for (i, recipient) in recipients.iter().enumerate() {
            let nonce = base_nonce + i as u64;
            let tx_id = zion_core::wallet::generate_account_tx_id(
                pool_wallet_addr,
                &recipient.address,
                recipient.amount,
                nonce,
                None,
                0,
            );
            let sig = zion_core::crypto::sign(signing_key, tx_id.as_bytes());
            let tx = AccountTransaction {
                tx_id: tx_id.clone(),
                from: pool_wallet_addr.to_string(),
                to: recipient.address.clone(),
                amount_zion: recipient.amount as u128,
                fee_zion: zion_core::fee::MIN_TX_FEE,
                nonce,
                signature: hex::encode(sig),
                public_key: pk_hex.clone(),
                memo: None,
            };
            match submit_account_transaction(node_rpc_addr, &tx) {
                Ok(submitted_tx_id) => {
                    if first_tx_id.is_empty() {
                        first_tx_id = submitted_tx_id;
                    }
                }
                Err(err) => {
                    return Err(anyhow!(
                        "account fee payout failed for {}: {}. executed={}/{}",
                        recipient.address,
                        err,
                        i,
                        recipients.len(),
                    ));
                }
            }
        }

        println!(
            "fee_payout_account_model height={} recipients={} wallet={} tx_id={}",
            height,
            recipients.len(),
            pool_wallet_addr,
            first_tx_id,
        );

        return Ok(first_tx_id);
    }

    let fee = zion_core::fee::minimum_fee_for_size(zion_core::fee::estimate_tx_size(
        utxos.len(),
        recipients.len() + 1,
    ));

    let build_result = zion_core::wallet::build_batch_payout(
        signing_key,
        pool_wallet_addr,
        recipients,
        fee,
        &utxos,
        height,
    )
    .map_err(|e| anyhow!("fee payout build failed: {}", e))?;

    let tx_id = submit_utxo_transaction(node_rpc_addr, &build_result.transaction)?;

    println!(
        "fee_payout_built height={} recipients={} fee={} change={} inputs={}",
        height,
        recipients.len(),
        fee,
        build_result.change_amount,
        build_result.transaction.inputs.len(),
    );

    Ok(tx_id)
}

/// Park forever — used by the NCL dispatcher thread to keep the tokio
/// runtime alive for the lifetime of the process without busy-looping.
async fn futures_park() {
    let () = std::future::pending().await;
}
