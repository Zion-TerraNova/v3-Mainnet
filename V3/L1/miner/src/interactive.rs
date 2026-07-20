//! Interactive TUI for zion-miner
//!
//! Cross-platform keyboard control using crossterm:
//!   h  = toggle hashrate dashboard
//!   a  = cycle algorithm (Lite v1 -> Fire -> Ekam v2)
//!   c  = toggle CPU mining
//!   g  = toggle GPU mining
//!   d  = toggle dual mode
//!   i  = show hardware info
//!   p  = pause / resume
//!   r  = reconnect to pool
//!   v  = toggle verbose wire logging
//!   1-9 = set thread count
//!   q / Esc = quit gracefully

#![allow(dead_code)]

use std::collections::VecDeque;
use std::io::{self, stdout, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, ClearType},
};

use crate::ui;

/* ========================================================================= */
/* Shared control state                                                      */
/* ========================================================================= */

#[derive(Debug, Clone)]
pub enum MiningMode {
    CpuOnly,
    GpuOnly,
    Dual,
}

#[derive(Debug, Clone)]
pub struct MinerControl {
    pub pause: bool,
    pub algorithm: String,
    pub mode: MiningMode,
    pub cpu_enabled: bool,
    pub gpu_enabled: bool,
    pub dual_mode: bool,
    pub threads: usize,
    pub show_dashboard: bool,
    pub verbose: bool,
    pub requested_reconnect: bool,
    pub requested_quit: bool,
    pub thread_override: Option<usize>,
    /// Stream 3 CPU external coin (empty = pool/auto)
    pub cpu_coin: String,
    /// Stream 2 GPU external coin (empty = pool/auto)
    pub gpu_coin: String,
    /// Show extended metrics panel
    pub show_metrics: bool,
    /// Show online best miners panel
    pub show_online: bool,
}

const CPU_COIN_OPTIONS: &[&str] = &["auto", "VRSC", "XMR", "RTM"];
const GPU_COIN_OPTIONS: &[&str] = &[
    "auto", "RVN", "KAS", "ALPH", "DCR", "ERG", "ETC", "CLORE", "MEWC", "EVR", "FLUX", "EPIC",
    "ZANO",
];

impl MinerControl {
    pub fn new(algorithm: &str, threads: usize, gpu: bool, cpu_coin: &str, gpu_coin: &str) -> Self {
        Self {
            pause: false,
            algorithm: algorithm.to_string(),
            mode: if gpu {
                MiningMode::GpuOnly
            } else {
                MiningMode::CpuOnly
            },
            cpu_enabled: !gpu,
            gpu_enabled: gpu,
            dual_mode: false,
            threads,
            show_dashboard: true,
            verbose: false,
            requested_reconnect: false,
            requested_quit: false,
            thread_override: None,
            cpu_coin: cpu_coin.to_uppercase(),
            gpu_coin: gpu_coin.to_uppercase(),
            show_metrics: true,
            show_online: true,
        }
    }

    pub fn cycle_algorithm(&mut self) {
        const ALGOS: &[&str] = &[
            "deeksha_lite_v1",
            "deeksha_lite_fire",
            "cosmic_harmony_ekam_deeksha_v2",
        ];
        let idx = ALGOS.iter().position(|&a| a == self.algorithm).unwrap_or(0);
        self.algorithm = ALGOS[(idx + 1) % ALGOS.len()].to_string();
    }

    pub fn toggle_cpu(&mut self) {
        self.cpu_enabled = !self.cpu_enabled;
        self.recompute_mode();
    }

    pub fn toggle_gpu(&mut self) {
        self.gpu_enabled = !self.gpu_enabled;
        self.recompute_mode();
    }

    pub fn toggle_dual(&mut self) {
        self.dual_mode = !self.dual_mode;
        if self.dual_mode {
            self.cpu_enabled = true;
            self.gpu_enabled = true;
        }
        self.recompute_mode();
    }

    pub fn cycle_cpu_coin(&mut self) {
        let current = self.cpu_coin.as_str();
        let idx = CPU_COIN_OPTIONS
            .iter()
            .position(|&c| c.eq_ignore_ascii_case(current))
            .unwrap_or(0);
        self.cpu_coin = CPU_COIN_OPTIONS[(idx + 1) % CPU_COIN_OPTIONS.len()].to_uppercase();
    }

    pub fn cycle_gpu_coin(&mut self) {
        let current = self.gpu_coin.as_str();
        let idx = GPU_COIN_OPTIONS
            .iter()
            .position(|&c| c.eq_ignore_ascii_case(current))
            .unwrap_or(0);
        self.gpu_coin = GPU_COIN_OPTIONS[(idx + 1) % GPU_COIN_OPTIONS.len()].to_uppercase();
    }

    pub fn toggle_metrics(&mut self) {
        self.show_metrics = !self.show_metrics;
    }

    pub fn toggle_online(&mut self) {
        self.show_online = !self.show_online;
    }

    fn recompute_mode(&mut self) {
        self.mode = match (self.cpu_enabled, self.gpu_enabled, self.dual_mode) {
            (true, true, true) => MiningMode::Dual,
            (true, true, false) => MiningMode::GpuOnly,
            (true, false, _) => MiningMode::CpuOnly,
            (false, true, _) => MiningMode::GpuOnly,
            (false, false, _) => {
                self.cpu_enabled = true;
                MiningMode::CpuOnly
            }
        };
    }
}

/* ========================================================================= */
/* Global TUI mode flag                                                      */
/* ========================================================================= */

/// Set to true when interactive TUI is active.
/// maybe_print_status() and print_speed_table() check this to suppress
/// stdout output that would interfere with the alternate-screen dashboard.
pub static TUI_ACTIVE: AtomicBool = AtomicBool::new(false);

/* ========================================================================= */
/* Hashrate tracker with sliding time windows                                */
/* ========================================================================= */

/// One sample: (timestamp, cumulative total hashes at that moment)
struct Sample {
    ts: Instant,
    total: u64,
}

struct Window {
    samples: VecDeque<Sample>,
    max_age_secs: u64,
}

impl Window {
    fn new(max_age_secs: u64) -> Self {
        Self {
            samples: VecDeque::new(),
            max_age_secs,
        }
    }

    fn push(&mut self, total: u64) {
        let now = Instant::now();
        self.samples.push_back(Sample { ts: now, total });
        // Prune samples older than window, but always keep at least 2
        // so rate_hps() can compute a delta (important for slow-hash
        // algorithms like RandomX where batch completions are infrequent).
        while self.samples.len() > 2 {
            let age = now
                .duration_since(self.samples.front().unwrap().ts)
                .as_secs();
            if age > self.max_age_secs {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    /// Compute hashes/sec over the window span.
    fn rate_hps(&self) -> f64 {
        if self.samples.len() < 2 {
            return 0.0;
        }
        let first = self.samples.front().unwrap();
        let last = self.samples.back().unwrap();
        let dt = last.ts.duration_since(first.ts).as_secs_f64();
        if dt < 0.1 {
            return 0.0;
        }
        let hash_delta = last.total.saturating_sub(first.total) as f64;
        hash_delta / dt
    }
}

/// Triple sliding windows (10s / 60s / 15m) for a single stream.
struct StreamWindows {
    w10s: Window,
    w60s: Window,
    w15m: Window,
}

impl StreamWindows {
    fn new() -> Self {
        Self {
            w10s: Window::new(10),
            w60s: Window::new(60),
            w15m: Window::new(900),
        }
    }

    fn push(&mut self, total: u64) {
        self.w10s.push(total);
        self.w60s.push(total);
        self.w15m.push(total);
    }

    fn rates(&self) -> (f64, f64, f64) {
        (
            self.w10s.rate_hps(),
            self.w60s.rate_hps(),
            self.w15m.rate_hps(),
        )
    }
}

/// One online miner entry from the pool telemetry endpoint.
#[derive(Clone, Debug, Default)]
pub struct OnlineMiner {
    pub worker: String,
    pub coin: String,
    pub algorithm: String,
    pub hashrate: f64,
}

/// Snapshot of pool-side online miners + aggregate pool info.
#[derive(Clone, Debug, Default)]
pub struct OnlineMinerSnapshot {
    pub pool_hashrate: f64,
    pub active_miners: u64,
    pub total_miners: u64,
    pub top_miners: Vec<OnlineMiner>,
}

pub struct HashrateTracker {
    pub cpu_hashes: AtomicU64,
    pub gpu_hashes: AtomicU64,
    pub total_hashes: AtomicU64,
    pub accepted_shares: AtomicU64,
    pub rejected_shares: AtomicU64,
    // ── Per-stream share counters (3-stream parallel mining) ──
    /// Stream 1: ZION Deeksha accepted shares
    pub zion_accepted: AtomicU64,
    /// Stream 1: ZION Deeksha rejected shares
    pub zion_rejected: AtomicU64,
    /// Stream 2: GPU external profit coin accepted shares
    pub gpu_ext_accepted: AtomicU64,
    /// Stream 2: GPU external profit coin rejected shares
    pub gpu_ext_rejected: AtomicU64,
    /// Stream 3: CPU external (Verus/RandomX/etc) accepted shares
    pub cpu_ext_accepted: AtomicU64,
    /// Stream 3: CPU external (Verus/RandomX/etc) rejected shares
    pub cpu_ext_rejected: AtomicU64,
    // ── Per-stream hash counters ──
    pub zion_hashes: AtomicU64,
    pub gpu_ext_hashes: AtomicU64,
    pub cpu_ext_hashes: AtomicU64,
    /// Pool height — updated by mining loop via set_pool_height()
    pub pool_height: AtomicU64,
    /// GPU device info set by mining loop after gpu_init (via set_gpu_info)
    pub gpu_info: Mutex<Vec<GpuInfoLine>>,
    /// Aggregate windows protected by a single mutex
    windows: Mutex<(Window, Window, Window)>, // 10s, 60s, 15m
    /// Per-stream windows
    zion_windows: Mutex<StreamWindows>,
    gpu_ext_windows: Mutex<StreamWindows>,
    cpu_ext_windows: Mutex<StreamWindows>,
    /// Current external GPU coin/algorithm (updated by external_gpu_thread)
    gpu_ext_coin: Mutex<String>,
    gpu_ext_algorithm: Mutex<String>,
    /// Current external CPU coin/algorithm (updated by external_cpu_thread)
    cpu_ext_coin: Mutex<String>,
    cpu_ext_algorithm: Mutex<String>,
    /// Whether external GPU stream is active (has a job)
    gpu_ext_active: AtomicU64,
    /// Whether external CPU stream is active (has a job)
    cpu_ext_active: AtomicU64,
    /// Pool-side online miner snapshot (updated by TUI poller thread)
    pub online_snapshot: Mutex<OnlineMinerSnapshot>,
}

impl HashrateTracker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            cpu_hashes: AtomicU64::new(0),
            gpu_hashes: AtomicU64::new(0),
            total_hashes: AtomicU64::new(0),
            accepted_shares: AtomicU64::new(0),
            rejected_shares: AtomicU64::new(0),
            zion_accepted: AtomicU64::new(0),
            zion_rejected: AtomicU64::new(0),
            gpu_ext_accepted: AtomicU64::new(0),
            gpu_ext_rejected: AtomicU64::new(0),
            cpu_ext_accepted: AtomicU64::new(0),
            cpu_ext_rejected: AtomicU64::new(0),
            zion_hashes: AtomicU64::new(0),
            gpu_ext_hashes: AtomicU64::new(0),
            cpu_ext_hashes: AtomicU64::new(0),
            pool_height: AtomicU64::new(0),
            gpu_info: Mutex::new(Vec::new()),
            windows: Mutex::new((Window::new(10), Window::new(60), Window::new(900))),
            zion_windows: Mutex::new(StreamWindows::new()),
            gpu_ext_windows: Mutex::new(StreamWindows::new()),
            cpu_ext_windows: Mutex::new(StreamWindows::new()),
            gpu_ext_coin: Mutex::new(String::new()),
            gpu_ext_algorithm: Mutex::new(String::new()),
            cpu_ext_coin: Mutex::new(String::new()),
            cpu_ext_algorithm: Mutex::new(String::new()),
            gpu_ext_active: AtomicU64::new(0),
            cpu_ext_active: AtomicU64::new(0),
            online_snapshot: Mutex::new(OnlineMinerSnapshot::default()),
        })
    }

    /// Called once by mining loop after successful GPU init.
    pub fn set_gpu_info(&self, info: Vec<GpuInfoLine>) {
        if let Ok(mut g) = self.gpu_info.lock() {
            *g = info;
        }
    }

    pub fn get_gpu_info(&self) -> Vec<GpuInfoLine> {
        self.gpu_info.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Set current external GPU coin/algorithm (called by external_gpu_thread)
    pub fn set_gpu_ext_job(&self, coin: &str, algorithm: &str) {
        if let Ok(mut c) = self.gpu_ext_coin.lock() {
            *c = coin.to_string();
        }
        if let Ok(mut a) = self.gpu_ext_algorithm.lock() {
            *a = algorithm.to_string();
        }
        self.gpu_ext_active.store(1, Ordering::Relaxed);
    }

    /// Mark external GPU stream as idle (no job)
    pub fn clear_gpu_ext_job(&self) {
        self.gpu_ext_active.store(0, Ordering::Relaxed);
    }

    /// Set current external CPU coin/algorithm (called by external_cpu_thread)
    pub fn set_cpu_ext_job(&self, coin: &str, algorithm: &str) {
        if let Ok(mut c) = self.cpu_ext_coin.lock() {
            *c = coin.to_string();
        }
        if let Ok(mut a) = self.cpu_ext_algorithm.lock() {
            *a = algorithm.to_string();
        }
        self.cpu_ext_active.store(1, Ordering::Relaxed);
    }

    /// Mark external CPU stream as idle (no job)
    pub fn clear_cpu_ext_job(&self) {
        self.cpu_ext_active.store(0, Ordering::Relaxed);
    }

    /// Build per-stream stats for the triple-stream display.
    /// `zion_algorithm` is the current ZION algorithm (from control/telemetry).
    pub fn build_stream_stats(&self, zion_algorithm: &str) -> Vec<crate::ui::StreamStats> {
        let (z10, z60, z15m) = if let Ok(w) = self.zion_windows.lock() {
            w.rates()
        } else {
            (0.0, 0.0, 0.0)
        };
        let (g10, g60, _g15m) = if let Ok(w) = self.gpu_ext_windows.lock() {
            w.rates()
        } else {
            (0.0, 0.0, 0.0)
        };
        let (c10, c60, _c15m) = if let Ok(w) = self.cpu_ext_windows.lock() {
            w.rates()
        } else {
            (0.0, 0.0, 0.0)
        };

        let gpu_coin = self.gpu_ext_coin.lock().map(|c| c.clone()).unwrap_or_default();
        let gpu_algo = self.gpu_ext_algorithm.lock().map(|a| a.clone()).unwrap_or_default();
        let cpu_coin = self.cpu_ext_coin.lock().map(|c| c.clone()).unwrap_or_default();
        let cpu_algo = self.cpu_ext_algorithm.lock().map(|a| a.clone()).unwrap_or_default();
        let gpu_active = self.gpu_ext_active.load(Ordering::Relaxed) == 1;
        let cpu_active = self.cpu_ext_active.load(Ordering::Relaxed) == 1;

        vec![
            crate::ui::StreamStats {
                label: "ZION",
                coin: "ZION".to_string(),
                algorithm: zion_algorithm.to_string(),
                hashrate_10s: z10,
                hashrate_60s: z60,
                hashrate_15m: z15m,
                accepted: self.zion_accepted.load(Ordering::Relaxed),
                rejected: self.zion_rejected.load(Ordering::Relaxed),
                active: true, // ZION is always active
            },
            crate::ui::StreamStats {
                label: "GPU PROFIT",
                coin: gpu_coin,
                algorithm: gpu_algo,
                hashrate_10s: g10,
                hashrate_60s: g60,
                hashrate_15m: 0.0,
                accepted: self.gpu_ext_accepted.load(Ordering::Relaxed),
                rejected: self.gpu_ext_rejected.load(Ordering::Relaxed),
                active: gpu_active,
            },
            crate::ui::StreamStats {
                label: "CPU PROFIT",
                coin: cpu_coin,
                algorithm: cpu_algo,
                hashrate_10s: c10,
                hashrate_60s: c60,
                hashrate_15m: 0.0,
                accepted: self.cpu_ext_accepted.load(Ordering::Relaxed),
                rejected: self.cpu_ext_rejected.load(Ordering::Relaxed),
                active: cpu_active,
            },
        ]
    }

    pub fn record_cpu_hashes(&self, n: u64) {
        self.cpu_hashes.fetch_add(n, Ordering::Relaxed);
        let total = self.total_hashes.fetch_add(n, Ordering::Relaxed) + n;
        self.push_windows(total);
    }

    pub fn record_gpu_hashes(&self, n: u64) {
        self.gpu_hashes.fetch_add(n, Ordering::Relaxed);
        let total = self.total_hashes.fetch_add(n, Ordering::Relaxed) + n;
        self.push_windows(total);
    }

    /// Stream 1 (ZION Deeksha) hash progress.
    pub fn record_zion_hashes(&self, n: u64) {
        self.zion_hashes.fetch_add(n, Ordering::Relaxed);
        let total = self.zion_hashes.load(Ordering::Relaxed);
        if let Ok(mut w) = self.zion_windows.lock() {
            w.push(total);
        }
    }

    /// Stream 2 (GPU external profit coin) hash progress.
    pub fn record_gpu_ext_hashes(&self, n: u64) {
        self.gpu_ext_hashes.fetch_add(n, Ordering::Relaxed);
        let total = self.gpu_ext_hashes.load(Ordering::Relaxed);
        if let Ok(mut w) = self.gpu_ext_windows.lock() {
            w.push(total);
        }
    }

    /// Stream 1 (ZION Deeksha) hashrate over the 60s window (H/s).
    /// Used by the adaptive GPU duty-cycle scheduler to balance Stream 1
    /// vs Stream 2 GPU time-slicing based on actual hashrate.
    pub fn zion_hps_60s(&self) -> f64 {
        if let Ok(w) = self.zion_windows.lock() {
            let (_, h60, _) = w.rates();
            h60
        } else {
            0.0
        }
    }

    /// Stream 2 (GPU external profit coin) hashrate over the 60s window (H/s).
    pub fn gpu_ext_hps_60s(&self) -> f64 {
        if let Ok(w) = self.gpu_ext_windows.lock() {
            let (_, h60, _) = w.rates();
            h60
        } else {
            0.0
        }
    }

    /// Stream 3 (CPU external Verus/RandomX) hashrate over the 60s window (H/s).
    pub fn cpu_ext_hps_60s(&self) -> f64 {
        if let Ok(w) = self.cpu_ext_windows.lock() {
            let (_, h60, _) = w.rates();
            h60
        } else {
            0.0
        }
    }

    /// Stream 3 (CPU external Verus/RandomX/etc) hash progress.
    pub fn record_cpu_ext_hashes(&self, n: u64) {
        self.cpu_ext_hashes.fetch_add(n, Ordering::Relaxed);
        let total = self.cpu_ext_hashes.load(Ordering::Relaxed);
        if let Ok(mut w) = self.cpu_ext_windows.lock() {
            w.push(total);
        }
    }

    pub fn record_share(&self, accepted: bool) {
        if accepted {
            self.accepted_shares.fetch_add(1, Ordering::Relaxed);
        } else {
            self.rejected_shares.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record a Stream 1 (ZION Deeksha) share result.
    pub fn record_zion_share(&self, accepted: bool) {
        if accepted {
            self.zion_accepted.fetch_add(1, Ordering::Relaxed);
            self.accepted_shares.fetch_add(1, Ordering::Relaxed);
        } else {
            self.zion_rejected.fetch_add(1, Ordering::Relaxed);
            self.rejected_shares.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record a Stream 2 (GPU external profit coin) share result.
    pub fn record_gpu_ext_share(&self, accepted: bool) {
        if accepted {
            self.gpu_ext_accepted.fetch_add(1, Ordering::Relaxed);
            self.accepted_shares.fetch_add(1, Ordering::Relaxed);
        } else {
            self.gpu_ext_rejected.fetch_add(1, Ordering::Relaxed);
            self.rejected_shares.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record a Stream 3 (CPU external Verus/RandomX/etc) share result.
    pub fn record_cpu_ext_share(&self, accepted: bool) {
        if accepted {
            self.cpu_ext_accepted.fetch_add(1, Ordering::Relaxed);
            self.accepted_shares.fetch_add(1, Ordering::Relaxed);
        } else {
            self.cpu_ext_rejected.fetch_add(1, Ordering::Relaxed);
            self.rejected_shares.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn set_pool_height(&self, h: u64) {
        self.pool_height.store(h, Ordering::Relaxed);
    }

    fn push_windows(&self, total: u64) {
        if let Ok(mut w) = self.windows.lock() {
            w.0.push(total);
            w.1.push(total);
            w.2.push(total);
        }
    }

    pub fn compute_rates(&self) -> ComputedHashrates {
        let (r10, r60, r15m) = if let Ok(w) = self.windows.lock() {
            (w.0.rate_hps(), w.1.rate_hps(), w.2.rate_hps())
        } else {
            (0.0, 0.0, 0.0)
        };
        let (z10, z60, z15m) = if let Ok(w) = self.zion_windows.lock() {
            w.rates()
        } else {
            (0.0, 0.0, 0.0)
        };
        let (g10, g60, g15m) = if let Ok(w) = self.gpu_ext_windows.lock() {
            w.rates()
        } else {
            (0.0, 0.0, 0.0)
        };
        let (c10, c60, c15m) = if let Ok(w) = self.cpu_ext_windows.lock() {
            w.rates()
        } else {
            (0.0, 0.0, 0.0)
        };
        ComputedHashrates {
            total_hps: r10, // "current" = most recent 10s window
            total_10s_hps: r10,
            total_60s_hps: r60,
            total_15m_hps: r15m,
            cpu_total: self.cpu_hashes.load(Ordering::Relaxed),
            gpu_total: self.gpu_hashes.load(Ordering::Relaxed),
            accepted: self.accepted_shares.load(Ordering::Relaxed),
            rejected: self.rejected_shares.load(Ordering::Relaxed),
            zion_accepted: self.zion_accepted.load(Ordering::Relaxed),
            zion_rejected: self.zion_rejected.load(Ordering::Relaxed),
            gpu_ext_accepted: self.gpu_ext_accepted.load(Ordering::Relaxed),
            gpu_ext_rejected: self.gpu_ext_rejected.load(Ordering::Relaxed),
            cpu_ext_accepted: self.cpu_ext_accepted.load(Ordering::Relaxed),
            cpu_ext_rejected: self.cpu_ext_rejected.load(Ordering::Relaxed),
            zion_10s_hps: z10,
            zion_60s_hps: z60,
            zion_15m_hps: z15m,
            gpu_ext_10s_hps: g10,
            gpu_ext_60s_hps: g60,
            gpu_ext_15m_hps: g15m,
            cpu_ext_10s_hps: c10,
            cpu_ext_60s_hps: c60,
            cpu_ext_15m_hps: c15m,
            zion_total: self.zion_hashes.load(Ordering::Relaxed),
            gpu_ext_total: self.gpu_ext_hashes.load(Ordering::Relaxed),
            cpu_ext_total: self.cpu_ext_hashes.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ComputedHashrates {
    pub total_hps: f64,
    pub total_10s_hps: f64,
    pub total_60s_hps: f64,
    pub total_15m_hps: f64,
    pub cpu_total: u64,
    pub gpu_total: u64,
    pub accepted: u64,
    pub rejected: u64,
    /// Stream 1: ZION Deeksha
    pub zion_accepted: u64,
    pub zion_rejected: u64,
    /// Stream 2: GPU external profit coin
    pub gpu_ext_accepted: u64,
    pub gpu_ext_rejected: u64,
    /// Stream 3: CPU external Verus/RandomX/etc
    pub cpu_ext_accepted: u64,
    pub cpu_ext_rejected: u64,
    /// Per-stream hashrates (10s / 60s / 15m)
    pub zion_10s_hps: f64,
    pub zion_60s_hps: f64,
    pub zion_15m_hps: f64,
    pub gpu_ext_10s_hps: f64,
    pub gpu_ext_60s_hps: f64,
    pub gpu_ext_15m_hps: f64,
    pub cpu_ext_10s_hps: f64,
    pub cpu_ext_60s_hps: f64,
    pub cpu_ext_15m_hps: f64,
    pub zion_total: u64,
    pub gpu_ext_total: u64,
    pub cpu_ext_total: u64,
}

/* ========================================================================= */
/* Pool API helpers for online best miners                                   */
/* ========================================================================= */

fn derive_pool_api_addr(pool_addr: &str) -> String {
    std::env::var("ZION_POOL_API_ADDR")
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
        })
}

fn http_get_json(api_addr: &str, path: &str) -> Option<serde_json::Value> {
    let socket_addrs: Vec<std::net::SocketAddr> = api_addr.to_socket_addrs().ok()?.collect();
    if socket_addrs.is_empty() {
        return None;
    }
    let mut stream = TcpStream::connect_timeout(&socket_addrs[0], Duration::from_secs(3)).ok()?;
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, api_addr
    );
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => response.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
        if response.len() > 262144 {
            break;
        }
    }
    let response_str = String::from_utf8_lossy(&response);
    let body_start = response_str
        .find("\r\n\r\n")
        .map(|p| p + 4)
        .or_else(|| response_str.find("\n\n").map(|p| p + 2))?;
    let body = &response_str[body_start..];
    serde_json::from_str::<serde_json::Value>(body).ok()
}

fn parse_online_miner(v: &serde_json::Value) -> Option<OnlineMiner> {
    let obj = v.as_object()?;
    let worker = obj
        .get("worker_name")
        .and_then(|x| x.as_str())
        .unwrap_or("?")
        .to_string();
    let hashrate = obj.get("hashrate").and_then(|x| x.as_f64()).unwrap_or(0.0);
    let mut coin = obj
        .get("streams")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.first())
        .and_then(|s| s.as_object())
        .and_then(|s| s.get("coin"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let mut algorithm = obj
        .get("streams")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.first())
        .and_then(|s| s.as_object())
        .and_then(|s| s.get("algorithm"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    if coin.is_empty() {
        coin = obj.get("algorithm").and_then(|x| x.as_str()).unwrap_or("?").to_string();
    }
    if algorithm.is_empty() {
        algorithm = obj.get("backend").and_then(|x| x.as_str()).unwrap_or("?").to_string();
    }
    Some(OnlineMiner {
        worker,
        coin,
        algorithm,
        hashrate,
    })
}

pub fn fetch_online_snapshot(pool_addr: &str) -> Option<OnlineMinerSnapshot> {
    let api_addr = derive_pool_api_addr(pool_addr);

    // Fetch pool aggregate stats for hashrate + active miner count.
    let mut snapshot = OnlineMinerSnapshot::default();
    if let Some(stats) = http_get_json(&api_addr, "/stats") {
        snapshot.pool_hashrate = stats
            .get("hashrate")
            .and_then(|h| h.get("pool"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        snapshot.active_miners = stats
            .get("miners")
            .and_then(|m| m.get("active"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
    }

    // Fetch top miners by live hashrate.
    if let Some(data) = http_get_json(&api_addr, "/miners?limit=50") {
        snapshot.total_miners = data.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
        if let Some(miners) = data.get("miners").and_then(|v| v.as_array()) {
            let mut top: Vec<OnlineMiner> = miners
                .iter()
                .filter_map(parse_online_miner)
                .collect();
            top.sort_by(|a, b| b.hashrate.partial_cmp(&a.hashrate).unwrap_or(std::cmp::Ordering::Equal));
            top.truncate(5);
            snapshot.top_miners = top;
        }
    }

    if snapshot.pool_hashrate == 0.0 && snapshot.top_miners.is_empty() {
        None
    } else {
        Some(snapshot)
    }
}

/* ========================================================================= */
/* Dashboard renderer                                                        */
/* ========================================================================= */

/// Short display names for algorithms
fn algo_display(algo: &str) -> &str {
    match algo {
        "deeksha_lite_v1" => "Deeksha Lite v1",
        "deeksha_lite_fire" => "Deeksha Lite Fire",
        "cosmic_harmony_ekam_deeksha_v2" => "Ekam Deeksha v2",
        other => other,
    }
}

pub(crate) fn draw_dashboard(
    control: &MinerControl,
    rates: &ComputedHashrates,
    uptime_secs: u64,
    pool_height: u64,
    gpu_info: &[GpuInfoLine],
    metrics: &Arc<Mutex<crate::MinerMetricsSnapshot>>,
    hashrate: &Arc<HashrateTracker>,
) -> io::Result<()> {
    let mut out = stdout();

    // ── Stable redraw: full clear + move home ──
    queue!(
        out,
        cursor::MoveTo(0, 0),
        terminal::Clear(ClearType::All),
    )?;

    let (cols, rows) = terminal::size().unwrap_or((80, 24));
    let _ = cols; // kept for future truncation logic; currently target 80 cols

    // ── Title bar ──
    let title = format!(
        " ZION v3.0.6  Triple Parallel  |  {}",
        algo_display(&control.algorithm)
    );
    let title_padded = format!("{:<78}", title);
    queue!(
        out,
        SetBackgroundColor(Color::Rgb {
            r: 20,
            g: 20,
            b: 50
        }),
        SetForegroundColor(Color::Cyan),
        Print(format!(" {title_padded}\n")),
        ResetColor,
    )?;

    // ── Status line ──
    let status_color = if control.pause {
        Color::Yellow
    } else {
        Color::Green
    };
    let status_text = if control.pause { "PAUSED " } else { "RUNNING" };
    let mode_str = match control.mode {
        MiningMode::CpuOnly => "CPU ",
        MiningMode::GpuOnly => "GPU ",
        MiningMode::Dual => "DUAL",
    };
    let gpu_actual = hashrate.gpu_ext_coin.lock().map(|c| c.clone()).unwrap_or_default();
    let cpu_actual = hashrate.cpu_ext_coin.lock().map(|c| c.clone()).unwrap_or_default();
    let gpu_coin = if !gpu_actual.is_empty() {
        gpu_actual
    } else if control.gpu_coin.is_empty() {
        "auto".to_string()
    } else {
        control.gpu_coin.clone()
    };
    let cpu_coin = if !cpu_actual.is_empty() {
        cpu_actual
    } else if control.cpu_coin.is_empty() {
        "auto".to_string()
    } else {
        control.cpu_coin.clone()
    };
    queue!(
        out,
        Print("  "),
        SetForegroundColor(status_color),
        Print(status_text.to_string()),
        ResetColor,
        Print(format!(
            "  algo={:<24} mode={:<4} threads={:<2}  CPU={:<5} GPU={:<5}\n",
            algo_display(&control.algorithm), mode_str, control.threads, cpu_coin, gpu_coin
        )),
    )?;

    // ── Separator ──
    queue!(
        out,
        SetForegroundColor(Color::DarkGrey),
        Print("  ------------------------------------------------------------------------------\n"),
        ResetColor,
    )?;

    // ── Hashrate ──
    let (v10, u10) = ui::fmt_hashrate(rates.total_10s_hps);
    let (v60, u60) = ui::fmt_hashrate(rates.total_60s_hps);
    let (v15m, u15m) = ui::fmt_hashrate(rates.total_15m_hps);
    queue!(
        out,
        Print(format!(
            "  Hashrate  10s {:>9}{:<4}  60s {:>9}{:<4}  15m {:>9}{:<4}\n",
            v10, u10, v60, u60, v15m, u15m
        )),
    )?;
    let cpu_hps = if uptime_secs > 0 {
        rates.cpu_total as f64 / uptime_secs as f64
    } else {
        0.0
    };
    let gpu_hps = if uptime_secs > 0 {
        rates.gpu_total as f64 / uptime_secs as f64
    } else {
        0.0
    };
    let (vcpu_tot, ucpu_tot) = ui::fmt_hashrate(cpu_hps);
    let (vgpu_tot, ugpu_tot) = ui::fmt_hashrate(gpu_hps);
    queue!(
        out,
        Print(format!(
            "  CPU total {:>9}{:<4}  GPU total {:>9}{:<4}\n",
            vcpu_tot, ucpu_tot, vgpu_tot, ugpu_tot
        )),
    )?;

    // ── Triple Stream Shares (Claymore-style per-stream breakdown) ──
    let zion_total = rates.zion_accepted + rates.zion_rejected;
    let zion_pct = if zion_total > 0 { rates.zion_accepted as f64 * 100.0 / zion_total as f64 } else { 100.0 };
    let gpu_ext_total = rates.gpu_ext_accepted + rates.gpu_ext_rejected;
    let gpu_ext_pct = if gpu_ext_total > 0 { rates.gpu_ext_accepted as f64 * 100.0 / gpu_ext_total as f64 } else { 100.0 };
    let cpu_ext_total = rates.cpu_ext_accepted + rates.cpu_ext_rejected;
    let cpu_ext_pct = if cpu_ext_total > 0 { rates.cpu_ext_accepted as f64 * 100.0 / cpu_ext_total as f64 } else { 100.0 };

    let (zion_hr, zion_unit) = ui::fmt_hashrate(rates.zion_10s_hps);
    queue!(
        out,
        Print("  Stream 1 "),
        SetForegroundColor(Color::Cyan),
        Print("ZION"),
        ResetColor,
        Print(format!(
            "     {:>7}{:<3} {:>5}/{:<3} ({:>5.1}%)\n",
            zion_hr, zion_unit, rates.zion_accepted, rates.zion_rejected, zion_pct
        )),
    )?;
    let (gpu_ext_hr, gpu_ext_unit) = ui::fmt_hashrate(rates.gpu_ext_10s_hps);
    queue!(
        out,
        Print("  Stream 2 "),
        SetForegroundColor(Color::Magenta),
        Print("GPU PROFIT"),
        ResetColor,
        Print(format!(
            " {:>7}{:<3} {:>5}/{:<3} ({:>5.1}%)  coin={:<5}\n",
            gpu_ext_hr, gpu_ext_unit, rates.gpu_ext_accepted, rates.gpu_ext_rejected, gpu_ext_pct, gpu_coin
        )),
    )?;
    let (cpu_ext_hr, cpu_ext_unit) = ui::fmt_hashrate(rates.cpu_ext_10s_hps);
    queue!(
        out,
        Print("  Stream 3 "),
        SetForegroundColor(Color::Yellow),
        Print("CPU PROFIT"),
        ResetColor,
        Print(format!(
            " {:>7}{:<3} {:>5}/{:<3} ({:>5.1}%)  coin={:<5}\n",
            cpu_ext_hr, cpu_ext_unit, rates.cpu_ext_accepted, rates.cpu_ext_rejected, cpu_ext_pct, cpu_coin
        )),
    )?;

    // ── Total Shares ──
    let acc = rates.accepted;
    let rej = rates.rejected;
    let total = acc + rej;
    let pct = if total > 0 { acc as f64 * 100.0 / total as f64 } else { 100.0 };
    let rej_col = if rej > 0 { Color::Red } else { Color::DarkGrey };
    queue!(
        out,
        Print("  Total     "),
        SetForegroundColor(Color::Green),
        Print(format!("{acc} accepted")),
        ResetColor,
        Print("  /  "),
        SetForegroundColor(rej_col),
        Print(format!("{rej} rejected")),
        ResetColor,
        Print(format!("  ({pct:.1}%)\n")),
    )?;

    // ── Pool info ──
    let online = hashrate
        .online_snapshot
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();
    let (pool_hr, pool_hr_unit) = ui::fmt_hashrate(online.pool_hashrate);
    queue!(
        out,
        Print(format!(
            "  Pool      height={:<6}  uptime={:<8}  poolHR={:>7}{:<3}  active_miners={}/{}\n",
            pool_height,
            ui::fmt_uptime(uptime_secs),
            pool_hr,
            pool_hr_unit,
            online.active_miners,
            online.total_miners
        )),
    )?;

    // ── Extended metrics panel ──
    if control.show_metrics && rows > 18 {
        let tui = metrics.lock().map(|m| m.as_tui()).unwrap_or_default();
        queue!(
            out,
            SetForegroundColor(Color::DarkGrey),
            Print("  ------------------------------------------------------------------------------\n"),
            ResetColor,
        )?;
        queue!(
            out,
            Print(format!(
                "  Metrics   latency avg/max: {:>6.1}/{:<4}ms  best_batch={:<4}ms  remote_ttl={:<4}ms  peak={:>7.2} H/s\n",
                tui.submit_avg_ms, tui.submit_max_ms, tui.best_batch_ms, tui.remote_ttl_ms, tui.hashrate_max
            )),
        )?;
        queue!(
            out,
            Print(format!(
                "            iter={:<4}  threads={:<2}  nonce={:<6}  status={:<10}  backend={:<8}\n",
                tui.current_iteration, tui.threads, tui.nonce_window, tui.status, tui.backend
            )),
        )?;

        let (zz10, zz60, zz15) = (rates.zion_10s_hps, rates.zion_60s_hps, rates.zion_15m_hps);
        let (gz10, gz60, gz15) = (rates.gpu_ext_10s_hps, rates.gpu_ext_60s_hps, rates.gpu_ext_15m_hps);
        let (cz10, cz60, cz15) = (rates.cpu_ext_10s_hps, rates.cpu_ext_60s_hps, rates.cpu_ext_15m_hps);
        let (zv10, zu10) = ui::fmt_hashrate(zz10); let (zv60, zu60) = ui::fmt_hashrate(zz60); let (zv15, zu15) = ui::fmt_hashrate(zz15);
        let (gv10, gu10) = ui::fmt_hashrate(gz10); let (gv60, gu60) = ui::fmt_hashrate(gz60); let (gv15, gu15) = ui::fmt_hashrate(gz15);
        let (cv10, cu10) = ui::fmt_hashrate(cz10); let (cv60, cu60) = ui::fmt_hashrate(cz60); let (cv15, cu15) = ui::fmt_hashrate(cz15);
        queue!(
            out,
            Print(format!(
                "  Windows   ZION 10s{:>7}{:<3} 60s{:>7}{:<3} 15m{:>7}{:<3}\n",
                zv10, zu10, zv60, zu60, zv15, zu15
            )),
        )?;
        queue!(
            out,
            Print(format!(
                "            GPU  10s{:>7}{:<3} 60s{:>7}{:<3} 15m{:>7}{:<3}\n",
                gv10, gu10, gv60, gu60, gv15, gu15
            )),
        )?;
        queue!(
            out,
            Print(format!(
                "            CPU  10s{:>7}{:<3} 60s{:>7}{:<3} 15m{:>7}{:<3}\n",
                cv10, cu10, cv60, cu60, cv15, cu15
            )),
        )?;
    }

    // ── Online best miners panel ──
    if control.show_online && rows > 22 {
        queue!(
            out,
            SetForegroundColor(Color::DarkGrey),
            Print("  ------------------------------------------------------------------------------\n"),
            ResetColor,
        )?;
        queue!(
            out,
            SetForegroundColor(Color::Cyan),
            Print("  ONLINE BEST MINERS\n"),
            ResetColor,
        )?;
        if online.top_miners.is_empty() {
            queue!(
                out,
                SetForegroundColor(Color::DarkGrey),
                Print("  (waiting for pool API ... forward port 8455 or set ZION_POOL_API_ADDR)\n"),
                ResetColor,
            )?;
        } else {
            for (i, m) in online.top_miners.iter().enumerate() {
                let (hr, unit) = ui::fmt_hashrate(m.hashrate);
                let worker = if m.worker.len() > 14 { &m.worker[..14] } else { &m.worker };
                let coin = if m.coin.len() > 8 { &m.coin[..8] } else { &m.coin };
                let algo = if m.algorithm.len() > 12 { &m.algorithm[..12] } else { &m.algorithm };
                queue!(
                    out,
                    Print(format!(
                        "  #{:<2} {:<14} {:>8.2}{:<3}  {:<8}  {:<12}\n",
                        i + 1, worker, hr, unit, coin, algo
                    )),
                )?;
            }
        }
    }

    // ── GPU devices (max 2) ──
    queue!(
        out,
        SetForegroundColor(Color::DarkGrey),
        Print("  ------------------------------------------------------------------------------\n"),
        ResetColor,
    )?;
    if gpu_info.is_empty() {
        queue!(out, Print("  GPU       (no devices)\n"))?;
        queue!(out, Print("\n"))?;
    } else {
        for g in gpu_info.iter().take(2) {
            queue!(out, Print(format!("  GPU #{:<2}   {}\n", g.index, g.info)),)?;
        }
        if gpu_info.len() == 1 {
            queue!(out, Print("\n"))?;
        }
    }

    // ── Separator ──
    queue!(
        out,
        SetForegroundColor(Color::DarkGrey),
        Print("  ------------------------------------------------------------------------------\n"),
        ResetColor,
    )?;

    // ── Hotkeys ──
    queue!(
        out,
        SetForegroundColor(Color::DarkGrey),
        Print("  [a]algo [c]CPU [C]cpu-coin [g]GPU [G]gpu-coin [d]dual [p]pause [r]recon [m]metrics [o]online [v]verb [q]quit\n"),
        ResetColor,
    )?;

    out.flush()?;
    Ok(())
}

#[derive(Clone)]
pub struct GpuInfoLine {
    pub index: usize,
    pub info: String,
}

/* ========================================================================= */
/* Keyboard handler                                                          */
/* ========================================================================= */

pub fn spawn_input_thread(control: Arc<Mutex<MinerControl>>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        loop {
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(Event::Key(KeyEvent {
                    code, modifiers, ..
                })) = event::read()
                {
                    let mut c = control.lock().unwrap();

                    if c.requested_quit {
                        break;
                    }

                    match code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            c.requested_quit = true;
                        }
                        KeyCode::Char('h') => {
                            c.show_dashboard = !c.show_dashboard;
                        }
                        KeyCode::Char('p') => {
                            c.pause = !c.pause;
                        }
                        KeyCode::Char('a') => {
                            c.cycle_algorithm();
                            // Reconnect so pool gets new algo in Hello
                            c.requested_reconnect = true;
                        }
                        KeyCode::Char('c') if modifiers != KeyModifiers::CONTROL && modifiers != KeyModifiers::SHIFT => {
                            c.toggle_cpu();
                        }
                        KeyCode::Char('C') => {
                            c.cycle_cpu_coin();
                        }
                        KeyCode::Char('g') if modifiers != KeyModifiers::SHIFT => {
                            c.toggle_gpu();
                        }
                        KeyCode::Char('G') => {
                            c.cycle_gpu_coin();
                        }
                        KeyCode::Char('d') => {
                            c.toggle_dual();
                        }
                        KeyCode::Char('r') => {
                            c.requested_reconnect = true;
                        }
                        KeyCode::Char('m') => {
                            c.toggle_metrics();
                        }
                        KeyCode::Char('o') => {
                            c.toggle_online();
                        }
                        KeyCode::Char('v') => {
                            c.verbose = !c.verbose;
                        }
                        KeyCode::Char(ch) if ch.is_ascii_digit() && ch != '0' => {
                            let n = ch as usize - '0' as usize;
                            c.thread_override = Some(n);
                            c.threads = n;
                        }
                        _ => {}
                    }

                    // Ctrl+C quits
                    if code == KeyCode::Char('c') && modifiers == KeyModifiers::CONTROL {
                        c.requested_quit = true;
                    }
                }
            }
        }
    })
}

/* ========================================================================= */
/* Entry point                                                               */
/* ========================================================================= */

/// Run the interactive TUI (blocks until user presses q/Esc).
/// Mining loop should be running in a separate thread.
pub(crate) fn run_interactive(
    control: Arc<Mutex<MinerControl>>,
    hashrate: Arc<HashrateTracker>,
    metrics: Arc<Mutex<crate::MinerMetricsSnapshot>>,
    pool_addr: String,
) -> io::Result<()> {
    TUI_ACTIVE.store(true, Ordering::Relaxed);

    terminal::enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, cursor::Hide, terminal::EnterAlternateScreen)?;

    let input_handle = spawn_input_thread(Arc::clone(&control));

    // Pool API background poller — refreshes online best-miner snapshot
    let online_hashrate = Arc::clone(&hashrate);
    let online_control = Arc::clone(&control);
    let _online_poller = thread::spawn(move || {
        let mut consecutive_errors = 0u32;
        loop {
            thread::sleep(Duration::from_secs(10));
            if let Some(snapshot) = fetch_online_snapshot(&pool_addr) {
                if let Ok(mut guard) = online_hashrate.online_snapshot.lock() {
                    *guard = snapshot;
                }
                consecutive_errors = 0;
            } else {
                consecutive_errors = consecutive_errors.saturating_add(1);
                // Back off after repeated failures to avoid spamming the local API port
                if consecutive_errors > 6 {
                    thread::sleep(Duration::from_secs(60));
                }
            }
            // Stop when the TUI signals quit (best-effort detection)
            if online_control.lock().unwrap().requested_quit {
                break;
            }
        }
    });

    let dashboard_control = Arc::clone(&control);
    let dashboard_hashrate = Arc::clone(&hashrate);
    let dashboard_metrics = Arc::clone(&metrics);
    let started_at = Instant::now();

    let dashboard_handle = thread::spawn(move || {
        // Initial full clear
        let _ = execute!(
            io::stdout(),
            cursor::MoveTo(0, 0),
            terminal::Clear(ClearType::All),
        );

        loop {
            thread::sleep(Duration::from_millis(800));

            let c = dashboard_control.lock().unwrap();
            if c.requested_quit {
                break;
            }
            let show = c.show_dashboard;
            let control_snapshot = c.clone();
            drop(c);

            if !show {
                continue;
            }

            // GPU info comes from HashrateTracker (set by mining loop after gpu_init)
            // We never call query_gpu_details() from TUI thread — that requires OpenCL
            // context which lives in the mining thread.
            let cached_gpu_info = dashboard_hashrate.get_gpu_info();

            let rates = dashboard_hashrate.compute_rates();
            let pool_height = dashboard_hashrate.pool_height.load(Ordering::Relaxed);
            let uptime = started_at.elapsed().as_secs();
            let _ = draw_dashboard(
                &control_snapshot,
                &rates,
                uptime,
                pool_height,
                &cached_gpu_info,
                &dashboard_metrics,
                &dashboard_hashrate,
            );
        }
    });

    // Block until quit
    loop {
        thread::sleep(Duration::from_millis(100));
        if control.lock().unwrap().requested_quit {
            break;
        }
    }

    // Cleanup
    let _ = input_handle.join();
    let _ = dashboard_handle.join();
    execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    TUI_ACTIVE.store(false, Ordering::Relaxed);
    Ok(())
}
