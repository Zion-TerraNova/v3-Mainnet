//! ZION DAO Daemon
//!
//! Runs two concurrent services:
//!   1. L1 Scanner — polls L1 blockchain for DAO governance memos
//!   2. HTTP API   — serves REST endpoints on :8080 (configurable)
//!
//! ## Usage
//!
//! ```sh
//! # Default (DB at ./dao.db, API on :8080, L1 RPC at http://127.0.0.1:8443/jsonrpc)
//! cargo run --bin zion-dao
//!
//! # Override via env vars
//! DAO_DB_PATH=/var/lib/zion/dao.db \
//! DAO_API_PORT=9090 \
//! DAO_L1_RPC=http://localhost:8444/jsonrpc \
//! ZION_DAO_API_KEY=my-secret \
//! cargo run --bin zion-dao
//! ```

use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::Method;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use zion_dao::api::{dao_router, AppState};
use zion_dao::config::DaoConfig;
use zion_dao::db::DaoDb;
use zion_dao::l1_scanner::{L1Scanner, ScannerConfig};
use zion_dao::metrics::DaoMetrics;
use zion_dao::treasury::Treasury;
use zion_dao::types::{Guardian, DAO_TREASURY_TOTAL};

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // Logging
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive("zion_dao=info".parse().unwrap()))
        .init();

    info!("ZION DAO Daemon v2.9.6 starting...");

    // ── Config: TOML file + env var overrides ──────────────────────────────
    // Priority: defaults < DAO_CONFIG=<path>.toml < individual env vars
    let cfg = DaoConfig::load(None);
    info!(
        "Config: name={} api_port={} db={}",
        cfg.name, cfg.api_port, cfg.db_path
    );

    if cfg.api_key.is_empty() {
        tracing::warn!("ZION_DAO_API_KEY not set — write endpoints disabled");
    }

    // ── Open SQLite DB ─────────────────────────────────────────────────────
    let dao_db = match DaoDb::open(&cfg.db_path) {
        Ok(db) => {
            info!("DB opened at {}", cfg.db_path);
            db
        }
        Err(e) => {
            error!("Failed to open DB at {}: {}", cfg.db_path, e);
            std::process::exit(1);
        }
    };
    let db = Arc::new(Mutex::new(dao_db));

    // ── Load config (minimal, uses defaults for now) ───────────────────────
    let dao_config = Arc::new(cfg.clone());

    // ── Initialize metrics ─────────────────────────────────────────────────
    let dao_metrics = DaoMetrics::new();
    info!(
        "📊 Prometheus metrics: http://0.0.0.0:{}/metrics",
        cfg.api_port
    );

    // ── Initialize Treasury (guardian set + premine total) ───────────────
    let guardian_set: Vec<Guardian> = cfg
        .guardians
        .iter()
        .map(|g| Guardian {
            name: g.name.clone(),
            address: g.address.clone(),
            public_key: g.public_key.clone(),
            is_active: true,
        })
        .collect();
    let treasury = Arc::new(Mutex::new(Treasury::new(guardian_set, DAO_TREASURY_TOTAL)));

    // ── Build Axum app ─────────────────────────────────────────────────────
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    let state = AppState {
        db: Arc::clone(&db),
        config: Arc::clone(&dao_config),
        api_key: cfg.api_key.clone(),
        metrics: Arc::clone(&dao_metrics),
        treasury: Arc::clone(&treasury),
    };

    let app = dao_router(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    // ── Bind address ───────────────────────────────────────────────────────
    let bind_host = std::env::var("DAO_API_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let addr: SocketAddr = format!("{}:{}", bind_host, cfg.api_port)
        .parse()
        .expect("Invalid api_port in DAO config");

    info!("HTTP API listening on http://{}", addr);

    // ── Start L1 scanner ───────────────────────────────────────────────────
    let scanner_cfg = ScannerConfig {
        rpc_url: cfg.l1_rpc_url.clone(),
        poll_interval: std::time::Duration::from_secs(cfg.scan_interval_secs),
        min_vote_weight: cfg.min_vote_weight,
        finality_blocks: cfg.finality_blocks,
    };
    let scanner =
        L1Scanner::new(scanner_cfg, Arc::clone(&db)).with_metrics(Arc::clone(&dao_metrics));

    let scanner_handle = tokio::spawn(async move {
        scanner.run().await;
    });

    // ── Start HTTP server ──────────────────────────────────────────────────
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind TCP listener");

    let server_handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("HTTP server error: {}", e);
        }
    });

    // ── Graceful shutdown ──────────────────────────────────────────────────
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install ctrl+c handler");

    info!("Shutdown signal received — exiting...");
    scanner_handle.abort();
    server_handle.abort();
}
