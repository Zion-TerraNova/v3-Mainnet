//! ZION OASIS — V3 L4 Consciousness Mining Game Server
//!
//! Usage:
//!   zion-oasis [--config path/to/oasis.toml]
//!
//! Environment variables:
//!   OASIS_PORT    — override API port (default: 8094)
//!   OASIS_DB      — path to SQLite database (default: ./oasis.db)
//!   OASIS_BIND    — bind address (default: 0.0.0.0)
//!   RUST_LOG      — log level (default: info)

use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use zion_oasis::{
    config::OasisConfig,
    db::OasisDb,
    metrics::OasisMetrics,
    quests::{QuestManager, QuestRegistry},
    server::{start_server, OasisState},
    websocket::WsHub,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logging
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    info!("🎮 Starting ZION OASIS v{}", env!("CARGO_PKG_VERSION"));

    // Config — defaults + env overrides
    let mut config = OasisConfig::default();

    if let Ok(port) = std::env::var("OASIS_PORT") {
        config.port = port.parse().unwrap_or(config.port);
    }
    if let Ok(bind) = std::env::var("OASIS_BIND") {
        config.bind = bind;
    }
    if let Ok(url) = std::env::var("OASIS_HIRAN_URL") {
        config.hiran_endpoint = Some(url);
    }
    if let Ok(enabled) = std::env::var("OASIS_HIRAN_ENABLED") {
        config.hiran_enabled = enabled.eq_ignore_ascii_case("true");
    }

    // Database
    let db_path = std::env::var("OASIS_DB").unwrap_or_else(|_| "./oasis.db".to_string());
    info!("Opening OASIS database: {}", db_path);
    let db = OasisDb::open(&db_path)?;

    // Load quest definitions from avatars.json
    let avatars_path =
        std::env::var("OASIS_AVATARS_PATH").unwrap_or_else(|_| "data/avatars.json".to_string());
    let quest_mgr = match std::fs::read_to_string(&avatars_path) {
        Ok(data) => match QuestRegistry::from_avatars_json(&data) {
            Ok(registry) => {
                info!("Loaded {} quests from {}", registry.len(), avatars_path);
                Arc::new(QuestManager::new(registry))
            }
            Err(e) => {
                tracing::warn!("Failed to parse quest registry: {}", e);
                Arc::new(QuestManager::new(QuestRegistry::default()))
            }
        },
        Err(e) => {
            tracing::warn!("Could not read {}: {}", avatars_path, e);
            Arc::new(QuestManager::new(QuestRegistry::default()))
        }
    };

    // Metrics
    let metrics = OasisMetrics::new();

    // Seed active_quests gauge from quest registry
    metrics.active_quests.store(
        quest_mgr.registry.len() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );

    // WebSocket broadcast hub
    let ws_hub = WsHub::new();

    info!("Consciousness levels: 9 (Physical → OnTheStar)");
    info!("Reward pool: 8,250,000,000 ZION over 10 years");
    if config.hiran_enabled {
        info!(
            "🤖 Hiran AI enabled — endpoint: {}",
            config
                .hiran_endpoint
                .as_deref()
                .unwrap_or("http://localhost:8002")
        );
    } else {
        info!("Hiran AI disabled (set OASIS_HIRAN_ENABLED=true to enable)");
    }

    let state = OasisState::new(db, config, quest_mgr, metrics, Some(ws_hub));

    start_server(state).await
}
