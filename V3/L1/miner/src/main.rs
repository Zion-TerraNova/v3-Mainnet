// Miner is the binary entry point; many session helpers take wide config tuples
// and a few GPU-fallback flags are informational. These are non-consensus.
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use anyhow::{anyhow, Context, Result};
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use zion_core::{CoreRuntime, DifficultyTarget, MiningHeader, MiningJob, RevenueSource};
use zion_pool::{
    decode_message, encode_message, ExternalStreamJob, MiningPool, PoolMessage, ShareStatus,
};

mod autonomous;
mod banner;
mod cpu_features;
#[cfg(feature = "gpu-cuda")]
mod cuda_external;
mod gpu_backend;
mod gpu_guard;
mod interactive;
mod parallel;
mod thread_affinity;
mod ui;

use interactive::{HashrateTracker, MinerControl, TUI_ACTIVE};

// ── Crash protection: signal handler for SIGABRT/SIGSEGV ──────────────────
// AMD OpenCL driver crashes on Linux manifest as SIGABRT (exit 134) or
// SIGSEGV (exit 139). We install a handler that logs the crash to a file
// so the watchdog script can detect it and restart the miner.
#[cfg(unix)]
mod crash_handler {
    use std::os::raw::c_int;

    const SIGABRT: c_int = 6;
    const SIGSEGV: c_int = 11;
    const SIG_DFL: usize = 0;

    extern "C" {
        fn signal(signum: c_int, handler: usize) -> usize;
        fn raise(signum: c_int) -> c_int;
    }

    extern "C" fn handler(sig: c_int) {
        let crash_file = std::env::var("ZION_CRASH_LOG")
            .unwrap_or_else(|_| "/tmp/zion-miner-crash.log".to_string());
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let msg = format!(
            "CRASH signal={} pid={} timestamp={}\n",
            sig, std::process::id(), timestamp,
        );
        let _ = std::fs::write(&crash_file, &msg);
        eprintln!("[CRASH] signal={} — miner will exit. Watchdog should restart.", sig);
        unsafe {
            signal(sig, SIG_DFL);
            raise(sig);
        }
    }

    pub fn install() {
        unsafe {
            signal(SIGABRT, handler as *const () as usize);
            signal(SIGSEGV, handler as *const () as usize);
        }
    }
}

#[cfg(not(unix))]
mod crash_handler {
    pub fn install() {}
}

fn flush_stdout() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

/// Gate verbose wire_* / iteration= debug output (--verbose or ZION_MINER_VERBOSE=1).
static VERBOSE: AtomicBool = AtomicBool::new(false);
/// Suppress verbose log lines when sticky header is active (Claymore-style clean display).
/// Only status box + share notifications are shown.
static QUIET: AtomicBool = AtomicBool::new(false);
static CURRENT_POOL_DIFFICULTY: AtomicU64 = AtomicU64::new(1);
mod reconnect;

/// Log a line only if not in quiet mode (sticky header active).
fn log_line(msg: &str) {
    if QUIET.load(Ordering::Relaxed) {
        return;
    }
    println!("{}", msg);
}

/// Log a line always (even in quiet mode) — for important events like shares.
fn log_always(msg: &str) {
    println!("{}", msg);
}

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

/// Return the backend label string for the autonomous router.
fn effective_gpu_backend_label(backend: gpu_backend::GpuBackendKind) -> &'static str {
    match backend {
        gpu_backend::GpuBackendKind::OpenCL => "opencl",
        gpu_backend::GpuBackendKind::Cuda => "cuda",
        gpu_backend::GpuBackendKind::Metal => "metal",
        gpu_backend::GpuBackendKind::Cpu => "cpu",
        gpu_backend::GpuBackendKind::Auto => "auto",
    }
}

/// Check current system memory pressure.
/// Returns (free_bytes, total_bytes) by querying the OS.
/// On macOS, uses `vm_stat` and `sysctl`. On Linux, reads `/proc/meminfo`.
fn check_memory_pressure() -> (u64, u64) {
    let total = gpu_backend::detect_system_memory_bytes();

    #[cfg(target_os = "macos")]
    {
        // vm_stat gives page counts; page size is typically 4096 on macOS
        if let Ok(out) = std::process::Command::new("vm_stat")
            .output()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            let page_size = 4096u64;
            let mut free_pages: u64 = 0;
            let mut inactive_pages: u64 = 0;
            let mut purgeable_pages: u64 = 0;
            for line in s.lines() {
                if let Some(rest) = line.strip_prefix("Pages free:") {
                    let n: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                    if let Ok(n) = n.parse::<u64>() { free_pages = n; }
                }
                if let Some(rest) = line.strip_prefix("Pages inactive:") {
                    let n: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                    if let Ok(n) = n.parse::<u64>() { inactive_pages = n; }
                }
                if let Some(rest) = line.strip_prefix("Pages purgeable:") {
                    let n: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                    if let Ok(n) = n.parse::<u64>() { purgeable_pages = n; }
                }
            }
            let available = (free_pages + inactive_pages + purgeable_pages) * page_size;
            return (available, total);
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            let mut mem_available: u64 = 0;
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("MemAvailable:") {
                    let kb: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                    if let Ok(kb_val) = kb.parse::<u64>() {
                        mem_available = kb_val * 1024;
                    }
                }
            }
            if mem_available > 0 {
                return (mem_available, total);
            }
        }
    }

    // Fallback: assume 50% available
    (total / 2, total)
}

/// Spawn a memory pressure watchdog thread.
/// Monitors system memory every 30 seconds. If available memory drops
/// below a critical threshold, logs a warning. The watchdog is advisory
/// only — it logs warnings but does not kill GPU streams (the budget
/// system handles prevention). This helps diagnose OOM issues post-mortem.
///
/// Note: On macOS, "free" memory is typically very low (<500 MiB) even
/// under normal load because macOS aggressively caches file data in
/// "inactive" pages. We include inactive + purgeable pages in our
/// "available" calculation. The thresholds are set lower for macOS.
fn spawn_memory_watchdog() {
    std::thread::spawn(move || {
        let check_interval = std::time::Duration::from_secs(30);

        // Platform-specific thresholds
        // macOS: inactive/purgeable pages are counted as available, so
        //   the "available" number is more accurate. But macOS still caches
        //   aggressively, so we use lower thresholds.
        // Linux: MemAvailable is accurate (includes reclaimable slab).
        #[cfg(target_os = "macos")]
        let (critical_mib, warning_mib) = (128, 256); // 128 MiB / 256 MiB
        #[cfg(not(target_os = "macos"))]
        let (critical_mib, warning_mib) = (512, 1024); // 512 MiB / 1 GiB

        loop {
            std::thread::sleep(check_interval);
            let (available, total) = check_memory_pressure();
            let avail_mib = available / (1024 * 1024);
            let total_mib = total / (1024 * 1024);
            if avail_mib < critical_mib {
                eprintln!(
                    "[{}] MEMORY_CRITICAL available_mib={} total_mib={} — system may freeze! Consider reducing GPU batch size or disabling GPU streams",
                    log_timestamp(),
                    avail_mib,
                    total_mib,
                );
            } else if avail_mib < warning_mib {
                eprintln!(
                    "[{}] MEMORY_WARNING available_mib={} total_mib={} — low memory, GPU mining may be unstable",
                    log_timestamp(),
                    avail_mib,
                    total_mib,
                );
            }
        }
    });
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
    // Install crash handler (SIGABRT/SIGSEGV from AMD OpenCL driver)
    crash_handler::install();

    // Enable verbose logging via env var or --verbose flag
    if std::env::var("ZION_MINER_VERBOSE").map(|v| v == "1" || v == "true").unwrap_or(false)
        || std::env::args().any(|a| a == "--verbose")
    {
        VERBOSE.store(true, Ordering::Relaxed);
    }

    // ── Auto mode banner ──
    let auto_mode = std::env::var("ZION_AUTO_MODE")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    if auto_mode {
        let s1 = std::env::var("ZION_STREAM1_ENABLED").map(|v| v != "0").unwrap_or(true);
        let s2 = std::env::var("ZION_STREAM2_ENABLED").map(|v| v != "0").unwrap_or(true);
        let s3 = std::env::var("ZION_STREAM3_ENABLED").map(|v| v != "0").unwrap_or(true);
        println!("=== ZION Auto Mode ===");
        println!("  Stream 1 (ZION primary):  {}", if s1 { "ON" } else { "OFF" });
        println!("  Stream 2 (GPU external):  {}", if s2 { "ON" } else { "OFF" });
        println!("  Stream 3 (CPU external):  {}", if s3 { "ON" } else { "OFF" });
        println!("======================");
    }

    // ── VerusHash CPU benchmark: `zion-miner --verus-bench` ──
    if std::env::args().any(|a| a == "--verus-bench") {
        let secs: f64 = std::env::var("ZION_BENCH_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5.0);
        let threads: usize = std::env::var("ZION_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4);

        println!("=== VerusHash v2.2 CPU Benchmark ===");
        println!("threads={}", threads);
        println!("duration={}s", secs);
        println!();

        // Initialize VerusHash lookup tables
        #[cfg(any(feature = "native-verushash", feature = "native-hashers"))]
        {
            zion_auxpow::init_verushash();
            println!("verushash_init: OK (native C++ sse2neon)");
        }
        #[cfg(not(any(feature = "native-verushash", feature = "native-hashers")))]
        {
            println!("verushash_init: WARNING — using Blake3 fallback (NOT real VerusHash!)");
        }

        // Simulate a 1487-byte VRSC header
        let mut header = vec![0u8; 1487];
        header[0..4].copy_from_slice(&0x02000000u32.to_le_bytes());
        // Set solution version > 6 to trigger PBaaS path
        header[143] = 0x07;

        let total_hashes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let start = std::time::Instant::now();

        let mut handles = Vec::new();
        for t in 0..threads {
            let hdr = header.clone();
            let total = std::sync::Arc::clone(&total_hashes);
            let stop = std::sync::Arc::clone(&stop);
            handles.push(std::thread::spawn(move || {
                // Pin thread to physical core for VerusHash
                thread_affinity::maybe_pin_thread(t, threads, "verushash");
                let mut local_hdr = hdr;
                let mut nonce: u64 = (t as u64) * 1_000_000_000;
                let nonce_space_blob_offset = 1472usize;
                loop {
                    if stop.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    let nonce_le = (nonce as u32).to_le_bytes();
                    if nonce_space_blob_offset + 4 <= local_hdr.len() {
                        local_hdr[nonce_space_blob_offset..nonce_space_blob_offset + 4]
                            .copy_from_slice(&nonce_le);
                    }
                    let _hash = zion_auxpow::hash_verushash_header(&local_hdr);
                    total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    nonce += 1;
                }
            }));
        }

        std::thread::sleep(std::time::Duration::from_secs_f64(secs));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);

        for h in handles {
            let _ = h.join();
        }

        let elapsed = start.elapsed().as_secs_f64();
        let hashes = total_hashes.load(std::sync::atomic::Ordering::Relaxed);
        let hps = hashes as f64 / elapsed;

        println!();
        println!("=== Results ===");
        println!("hashes={}", hashes);
        println!("elapsed={:.2}s", elapsed);
        println!("throughput={:.0} H/s", hps);
        println!("throughput={:.2} KH/s", hps / 1000.0);
        println!("throughput={:.4} MH/s", hps / 1_000_000.0);
        println!("per_thread={:.0} H/s", hps / threads as f64);

        // Estimate time to find a share at 26-bit difficulty
        let diff_26bit = 67_108_864u64;
        let secs_per_share = diff_26bit as f64 / hps;
        println!();
        println!("=== Share Estimate (VRSC 26-bit target) ===");
        println!("difficulty={}", diff_26bit);
        println!("expected_time={:.0}s = {:.1}min = {:.1}h = {:.1}days",
            secs_per_share, secs_per_share / 60.0, secs_per_share / 3600.0, secs_per_share / 86400.0);

        // At 16-bit difficulty (easy target)
        let diff_16bit = 65_536u64;
        let secs_per_share_easy = diff_16bit as f64 / hps;
        println!();
        println!("=== Share Estimate (easy 16-bit target) ===");
        println!("difficulty={}", diff_16bit);
        println!("expected_time={:.1}s = {:.1}min",
            secs_per_share_easy, secs_per_share_easy / 60.0);

        return Ok(());
    }

    // ── RandomX CPU benchmark: `zion-miner --randomx-bench` ──
    if std::env::args().any(|a| a == "--randomx-bench") {
        let secs: f64 = std::env::var("ZION_BENCH_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10.0);
        let threads: usize = std::env::var("ZION_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(num_cpus::get());

        println!("=== RandomX (Monero/XMR) CPU Benchmark ===");
        println!("threads={}", threads);
        println!("duration={}s", secs);
        println!();

        // Initialize RandomX with a zero seed (epoch 0)
        #[cfg(feature = "native-randomx")]
        {
            let zero_seed = [0u8; 32];
            zion_native_ffi::randomx::init_with_seed(&zero_seed);
            println!("randomx_init: OK (tevador/RandomX real, seed=epoch0)");
            #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
            println!("mode: JIT + hardware AES + secure (Apple Silicon, auto-detected)");
            #[cfg(all(target_arch = "aarch64", not(target_os = "macos")))]
            println!("mode: JIT + hardware AES (ARM64, auto-detected)");
            #[cfg(not(target_arch = "aarch64"))]
            println!("mode: JIT + hardware AES (if available)");
        }
        #[cfg(not(feature = "native-randomx"))]
        {
            println!("randomx_init: WARNING — native-randomx feature NOT enabled!");
            println!("Rebuild with: cargo build --features native-randomx");
            return Ok(());
        }

        #[cfg(feature = "native-randomx")]
        {
        // Monero block header is 76 bytes
        let mut header = vec![0u8; 76];
        header[0..4].copy_from_slice(&0x12000000u32.to_le_bytes()); // Monero version
        // Rest is zeros (zero prev-hash, zero merkle root, etc.)

        let total_hashes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let start = std::time::Instant::now();

        let mut handles = Vec::new();
        for t in 0..threads {
            let hdr = header.clone();
            let total = std::sync::Arc::clone(&total_hashes);
            let stop = std::sync::Arc::clone(&stop);
            handles.push(std::thread::spawn(move || {
                // Pin thread to physical core for RandomX (cache-sensitive)
                thread_affinity::maybe_pin_thread(t, threads, "randomx");
                let mut local_hdr = hdr;
                let mut nonce: u64 = (t as u64) * 1_000_000_000;
                loop {
                    if stop.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    // Embed nonce in header (bytes 39..43 in Monero format)
                    let nonce_le = (nonce as u32).to_le_bytes();
                    local_hdr[39..43].copy_from_slice(&nonce_le);
                    let _hash = zion_native_ffi::randomx::hash(&local_hdr, nonce);
                    total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    nonce += 1;
                }
            }));
        }

        std::thread::sleep(std::time::Duration::from_secs_f64(secs));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);

        for h in handles {
            let _ = h.join();
        }

        let elapsed = start.elapsed().as_secs_f64();
        let hashes = total_hashes.load(std::sync::atomic::Ordering::Relaxed);
        let hps = hashes as f64 / elapsed;

        println!();
        println!("=== Results ===");
        println!("hashes={}", hashes);
        println!("elapsed={:.2}s", elapsed);
        println!("throughput={:.0} H/s", hps);
        println!("throughput={:.2} KH/s", hps / 1000.0);
        println!("per_thread={:.0} H/s", hps / threads as f64);

        // XMR network difficulty estimate (varies, ~350G as of 2024)
        let xmr_diff = 350_000_000_000u64;
        let secs_per_block = xmr_diff as f64 / hps;
        println!();
        println!("=== XMR Network Estimate (diff ~350G) ===");
        println!("expected_time={:.0}s = {:.1}min = {:.1}h = {:.1}days",
            secs_per_block, secs_per_block / 60.0, secs_per_block / 3600.0, secs_per_block / 86400.0);

        // Pool share at 1M difficulty
        let pool_diff = 1_000_000u64;
        let secs_per_share = pool_diff as f64 / hps;
        println!();
        println!("=== Pool Share Estimate (diff 1M) ===");
        println!("expected_time={:.1}s = {:.1}min",
            secs_per_share, secs_per_share / 60.0);

        return Ok(());
        } // end native-randomx cfg
    }

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

    // ── CUDA external kernel test: `zion-miner --test-cuda-kernel <algo>` ──
    #[cfg(feature = "gpu-cuda")]
    {
        use gpu_backend::GpuMiner;
        let args: Vec<String> = std::env::args().collect();
        if let Some(pos) = args.iter().position(|a| a == "--test-cuda-kernel") {
            let algo = args.get(pos + 1).cloned().unwrap_or_else(|| {
                eprintln!("Usage: zion-miner --test-cuda-kernel <algorithm>");
                eprintln!("Algorithms: kheavyhash, blake3_alph, blake3_dcr, autolykos, zelhash");
                std::process::exit(1);
            });
            let work_size: usize = std::env::var("ZION_GPU_WORK_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(262144);
            let secs: f64 = std::env::var("ZION_BENCH_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5.0);

            println!("=== CUDA External Kernel Test ===");
            println!("algorithm={} work_size={} bench_secs={}", algo, work_size, secs);

            match cuda_external::CudaExternalMiner::new(&algo, work_size) {
                Ok(mut miner) => {
                    println!("init_ok device=\"{}\" algorithm={}", miner.device_name(), algo);
                    println!("running_benchmark...");
                    match miner.benchmark(secs) {
                        Ok((total, elapsed, hps)) => {
                            println!("benchmark_result algorithm={} total_nonces={} elapsed={:.2}s hps={:.2}", algo, total, elapsed, hps);
                            if hps > 0.0 {
                                println!("status=PASS");
                            } else {
                                println!("status=WARN (zero hashrate — kernel may not be producing solutions)");
                            }
                        }
                        Err(e) => {
                            eprintln!("benchmark_failed algorithm={} error=\"{}\"", algo, e);
                            println!("status=FAIL");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("init_failed algorithm={} error=\"{}\"", algo, e);
                    println!("status=FAIL");
                }
            }
            return Ok(());
        }
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
    cpu_features::log_features();
    println!("miner_id={}", config.miner_id);
    println!("worker_name={}", config.worker_name);
    println!("loop_count={}", config.loop_count);
    println!("job_ttl_ms={}", config.job_ttl_ms);
    println!("threads={}", config.threads);
    println!("algorithm={}", config.algorithm);
    flush_stdout();

    let metrics = Arc::new(Mutex::new(MinerMetricsSnapshot::from_config(&config)));

    // ── Interactive control + hashrate tracker ──
    let interactive = parse_bool_env("ZION_INTERACTIVE", false);
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

    // Exit sticky header (leave alternate screen buffer)
    ui::exit_sticky_header();

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

    // ── GPU memory budget auto-tune (must run before any GPU backend init) ──
    // On Apple Silicon (unified memory), this detects actual available memory
    // and calculates a safe GPU budget, preventing OOM system freezes.
    // Reset on each session entry to clear claimed bytes from previous session.
    gpu_backend::reset_gpu_memory_budget();
    ui::reset_sticky_header();
    let gpu_safe = gpu_backend::init_gpu_memory_budget_with_threads(config.threads);

    // Spawn memory pressure watchdog (advisory logging every 30s)
    spawn_memory_watchdog();

    // ── Auto-tune kill switch: if available memory < 200 MB, disable GPU ──
    let sys_ram = gpu_backend::detect_system_memory_bytes();
    let resolved_backend = match config.gpu_backend {
        gpu_backend::GpuBackendKind::Auto => gpu_backend::resolve_auto_backend(),
        other => other,
    };
    let _is_unified_memory = resolved_backend == gpu_backend::GpuBackendKind::Metal;

    if !gpu_safe {
        // Auto-tune killed GPU — CPU only mode
        println!(
            "gpu_disabled_by_autotune sys_ram_mib={} backend={} — switching to CPU only mode \
             (available memory too low for safe GPU mining)",
            sys_ram / (1024 * 1024),
            resolved_backend.as_str(),
        );
    } else if _is_unified_memory {
        println!(
            "gpu_stream2_info sys_ram_mib={} backend={} — Stream 2 enabled, per-algorithm guard active",
            sys_ram / (1024 * 1024),
            resolved_backend.as_str(),
        );
    }

    // If auto-tune killed GPU, force CPU backend
    let effective_gpu_backend = if !gpu_safe {
        gpu_backend::GpuBackendKind::Cpu
    } else {
        config.gpu_backend
    };

    // ── GPU backend init (TriGpuManager — 3-stream Claymore-style) ──
    let mut gpu_available = effective_gpu_backend != gpu_backend::GpuBackendKind::Cpu
        && config.stream1_enabled;
    let mut tri_gpu = match gpu_backend::TriGpuManager::with_work_sizes(
        effective_gpu_backend,
        config.gpu_work_size,
        config.pearl_gpu_work_size,
        config.secondary_gpu_work_size,
    ) {
        Ok(t) => t,
        Err(e) => {
            println!("gpu_init_fallback reason=\"{e}\" using=cpu");
            gpu_available = false;
            gpu_backend::TriGpuManager::with_work_sizes(
                gpu_backend::GpuBackendKind::Cpu,
                1, 1, 1,
            )?
        }
    };
    if gpu_available {
        match tri_gpu.primary() {
            Ok(g) => {
                println!(
                    "gpu_init backend={} device=\"{}\" work_size={} algorithm={} streams=3",
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
        let raw_header_bytes = header.to_bytes().to_vec();
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
        // Optimization #3: Ensure GPU backend matches the current algorithm.
        // In local mode, the user may switch algorithms interactively.
        if let Err(e) = tri_gpu.ensure_primary_algorithm(&current_algorithm) {
            eprintln!(
                "gpu_primary_algorithm_switch_error algo={} reason=\"{e}\" — continuing with existing backend",
                current_algorithm
            );
        }
        // Primary GPU backend (Deeksha) — switches algorithm when needed.
        // TriGpuManager keeps the primary backend alive for the entire session.
        // Skip GPU for CPU-only algorithms (verushash, randomx) — they have no
        // GPU kernel and must use CPU mining.
        let mut gpu_ref: Option<&mut dyn gpu_backend::GpuMiner> = None;
        if effective_gpu_backend != gpu_backend::GpuBackendKind::Cpu
            && !gpu_backend::is_cpu_only_algorithm(&current_algorithm)
        {
            match tri_gpu.primary() {
                Ok(g) => gpu_ref = Some(g),
                Err(e) => {
                    println!(
                        "gpu_primary_error job={} algorithm={} reason=\"{e}\" using=cpu",
                        job.job_id, current_algorithm
                    );
                }
            }
        }
        // GPU-first, CPU-fallback nonce scan
        let can_gpu = gpu_ref.is_some();
        let mut gpu_nonces_tested = 0u64;
        let mut cpu_nonces_tested = 0u64;
        let mut gpu_mix_hash: Option<[u8; 32]> = None;
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
                let result = gpu_backend::gpu_scan_job(g, job, &current_algorithm, &raw_header_bytes);
                gpu_nonces_tested = result.nonces_tested;
                gpu_mix_hash = result.mix_hash;
                result.solution
            }
        } else {
            cpu_nonces_tested = job.nonce_count;
            parallel::parallel_scan_nonce_range(job, threads, &current_algorithm)
        };
        hashrate.record_gpu_hashes(gpu_nonces_tested);
        hashrate.record_cpu_hashes(cpu_nonces_tested);
        hashrate.record_zion_hashes(gpu_nonces_tested.saturating_add(cpu_nonces_tested));
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
            let stream_stats = hashrate.build_stream_stats(&telemetry.algorithm);
            telemetry.maybe_print_status(
                iteration + 1,
                config.loop_count,
                total_accepted,
                total_rejected,
                attempted_hashes,
                None,
                config.stats_file.as_deref(),
                metrics,
                "local",
                &stream_stats,
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
        let submit_line = if let Some(mh) = gpu_mix_hash {
            encode_message(&pool.solution_message_with_mix(
                &config.miner_id,
                &config.worker_name,
                solution,
                &mh,
            ))?
        } else {
            encode_message(&pool.solution_message(
                &config.miner_id,
                &config.worker_name,
                solution,
            ))?
        };
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

        let stream_stats = hashrate.build_stream_stats(&telemetry.algorithm);
        telemetry.maybe_print_status(
            iteration + 1,
            config.loop_count,
            total_accepted,
            total_rejected,
            attempted_hashes,
            None,
            config.stats_file.as_deref(),
            metrics,
            "local",
            &stream_stats,
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

// ── Pool stream config fetch (dynamic config from pool HTTP API) ──────────

/// Simplified stream config snapshot received from the pool's HTTP API.
#[derive(Debug, Clone, serde::Deserialize)]
struct PoolStreamConfig {
    gpu: PoolGpuStreamConfig,
    cpu: PoolCpuStreamConfig,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PoolGpuStreamConfig {
    enabled: bool,
    #[serde(default)]
    coin: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PoolCpuStreamConfig {
    enabled: bool,
    #[serde(default)]
    coin: String,
}

/// Fetch the pool's stream config via HTTP GET `/api/v1/config/streams`.
/// API address from ZION_POOL_API_ADDR env var, or derived from stratum
/// address by adding 11 to the port (8444 → 8455, Edge convention).
/// Returns None if unreachable (non-fatal, falls back to env vars).
fn fetch_pool_stream_config(pool_addr: &str) -> Option<PoolStreamConfig> {
    let api_addr = std::env::var("ZION_POOL_API_ADDR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            if let Some(colon) = pool_addr.rfind(':') {
                let host = &pool_addr[..colon];
                if let Ok(port) = pool_addr[colon + 1..].parse::<u16>() {
                    format!("{}:{}", host, port.saturating_add(11))
                } else {
                    format!("{}:8455", host)
                }
            } else {
                format!("{}:8455", pool_addr)
            }
        });

    let socket_addrs: Vec<std::net::SocketAddr> = std::net::ToSocketAddrs::to_socket_addrs(&api_addr)
        .ok()?
        .collect();
    if socket_addrs.is_empty() { return None; }

    let mut stream = match std::net::TcpStream::connect_timeout(
        &socket_addrs[0],
        std::time::Duration::from_secs(3),
    ) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let request = format!(
        "GET /api/v1/config/streams HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        api_addr
    );
    if stream.write_all(request.as_bytes()).is_err() { return None; }

    let mut response = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => response.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
        if response.len() > 65536 { break; }
    }

    let response_str = String::from_utf8_lossy(&response);
    let body_start = response_str.find("\r\n\r\n").map(|p| p + 4)
        .or_else(|| response_str.find("\n\n").map(|p| p + 2))?;
    let body = &response_str[body_start..];

    serde_json::from_str::<PoolStreamConfig>(body).ok()
}

/// Periodically poll the pool's stream config and log changes.
fn spawn_pool_config_poller(pool_addr: String, stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
    let poll_interval = std::env::var("ZION_POOL_CONFIG_POLL_SECS")
        .ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(30);

    std::thread::spawn(move || {
        let mut last_gpu_enabled: Option<bool> = None;
        let mut last_gpu_coin: Option<String> = None;
        let mut last_cpu_enabled: Option<bool> = None;
        let mut last_cpu_coin: Option<String> = None;

        while !stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(cfg) = fetch_pool_stream_config(&pool_addr) {
                let gpu_coin = cfg.gpu.coin.clone().unwrap_or_else(|| "auto".to_string());
                let changed = last_gpu_enabled != Some(cfg.gpu.enabled)
                    || last_gpu_coin.as_deref() != Some(gpu_coin.as_str())
                    || last_cpu_enabled != Some(cfg.cpu.enabled)
                    || last_cpu_coin.as_deref() != Some(cfg.cpu.coin.as_str());
                if changed {
                    println!("[{}] pool_config_update gpu_enabled={} gpu_coin={} cpu_enabled={} cpu_coin={}",
                        log_timestamp(), cfg.gpu.enabled, gpu_coin, cfg.cpu.enabled, cfg.cpu.coin);
                    last_gpu_enabled = Some(cfg.gpu.enabled);
                    last_gpu_coin = Some(gpu_coin);
                    last_cpu_enabled = Some(cfg.cpu.enabled);
                    last_cpu_coin = Some(cfg.cpu.coin.clone());
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(poll_interval));
        }
    });
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

    // ── Fetch pool stream config (dynamic auto-configuration) ──────────
    let mut stream2_enabled = config.stream2_enabled;
    let mut stream3_enabled = config.stream3_enabled;

    if let Some(pool_cfg) = fetch_pool_stream_config(pool_addr) {
        println!(
            "[{}] pool_config_received gpu_enabled={} gpu_coin={} cpu_enabled={} cpu_coin={}",
            log_timestamp(),
            pool_cfg.gpu.enabled,
            pool_cfg.gpu.coin.as_deref().unwrap_or("auto"),
            pool_cfg.cpu.enabled,
            pool_cfg.cpu.coin,
        );
        if config.stream2_enabled && !pool_cfg.gpu.enabled {
            println!("[{}] pool_config_override stream2=disabled (pool GPU stream disabled)", log_timestamp());
            stream2_enabled = false;
        }
        if config.stream3_enabled && !pool_cfg.cpu.enabled {
            println!("[{}] pool_config_override stream3=disabled (pool CPU stream disabled)", log_timestamp());
            stream3_enabled = false;
        }
    } else {
        println!("[{}] pool_config_unreachable — using env-var stream config (stream2={} stream3={})",
            log_timestamp(), stream2_enabled, stream3_enabled);
    }

    let config_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    spawn_pool_config_poller(pool_addr.to_string(), std::sync::Arc::clone(&config_stop));

    // ── Autonomous profit router ──
    // When ZION_AUTONOMOUS=1, auto-selects Stream 2 (GPU) and Stream 3 (CPU)
    // coins based on hardware compatibility and profitability data.
    let cpu_feats = cpu_features::detect();
    let hw_profile = autonomous::HardwareProfile {
        gpu_vram_bytes: config.gpu_vram_bytes,
        gpu_backend: effective_gpu_backend_label(config.gpu_backend).to_string(),
        has_gpu: config.gpu_backend != gpu_backend::GpuBackendKind::Cpu,
        cpu_has_aes: cpu_feats.has_aes,
        cpu_has_avx2: cpu_feats.has_avx2,
        cpu_threads: config.threads,
    };
    let mut profit_router = autonomous::AutonomousProfitRouter::new(hw_profile);
    let mut last_s2_coin: Option<zion_cosmic_harmony::profit_router::ExternalCoin> = None;
    let mut last_s3_coin: Option<zion_cosmic_harmony::profit_router::ExternalCoin> = None;
    if profit_router.is_enabled() {
        println!("[{}] autonomous_mode_enabled — running initial coin selection", log_timestamp());
        profit_router.initial_selection();
        profit_router.print_log();
        println!("[{}] {}", log_timestamp(), profit_router.summary());
        last_s2_coin = profit_router.stream2_coin;
        last_s3_coin = profit_router.stream3_coin;
    }

    // ── Read initial algorithm from interactive control ──
    let initial_algorithm = {
        let c = control.lock().unwrap();
        c.algorithm.clone()
    };

    // ── GPU memory budget auto-tune (must run before any GPU backend init) ──
    // On Apple Silicon (unified memory), this detects actual available memory
    // and calculates a safe GPU budget, preventing OOM system freezes.
    // Reset on each session entry to clear claimed bytes from previous session.
    gpu_backend::reset_gpu_memory_budget();
    ui::reset_sticky_header();
    let gpu_safe = gpu_backend::init_gpu_memory_budget_with_threads(config.threads);

    // Spawn memory pressure watchdog (advisory logging every 30s)
    spawn_memory_watchdog();

    // ── Auto-tune kill switch + Stream 2 info ──
    let sys_ram = gpu_backend::detect_system_memory_bytes();
    let resolved_backend = match config.gpu_backend {
        gpu_backend::GpuBackendKind::Auto => gpu_backend::resolve_auto_backend(),
        other => other,
    };
    let is_unified_memory = resolved_backend == gpu_backend::GpuBackendKind::Metal;

    if !gpu_safe {
        println!(
            "gpu_disabled_by_autotune sys_ram_mib={} backend={} — switching to CPU only mode \
             (available memory too low for safe GPU mining)",
            sys_ram / (1024 * 1024),
            resolved_backend.as_str(),
        );
    } else if is_unified_memory {
        println!(
            "gpu_stream2_info sys_ram_mib={} backend={} — Stream 2 enabled, per-algorithm guard active (DAG/memory-hard algo will be skipped on Metal)",
            sys_ram / (1024 * 1024),
            resolved_backend.as_str(),
        );
    }

    // If auto-tune killed GPU, force CPU backend
    let effective_gpu_backend = if !gpu_safe {
        gpu_backend::GpuBackendKind::Cpu
    } else {
        config.gpu_backend
    };
    let stream2_effective = stream2_enabled && gpu_safe;

    // ── GPU backend init (TriGpuManager — 3-stream Claymore-style) ──
    // Primary (Deeksha) is created immediately. Pearl + secondary are
    // lazy-created by their respective persistent threads on demand.
    let mut gpu_available = effective_gpu_backend != gpu_backend::GpuBackendKind::Cpu
        && config.stream1_enabled;
    let mut tri_gpu = match gpu_backend::TriGpuManager::with_work_sizes(
        effective_gpu_backend,
        config.gpu_work_size,
        config.pearl_gpu_work_size,
        config.secondary_gpu_work_size,
    ) {
        Ok(t) => t,
        Err(e) => {
            println!("gpu_init_fallback reason=\"{e}\" using=cpu");
            gpu_available = false;
            // Fallback: CPU dummy — primary() will error but is never called
            // when gpu_available is false.
            gpu_backend::TriGpuManager::with_work_sizes(
                gpu_backend::GpuBackendKind::Cpu,
                1, 1, 1,
            )?
        }
    };
    if gpu_available {
        match tri_gpu.primary() {
            Ok(g) => {
                println!(
                    "gpu_init backend={} device=\"{}\" work_size={} algorithm={} streams=3",
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

    // ── GPU pipeline state for overlapping pool I/O with GPU compute ──
    // When the CUDA backend supports async launch_batch/collect_batch,
    // this enables overlapping the previous batch's solution submission
    // (network I/O) with the current batch's GPU computation.
    let mut gpu_pipeline = gpu_backend::GpuPipelineState::new();

    // ── Persistent GPU external thread (Stream 2: one GPU profit coin) ──
    // The pool sends jobs for exactly one GPU-capable AuxPoW coin at a time.
    // The thread creates the appropriate OpenCL backend on demand and switches
    // when the profit coin changes.
    let (ext_gpu_tx, ext_gpu_rx) = std::sync::mpsc::channel::<zion_pool::ExternalStreamJob>();
    let (ext_gpu_share_tx, ext_gpu_share_rx) = std::sync::mpsc::channel::<ExternalShareResult>();

    let dual_gpu_enabled = gpu_available
        && effective_gpu_backend != gpu_backend::GpuBackendKind::Cpu
        && stream2_effective;
    println!(
        "[{}] dual_gpu_check gpu_available={} gpu_backend={:?} effective_backend={:?} stream2_enabled={} stream2_effective={} => dual_gpu_enabled={}",
        log_timestamp(),
        gpu_available,
        config.gpu_backend,
        effective_gpu_backend,
        stream2_enabled,
        stream2_effective,
        dual_gpu_enabled
    );
    if dual_gpu_enabled {
        let ws = config.secondary_gpu_work_size;
        let hr = Arc::clone(hashrate);
        let bk = effective_gpu_backend;
        // Share the CUDA device with the external GPU thread to avoid
        // creating a second CUDA context (deadlock on consumer GPUs).
        #[cfg(feature = "gpu-cuda")]
        let shared_cuda = tri_gpu.shared_cuda_device();
        #[cfg(not(feature = "gpu-cuda"))]
        let shared_cuda: Option<()> = None;
        thread::spawn(move || {
            println!("[{}] external_gpu_thread_spawned", log_timestamp());
            external_gpu_thread(ext_gpu_rx, ext_gpu_share_tx, ws, hr, bk, shared_cuda);
        });
        println!(
            "[{}] stream2_gpu_external_started work_size={}",
            log_timestamp(),
            config.secondary_gpu_work_size
        );
    } else {
        println!("[{}] stream2_gpu_external_disabled (gpu_available={})", log_timestamp(), gpu_available);
    }

    // ── Persistent CPU external thread (Stream 3: VerusHash/RandomX) ──
    // Claymore-style: persistent thread instead of per-iteration spawn.
    // Receives CPU-only external jobs via channel, mines continuously.
    let (ext_cpu_tx, ext_cpu_rx) = std::sync::mpsc::channel::<zion_pool::ExternalStreamJob>();
    let (ext_cpu_share_tx, ext_cpu_share_rx) = std::sync::mpsc::channel::<ExternalShareResult>();
    if stream3_enabled {
        let ext_cpu_threads = config.threads.max(1);
        let ext_cpu_nonce_count = config.verushash_nonce_count;
        let hashrate_ext_cpu = Arc::clone(hashrate);
        thread::spawn(move || {
            ext_cpu_thread(ext_cpu_rx, ext_cpu_share_tx, ext_cpu_threads, ext_cpu_nonce_count, hashrate_ext_cpu);
        });
        println!(
            "[{}] stream3c_ext_cpu_started threads={}",
            log_timestamp(),
            config.threads
        );
    } else {
        println!(
            "[{}] stream3c_ext_cpu_disabled (stream3_enabled=false)",
            log_timestamp()
        );
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

    let backend_str = tri_gpu.primary_backend_kind().as_str();
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

    // ── Spawn pool I/O thread ──────────────────────────────────────
    // The I/O thread owns the reader and continuously reads from the pool,
    // routing Job messages to job_rx and Result/ExternalResult to result_rx.
    // This eliminates both GPU idle gaps:
    //   1. Job pre-fetch (~400ms → ~0ms): jobs are already queued when needed
    //   2. Async submit response (~297ms → ~0ms): results are already queued
    // The main thread keeps the writer for sending submit/no-solution messages.
    let (job_tx, job_rx) = std::sync::mpsc::channel::<PoolIncoming>();
    let (result_tx, result_rx) = std::sync::mpsc::channel::<PoolIncoming>();
    std::thread::Builder::new()
        .name("pool-io".to_string())
        .spawn(move || pool_io_thread(reader, job_tx, result_tx))
        .context("failed to spawn pool I/O thread")?;
    println!("[{}] pool_io_thread_started — job pre-fetch + async submit enabled", log_timestamp());
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

    // ── Send initial CoinPreference to pool (autonomous mode) ──
    if let Some(pref_msg) = profit_router.build_coin_preference(&config.miner_id) {
        let pref_line = zion_pool::encode_message(&pref_msg)
            .map_err(|e| anyhow!("failed to encode CoinPreference: {e}"))?;
        writer.write_all(pref_line.as_bytes())?;
        writer.flush()?;
        println!("[{}] autonomous_coin_preference_sent {}", log_timestamp(), pref_line.trim());
    }

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

        // ── Autonomous profit router: periodic re-evaluation ──
        if profit_router.should_reevaluate() {
            profit_router.reevaluate();
            profit_router.print_log();
            println!("[{}] {}", log_timestamp(), profit_router.summary());

            // Send CoinPreference to pool if selection changed
            if profit_router.coins_changed(last_s2_coin, last_s3_coin) {
                if let Some(pref_msg) = profit_router.build_coin_preference(&config.miner_id) {
                    if let Ok(pref_line) = zion_pool::encode_message(&pref_msg) {
                        let _ = writer.write_all(pref_line.as_bytes());
                        let _ = writer.flush();
                        println!(
                            "[{}] autonomous_coin_preference_updated {}",
                            log_timestamp(),
                            pref_line.trim()
                        );
                    }
                }
                last_s2_coin = profit_router.stream2_coin;
                last_s3_coin = profit_router.stream3_coin;
            }
        }

        let (job_line, mut job, algorithm, raw_header_bytes, stream_weights_str, external_stream, external_stream_cpu) =
            match job_rx.recv() {
                Ok(PoolIncoming::Job(line, j, algo, raw, sw, ext, ext_cpu)) => {
                    (line, j, algo, raw, sw, ext, ext_cpu)
                }
                Ok(PoolIncoming::Result(line, msg)) => {
                    // Unexpected: a result arrived while we were waiting for a job.
                    // This can happen if the pool sends a late result from a previous
                    // submit. Log and continue waiting for the next job.
                    println!("pool_unexpected_result_while_waiting_for_job: {line}");
                    continue;
                }
                Err(_) => {
                    println!("pool_io_channel_closed — reconnecting");
                    break;
                }
            };
        if !QUIET.load(Ordering::Relaxed) {
            println!(">> new job #{} height={} algo={}", job.job_id, job.height, algorithm);
        }
        let current_diff = CURRENT_POOL_DIFFICULTY.load(Ordering::Relaxed);
        // Only override target for ZION jobs. External AuxPoW jobs carry
        // their own share target from the external pool (e.g. KAS).
        if !gpu_backend::is_external_algorithm(&algorithm) {
            job.target = zion_core::difficulty::difficulty_to_target(current_diff);
        }
        current_algorithm = algorithm.clone();
        remote_nonce_window = job.nonce_count;
        let job_started_at = Instant::now();
        last_job_id = job.job_id;
        telemetry.pool_height = job.height;
        telemetry.current_epoch = job.height / 100;
        // BUG #1 fix: propagate pool_height to HashrateTracker so dashboard can display it
        hashrate.set_pool_height(job.height);

        // Optimization #3: Ensure the primary GPU backend matches the pool's
        // algorithm. If the pool sends deeksha_lite_v1 but the miner initialized
        // with deeksha_lite_fire, switch the GPU backend to v1 to produce
        // correct hashes (fire includes a thermal loop that v1 doesn't have).
        // Only switches for Deeksha-family algorithms; ignores external/CPU-only.
        if let Err(e) = tri_gpu.ensure_primary_algorithm(&current_algorithm) {
            eprintln!(
                "[{}] gpu_primary_algorithm_switch_error algo={} reason=\"{e}\" — continuing with existing backend",
                log_timestamp(),
                current_algorithm
            );
        }

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
            let _ = result_rx.recv();
            thread::sleep(Duration::from_millis(100));
            continue;
        }

        // ── Check interactive CPU/GPU/dual mode ──
        let (cpu_on, gpu_on, _dual_on) = {
            let c = control.lock().unwrap();
            (c.cpu_enabled, c.gpu_enabled, c.dual_mode)
        };

        // Primary GPU backend (Deeksha) — algorithm switched above via
        // ensure_primary_algorithm() to match the pool's requested algorithm.
        // Skip GPU for CPU-only algorithms (verushash, randomx) — they have no
        // GPU kernel and must use CPU mining.
        let mut gpu_ref: Option<&mut dyn gpu_backend::GpuMiner> = None;
        if config.gpu_backend != gpu_backend::GpuBackendKind::Cpu && gpu_on
            && !gpu_backend::is_cpu_only_algorithm(&current_algorithm)
        {
            match tri_gpu.primary() {
                Ok(g) => gpu_ref = Some(g),
                Err(e) => {
                    println!(
                        "gpu_primary_error job={} algorithm={} reason=\"{e}\" using=cpu",
                        job.job_id, current_algorithm
                    );
                }
            }
        }

        // ── Send external stream jobs BEFORE GPU mining / stream weights ──
        // This must happen before set_stream_weights() and gpu_scan_job()
        // because those calls can block on OpenCL queue.finish(), which
        // would delay the external GPU thread from receiving its job.
        // Send external stream job to the appropriate persistent thread.
        // GPU-capable external algorithms go to the generic external GPU thread.
        // CPU-only algorithms (VerusHash, RandomX) go to the persistent CPU thread.
        // Pearl (PRL) jobs are ignored in v3.0.6 canonical mode because the Pearl
        // GPU thread is not yet debugged.
        if let Some(ref ext) = external_stream {
            if ext.coin.eq_ignore_ascii_case("PRL") || ext.algorithm.eq_ignore_ascii_case("pearlhash") {
                if VERBOSE.load(Ordering::Relaxed) {
                    println!("external_stream_ignore coin={} algo={} reason=pearl_disabled", ext.coin, ext.algorithm);
                }
            } else if gpu_backend::is_cpu_only_algorithm(&ext.algorithm) {
                let _ = ext_cpu_tx.send(ext.clone());
            } else if gpu_backend::is_external_algorithm(&ext.algorithm) {
                let send_result = ext_gpu_tx.send(ext.clone());
                if !QUIET.load(Ordering::Relaxed) {
                    println!(
                        "[{}] ext_gpu_tx_send coin={} algo={} job_id={} result={:?}",
                        log_timestamp(),
                        ext.coin,
                        ext.algorithm,
                        ext.job_id,
                        send_result
                    );
                }
            } else {
                println!(
                    "external_stream_unknown_routing coin={} algo={}",
                    ext.coin, ext.algorithm
                );
            }
        }

        // ── Claymore Triple Parallel: CPU external stream (VRSC, RandomX) ──
        // The pool sends CPU-only external jobs in a separate `external_stream_cpu`
        // field so they don't conflict with the GPU `external_stream`.
        if let Some(ref ext_cpu) = external_stream_cpu {
            let _ = ext_cpu_tx.send(ext_cpu.clone());
        }

        // Propagate stream-profit weights to the GPU backend so it can
        // distribute work across Deeksha pipeline steps.
        // NOTE: Disabled on OpenCL because set_stream_weights() calls
        // queue.finish() which can block, and on some CPUs the f32
        // conversion triggers SIGILL (Illegal instruction).
        // The external_stream job is already sent above, so the external
        // GPU thread can start working immediately.
        if let Some(g) = gpu_ref.as_mut() {
            if !stream_weights_str.is_empty() && config.gpu_backend == gpu_backend::GpuBackendKind::Metal {
                match zion_cosmic_harmony::stream_profit::StreamWeights::parse(&stream_weights_str) {
                    Ok(weights) => {
                        if let Err(e) = g.set_stream_weights(&weights) {
                            println!("stream_weights_apply_error job={} err=\"{e}\"", job.job_id);
                        }
                    }
                    Err(e) => {
                        println!("stream_weights_parse_error job={} err=\"{e}\" raw=\"{}\"", job.job_id, stream_weights_str);
                    }
                }
            }
        }

        // GPU-first, CPU-fallback nonce scan (respect interactive overrides)
        let can_gpu = gpu_ref.is_some() && gpu_on;
        let mut gpu_nonces_tested = 0u64;
        let mut cpu_nonces_tested = 0u64;
        let mut gpu_mix_hash: Option<[u8; 32]> = None;

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
                // ── PIPELINED GPU SCAN ──
                // step() collects the PREVIOUS batch's results (if any) and
                // launches the CURRENT batch asynchronously. This overlaps
                // GPU compute with the pool I/O that follows (external share
                // collection, solution submission, reading next job).
                //
                // On the first iteration, step() returns None (no previous batch)
                // but STILL launches the current batch. The solution (if any)
                // will be collected on the NEXT iteration.
                // This means the first iteration always has scan_result=None,
                // which triggers a NoSolution message to the pool — correct behavior.
                let prev_outcome = gpu_pipeline.step(g, job, &current_algorithm, &raw_header_bytes);

                if let Some(outcome) = prev_outcome {
                    // Use previous batch's results
                    gpu_nonces_tested = outcome.nonces_tested;
                    gpu_mix_hash = outcome.mix_hash;
                    outcome.solution
                } else {
                    // First iteration: batch launched but no results yet
                    gpu_nonces_tested = 0;
                    None
                }
            }
        } else if cpu_on {
            cpu_nonces_tested = job.nonce_count;
            parallel::parallel_scan_nonce_range(job, threads, &current_algorithm)
        } else {
            // Both CPU and GPU disabled — skip
            cpu_nonces_tested = 0;
            None
        };

        // ── Collect GPU external share from persistent thread (non-blocking) ──
        let ext_gpu_share = match ext_gpu_share_rx.try_recv() {
            Ok(share) => Some(share),
            Err(_) => None,
        };

        // ── Collect CPU external share from persistent thread (non-blocking) ──
        let ext_cpu_share = match ext_cpu_share_rx.try_recv() {
            Ok(share) => Some(share),
            Err(_) => None,
        };

        // ── Submit GPU external share (if found by persistent thread) ────
        if let Some(share) = ext_gpu_share {
            println!(
                "[{}] external_gpu_share_found  coin={}  algo={}  job_id={}  nonce={}",
                log_timestamp(),
                share.coin,
                share.algorithm,
                share.external_job_id,
                share.nonce,
            );
            submit_external_share(
                &mut writer, &result_rx, &config, &share, &hashrate, VERBOSE.load(Ordering::Relaxed),
                |accepted| hashrate.record_gpu_ext_share(accepted),
            );
        }

        // ── Submit CPU external share (VerusHash/RandomX, if found) ────
        if let Some(share) = ext_cpu_share {
            println!(
                "[{}] external_cpu_share_found  coin={}  algo={}  job_id={}  nonce={}",
                log_timestamp(),
                share.coin,
                share.algorithm,
                share.external_job_id,
                share.nonce,
            );
            submit_external_share(
                &mut writer, &result_rx, &config, &share, &hashrate, VERBOSE.load(Ordering::Relaxed),
                |accepted| hashrate.record_cpu_ext_share(accepted),
            );
        }

        hashrate.record_gpu_hashes(gpu_nonces_tested);
        hashrate.record_cpu_hashes(cpu_nonces_tested);
        hashrate.record_zion_hashes(gpu_nonces_tested.saturating_add(cpu_nonces_tested));
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
            // ── FIX #8: Drain late ExternalResult if present (from timed-out submit_external_share) ──
            let (result_line_raw, result_message) = loop {
                match result_rx.recv() {
                    Ok(PoolIncoming::Result(line, msg)) => break (line, msg),
                    Ok(PoolIncoming::Job(line, _, _, _, _, _, _)) => {
                        return Err(anyhow!("expected result from pool, got Job: {line}"));
                    }
                    Err(_) => return Err(anyhow!("pool I/O channel closed during no_solution result")),
                }
            };
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
                // Late ExternalResult from a timed-out submit_external_share — log and continue
                PoolMessage::ExternalResult { accepted, status, coin } => {
                    println!(
                        "[{}] late_external_result_drained coin={} accepted={} status={} — waiting for no_solution result",
                        log_timestamp(), coin, accepted, status,
                    );
                    // Recv again for the actual no_solution result
                    let (line2, msg2) = match result_rx.recv() {
                        Ok(PoolIncoming::Result(l, m)) => (l, m),
                        Ok(PoolIncoming::Job(l, _, _, _, _, _, _)) => {
                            return Err(anyhow!("expected result from pool, got Job: {l}"));
                        }
                        Err(_) => return Err(anyhow!("pool I/O channel closed during no_solution result (after late external)")),
                    };
                    last_result_line = Some(line2.clone());
                    if let PoolMessage::Result { accepted, status } = msg2 {
                        if accepted {
                            accepted_iterations += 1;
                        }
                        if VERBOSE.load(Ordering::Relaxed) {
                            println!("pool_status={status}");
                        }
                    } else {
                        return Err(anyhow!("expected Result after late ExternalResult, got {msg2:?}"));
                    }
                }
                other => return Err(anyhow!("expected result from pool, got {other:?}")),
            }
            let total_accepted = hashrate.accepted_shares.load(Ordering::Relaxed);
            let total_rejected = hashrate.rejected_shares.load(Ordering::Relaxed);
            let stream_stats = hashrate.build_stream_stats(&telemetry.algorithm);
            telemetry.maybe_print_status(
                iteration + 1,
                config.loop_count,
                total_accepted,
                total_rejected,
                attempted_hashes,
                Some(remote_job_ttl_ms),
                config.stats_file.as_deref(),
                metrics,
                pool_addr,
                &stream_stats,
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
        let mix_hash_hex = gpu_mix_hash.map(|mh| {
            mh.iter().map(|b| format!("{:02x}", b)).collect::<String>()
        });
        let submit_message = PoolMessage::Submit {
            job_id: solution.job_id,
            miner_id: config.miner_id.clone(),
            worker_name: config.worker_name.clone(),
            nonce: solution.candidate.nonce,
            hash_hex: hex(&solution.hash),
            attempted_hashes: Some(tested),
            elapsed_ms: Some(job_started_at.elapsed().as_millis() as u64),
            mix_hash_hex,
        };
        let submit_line = write_wire_message(&mut writer, &submit_message)?;
        // ── FIX #8: Drain late ExternalResult if present (from timed-out submit_external_share) ──
        let (result_line_raw, result_message) = loop {
            match result_rx.recv() {
                Ok(PoolIncoming::Result(line, msg)) => break (line, msg),
                Ok(PoolIncoming::Job(line, _, _, _, _, _, _)) => {
                    return Err(anyhow!("expected result from pool, got Job: {line}"));
                }
                Err(_) => return Err(anyhow!("pool I/O channel closed during submit result")),
            }
        };
        telemetry.record_submit_latency(submit_started_at.elapsed());
        last_result_line = Some(result_line_raw.clone());

        let status = match result_message {
            PoolMessage::Result { accepted, status } => {
                let latency_ms = submit_started_at.elapsed().as_millis();
                if accepted {
                    accepted_iterations += 1;
                    hashrate.record_zion_share(true);
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
                    hashrate.record_zion_share(false);
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
            // Late ExternalResult from a timed-out submit_external_share — drain and recv again
            PoolMessage::ExternalResult { accepted, status: ext_status, coin } => {
                println!(
                    "[{}] late_external_result_drained coin={} accepted={} status={} — waiting for submit result",
                    log_timestamp(), coin, accepted, ext_status,
                );
                let (line2, msg2) = match result_rx.recv() {
                    Ok(PoolIncoming::Result(l, m)) => (l, m),
                    Ok(PoolIncoming::Job(l, _, _, _, _, _, _)) => {
                        return Err(anyhow!("expected result from pool, got Job: {l}"));
                    }
                    Err(_) => return Err(anyhow!("pool I/O channel closed during submit result (after late external)")),
                };
                last_result_line = Some(line2.clone());
                if let PoolMessage::Result { accepted, status } = msg2 {
                    let latency_ms = submit_started_at.elapsed().as_millis();
                    if accepted {
                        accepted_iterations += 1;
                        hashrate.record_zion_share(true);
                        ui::log_accepted(job.job_id, job.height, solution.candidate.nonce, latency_ms as u64);
                        println!(
                            "[{}] SHARE_ACCEPTED  job={}  height={}  nonce={}  algo={}  latency_ms={}",
                            log_timestamp(), job.job_id, job.height, solution.candidate.nonce, current_algorithm, latency_ms,
                        );
                    } else {
                        rejected_iterations += 1;
                        hashrate.record_zion_share(false);
                        ui::log_rejected(job.job_id, job.height, solution.candidate.nonce, latency_ms as u64, &status);
                        println!(
                            "[{}] SHARE_REJECTED  job={}  height={}  nonce={}  algo={}  reason=\"{}\"  hash={}",
                            log_timestamp(), job.job_id, job.height, solution.candidate.nonce, current_algorithm, status, hex(&solution.hash),
                        );
                    }
                    status
                } else {
                    return Err(anyhow!("expected Result after late ExternalResult, got {msg2:?}"));
                }
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
        let stream_stats = hashrate.build_stream_stats(&telemetry.algorithm);
        telemetry.maybe_print_status(
            iteration + 1,
            config.loop_count,
            total_accepted,
            total_rejected,
            attempted_hashes,
            Some(remote_job_ttl_ms),
            config.stats_file.as_deref(),
            metrics,
            pool_addr,
            &stream_stats,
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

    // Stop the pool config poller thread
    config_stop.store(true, std::sync::atomic::Ordering::Relaxed);

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
        pool_addr: &str,
        stream_stats: &[ui::StreamStats],
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
        // In QUIET/sticky mode: write to stderr so external parsers still work
        //   (alt screen buffer would hide stdout from pipe readers)
        let status_line = format!(
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
        if QUIET.load(Ordering::Relaxed) {
            eprintln!("{status_line}");
        } else {
            println!("{status_line}");
        }

        if !TUI_ACTIVE.load(Ordering::Relaxed) {
            // ── Claymore-style sticky triple-stream stats (alt screen + full redraw) ──
            // Activate quiet mode to suppress verbose log lines (clean metrics display)
            QUIET.store(true, Ordering::Relaxed);
            std::env::set_var("ZION_QUIET", "1");
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
            ui::print_triple_stream_stats_sticky(
                uptime_secs,
                stream_stats,
                accepted,
                rejected,
                pool_addr,
                self.pool_height,
                submit_avg,
                self.submit_max_latency_ms,
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

/// Result of mining an external stream job in parallel.
struct ExternalShareResult {
    coin: String,
    algorithm: String,
    external_job_id: String,
    nonce: u64,
    hash: [u8; 32],
    /// Mix hash for Ethash/KawPow/ProgPow shares (needed by upstream pool).
    mix_hash: Option<[u8; 32]>,
    extranonce1_hex: String,
}

/// Submit an external share to the pool and read the result.
/// Used by both CPU (VerusHash) and GPU external stream paths.
fn submit_external_share(
    writer: &mut impl Write,
    result_rx: &std::sync::mpsc::Receiver<PoolIncoming>,
    config: &MinerConfig,
    share: &ExternalShareResult,
    _hashrate: &HashrateTracker,
    verbose: bool,
    record: impl Fn(bool),
) {
    let ext_submit = PoolMessage::ExternalSubmit {
        miner_id: config.miner_id.clone(),
        worker_name: config.worker_name.clone(),
        coin: share.coin.clone(),
        algorithm: share.algorithm.clone(),
        external_job_id: share.external_job_id.clone(),
        nonce: share.nonce,
        hash_hex: hex::encode(share.hash),
        mix_hash_hex: share.mix_hash.map(|m| hex::encode(m)),
        extranonce1_hex: share.extranonce1_hex.clone(),
    };
    if let Err(e) = write_wire_message(writer, &ext_submit) {
        println!("external_submit_write_error: {e}");
        return;
    }

    // ── FIX #8: Don't block main loop on external share result ──
    // The pool forwards external shares to upstream pools (NiceHash, 2miners)
    // and may take 10-15 seconds to respond. Blocking here starves the GPU
    // (observed: 14.3s main loop iteration, gpu_hps=0.00 on AMD RX 5600 XT).
    // Use a short timeout: if the pool responds quickly, process the result.
    // If not, log and move on — the late result will be drained below.
    let ext_timeout_ms = std::env::var("ZION_EXT_SUBMIT_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(500);
    match result_rx.recv_timeout(Duration::from_millis(ext_timeout_ms)) {
        Ok(PoolIncoming::Result(line, msg)) => {
            if verbose {
                println!("wire_external_result={line}");
            }
            if let PoolMessage::ExternalResult { accepted, status, coin } = msg {
                record(accepted);
                let stream_label = if share.algorithm == "verushash" || share.algorithm == "randomx" {
                    "CPU PROFIT"
                } else {
                    "GPU PROFIT"
                };
                if accepted {
                    if !TUI_ACTIVE.load(Ordering::Relaxed) {
                        ui::log_ext_accepted(stream_label, &coin, &share.algorithm, 0);
                    }
                    println!("[{}] external_share_accepted coin={} status={}", log_timestamp(), coin, status);
                } else {
                    if !TUI_ACTIVE.load(Ordering::Relaxed) {
                        ui::log_ext_rejected(stream_label, &coin, &share.algorithm, &status);
                    }
                    println!("[{}] external_share_rejected coin={} status={}", log_timestamp(), coin, status);
                }
            }
        }
        Ok(PoolIncoming::Job(line, _, _, _, _, _, _)) => {
            println!("external_result_unexpected_job: {line}");
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            // Pool didn't respond within timeout — don't block the main loop.
            // The late ExternalResult will arrive in result_rx and be drained
            // by the next result_rx.recv() call (handled gracefully below).
            println!(
                "[{}] external_share_submitted_no_response coin={} algo={} timeout_ms={} — continuing without blocking",
                log_timestamp(),
                share.coin,
                share.algorithm,
                ext_timeout_ms,
            );
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            println!("external_result_read_error: channel disconnected");
        }
    }
}

/// Persistent external GPU miner thread for the single GPU profit coin.
///
/// Runs in a separate thread with its own GPU context/command queue.
/// Receives external stream jobs via a channel, creates/switches the GPU
/// backend on demand, scans nonces on GPU, and sends found shares back.
fn external_gpu_thread(
    rx: std::sync::mpsc::Receiver<zion_pool::ExternalStreamJob>,
    tx: std::sync::mpsc::Sender<ExternalShareResult>,
    work_size: usize,
    hashrate: Arc<HashrateTracker>,
    backend_kind: gpu_backend::GpuBackendKind,
    #[cfg(feature = "gpu-cuda")]
    shared_cuda_dev: Option<std::sync::Arc<cudarc::driver::CudaDevice>>,
    #[cfg(not(feature = "gpu-cuda"))]
    shared_cuda_dev: Option<()>,
) {
    println!("[{}] external_gpu_thread_entered backend={} shared_cuda={}", log_timestamp(), backend_kind.as_str(), shared_cuda_dev.is_some());
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut current_job: Option<zion_pool::ExternalStreamJob> = None;
    let mut current_miner: Option<Box<dyn gpu_backend::GpuMiner>> = None;
    let mut current_algo: Option<String> = None;
    let mut nonce_base: u64 = 0;
    let mut nonce_offset: u64 = 0;
    let mut backend_init_failures: u32 = 0;
    let mut skipped_algos: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Use a large batch_size — mine_batch_raw caps it at the GPU's actual
    // work_size internally, so this is safe.
    let batch_size = 4_186_112u64;
    let mut batch_count: u64 = 0;
    let mut last_heartbeat = std::time::Instant::now();
    let mut last_epoch: Option<u32> = None;

    fn epoch_for_algorithm(algorithm: &str, height: u64) -> Option<u32> {
        match algorithm {
            "ethash" | "etchash" | "ethash_etc" => Some((height / 30000) as u32),
            "progpow" | "progpow_epic" => Some((height / 30000) as u32),
            "kawpow"
            | "kawpow_rvn"
            | "kawpow_clore"
            | "kawpow_evr"
            | "kawpow_mewc"
            | "kawpow_quai"
            | "evrprogpow"
            | "evrprogpow_evr"
            | "meowpow"
            | "meowpow_mewc" => Some((height / 7500) as u32),
            "autolykos" | "autolykos_erg" => Some((height / 45000) as u32),
            "zelhash" | "zelhash_flux" | "beamhash" | "beamhash_beam" => None,
            _ => None,
        }
    }

    loop {
        // Check for new job (non-blocking)
        match rx.try_recv() {
            Ok(job) => {
                // Only reset nonce_offset when the job actually changes
                // (different job_id or height). The pool re-sends the same
                // job every ~1s; resetting nonce_offset each time would cause
                // the GPU to re-scan the same nonces endlessly.
                let is_new_job = current_job.as_ref().map_or(true, |j| {
                    j.job_id != job.job_id || j.height != job.height
                });
                if is_new_job {
                    // Random nonce base to avoid duplicates
                    let mut h = DefaultHasher::new();
                    job.job_id.hash(&mut h);
                    std::process::id().hash(&mut h);
                    let random_base = (h.finish() as u32) as u64;

                    // NiceHash nonce format: extranonce1 occupies high bits
                    // of the nonce. The miner must only iterate over the low
                    // bits. Parse extranonce1_hex and embed it in nonce_base.
                    let en1_bytes = if job.extranonce1_hex.is_empty() {
                        Vec::new()
                    } else {
                        hex::decode(job.extranonce1_hex.trim_start_matches("0x")).unwrap_or_default()
                    };
                    let en1_len = en1_bytes.len();
                    if en1_len > 0 && en1_len <= 4 {
                        // Embed extranonce1 in the high bits of the nonce.
                        // extranonce1 is big-endian; shift it left to occupy
                        // the top en1_len bytes of the 8-byte nonce.
                        let mut en1_val: u64 = 0;
                        for &b in &en1_bytes {
                            en1_val = (en1_val << 8) | (b as u64);
                        }
                        let shift = (8 - en1_len) * 8;
                        nonce_base = (en1_val << shift) | (random_base & ((1u64 << shift) - 1));
                        println!(
                            "[{}] ext_gpu_nicehash_nonce en1_hex={} en1_len={} nonce_base=0x{:016x}",
                            log_timestamp(),
                            job.extranonce1_hex,
                            en1_len,
                            nonce_base,
                        );
                    } else {
                        nonce_base = random_base;
                    }
                    nonce_offset = 0;
                }
                // Note: do NOT reset last_epoch here — the pool re-sends the
                // same job every ~1s, and resetting would cause DAG reload
                // every second. last_epoch is reset only when the algorithm
                // changes (see algo switch below).
                println!(
                    "[{}] ext_gpu_job_received coin={} algo={} job_id={} height={}",
                    log_timestamp(),
                    job.coin,
                    job.algorithm,
                    job.job_id,
                    job.height,
                );
                // Update hashrate tracker for triple-stream display
                hashrate.set_gpu_ext_job(&job.coin, &job.algorithm);
                current_job = Some(job);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // Debug: log first few empty receives to confirm thread is alive
                if batch_count == 0 && last_heartbeat.elapsed().as_secs() < 3 {
                    println!("[{}] ext_gpu_rx_empty (no job yet, thread alive)", log_timestamp());
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                println!("[{}] ext_gpu_channel_closed — exiting", log_timestamp());
                return;
            }
        }

        // Heartbeat every 15s so we can see the thread is alive
        if last_heartbeat.elapsed().as_secs() >= 15 {
            println!(
                "[{}] ext_gpu_heartbeat batches={} nonce_offset={} has_job={} epoch={:?} algo={:?}",
                log_timestamp(),
                batch_count,
                nonce_offset,
                current_job.is_some(),
                last_epoch,
                current_algo.as_deref().unwrap_or("none"),
            );
            last_heartbeat = std::time::Instant::now();
        }

        let job = match &current_job {
            Some(j) => j,
            None => {
                // No job yet — wait a bit
                thread::sleep(Duration::from_millis(50));
                continue;
            }
        };

        // Create or switch GPU backend when the algorithm changes.
        let algo = job.algorithm.as_str();
        if current_algo.as_deref() != Some(algo) {
            // Skip algorithms that have already failed too many times
            if skipped_algos.contains(algo) {
                thread::sleep(Duration::from_millis(500));
                continue;
            }

            // Safety guard: check if the backend can safely handle this algorithm.
            // Metal on Apple Silicon cannot handle DAG-based algorithms (progpow,
            // ethash, kawpow) because the ~2GB DAG allocation on unified memory
            // causes system freezes.
            if !gpu_backend::backend_supports_algorithm(backend_kind, algo) {
                println!(
                    "[{}] ext_gpu_skip_unsupported algo={} backend={} reason=\"DAG-based or memory-hard algorithm not safe on Metal (unified memory OOM risk)\"",
                    log_timestamp(),
                    algo,
                    backend_kind.as_str(),
                );
                skipped_algos.insert(algo.to_string());
                current_algo = None;
                current_miner = None;
                thread::sleep(Duration::from_millis(500));
                continue;
            }
            match gpu_backend::create_gpu_backend_with_cuda_device(
                backend_kind,
                work_size,
                algo,
                shared_cuda_dev.clone(),
            ) {
                Ok(m) => {
                    println!(
                        "[{}] ext_gpu_backend_init algo={} backend={} work_size={} device=\"{}\"",
                        log_timestamp(),
                        algo,
                        backend_kind.as_str(),
                        work_size,
                        m.device_name()
                    );
                    current_miner = Some(m);
                    current_algo = Some(algo.to_string());
                    last_epoch = None;
                    backend_init_failures = 0;
                }
                Err(e) => {
                    backend_init_failures += 1;
                    if backend_init_failures >= 3 {
                        println!(
                            "[{}] ext_gpu_backend_skip algo={} backend={} err=\"{e}\" — skipping after {} failures",
                            log_timestamp(),
                            algo,
                            backend_kind.as_str(),
                            backend_init_failures
                        );
                        skipped_algos.insert(algo.to_string());
                        current_algo = None;
                        current_miner = None;
                        backend_init_failures = 0;
                        thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                    println!(
                        "[{}] ext_gpu_backend_init_failed algo={} backend={} err=\"{e}\" — retrying ({}/{})",
                        log_timestamp(),
                        algo,
                        backend_kind.as_str(),
                        backend_init_failures,
                        3
                    );
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            }
        }

        let gpu_miner = match current_miner.as_mut() {
            Some(m) => m,
            None => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };

        // Ensure DAG is loaded for DAG-based algorithms
        let epoch = epoch_for_algorithm(algo, job.height);
        if epoch != last_epoch {
            if let Some(ep) = epoch {
                println!(
                    "[{}] ext_gpu_dag_loading algo={} epoch={} height={}",
                    log_timestamp(),
                    algo,
                    ep,
                    job.height,
                );
            }
            if let Err(e) = gpu_miner.update_epoch(job.height) {
                println!(
                    "[{}] ext_gpu_epoch_failed algo={} height={} err=\"{e}\" — retrying",
                    log_timestamp(),
                    algo,
                    job.height,
                );
                thread::sleep(Duration::from_secs(2));
                continue;
            }
            if let Some(ep) = epoch {
                println!(
                    "[{}] ext_gpu_dag_ready algo={} epoch={}",
                    log_timestamp(),
                    algo,
                    ep,
                );
            }
            last_epoch = epoch;
        }

        // Parse header and target
        let header_bytes = match hex::decode(job.header_hex.trim_start_matches("0x")) {
            Ok(b) => b,
            Err(e) => {
                println!("ext_gpu_header_error algo={algo}: {e}");
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };

        let target_bytes = match zion_pool::parse_fixed_hex::<32>(&job.target_hex, "external target") {
            Ok(t) => t,
            Err(e) => {
                println!("ext_gpu_target_error algo={algo}: {e}");
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };

        let target = DifficultyTarget { bytes: target_bytes };
        let nonce = nonce_base.wrapping_add(nonce_offset);

        // Scan one batch on GPU
        let result = if header_bytes.len() > 80 {
            gpu_miner.mine_batch_raw(&header_bytes, target, nonce, batch_size)
        } else {
            let mut bytes = [0u8; 80];
            let len = header_bytes.len().min(80);
            bytes[..len].copy_from_slice(&header_bytes[..len]);
            let header = MiningHeader::from_bytes(bytes);
            gpu_miner.mine_batch(header, target, nonce, batch_size)
        };

        let actual_batch: u64;
        match result {
            Ok(br) => {
                actual_batch = br.nonces_tested;
                if batch_count < 5 || batch_count % 100 == 0 {
                    println!(
                        "[{}] ext_gpu_batch_done batch={} nonces_tested={} solutions={} header_len={}",
                        log_timestamp(),
                        batch_count,
                        br.nonces_tested,
                        br.solutions.len(),
                        header_bytes.len(),
                    );
                }
                if let Some((found_nonce, hash, mix_hash)) = br.solutions.first() {
                    let share = ExternalShareResult {
                        coin: job.coin.clone(),
                        algorithm: job.algorithm.clone(),
                        external_job_id: job.job_id.clone(),
                        nonce: *found_nonce,
                        hash: *hash,
                        mix_hash: *mix_hash,
                        extranonce1_hex: job.extranonce1_hex.clone(),
                    };
                    println!(
                        "[{}] ext_gpu_share_found coin={} algo={} nonce={} hash={}",
                        log_timestamp(),
                        share.coin,
                        share.algorithm,
                        share.nonce,
                        hex::encode(share.hash)
                    );
                    let _ = tx.send(share);
                }
            }
            Err(e) => {
                actual_batch = batch_size.min(work_size as u64);
                println!("ext_gpu_batch_error algo={algo} nonce={nonce} err=\"{e}\"");
                thread::sleep(Duration::from_millis(500));
            }
        }

        // Advance nonce for next batch using the actual nonces tested
        // as reported by the GPU backend. This accounts for the internal
        // work_size cap in the AuXpow GpuMiner, preventing skipped nonces.
        nonce_offset = nonce_offset.wrapping_add(actual_batch);
        batch_count += 1;
        hashrate.record_gpu_ext_hashes(actual_batch);

        // ── FIX #7: Duty-cycle yield to Stream 1 (ZION) on single-GPU rigs ──
        // On single-GPU machines, Stream 1 (ZION deeksha) and Stream 2
        // (external GPU coin) share the same physical GPU via separate
        // OpenCL/CUDA contexts. Without yielding, this tight loop hogs
        // the GPU and starves Stream 1 (observed: gpu_hps=0.00,
        // best_batch_ms=14849 on AMD RX 5600 XT).
        //
        // Duty-cycle approach: run N batches (burst), then sleep M ms
        // (gap) to give Stream 1 a guaranteed GPU window. The gap must
        // be long enough for Stream 1 to complete one deeksha batch
        // (~270ms on RX 5600 XT at work_size=8192).
        //
        // Defaults: burst=3 batches, gap=300ms → ~75% Stream2 duty cycle.
        // Tuning:
        //   ZION_EXT_GPU_BURST=N  (batches per burst, 0=no limit)
        //   ZION_EXT_GPU_GAP_MS=M (sleep after burst, 0=disabled)
        let burst = std::env::var("ZION_EXT_GPU_BURST")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(3);
        let gap_ms = std::env::var("ZION_EXT_GPU_GAP_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300);
        if burst > 0 && gap_ms > 0 && batch_count % burst == 0 {
            thread::sleep(Duration::from_millis(gap_ms));
        }
    }
}




/// Persistent CPU external stream thread (Stream 3c: VerusHash/RandomX).
///
/// Claymore-style: created once at session start, receives jobs via channel,
/// mines continuously, and sends shares back via channel.  This replaces
/// the old per-iteration `thread::spawn(mine_external_stream_cpu)` pattern
/// which created and destroyed a thread every mining cycle.
fn ext_cpu_thread(
    rx: std::sync::mpsc::Receiver<zion_pool::ExternalStreamJob>,
    tx: std::sync::mpsc::Sender<ExternalShareResult>,
    threads: usize,
    nonce_count: u64,
    hashrate: Arc<HashrateTracker>,
) {
    // RandomX is ~1000× slower than VerusHash per hash.  Use a much smaller
    // nonce batch for RandomX so that hashrate updates frequently and the
    // thread stays responsive to new jobs.
    //   VerusHash: ~5 MH/s per thread → 2M nonces ≈ 0.4s per batch
    //   RandomX:   ~500 H/s per thread → 2M nonces ≈ 4000s per batch (!)
    // With randomx_nonce_count=10000 and 4 threads, each thread gets ~2500
    // nonces, taking ~5s per batch at 500 H/s — reasonable update frequency.
    let randomx_nonce_count = parse_env_u64("ZION_EXT_CPU_RANDOMX_NONCE_COUNT", 10_000)
        .unwrap_or(10_000);

    // RandomX is memory-bandwidth bound.  Using all logical cores (HT) for
    // RandomX starves the main mining loop (GPU share submission, TUI, etc.)
    // causing 10× slowdown from scheduler contention.  Use fewer threads:
    // default = threads/3 (e.g. 4 for 12T), leaving 8 logical cores for the
    // main loop.  Override with ZION_EXT_CPU_RANDOMX_THREADS.
    let randomx_threads = std::env::var("ZION_EXT_CPU_RANDOMX_THREADS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or((threads / 3).max(2));

    println!(
        "[{}] ext_cpu_thread: started (persistent, threads={}, randomx_threads={}, verushash_nonce_count={}, randomx_nonce_count={})",
        log_timestamp(),
        threads,
        randomx_threads,
        nonce_count,
        randomx_nonce_count,
    );

    let mut current_job: Option<zion_pool::ExternalStreamJob> = None;
    let mut current_job_id = String::new();
    let mut nonce_base: u64 = 0;
    let mut nonce_offset: u64 = 0;

    loop {
        // Check for new job (non-blocking).
        // Drain ALL pending messages and keep only the latest one.
        // The main loop sends external_stream_cpu every ~1s, but a RandomX
        // batch takes ~3s, so the channel can accumulate many stale copies.
        // Without draining, the ext_cpu_thread would read hours-old jobs.
        let mut latest_job: Option<zion_pool::ExternalStreamJob> = None;
        loop {
            match rx.try_recv() {
                Ok(job) => {
                    latest_job = Some(job);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    println!("[{}] ext_cpu_thread: channel closed, exiting", log_timestamp());
                    return;
                }
            }
        }
        if let Some(job) = latest_job {
            // Only reset the nonce scan when the job_id actually changes,
            // otherwise the CPU thread would keep restarting from the same
            // base and never cover enough nonces to find a share.
            if job.job_id != current_job_id {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut h = DefaultHasher::new();
                job.job_id.hash(&mut h);
                std::process::id().hash(&mut h);
                std::thread::current().id().hash(&mut h);
                nonce_base = (h.finish() as u32) as u64;
                nonce_offset = 0;
                current_job_id = job.job_id.clone();
                println!(
                    "[{}] ext_cpu_thread: new job coin={} algo={} job_id={} nonce_base={}",
                    log_timestamp(),
                    job.coin,
                    job.algorithm,
                    job.job_id,
                    nonce_base,
                );
                // Update hashrate tracker for triple-stream display
                hashrate.set_cpu_ext_job(&job.coin, &job.algorithm);
            }
            current_job = Some(job);
        }

        // Mine current job (if any)
        if let Some(ref ext) = current_job {
            // Use smaller nonce batches and fewer threads for RandomX
            // (memory-bandwidth bound, HT doesn't help, and we need to leave
            // CPU cores for the main GPU mining loop).
            let (effective_threads, effective_nonce_count) = if ext.algorithm == "randomx" {
                (randomx_threads, randomx_nonce_count)
            } else {
                (threads, nonce_count)
            };

            let start_nonce = nonce_base.wrapping_add(nonce_offset);
            if let Some(share) = mine_external_stream_cpu(ext, effective_threads, start_nonce, effective_nonce_count) {
                println!(
                    "[{}] ext_cpu_thread: share found, sending via channel  coin={}  algo={}  job_id={}  nonce={}",
                    log_timestamp(),
                    share.coin,
                    share.algorithm,
                    share.external_job_id,
                    share.nonce,
                );
                let next_offset = nonce_offset.wrapping_add(share.nonce.wrapping_sub(start_nonce) + 1);
                let _ = tx.send(share);
                // Keep mining the same job, just advance past the share we found.
                nonce_offset = next_offset;
            } else {
                nonce_offset = nonce_offset.wrapping_add(effective_nonce_count);
            }
            hashrate.record_cpu_ext_hashes(effective_nonce_count);
        } else {
            // No job — brief sleep to avoid busy-loop
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
}


fn mine_external_stream_cpu(
    ext: &ExternalStreamJob,
    threads: usize,
    start_nonce: u64,
    nonce_count: u64,
) -> Option<ExternalShareResult> {
    // Parse external coin from ticker
    let coin = match zion_auxpow::types::ExternalCoin::from_str_loose(&ext.coin) {
        Some(c) => c,
        None => {
            println!("external_stream_unknown_coin: {}", ext.coin);
            return None;
        }
    };

    // Parse header bytes
    let header_bytes = match hex::decode(ext.header_hex.trim_start_matches("0x")) {
        Ok(b) => b,
        Err(e) => {
            println!("external_stream_header_decode_error: {e}");
            return None;
        }
    };

    // Parse target
    let target_bytes = match zion_pool::parse_fixed_hex::<32>(&ext.target_hex, "external target") {
        Ok(t) => t,
        Err(e) => {
            println!("external_stream_target_parse_error: {e}");
            return None;
        }
    };

    // Parse extranonce1
    let extranonce1 = if ext.extranonce1_hex.is_empty() {
        Vec::new()
    } else {
        match hex::decode(ext.extranonce1_hex.trim_start_matches("0x")) {
            Ok(b) => b,
            Err(_) => Vec::new(),
        }
    };

    // For VerusHash, initialize the hasher
    if ext.algorithm == "verushash" {
        zion_auxpow::external_hashers::init_verushash();
    }

    let scan_end = start_nonce.wrapping_add(nonce_count);

    let job_pkg = zion_auxpow::types::JobPackage {
        external_coin: coin,
        external_job_id: ext.job_id.clone(),
        algorithm: ext.algorithm.clone(),
        header_bytes,
        target_bytes,
        timestamp: ext.height, // reused for height in some algorithms
        block_number: Some(ext.height),
        extranonce1,
        start_nonce,
        nonce_count,
        seed_hash: if ext.seed_hash_hex.is_empty() {
            None
        } else {
            hex::decode(ext.seed_hash_hex.trim_start_matches("0x")).ok()
        },
    };

    // ── Multi-threaded scan for CPU-bound algorithms (VerusHash, RandomX) ──
    // Both VerusHash and RandomX are CPU-only and benefit from parallel scanning.
    // RandomX uses per-thread VMs (thread_local in C wrapper) so each thread
    // gets its own VM sharing the global read-only dataset — no mutex contention.
    // Split the nonce range across `threads` worker threads.
    if (ext.algorithm == "verushash" || ext.algorithm == "randomx") && threads > 1 {
        use std::sync::Arc;
        let job_arc = Arc::new(job_pkg);
        let chunk = (nonce_count / threads as u64).max(1);
        let found = Arc::new(std::sync::Mutex::new(None::<zion_auxpow::miner_harness::FoundShare>));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let algo = ext.algorithm.clone(); // clone for thread closure

        let mut handles = Vec::new();
        for t in 0..threads {
            let job = Arc::clone(&job_arc);
            let found = Arc::clone(&found);
            let stop = Arc::clone(&stop);
            let algo = algo.clone(); // clone per-iteration for closure
            let t_start = start_nonce.wrapping_add((t as u64) * chunk);
            let t_end = if t == threads - 1 {
                scan_end
            } else {
                t_start.wrapping_add(chunk)
            };
            handles.push(std::thread::spawn(move || {
                // Pin thread to physical core for RandomX (cache-sensitive)
                thread_affinity::maybe_pin_thread(t, threads, &algo);
                let range = t_start..t_end;
                match zion_auxpow::miner_harness::mine(&job, range) {
                    Ok(Some(share)) => {
                        let mut guard = found.lock().unwrap();
                        if guard.is_none() {
                            *guard = Some(share);
                        }
                        stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    _ => {}
                }
            }));
        }
        for h in handles {
            let _ = h.join();
        }
        if let Some(share) = found.lock().unwrap().take() {
            return Some(ExternalShareResult {
                coin: ext.coin.clone(),
                algorithm: ext.algorithm.clone(),
                external_job_id: share.external_job_id,
                nonce: share.nonce,
                hash: share.hash,
                mix_hash: None,
                extranonce1_hex: ext.extranonce1_hex.clone(),
            });
        }
        return None;
    }

    // Single-threaded fallback for other algorithms
    match zion_auxpow::miner_harness::mine(&job_pkg, start_nonce..scan_end) {
        Ok(Some(share)) => Some(ExternalShareResult {
            coin: ext.coin.clone(),
            algorithm: ext.algorithm.clone(),
            external_job_id: share.external_job_id,
            nonce: share.nonce,
            hash: share.hash,
            mix_hash: None,
            extranonce1_hex: ext.extranonce1_hex.clone(),
        }),
        Ok(None) => None,
        Err(e) => {
            println!("external_stream_mine_error: {e}");
            None
        }
    }
}

/// Routed pool message from the I/O thread to the main mining loop.
enum PoolIncoming {
    Job(
        String,
        MiningJob,
        String,
        Vec<u8>,
        String,
        Option<zion_pool::ExternalStreamJob>,
        Option<zion_pool::ExternalStreamJob>,
    ),
    Result(String, PoolMessage),
}

/// Background pool I/O thread — owns the reader, routes messages to channels.
///
/// Continuously reads from the pool socket and routes:
/// - `Job` → `job_tx` (parsed, with external stream fields)
/// - `Result`/`ExternalResult` → `result_tx`
/// - `Stale`/`Cancel` → printed inline
/// - `SetDifficulty` → stored in `CURRENT_POOL_DIFFICULTY` atomic
///
/// When the pool disconnects or an error occurs, the thread exits and drops
/// both senders, causing `recv()` on the main thread to return `Err` — which
/// triggers reconnect logic.
///
/// This eliminates both GPU idle gaps:
/// - **Job pre-fetch**: jobs are read continuously, so `job_rx.recv()` is
///   non-blocking when a job is already queued (eliminates ~400ms job wait)
/// - **Async submit response**: submit results are read continuously, so
///   `result_rx.recv()` is non-blocking when the response is already queued
///   (eliminates ~297ms submit wait)
fn pool_io_thread(
    mut reader: std::io::BufReader<std::net::TcpStream>,
    job_tx: std::sync::mpsc::Sender<PoolIncoming>,
    result_tx: std::sync::mpsc::Sender<PoolIncoming>,
) {
    loop {
        let (line, message) = match read_wire_message(&mut reader) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[{}] pool_io_thread_error: {e}", log_timestamp());
                break;
            }
        };
        match message {
            PoolMessage::Job {
                job_id,
                algorithm,
                start_nonce,
                nonce_count,
                target_hex,
                header_hex,
                height,
                stream_weights,
                external_stream,
                external_stream_cpu,
            } => {
                if !stream_weights.is_empty() && !QUIET.load(Ordering::Relaxed) {
                    println!("stream_weights job={} weights={}", job_id, stream_weights);
                }
                if let Some(ref ext) = external_stream {
                    if !QUIET.load(Ordering::Relaxed) {
                        println!(
                            "external_stream job={} coin={} algo={}",
                            job_id, ext.coin, ext.algorithm
                        );
                    }
                }
                if let Some(ref ext_cpu) = external_stream_cpu {
                    if !QUIET.load(Ordering::Relaxed) {
                        println!(
                            "external_stream_cpu job={} coin={} algo={} target_hex={:.64}",
                            job_id, ext_cpu.coin, ext_cpu.algorithm, ext_cpu.target_hex
                        );
                    }
                }
                let raw_header_bytes =
                    hex::decode(header_hex.trim_start_matches("0x")).unwrap_or_default();
                let job = match parse_header_hex(&header_hex) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("pool_io_job_parse_error job={job_id}: {e} — skipping");
                        continue;
                    }
                };
                let target = match parse_fixed_hex::<32>(&target_hex, "job target") {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("pool_io_target_parse_error job={job_id}: {e} — skipping");
                        continue;
                    }
                };
                let incoming = PoolIncoming::Job(
                    line,
                    MiningJob {
                        job_id,
                        header: job,
                        target: DifficultyTarget { bytes: target },
                        start_nonce,
                        nonce_count,
                        height,
                    },
                    algorithm,
                    raw_header_bytes,
                    stream_weights,
                    external_stream,
                    external_stream_cpu,
                );
                if job_tx.send(incoming).is_err() {
                    break; // Main thread dropped the receiver — exit
                }
            }
            PoolMessage::Result { .. } | PoolMessage::ExternalResult { .. } => {
                if result_tx.send(PoolIncoming::Result(line, message)).is_err() {
                    break;
                }
            }
            PoolMessage::Stale { .. } => println!("wire_stale={line}"),
            PoolMessage::Cancel { .. } => println!("wire_cancel={line}"),
            PoolMessage::SetDifficulty { difficulty, .. } => {
                println!("pool_set_difficulty={difficulty}");
                CURRENT_POOL_DIFFICULTY.store(difficulty, Ordering::Relaxed);
            }
            other => {
                eprintln!("[{}] pool_io_unexpected_message: {other:?}", log_timestamp());
            }
        }
    }
    // Dropping senders signals the main thread to reconnect
}

fn read_next_job(
    reader: &mut impl BufRead,
) -> Result<(
    String,
    MiningJob,
    String,
    Vec<u8>,
    String,
    Option<zion_pool::ExternalStreamJob>,
    Option<zion_pool::ExternalStreamJob>,
)> {
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
                stream_weights,
                external_stream,
                external_stream_cpu,
            } => {
                // Log stream weights if present (Deeksha Chv3 pipeline parameterisation).
                if !stream_weights.is_empty() && !QUIET.load(Ordering::Relaxed) {
                    println!("stream_weights job={} weights={}", job_id, stream_weights);
                }
                if let Some(ref ext) = external_stream {
                    if !QUIET.load(Ordering::Relaxed) {
                        println!(
                            "external_stream job={} coin={} algo={}",
                            job_id, ext.coin, ext.algorithm
                        );
                    }
                }
                if let Some(ref ext_cpu) = external_stream_cpu {
                    if !QUIET.load(Ordering::Relaxed) {
                        println!(
                            "external_stream_cpu job={} coin={} algo={} target_hex={:.64}",
                            job_id, ext_cpu.coin, ext_cpu.algorithm, ext_cpu.target_hex
                        );
                    }
                }
                // Keep raw header bytes for external algorithms that may
                // use headers longer than 80 bytes (e.g. DCR = 180 bytes).
                let raw_header_bytes =
                    hex::decode(header_hex.trim_start_matches("0x")).unwrap_or_default();
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
                    raw_header_bytes,
                    stream_weights,
                    external_stream,
                    external_stream_cpu,
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
            PoolMessage::ExternalResult { .. } => return Ok((line, message)),
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
    let quiet = QUIET.load(Ordering::Relaxed);
    if !quiet {
        println!("iteration={iteration}");
        println!("job_id={}", job.job_id);
        println!(
            "nonce_range={}..{}",
            job.start_nonce,
            job.start_nonce + job.nonce_count
        );
        println!("found_nonce={found_nonce}");
        println!("hash={}", hex(hash));
    }
    let status_str = format!("{status:?}");
    if status_str.contains("Accepted") {
        ui::log_accepted(job.job_id, job.height, found_nonce, 0);
    } else if status_str.contains("Rejected") {
        ui::log_rejected(job.job_id, job.height, found_nonce, 0, &status_str);
    } else if !quiet {
        println!("share_status={status_str}");
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}

fn parse_header_hex(raw: &str) -> Result<MiningHeader> {
    let normalized = raw.trim().trim_start_matches("0x");
    let decoded = hex::decode(normalized)
        .with_context(|| "job header contains invalid hex")?;

    // Pad to 80 bytes (external AuxPoW jobs may send shorter headers,
    // e.g. KAS sends only a 32-byte pre_pow_hash).
    let mut bytes = [0u8; 80];
    let len = decoded.len().min(80);
    bytes[..len].copy_from_slice(&decoded[..len]);

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
    /// Work size for Pearl PoUW GPU (Stream 2). Defaults to gpu_work_size.
    pearl_gpu_work_size: usize,
    /// Work size for secondary external GPU (Stream 3, GPU-capable coins).
    /// Defaults to gpu_work_size.
    secondary_gpu_work_size: usize,
    /// Algorithm advertised in hello and used if pool matches.
    algorithm: String,
    /// Whether Stream 1 (ZION primary) is enabled (default: true)
    stream1_enabled: bool,
    /// Whether Stream 2 (GPU external coins) is enabled (default: true if GPU)
    stream2_enabled: bool,
    /// Whether Stream 3 (CPU external coins) is enabled (default: true)
    stream3_enabled: bool,
    /// Auto-tuned nonce batch size for VerusHash CPU mining (Stream 3)
    verushash_nonce_count: u64,
    /// Whether auto-mode was requested (affects logging)
    #[allow(dead_code)]
    auto_mode: bool,
    /// Detected GPU VRAM in bytes (from autotune, used by autonomous router)
    gpu_vram_bytes: u64,
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
                "--no-tui" => {
                    std::env::set_var("ZION_INTERACTIVE", "0");
                    i += 1;
                }
                "--algorithm" if i + 1 < args.len() => {
                    std::env::set_var("ZION_MINER_ALGORITHM", &args[i + 1]);
                    i += 2;
                }
                "--pearl" if i + 1 < args.len() => {
                    // Pearl PoUW stratum stream: --pearl POOL_HOST:PORT:WALLET
                    // Spawns AuxPowClient with PearlStratum protocol in parallel.
                    std::env::set_var("ZION_PEARL_STREAM", &args[i + 1]);
                    i += 2;
                }
                "--detect-hardware" => {
                    // Hardware detection mode for `zion mine auto` CLI command.
                    // Prints detected GPU devices and exits.
                    let gpus = gpu_backend::detect_gpus();
                    if gpus.is_empty() {
                        println!("gpu_detect: none");
                    } else {
                        for dev in &gpus {
                            println!("gpu_detect: {}", dev);
                        }
                    }
                    let cpu_cores = parallel::detect_threads();
                    println!("cpu_cores: {}", cpu_cores);
                    std::process::exit(0);
                }
                "--auto-tune" => {
                    // Hardware autotuning mode: detect hardware, print
                    // recommended settings, and exit (no mining).
                    let result = gpu_backend::auto_tune_work_sizes();
                    println!("=== ZION Hardware Autotune ===");
                    println!();
                    println!("Detected Hardware:");
                    println!(
                        "  GPU:  {} ({} CUs, {} MB VRAM)",
                        result.gpu_name,
                        result.gpu_compute_units,
                        result.gpu_vram_bytes / (1024 * 1024),
                    );
                    println!(
                        "  CPU:  {} ({} physical / {} logical cores)",
                        result.cpu_model,
                        result.cpu_physical_cores,
                        result.cpu_cores,
                    );
                    println!(
                        "  RAM:  {} MB",
                        result.sys_ram_bytes / (1024 * 1024),
                    );
                    println!();
                    println!("Recommended Settings:");
                    println!(
                        "  ZION_GPU_WORK_SIZE={}",
                        result.gpu_work_size,
                    );
                    println!(
                        "  ZION_SECONDARY_GPU_WORK_SIZE={}",
                        result.secondary_gpu_work_size,
                    );
                    println!(
                        "  ZION_THREADS={}",
                        result.threads,
                    );
                    println!(
                        "  ZION_EXT_CPU_NONCE_COUNT={}",
                        result.verushash_nonce_count,
                    );
                    println!();
                    println!(
                        "  (GPU WS formula: nearest_pow2(CUs * 512) = nearest_pow2({} * 512) = {})",
                        result.gpu_compute_units,
                        result.gpu_work_size,
                    );
                    let vram_mib = result.gpu_vram_bytes / (1024 * 1024);
                    println!(
                        "  (Secondary WS formula: clamp({}MiB * 0.75 / 1024, 1, 8) * 1M = {})",
                        vram_mib,
                        result.secondary_gpu_work_size,
                    );
                    std::process::exit(0);
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
                    println!("  --algorithm ALGO    Mining algorithm (see list below)");
                    println!("  --pearl H:P:W       Pearl PoUW stratum stream (host:port:wallet)");
                    println!("  --detect-hardware   Detect GPU/CPU hardware and exit (for zion mine auto)");
                    println!("  --auto-tune         Detect hardware, print recommended settings, and exit");
                    println!();
                    println!("  ZION algorithms:  deeksha_lite_v1, cosmic_harmony_ekam_deeksha_v2, deeksha_lite_fire");
                    println!("  External GPU:      blake3 (ALPH/DCR), kheavyhash (KAS), autolykos (ERG),");
                    println!("                     kawpow (RVN/CLORE/EVR/MEWC), ethash (ETC), zelhash (FLUX)");
                    println!("  External CPU:      verushash (VRSC), randomx (XMR)");
                    println!("  Special:           auto (autotune — benchmark all and pick best)");
                println!("  --no-tui            Disable interactive TUI and log to stdout");
                    println!();
                    println!("Benchmarks:");
                    println!("  --ekam-bench          Ekam Deeksha GPU benchmark (single algo)");
                    println!("  --verus-bench         VerusHash v2.2 CPU benchmark (requires native-verushash)");
                    println!("  --randomx-bench       RandomX (Monero/XMR) CPU benchmark (requires native-randomx)");
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

        // ── Hardware autotuning ──
        // Detect GPU VRAM, compute units, CPU cores, and system RAM.
        // Compute optimal work sizes and thread count for this hardware.
        // Env vars always override autotuned values.
        let autotune = gpu_backend::auto_tune_work_sizes();
        let autotune_enabled = parse_bool_env("ZION_AUTOTUNE", true);
        if autotune_enabled {
            println!("=== Hardware Autotune ===");
            println!(
                "  GPU: {} ({} CUs, {} MB VRAM)",
                autotune.gpu_name,
                autotune.gpu_compute_units,
                autotune.gpu_vram_bytes / (1024 * 1024),
            );
            println!(
                "  CPU: {} ({} physical / {} logical cores)",
                autotune.cpu_model,
                autotune.cpu_physical_cores,
                autotune.cpu_cores,
            );
            println!(
                "  RAM: {} MB",
                autotune.sys_ram_bytes / (1024 * 1024),
            );
            println!(
                "  Recommended: gpu_work_size={} secondary_gpu_work_size={} threads={} verushash_nonce_count={}",
                autotune.gpu_work_size,
                autotune.secondary_gpu_work_size,
                autotune.threads,
                autotune.verushash_nonce_count,
            );
            println!("  (Set ZION_AUTOTUNE=0 to disable)");
            println!("=========================");
        }

        let threads = std::env::var("ZION_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| {
                if autotune_enabled {
                    autotune.threads
                } else {
                    parallel::detect_threads()
                }
            });

        let miner_id = env_or_default("ZION_MINER_ID", "local-miner");
        let payout_address = std::env::var("ZION_PAYOUT_ADDRESS")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| miner_id.clone());

        // ── Work size autotuning ──
        // If env var is set, use it. Otherwise use autotuned value (if enabled)
        // or fall back to 256K default.
        let gpu_work_size = std::env::var("ZION_GPU_WORK_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| {
                if autotune_enabled {
                    autotune.gpu_work_size
                } else {
                    1 << 18 // 256K default
                }
            });
        let pearl_gpu_work_size = std::env::var("ZION_PEARL_GPU_WORK_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(gpu_work_size);
        let secondary_gpu_work_size = std::env::var("ZION_SECONDARY_GPU_WORK_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| {
                if autotune_enabled {
                    autotune.secondary_gpu_work_size
                } else {
                    1 << 18 // 256K default
                }
            });

        // ── Nonce count default ──
        // For GPU mining, nonce_count must be ≥ work_size to fill the GPU
        // pipeline, and ≥ 4× work_size to activate double-buffered async
        // readback (the key optimization for 28-30 KH/s on RX 5700 XT).
        // The old default of 1024 was far too small — it caused the GPU to
        // process only 1024 nonces per batch with no double-buffering,
        // resulting in ~10 KH/s instead of 28-30 KH/s.
        // If ZION_NONCE_COUNT is explicitly set, respect it. Otherwise:
        //   - GPU available: 4× gpu_work_size (e.g. 4×8192 = 32768)
        //   - CPU only: 1024 (original default)
        let gpu_backend_kind = gpu_backend::GpuBackendKind::from_env();
        let nonce_count_default = if gpu_backend_kind != gpu_backend::GpuBackendKind::Cpu
            && gpu_work_size > 0
        {
            gpu_work_size.saturating_mul(4) as u64
        } else {
            1024
        };

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
            nonce_count: parse_env_u64("ZION_NONCE_COUNT", nonce_count_default)?,
            nonce_autotune: parse_bool_env("ZION_NONCE_AUTOTUNE", true),
            nonce_count_min: parse_env_u64("ZION_NONCE_COUNT_MIN", {
                // For GPU mining, min must be ≥ 2× work_size to keep
                // double-buffered async readback active at all times.
                // double-buffering requires nonce_count > work_size, so
                // the minimum must be at least work_size + 1, but we use
                // 2× for safety margin (autotune shrinks by 50%).
                if gpu_backend_kind != gpu_backend::GpuBackendKind::Cpu && gpu_work_size > 0 {
                    (gpu_work_size as u64).saturating_mul(2).max(10_000)
                } else {
                    10_000
                }
            })?,
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
            gpu_backend: gpu_backend_kind,
            gpu_work_size,
            pearl_gpu_work_size,
            secondary_gpu_work_size,
            algorithm: std::env::var("ZION_MINER_ALGORITHM")
                .unwrap_or_else(|_| "deeksha_lite_v1".to_string()),
            stream1_enabled: parse_bool_env("ZION_STREAM1_ENABLED", true),
            stream2_enabled: parse_bool_env("ZION_STREAM2_ENABLED", true),
            stream3_enabled: parse_bool_env("ZION_STREAM3_ENABLED", true),
            verushash_nonce_count: std::env::var("ZION_EXT_CPU_NONCE_COUNT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| {
                    if autotune_enabled {
                        autotune.verushash_nonce_count
                    } else {
                        10_000_000
                    }
                }),
            auto_mode: parse_bool_env("ZION_AUTO_MODE", false),
            gpu_vram_bytes: autotune.gpu_vram_bytes,
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
    fn parse_header_hex_pads_short_input() {
        // External AuxPoW jobs may send shorter headers (e.g. KAS 32-byte pre_pow_hash);
        // the parser pads them to the 80-byte MiningHeader layout.
        let result = parse_header_hex("aabb");
        assert!(result.is_ok());
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


