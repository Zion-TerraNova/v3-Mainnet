//! Professional colored miner UI (XMRig / GMiner style)
//!
//! Uses ANSI escape codes for colors and cursor control.
//! Windows Terminal, MINGW64, and most modern consoles support these.

// Display-only helpers; several color codes / loggers are kept for completeness.
#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use std::io::{self, Write};

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

/* ========================================================================= */
/* Helpers                                                                   */
/* ========================================================================= */

fn print_flush(s: &str) {
    let _ = io::stdout().write_all(s.as_bytes());
    let _ = io::stdout().flush();
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
    print_flush(&s);
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
    print_flush(&s);
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
    print_flush(&s);
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
    print_flush(&s);
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
    print_flush(&s);
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
    print_flush(&s);
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
    print_flush(&s);
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
    print_flush(&s);
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
    s.push_str("GPU Miner");
    s.push_str(RESET);
    s.push_str("           ║\n");
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
