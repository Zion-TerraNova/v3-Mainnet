//! Bridge metrics and monitoring.
//!
//! Exposes a Prometheus-compatible `/metrics` HTTP endpoint on `metrics.port`
//! (default 9100). All counters are pure atomic — no external crate required.
//!
//! ## Prometheus endpoint
//!
//! ```text
//! GET http://localhost:9100/metrics
//! GET http://localhost:9100/health
//! ```

use axum::{routing::get, Router};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::info;

/// Runtime metrics for the bridge relay.
#[derive(Debug)]
pub struct BridgeMetrics {
    start_time: Instant,

    // Counters
    pub l1_locks_detected: AtomicU64,
    pub l1_locks_finalized: AtomicU64,
    pub evm_mints_submitted: AtomicU64,
    pub evm_mints_confirmed: AtomicU64,

    pub evm_burns_detected: AtomicU64,
    pub l1_unlocks_submitted: AtomicU64,
    pub l1_unlocks_confirmed: AtomicU64,

    pub errors: AtomicU64,
    pub l1_poll_count: AtomicU64,
    pub evm_poll_count: AtomicU64,

    // Last processed heights
    pub last_l1_height: AtomicU64,
    pub last_evm_block: AtomicU64,

    // Alias — watchdog uses this name for clarity
    pub last_l1_block: AtomicU64,

    // Watchdog / health
    /// Number of times the watchdog detected a L1 block stall.
    pub watchdog_stalls: AtomicU64,
    /// 0 = running, 1 = paused (auto_pause_on_anomaly triggered).
    pub bridge_paused: AtomicU64,
}

impl BridgeMetrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            start_time: Instant::now(),
            l1_locks_detected: AtomicU64::new(0),
            l1_locks_finalized: AtomicU64::new(0),
            evm_mints_submitted: AtomicU64::new(0),
            evm_mints_confirmed: AtomicU64::new(0),
            evm_burns_detected: AtomicU64::new(0),
            l1_unlocks_submitted: AtomicU64::new(0),
            l1_unlocks_confirmed: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            l1_poll_count: AtomicU64::new(0),
            evm_poll_count: AtomicU64::new(0),
            last_l1_height: AtomicU64::new(0),
            last_evm_block: AtomicU64::new(0),
            last_l1_block: AtomicU64::new(0),
            watchdog_stalls: AtomicU64::new(0),
            bridge_paused: AtomicU64::new(0),
        })
    }

    /// Update `last_l1_block` (and alias `last_l1_height`) to the given height.
    pub fn update_l1_block(&self, height: u64) {
        self.last_l1_height.store(height, Ordering::Relaxed);
        self.last_l1_block.store(height, Ordering::Relaxed);
    }

    /// Watchdog: mark L1 as healthy (clear stall state).
    pub fn set_watchdog_ok(&self) {
        // No dedicated "ok" counter — just a no-op marker for future extension.
        // Callers can read `watchdog_stalls` to know if a stall occurred.
    }

    /// Set bridge paused state (1 = paused, 0 = running).
    pub fn set_bridge_paused(&self, paused: bool) {
        self.bridge_paused
            .store(if paused { 1 } else { 0 }, Ordering::SeqCst);
    }

    /// Returns true if the bridge is currently paused by the watchdog.
    pub fn is_paused(&self) -> bool {
        self.bridge_paused.load(Ordering::Relaxed) == 1
    }

    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            uptime_secs: self.uptime_secs(),
            l1_locks_detected: self.l1_locks_detected.load(Ordering::Relaxed),
            l1_locks_finalized: self.l1_locks_finalized.load(Ordering::Relaxed),
            evm_mints_submitted: self.evm_mints_submitted.load(Ordering::Relaxed),
            evm_mints_confirmed: self.evm_mints_confirmed.load(Ordering::Relaxed),
            evm_burns_detected: self.evm_burns_detected.load(Ordering::Relaxed),
            l1_unlocks_submitted: self.l1_unlocks_submitted.load(Ordering::Relaxed),
            l1_unlocks_confirmed: self.l1_unlocks_confirmed.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            last_l1_height: self.last_l1_height.load(Ordering::Relaxed),
            last_evm_block: self.last_evm_block.load(Ordering::Relaxed),
            watchdog_stalls: self.watchdog_stalls.load(Ordering::Relaxed),
            bridge_paused: self.bridge_paused.load(Ordering::Relaxed),
        }
    }

    /// Render metrics in Prometheus text exposition format.
    ///
    /// Compatible with `prometheus_client`, `node_exporter`, Grafana, etc.
    pub fn render_prometheus(&self) -> String {
        let snap = self.snapshot();
        let mut out = String::with_capacity(1024);

        let metrics: &[(&str, &str, &str, u64)] = &[
            (
                "counter",
                "zion_bridge_uptime_seconds",
                "Bridge relay uptime in seconds",
                snap.uptime_secs,
            ),
            (
                "counter",
                "zion_bridge_l1_locks_detected_total",
                "Total L1 lock transactions detected",
                snap.l1_locks_detected,
            ),
            (
                "counter",
                "zion_bridge_l1_locks_finalized_total",
                "Total L1 lock transactions finalized (enough confirmations)",
                snap.l1_locks_finalized,
            ),
            (
                "counter",
                "zion_bridge_evm_mints_submitted_total",
                "Total wZION mint transactions submitted on EVM",
                snap.evm_mints_submitted,
            ),
            (
                "counter",
                "zion_bridge_evm_mints_confirmed_total",
                "Total wZION mint transactions confirmed on EVM",
                snap.evm_mints_confirmed,
            ),
            (
                "counter",
                "zion_bridge_evm_burns_detected_total",
                "Total wZION burn events detected on EVM",
                snap.evm_burns_detected,
            ),
            (
                "counter",
                "zion_bridge_l1_unlocks_submitted_total",
                "Total L1 unlock transactions submitted",
                snap.l1_unlocks_submitted,
            ),
            (
                "counter",
                "zion_bridge_l1_unlocks_confirmed_total",
                "Total L1 unlock transactions confirmed",
                snap.l1_unlocks_confirmed,
            ),
            (
                "counter",
                "zion_bridge_errors_total",
                "Total bridge relay errors",
                snap.errors,
            ),
            (
                "gauge",
                "zion_bridge_last_l1_height",
                "Last processed L1 block height",
                snap.last_l1_height,
            ),
            (
                "gauge",
                "zion_bridge_last_evm_block",
                "Last processed EVM block number",
                snap.last_evm_block,
            ),
            (
                "counter",
                "zion_bridge_watchdog_stalls_total",
                "Number of L1 block stall events detected by the watchdog",
                snap.watchdog_stalls,
            ),
            (
                "gauge",
                "zion_bridge_paused",
                "1 if the bridge is paused by the watchdog (auto_pause_on_anomaly), 0 otherwise",
                snap.bridge_paused,
            ),
        ];

        for (kind, name, help, value) in metrics {
            out.push_str(&format!("# HELP {name} {help}\n"));
            out.push_str(&format!("# TYPE {name} {kind}\n"));
            out.push_str(&format!("{name} {value}\n"));
        }

        out
    }
}

/// Spawn a lightweight Prometheus HTTP endpoint.
///
/// Serves:
/// - `GET /metrics` — Prometheus text format
/// - `GET /health`  — JSON `{"status":"ok"}`
///
/// Call with `tokio::spawn(serve_metrics(metrics, port))`.
pub async fn serve_metrics(metrics: Arc<BridgeMetrics>, port: u16) {
    let app = Router::new()
        .route(
            "/metrics",
            get({
                let m = Arc::clone(&metrics);
                move || {
                    let txt = m.render_prometheus();
                    async move {
                        (
                            [(
                                axum::http::header::CONTENT_TYPE,
                                "text/plain; version=0.0.4",
                            )],
                            txt,
                        )
                    }
                }
            }),
        )
        .route(
            "/health",
            get(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
        );

    let bind_host =
        std::env::var("BRIDGE_METRICS_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let addr = format!("{bind_host}:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Metrics server bind failed on {addr}: {e}");
            return;
        }
    };
    info!("📊 Metrics endpoint: http://{addr}/metrics");
    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!("Metrics server error: {e}");
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    pub uptime_secs: u64,
    pub l1_locks_detected: u64,
    pub l1_locks_finalized: u64,
    pub evm_mints_submitted: u64,
    pub evm_mints_confirmed: u64,
    pub evm_burns_detected: u64,
    pub l1_unlocks_submitted: u64,
    pub l1_unlocks_confirmed: u64,
    pub errors: u64,
    pub last_l1_height: u64,
    pub last_evm_block: u64,
    pub watchdog_stalls: u64,
    pub bridge_paused: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_initial_state() {
        let m = BridgeMetrics::new();
        let snap = m.snapshot();
        assert_eq!(snap.l1_locks_detected, 0);
        assert_eq!(snap.l1_locks_finalized, 0);
        assert_eq!(snap.evm_mints_submitted, 0);
        assert_eq!(snap.evm_burns_detected, 0);
        assert_eq!(snap.errors, 0);
        assert_eq!(snap.last_l1_height, 0);
        assert_eq!(snap.last_evm_block, 0);
    }

    #[test]
    fn test_metrics_counters() {
        let m = BridgeMetrics::new();

        m.l1_locks_detected.fetch_add(5, Ordering::Relaxed);
        m.l1_locks_finalized.fetch_add(3, Ordering::Relaxed);
        m.evm_mints_submitted.fetch_add(3, Ordering::Relaxed);
        m.evm_mints_confirmed.fetch_add(2, Ordering::Relaxed);
        m.evm_burns_detected.fetch_add(1, Ordering::Relaxed);
        m.errors.fetch_add(1, Ordering::Relaxed);
        m.last_l1_height.store(12345, Ordering::Relaxed);
        m.last_evm_block.store(99999, Ordering::Relaxed);

        let snap = m.snapshot();
        assert_eq!(snap.l1_locks_detected, 5);
        assert_eq!(snap.l1_locks_finalized, 3);
        assert_eq!(snap.evm_mints_submitted, 3);
        assert_eq!(snap.evm_mints_confirmed, 2);
        assert_eq!(snap.evm_burns_detected, 1);
        assert_eq!(snap.errors, 1);
        assert_eq!(snap.last_l1_height, 12345);
        assert_eq!(snap.last_evm_block, 99999);
    }

    #[test]
    fn test_metrics_uptime() {
        let m = BridgeMetrics::new();
        // Should be at least 0 seconds since creation
        assert!(m.uptime_secs() < 5);
    }

    #[test]
    fn test_metrics_snapshot_serialization() {
        let m = BridgeMetrics::new();
        m.l1_locks_detected.fetch_add(10, Ordering::Relaxed);
        let snap = m.snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"l1_locks_detected\":10"));
    }

    #[test]
    fn test_metrics_thread_safe() {
        use std::thread;
        let m = BridgeMetrics::new();
        let m2 = m.clone();

        let handle = thread::spawn(move || {
            for _ in 0..1000 {
                m2.l1_locks_detected.fetch_add(1, Ordering::Relaxed);
            }
        });

        for _ in 0..1000 {
            m.l1_locks_detected.fetch_add(1, Ordering::Relaxed);
        }

        handle.join().unwrap();
        assert_eq!(m.l1_locks_detected.load(Ordering::Relaxed), 2000);
    }
}
