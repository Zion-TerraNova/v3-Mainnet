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
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
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
/* TTY handle for TUI output (bypasses redirected stdout)                    */
/* ========================================================================= */

/// Global TTY file handle — when the TUI is active, mining thread stdout is
/// redirected to a log file. The TUI writes directly to /dev/tty instead.
static TTY: OnceLock<std::fs::File> = OnceLock::new();

/// Write-only wrapper around a File. This disambiguates crossterm's `queue!`
/// macro, which requires `by_ref()` — `File` implements both `Read` and
/// `Write`, causing ambiguity. This wrapper only implements `Write`.
struct TtyWriter {
    inner: std::fs::File,
}

/// Track how many lines the previous frame wrote, so we can move the cursor
/// up that many lines before redrawing (avoids terminal scroll).
static PREV_FRAME_LINES: AtomicUsize = AtomicUsize::new(0);

impl Write for TtyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Get a TtyWriter for TUI output. Opens /dev/tty if not yet initialized.
fn tty() -> TtyWriter {
    let file = if let Some(f) = TTY.get() {
        f.try_clone().unwrap_or_else(|_| open_tty_or_fallback())
    } else {
        // Initialize on first call
        let f = open_tty_or_fallback();
        let _ = TTY.set(f.try_clone().unwrap_or_else(|_| {
            std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/null")
                .unwrap_or_else(|_| panic!("no TTY and no /dev/null"))
        }));
        f
    };
    TtyWriter { inner: file }
}

fn open_tty_or_fallback() -> std::fs::File {
    #[cfg(unix)]
    {
        std::fs::OpenOptions::new()
            .write(true)
            .read(true)
            .open("/dev/tty")
            .unwrap_or_else(|_| {
                // Fallback: clone fd 1 (stdout) before it gets redirected
                use std::os::unix::io::FromRawFd;
                unsafe {
                    let fd = dup(1);
                    if fd >= 0 {
                        return std::fs::File::from_raw_fd(fd);
                    }
                }
                // Last resort: /dev/null
                std::fs::OpenOptions::new()
                    .write(true)
                    .open("/dev/null")
                    .unwrap_or_else(|_| panic!("no TTY and no /dev/null available"))
            })
    }
    #[cfg(not(unix))]
    {
        // Windows: no /dev/tty, use CONOUT$ or NUL
        std::fs::OpenOptions::new()
            .write(true)
            .open("CONOUT$")
            .unwrap_or_else(|_| {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open("NUL")
                    .unwrap_or_else(|_| panic!("no CONOUT$ and no NUL available"))
            })
    }
}

#[cfg(unix)]
extern "C" {
    fn dup(oldfd: i32) -> i32;
    fn dup2(oldfd: i32, newfd: i32) -> i32;
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}

/// Window size structure for TIOCGWINSZ ioctl
#[repr(C)]
struct libc_winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

/// TIOCGWINSZ ioctl request number (Linux: 0x5413)
const TIOCGWINSZ: u64 = 0x5413;

/// Safe wrapper for ioctl(fd, TIOCGWINSZ, &winsize)
#[cfg(unix)]
unsafe fn ioctl_tiocgwinsz(fd: i32, ws: &mut libc_winsize) -> i32 {
    ioctl(fd, TIOCGWINSZ, ws as *mut libc_winsize)
}

/// Redirect stdout (fd 1) to a log file. Called from main.rs when the
/// interactive TUI is enabled, BEFORE the mining thread is spawned.
/// Returns true on success.
pub fn redirect_stdout_to_log(path: &str) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let log = match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            Ok(f) => f,
            Err(_) => return false,
        };
        // Initialize TTY handle from /dev/tty before redirecting stdout
        if let Ok(t) = std::fs::OpenOptions::new()
            .write(true)
            .read(true)
            .open("/dev/tty")
        {
            let _ = TTY.set(t);
        }
        // Redirect fd 1 → log file
        unsafe {
            let log_fd = log.as_raw_fd();
            if dup2(log_fd, 1) < 0 {
                return false;
            }
            // Don't drop `log` — its fd is now fd 1, owned by the OS
            std::mem::forget(log);
        }
        true
    }
    #[cfg(not(unix))]
    {
        // Windows: no fd redirection, just open the log file
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .is_ok()
    }
}

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
            show_metrics: false,
            show_online: false,
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

/// One share log entry for the TUI share log panel.
#[derive(Clone, Debug)]
pub struct ShareLogEntry {
    pub timestamp: Instant,
    pub stream: String,
    pub accepted: bool,
    pub job_id: u64,
    pub latency_ms: u64,
    pub reason: String,
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
    /// Recent share log (ring buffer, max 8 entries) for TUI share log panel
    pub share_log: Mutex<VecDeque<ShareLogEntry>>,
    /// Hashrate history samples for sparkline graph (10s window, sampled every ~1s)
    pub hr_history: Mutex<VecDeque<f64>>,
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
            share_log: Mutex::new(VecDeque::with_capacity(8)),
            hr_history: Mutex::new(VecDeque::with_capacity(64)),
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

    // ── Share log + hashrate history (TUI pro dashboard) ──

    /// Push a share log entry (called from main.rs share handlers).
    pub fn log_share(&self, stream: &str, accepted: bool, job_id: u64, latency_ms: u64, reason: &str) {
        if let Ok(mut log) = self.share_log.lock() {
            if log.len() >= 8 {
                log.pop_front();
            }
            log.push_back(ShareLogEntry {
                timestamp: Instant::now(),
                stream: stream.to_string(),
                accepted,
                job_id,
                latency_ms,
                reason: reason.to_string(),
            });
        }
    }

    /// Sample the current 10s hashrate into the history buffer for the sparkline.
    /// Called by the TUI dashboard thread on each redraw.
    pub fn sample_hr_history(&self) {
        let hr = if let Ok(w) = self.windows.lock() {
            w.0.rate_hps()
        } else {
            0.0
        };
        if let Ok(mut hist) = self.hr_history.lock() {
            if hist.len() >= 64 {
                hist.pop_front();
            }
            hist.push_back(hr);
        }
    }

    /// Get the hashrate history samples for sparkline rendering.
    pub fn get_hr_history(&self) -> Vec<f64> {
        self.hr_history.lock().map(|h| h.iter().copied().collect()).unwrap_or_default()
    }

    /// Get a copy of the share log.
    pub fn get_share_log(&self) -> Vec<ShareLogEntry> {
        self.share_log.lock().map(|l| l.iter().cloned().collect()).unwrap_or_default()
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
    let mut out = tty();

    // ── Reposition cursor: move up by previous frame's line count, then clear down ──
    // This approach is more robust inside screen sessions than Clear(All) + MoveTo(0,0)
    use std::io::Write;
    if PREV_FRAME_LINES.load(Ordering::Relaxed) > 0 {
        let n = PREV_FRAME_LINES.load(Ordering::Relaxed);
        // Move cursor up N lines, carriage return to col 0, then clear from cursor to end of screen
        write!(out, "\x1b[{}A\r\x1b[J", n)?;
        out.flush()?;
    } else {
        // First frame: clear screen and move home
        write!(out, "\x1b[2J\x1b[H")?;
        out.flush()?;
    }

    // ── Adaptive width: query /dev/tty directly for real terminal size ──
    // Inside detached screen sessions, the pty often reports 80x24 regardless
    // of the actual terminal width. We allow override via ZION_TUI_WIDTH.
    let (term_cols, term_rows) = {
        // Check for explicit override first
        let override_w = std::env::var("ZION_TUI_WIDTH")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|&w| w >= 40);
        if let Some(w) = override_w {
            let h = terminal::size().map(|(_, h)| h).unwrap_or(24);
            (w, h)
        } else {
            // Try ioctl on /dev/tty (Unix only)
            #[cfg(unix)]
            {
                let mut ws: libc_winsize = libc_winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
                let tty_fd = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open("/dev/tty")
                    .ok();
                let mut got_size = false;
                if let Some(ref f) = tty_fd {
                    use std::os::unix::io::AsRawFd;
                    unsafe {
                        if ioctl_tiocgwinsz(f.as_raw_fd(), &mut ws) == 0 {
                            got_size = true;
                        }
                    }
                }
                if got_size && ws.ws_col > 0 && ws.ws_row > 0 {
                    (ws.ws_col, ws.ws_row)
                } else {
                    terminal::size().unwrap_or((80, 24))
                }
            }
            #[cfg(not(unix))]
            {
                terminal::size().unwrap_or((80, 24))
            }
        }
    };
    // Clamp width to 40..80
    let iw = (term_cols as usize).clamp(40, 80).saturating_sub(2); // inner width
    let avail_rows = term_rows as usize; // total usable rows

    // ── Helper: pad/truncate a string to exactly iw display columns ──
    fn pad_to(text: &str, width: usize) -> String {
        let chars: Vec<char> = text.chars().collect();
        if chars.len() > width {
            chars[..width].iter().collect()
        } else {
            let mut s: String = chars.iter().collect();
            for _ in chars.len()..width {
                s.push(' ');
            }
            s
        }
    }

    // ── Helper: center text within a given width ──
    fn center_text(text: &str, width: usize) -> String {
        let chars: Vec<char> = text.chars().collect();
        if chars.len() >= width {
            return chars[..width].iter().collect();
        }
        let pad = width - chars.len();
        let left = pad / 2;
        let right = pad - left;
        format!("{}{}{}", " ".repeat(left), text, " ".repeat(right))
    }

    // ── Borders (Unicode box-drawing characters) ──
    let top: String = format!("┌{}┐", "─".repeat(iw));
    let mid: String = format!("├{}┤", "─".repeat(iw));
    let bot: String = format!("└{}┘", "─".repeat(iw));

    // ── Helper: write a border line ──
    macro_rules! wborder {
        ($b:expr) => { queue!(out, Print(format!("\r{}\n", $b)))? };
    }
    // ── Helper: write a content line with padding ──
    macro_rules! wline {
        ($text:expr) => {
            queue!(out, Print(format!("\r│{}│\n", pad_to($text, iw))))?
        };
    }
    // ── Helper: write a section header ──
    macro_rules! wheader {
        ($text:expr) => {{
            queue!(out,
                SetForegroundColor(Color::Cyan),
                Print(format!("\r│{}│\n", pad_to(&format!(" {} ", $text), iw))),
                ResetColor,
            )?
        }};
    }

    // ── Title bar ──
    let algo_short = algo_display(&control.algorithm);
    let title_text = format!("ZION MINER v3.0.6 - Triple Parallel - {}", algo_short);
    let title = center_text(&title_text, iw);
    queue!(
        out,
        SetBackgroundColor(Color::Rgb { r: 0, g: 42, b: 78 }),
        SetForegroundColor(Color::White),
        Print(format!("\r{}\n", top)),
        Print(format!("\r│{}│\n", pad_to(&title, iw))),
        ResetColor,
    )?;

    // ── Status line ──
    let status_color = if control.pause { Color::Yellow } else { Color::Green };
    let status_dot = if control.pause { "o PAUSED " } else { "* RUNNING" };
    let mode_str = match control.mode {
        MiningMode::CpuOnly => "CPU",
        MiningMode::GpuOnly => "GPU",
        MiningMode::Dual => "DUAL",
    };
    let gpu_actual = hashrate.gpu_ext_coin.lock().map(|c| c.clone()).unwrap_or_default();
    let cpu_actual = hashrate.cpu_ext_coin.lock().map(|c| c.clone()).unwrap_or_default();
    let gpu_coin = if !gpu_actual.is_empty() { gpu_actual }
        else if control.gpu_coin.is_empty() { "auto".to_string() }
        else { control.gpu_coin.clone() };
    let cpu_coin = if !cpu_actual.is_empty() { cpu_actual }
        else if control.cpu_coin.is_empty() { "auto".to_string() }
        else { control.cpu_coin.clone() };

    wborder!(mid);
    // Status line with colored dot
    let rest_after_dot = format!(
        " algo={:<12} mode={:<4} thr={:<2} CPU={:<4} GPU={:<4}",
        algo_display(&control.algorithm), mode_str, control.threads, cpu_coin, gpu_coin,
    );
    let dot_len = status_dot.chars().count();
    let pad_needed = iw.saturating_sub(dot_len + 1 + rest_after_dot.chars().count());
    let mut padded_rest = rest_after_dot;
    for _ in 0..pad_needed { padded_rest.push(' '); }
    // Truncate if too long
    while (dot_len + 1 + padded_rest.chars().count()) > iw {
        padded_rest.pop();
    }
    queue!(
        out,
        Print("\r│ "),
        SetForegroundColor(status_color),
        Print(status_dot),
        ResetColor,
        Print(&padded_rest),
        Print("│\n"),
    )?;

    // ── Hashrate section ──
    wborder!(mid);
    wheader!("HASHRATE");

    let (v10, u10) = ui::fmt_hashrate(rates.total_10s_hps);
    let (v60, u60) = ui::fmt_hashrate(rates.total_60s_hps);
    let (v15m, u15m) = ui::fmt_hashrate(rates.total_15m_hps);
    queue!(
        out,
        SetForegroundColor(Color::White),
        Print(format!("\r│{}│\n", pad_to(&format!(
            "   10s {:>6.2}{}  60s {:>6.2}{}  15m {:>6.2}{}",
            v10, u10, v60, u60, v15m, u15m,
        ), iw))),
        ResetColor,
    )?;

    // Sparkline — only if we have enough vertical room (need >24 to avoid scroll)
    if avail_rows > 26 {
        let history = hashrate.get_hr_history();
        let spark = render_sparkline(&history, iw.saturating_sub(6));
        queue!(
            out,
            SetForegroundColor(Color::Green),
            Print(format!("\r│{}│\n", pad_to(&format!("   {}", spark), iw))),
            ResetColor,
        )?;
    }

    // ── Streams section ──
    wborder!(mid);
    wheader!("STREAMS");

    // Stream 1: ZION
    let zion_total = rates.zion_accepted + rates.zion_rejected;
    let zion_pct = if zion_total > 0 { rates.zion_accepted as f64 * 100.0 / zion_total as f64 } else { 100.0 };
    let (zh, zu) = ui::fmt_hashrate(rates.zion_10s_hps);
    let s1_content = format!(
        " #1 ZION {:<12} {:>7.2}{} {:>3}/{:<2} ({:>4.1}%)",
        algo_display(&control.algorithm), zh, zu,
        rates.zion_accepted, rates.zion_rejected, zion_pct,
    );
    let s1_padded = pad_to(&s1_content, iw);
    let s1_label = "#1 ZION";
    let s1_after: String = s1_padded.chars().skip(s1_label.len() + 1).collect();
    queue!(
        out,
        Print("\r│ "),
        SetForegroundColor(Color::White),
        Print(s1_label),
        ResetColor,
        SetForegroundColor(Color::Grey),
        Print(&s1_after),
        ResetColor,
        Print("│\n"),
    )?;

    // Stream 2: GPU PROFIT
    let gpu_total = rates.gpu_ext_accepted + rates.gpu_ext_rejected;
    let gpu_pct = if gpu_total > 0 { rates.gpu_ext_accepted as f64 * 100.0 / gpu_total as f64 } else { 100.0 };
    let gpu_active = hashrate.gpu_ext_active.load(Ordering::Relaxed) == 1;
    let (gh, gu) = ui::fmt_hashrate(rates.gpu_ext_10s_hps);
    let gpu_hr = if gpu_active { format!("{:>8.2} {:<3}", gh, gu) } else { "    idle       ".to_string() };
    let s2_content = format!(
        " #2 GPU {:<8} {} {:>3}/{:<2} ({:>4.1}%) coin={:<4}",
        if gpu_active { "—" } else { "idle" }, gpu_hr,
        rates.gpu_ext_accepted, rates.gpu_ext_rejected, gpu_pct, gpu_coin,
    );
    let s2_padded = pad_to(&s2_content, iw);
    let s2_label = "#2 GPU";
    let s2_after: String = s2_padded.chars().skip(s2_label.len() + 1).collect();
    queue!(
        out,
        Print("\r│ "),
        SetForegroundColor(Color::Magenta),
        Print(s2_label),
        ResetColor,
        SetForegroundColor(Color::Grey),
        Print(&s2_after),
        ResetColor,
        Print("│\n"),
    )?;

    // Stream 3: CPU PROFIT
    let cpu_total = rates.cpu_ext_accepted + rates.cpu_ext_rejected;
    let cpu_pct = if cpu_total > 0 { rates.cpu_ext_accepted as f64 * 100.0 / cpu_total as f64 } else { 100.0 };
    let cpu_active = hashrate.cpu_ext_active.load(Ordering::Relaxed) == 1;
    let (ch, cu) = ui::fmt_hashrate(rates.cpu_ext_10s_hps);
    let cpu_hr = if cpu_active { format!("{:>8.2} {:<3}", ch, cu) } else { "    idle       ".to_string() };
    let cpu_algo = hashrate.cpu_ext_algorithm.lock().map(|a| a.clone()).unwrap_or_default();
    let cpu_algo_d = if cpu_algo.is_empty() { "—" } else { algo_display(&cpu_algo) };
    let s3_content = format!(
        " #3 CPU {:<12} {} {:>3}/{:<2} ({:>4.1}%) coin={:<4}",
        cpu_algo_d, cpu_hr,
        rates.cpu_ext_accepted, rates.cpu_ext_rejected, cpu_pct, cpu_coin,
    );
    let s3_padded = pad_to(&s3_content, iw);
    let s3_label = "#3 CPU";
    let s3_after: String = s3_padded.chars().skip(s3_label.len() + 1).collect();
    queue!(
        out,
        Print("\r│ "),
        SetForegroundColor(Color::Yellow),
        Print(s3_label),
        ResetColor,
        SetForegroundColor(Color::Grey),
        Print(&s3_after),
        ResetColor,
        Print("│\n"),
    )?;

    // ── Shares section ──
    wborder!(mid);
    wheader!("SHARES");

    let acc = rates.accepted;
    let rej = rates.rejected;
    let total_shares = acc + rej;
    let share_pct = if total_shares > 0 { acc as f64 * 100.0 / total_shares as f64 } else { 100.0 };
    let acc_s = format!("   Acc:{:>3}", acc);
    let rej_s = format!(" Rej:{:>3}", rej);
    let rest_s = format!(" Eff:{:>5.1}% Up:{}", share_pct, fmt_hms(uptime_secs));
    let total_len = acc_s.chars().count() + rej_s.chars().count() + rest_s.chars().count();
    let pad_right = " ".repeat(iw.saturating_sub(total_len));
    queue!(
        out,
        Print("\r│"),
        SetForegroundColor(Color::Green),
        Print(&acc_s),
        SetForegroundColor(Color::Red),
        Print(&rej_s),
        ResetColor,
        Print(&rest_s),
        Print(&pad_right),
        Print("│\n"),
    )?;

    // Share log — adaptive count based on available rows
    let share_log = hashrate.get_share_log();
    let max_log_entries = if avail_rows >= 40 { 5 }
        else if avail_rows >= 32 { 3 }
        else if avail_rows >= 27 { 2 }
        else { 1 };
    for entry in share_log.iter().rev().take(max_log_entries) {
        let sym = if entry.accepted { "+" } else { "x" };
        let sym_col = if entry.accepted { Color::Green } else { Color::Red };
        let word = if entry.accepted { "OK " } else { "REJ" };
        let reason = if entry.reason.is_empty() {
            String::new()
        } else {
            let r: String = entry.reason.chars().take(18).collect();
            format!(" {}", r)
        };
        let time_str = fmt_time(entry.timestamp);
        // Build content: "  [HH:MM:SS] + OK  stream=ZION  job=123  reason"
        let content = format!(
            "  [{}] {} {} stream={:<4} job={}{}",
            time_str, sym, word, entry.stream, entry.job_id, reason,
        );
        let padded = pad_to(&content, iw);
        // Find position of sym char in padded string for coloring
        let sym_pos = format!("  [{}] ", time_str).chars().count();
        let before: String = padded.chars().take(sym_pos).collect();
        let after: String = padded.chars().skip(sym_pos + 1).collect();
        queue!(
            out,
            Print("\r│"),
            Print(&before),
            SetForegroundColor(sym_col),
            Print(sym),
            ResetColor,
            Print(&after),
            Print("│\n"),
        )?;
    }
    if share_log.is_empty() {
        queue!(
            out,
            SetForegroundColor(Color::DarkGrey),
            Print(format!("\r│{}│\n", pad_to("   (no shares yet)", iw))),
            ResetColor,
        )?;
    }

    // ── Pool section ──
    wborder!(mid);
    wheader!("POOL");

    let online = hashrate.online_snapshot.lock().map(|g| g.clone()).unwrap_or_default();
    let (phr, phu) = ui::fmt_hashrate(online.pool_hashrate);
    queue!(
        out,
        SetForegroundColor(Color::White),
        Print(format!("\r│{}│\n", pad_to(&format!(
            "   h={} up={} pHR={:>6.2}{} min={}/{}",
            pool_height, fmt_hms(uptime_secs), phr, phu, online.active_miners, online.total_miners,
        ), iw))),
        ResetColor,
    )?;

    // ── Metrics (if enabled) ──
    if control.show_metrics {
        wborder!(mid);
        wheader!("METRICS");
        let tui = metrics.lock().map(|m| m.as_tui()).unwrap_or_default();
        wline!(&format!(
            "   lat avg/max: {:.0}/{:?}ms  batch={}ms  ttl={}ms  peak={:.1}",
            tui.submit_avg_ms, tui.submit_max_ms, tui.best_batch_ms, tui.remote_ttl_ms, tui.hashrate_max,
        ));
        wline!(&format!(
            "   iter={}  threads={}  nonce={}  status={}  backend={}",
            tui.current_iteration, tui.threads, tui.nonce_window, tui.status, tui.backend,
        ));
    }

    // ── Online best miners (if enabled) ──
    if control.show_online {
        wborder!(mid);
        wheader!("ONLINE BEST MINERS");
        if online.top_miners.is_empty() {
            queue!(
                out,
                SetForegroundColor(Color::DarkGrey),
                Print(format!("\r│{}│\n", pad_to("   (waiting for pool API...)", iw))),
                ResetColor,
            )?;
        } else {
            for (i, m) in online.top_miners.iter().enumerate() {
                let (hr, unit) = ui::fmt_hashrate(m.hashrate);
                let worker = if m.worker.len() > 14 { &m.worker[..14] } else { &m.worker };
                let coin = if m.coin.len() > 8 { &m.coin[..8] } else { &m.coin };
                let algo = if m.algorithm.len() > 14 { &m.algorithm[..14] } else { &m.algorithm };
                wline!(&format!(
                    "   #{} {:<14} {:>8.2}{:<3} {:<8} {}",
                    i + 1, worker, hr, unit, coin, algo,
                ));
            }
        }
    }

    // ── GPU devices ── (skip if no devices and terminal is short)
    if !gpu_info.is_empty() || avail_rows >= 30 {
        wborder!(mid);
        if gpu_info.is_empty() {
            wline!("   GPU: (no devices)");
        } else {
            for g in gpu_info.iter().take(2) {
                wline!(&format!("   GPU #{:<2} {}", g.index, g.info));
            }
        }
    }

    // ── Hotkeys ──
    wborder!(mid);
    queue!(
        out,
        SetForegroundColor(Color::DarkGrey),
        Print(format!("\r│{}│\n", pad_to(" [a]lgo [c]CPU [C]coinCPU [g]GPU [G]coinGPU [d]ual [p]ause", iw))),
        Print(format!("\r│{}│\n", pad_to(" [r]econ [m]etrics [o]nline [v]erbose [1-9]thr [q]uit", iw))),
        ResetColor,
    )?;

    // ── Bottom border (no trailing \n to avoid terminal scroll) ──
    queue!(out, Print(format!("\r{}", bot)))?;

    out.flush()?;

    // Count lines written this frame for next frame's cursor-up repositioning
    // Each \n in the output = one line. Bottom border has no \n = still 1 line.
    let lines_this_frame: usize = {
        // Count based on sections rendered
        let mut n = 0usize;
        n += 1; // top border
        n += 1; // title
        n += 1; // mid (before status)
        n += 1; // status
        n += 1; // mid (before hashrate)
        n += 1; // HASHRATE header
        n += 1; // hashrate values
        if avail_rows > 26 { n += 1; } // sparkline
        n += 1; // mid (before streams)
        n += 1; // STREAMS header
        n += 1; // stream 1
        n += 1; // stream 2
        n += 1; // stream 3
        n += 1; // mid (before shares)
        n += 1; // SHARES header
        n += 1; // shares summary
        // share log entries
        if share_log.is_empty() {
            n += 1; // "(no shares yet)" placeholder
        } else {
            n += share_log.len().min(max_log_entries);
        }
        n += 1; // mid (before pool)
        n += 1; // POOL header
        n += 1; // pool content
        if control.show_metrics { n += 4; } // metrics section
        if control.show_online { n += 3; } // online section (minimum)
        if !gpu_info.is_empty() || avail_rows >= 30 { n += 2; } // GPU section
        n += 1; // mid (before hotkeys)
        n += 2; // hotkeys (2 lines)
        n += 1; // bottom border (no \n but still a line)
        n
    };
    PREV_FRAME_LINES.store(lines_this_frame, Ordering::Relaxed);

    Ok(())
}

struct DashboardFrame<'a> {
    out: &'a mut TtyWriter,
    width: usize,
    lines: usize,
}

impl<'a> DashboardFrame<'a> {
    fn new(out: &'a mut TtyWriter, width: usize) -> Self {
        Self { out, width, lines: 0 }
    }

    fn top(&mut self) -> io::Result<()> {
        queue!(
            self.out,
            SetForegroundColor(Color::DarkGrey),
            Print(format!("\r┌{}┐\n", "─".repeat(self.width))),
            ResetColor,
        )?;
        self.lines += 1;
        Ok(())
    }

    fn title(&mut self, text: &str) -> io::Result<()> {
        let title = dashboard_center(text, self.width);
        queue!(
            self.out,
            SetBackgroundColor(Color::Rgb { r: 0, g: 42, b: 78 }),
            SetForegroundColor(Color::White),
            Print(format!("\r│{}│\n", title)),
            ResetColor,
        )?;
        self.lines += 1;
        Ok(())
    }

    fn rule(&mut self, label: &str, accent: Color) -> io::Result<()> {
        let label = format!(" {} ", dashboard_clip(label, self.width.saturating_sub(3)));
        let fill_len = self.width.saturating_sub(1 + label.chars().count());
        queue!(
            self.out,
            SetForegroundColor(Color::DarkGrey),
            Print("\r├─"),
            SetForegroundColor(accent),
            Print(label),
            SetForegroundColor(Color::DarkGrey),
            Print(format!("{}┤\n", "─".repeat(fill_len))),
            ResetColor,
        )?;
        self.lines += 1;
        Ok(())
    }

    fn plain(&mut self, text: &str, color: Color) -> io::Result<()> {
        self.parts(&[(color, text.to_string())])
    }

    fn parts(&mut self, parts: &[(Color, String)]) -> io::Result<()> {
        queue!(self.out, Print("\r│"))?;
        let mut used = 0usize;
        for (color, text) in parts {
            if used >= self.width {
                break;
            }
            let clipped = dashboard_clip(text, self.width - used);
            if clipped.is_empty() {
                continue;
            }
            used += clipped.chars().count();
            queue!(
                self.out,
                SetForegroundColor(color.clone()),
                Print(clipped),
            )?;
        }
        if used < self.width {
            queue!(
                self.out,
                ResetColor,
                Print(" ".repeat(self.width - used)),
            )?;
        }
        queue!(self.out, ResetColor, Print("│\n"))?;
        self.lines += 1;
        Ok(())
    }

    fn bottom(&mut self) -> io::Result<()> {
        queue!(
            self.out,
            SetForegroundColor(Color::DarkGrey),
            Print(format!("\r└{}┘", "─".repeat(self.width))),
            ResetColor,
        )?;
        self.lines += 1;
        Ok(())
    }
}

fn dashboard_clip(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

fn dashboard_pad(text: &str, width: usize) -> String {
    let clipped = dashboard_clip(text, width);
    format!("{}{}", clipped, " ".repeat(width.saturating_sub(clipped.chars().count())))
}

fn dashboard_center(text: &str, width: usize) -> String {
    let clipped = dashboard_clip(text, width);
    let padding = width.saturating_sub(clipped.chars().count());
    let left = padding / 2;
    format!("{}{}{}", " ".repeat(left), clipped, " ".repeat(padding - left))
}

fn dashboard_rate(hps: f64) -> String {
    let (value, unit) = ui::fmt_hashrate(hps);
    format!("{} {}", value, unit)
}

fn dashboard_short_algo(algo: &str) -> &str {
    match algo.to_ascii_lowercase().as_str() {
        "ghostrider" => "Ghostrider",
        "randomx" => "RandomX",
        "verushash" => "VerusHash",
        "kawpow" => "KawPow",
        "autolykos" => "Autolykos",
        "kheavyhash" => "KHeavyHash",
        _ => algo_display(algo),
    }
}

fn dashboard_efficiency(accepted: u64, rejected: u64) -> String {
    let total = accepted.saturating_add(rejected);
    if total == 0 {
        "-".to_string()
    } else {
        format!("{:.0}%", accepted as f64 * 100.0 / total as f64)
    }
}

fn dashboard_share_reason(reason: &str) -> String {
    let lower = reason.to_ascii_lowercase();
    if lower.contains("invalid share") {
        "invalid".to_string()
    } else if lower.contains("stale") {
        "stale".to_string()
    } else if lower.contains("job not found") {
        "job missing".to_string()
    } else {
        dashboard_clip(reason.trim(), 14)
    }
}

fn dashboard_stream_row(
    frame: &mut DashboardFrame<'_>,
    number: usize,
    label: &str,
    description: &str,
    rate_hps: f64,
    active: bool,
    accepted: u64,
    rejected: u64,
    label_color: Color,
) -> io::Result<()> {
    let prefix = format!(" {}  {:<9} ", number, label);
    let description = dashboard_pad(description, 18);
    let rate = dashboard_pad(
        &format!("{:>11}", if active { dashboard_rate(rate_hps) } else { "-".to_string() }),
        11,
    );
    let shares = format!("{:>5}", format!("{}/{}", accepted, rejected));
    let efficiency = format!("{:>5}", dashboard_efficiency(accepted, rejected));
    let share_color = if rejected > 0 { Color::Red } else if accepted > 0 { Color::Green } else { Color::DarkGrey };
    frame.parts(&[
        (label_color, prefix),
        (Color::DarkGrey, format!("{} ", description)),
        (if active { Color::White } else { Color::DarkGrey }, rate),
        (Color::DarkGrey, " ".to_string()),
        (share_color.clone(), shares),
        (Color::DarkGrey, " ".to_string()),
        (share_color, efficiency),
    ])
}

pub(crate) fn draw_dashboard_redesign(
    control: &MinerControl,
    rates: &ComputedHashrates,
    uptime_secs: u64,
    pool_height: u64,
    gpu_info: &[GpuInfoLine],
    metrics: &Arc<Mutex<crate::MinerMetricsSnapshot>>,
    hashrate: &Arc<HashrateTracker>,
) -> io::Result<()> {
    let mut out = tty();
    let previous_lines = PREV_FRAME_LINES.load(Ordering::Relaxed);
    if previous_lines > 0 {
        write!(out, "\x1b[{}A\r\x1b[J", previous_lines)?;
    } else {
        write!(out, "\x1b[2J\x1b[H")?;
    }

    let (term_cols, term_rows) = {
        let override_w = std::env::var("ZION_TUI_WIDTH")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|&w| w >= 40);
        if let Some(width) = override_w {
            let height = terminal::size().map(|(_, h)| h).unwrap_or(24);
            (width, height)
        } else {
            #[cfg(unix)]
            {
                let mut ws = libc_winsize {
                    ws_row: 0,
                    ws_col: 0,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                let tty_fd = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open("/dev/tty")
                    .ok();
                let got_size = tty_fd.as_ref().map(|file| {
                    use std::os::unix::io::AsRawFd;
                    unsafe { ioctl_tiocgwinsz(file.as_raw_fd(), &mut ws) == 0 }
                }).unwrap_or(false);
                if got_size && ws.ws_col > 0 && ws.ws_row > 0 {
                    (ws.ws_col, ws.ws_row)
                } else {
                    terminal::size().unwrap_or((80, 24))
                }
            }
            #[cfg(not(unix))]
            {
                terminal::size().unwrap_or((80, 24))
            }
        }
    };
    let width = (term_cols as usize).clamp(40, 80).saturating_sub(2);
    let available_rows = term_rows as usize;
    let show_metrics = control.show_metrics && available_rows >= 24;
    let show_online = control.show_online
        && available_rows >= 24
        && (!show_metrics || available_rows >= 32);
    let compact_panel = show_metrics || show_online;
    let show_sparkline = !compact_panel && available_rows >= 24;
    let show_gpu = !compact_panel && (!gpu_info.is_empty() || available_rows >= 27);
    let max_log_entries = if compact_panel {
        1
    } else if available_rows >= 40 {
        5
    } else if available_rows >= 32 {
        4
    } else if available_rows >= 27 {
        3
    } else {
        2
    };
    let tui_metrics = metrics.lock().map(|snapshot| snapshot.as_tui()).unwrap_or_default();
    let peak_hashrate = if tui_metrics.hashrate_max > 0.0 {
        tui_metrics.hashrate_max
    } else {
        rates.total_10s_hps
    };

    let gpu_actual = hashrate.gpu_ext_coin.lock().map(|coin| coin.clone()).unwrap_or_default();
    let cpu_actual = hashrate.cpu_ext_coin.lock().map(|coin| coin.clone()).unwrap_or_default();
    let gpu_algo = hashrate.gpu_ext_algorithm.lock().map(|algo| algo.clone()).unwrap_or_default();
    let cpu_algo = hashrate.cpu_ext_algorithm.lock().map(|algo| algo.clone()).unwrap_or_default();
    let gpu_coin = if gpu_actual.is_empty() {
        if control.gpu_coin.is_empty() { "AUTO".to_string() } else { control.gpu_coin.clone() }
    } else {
        gpu_actual
    };
    let cpu_coin = if cpu_actual.is_empty() {
        if control.cpu_coin.is_empty() { "AUTO".to_string() } else { control.cpu_coin.clone() }
    } else {
        cpu_actual
    };
    let gpu_active = hashrate.gpu_ext_active.load(Ordering::Relaxed) == 1;
    let cpu_active = hashrate.cpu_ext_active.load(Ordering::Relaxed) == 1;
    let online = hashrate.online_snapshot.lock().map(|snapshot| snapshot.clone()).unwrap_or_default();

    let mut frame = DashboardFrame::new(&mut out, width);
    frame.top()?;
    #[cfg(feature = "public_build")]
    let header_label = "ZION MINER";
    #[cfg(not(feature = "public_build"))]
    let header_label = "ZION MINER  |  TRIPLE STREAM";
    frame.title(&format!(
        "{}  |  {}",
        header_label,
        algo_display(&control.algorithm),
    ))?;

    let (status, status_color) = if control.pause {
        ("o PAUSED", Color::Yellow)
    } else {
        ("* RUNNING", Color::Green)
    };
    let mode = match control.mode {
        MiningMode::CpuOnly => "CPU",
        MiningMode::GpuOnly => "GPU",
        MiningMode::Dual => "DUAL",
    };
    let backend = if control.gpu_enabled && !gpu_info.is_empty() {
        "OPENCL"
    } else if control.gpu_enabled {
        "GPU"
    } else {
        "CPU"
    };
    frame.parts(&[
        (status_color, format!(" {}", status)),
        (Color::DarkGrey, format!("  {}  {}  UP {}", mode, backend, fmt_hms(uptime_secs))),
    ])?;

    frame.rule("HASHRATE", Color::Cyan)?;
    frame.parts(&[
        (Color::DarkGrey, "  10s ".to_string()),
        (Color::White, format!("{:>10}", dashboard_rate(rates.total_10s_hps))),
        (Color::DarkGrey, "   60s ".to_string()),
        (Color::White, format!("{:>10}", dashboard_rate(rates.total_60s_hps))),
        (Color::DarkGrey, "   peak ".to_string()),
        (Color::Green, format!("{:>10}", dashboard_rate(peak_hashrate))),
    ])?;
    if show_sparkline {
        let spark = render_sparkline(&hashrate.get_hr_history(), width.saturating_sub(8));
        frame.parts(&[(Color::DarkGrey, "  trend ".to_string()), (Color::Green, spark)])?;
    }

    frame.rule("STREAMS", Color::Magenta)?;
    frame.plain(
        &format!(" #  {:<9} {:<18} {:>11} {:>5} {:>5}", "STREAM", "ALGORITHM", "RATE", "A/R", "EFF"),
        Color::DarkGrey,
    )?;
    let zion_desc = algo_display(&control.algorithm).to_string();
    dashboard_stream_row(
        &mut frame,
        1,
        "ZION",
        &zion_desc,
        rates.zion_10s_hps,
        true,
        rates.zion_accepted,
        rates.zion_rejected,
        Color::White,
    )?;
    // In public_build, hide Stream 2 (GPU/ZANO) and Stream 3 (CPU/VRSC).
    // Triple Stream still runs internally — only the display is suppressed.
    #[cfg(not(feature = "public_build"))]
    {
    let gpu_desc = if gpu_active {
        format!("{} / {}", gpu_coin, dashboard_short_algo(&gpu_algo))
    } else {
        "idle".to_string()
    };
    dashboard_stream_row(
        &mut frame,
        2,
        "GPU",
        &gpu_desc,
        rates.gpu_ext_10s_hps,
        gpu_active,
        rates.gpu_ext_accepted,
        rates.gpu_ext_rejected,
        Color::Magenta,
    )?;
    let cpu_desc = if cpu_active {
        format!("{} / {}", cpu_coin, dashboard_short_algo(&cpu_algo))
    } else {
        "idle".to_string()
    };
    dashboard_stream_row(
        &mut frame,
        3,
        "CPU",
        &cpu_desc,
        rates.cpu_ext_10s_hps,
        cpu_active,
        rates.cpu_ext_accepted,
        rates.cpu_ext_rejected,
        Color::Yellow,
    )?;
    } // end not(public_build)

    frame.rule("SHARES", Color::Yellow)?;
    let share_total = rates.accepted.saturating_add(rates.rejected);
    let share_efficiency = if share_total > 0 {
        format!("{:.1}%", rates.accepted as f64 * 100.0 / share_total as f64)
    } else {
        "-".to_string()
    };
    frame.parts(&[
        (Color::DarkGrey, "  ".to_string()),
        (Color::Green, format!("ACCEPT {:>3}", rates.accepted)),
        (Color::DarkGrey, "   ".to_string()),
        (Color::Red, format!("REJECT {:>3}", rates.rejected)),
        (Color::DarkGrey, format!("   EFF {:>5}   UP {}", share_efficiency, fmt_hms(uptime_secs))),
    ])?;

    let share_log = hashrate.get_share_log();
    for entry in share_log.iter().rev().take(max_log_entries) {
        let symbol = if entry.accepted { "+" } else { "x" };
        let word = if entry.accepted { "OK" } else { "REJ" };
        let reason = dashboard_share_reason(&entry.reason);
        // In public_build, all shares display as "ZION" regardless of which
        // stream actually found them (Triple Stream runs silently).
        #[cfg(feature = "public_build")]
        let stream_label = "ZION";
        #[cfg(not(feature = "public_build"))]
        let stream_label = dashboard_clip(&entry.stream, 4);
        let tail = format!(
            " {} {:<4} job={:<5} {:>4}ms {}",
            word,
            stream_label,
            entry.job_id,
            entry.latency_ms,
            reason,
        );
        frame.parts(&[
            (Color::DarkGrey, format!("  [{}] ", fmt_time(entry.timestamp))),
            (if entry.accepted { Color::Green } else { Color::Red }, symbol.to_string()),
            (Color::DarkGrey, tail),
        ])?;
    }
    if share_log.is_empty() {
        frame.plain("  waiting for first share...", Color::DarkGrey)?;
    }

    frame.rule("POOL", Color::Blue)?;
    let (pool_rate, pool_unit) = ui::fmt_hashrate(online.pool_hashrate);
    frame.parts(&[
        (Color::DarkGrey, "  HEIGHT ".to_string()),
        (Color::White, pool_height.to_string()),
        (Color::DarkGrey, "   MINERS ".to_string()),
        (Color::White, format!("{}/{}", online.active_miners, online.total_miners)),
        (Color::DarkGrey, "   POOL ".to_string()),
        (Color::White, format!("{} {}", pool_rate, pool_unit)),
        (Color::DarkGrey, format!("   UP {}", fmt_hms(uptime_secs))),
    ])?;

    if show_gpu {
        frame.rule("HARDWARE", Color::Magenta)?;
        if gpu_info.is_empty() {
            frame.plain("  GPU  no device detected", Color::DarkGrey)?;
        } else {
            for gpu in gpu_info.iter().take(2) {
                frame.parts(&[
                    (Color::Magenta, format!("  GPU #{}  ", gpu.index)),
                    (Color::DarkGrey, dashboard_clip(&gpu.info, width.saturating_sub(10))),
                ])?;
            }
        }
    }

    if show_metrics {
        frame.rule("METRICS", Color::Cyan)?;
        frame.plain(
            &format!(
                "  latency {:>4}/{:<4}ms   batch {:>4}ms   ttl {:>5}ms",
                tui_metrics.submit_avg_ms.round() as u64,
                tui_metrics.submit_max_ms,
                tui_metrics.best_batch_ms,
                tui_metrics.remote_ttl_ms,
            ),
            Color::DarkGrey,
        )?;
        frame.plain(
            &format!(
                "  iter {:>5}   nonce {:>8}   backend {:<6}   status {}",
                tui_metrics.current_iteration,
                tui_metrics.nonce_window,
                dashboard_clip(&tui_metrics.backend, 6),
                dashboard_clip(&tui_metrics.status, 12),
            ),
            Color::DarkGrey,
        )?;
    }

    if show_online {
        frame.rule("ONLINE BEST", Color::Blue)?;
        if online.top_miners.is_empty() {
            frame.plain("  waiting for pool telemetry...", Color::DarkGrey)?;
        } else {
            frame.plain("  #  WORKER          RATE       COIN  ALGO", Color::DarkGrey)?;
            for (index, miner) in online.top_miners.iter().take(3).enumerate() {
                let (value, unit) = ui::fmt_hashrate(miner.hashrate);
                frame.plain(
                    &format!(
                        "  {:>1}  {:<14} {:>7.2} {:<3}  {:<5} {}",
                        index + 1,
                        dashboard_clip(&miner.worker, 14),
                        value,
                        unit,
                        dashboard_clip(&miner.coin, 5),
                        dashboard_clip(&miner.algorithm, width.saturating_sub(39)),
                    ),
                    Color::DarkGrey,
                )?;
            }
        }
    }

    frame.rule("CONTROLS", Color::DarkGrey)?;
    frame.plain("  [a]algo [p]pause [r]reconnect [q]quit", Color::DarkGrey)?;
    frame.plain("  [c/g]cpu/gpu [d]dual [C/G]coins [m/o]panels [1-9]threads", Color::DarkGrey)?;
    frame.bottom()?;

    let lines_this_frame = frame.lines;
    drop(frame);
    out.flush()?;
    PREV_FRAME_LINES.store(lines_this_frame, Ordering::Relaxed);
    Ok(())
}

/* ========================================================================= */
/* Pro-style dashboard helpers                                               */
/* ========================================================================= */

const SPARK_CHARS: &[char] = &['.', ':', '-', '=', '+', '*', '#', '@'];

fn render_sparkline(history: &[f64], width: usize) -> String {
    if history.is_empty() || width == 0 {
        return String::new();
    }
    let max = history.iter().cloned().fold(0.0f64, f64::max).max(0.001);
    let min = history.iter().cloned().fold(f64::INFINITY, f64::min);
    let range = (max - min).max(0.001);
    let start = if history.len() > width { history.len() - width } else { 0 };
    let samples = &history[start..];
    let mut s = String::with_capacity(width);
    for &v in samples {
        let normalized = ((v - min) / range * 7.0).round() as usize;
        s.push(SPARK_CHARS[normalized.min(7)]);
    }
    let pad = width.saturating_sub(samples.len());
    for _ in 0..pad {
        s.insert(0, SPARK_CHARS[0]);
    }
    s
}

/// Format elapsed seconds as HH:MM:SS.
fn fmt_hms(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

/// Format a timestamp age as HH:MM:SS.
fn fmt_time(ts: Instant) -> String {
    let age = Instant::now().duration_since(ts);
    let total = age.as_secs();
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
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
    PREV_FRAME_LINES.store(0, Ordering::Relaxed);

    terminal::enable_raw_mode()?;
    let mut tty_out = tty();
    execute!(tty_out, cursor::Hide, terminal::EnterAlternateScreen)?;

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
            tty(),
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

            // Sample hashrate history for sparkline graph
            dashboard_hashrate.sample_hr_history();
            let _ = draw_dashboard_redesign(
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
    execute!(tty(), cursor::Show, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    TUI_ACTIVE.store(false, Ordering::Relaxed);
    PREV_FRAME_LINES.store(0, Ordering::Relaxed);
    Ok(())
}
