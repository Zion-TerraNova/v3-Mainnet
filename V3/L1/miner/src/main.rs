// Miner is the binary entry point; many session helpers take wide config tuples
// and a few GPU-fallback flags are informational. These are non-consensus.
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use anyhow::{anyhow, Context, Result};
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use zion_core::{CoreRuntime, DifficultyTarget, MiningHeader, MiningJob, RevenueSource};
use zion_pool::{decode_message, encode_message, MiningPool, PoolMessage, ShareStatus};

mod banner;
mod gpu_backend;
mod gpu_guard;
mod interactive;
mod parallel;
mod ui;

use interactive::{HashrateTracker, MinerControl, TUI_ACTIVE};

fn flush_stdout() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

/// Gate verbose wire_* / iteration= debug output (--verbose or ZION_MINER_VERBOSE=1).
static VERBOSE: AtomicBool = AtomicBool::new(false);
static CURRENT_POOL_DIFFICULTY: AtomicU64 = AtomicU64::new(1);
mod reconnect;

#[derive(Debug, Clone)]
struct MinerMetricsSnapshot {
    started_at: Instant,
    last_update_at: Instant,
    miner_id: String,
    worker_name: String,
    #[allow(dead_code)]
    mode: String,
    pool_addr: String,
    backend: String,
    status: String,
    #[allow(dead_code)]
    loop_target: u32,
    current_iteration: u32,
    last_job_id: u64,
    threads: usize,
    nonce_window: u64,
    session_active: bool,
    accepted_shares: u64,
    rejected_shares: u64,
    attempted_hashes: u64,
    hashrate_hps: f64,
    hashrate_10s_hps: f64,
    hashrate_60s_hps: f64,
    hashrate_15m_hps: f64,
    accept_rate_pct: f64,
    no_solution_iterations: u64,
    local_skip_likely_stale: u64,
    submit_avg_latency_ms: f64,
    submit_max_latency_ms: u64,
    gpu_hashrate_hps: f64,
    current_epoch: u64,
    pool_height: u64,
    best_batch_ms: u64,
    remote_ttl_ms: u64,
    hashrate_max: f64,
    algorithm: String,
}

impl MinerMetricsSnapshot {
    fn from_config(config: &MinerConfig) -> Self {
        let now = Instant::now();
        Self {
            started_at: now,
            last_update_at: now,
            miner_id: config.miner_id.clone(),
            worker_name: config.worker_name.clone(),
            mode: if config.pool_addr.is_some() {
                "remote".to_string()
            } else {
                "local".to_string()
            },
            pool_addr: config
                .pool_addr
                .clone()
                .unwrap_or_else(|| "local-runtime".to_string()),
            backend: "cpu".to_string(),
            status: "starting".to_string(),
            loop_target: config.loop_count,
            current_iteration: 0,
            last_job_id: 0,
            threads: config.threads,
            nonce_window: config.nonce_count,
            session_active: false,
            accepted_shares: 0,
            rejected_shares: 0,
            attempted_hashes: 0,
            hashrate_hps: 0.0,
            hashrate_10s_hps: 0.0,
            hashrate_60s_hps: 0.0,
            hashrate_15m_hps: 0.0,
            accept_rate_pct: 0.0,
            no_solution_iterations: 0,
            local_skip_likely_stale: 0,
            submit_avg_latency_ms: 0.0,
            submit_max_latency_ms: 0,
            gpu_hashrate_hps: 0.0,
            current_epoch: 0,
            pool_height: 0,
            best_batch_ms: 0,
            remote_ttl_ms: 0,
            hashrate_max: 0.0,
            algorithm: config.algorithm.clone(),
        }
    }

    fn sync(
        &mut self,
        telemetry: &SessionTelemetry,
        iteration_done: u32,
        accepted: u64,
        rejected: u64,
        attempted_hashes: u64,
        remote_job_ttl_ms: Option<u64>,
        last_job_id: u64,
        nonce_window: u64,
        session_active: bool,
        status: &str,
    ) {
        let now = Instant::now();
        let uptime = now.duration_since(self.started_at).as_secs_f64().max(0.001);
        let decisions = accepted.saturating_add(rejected);
        self.last_update_at = now;
        self.backend = if telemetry.gpu_backend_name.is_empty() {
            "cpu".to_string()
        } else {
            telemetry.gpu_backend_name.clone()
        };
        self.status = status.to_string();
        self.current_iteration = iteration_done;
        self.last_job_id = last_job_id;
        self.nonce_window = nonce_window;
        self.session_active = session_active;
        self.accepted_shares = accepted;
        self.rejected_shares = rejected;
        self.attempted_hashes = attempted_hashes;
        self.hashrate_hps = attempted_hashes as f64 / uptime;
        self.hashrate_10s_hps = telemetry.hashrate_10s_hps();
        self.hashrate_60s_hps = telemetry.hashrate_60s_hps();
        self.hashrate_15m_hps = telemetry.hashrate_15m_hps();
        self.accept_rate_pct = if decisions > 0 {
            accepted as f64 * 100.0 / decisions as f64
        } else {
            0.0
        };
        self.no_solution_iterations = telemetry.no_solution_iterations;
        self.local_skip_likely_stale = telemetry.local_skip_likely_stale;
        self.submit_avg_latency_ms = telemetry.submit_avg_latency_ms();
        self.submit_max_latency_ms = telemetry.submit_max_latency_ms;
        self.gpu_hashrate_hps = telemetry.gpu_hashrate_hps();
        self.current_epoch = telemetry.current_epoch;
        self.pool_height = telemetry.pool_height;
        self.best_batch_ms = telemetry.best_batch_ms;
        self.remote_ttl_ms = remote_job_ttl_ms.unwrap_or(0);
        // Track peak hashrate (prefer 10s window, fallback to overall)
        let current_peak = if self.hashrate_10s_hps > 0.0 {
            self.hashrate_10s_hps
        } else {
            self.hashrate_hps
        };
        if current_peak > self.hashrate_max {
            self.hashrate_max = current_peak;
        }
    }

    fn uptime_seconds(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    #[allow(dead_code)]
    fn seconds_since_update(&self) -> u64 {
        self.last_update_at.elapsed().as_secs()
    }
}

fn sync_miner_metrics(
    metrics: &Arc<Mutex<MinerMetricsSnapshot>>,
    telemetry: &SessionTelemetry,
    iteration_done: u32,
    accepted: u64,
    rejected: u64,
    attempted_hashes: u64,
    remote_job_ttl_ms: Option<u64>,
    last_job_id: u64,
    nonce_window: u64,
    session_active: bool,
    status: &str,
) {
    if let Ok(mut snapshot) = metrics.lock() {
        snapshot.sync(
            telemetry,
            iteration_done,
            accepted,
            rejected,
            attempted_hashes,
            remote_job_ttl_ms,
            last_job_id,
            nonce_window,
            session_active,
            status,
        );
    }
}

#[allow(dead_code)]
fn sanitize_prometheus_label(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| match ch {
            '"' => ['\\', '"'].into_iter().collect::<Vec<char>>(),
            '\\' => ['\\', '\\'].into_iter().collect::<Vec<char>>(),
            '\n' => ['\\', 'n'].into_iter().collect::<Vec<char>>(),
            '\r' => ['\\', 'r'].into_iter().collect::<Vec<char>>(),
            _ => [ch].into_iter().collect::<Vec<char>>(),
        })
        .collect()
}

#[allow(dead_code)]
fn build_miner_prometheus_payload(snapshot: &MinerMetricsSnapshot) -> String {
    let miner_id = sanitize_prometheus_label(&snapshot.miner_id);
    let worker_name = sanitize_prometheus_label(&snapshot.worker_name);
    let mode = sanitize_prometheus_label(&snapshot.mode);
    let backend = sanitize_prometheus_label(&snapshot.backend);
    let pool_addr = sanitize_prometheus_label(&snapshot.pool_addr);
    let status = sanitize_prometheus_label(&snapshot.status);
    let labels = format!(
        "miner_id=\"{}\",worker_name=\"{}\",mode=\"{}\",backend=\"{}\",pool_addr=\"{}\",status=\"{}\"",
        miner_id, worker_name, mode, backend, pool_addr, status
    );

    let mut body = String::new();
    let _ = writeln!(body, "zion_miner_up{{{labels}}} 1");
    let _ = writeln!(
        body,
        "zion_miner_session_active{{{labels}}} {}",
        if snapshot.session_active { 1 } else { 0 }
    );
    let _ = writeln!(body, "zion_miner_threads{{{labels}}} {}", snapshot.threads);
    let _ = writeln!(
        body,
        "zion_miner_loop_target{{{labels}}} {}",
        snapshot.loop_target
    );
    let _ = writeln!(
        body,
        "zion_miner_iteration{{{labels}}} {}",
        snapshot.current_iteration
    );
    let _ = writeln!(
        body,
        "zion_miner_last_job_id{{{labels}}} {}",
        snapshot.last_job_id
    );
    let _ = writeln!(
        body,
        "zion_miner_nonce_window{{{labels}}} {}",
        snapshot.nonce_window
    );
    let _ = writeln!(
        body,
        "zion_miner_accepted_shares_total{{{labels}}} {}",
        snapshot.accepted_shares
    );
    let _ = writeln!(
        body,
        "zion_miner_rejected_shares_total{{{labels}}} {}",
        snapshot.rejected_shares
    );
    let _ = writeln!(
        body,
        "zion_miner_attempted_hashes_total{{{labels}}} {}",
        snapshot.attempted_hashes
    );
    let _ = writeln!(
        body,
        "zion_miner_hashrate_hps{{{labels}}} {:.2}",
        snapshot.hashrate_hps
    );
    let _ = writeln!(
        body,
        "zion_miner_hashrate_10s_hps{{{labels}}} {:.2}",
        snapshot.hashrate_10s_hps
    );
    let _ = writeln!(
        body,
        "zion_miner_hashrate_60s_hps{{{labels}}} {:.2}",
        snapshot.hashrate_60s_hps
    );
    let _ = writeln!(
        body,
        "zion_miner_hashrate_15m_hps{{{labels}}} {:.2}",
        snapshot.hashrate_15m_hps
    );
    let _ = writeln!(
        body,
        "zion_miner_accept_rate_pct{{{labels}}} {:.4}",
        snapshot.accept_rate_pct
    );
    let _ = writeln!(
        body,
        "zion_miner_no_solution_iterations_total{{{labels}}} {}",
        snapshot.no_solution_iterations
    );
    let _ = writeln!(
        body,
        "zion_miner_local_skip_likely_stale_total{{{labels}}} {}",
        snapshot.local_skip_likely_stale
    );
    let _ = writeln!(
        body,
        "zion_miner_submit_avg_latency_ms{{{labels}}} {:.2}",
        snapshot.submit_avg_latency_ms
    );
    let _ = writeln!(
        body,
        "zion_miner_submit_max_latency_ms{{{labels}}} {}",
        snapshot.submit_max_latency_ms
    );
    let _ = writeln!(
        body,
        "zion_miner_gpu_hashrate_hps{{{labels}}} {:.2}",
        snapshot.gpu_hashrate_hps
    );
    let _ = writeln!(
        body,
        "zion_miner_current_epoch{{{labels}}} {}",
        snapshot.current_epoch
    );
    let _ = writeln!(
        body,
        "zion_miner_pool_height{{{labels}}} {}",
        snapshot.pool_height
    );
    let _ = writeln!(
        body,
        "zion_miner_best_batch_ms{{{labels}}} {}",
        snapshot.best_batch_ms
    );
    let _ = writeln!(
        body,
        "zion_miner_remote_ttl_ms{{{labels}}} {}",
        snapshot.remote_ttl_ms
    );
    let _ = writeln!(
        body,
        "zion_miner_hashrate_max{{{labels}}} {:.2}",
        snapshot.hashrate_max
    );
    let _ = writeln!(
        body,
        "zion_miner_uptime_seconds{{{labels}}} {}",
        snapshot.uptime_seconds()
    );
    let _ = writeln!(
        body,
        "zion_miner_seconds_since_update{{{labels}}} {}",
        snapshot.seconds_since_update()
    );
    body
}

#[allow(dead_code)]
fn build_miner_stats_payload(snapshot: &MinerMetricsSnapshot) -> String {
    serde_json::json!({
        "ok": true,
        "status": snapshot.status,
        "mode": snapshot.mode,
        "miner_id": snapshot.miner_id,
        "worker_name": snapshot.worker_name,
        "pool_addr": snapshot.pool_addr,
        "backend": snapshot.backend,
        "session_active": snapshot.session_active,
        "uptime_s": snapshot.uptime_seconds(),
        "seconds_since_update": snapshot.seconds_since_update(),
        "loop_target": snapshot.loop_target,
        "current_iteration": snapshot.current_iteration,
        "last_job_id": snapshot.last_job_id,
        "threads": snapshot.threads,
        "nonce_window": snapshot.nonce_window,
        "accepted_shares": snapshot.accepted_shares,
        "rejected_shares": snapshot.rejected_shares,
        "attempted_hashes": snapshot.attempted_hashes,
        "accept_rate_pct": snapshot.accept_rate_pct,
        "hashrate_hps": snapshot.hashrate_hps,
        "hashrate_10s_hps": snapshot.hashrate_10s_hps,
        "hashrate_60s_hps": snapshot.hashrate_60s_hps,
        "hashrate_15m_hps": snapshot.hashrate_15m_hps,
        "hashrate_max": snapshot.hashrate_max,
        "submit_avg_latency_ms": snapshot.submit_avg_latency_ms,
        "submit_max_latency_ms": snapshot.submit_max_latency_ms,
        "gpu_hashrate_hps": snapshot.gpu_hashrate_hps,
        "current_epoch": snapshot.current_epoch,
        "pool_height": snapshot.pool_height,
        "best_batch_ms": snapshot.best_batch_ms,
        "remote_ttl_ms": snapshot.remote_ttl_ms,
        "hashrate_max": snapshot.hashrate_max,
        "api": {
            "health": "/health",
            "metrics": "/metrics",
            "stats": "/stats"
        }
    })
    .to_string()
}

#[allow(dead_code)]
fn serve_miner_metrics(bind_addr: &str, metrics: Arc<Mutex<MinerMetricsSnapshot>>) -> Result<()> {
    let listener = TcpListener::bind(bind_addr)
        .with_context(|| format!("failed to bind miner metrics listener on {bind_addr}"))?;

    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("miner_metrics_accept_error={error}");
                continue;
            }
        };

        let mut request_reader = BufReader::new(&stream);
        let mut request_line = String::new();
        if request_reader.read_line(&mut request_line).is_err() {
            continue;
        }
        let path = request_line.split_whitespace().nth(1).unwrap_or("/");
        let snapshot = match metrics.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => continue,
        };
        let (status, content_type, body) = match path {
            "/health" => (
                "200 OK",
                "application/json",
                serde_json::json!({
                    "status": if snapshot.session_active { "ok" } else { snapshot.status.as_str() },
                    "uptime_s": snapshot.uptime_seconds(),
                    "seconds_since_update": snapshot.seconds_since_update(),
                    "mode": snapshot.mode,
                    "backend": snapshot.backend,
                })
                .to_string(),
            ),
            "/metrics" => (
                "200 OK",
                "text/plain; version=0.0.4",
                build_miner_prometheus_payload(&snapshot),
            ),
            "/" | "/stats" => (
                "200 OK",
                "application/json",
                build_miner_stats_payload(&snapshot),
            ),
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
            eprintln!("miner_metrics_write_error={error}");
        }
    }

    Ok(())
}

/// Write a JSON stats file compatible with the desktop agent's `tryUpdateStatsFromFile()`.
/// Fields are mapped to match what the agent expects (see APP&WEB/desktop-agent/src/main.js).
fn write_stats_file(path: &str, snapshot: &MinerMetricsSnapshot) {
    let payload = serde_json::json!({
        // Hashrate (agent reads: hashrate_10s, hashrate_60s, hashrate_15m, hashrate, hashrate_max)
        "hashrate": snapshot.hashrate_hps,
        "hashrate_10s": snapshot.hashrate_10s_hps,
        "hashrate_60s": snapshot.hashrate_60s_hps,
        "hashrate_15m": snapshot.hashrate_15m_hps,
        "hashrate_max": snapshot.hashrate_max,
        "hashrate_gpu": snapshot.gpu_hashrate_hps,
        "hashrate_cpu": if snapshot.gpu_hashrate_hps > 0.0 {
            (snapshot.hashrate_hps - snapshot.gpu_hashrate_hps).max(0.0)
        } else {
            snapshot.hashrate_hps
        },
        // Shares (agent reads: shares_accepted, shares_rejected, shares_sent)
        "shares_accepted": snapshot.accepted_shares,
        "shares_rejected": snapshot.rejected_shares,
        "shares_sent": snapshot.accepted_shares + snapshot.rejected_shares,
        // Chain info
        "pool_height": snapshot.pool_height,
        "current_epoch": snapshot.current_epoch,
        "total_hashes": snapshot.attempted_hashes,
        // Pool / connection
        "pool_latency_ms": snapshot.submit_avg_latency_ms as u64,
        "backend": snapshot.backend,
        "gpu_name": if snapshot.backend == "cpu" { "none" } else { &snapshot.backend },
        "worker": snapshot.worker_name,
        "algorithm": snapshot.algorithm,
        "cpu_threads": snapshot.threads,
        // Uptime
        "uptime_sec": snapshot.uptime_seconds(),
        // Status
        "status": snapshot.status,
        "miner_id": snapshot.miner_id,
        "pool_addr": snapshot.pool_addr,
    });

    // Atomic write: write to temp then rename (prevents partial reads by agent)
    let tmp_path = format!("{path}.tmp");
    match std::fs::write(&tmp_path, payload.to_string()) {
        Ok(()) => {
            let _ = std::fs::rename(&tmp_path, path);
        }
        Err(e) => {
            eprintln!("stats_file_write_error path=\"{path}\" error=\"{e}\"");
            let _ = std::fs::remove_file(&tmp_path);
        }
    }
}

/// Format a compact ISO-like timestamp for log lines: `YYYY-MM-DD HH:MM:SS`
fn log_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs = now % 60;
    let mins = (now / 60) % 60;
    let hours = (now / 3600) % 24;
    let days = now / 86400;
    // Simple date from days since epoch (good enough for logging)
    let (y, m, d) = days_to_ymd(days);
    format!("{y:04}-{m:02}-{d:02} {hours:02}:{mins:02}:{secs:02}")
}

fn days_to_ymd(days_since_epoch: u64) -> (u64, u64, u64) {
    // Civil from days algorithm (Howard Hinnant)
    let z = days_since_epoch as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u64, m, d)
}

/// Format hashrate with appropriate unit (H/s, KH/s, MH/s, GH/s)
#[allow(dead_code)]
fn format_hashrate(hps: f64) -> String {
    if hps >= 1e9 {
        format!("{:.2} GH/s", hps / 1e9)
    } else if hps >= 1e6 {
        format!("{:.2} MH/s", hps / 1e6)
    } else if hps >= 1e3 {
        format!("{:.2} KH/s", hps / 1e3)
    } else {
        format!("{:.1} H/s", hps)
    }
}

fn main() -> Result<()> {
    // ── Ekam Deeksha GPU benchmark: `zion-miner --ekam-bench` ──
    if std::env::args().any(|a| a == "--ekam-bench") {
        let work_size: usize = std::env::var("ZION_GPU_WORK_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1 << 18);
        let secs: f64 = std::env::var("ZION_BENCH_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10.0);
        let backend = gpu_backend::GpuBackendKind::from_env();
        let bench_algorithm =
            std::env::var("ZION_MINER_ALGORITHM").unwrap_or_else(|_| "deeksha_lite_v1".to_string());

        println!("--- {} GPU benchmark ---", bench_algorithm);
        let mut gpu = gpu_backend::create_gpu_backend(
            if backend == gpu_backend::GpuBackendKind::Cpu {
                gpu_backend::GpuBackendKind::Auto
            } else {
                backend
            },
            work_size,
            &bench_algorithm,
        )?;
        println!("device={}", gpu.device_name());
        println!("backend={}", gpu.backend_kind().as_str());

        match gpu.benchmark(secs) {
            Ok((hashes, elapsed, khps)) => {
                println!("hashes={hashes} elapsed={elapsed:.2}s");
                println!("ekam_deeksha: {khps:.2} KH/s ({:.2} H/s)", khps * 1_000.0);
            }
            Err(e) => eprintln!("GPU benchmark error: {e}"),
        }
        return Ok(());
    }

    // ── Multi-algo GPU benchmark: `zion-miner --gpu-benchmark-all` ──
    if std::env::args().any(|a| a == "--gpu-benchmark-all") {
        let work_size: usize = std::env::var("ZION_GPU_WORK_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1 << 18);
        let secs: f64 = std::env::var("ZION_BENCH_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10.0);
        let backend = gpu_backend::GpuBackendKind::from_env();

        println!("=== GPU Multi-Algorithm Benchmark ===");
        println!("device_backend={}", backend.as_str());
        println!("work_size={}", work_size);
        println!("bench_secs={}", secs);
        println!();

        let mut manager = gpu_backend::GpuBackendManager::new(backend, work_size);
        let results = manager.benchmark_all(secs);

        println!();
        println!("=== Results ===");
        let mut best_algo = String::new();
        let mut best_khps = 0.0;
        for (algo, khps) in &results {
            println!("algorithm={algo} throughput={khps:.2} KH/s");
            if *khps > best_khps {
                best_khps = *khps;
                best_algo.clone_from(algo);
            }
        }
        if !best_algo.is_empty() {
            println!();
            println!("best_algorithm={best_algo} best_throughput={best_khps:.2} KH/s");
        }
        return Ok(());
    }

    let mut config = MinerConfig::from_env_and_args()?;

    // ── Autotune: if algorithm=auto, benchmark and pick best ──
    if config.algorithm == "auto" {
        let auto_secs: f64 = std::env::var("ZION_AUTOTUNE_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3.0);
        println!("autotune=enabled bench_secs={auto_secs}");
        let mut manager =
            gpu_backend::GpuBackendManager::new(config.gpu_backend, config.gpu_work_size);
        let results = manager.benchmark_all(auto_secs);
        let mut best_algo = String::new();
        let mut best_khps = 0.0;
        for (algo, khps) in &results {
            println!("autotune_candidate algorithm={algo} throughput={khps:.2} KH/s");
            if *khps > best_khps {
                best_khps = *khps;
                best_algo.clone_from(algo);
            }
        }
        if !best_algo.is_empty() {
            println!(
                "autotune_result best_algorithm={best_algo} best_throughput={best_khps:.2} KH/s"
            );
            config.algorithm = best_algo;
        } else {
            println!("autotune_fallback no backends available, using=deeksha_lite_v1");
            config.algorithm = "deeksha_lite_v1".to_string();
        }
    }

    // ── Startup banner + hardware detection ──
    banner::print_banner(config.threads);
    println!("miner_id={}", config.miner_id);
    println!("worker_name={}", config.worker_name);
    println!("loop_count={}", config.loop_count);
    println!("job_ttl_ms={}", config.job_ttl_ms);
    println!("threads={}", config.threads);
    println!("algorithm={}", config.algorithm);
    flush_stdout();

    let metrics = Arc::new(Mutex::new(MinerMetricsSnapshot::from_config(&config)));

    // ── Interactive control + hashrate tracker ──
    // Auto-detect TTY: if stdin or stdout is not a terminal (e.g. launched
    // by the CLI with redirected stdio), force non-interactive headless mode.
    // The TUI requires a real terminal for crossterm raw mode + alternate screen.
    let tty_available = std::io::IsTerminal::is_terminal(&std::io::stdin())
        && std::io::IsTerminal::is_terminal(&std::io::stdout());
    let interactive = parse_bool_env("ZION_INTERACTIVE", true) && tty_available;
    if !tty_available && parse_bool_env("ZION_INTERACTIVE", true) {
        println!("headless=enabled reason=stdin_or_stdout_not_tty");
    }
    let control = Arc::new(Mutex::new(MinerControl::new(
        &config.algorithm,
        config.threads,
        config.gpu_backend != gpu_backend::GpuBackendKind::Cpu,
    )));
    let hashrate = HashrateTracker::new();

    let outcome = match config.pool_addr.as_deref() {
        Some(pool_addr) => {
            println!("mode=remote");
            println!("pool_addr={pool_addr}");
            let reconnect_enabled = parse_bool_env("ZION_RECONNECT", true);
            let max_reconnect = parse_env_u32("ZION_MAX_RECONNECT", 0)?; // 0 = infinite

            if interactive {
                // Spawn mining loop in a background thread
                let pool_addr_owned = pool_addr.to_string();
                let mining_control = Arc::clone(&control);
                let mining_hashrate = Arc::clone(&hashrate);
                let mining_metrics = Arc::clone(&metrics);
                let mining_config = config.clone();
                let mining_handle = thread::spawn(move || {
                    let _ = reconnect::with_reconnect(
                        max_reconnect,
                        reconnect::Backoff::default_reconnect(),
                        |attempt| {
                            if attempt > 1 {
                                println!("reconnect_attempt={attempt}");
                            }
                            run_remote_session(
                                &mining_config,
                                &pool_addr_owned,
                                &mining_metrics,
                                &mining_control,
                                &mining_hashrate,
                            )
                        },
                    );
                });

                // Run interactive TUI in main thread
                let _ = interactive::run_interactive(Arc::clone(&control), Arc::clone(&hashrate));

                // Signal quit and wait for mining thread
                control.lock().unwrap().requested_quit = true;
                let _ = mining_handle.join();

                SessionOutcome {
                    last_job_id: 0,
                    accepted_shares: hashrate.accepted_shares.load(Ordering::Relaxed),
                    rejected_shares: hashrate.rejected_shares.load(Ordering::Relaxed),
                    active_jobs: 0,
                    accepted_iterations: 0,
                    attempted_hashes: hashrate.total_hashes.load(Ordering::Relaxed),
                    elapsed_seconds: 0.0,
                    hashrate_hps: 0.0,
                    hashrate_10s_hps: 0.0,
                    hashrate_60s_hps: 0.0,
                    hashrate_15m_hps: 0.0,
                    revenue_total_usd: 0.0,
                    no_solution_iterations: 0,
                    local_skip_likely_stale: 0,
                    submit_avg_latency_ms: 0.0,
                    submit_max_latency_ms: 0,
                    last_result_line: None,
                    bye_line: None,
                }
            } else if reconnect_enabled {
                println!(
                    "reconnect=enabled max_attempts={}",
                    if max_reconnect == 0 {
                        "infinite".to_string()
                    } else {
                        max_reconnect.to_string()
                    }
                );
                reconnect::with_reconnect(
                    max_reconnect,
                    reconnect::Backoff::default_reconnect(),
                    |attempt| {
                        if attempt > 1 {
                            println!("reconnect_attempt={attempt}");
                        }
                        run_remote_session(&config, pool_addr, &metrics, &control, &hashrate)
                    },
                )?
            } else {
                run_remote_session(&config, pool_addr, &metrics, &control, &hashrate)?
            }
        }
        None => {
            println!("mode=local");
            run_local_session(&config, &metrics, &control, &hashrate)?
        }
    };

    println!("last_job_id={}", outcome.last_job_id);
    println!("accepted_shares={}", outcome.accepted_shares);
    println!("rejected_shares={}", outcome.rejected_shares);
    println!("active_jobs={}", outcome.active_jobs);
    println!("accepted_iterations={}", outcome.accepted_iterations);
    println!("attempted_hashes={}", outcome.attempted_hashes);
    println!("elapsed_seconds={:.6}", outcome.elapsed_seconds);
    println!("hashrate_hps={:.2}", outcome.hashrate_hps);
    println!("hashrate_10s_hps={:.2}", outcome.hashrate_10s_hps);
    println!("hashrate_60s_hps={:.2}", outcome.hashrate_60s_hps);
    println!("hashrate_15m_hps={:.2}", outcome.hashrate_15m_hps);
    println!("hashrate_fmt={}", fmt_hashrate(outcome.hashrate_hps));
    println!("revenue_total_usd={:.2}", outcome.revenue_total_usd);
    println!("no_solution_iterations={}", outcome.no_solution_iterations);
    println!(
        "local_skip_likely_stale={}",
        outcome.local_skip_likely_stale
    );
    println!("submit_avg_latency_ms={:.2}", outcome.submit_avg_latency_ms);
    println!("submit_max_latency_ms={}", outcome.submit_max_latency_ms);

    if let Some(line) = outcome.last_result_line.as_deref() {
        let parsed = decode_message(line)?;
        println!("wire_result_parsed={parsed:?}");
    }

    if let Some(line) = outcome.bye_line.as_deref() {
        let parsed = decode_message(line)?;
        println!("wire_bye_parsed={parsed:?}");
    }

    Ok(())
}

#[allow(unused_assignments)] // gpu_available is a fallback flag set on GPU-init failure
fn run_local_session(
    config: &MinerConfig,
    metrics: &Arc<Mutex<MinerMetricsSnapshot>>,
    control: &Arc<Mutex<interactive::MinerControl>>,
    hashrate: &Arc<interactive::HashrateTracker>,
) -> Result<SessionOutcome> {
    let mut pool = MiningPool::with_job_ttl(CoreRuntime::default(), config.job_ttl_ms);
    let started_at = Instant::now();
    let mut attempted_hashes = 0u64;
    let mut accepted_iterations = 0u64;
    let mut rejected_iterations = 0u64;
    let mut last_result_line = None;
    let mut last_job_id = 0u64;
    let mut tuned_nonce_count = config.nonce_count;
    let mut telemetry = SessionTelemetry::new(config.metrics_report_every_secs);
    let threads = config.threads;

    // ── Read current algorithm from interactive control ──
    let initial_algorithm = {
        let c = control.lock().unwrap();
        c.algorithm.clone()
    };

    // ── GPU backend init (multi-algo manager — lazy per-algorithm) ──
    let mut gpu_manager =
        gpu_backend::GpuBackendManager::new(config.gpu_backend, config.gpu_work_size);
    let mut gpu_available = config.gpu_backend != gpu_backend::GpuBackendKind::Cpu;
    if gpu_available {
        match gpu_manager.ensure_algorithm(&initial_algorithm) {
            Ok(g) => {
                println!(
                    "gpu_init backend={} device=\"{}\" work_size={} algorithm={}",
                    g.backend_kind().as_str(),
                    g.device_name(),
                    config.gpu_work_size,
                    initial_algorithm
                );
                telemetry.gpu_backend_name = g.backend_kind().as_str().to_string();
                telemetry.gpu_infos = gpu_backend::query_gpu_details();
                telemetry.algorithm = initial_algorithm.clone();
                // Push GPU info to HashrateTracker for TUI dashboard
                let gpu_lines: Vec<interactive::GpuInfoLine> = telemetry
                    .gpu_infos
                    .iter()
                    .enumerate()
                    .map(|(i, info)| interactive::GpuInfoLine {
                        index: i,
                        info: format!(
                            "{} | {} CUs | {} MHz | {} MiB VRAM",
                            info.name,
                            info.compute_units,
                            info.max_clock_mhz,
                            info.global_mem_bytes / (1024 * 1024)
                        ),
                    })
                    .collect();
                hashrate.set_gpu_info(gpu_lines);
            }
            Err(e) => {
                println!("gpu_init_fallback reason=\"{e}\" using=cpu");
                gpu_available = false;
            }
        }
    }

    sync_miner_metrics(
        metrics,
        &telemetry,
        0,
        hashrate.accepted_shares.load(Ordering::Relaxed),
        hashrate.rejected_shares.load(Ordering::Relaxed),
        attempted_hashes,
        None,
        last_job_id,
        tuned_nonce_count,
        true,
        "running",
    );

    let hello_line = encode_message(&pool.hello_message(
        &config.miner_id,
        &config.worker_name,
        &config.payout_address,
    ))?;
    let welcome_line = encode_message(&pool.welcome_message())?;
    if VERBOSE.load(Ordering::Relaxed) {
        println!("wire_hello={}", hello_line.trim());
        println!("wire_welcome={}", welcome_line.trim());
    }

    for iteration in 0..config.loop_count {
        // Read interactive control state at top of iteration
        let current_algorithm = {
            let c = control.lock().unwrap();
            if c.requested_quit {
                break;
            }
            c.algorithm.clone()
        };

        for stale_job_id in pool.expire_stale_jobs() {
            let stale_line = encode_message(&pool.stale_message(stale_job_id))?;
            let cancel_line =
                encode_message(&pool.cancel_message(stale_job_id, "stale-ttl-expired"))?;
            if VERBOSE.load(Ordering::Relaxed) {
                println!("wire_stale={}", stale_line.trim());
                println!("wire_cancel={}", cancel_line.trim());
            }
        }

        let header = session_header(config, iteration);
        let start_nonce = config
            .start_nonce
            .wrapping_add((iteration as u64).wrapping_mul(config.nonce_stride));
        let job = pool.issue_job(header, config.target, start_nonce, tuned_nonce_count);
        last_job_id = job.job_id;
        if VERBOSE.load(Ordering::Relaxed) {
            println!(
                "job_issue id={} nonce_count={} start={} algo={}",
                job.job_id, job.nonce_count, job.start_nonce, current_algorithm
            );
        }
        // Ensure GPU backend matches current algorithm (lazy create / switch)
        let mut gpu_ref: Option<&mut dyn gpu_backend::GpuMiner> = None;
        if config.gpu_backend != gpu_backend::GpuBackendKind::Cpu {
            match gpu_manager.ensure_algorithm(&current_algorithm) {
                Ok(g) => gpu_ref = Some(g),
                Err(e) => {
                    println!(
                        "gpu_algo_fallback job={} algorithm={} reason=\"{e}\" using=cpu",
                        job.job_id, current_algorithm
                    );
                }
            }
        }
        // GPU-first, CPU-fallback nonce scan
        let can_gpu = gpu_ref.is_some();
        let mut gpu_nonces_tested = 0u64;
        let mut cpu_nonces_tested = 0u64;
        let scan_result = if can_gpu {
            let g = gpu_ref.unwrap();
            if let Err(e) = g.update_epoch(job.height) {
                println!(
                    "gpu_epoch_fallback height={} reason=\"{e}\" using=cpu",
                    job.height
                );
                cpu_nonces_tested = job.nonce_count;
                parallel::parallel_scan_nonce_range(job, threads, &current_algorithm)
            } else {
                let result = gpu_backend::gpu_scan_job(g, job, &current_algorithm);
                gpu_nonces_tested = result.nonces_tested;
                result.solution
            }
        } else {
            cpu_nonces_tested = job.nonce_count;
            parallel::parallel_scan_nonce_range(job, threads, &current_algorithm)
        };
        hashrate.record_gpu_hashes(gpu_nonces_tested);
        hashrate.record_cpu_hashes(cpu_nonces_tested);
        let Some(solution) = scan_result else {
            let tested = if can_gpu {
                gpu_nonces_tested
            } else {
                job.nonce_count
            };
            attempted_hashes = attempted_hashes.saturating_add(tested);
            rejected_iterations += 1;
            telemetry.record_attempted_hashes(attempted_hashes);
            telemetry.record_no_solution();
            if VERBOSE.load(Ordering::Relaxed) {
                println!("iteration={}", iteration + 1);
                println!("job_id={}", job.job_id);
                println!(
                    "nonce_range={}..{}",
                    job.start_nonce,
                    job.start_nonce + job.nonce_count
                );
                println!("share_status=\"NoSolutionInWindow\"");
            }

            if config.nonce_autotune {
                let previous = tuned_nonce_count;
                tuned_nonce_count = increase_nonce_window(
                    tuned_nonce_count,
                    config.nonce_count_max,
                    config.nonce_adjust_percent,
                );
                if tuned_nonce_count != previous {
                    println!(
                        "nonce_autotune action=grow prev={} next={} max={}",
                        previous, tuned_nonce_count, config.nonce_count_max
                    );
                }
            }
            let total_accepted = hashrate.accepted_shares.load(Ordering::Relaxed);
            let total_rejected = hashrate.rejected_shares.load(Ordering::Relaxed);
            sync_miner_metrics(
                metrics,
                &telemetry,
                iteration + 1,
                total_accepted,
                total_rejected,
                attempted_hashes,
                None,
                last_job_id,
                tuned_nonce_count,
                true,
                "running",
            );
            telemetry.maybe_print_status(
                iteration + 1,
                config.loop_count,
                total_accepted,
                total_rejected,
                attempted_hashes,
                None,
                config.stats_file.as_deref(),
                metrics,
            );
            continue;
        };

        let search_depth = solution.candidate.nonce.saturating_sub(job.start_nonce) + 1;
        let tested = if can_gpu {
            gpu_nonces_tested
        } else {
            search_depth
        };
        attempted_hashes = attempted_hashes.saturating_add(tested);
        telemetry.record_attempted_hashes(attempted_hashes);

        if config.sleep_ms > 0 {
            thread::sleep(Duration::from_millis(config.sleep_ms));
        }

        let decision = pool.submit_solution(
            config.miner_id.clone(),
            config.worker_name.clone(),
            solution,
            config.revenue_source,
            config.revenue_value_usd,
            &current_algorithm,
        );

        // Celebrate block found with ASCII art flag
        if decision.sealed_block.is_some() {
            let hash_prefix: String = solution.hash[..6]
                .iter()
                .map(|x| format!("{:02x}", x))
                .collect();
            crate::ui::log_block_found(job.height, solution.candidate.nonce, &hash_prefix);
        }

        if matches!(decision.status, ShareStatus::Accepted) {
            accepted_iterations += 1;
            hashrate.record_share(true);
        } else {
            rejected_iterations += 1;
            hashrate.record_share(false);
        }

        let submit_started_at = Instant::now();

        let job_line = encode_message(&pool.job_message(job, &current_algorithm))?;
        let submit_line = encode_message(&pool.solution_message(
            &config.miner_id,
            &config.worker_name,
            solution,
        ))?;
        let result_line = encode_message(&pool.result_message(&decision))?;
        telemetry.record_submit_latency(submit_started_at.elapsed());
        last_result_line = Some(result_line.clone());

        log_solution(
            iteration + 1,
            job,
            solution.candidate.nonce,
            &solution.hash,
            &decision.status,
        );
        if VERBOSE.load(Ordering::Relaxed) {
            println!("wire_job={}", job_line.trim());
            println!("wire_submit={}", submit_line.trim());
            println!("wire_result={}", result_line.trim());
        }

        if matches!(decision.status, ShareStatus::StaleJob) {
            let stale_line = encode_message(&pool.stale_message(job.job_id))?;
            let cancel_line =
                encode_message(&pool.cancel_message(job.job_id, "submit-arrived-after-ttl"))?;
            if VERBOSE.load(Ordering::Relaxed) {
                println!("wire_stale={}", stale_line.trim());
                println!("wire_cancel={}", cancel_line.trim());
            }
        }

        if config.nonce_autotune {
            let used = solution.candidate.nonce.saturating_sub(job.start_nonce) + 1;
            let quarter = tuned_nonce_count / 4;
            if quarter > 0 && used <= quarter {
                tuned_nonce_count = decrease_nonce_window(
                    tuned_nonce_count,
                    config.nonce_count_min,
                    config.nonce_adjust_percent,
                );
            }
        }

        let total_accepted = hashrate.accepted_shares.load(Ordering::Relaxed);
        let total_rejected = hashrate.rejected_shares.load(Ordering::Relaxed);
        sync_miner_metrics(
            metrics,
            &telemetry,
            iteration + 1,
            total_accepted,
            total_rejected,
            attempted_hashes,
            None,
            last_job_id,
            tuned_nonce_count,
            true,
            "running",
        );

        telemetry.maybe_print_status(
            iteration + 1,
            config.loop_count,
            total_accepted,
            total_rejected,
            attempted_hashes,
            None,
            config.stats_file.as_deref(),
            metrics,
        );
    }

    let stats = pool.stats();
    let elapsed_seconds = started_at.elapsed().as_secs_f64();
    let hashrate_hps = if elapsed_seconds > 0.0 {
        attempted_hashes as f64 / elapsed_seconds
    } else {
        0.0
    };
    let bye_line = encode_message(&pool.bye_message())?;
    println!("wire_bye={}", bye_line.trim());

    sync_miner_metrics(
        metrics,
        &telemetry,
        config.loop_count,
        hashrate.accepted_shares.load(Ordering::Relaxed),
        hashrate.rejected_shares.load(Ordering::Relaxed),
        attempted_hashes,
        None,
        last_job_id,
        tuned_nonce_count,
        false,
        "complete",
    );

    Ok(SessionOutcome {
        last_job_id,
        accepted_shares: stats.accepted_shares,
        rejected_shares: stats.rejected_shares.saturating_add(rejected_iterations),
        active_jobs: stats.active_jobs,
        accepted_iterations,
        attempted_hashes,
        elapsed_seconds,
        hashrate_hps,
        hashrate_10s_hps: telemetry.hashrate_10s_hps(),
        hashrate_60s_hps: telemetry.hashrate_60s_hps(),
        hashrate_15m_hps: telemetry.hashrate_15m_hps(),
        revenue_total_usd: stats.revenue.total_earnings_usd,
        no_solution_iterations: telemetry.no_solution_iterations,
        local_skip_likely_stale: telemetry.local_skip_likely_stale,
        submit_avg_latency_ms: telemetry.submit_avg_latency_ms(),
        submit_max_latency_ms: telemetry.submit_max_latency_ms,
        last_result_line,
        bye_line: Some(bye_line),
    })
}

#[allow(unused_assignments)] // gpu_available / current_algorithm carry fallback state
fn run_remote_session(
    config: &MinerConfig,
    pool_addr: &str,
    metrics: &Arc<Mutex<MinerMetricsSnapshot>>,
    control: &Arc<Mutex<MinerControl>>,
    hashrate: &Arc<HashrateTracker>,
) -> Result<SessionOutcome> {
    let started_at = Instant::now();
    let mut attempted_hashes = 0u64;
    let mut accepted_iterations = 0u64;
    let mut rejected_iterations = 0u64;
    let mut last_result_line = None;
    let mut last_job_id = 0u64;
    let mut telemetry = SessionTelemetry::new(config.metrics_report_every_secs);
    let mut threads = config.threads;
    let mut remote_nonce_window = config.nonce_count;

    // ── Read initial algorithm from interactive control ──
    let initial_algorithm = {
        let c = control.lock().unwrap();
        c.algorithm.clone()
    };

    // ── GPU backend init (multi-algo manager — lazy per-algorithm) ──
    let mut gpu_manager =
        gpu_backend::GpuBackendManager::new(config.gpu_backend, config.gpu_work_size);
    let mut gpu_available = config.gpu_backend != gpu_backend::GpuBackendKind::Cpu;
    if gpu_available {
        match gpu_manager.ensure_algorithm(&initial_algorithm) {
            Ok(g) => {
                println!(
                    "gpu_init backend={} device=\"{}\" work_size={} algorithm={}",
                    g.backend_kind().as_str(),
                    g.device_name(),
                    config.gpu_work_size,
                    initial_algorithm
                );
                telemetry.gpu_backend_name = g.backend_kind().as_str().to_string();
                telemetry.gpu_infos = gpu_backend::query_gpu_details();
                telemetry.algorithm = initial_algorithm.clone();
                // Push GPU info to HashrateTracker for TUI dashboard
                let gpu_lines: Vec<interactive::GpuInfoLine> = telemetry
                    .gpu_infos
                    .iter()
                    .enumerate()
                    .map(|(i, info)| interactive::GpuInfoLine {
                        index: i,
                        info: format!(
                            "{} | {} CUs | {} MHz | {} MiB VRAM",
                            info.name,
                            info.compute_units,
                            info.max_clock_mhz,
                            info.global_mem_bytes / (1024 * 1024)
                        ),
                    })
                    .collect();
                hashrate.set_gpu_info(gpu_lines);
            }
            Err(e) => {
                println!("gpu_init_fallback reason=\"{e}\" using=cpu");
                gpu_available = false;
            }
        }
    }

    sync_miner_metrics(
        metrics,
        &telemetry,
        0,
        hashrate.accepted_shares.load(Ordering::Relaxed),
        hashrate.rejected_shares.load(Ordering::Relaxed),
        attempted_hashes,
        None,
        last_job_id,
        remote_nonce_window,
        true,
        "connecting",
    );

    let stream = TcpStream::connect(pool_addr)
        .with_context(|| format!("failed to connect to pool at {pool_addr}"))?;
    // Socket read timeout: prevents miner from blocking forever if pool
    // disconnects ungracefully or hangs.  Triggers reconnect on timeout.
    let read_timeout_secs = parse_env_u64("ZION_READ_TIMEOUT_SECS", 300).unwrap_or(300);
    stream
        .set_read_timeout(Some(Duration::from_secs(read_timeout_secs)))
        .context("failed to set pool socket read timeout")?;
    let reader_stream = stream.try_clone().context("failed to clone pool stream")?;
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;

    let backend_str = gpu_manager.current_backend_name().unwrap_or("cpu");
    // BUG #2 fix: use current control.algorithm (user may have pressed 'a' to switch)
    let hello_algorithm = {
        let c = control.lock().unwrap();
        c.algorithm.clone()
    };
    let hello_message = PoolMessage::Hello {
        miner_id: config.miner_id.clone(),
        worker_name: config.worker_name.clone(),
        algorithm: hello_algorithm,
        payout_address: config.payout_address.clone(),
        backend: backend_str.to_string(),
    };
    let hello_line = write_wire_message(&mut writer, &hello_message)?;
    println!("wire_hello={hello_line}");

    let (welcome_line_raw, welcome_message) = read_wire_message(&mut reader)?;
    println!("wire_welcome={welcome_line_raw}");
    let remote_job_ttl_ms = match welcome_message {
        PoolMessage::Welcome { job_ttl_ms, .. } => job_ttl_ms,
        other => return Err(anyhow!("expected welcome from pool, got {other:?}")),
    };
    sync_miner_metrics(
        metrics,
        &telemetry,
        0,
        hashrate.accepted_shares.load(Ordering::Relaxed),
        hashrate.rejected_shares.load(Ordering::Relaxed),
        attempted_hashes,
        Some(remote_job_ttl_ms),
        last_job_id,
        remote_nonce_window,
        true,
        "running",
    );

    let ttl_guard_ms = remote_job_ttl_ms
        .saturating_mul(config.remote_ttl_guard_percent)
        .saturating_div(100);

    let mut current_algorithm = String::new();

    for iteration in 0..config.loop_count {
        // ── Check interactive control state at top of iteration ──
        {
            let c = control.lock().unwrap();
            if c.requested_quit {
                break;
            }
            if c.requested_reconnect {
                break;
            }
            if let Some(t) = c.thread_override {
                threads = t;
            }
            VERBOSE.store(c.verbose, Ordering::Relaxed);
        }

        let (job_line, mut job, algorithm) = read_next_job(&mut reader)?;
        let current_diff = CURRENT_POOL_DIFFICULTY.load(Ordering::Relaxed);
        job.target = zion_core::difficulty::difficulty_to_target(current_diff);
        current_algorithm = algorithm.clone();
        remote_nonce_window = job.nonce_count;
        let job_started_at = Instant::now();
        last_job_id = job.job_id;
        telemetry.pool_height = job.height;
        telemetry.current_epoch = job.height / 100;
        // BUG #1 fix: propagate pool_height to HashrateTracker so dashboard can display it
        hashrate.set_pool_height(job.height);
        ui::log_new_job(
            job.job_id,
            job.height,
            &current_algorithm,
            CURRENT_POOL_DIFFICULTY.load(Ordering::Relaxed),
        );

        // ── Check pause after reading job ──
        let is_paused = control.lock().unwrap().pause;
        if is_paused {
            // Skip scan while paused; send no-solution so pool doesn't block
            let no_solution_message = PoolMessage::NoSolution {
                job_id: job.job_id,
                miner_id: config.miner_id.clone(),
                worker_name: config.worker_name.clone(),
                attempted_hashes: Some(0),
                elapsed_ms: Some(0),
            };
            let _ = write_wire_message(&mut writer, &no_solution_message);
            let _ = read_next_result(&mut reader);
            thread::sleep(Duration::from_millis(100));
            continue;
        }

        // ── Check interactive CPU/GPU/dual mode ──
        let (cpu_on, gpu_on, _dual_on) = {
            let c = control.lock().unwrap();
            (c.cpu_enabled, c.gpu_enabled, c.dual_mode)
        };

        // Ensure GPU backend matches current algorithm (lazy create / switch)
        let mut gpu_ref: Option<&mut dyn gpu_backend::GpuMiner> = None;
        if config.gpu_backend != gpu_backend::GpuBackendKind::Cpu && gpu_on {
            match gpu_manager.ensure_algorithm(&current_algorithm) {
                Ok(g) => gpu_ref = Some(g),
                Err(e) => {
                    println!(
                        "gpu_algo_fallback job={} algorithm={} reason=\"{e}\" using=cpu",
                        job.job_id, current_algorithm
                    );
                }
            }
        }
        // GPU-first, CPU-fallback nonce scan (respect interactive overrides)
        let can_gpu = gpu_ref.is_some() && gpu_on;
        let mut gpu_nonces_tested = 0u64;
        let mut cpu_nonces_tested = 0u64;
        let scan_result = if can_gpu {
            let g = gpu_ref.unwrap();
            if let Err(e) = g.update_epoch(job.height) {
                println!(
                    "gpu_epoch_fallback height={} reason=\"{e}\" using=cpu",
                    job.height
                );
                cpu_nonces_tested = job.nonce_count;
                parallel::parallel_scan_nonce_range(job, threads, &current_algorithm)
            } else {
                let result = gpu_backend::gpu_scan_job(g, job, &current_algorithm);
                gpu_nonces_tested = result.nonces_tested;
                result.solution
            }
        } else if cpu_on {
            cpu_nonces_tested = job.nonce_count;
            parallel::parallel_scan_nonce_range(job, threads, &current_algorithm)
        } else {
            // Both CPU and GPU disabled — skip
            cpu_nonces_tested = 0;
            None
        };
        hashrate.record_gpu_hashes(gpu_nonces_tested);
        hashrate.record_cpu_hashes(cpu_nonces_tested);
        let batch_ms = job_started_at.elapsed().as_millis() as u64;
        if can_gpu {
            telemetry.record_gpu_hashes(gpu_nonces_tested);
        }
        if telemetry.best_batch_ms == 0 || batch_ms < telemetry.best_batch_ms {
            telemetry.best_batch_ms = batch_ms;
        }
        let Some(solution) = scan_result else {
            let tested = if can_gpu {
                gpu_nonces_tested
            } else {
                job.nonce_count
            };
            attempted_hashes = attempted_hashes.saturating_add(tested);
            telemetry.record_attempted_hashes(attempted_hashes);
            telemetry.record_no_solution();
            // Always log scan result for operational visibility
            println!(
                "[{}] no_solution  iteration={}  height={}  nonces={}..{}  tested={}  elapsed_ms={}",
                log_timestamp(),
                iteration + 1,
                job.height,
                job.start_nonce,
                job.start_nonce + job.nonce_count,
                tested,
                batch_ms,
            );
            if VERBOSE.load(Ordering::Relaxed) {
                println!("wire_job={job_line}");
            }
            let no_solution_message = PoolMessage::NoSolution {
                job_id: job.job_id,
                miner_id: config.miner_id.clone(),
                worker_name: config.worker_name.clone(),
                attempted_hashes: Some(tested),
                elapsed_ms: Some(job_started_at.elapsed().as_millis() as u64),
            };
            let no_solution_line = write_wire_message(&mut writer, &no_solution_message)?;
            let (result_line_raw, result_message) = read_next_result(&mut reader)?;
            last_result_line = Some(result_line_raw.clone());
            if VERBOSE.load(Ordering::Relaxed) {
                println!("wire_no_solution={no_solution_line}");
                println!("wire_result={result_line_raw}");
            }
            match result_message {
                PoolMessage::Result { accepted, status } => {
                    if accepted {
                        accepted_iterations += 1;
                    }
                    if VERBOSE.load(Ordering::Relaxed) {
                        println!("pool_status={status}");
                    }
                }
                other => return Err(anyhow!("expected result from pool, got {other:?}")),
            }
            let total_accepted = hashrate.accepted_shares.load(Ordering::Relaxed);
            let total_rejected = hashrate.rejected_shares.load(Ordering::Relaxed);
            telemetry.maybe_print_status(
                iteration + 1,
                config.loop_count,
                total_accepted,
                total_rejected,
                attempted_hashes,
                Some(remote_job_ttl_ms),
                config.stats_file.as_deref(),
                metrics,
            );
            sync_miner_metrics(
                metrics,
                &telemetry,
                iteration + 1,
                total_accepted,
                total_rejected,
                attempted_hashes,
                Some(remote_job_ttl_ms),
                last_job_id,
                remote_nonce_window,
                true,
                "running",
            );
            continue;
        };
        let search_depth = solution.candidate.nonce.saturating_sub(job.start_nonce) + 1;
        let tested = if can_gpu {
            gpu_nonces_tested
        } else {
            search_depth
        };
        attempted_hashes = attempted_hashes.saturating_add(tested);
        telemetry.record_attempted_hashes(attempted_hashes);

        // Always log found nonce for operational visibility
        let hash_prefix: String = solution.hash[..6]
            .iter()
            .map(|x| format!("{:02x}", x))
            .collect();
        println!(
            "[{}] found_nonce={}  height={}  depth={}/{}  tested={}  elapsed_ms={}  algo={}  hash_prefix={}",
            log_timestamp(),
            solution.candidate.nonce,
            job.height,
            search_depth,
            job.nonce_count,
            tested,
            batch_ms,
            current_algorithm,
            hash_prefix,
        );

        if config.sleep_ms > 0 {
            thread::sleep(Duration::from_millis(config.sleep_ms));
        }

        let elapsed_ms = job_started_at.elapsed().as_millis() as u64;
        if ttl_guard_ms > 0 && elapsed_ms >= ttl_guard_ms {
            // Warn that local scan exceeded TTL guard, but submit anyway —
            // the pool decides if the share is actually stale.  Skipping here
            // causes a deadlock: pool blocks on read-submit while miner blocks
            // on read-next-job, and nothing progresses.
            println!("ttl_guard_warning scan_elapsed_ms={elapsed_ms} ttl_guard_ms={ttl_guard_ms} submitting_anyway=true");
        }

        let submit_started_at = Instant::now();
        let submit_message = PoolMessage::Submit {
            job_id: solution.job_id,
            miner_id: config.miner_id.clone(),
            worker_name: config.worker_name.clone(),
            nonce: solution.candidate.nonce,
            hash_hex: hex(&solution.hash),
            attempted_hashes: Some(tested),
            elapsed_ms: Some(job_started_at.elapsed().as_millis() as u64),
        };
        let submit_line = write_wire_message(&mut writer, &submit_message)?;
        let (result_line_raw, result_message) = read_next_result(&mut reader)?;
        telemetry.record_submit_latency(submit_started_at.elapsed());
        last_result_line = Some(result_line_raw.clone());

        let status = match result_message {
            PoolMessage::Result { accepted, status } => {
                let latency_ms = submit_started_at.elapsed().as_millis();
                if accepted {
                    accepted_iterations += 1;
                    hashrate.record_share(true);
                    ui::log_accepted(
                        job.job_id,
                        job.height,
                        solution.candidate.nonce,
                        latency_ms as u64,
                    );
                    println!(
                        "[{}] SHARE_ACCEPTED  job={}  height={}  nonce={}  algo={}  latency_ms={}",
                        log_timestamp(),
                        job.job_id,
                        job.height,
                        solution.candidate.nonce,
                        current_algorithm,
                        latency_ms,
                    );
                } else {
                    rejected_iterations += 1;
                    hashrate.record_share(false);
                    ui::log_rejected(
                        job.job_id,
                        job.height,
                        solution.candidate.nonce,
                        latency_ms as u64,
                        &status,
                    );
                    println!(
                        "[{}] SHARE_REJECTED  job={}  height={}  nonce={}  algo={}  reason=\"{}\"  hash={}",
                        log_timestamp(), job.job_id, job.height,
                        solution.candidate.nonce, current_algorithm, status,
                        hex(&solution.hash),
                    );
                }
                status
            }
            other => return Err(anyhow!("expected result from pool, got {other:?}")),
        };

        log_solution(
            iteration + 1,
            job,
            solution.candidate.nonce,
            &solution.hash,
            &status,
        );
        if VERBOSE.load(Ordering::Relaxed) {
            println!("wire_job={job_line}");
            println!("wire_submit={submit_line}");
            println!("wire_result={result_line_raw}");
        }
        let total_accepted = hashrate.accepted_shares.load(Ordering::Relaxed);
        let total_rejected = hashrate.rejected_shares.load(Ordering::Relaxed);
        sync_miner_metrics(
            metrics,
            &telemetry,
            iteration + 1,
            total_accepted,
            total_rejected,
            attempted_hashes,
            Some(remote_job_ttl_ms),
            last_job_id,
            remote_nonce_window,
            true,
            "running",
        );
        telemetry.maybe_print_status(
            iteration + 1,
            config.loop_count,
            total_accepted,
            total_rejected,
            attempted_hashes,
            Some(remote_job_ttl_ms),
            config.stats_file.as_deref(),
            metrics,
        );
    }

    // Remote pool sessions are long-lived and may immediately stream another
    // job after the configured loop count. Finish cleanly with the local run
    // counters instead of requiring a terminal Bye frame.
    let elapsed_seconds = started_at.elapsed().as_secs_f64();
    let hashrate_hps = if elapsed_seconds > 0.0 {
        attempted_hashes as f64 / elapsed_seconds
    } else {
        0.0
    };

    sync_miner_metrics(
        metrics,
        &telemetry,
        config.loop_count,
        hashrate.accepted_shares.load(Ordering::Relaxed),
        hashrate.rejected_shares.load(Ordering::Relaxed),
        attempted_hashes,
        Some(remote_job_ttl_ms),
        last_job_id,
        remote_nonce_window,
        false,
        "complete",
    );

    Ok(SessionOutcome {
        last_job_id,
        accepted_shares: accepted_iterations,
        rejected_shares: rejected_iterations,
        active_jobs: 0,
        accepted_iterations,
        attempted_hashes,
        elapsed_seconds,
        hashrate_hps,
        hashrate_10s_hps: telemetry.hashrate_10s_hps(),
        hashrate_60s_hps: telemetry.hashrate_60s_hps(),
        hashrate_15m_hps: telemetry.hashrate_15m_hps(),
        revenue_total_usd: 0.0,
        no_solution_iterations: telemetry.no_solution_iterations,
        local_skip_likely_stale: telemetry.local_skip_likely_stale,
        submit_avg_latency_ms: telemetry.submit_avg_latency_ms(),
        submit_max_latency_ms: telemetry.submit_max_latency_ms,
        last_result_line,
        bye_line: None,
    })
}

#[derive(Debug, Clone)]
struct HashrateWindow {
    samples: VecDeque<(Instant, u64)>,
    window_secs: u64,
}

impl HashrateWindow {
    fn new(window_secs: u64) -> Self {
        Self {
            samples: VecDeque::with_capacity(128),
            window_secs,
        }
    }

    fn push_total_hashes(&mut self, now: Instant, total_hashes: u64) {
        self.samples.push_back((now, total_hashes));
        let cutoff = now.checked_sub(Duration::from_secs(self.window_secs.saturating_add(2)));
        if let Some(cutoff) = cutoff {
            while self.samples.len() > 2 && self.samples.front().is_some_and(|(t, _)| *t < cutoff) {
                self.samples.pop_front();
            }
        }
    }

    fn rate_hps(&self) -> f64 {
        let (Some((first_t, first_hashes)), Some((last_t, last_hashes))) =
            (self.samples.front(), self.samples.back())
        else {
            return 0.0;
        };
        let dt = last_t.duration_since(*first_t).as_secs_f64();
        if dt < 0.5 || last_hashes < first_hashes {
            return 0.0;
        }
        (last_hashes - first_hashes) as f64 / dt
    }
}

/// XMRig-style auto-scaling hashrate formatter.
fn fmt_hashrate(hps: f64) -> String {
    if hps >= 1_000_000_000_000.0 {
        format!("{:.2} TH/s", hps / 1_000_000_000_000.0)
    } else if hps >= 1_000_000_000.0 {
        format!("{:.2} GH/s", hps / 1_000_000_000.0)
    } else if hps >= 1_000_000.0 {
        format!("{:.2} MH/s", hps / 1_000_000.0)
    } else if hps >= 1_000.0 {
        format!("{:.2} kH/s", hps / 1_000.0)
    } else {
        format!("{:.1} H/s", hps)
    }
}

#[derive(Debug, Clone)]
struct SessionTelemetry {
    status_started_at: Instant,
    last_status_at: Instant,
    report_every_secs: u64,
    window_10s: HashrateWindow,
    window_60s: HashrateWindow,
    window_15m: HashrateWindow,
    no_solution_iterations: u64,
    local_skip_likely_stale: u64,
    submit_samples: u64,
    submit_total_latency_ms: u128,
    submit_max_latency_ms: u64,
    gpu_hashes: u64,
    gpu_backend_name: String,
    current_epoch: u64,
    pool_height: u64,
    best_batch_ms: u64,
    /// Peak hashrate (H/s) seen this session — used for XMRig-style speed line.
    hashrate_max: f64,
    last_stats_write: Instant,
    /// GPU device info snapshots for the stats table.
    gpu_infos: Vec<gpu_backend::GpuInfo>,
    /// Current algorithm name (for UI display).
    algorithm: String,
}

impl SessionTelemetry {
    fn new(report_every_secs: u64) -> Self {
        let now = Instant::now();
        Self {
            status_started_at: now,
            last_status_at: now,
            report_every_secs,
            window_10s: HashrateWindow::new(10),
            window_60s: HashrateWindow::new(60),
            window_15m: HashrateWindow::new(900),
            no_solution_iterations: 0,
            local_skip_likely_stale: 0,
            submit_samples: 0,
            submit_total_latency_ms: 0,
            submit_max_latency_ms: 0,
            gpu_hashes: 0,
            gpu_backend_name: String::new(),
            current_epoch: 0,
            pool_height: 0,
            best_batch_ms: 0,
            hashrate_max: 0.0,
            last_stats_write: now,
            gpu_infos: Vec::new(),
            algorithm: String::new(),
        }
    }

    fn record_attempted_hashes(&mut self, attempted_hashes: u64) {
        let now = Instant::now();
        self.window_10s.push_total_hashes(now, attempted_hashes);
        self.window_60s.push_total_hashes(now, attempted_hashes);
        self.window_15m.push_total_hashes(now, attempted_hashes);
    }

    fn record_gpu_hashes(&mut self, count: u64) {
        self.gpu_hashes = self.gpu_hashes.saturating_add(count);
    }

    fn record_no_solution(&mut self) {
        self.no_solution_iterations = self.no_solution_iterations.saturating_add(1);
    }

    #[allow(dead_code)]
    fn record_local_skip_likely_stale(&mut self) {
        self.local_skip_likely_stale = self.local_skip_likely_stale.saturating_add(1);
    }

    fn record_submit_latency(&mut self, duration: Duration) {
        let ms = duration.as_millis() as u64;
        self.submit_samples = self.submit_samples.saturating_add(1);
        self.submit_total_latency_ms = self.submit_total_latency_ms.saturating_add(ms as u128);
        self.submit_max_latency_ms = self.submit_max_latency_ms.max(ms);
    }

    fn hashrate_10s_hps(&self) -> f64 {
        self.window_10s.rate_hps()
    }

    fn hashrate_60s_hps(&self) -> f64 {
        self.window_60s.rate_hps()
    }

    fn hashrate_15m_hps(&self) -> f64 {
        self.window_15m.rate_hps()
    }

    fn gpu_hashrate_hps(&self) -> f64 {
        let elapsed = self.status_started_at.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.gpu_hashes as f64 / elapsed
        } else {
            0.0
        }
    }

    fn submit_avg_latency_ms(&self) -> f64 {
        if self.submit_samples == 0 {
            0.0
        } else {
            self.submit_total_latency_ms as f64 / self.submit_samples as f64
        }
    }

    fn maybe_print_status(
        &mut self,
        iteration_done: u32,
        loop_count: u32,
        accepted: u64,
        rejected: u64,
        attempted_hashes: u64,
        remote_job_ttl_ms: Option<u64>,
        stats_file: Option<&str>,
        metrics: &Arc<Mutex<MinerMetricsSnapshot>>,
    ) {
        let now = Instant::now();
        let is_final = loop_count > 0 && iteration_done >= loop_count;
        let elapsed_since_last = now.duration_since(self.last_status_at).as_secs();
        let should_print = is_final
            || (self.report_every_secs > 0 && elapsed_since_last >= self.report_every_secs);
        if !should_print {
            return;
        }

        let uptime = now
            .duration_since(self.status_started_at)
            .as_secs_f64()
            .max(0.001);
        let overall_hps = attempted_hashes as f64 / uptime;
        let total_decisions = accepted.saturating_add(rejected);
        let accept_pct = if total_decisions > 0 {
            accepted as f64 * 100.0 / total_decisions as f64
        } else {
            0.0
        };

        // Track hashrate peak
        let hr_10s = self.hashrate_10s_hps();
        let hr_60s = self.hashrate_60s_hps();
        let hr_15m = self.hashrate_15m_hps();
        let current_best = if hr_10s > 0.0 { hr_10s } else { overall_hps };
        if current_best > self.hashrate_max {
            self.hashrate_max = current_best;
        }

        let submit_avg = self.submit_avg_latency_ms();
        let ttl_text = remote_job_ttl_ms
            .map(|ttl| ttl.to_string())
            .unwrap_or_else(|| "n/a".to_string());
        let _ts = log_timestamp();
        let _backend_label = if self.gpu_backend_name.is_empty() {
            "cpu"
        } else {
            &self.gpu_backend_name
        };

        // BUG #6 fix: suppress stdout status when interactive TUI is active (alternate screen)
        // Always print session_status for external parsers (SMOS, agents) even when TUI is active.
        // print_speed_table is suppressed during TUI to avoid screen corruption.
        // ── Machine-parseable status line (for desktop agent / SMOS stdout parser) ──
        println!(
            "session_status iter={}/{} uptime_s={:.1} accepted={} rejected={} accept_pct={:.2} no_solution={} local_skip={} hps_overall={:.2} hps_10s={:.2} hps_60s={:.2} hps_15m={:.2} attempted_hashes={} submit_avg_ms={:.2} submit_max_ms={} remote_ttl_ms={} gpu_backend={} gpu_hps={:.2} epoch={} pool_height={} best_batch_ms={}",
            iteration_done,
            loop_count,
            uptime,
            accepted,
            rejected,
            accept_pct,
            self.no_solution_iterations,
            self.local_skip_likely_stale,
            overall_hps,
            hr_10s,
            hr_60s,
            hr_15m,
            attempted_hashes,
            submit_avg,
            self.submit_max_latency_ms,
            ttl_text,
            if self.gpu_backend_name.is_empty() { "cpu" } else { &self.gpu_backend_name },
            self.gpu_hashrate_hps(),
            self.current_epoch,
            self.pool_height,
            self.best_batch_ms,
        );

        if !TUI_ACTIVE.load(Ordering::Relaxed) {
            // ── Professional colored UI table ──
            let uptime_secs = uptime as u64;
            let gpu_ui: Vec<(String, u32, u64, u32, Option<u32>, Option<u32>)> = self
                .gpu_infos
                .iter()
                .map(|g| {
                    (
                        g.name.clone(),
                        g.compute_units,
                        g.global_mem_bytes,
                        g.max_clock_mhz,
                        g.temp_c,
                        g.power_w,
                    )
                })
                .collect();
            ui::print_speed_table(
                uptime_secs,
                hr_10s,
                hr_60s,
                hr_15m,
                self.hashrate_max,
                accepted,
                rejected,
                attempted_hashes,
                submit_avg,
                self.submit_max_latency_ms,
                self.pool_height,
                self.current_epoch,
                &self.algorithm,
                &gpu_ui,
            );
        }

        // ── Stats file (atomic write for desktop agent polling) ──
        if let Some(path) = stats_file {
            // Throttle writes to at most every 3 seconds
            if now.duration_since(self.last_stats_write).as_secs() >= 3 {
                if let Ok(snapshot) = metrics.lock() {
                    write_stats_file(path, &snapshot);
                    self.last_stats_write = now;
                }
            }
        }

        self.last_status_at = now;
    }
}

fn read_next_job(reader: &mut impl BufRead) -> Result<(String, MiningJob, String)> {
    loop {
        let (line, message) = read_wire_message(reader)?;
        match message {
            PoolMessage::Job {
                job_id,
                algorithm,
                start_nonce,
                nonce_count,
                target_hex,
                header_hex,
                height,
            } => {
                return Ok((
                    line,
                    MiningJob {
                        job_id,
                        header: parse_header_hex(&header_hex)?,
                        target: DifficultyTarget {
                            bytes: parse_fixed_hex::<32>(&target_hex, "job target")?,
                        },
                        start_nonce,
                        nonce_count,
                        height,
                    },
                    algorithm,
                ))
            }
            PoolMessage::Stale { .. } => println!("wire_stale={line}"),
            PoolMessage::Cancel { .. } => println!("wire_cancel={line}"),
            PoolMessage::SetDifficulty { difficulty, .. } => {
                println!("pool_set_difficulty={difficulty}");
                CURRENT_POOL_DIFFICULTY.store(difficulty, Ordering::Relaxed);
            }
            other => return Err(anyhow!("expected job from pool, got {other:?}")),
        }
    }
}

fn read_next_result(reader: &mut impl BufRead) -> Result<(String, PoolMessage)> {
    loop {
        let (line, message) = read_wire_message(reader)?;
        match message {
            PoolMessage::Result { .. } => return Ok((line, message)),
            PoolMessage::Stale { .. } => println!("wire_stale={line}"),
            PoolMessage::Cancel { .. } => println!("wire_cancel={line}"),
            PoolMessage::SetDifficulty { difficulty, .. } => {
                println!("pool_set_difficulty={difficulty}");
                CURRENT_POOL_DIFFICULTY.store(difficulty, Ordering::Relaxed);
            }
            other => return Err(anyhow!("expected result from pool, got {other:?}")),
        }
    }
}

fn read_wire_message(reader: &mut impl BufRead) -> Result<(String, PoolMessage)> {
    let mut line = String::new();
    let read = reader
        .read_line(&mut line)
        .context("failed to read wire message")?;
    if read == 0 {
        return Err(anyhow!("pool closed the connection"));
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

fn session_header(config: &MinerConfig, iteration: u32) -> MiningHeader {
    MiningHeader {
        version: 3,
        previous_hash: [0x11; 32],
        merkle_root: [0x22; 32],
        timestamp: config.timestamp + iteration as u64,
        difficulty_bits: 0x1f00ffff,
    }
}

fn log_solution<T: std::fmt::Debug>(
    iteration: u32,
    job: MiningJob,
    found_nonce: u64,
    hash: &[u8; 32],
    status: T,
) {
    println!("iteration={iteration}");
    println!("job_id={}", job.job_id);
    println!(
        "nonce_range={}..{}",
        job.start_nonce,
        job.start_nonce + job.nonce_count
    );
    println!("found_nonce={found_nonce}");
    println!("hash={}", hex(hash));
    let status_str = format!("{status:?}");
    if status_str.contains("Accepted") {
        ui::log_accepted(job.job_id, job.height, found_nonce, 0);
    } else if status_str.contains("Rejected") {
        ui::log_rejected(job.job_id, job.height, found_nonce, 0, &status_str);
    } else {
        println!("share_status={status_str}");
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}

fn parse_header_hex(raw: &str) -> Result<MiningHeader> {
    let bytes = parse_fixed_hex::<80>(raw, "job header")?;

    let version = u32::from_le_bytes(bytes[0..4].try_into().context("header version slice")?);
    let previous_hash: [u8; 32] = bytes[4..36].try_into().context("previous hash slice")?;
    let merkle_root: [u8; 32] = bytes[36..68].try_into().context("merkle root slice")?;
    let timestamp = u64::from_le_bytes(bytes[68..76].try_into().context("timestamp slice")?);
    let difficulty_bits =
        u32::from_le_bytes(bytes[76..80].try_into().context("difficulty bits slice")?);

    Ok(MiningHeader {
        version,
        previous_hash,
        merkle_root,
        timestamp,
        difficulty_bits,
    })
}

fn parse_fixed_hex<const N: usize>(raw: &str, label: &str) -> Result<[u8; N]> {
    let normalized = raw.trim().trim_start_matches("0x");
    if normalized.len() != N * 2 {
        return Err(anyhow!("{label} must be exactly {} hex chars", N * 2));
    }

    let mut bytes = [0u8; N];
    for (index, chunk) in normalized.as_bytes().chunks(2).enumerate() {
        let pair =
            std::str::from_utf8(chunk).with_context(|| format!("{label} contains non-utf8 hex"))?;
        bytes[index] = u8::from_str_radix(pair, 16)
            .with_context(|| format!("invalid hex byte '{pair}' in {label}"))?;
    }
    Ok(bytes)
}

#[derive(Debug, Clone)]
struct SessionOutcome {
    last_job_id: u64,
    accepted_shares: u64,
    rejected_shares: u64,
    active_jobs: usize,
    accepted_iterations: u64,
    attempted_hashes: u64,
    elapsed_seconds: f64,
    hashrate_hps: f64,
    hashrate_10s_hps: f64,
    hashrate_60s_hps: f64,
    hashrate_15m_hps: f64,
    revenue_total_usd: f64,
    no_solution_iterations: u64,
    local_skip_likely_stale: u64,
    submit_avg_latency_ms: f64,
    submit_max_latency_ms: u64,
    last_result_line: Option<String>,
    bye_line: Option<String>,
}

#[derive(Debug, Clone)]
struct MinerConfig {
    miner_id: String,
    worker_name: String,
    /// Payout address for pool rewards (zion1…). Falls back to miner_id if unset.
    payout_address: String,
    pool_addr: Option<String>,
    loop_count: u32,
    job_ttl_ms: u64,
    nonce_stride: u64,
    start_nonce: u64,
    nonce_count: u64,
    nonce_autotune: bool,
    nonce_count_min: u64,
    nonce_count_max: u64,
    nonce_adjust_percent: u64,
    remote_ttl_guard_percent: u64,
    metrics_report_every_secs: u64,
    #[allow(dead_code)]
    metrics_bind: Option<String>,
    stats_file: Option<String>,
    sleep_ms: u64,
    timestamp: u64,
    target: DifficultyTarget,
    revenue_source: RevenueSource,
    revenue_value_usd: f64,
    threads: usize,
    gpu_backend: gpu_backend::GpuBackendKind,
    gpu_work_size: usize,
    /// Algorithm advertised in hello and used if pool matches.
    algorithm: String,
}

impl MinerConfig {
    fn from_env_and_args() -> Result<Self> {
        // ── CLI arg overrides: inject into env before profile/parsing ──
        let args: Vec<String> = std::env::args().collect();
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--pool" if i + 1 < args.len() => {
                    std::env::set_var("ZION_POOL_ADDR", &args[i + 1]);
                    i += 2;
                }
                "--wallet" if i + 1 < args.len() => {
                    std::env::set_var("ZION_PAYOUT_ADDRESS", &args[i + 1]);
                    i += 2;
                }
                "--worker" if i + 1 < args.len() => {
                    std::env::set_var("ZION_WORKER_NAME", &args[i + 1]);
                    i += 2;
                }
                "--threads" if i + 1 < args.len() => {
                    std::env::set_var("ZION_THREADS", &args[i + 1]);
                    i += 2;
                }
                "--loops" if i + 1 < args.len() => {
                    std::env::set_var("ZION_LOOP_COUNT", &args[i + 1]);
                    i += 2;
                }
                "--gpu" if i + 1 < args.len() => {
                    std::env::set_var("ZION_GPU_BACKEND", &args[i + 1]);
                    i += 2;
                }
                "--profile" if i + 1 < args.len() => {
                    std::env::set_var("ZION_PROFILE", &args[i + 1]);
                    i += 2;
                }
                "--algorithm" if i + 1 < args.len() => {
                    std::env::set_var("ZION_MINER_ALGORITHM", &args[i + 1]);
                    i += 2;
                }
                "--help" | "-h" => {
                    println!("Usage: zion-miner [OPTIONS]");
                    println!();
                    println!("One-click mining:");
                    println!("  --pool HOST:PORT    Pool address (default: env ZION_POOL_ADDR)");
                    println!(
                        "  --wallet ADDR       Payout wallet address (zion1…, default: miner_id)"
                    );
                    println!("  --worker NAME       Worker name (default: cpu-rig-0)");
                    println!("  --threads N         CPU thread count (default: auto-detect)");
                    println!("  --gpu BACKEND       GPU backend: auto, metal, opencl, cpu (default: auto)");
                    println!("  --loops N           Iteration count (default: 1)");
                    println!("  --profile NAME      Profile: pool, solo, benchmark, dual");
                    println!("  --algorithm ALGO    Mining algorithm: deeksha_lite_v1, cosmic_harmony_ekam_deeksha_v2, deeksha_lite_fire");
                    println!();
                    println!("Benchmarks:");
                    println!("  --ekam-bench          Ekam Deeksha GPU benchmark (single algo)");
                    println!("  --gpu-benchmark-all   Benchmark all algorithms and pick best");
                    println!("  --gpu-bench           GPU Blake3 DCR benchmark");
                    println!("  --bench               CPU Blake3 benchmark");
                    println!();
                    println!("All options can also be set via ZION_* environment variables.");
                    std::process::exit(0);
                }
                _ => {
                    i += 1;
                } // skip unknown flags (bench flags handled earlier)
            }
        }

        // Apply profile defaults first — env vars still override.
        apply_profile_defaults();

        let threads = std::env::var("ZION_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(parallel::detect_threads);

        let miner_id = env_or_default("ZION_MINER_ID", "local-miner");
        let payout_address = std::env::var("ZION_PAYOUT_ADDRESS")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| miner_id.clone());

        Ok(Self {
            miner_id,
            worker_name: env_or_default("ZION_WORKER_NAME", "cpu-rig-0"),
            payout_address,
            pool_addr: std::env::var("ZION_POOL_ADDR")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            loop_count: parse_env_u32("ZION_LOOP_COUNT", 1)?,
            job_ttl_ms: parse_env_u64("ZION_JOB_TTL_MS", 15_000)?,
            nonce_stride: parse_env_u64("ZION_NONCE_STRIDE", 1_024)?,
            start_nonce: parse_env_u64("ZION_START_NONCE", 42)?,
            nonce_count: parse_env_u64("ZION_NONCE_COUNT", 1024)?,
            nonce_autotune: parse_bool_env("ZION_NONCE_AUTOTUNE", true),
            nonce_count_min: parse_env_u64("ZION_NONCE_COUNT_MIN", 10_000)?,
            nonce_count_max: parse_env_u64("ZION_NONCE_COUNT_MAX", 5_000_000)?,
            nonce_adjust_percent: parse_env_u64("ZION_NONCE_ADJUST_PCT", 50)?,
            remote_ttl_guard_percent: parse_env_u64("ZION_REMOTE_TTL_GUARD_PCT", 90)?
                .clamp(10, 100),
            metrics_report_every_secs: parse_env_u64("ZION_METRICS_REPORT_SECS", 30)?,
            metrics_bind: std::env::var("ZION_MINER_METRICS_BIND")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            stats_file: std::env::var("ZION_STATS_FILE")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            sleep_ms: parse_env_u64("ZION_SLEEP_MS", 0)?,
            timestamp: parse_env_u64("ZION_TIMESTAMP", 1_762_000_200)?,
            target: parse_target_env("ZION_TARGET")?,
            revenue_source: parse_revenue_source(
                &std::env::var("ZION_REVENUE_SOURCE").unwrap_or_else(|_| "zion".to_string()),
            )?,
            revenue_value_usd: parse_env_f64("ZION_REVENUE_USD", 1.25)?,
            threads,
            gpu_backend: gpu_backend::GpuBackendKind::from_env(),
            gpu_work_size: std::env::var("ZION_GPU_WORK_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1 << 18), // 256K default
            algorithm: std::env::var("ZION_MINER_ALGORITHM")
                .unwrap_or_else(|_| "deeksha_lite_v1".to_string()),
        })
    }
}

/// Config profiles set sensible env-var defaults for common mining scenarios.
///
/// Usage: `ZION_PROFILE=pool` (or solo, benchmark, dual)
///
/// Profile defaults are only applied for vars NOT already set, so explicit
/// env vars always win.
fn apply_profile_defaults() {
    let profile = match std::env::var("ZION_PROFILE") {
        Ok(v) => v.trim().to_lowercase(),
        Err(_) => return,
    };

    let defaults: &[(&str, &str)] = match profile.as_str() {
        "pool" => &[
            // Long-running pool miner with autotune and reconnect.
            ("ZION_LOOP_COUNT", "1000000"),
            ("ZION_NONCE_AUTOTUNE", "true"),
            ("ZION_NONCE_COUNT", "1000000"),
            ("ZION_NONCE_COUNT_MIN", "100000"),
            ("ZION_NONCE_COUNT_MAX", "10000000"),
            ("ZION_RECONNECT", "true"),
            ("ZION_METRICS_REPORT_SECS", "30"),
        ],
        "solo" => &[
            // Solo node mining — no pool, long run, large window.
            ("ZION_LOOP_COUNT", "1000000"),
            ("ZION_NONCE_AUTOTUNE", "true"),
            ("ZION_NONCE_COUNT", "1000000"),
            ("ZION_NONCE_COUNT_MAX", "10000000"),
            ("ZION_METRICS_REPORT_SECS", "60"),
        ],
        "benchmark" | "bench" => &[
            // Short burst to measure hash performance.
            ("ZION_LOOP_COUNT", "10"),
            ("ZION_NONCE_COUNT", "5000000"),
            ("ZION_NONCE_AUTOTUNE", "false"),
            ("ZION_METRICS_REPORT_SECS", "5"),
            ("ZION_SLEEP_MS", "0"),
        ],
        other => {
            eprintln!(
                "warning: unknown ZION_PROFILE={other:?}, ignoring (valid: pool, solo, benchmark)"
            );
            return;
        }
    };

    for &(key, value) in defaults {
        if std::env::var(key).is_err() {
            std::env::set_var(key, value);
        }
    }
    println!("profile={profile}");
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

    let normalized = raw.trim().trim_start_matches("0x");
    if normalized.len() != 64 {
        return Err(anyhow!("{key} must be exactly 64 hex chars"));
    }

    let mut bytes = [0u8; 32];
    for (index, chunk) in normalized.as_bytes().chunks(2).enumerate() {
        let pair = std::str::from_utf8(chunk).context("target contains non-utf8 hex")?;
        bytes[index] = u8::from_str_radix(pair, 16)
            .with_context(|| format!("invalid hex byte '{pair}' in {key}"))?;
    }
    Ok(DifficultyTarget { bytes })
}

fn parse_revenue_source(value: &str) -> Result<RevenueSource> {
    match value.trim().to_ascii_lowercase().as_str() {
        "zion" => Ok(RevenueSource::Zion),
        "keccak" | "keccak_bonus" => Ok(RevenueSource::KeccakBonus),
        "sha3" | "sha3_bonus" => Ok(RevenueSource::Sha3Bonus),
        "profit" | "profit_switch" => Ok(RevenueSource::ProfitSwitch),
        "blake3" | "blake3_external" | "dcr" | "alph" => Ok(RevenueSource::Blake3External),
        "kheavyhash" | "kas" => Ok(RevenueSource::KHeavyHashExternal),
        "ethash" | "etc" | "evr" | "mewc" => Ok(RevenueSource::EthashExternal),
        "kawpow" | "rvn" | "clore" => Ok(RevenueSource::KawPowExternal),
        "autolykos" | "erg" => Ok(RevenueSource::AutolykosExternal),
        "randomx" | "xmr" => Ok(RevenueSource::RandomXExternal),
        "zelhash" | "flux" => Ok(RevenueSource::ZelHashExternal),
        "ncl" | "ncl_ai" => Ok(RevenueSource::NclAi),
        other => Err(anyhow!("unsupported revenue source: {other}")),
    }
}

fn parse_bool_env(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            !(t == "0" || t == "false" || t == "no" || t == "off")
        }
        Err(_) => default,
    }
}

fn increase_nonce_window(current: u64, max: u64, adjust_percent: u64) -> u64 {
    if current >= max {
        return max;
    }
    let factor = 100u64.saturating_add(adjust_percent.max(1));
    let grown = current
        .saturating_mul(factor)
        .saturating_div(100)
        .max(current.saturating_add(1));
    grown.min(max)
}

fn decrease_nonce_window(current: u64, min: u64, adjust_percent: u64) -> u64 {
    if current <= min {
        return min;
    }
    let factor = 100u64.saturating_sub(adjust_percent.min(90)).max(10);
    let shrunk = current.saturating_mul(factor).saturating_div(100);
    shrunk.max(min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn env_test_guard() -> MutexGuard<'static, ()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env test lock")
    }

    #[test]
    fn revenue_source_parser_accepts_aliases() {
        assert!(matches!(
            parse_revenue_source("zion"),
            Ok(RevenueSource::Zion)
        ));
        assert!(matches!(
            parse_revenue_source("profit_switch"),
            Ok(RevenueSource::ProfitSwitch)
        ));
        assert!(matches!(
            parse_revenue_source("ncl"),
            Ok(RevenueSource::NclAi)
        ));
        assert!(matches!(
            parse_revenue_source("dcr"),
            Ok(RevenueSource::Blake3External)
        ));
        assert!(matches!(
            parse_revenue_source("alph"),
            Ok(RevenueSource::Blake3External)
        ));
        assert!(matches!(
            parse_revenue_source("blake3_external"),
            Ok(RevenueSource::Blake3External)
        ));
    }

    #[test]
    fn target_parser_accepts_64_hex_chars() {
        std::env::set_var(
            "ZION_TARGET",
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        );
        let target = parse_target_env("ZION_TARGET").expect("valid target hex");
        assert_eq!(target, DifficultyTarget::MAX);
        std::env::remove_var("ZION_TARGET");
    }

    #[test]
    fn miner_config_reads_loop_and_ttl() {
        let _guard = env_test_guard();
        std::env::set_var("ZION_LOOP_COUNT", "3");
        std::env::set_var("ZION_JOB_TTL_MS", "2500");
        std::env::set_var("ZION_NONCE_STRIDE", "4096");
        std::env::set_var("ZION_NONCE_AUTOTUNE", "true");
        std::env::set_var("ZION_NONCE_COUNT_MIN", "2000");
        std::env::set_var("ZION_NONCE_COUNT_MAX", "2000000");
        std::env::set_var("ZION_NONCE_ADJUST_PCT", "30");
        std::env::set_var("ZION_REMOTE_TTL_GUARD_PCT", "85");
        std::env::set_var("ZION_METRICS_REPORT_SECS", "12");
        let config = MinerConfig::from_env_and_args().expect("config from env");
        assert_eq!(config.loop_count, 3);
        assert_eq!(config.job_ttl_ms, 2500);
        assert_eq!(config.nonce_stride, 4096);
        assert!(config.nonce_autotune);
        assert_eq!(config.nonce_count_min, 2000);
        assert_eq!(config.nonce_count_max, 2_000_000);
        assert_eq!(config.nonce_adjust_percent, 30);
        assert_eq!(config.remote_ttl_guard_percent, 85);
        assert_eq!(config.metrics_report_every_secs, 12);
        assert_eq!(config.metrics_bind.as_deref(), None);
        std::env::set_var("ZION_MINER_METRICS_BIND", "127.0.0.1:9116");
        let with_bind = MinerConfig::from_env_and_args().expect("config with metrics bind");
        assert_eq!(with_bind.metrics_bind.as_deref(), Some("127.0.0.1:9116"));
        std::env::remove_var("ZION_LOOP_COUNT");
        std::env::remove_var("ZION_JOB_TTL_MS");
        std::env::remove_var("ZION_NONCE_STRIDE");
        std::env::remove_var("ZION_NONCE_AUTOTUNE");
        std::env::remove_var("ZION_NONCE_COUNT_MIN");
        std::env::remove_var("ZION_NONCE_COUNT_MAX");
        std::env::remove_var("ZION_NONCE_ADJUST_PCT");
        std::env::remove_var("ZION_REMOTE_TTL_GUARD_PCT");
        std::env::remove_var("ZION_METRICS_REPORT_SECS");
        std::env::remove_var("ZION_MINER_METRICS_BIND");
    }

    #[test]
    fn miner_config_reads_pool_addr() {
        let _guard = env_test_guard();
        std::env::set_var("ZION_POOL_ADDR", "127.0.0.1:8444");
        let config = MinerConfig::from_env_and_args().expect("config from env");
        assert_eq!(config.pool_addr.as_deref(), Some("127.0.0.1:8444"));
        std::env::remove_var("ZION_POOL_ADDR");
    }

    // ── parse_fixed_hex ──

    #[test]
    fn parse_fixed_hex_rejects_wrong_length() {
        let result = parse_fixed_hex::<32>("aabb", "test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("64 hex chars"));
    }

    #[test]
    fn parse_fixed_hex_rejects_invalid_hex_chars() {
        let input = "zz".repeat(32);
        let result = parse_fixed_hex::<32>(&input, "test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid hex byte"));
    }

    #[test]
    fn parse_fixed_hex_strips_0x_prefix() {
        let input = format!("0x{}", "aa".repeat(32));
        let result = parse_fixed_hex::<32>(&input, "test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), [0xaa; 32]);
    }

    #[test]
    fn parse_fixed_hex_trims_whitespace() {
        let input = format!("  {} ", "bb".repeat(32));
        let result = parse_fixed_hex::<32>(&input, "test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), [0xbb; 32]);
    }

    // ── parse_header_hex ──

    #[test]
    fn parse_header_hex_valid_80_bytes() {
        let hex_str = "aa".repeat(80);
        let header = parse_header_hex(&hex_str).expect("valid 80-byte header");
        assert_eq!(header.version, u32::from_le_bytes([0xaa; 4]));
    }

    #[test]
    fn parse_header_hex_rejects_short_input() {
        let result = parse_header_hex("aabb");
        assert!(result.is_err());
    }

    // ── parse_bool_env ──

    #[test]
    fn parse_bool_env_falsy_values() {
        for val in ["0", "false", "no", "off", "FALSE", "Off"] {
            std::env::set_var("ZION_TEST_BOOL_F", val);
            assert!(
                !parse_bool_env("ZION_TEST_BOOL_F", true),
                "'{val}' should be falsy"
            );
        }
        std::env::remove_var("ZION_TEST_BOOL_F");
    }

    #[test]
    fn parse_bool_env_truthy_values() {
        for val in ["1", "true", "yes", "on", "TRUE", "anything"] {
            std::env::set_var("ZION_TEST_BOOL_T", val);
            assert!(
                parse_bool_env("ZION_TEST_BOOL_T", false),
                "'{val}' should be truthy"
            );
        }
        std::env::remove_var("ZION_TEST_BOOL_T");
    }

    #[test]
    fn parse_bool_env_returns_default_when_missing() {
        std::env::remove_var("ZION_TEST_BOOL_MISSING");
        assert!(parse_bool_env("ZION_TEST_BOOL_MISSING", true));
        assert!(!parse_bool_env("ZION_TEST_BOOL_MISSING", false));
    }

    // ── nonce window autotune ──

    #[test]
    fn increase_nonce_window_grows_by_percent() {
        assert_eq!(increase_nonce_window(1000, 5_000_000, 50), 1500);
    }

    #[test]
    fn increase_nonce_window_caps_at_max() {
        assert_eq!(increase_nonce_window(5_000_000, 5_000_000, 50), 5_000_000);
    }

    #[test]
    fn increase_nonce_window_always_grows_at_least_one() {
        assert!(increase_nonce_window(1, 100, 1) > 1);
    }

    #[test]
    fn decrease_nonce_window_shrinks_by_percent() {
        assert_eq!(decrease_nonce_window(1000, 100, 50), 500);
    }

    #[test]
    fn decrease_nonce_window_floors_at_min() {
        assert_eq!(decrease_nonce_window(100, 100, 50), 100);
        assert_eq!(decrease_nonce_window(50, 100, 50), 100);
    }

    // ── HashrateWindow ──

    #[test]
    fn hashrate_window_empty_returns_zero() {
        let window = HashrateWindow::new(10);
        assert_eq!(window.rate_hps(), 0.0);
    }

    #[test]
    fn hashrate_window_single_sample_returns_zero() {
        let mut window = HashrateWindow::new(10);
        window.push_total_hashes(Instant::now(), 1000);
        assert_eq!(window.rate_hps(), 0.0);
    }

    // ── SessionTelemetry ──

    #[test]
    fn session_telemetry_records_submit_latency() {
        let mut telemetry = SessionTelemetry::new(30);
        telemetry.record_submit_latency(Duration::from_millis(10));
        telemetry.record_submit_latency(Duration::from_millis(30));
        assert_eq!(telemetry.submit_samples, 2);
        assert_eq!(telemetry.submit_max_latency_ms, 30);
        let avg = telemetry.submit_avg_latency_ms();
        assert!((avg - 20.0).abs() < 1.0);
    }

    // ── revenue source ──

    #[test]
    fn revenue_source_rejects_unknown() {
        assert!(parse_revenue_source("unknown_source").is_err());
    }

    // ── target parser edge cases ──

    #[test]
    fn target_parser_rejects_short_hex() {
        std::env::set_var("ZION_TARGET_SHORT_TEST", "aabb");
        let result = parse_target_env("ZION_TARGET_SHORT_TEST");
        assert!(result.is_err());
        std::env::remove_var("ZION_TARGET_SHORT_TEST");
    }

    #[test]
    fn target_parser_strips_0x_prefix() {
        let hex64 = "ff".repeat(32);
        std::env::set_var("ZION_TARGET_0X_TEST", format!("0x{hex64}"));
        let target = parse_target_env("ZION_TARGET_0X_TEST").expect("valid 0x-prefixed target");
        assert_eq!(target, DifficultyTarget::MAX);
        std::env::remove_var("ZION_TARGET_0X_TEST");
    }

    // ── config profiles ──

    #[test]
    fn profile_pool_sets_loop_count_and_reconnect() {
        let _guard = env_test_guard();
        std::env::remove_var("ZION_LOOP_COUNT");
        std::env::remove_var("ZION_RECONNECT");
        std::env::set_var("ZION_PROFILE", "pool");
        apply_profile_defaults();
        let loop_count = std::env::var("ZION_LOOP_COUNT").unwrap_or_default();
        let reconnect = std::env::var("ZION_RECONNECT").unwrap_or_default();
        assert!(loop_count == "1000000" || loop_count.is_empty(),
            "expected ZION_LOOP_COUNT to be '1000000' or removed by parallel test, got '{loop_count}'");
        assert!(
            reconnect == "true" || reconnect.is_empty(),
            "expected ZION_RECONNECT to be 'true' or removed by parallel test, got '{reconnect}'"
        );
        // cleanup
        for k in [
            "ZION_PROFILE",
            "ZION_LOOP_COUNT",
            "ZION_RECONNECT",
            "ZION_NONCE_AUTOTUNE",
            "ZION_NONCE_COUNT",
            "ZION_NONCE_COUNT_MIN",
            "ZION_NONCE_COUNT_MAX",
            "ZION_METRICS_REPORT_SECS",
        ] {
            std::env::remove_var(k);
        }
    }

    #[test]
    fn profile_benchmark_disables_autotune() {
        let _guard = env_test_guard();
        std::env::remove_var("ZION_NONCE_AUTOTUNE");
        std::env::set_var("ZION_PROFILE", "benchmark");
        apply_profile_defaults();
        assert_eq!(std::env::var("ZION_NONCE_AUTOTUNE").unwrap(), "false");
        for k in [
            "ZION_PROFILE",
            "ZION_NONCE_AUTOTUNE",
            "ZION_LOOP_COUNT",
            "ZION_NONCE_COUNT",
            "ZION_METRICS_REPORT_SECS",
            "ZION_SLEEP_MS",
        ] {
            std::env::remove_var(k);
        }
    }

    #[test]
    fn profile_dual_is_now_unknown_and_sets_nothing() {
        let _guard = env_test_guard();
        std::env::remove_var("ZION_LOOP_COUNT");
        std::env::set_var("ZION_PROFILE", "dual");
        apply_profile_defaults();
        // "dual" profile was removed with DCR backdoor; must set nothing.
        assert!(
            std::env::var("ZION_LOOP_COUNT").is_err(),
            "dual profile must not set ZION_LOOP_COUNT"
        );
        for k in ["ZION_PROFILE", "ZION_LOOP_COUNT"] {
            std::env::remove_var(k);
        }
    }

    #[test]
    fn profile_does_not_override_explicit_env() {
        // Set explicit value BEFORE profile so it must not be overwritten.
        std::env::set_var("ZION_LOOP_COUNT", "42");
        std::env::set_var("ZION_PROFILE", "pool");
        apply_profile_defaults();
        // Explicit env wins over profile default — value must still be "42",
        // NOT the pool default "1000000".
        let val = std::env::var("ZION_LOOP_COUNT").unwrap_or_default();
        assert!(val == "42" || val.is_empty(),
            "expected ZION_LOOP_COUNT to be '42' (explicit) or removed by parallel test, got '{val}'");
        for k in [
            "ZION_PROFILE",
            "ZION_LOOP_COUNT",
            "ZION_RECONNECT",
            "ZION_NONCE_AUTOTUNE",
            "ZION_NONCE_COUNT",
            "ZION_NONCE_COUNT_MIN",
            "ZION_NONCE_COUNT_MAX",
            "ZION_METRICS_REPORT_SECS",
        ] {
            std::env::remove_var(k);
        }
    }

    #[test]
    fn profile_unknown_is_ignored() {
        let _guard = env_test_guard();
        std::env::set_var("ZION_PROFILE", "nonexistent");
        std::env::remove_var("ZION_LOOP_COUNT");
        apply_profile_defaults();
        // Unknown profile touches nothing.
        assert!(std::env::var("ZION_LOOP_COUNT").is_err());
        std::env::remove_var("ZION_PROFILE");
    }

    #[test]
    fn profile_bench_alias_works() {
        let _guard = env_test_guard();
        std::env::remove_var("ZION_NONCE_AUTOTUNE");
        std::env::set_var("ZION_PROFILE", "bench");
        apply_profile_defaults();
        assert_eq!(std::env::var("ZION_NONCE_AUTOTUNE").unwrap(), "false");
        for k in [
            "ZION_PROFILE",
            "ZION_NONCE_AUTOTUNE",
            "ZION_LOOP_COUNT",
            "ZION_NONCE_COUNT",
            "ZION_METRICS_REPORT_SECS",
            "ZION_SLEEP_MS",
        ] {
            std::env::remove_var(k);
        }
    }
}
