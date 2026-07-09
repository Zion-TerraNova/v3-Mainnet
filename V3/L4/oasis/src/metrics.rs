//! OASIS Prometheus-compatible metrics.
//!
//! Exposes a `/metrics` HTTP endpoint on a dedicated port (default 9101).
//! All counters are pure atomic — no external crate required.
//!
//! ## Prometheus endpoint
//!
//! ```text
//! GET http://localhost:9101/metrics
//! GET http://localhost:9101/health
//! ```

use axum::{routing::get, Router};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::info;

/// Runtime metrics for the OASIS game server.
#[derive(Debug)]
pub struct OasisMetrics {
    start_time: Instant,

    // Request counters
    pub requests_total: AtomicU64,
    pub xp_awards_total: AtomicU64,
    pub quest_completions_total: AtomicU64,
    pub guild_creations_total: AtomicU64,
    pub guild_joins_total: AtomicU64,
    pub combat_resolutions_total: AtomicU64,
    pub raid_team_creations_total: AtomicU64,
    pub errors_total: AtomicU64,

    // Gauges
    pub player_count: AtomicU64,
    pub active_guilds: AtomicU64,
    pub active_quests: AtomicU64,
}

impl OasisMetrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            start_time: Instant::now(),
            requests_total: AtomicU64::new(0),
            xp_awards_total: AtomicU64::new(0),
            quest_completions_total: AtomicU64::new(0),
            guild_creations_total: AtomicU64::new(0),
            guild_joins_total: AtomicU64::new(0),
            combat_resolutions_total: AtomicU64::new(0),
            raid_team_creations_total: AtomicU64::new(0),
            errors_total: AtomicU64::new(0),
            player_count: AtomicU64::new(0),
            active_guilds: AtomicU64::new(0),
            active_quests: AtomicU64::new(0),
        })
    }

    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            uptime_secs: self.uptime_secs(),
            requests_total: self.requests_total.load(Ordering::Relaxed),
            xp_awards_total: self.xp_awards_total.load(Ordering::Relaxed),
            quest_completions_total: self.quest_completions_total.load(Ordering::Relaxed),
            guild_creations_total: self.guild_creations_total.load(Ordering::Relaxed),
            guild_joins_total: self.guild_joins_total.load(Ordering::Relaxed),
            combat_resolutions_total: self.combat_resolutions_total.load(Ordering::Relaxed),
            raid_team_creations_total: self.raid_team_creations_total.load(Ordering::Relaxed),
            errors_total: self.errors_total.load(Ordering::Relaxed),
            player_count: self.player_count.load(Ordering::Relaxed),
            active_guilds: self.active_guilds.load(Ordering::Relaxed),
            active_quests: self.active_quests.load(Ordering::Relaxed),
        }
    }

    /// Render metrics in Prometheus text exposition format.
    pub fn render_prometheus(&self) -> String {
        let snap = self.snapshot();
        let mut out = String::with_capacity(1024);

        let metrics: &[(&str, &str, &str, u64)] = &[
            (
                "counter",
                "zion_oasis_uptime_seconds",
                "OASIS server uptime in seconds",
                snap.uptime_secs,
            ),
            (
                "counter",
                "zion_oasis_requests_total",
                "Total HTTP requests served",
                snap.requests_total,
            ),
            (
                "counter",
                "zion_oasis_xp_awards_total",
                "Total XP awards granted",
                snap.xp_awards_total,
            ),
            (
                "counter",
                "zion_oasis_quest_completions_total",
                "Total quests completed by players",
                snap.quest_completions_total,
            ),
            (
                "counter",
                "zion_oasis_guild_creations_total",
                "Total guilds created",
                snap.guild_creations_total,
            ),
            (
                "counter",
                "zion_oasis_guild_joins_total",
                "Total guild join requests",
                snap.guild_joins_total,
            ),
            (
                "counter",
                "zion_oasis_combat_resolutions_total",
                "Total combat resolutions processed",
                snap.combat_resolutions_total,
            ),
            (
                "counter",
                "zion_oasis_raid_team_creations_total",
                "Total raid teams created",
                snap.raid_team_creations_total,
            ),
            (
                "counter",
                "zion_oasis_errors_total",
                "Total server errors",
                snap.errors_total,
            ),
            (
                "gauge",
                "zion_oasis_player_count",
                "Current number of registered players",
                snap.player_count,
            ),
            (
                "gauge",
                "zion_oasis_active_guilds",
                "Current number of active guilds",
                snap.active_guilds,
            ),
            (
                "gauge",
                "zion_oasis_active_quests",
                "Current number of active quest definitions",
                snap.active_quests,
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
pub async fn serve_metrics(metrics: Arc<OasisMetrics>, port: u16) {
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

    let bind_host = std::env::var("OASIS_METRICS_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let addr = format!("{bind_host}:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Metrics server bind failed on {addr}: {e}");
            return;
        }
    };
    info!("📊 OASIS metrics endpoint: http://{addr}/metrics");
    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!("Metrics server error: {e}");
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    pub uptime_secs: u64,
    pub requests_total: u64,
    pub xp_awards_total: u64,
    pub quest_completions_total: u64,
    pub guild_creations_total: u64,
    pub guild_joins_total: u64,
    pub combat_resolutions_total: u64,
    pub raid_team_creations_total: u64,
    pub errors_total: u64,
    pub player_count: u64,
    pub active_guilds: u64,
    pub active_quests: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_initial_state() {
        let m = OasisMetrics::new();
        let snap = m.snapshot();
        assert_eq!(snap.requests_total, 0);
        assert_eq!(snap.xp_awards_total, 0);
        assert_eq!(snap.errors_total, 0);
        assert_eq!(snap.player_count, 0);
    }

    #[test]
    fn test_metrics_counters() {
        let m = OasisMetrics::new();
        m.requests_total.fetch_add(10, Ordering::Relaxed);
        m.xp_awards_total.fetch_add(5, Ordering::Relaxed);
        m.quest_completions_total.fetch_add(3, Ordering::Relaxed);
        m.errors_total.fetch_add(1, Ordering::Relaxed);
        m.player_count.store(42, Ordering::Relaxed);

        let snap = m.snapshot();
        assert_eq!(snap.requests_total, 10);
        assert_eq!(snap.xp_awards_total, 5);
        assert_eq!(snap.quest_completions_total, 3);
        assert_eq!(snap.errors_total, 1);
        assert_eq!(snap.player_count, 42);
    }

    #[test]
    fn test_metrics_prometheus_output() {
        let m = OasisMetrics::new();
        m.requests_total.fetch_add(7, Ordering::Relaxed);
        let out = m.render_prometheus();
        assert!(out.contains("zion_oasis_requests_total 7"));
        assert!(out.contains("# HELP zion_oasis_uptime_seconds"));
        assert!(out.contains("# TYPE zion_oasis_errors_total counter"));
    }

    #[test]
    fn test_metrics_thread_safe() {
        use std::thread;
        let m = OasisMetrics::new();
        let m2 = m.clone();

        let handle = thread::spawn(move || {
            for _ in 0..1000 {
                m2.requests_total.fetch_add(1, Ordering::Relaxed);
            }
        });

        for _ in 0..1000 {
            m.requests_total.fetch_add(1, Ordering::Relaxed);
        }

        handle.join().unwrap();
        assert_eq!(m.requests_total.load(Ordering::Relaxed), 2000);
    }
}
