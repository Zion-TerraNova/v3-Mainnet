//! Professional colored miner UI (XMRig / GMiner style)
//!
//! Uses ANSI escape codes for colors and cursor control.
//! Windows Terminal, MINGW64, and most modern consoles support these.

// Display-only helpers; several color codes / loggers are kept for completeness.
#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

/* ========================================================================= */
/* libc FFI declarations (minimal — just what we need for stdout redirect)   */
/* ========================================================================= */

#[cfg(target_family = "unix")]
extern "C" {
    fn dup(oldfd: i32) -> i32;
    fn dup2(oldfd: i32, newfd: i32) -> i32;
    fn close(fd: i32) -> i32;
    fn open(path: *const i8, flags: i32, ...) -> i32;
    fn write(fd: i32, buf: *const u8, count: usize) -> isize;
}

#[cfg(target_family = "unix")]
const O_WRONLY: i32 = 1;

/* ========================================================================= */
/* Sticky header state (Claymore-style fixed metrics + scrolling logs)       */
/* ========================================================================= */

static STICKY_ACTIVE: AtomicBool = AtomicBool::new(false);
static STICKY_LINES: AtomicUsize = AtomicUsize::new(0);

/// File descriptor for /dev/tty — used to write UI directly to the terminal,
/// bypassing stdout (which is redirected to /dev/null in sticky mode).
/// This prevents ALL println! calls from other threads from corrupting the display.
static TTY_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// Original stdout fd (saved before redirect) — restored on exit.
static SAVED_STDOUT_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// Ring buffer for recent log lines (displayed below the sticky header).
static LOG_RING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
const LOG_RING_MAX: usize = 200;

/// Add a log line to the ring buffer (called from println interceptor).
pub fn push_log_line(line: &str) {
    if !STICKY_ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    if let Ok(mut buf) = LOG_RING.lock() {
        if buf.len() >= LOG_RING_MAX {
            buf.remove(0);
        }
        buf.push(line.to_string());
    }
}

/// Reset sticky header state (call on reconnect / new session).
pub fn reset_sticky_header() {
    STICKY_ACTIVE.store(false, Ordering::SeqCst);
    STICKY_LINES.store(0, Ordering::SeqCst);
    if let Ok(mut buf) = LOG_RING.lock() {
        buf.clear();
    }
}

/// Write directly to /dev/tty (bypasses redirected stdout).
/// Falls back to stdout if /dev/tty is not available.
fn tty_write(s: &str) {
    #[cfg(target_family = "unix")]
    {
        let fd = TTY_FD.load(Ordering::SeqCst);
        if fd >= 0 {
            // SAFETY: write() is a simple syscall, fd is valid (opened once)
            unsafe {
                write(fd, s.as_ptr(), s.len());
            }
            return;
        }
    }
    // Fallback: use stdout (also used on non-Unix platforms)
    let _ = io::stdout().write_all(s.as_bytes());
    let _ = io::stdout().flush();
}

/// Redirect stdout (fd 1) to /dev/null so that ALL println! calls from
/// any thread go nowhere and cannot corrupt the sticky header display.
/// UI output is written directly to /dev/tty via `tty_write()`.
fn redirect_stdout_to_null() {
    #[cfg(target_family = "unix")]
    {
        // Save original stdout
        let saved = unsafe { dup(1) };
        SAVED_STDOUT_FD.store(saved, Ordering::SeqCst);

        // Open /dev/null for writing
        let null_fd = unsafe {
            open(b"/dev/null\0".as_ptr() as *const i8, O_WRONLY)
        };
        if null_fd >= 0 {
            // Redirect fd 1 (stdout) to /dev/null
            unsafe {
                dup2(null_fd, 1);
                close(null_fd);
            }
        }
    }
}

/// Restore original stdout (call on exit to restore normal terminal behavior).
fn restore_stdout() {
    #[cfg(target_family = "unix")]
    {
        let saved = SAVED_STDOUT_FD.swap(-1, Ordering::SeqCst);
        if saved >= 0 {
            unsafe {
                dup2(saved, 1);
                close(saved);
            }
        }
    }
}

/// Open /dev/tty for direct UI output.
fn open_tty() {
    #[cfg(target_family = "unix")]
    {
        let fd = unsafe {
            open(b"/dev/tty\0".as_ptr() as *const i8, O_WRONLY)
        };
        TTY_FD.store(fd, Ordering::SeqCst);
    }
}

/// Close /dev/tty fd.
fn close_tty() {
    #[cfg(target_family = "unix")]
    {
        let fd = TTY_FD.swap(-1, Ordering::SeqCst);
        if fd >= 0 {
            unsafe {
                close(fd);
            }
        }
    }
}

/* ========================================================================= */
/* ANSI color codes                                                          */
/* ========================================================================= */

pub const RESET: &str = "\x1B[0m";
pub const BOLD: &str = "\x1B[1m";
pub const DIM: &str = "\x1B[2m";
pub const UNDERLINE: &str = "\x1B[4m";

pub const BLACK: &str = "\x1B[30m";
pub const RED: &str = "\x1B[31m";
pub const GREEN: &str = "\x1B[32m";
pub const YELLOW: &str = "\x1B[33m";
pub const BLUE: &str = "\x1B[34m";
pub const MAGENTA: &str = "\x1B[35m";
pub const CYAN: &str = "\x1B[36m";
pub const WHITE: &str = "\x1B[37m";

pub const BRIGHT_BLACK: &str = "\x1B[90m";
pub const BRIGHT_RED: &str = "\x1B[91m";
pub const BRIGHT_GREEN: &str = "\x1B[92m";
pub const BRIGHT_YELLOW: &str = "\x1B[93m";
pub const BRIGHT_BLUE: &str = "\x1B[94m";
pub const BRIGHT_MAGENTA: &str = "\x1B[95m";
pub const BRIGHT_CYAN: &str = "\x1B[96m";
pub const BRIGHT_WHITE: &str = "\x1B[97m";

pub const CLEAR_LINE: &str = "\x1B[2K";
pub const CURSOR_UP: &str = "\x1B[1A";
pub const CURSOR_HIDE: &str = "\x1B[?25l";
pub const CURSOR_SHOW: &str = "\x1B[?25h";

// Alternate screen buffer (smcup/rmcup) — full screen takeover like Claymore
pub const ENTER_ALT_SCREEN: &str = "\x1B[?1049h";
pub const EXIT_ALT_SCREEN: &str = "\x1B[?1049l";

// Scroll region / cursor save-restore (DECSTBM + DECSC/DECRC)
pub const SAVE_CURSOR: &str = "\x1B7";      // DECSC — save cursor + attributes
pub const RESTORE_CURSOR: &str = "\x1B8";   // DECRC — restore cursor + attributes
pub const CLEAR_SCREEN: &str = "\x1B[2J";
pub const CLEAR_FROM_CURSOR: &str = "\x1B[J"; // clear from cursor to end of screen
pub const HOME: &str = "\x1B[H";              // cursor to top-left
pub const RESET_SCROLL_REGION: &str = "\x1B[r"; // full screen scroll region

/* ========================================================================= */
/* Helpers                                                                   */
/* ========================================================================= */

fn print_flush(s: &str) {
    if STICKY_ACTIVE.load(Ordering::SeqCst) {
        // In sticky mode: write to /dev/tty (stdout is redirected to /dev/null)
        tty_write(s);
    } else {
        let _ = io::stdout().write_all(s.as_bytes());
        let _ = io::stdout().flush();
    }
}

/// Log a line: in sticky mode, push to ring buffer; otherwise print directly.
/// Use this for ALL log/event functions (shares, jobs, connections, etc.)
fn log_or_sticky(s: &str) {
    if STICKY_ACTIVE.load(Ordering::SeqCst) {
        push_log_line(s);
    } else {
        print_flush(s);
    }
}

/// Auto-scale hashrate: pick best unit (H/s, KH/s, MH/s, GH/s, TH/s)
pub fn fmt_hashrate(hps: f64) -> (String, &'static str) {
    if hps >= 1e12 {
        (format!("{:.2}", hps / 1e12), "TH/s")
    } else if hps >= 1e9 {
        (format!("{:.2}", hps / 1e9), "GH/s")
    } else if hps >= 1e6 {
        (format!("{:.2}", hps / 1e6), "MH/s")
    } else if hps >= 1e3 {
        (format!("{:.2}", hps / 1e3), "KH/s")
    } else {
        (format!("{:.2}", hps), "H/s")
    }
}

/// Format uptime as HH:MM:SS
pub fn fmt_uptime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{:02}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}

/* ========================================================================= */
/* Colored share / event logging                                             */
/* ========================================================================= */

/// Print an accepted share line (green +)
pub fn log_accepted(job_id: u64, height: u64, nonce: u64, latency_ms: u64) {
    let mut s = String::new();
    s.push_str(GREEN);
    s.push('+');
    s.push_str(RESET);
    s.push(' ');
    s.push_str(CYAN);
    s.push_str("job=");
    s.push_str(RESET);
    s.push_str(&job_id.to_string());
    s.push(' ');
    s.push_str(YELLOW);
    s.push_str("height=");
    s.push_str(RESET);
    s.push_str(&height.to_string());
    s.push(' ');
    s.push_str(GREEN);
    s.push_str("nonce=");
    s.push_str(RESET);
    s.push_str(&nonce.to_string());
    s.push(' ');
    s.push_str(MAGENTA);
    s.push_str("latency=");
    s.push_str(RESET);
    s.push_str(&latency_ms.to_string());
    s.push_str("ms\n");
    // In sticky mode: push to ring buffer (displayed below header on next redraw)
    // Otherwise: print directly
    if STICKY_ACTIVE.load(Ordering::SeqCst) {
        push_log_line(&s);
    } else {
        print_flush(&s);
    }
}

/// Print a rejected share line (red -)
pub fn log_rejected(job_id: u64, height: u64, nonce: u64, latency_ms: u64, reason: &str) {
    let mut s = String::new();
    s.push_str(RED);
    s.push('-');
    s.push_str(RESET);
    s.push(' ');
    s.push_str(CYAN);
    s.push_str("job=");
    s.push_str(RESET);
    s.push_str(&job_id.to_string());
    s.push(' ');
    s.push_str(YELLOW);
    s.push_str("height=");
    s.push_str(RESET);
    s.push_str(&height.to_string());
    s.push(' ');
    s.push_str(RED);
    s.push_str("nonce=");
    s.push_str(RESET);
    s.push_str(&nonce.to_string());
    s.push(' ');
    s.push_str(MAGENTA);
    s.push_str("latency=");
    s.push_str(RESET);
    s.push_str(&latency_ms.to_string());
    s.push_str("ms ");
    s.push_str(RED);
    s.push_str("reason=");
    s.push_str(RESET);
    s.push_str(reason);
    s.push('\n');
    if STICKY_ACTIVE.load(Ordering::SeqCst) {
        push_log_line(&s);
    } else {
        print_flush(&s);
    }
}

/// Print an accepted external share (Claymore-style, per-stream)
/// stream_label: "GPU PROFIT" or "CPU PROFIT"
pub fn log_ext_accepted(stream_label: &str, coin: &str, algorithm: &str, latency_ms: u64) {
    let color = match stream_label {
        "GPU PROFIT" => BRIGHT_YELLOW,
        "CPU PROFIT" => BRIGHT_GREEN,
        _ => GREEN,
    };
    let mut s = String::new();
    s.push_str(GREEN);
    s.push('+');
    s.push_str(RESET);
    s.push(' ');
    s.push_str(color);
    s.push_str(BOLD);
    s.push_str(&format!("[{}]", stream_label));
    s.push_str(RESET);
    s.push(' ');
    s.push_str(CYAN);
    s.push_str("coin=");
    s.push_str(RESET);
    s.push_str(coin);
    s.push(' ');
    s.push_str(DIM);
    s.push_str("algo=");
    s.push_str(RESET);
    s.push_str(algorithm);
    s.push(' ');
    s.push_str(MAGENTA);
    s.push_str("latency=");
    s.push_str(RESET);
    s.push_str(&latency_ms.to_string());
    s.push_str("ms ");
    s.push_str(GREEN);
    s.push_str("ACCEPTED");
    s.push_str(RESET);
    s.push('\n');
    if STICKY_ACTIVE.load(Ordering::SeqCst) {
        push_log_line(&s);
    } else {
        print_flush(&s);
    }
}

/// Print a rejected external share (Claymore-style, per-stream)
pub fn log_ext_rejected(stream_label: &str, coin: &str, algorithm: &str, reason: &str) {
    let color = match stream_label {
        "GPU PROFIT" => BRIGHT_YELLOW,
        "CPU PROFIT" => BRIGHT_GREEN,
        _ => RED,
    };
    let mut s = String::new();
    s.push_str(RED);
    s.push('-');
    s.push_str(RESET);
    s.push(' ');
    s.push_str(color);
    s.push_str(BOLD);
    s.push_str(&format!("[{}]", stream_label));
    s.push_str(RESET);
    s.push(' ');
    s.push_str(CYAN);
    s.push_str("coin=");
    s.push_str(RESET);
    s.push_str(coin);
    s.push(' ');
    s.push_str(DIM);
    s.push_str("algo=");
    s.push_str(RESET);
    s.push_str(algorithm);
    s.push(' ');
    s.push_str(RED);
    s.push_str("REJECTED");
    s.push_str(RESET);
    s.push(' ');
    s.push_str(RED);
    s.push_str("reason=");
    s.push_str(RESET);
    s.push_str(reason);
    s.push('\n');
    if STICKY_ACTIVE.load(Ordering::SeqCst) {
        push_log_line(&s);
    } else {
        print_flush(&s);
    }
}

/// Print a block found celebration with ASCII art flag
pub fn log_block_found(height: u64, nonce: u64, hash_prefix: &str) {
    let flag = r#"
                                    ╔══════════════════════════════════════════════════════════════════╗
                                    ║                    🐋 KEPORKAK BLOCK FOUND! 🐋                   ║
                                    ║  ════════════════════════════════════════════════════════════════ ║
                                    ║  ███╗   ██╗███████╗██╗  ██╗    ██████╗ ██████╗ ██████╗ ███████╗   ║
                                    ║  ████╗  ██║██╔════╝██║ ██╔╝    ██╔══██╗██╔══██╗██╔══██╗██╔════╝   ║
                                    ║  ██╔██╗ ██║█████╗  █████╔╝     ██████╔╝███████║███████║███████╗   ║
                                    ║  ██║╚██╗██║██╔══╝  ██╔═██╗     ██╔══██╗██╔══██║██╔══██║╚════██║   ║
                                    ║  ██║ ╚████║███████╗██║  ██╗    ██████╔╝██║  ██║██║  ██║███████║   ║
                                    ║  ╚═╝  ╚═══╝╚══════╝╚═╝  ╚═╝    ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝   ║
                                    ║  ════════════════════════════════════════════════════════════════ ║
                                    ║  Height: {}  Nonce: {}  Hash: {}...                      ║
                                    ╚══════════════════════════════════════════════════════════════════╝"#;

    let flag_formatted = flag.replace("{}", &format!("{}  {}  {}", height, nonce, hash_prefix));

    let mut s = String::new();
    s.push_str(BRIGHT_YELLOW);
    s.push_str(&flag_formatted);
    s.push_str(RESET);
    s.push('\n');
    if STICKY_ACTIVE.load(Ordering::SeqCst) {
        push_log_line(&s);
    } else {
        print_flush(&s);
    }
}

/// Print a new-job notification (blue arrow)
pub fn log_new_job(job_id: u64, height: u64, algorithm: &str, difficulty: u64) {
    let mut s = String::new();
    s.push_str(BLUE);
    s.push_str(">>");
    s.push_str(RESET);
    s.push(' ');
    s.push_str(CYAN);
    s.push_str("new job");
    s.push_str(RESET);
    s.push(' ');
    s.push_str(DIM);
    s.push('#');
    s.push_str(RESET);
    s.push_str(&job_id.to_string());
    s.push(' ');
    s.push_str(YELLOW);
    s.push_str("height=");
    s.push_str(RESET);
    s.push_str(&height.to_string());
    s.push(' ');
    s.push_str(BLUE);
    s.push_str("algo=");
    s.push_str(RESET);
    s.push_str(algorithm);
    s.push(' ');
    s.push_str(MAGENTA);
    s.push_str("diff=");
    s.push_str(RESET);
    s.push_str(&difficulty.to_string());
    s.push('\n');
    if STICKY_ACTIVE.load(Ordering::SeqCst) {
        push_log_line(&s);
    } else {
        print_flush(&s);
    }
}

/// Print GPU epoch update
pub fn log_epoch_update(epoch: u64, height: u64) {
    let mut s = String::new();
    s.push_str(YELLOW);
    s.push('!');
    s.push_str(RESET);
    s.push(' ');
    s.push_str(DIM);
    s.push_str("epoch update");
    s.push_str(RESET);
    s.push(' ');
    s.push_str(YELLOW);
    s.push_str("epoch=");
    s.push_str(RESET);
    s.push_str(&epoch.to_string());
    s.push(' ');
    s.push_str(YELLOW);
    s.push_str("height=");
    s.push_str(RESET);
    s.push_str(&height.to_string());
    s.push('\n');
    log_or_sticky(&s);
}

/// Print connection status
pub fn log_connecting(pool: &str) {
    let mut s = String::new();
    s.push_str(YELLOW);
    s.push('*');
    s.push_str(RESET);
    s.push(' ');
    s.push_str(DIM);
    s.push_str("connecting to");
    s.push_str(RESET);
    s.push(' ');
    s.push_str(pool);
    s.push_str(" …\n");
    log_or_sticky(&s);
}

pub fn log_connected(pool: &str, latency_ms: u64) {
    let mut s = String::new();
    s.push_str(GREEN);
    s.push('*');
    s.push_str(RESET);
    s.push(' ');
    s.push_str(DIM);
    s.push_str("connected to");
    s.push_str(RESET);
    s.push(' ');
    s.push_str(pool);
    s.push(' ');
    s.push_str(GREEN);
    s.push_str("latency=");
    s.push_str(RESET);
    s.push_str(&latency_ms.to_string());
    s.push_str("ms\n");
    log_or_sticky(&s);
}

pub fn log_disconnected(pool: &str) {
    let mut s = String::new();
    s.push_str(RED);
    s.push('*');
    s.push_str(RESET);
    s.push(' ');
    s.push_str(DIM);
    s.push_str("disconnected from");
    s.push_str(RESET);
    s.push(' ');
    s.push_str(pool);
    s.push('\n');
    log_or_sticky(&s);
}

/* ========================================================================= */
/* Speed / stats table (XMRig style)                                         */
/* ========================================================================= */

/// Print a full-color status table (overwrites previous table if cursor is moved up).
/// `gpu_infos` = list of (device_name, compute_units, vram_mb, clock_mhz, temp_c, power_w)
///  temp_c and power_w may be None if not available.
pub fn print_speed_table(
    uptime_secs: u64,
    hr_10s: f64,
    hr_60s: f64,
    hr_15m: f64,
    hr_max: f64,
    accepted: u64,
    rejected: u64,
    attempted: u64,
    submit_avg: f64,
    submit_max: u64,
    pool_height: u64,
    current_epoch: u64,
    algorithm: &str,
    gpu_infos: &[(String, u32, u64, u32, Option<u32>, Option<u32>)],
) {
    let uptime = fmt_uptime(uptime_secs);
    let total = accepted + rejected;
    let accept_pct = if total > 0 {
        accepted as f64 * 100.0 / total as f64
    } else {
        100.0
    };

    let (v10, u10) = fmt_hashrate(hr_10s);
    let (v60, u60) = fmt_hashrate(hr_60s);
    let (v15, u15) = fmt_hashrate(hr_15m);
    let (vmx, umx) = fmt_hashrate(hr_max);

    // ── Speed line ──
    let mut s = String::new();
    s.push_str(BOLD);
    s.push_str(WHITE);
    s.push_str("speed");
    s.push_str(RESET);
    s.push_str("  ");
    s.push_str(CYAN);
    s.push_str(&format!("{:>8}", v10));
    s.push(' ');
    s.push_str(u10);
    s.push_str(RESET);
    s.push_str("  ");
    s.push_str(DIM);
    s.push_str(&format!("{:>8}", v60));
    s.push(' ');
    s.push_str(u60);
    s.push_str(RESET);
    s.push_str("  ");
    s.push_str(DIM);
    s.push_str(&format!("{:>8}", v15));
    s.push(' ');
    s.push_str(u15);
    s.push_str(RESET);
    s.push_str("  ");
    s.push_str(YELLOW);
    s.push_str("max");
    s.push_str(RESET);
    s.push(' ');
    s.push_str(GREEN);
    s.push_str(&format!("{:>8}", vmx));
    s.push(' ');
    s.push_str(umx);
    s.push_str(RESET);
    s.push('\n');
    print_flush(&s);

    // ── Shares line ──
    let rej_col = if rejected > 0 { BRIGHT_RED } else { DIM };
    let mut s2 = String::new();
    s2.push_str(BOLD);
    s2.push_str(WHITE);
    s2.push_str("shares");
    s2.push_str(RESET);
    s2.push(' ');
    s2.push_str(GREEN);
    s2.push_str(&accepted.to_string());
    s2.push_str(RESET);
    s2.push('/');
    s2.push_str(rej_col);
    s2.push_str(&rejected.to_string());
    s2.push_str(RESET);
    s2.push(' ');
    s2.push_str(DIM);
    s2.push('(');
    s2.push_str(&format!("{:.1}", accept_pct));
    s2.push_str("%)");
    s2.push_str(RESET);
    s2.push_str("  ");
    s2.push_str(WHITE);
    s2.push_str("hashes");
    s2.push_str(RESET);
    s2.push(' ');
    s2.push_str(&attempted.to_string());
    s2.push_str("  ");
    s2.push_str(MAGENTA);
    s2.push_str("pool latency");
    s2.push_str(RESET);
    s2.push(' ');
    s2.push_str(&format!("{:.0} / {:.0} ms", submit_avg, submit_max));
    s2.push_str("  ");
    s2.push_str(WHITE);
    s2.push_str("uptime");
    s2.push_str(RESET);
    s2.push(' ');
    s2.push_str(&uptime);
    s2.push('\n');
    print_flush(&s2);

    // ── Algorithm / epoch / height ──
    let mut s3 = String::new();
    s3.push_str(DIM);
    s3.push_str("algo=");
    s3.push_str(RESET);
    s3.push_str(algorithm);
    s3.push_str("  ");
    s3.push_str(DIM);
    s3.push_str("epoch=");
    s3.push_str(RESET);
    s3.push_str(&current_epoch.to_string());
    s3.push_str("  ");
    s3.push_str(DIM);
    s3.push_str("height=");
    s3.push_str(RESET);
    s3.push_str(&pool_height.to_string());
    s3.push('\n');
    print_flush(&s3);

    // ── GPU details ──
    if !gpu_infos.is_empty() {
        let mut s4 = String::new();
        s4.push_str(BOLD);
        s4.push_str(WHITE);
        s4.push_str("gpu");
        s4.push_str(RESET);
        s4.push('\n');
        print_flush(&s4);
        for (i, (name, cu, vram, clock, temp, power)) in gpu_infos.iter().enumerate() {
            let vram_gb = *vram as f64 / 1024.0 / 1024.0 / 1024.0;
            let temp_str = match temp {
                Some(t) => format!("{}°C", t),
                None => "n/a".to_string(),
            };
            let power_str = match power {
                Some(p) => format!("{}W", p),
                None => "n/a".to_string(),
            };
            let mut s5 = String::new();
            s5.push_str("  ");
            s5.push_str(GREEN);
            s5.push('#');
            s5.push_str(&i.to_string());
            s5.push_str(RESET);
            s5.push(' ');
            s5.push_str(name);
            s5.push_str("  ");
            s5.push_str(DIM);
            s5.push_str("CU=");
            s5.push_str(RESET);
            s5.push_str(&cu.to_string());
            s5.push_str("  ");
            s5.push_str(DIM);
            s5.push_str("VRAM=");
            s5.push_str(RESET);
            s5.push_str(&format!("{:.1} GiB", vram_gb));
            s5.push_str("  ");
            s5.push_str(DIM);
            s5.push_str("clk=");
            s5.push_str(RESET);
            s5.push_str(&format!("{} MHz", clock));
            s5.push_str("  ");
            s5.push_str(DIM);
            s5.push_str("temp=");
            s5.push_str(RESET);
            s5.push_str(&temp_str);
            s5.push_str("  ");
            s5.push_str(DIM);
            s5.push_str("pwr=");
            s5.push_str(RESET);
            s5.push_str(&power_str);
            s5.push('\n');
            print_flush(&s5);
        }
    }
}

/* ========================================================================= */
/* Claymore-style Triple Stream stats (no-TUI mode)                          */
/* ========================================================================= */

/// Per-stream data for the triple-stream stats display.
pub struct StreamStats {
    pub label: &'static str,      // "ZION", "GPU PROFIT", "CPU PROFIT"
    pub coin: String,             // "ZION", "EPIC", "VRSC", etc.
    pub algorithm: String,        // "deeksha_lite_v1", "progpow", "verushash"
    pub hashrate_10s: f64,
    pub hashrate_60s: f64,
    pub hashrate_15m: f64,
    pub accepted: u64,
    pub rejected: u64,
    pub active: bool,             // is this stream currently mining?
}

/// Print a Claymore-style triple-stream stats block.
///
/// Example output:
/// ```
/// ┌─────────────────────────────────────────────────────────────────────────┐
/// │  ZION v3.0.6 Triple Stream                              uptime 01:23:45 │
/// ├─────────────────────────────────────────────────────────────────────────┤
/// │  STREAM 1  ZION       deeksha_lite_v1     12.34 MH/s  ████░░░  45/0  ✓  │
/// │  STREAM 2  GPU PROFIT EPIC / progpow      SKIPPED (DAG-based on Metal)   │
/// │  STREAM 3  CPU PROFIT VRSC / verushash    234.5 H/s  ██░░░░░  12/0  ✓  │
/// ├─────────────────────────────────────────────────────────────────────────┤
/// │  TOTAL      12.57 MH/s    57 accepted / 0 rejected  (100.0%)            │
/// │  pool 62.171.141.136:8444   height 3623114   latency 12/45 ms           │
/// └─────────────────────────────────────────────────────────────────────────┘
/// ```
pub fn print_triple_stream_stats(
    uptime_secs: u64,
    streams: &[StreamStats],
    total_accepted: u64,
    total_rejected: u64,
    pool_addr: &str,
    pool_height: u64,
    submit_avg_ms: f64,
    submit_max_ms: u64,
    gpu_infos: &[(String, u32, u64, u32, Option<u32>, Option<u32>)],
) {
    let s = build_triple_stream_box(
        uptime_secs,
        streams,
        total_accepted,
        total_rejected,
        pool_addr,
        pool_height,
        submit_avg_ms,
        submit_max_ms,
        gpu_infos,
    );
    print_flush(&s);
}

/// Build the triple-stream stats box as a string (no printing).
/// Returns (box_string, line_count).
fn build_triple_stream_box(
    uptime_secs: u64,
    streams: &[StreamStats],
    total_accepted: u64,
    total_rejected: u64,
    pool_addr: &str,
    pool_height: u64,
    submit_avg_ms: f64,
    submit_max_ms: u64,
    gpu_infos: &[(String, u32, u64, u32, Option<u32>, Option<u32>)],
) -> String {
    let uptime = fmt_uptime(uptime_secs);
    let total = total_accepted + total_rejected;
    let accept_pct = if total > 0 {
        total_accepted as f64 * 100.0 / total as f64
    } else {
        100.0
    };

    // Sum active hashrates (10s window)
    let total_hps: f64 = streams.iter().filter(|s| s.active).map(|s| s.hashrate_10s).sum();
    let (tv, tu) = fmt_hashrate(total_hps);

    let w = 73; // inner width

    // ── Top border + title ──
    let mut s = String::new();
    s.push_str(CYAN);
    s.push_str(&format!("┌{}┐\n", "─".repeat(w)));
    s.push_str("│");
    s.push_str(BOLD);
    s.push_str(WHITE);
    #[cfg(feature = "public_build")]
    let title_str = "  ZION v3.0.6 Miner";
    #[cfg(not(feature = "public_build"))]
    let title_str = "  ZION v3.0.6 Triple Stream";
    s.push_str(title_str);
    s.push_str(RESET);
    let title_len = title_str.chars().count();
    let title_pad = w - title_len - 7 - uptime.len();
    s.push_str(&" ".repeat(title_pad));
    s.push_str(DIM);
    s.push_str("uptime ");
    s.push_str(RESET);
    s.push_str(&uptime);
    s.push_str(" │\n");
    s.push_str(&format!("├{}┤\n", "─".repeat(w)));

    // ── Per-stream lines ──
    // In public_build, filter out non-ZION streams (hide Triple Stream).
    #[cfg(feature = "public_build")]
    let visible_streams: Vec<&StreamStats> = streams.iter().filter(|s| s.label == "ZION").collect();
    #[cfg(not(feature = "public_build"))]
    let visible_streams: Vec<&StreamStats> = streams.iter().collect();
    for stream in visible_streams {
        s.push_str("│");
        if !stream.active {
            // Inactive stream
            s.push_str(BRIGHT_BLACK);
            s.push_str(&format!("  {:<10}", stream.label));
            s.push_str(RESET);
            s.push_str(DIM);
            let detail = format!("{} / {}", stream.coin, stream.algorithm);
            s.push_str(&format!(" {:<22}", detail));
            s.push_str(RESET);
            s.push_str(BRIGHT_BLACK);
            // Reason for inactive
            let reason = if stream.coin.is_empty() {
                "IDLE (no job from pool)"
            } else if stream.algorithm.contains("progpow")
                || stream.algorithm.contains("ethash")
                || stream.algorithm.contains("kawpow")
            {
                "SKIPPED (DAG-based on Metal)"
            } else if stream.algorithm.contains("zelhash")
                || stream.algorithm.contains("beamhash")
            {
                "SKIPPED (memory-hard on Metal)"
            } else {
                "IDLE"
            };
            let remaining = w - 10 - 23 - 2;
            s.push_str(&format!(" {:<width$}", reason, width = remaining));
            s.push_str(RESET);
            s.push_str(" │\n");
            continue;
        }

        // Active stream
        let (hv, hu) = fmt_hashrate(stream.hashrate_10s);
        let (h60v, h60u) = fmt_hashrate(stream.hashrate_60s);
        let stream_total = stream.accepted + stream.rejected;
        let stream_pct = if stream_total > 0 {
            stream.accepted as f64 * 100.0 / stream_total as f64
        } else {
            100.0
        };

        // Stream label with color
        let label_color = match stream.label {
            "ZION" => BRIGHT_CYAN,
            "GPU PROFIT" => BRIGHT_YELLOW,
            "CPU PROFIT" => BRIGHT_GREEN,
            _ => WHITE,
        };
        s.push_str(label_color);
        s.push_str(BOLD);
        s.push_str(&format!("  {:<10}", stream.label));
        s.push_str(RESET);

        // Coin / algorithm
        let detail = format!("{} / {}", stream.coin, stream.algorithm);
        s.push_str(DIM);
        s.push_str(&format!(" {:<22}", &detail[..detail.len().min(22)]));
        s.push_str(RESET);

        // Hashrate
        s.push_str(" ");
        s.push_str(WHITE);
        s.push_str(&format!("{:>8}", hv));
        s.push(' ');
        s.push_str(DIM);
        s.push_str(hu);
        s.push_str(RESET);

        // Mini bar chart (10 chars, based on 60s hashrate relative to max)
        let bar_max = streams.iter().filter(|s2| s2.active).map(|s2| s2.hashrate_60s).fold(0.0f64, f64::max).max(1.0);
        let bar_len = ((stream.hashrate_60s / bar_max) * 7.0).round() as usize;
        let bar_len = bar_len.min(7);
        s.push_str(DIM);
        s.push_str("  ");
        s.push_str(GREEN);
        s.push_str(&"█".repeat(bar_len));
        s.push_str(RESET);
        s.push_str(BRIGHT_BLACK);
        s.push_str(&"░".repeat(7 - bar_len));
        s.push_str(RESET);

        // Shares
        s.push_str("  ");
        s.push_str(GREEN);
        s.push_str(&stream.accepted.to_string());
        s.push_str(RESET);
        s.push_str(DIM);
        s.push('/');
        s.push_str(RESET);
        let rej_color = if stream.rejected > 0 { BRIGHT_RED } else { DIM };
        s.push_str(rej_color);
        s.push_str(&stream.rejected.to_string());
        s.push_str(RESET);

        // Trailing space to fill
        let used = 10 + 23 + 1 + 8 + 1 + 2 + 2 + 7 + 2 + stream.accepted.to_string().len() + 1 + stream.rejected.to_string().len();
        let pad = w.saturating_sub(used + 2);
        s.push_str(&" ".repeat(pad));
        s.push_str(" │\n");
    }

    // ── Separator ──
    s.push_str(&format!("├{}┤\n", "─".repeat(w)));

    // ── Total line ──
    s.push_str("│");
    s.push_str(BOLD);
    s.push_str(WHITE);
    s.push_str("  TOTAL");
    s.push_str(RESET);
    s.push_str("     ");
    s.push_str(CYAN);
    s.push_str(BOLD);
    s.push_str(&format!("{:>8}", tv));
    s.push(' ');
    s.push_str(tu);
    s.push_str(RESET);
    s.push_str("    ");
    s.push_str(GREEN);
    s.push_str(&total_accepted.to_string());
    s.push_str(RESET);
    s.push_str(DIM);
    s.push_str(" accepted / ");
    s.push_str(RESET);
    let rej_col = if total_rejected > 0 { BRIGHT_RED } else { DIM };
    s.push_str(rej_col);
    s.push_str(&total_rejected.to_string());
    s.push_str(RESET);
    s.push_str(DIM);
    s.push_str(" rejected  (");
    s.push_str(RESET);
    s.push_str(if accept_pct >= 99.0 { GREEN } else if accept_pct >= 95.0 { YELLOW } else { RED });
    s.push_str(BOLD);
    s.push_str(&format!("{:.1}%", accept_pct));
    s.push_str(RESET);
    s.push_str(DIM);
    s.push_str(")");
    s.push_str(RESET);
    let total_used = 8 + 8 + 1 + 2 + 4 + total_accepted.to_string().len() + 12 + total_rejected.to_string().len() + 13 + 4;
    let total_pad = w.saturating_sub(total_used);
    s.push_str(&" ".repeat(total_pad));
    s.push_str(" │\n");

    // ── Pool info line ──
    s.push_str("│");
    s.push_str(DIM);
    s.push_str("  pool ");
    s.push_str(RESET);
    s.push_str(&pool_addr);
    s.push_str(DIM);
    s.push_str("   height ");
    s.push_str(RESET);
    s.push_str(&pool_height.to_string());
    s.push_str(DIM);
    s.push_str("   latency ");
    s.push_str(RESET);
    s.push_str(&format!("{:.0}/{} ms", submit_avg_ms, submit_max_ms));
    let pool_used = 6 + pool_addr.len() + 10 + pool_height.to_string().len() + 10 + format!("{:.0}", submit_avg_ms).len() + 1 + submit_max_ms.to_string().len() + 4;
    let pool_pad = w.saturating_sub(pool_used);
    s.push_str(&" ".repeat(pool_pad));
    s.push_str(" │\n");

    // ── GPU info line (if available) ──
    if !gpu_infos.is_empty() {
        for (i, (name, cu, vram, clock, temp, power)) in gpu_infos.iter().enumerate() {
            let vram_gb = *vram as f64 / 1024.0 / 1024.0 / 1024.0;
            let temp_str = match temp {
                Some(t) => format!("{}°C", t),
                None => "n/a".to_string(),
            };
            let power_str = match power {
                Some(p) => format!("{}W", p),
                None => "n/a".to_string(),
            };
            s.push_str("│");
            s.push_str(DIM);
            s.push_str(&format!("  GPU#{} ", i));
            s.push_str(RESET);
            s.push_str(GREEN);
            s.push_str(name);
            s.push_str(RESET);
            s.push_str(DIM);
            let gpu_info = format!("  {}CU  {:.1}GiB  {}MHz  {}  {}", cu, vram_gb, clock, temp_str, power_str);
            s.push_str(&gpu_info);
            s.push_str(RESET);
            let gpu_used = 7 + i.to_string().len() + name.len() + gpu_info.len();
            let gpu_pad = w.saturating_sub(gpu_used);
            s.push_str(&" ".repeat(gpu_pad));
            s.push_str(" │\n");
        }
    }

    // ── Bottom border ──
    s.push_str(&format!("└{}┘\n", "─".repeat(w)));
    s.push_str(RESET);
    s
}

/* ========================================================================= */
/* Claymore-style sticky header (alternate screen + full redraw)             */
/* ========================================================================= */

/// Print the triple-stream stats box as a **sticky header** at the top of the
/// terminal.  Uses the **alternate screen buffer** (like Claymore/GMiner) so
/// the header stays fixed while recent log lines are shown below it.
///
/// This approach works in ALL terminals including `screen` — no DECSTBM
/// scroll region needed.  On each update, the entire screen is redrawn:
///   1. Header box (metrics) at top — always visible
///   2. Recent log lines below — scrolling ring buffer
///
/// Log lines are captured via `push_log_line()` and kept in a ring buffer.
///
/// Disable with `ZION_NO_STICKY=1`.
pub fn print_triple_stream_stats_sticky(
    uptime_secs: u64,
    streams: &[StreamStats],
    total_accepted: u64,
    total_rejected: u64,
    pool_addr: &str,
    pool_height: u64,
    submit_avg_ms: f64,
    submit_max_ms: u64,
    gpu_infos: &[(String, u32, u64, u32, Option<u32>, Option<u32>)],
) {
    // Allow disabling sticky mode via env var
    if std::env::var("ZION_NO_STICKY")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false)
    {
        print_triple_stream_stats(
            uptime_secs,
            streams,
            total_accepted,
            total_rejected,
            pool_addr,
            pool_height,
            submit_avg_ms,
            submit_max_ms,
            gpu_infos,
        );
        return;
    }

    let box_str = build_triple_stream_box(
        uptime_secs,
        streams,
        total_accepted,
        total_rejected,
        pool_addr,
        pool_height,
        submit_avg_ms,
        submit_max_ms,
        gpu_infos,
    );

    let line_count = box_str.matches('\n').count();

    // ── First-time initialization ──
    if !STICKY_ACTIVE.load(Ordering::SeqCst) {
        // Open /dev/tty for direct UI output (bypasses redirected stdout)
        open_tty();
        // Redirect stdout to /dev/null — ALL println! from any thread
        // will now go to /dev/null and cannot corrupt the display.
        // UI output goes to /dev/tty via tty_write().
        redirect_stdout_to_null();
        // Activate sticky mode
        STICKY_ACTIVE.store(true, Ordering::SeqCst);
        STICKY_LINES.store(line_count, Ordering::SeqCst);

        // Enter alternate screen + hide cursor (written to /dev/tty)
        let mut init = String::new();
        init.push_str(ENTER_ALT_SCREEN);
        init.push_str(CURSOR_HIDE);
        tty_write(&init);
    }

    STICKY_LINES.store(line_count, Ordering::SeqCst);

    // ── Full screen redraw (written to /dev/tty) ──
    let mut out = String::with_capacity(4096);

    // Clear screen and go home
    out.push_str(CLEAR_SCREEN);
    out.push_str(HOME);

    // Print the metrics box at top
    out.push_str(&box_str);

    // Print recent log lines below the header
    let log_start_row = line_count + 2; // blank line after header
    if let Ok(buf) = LOG_RING.lock() {
        // Show last N lines that fit (assume 50 rows available for logs)
        let max_log_rows = 50;
        let start = if buf.len() > max_log_rows {
            buf.len() - max_log_rows
        } else {
            0
        };
        for (i, line) in buf.iter().enumerate().skip(start) {
            let row = log_start_row + i - start;
            // Move to row, clear line, print log line (truncated to 80 chars)
            let truncated: String = line.chars().take(80).collect();
            out.push_str(&format!("\x1B[{};1H\x1B[2K{}", row, truncated));
        }
    }

    // Move cursor to bottom-left (out of the way)
    out.push_str("\x1B[999;1H");
    tty_write(&out);
}

/// Exit the alternate screen buffer (call on shutdown).
pub fn exit_sticky_header() {
    if STICKY_ACTIVE.load(Ordering::SeqCst) {
        // Leave alt screen + show cursor (to /dev/tty)
        let mut out = String::new();
        out.push_str(EXIT_ALT_SCREEN);
        out.push_str(CURSOR_SHOW);
        tty_write(&out);
        // Close /dev/tty
        close_tty();
        // Restore original stdout
        restore_stdout();
        STICKY_ACTIVE.store(false, Ordering::SeqCst);
    }
}

/* ========================================================================= */
/* Banner                                                                    */
/* ========================================================================= */

pub fn print_fancy_banner(threads: usize, version: &str, backend: &str) {
    let mut s = String::new();
    s.push_str(CYAN);
    s.push_str("╔══════════════════════════════════════════════════════════════════╗\n");
    s.push_str("║  ");
    s.push_str(BOLD);
    s.push_str(WHITE);
    s.push_str("  ███████╗██╗ ██████╗ ███╗   ██╗");
    s.push_str(RESET);
    s.push_str("  ");
    s.push_str(YELLOW);
    s.push('v');
    s.push_str(version);
    s.push_str(RESET);
    s.push_str("              ║\n");
    s.push_str("║  ");
    s.push_str(BOLD);
    s.push_str(WHITE);
    s.push_str("  ╚══███╔╝██║██╔═══██╗████╗  ██║");
    s.push_str(RESET);
    s.push_str("  ");
    s.push_str(DIM);
    s.push_str("Triple Stream");
    s.push_str(RESET);
    s.push_str("         ║\n");
    s.push_str("║  ");
    s.push_str(BOLD);
    s.push_str(WHITE);
    s.push_str("    ███╔╝ ██║██║   ██║██╔██╗ ██║");
    s.push_str(RESET);
    s.push_str("  ");
    s.push_str(DIM);
    s.push_str("backend=");
    s.push_str(backend);
    s.push_str(RESET);
    s.push_str("        ║\n");
    s.push_str("║  ");
    s.push_str(BOLD);
    s.push_str(WHITE);
    s.push_str("   ███╔╝  ██║██║   ██║██║╚██╗██║");
    s.push_str(RESET);
    s.push_str("  ");
    s.push_str(DIM);
    s.push_str("threads=");
    s.push_str(&threads.to_string());
    s.push_str(RESET);
    s.push_str("          ║\n");
    s.push_str("║  ");
    s.push_str(BOLD);
    s.push_str(WHITE);
    s.push_str("  ███████╗██║╚██████╔╝██║ ╚████║");
    s.push_str(RESET);
    s.push_str("  ");
    s.push_str(DIM);
    s.push_str("Ekam Deeksha");
    s.push_str(RESET);
    s.push_str("       ║\n");
    s.push_str("╚══════════════════════════════════════════════════════════════════╝\n");
    s.push_str(RESET);
    print_flush(&s);
}

/* ========================================================================= */
/* Table helper for device list                                              */
/* ========================================================================= */

/// Print a formatted two-column table.
pub fn print_kv_table(rows: &[(String, String)]) {
    let max_key = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (k, v) in rows {
        let mut s = String::new();
        s.push_str("  ");
        s.push_str(DIM);
        s.push_str(&format!("{:>width$}", k, width = max_key));
        s.push_str(RESET);
        s.push_str("  ");
        s.push_str(v);
        s.push('\n');
        print_flush(&s);
    }
}
