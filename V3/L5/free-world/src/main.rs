//! ZION Free World Daemon — V3 L5 Humanitarian Layer
//!
//! ## Usage
//! ```sh
//! cargo run --bin zion-free-world
//!
//! # Override via env vars
//! FREE_WORLD_PORT=8095 \
//! FREE_WORLD_DB=./free_world.db \
//! FREE_WORLD_L1_RPC=http://localhost:8443/jsonrpc \
//! cargo run --bin zion-free-world
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;

use axum::http::Method;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use zion_free_world::api::{free_world_router, AppState};
use zion_free_world::config::FreeWorldConfig;
use zion_free_world::db::FreeWorldDb;
use zion_free_world::hiran_bridge::FreeWorldHiranBridge;
use zion_free_world::l1_scanner::{L1Scanner, ScannerConfig};
use zion_free_world::metrics::FreeWorldMetrics;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive("zion_free_world=info".parse().unwrap()))
        .init();

    info!(
        "🌍 ZION Free World Daemon v{} starting...",
        env!("CARGO_PKG_VERSION")
    );

    let cfg = FreeWorldConfig::load(None);
    info!(
        "Config: name={} api_port={} db={}",
        cfg.name, cfg.port, cfg.db_path
    );

    let fw_db = match FreeWorldDb::open(&cfg.db_path) {
        Ok(db) => {
            info!("DB opened at {}", cfg.db_path);
            db
        }
        Err(e) => {
            error!("Failed to open DB at {}: {}", cfg.db_path, e);
            std::process::exit(1);
        }
    };
    let db = Arc::new(Mutex::new(fw_db));

    let _fw_config = Arc::new(cfg.clone());
    let metrics = Arc::new(FreeWorldMetrics::new());
    info!("📊 Prometheus metrics: http://0.0.0.0:{}/metrics", cfg.port);

    let hiran = Arc::new(FreeWorldHiranBridge::new(&cfg));
    if cfg.hiran_enabled {
        info!(
            "🤖 Hiran AI enabled — endpoint: {}",
            cfg.hiran_endpoint
                .as_deref()
                .unwrap_or("http://localhost:8002")
        );
    } else {
        info!("Hiran AI disabled (set FREE_WORLD_HIRAN_ENABLED=true to enable)");
    }

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    let state = AppState {
        db: Arc::clone(&db),
        api_key: cfg.api_key.clone(),
        metrics: Arc::clone(&metrics),
        hiran,
    };

    let app = free_world_router(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("{}:{}", cfg.bind, cfg.port)
        .parse()
        .expect("Invalid bind/port in Free World config");

    info!("HTTP API listening on http://{}", addr);

    let scanner_cfg = ScannerConfig {
        rpc_url: cfg.l1_rpc_url.clone(),
        poll_interval: std::time::Duration::from_secs(cfg.scan_interval_secs),
        fund_address: cfg.humanitarian_fund_address.clone(),
        finality_blocks: 6,
    };
    let scanner = L1Scanner::new(scanner_cfg, Arc::clone(&db));

    let scanner_handle = tokio::spawn(async move {
        scanner.run().await;
    });

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind TCP listener");

    let server_handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("HTTP server error: {}", e);
        }
    });

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install ctrl+c handler");

    info!("Shutdown signal received — exiting...");
    scanner_handle.abort();
    server_handle.abort();
}
