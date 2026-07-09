//! ZION Atomic Swap Daemon entry point.
//!
//! # Usage
//!
//! ```bash
//! # Start with a config file
//! ZION_SWAP_ESCROW_KEY=<64-char-hex> \
//! ZION_RPC_TOKEN=<token> \
//!   zion-atomic-swap --config /etc/zion/atomic-swap.toml
//!
//! # Or use the bundled example config
//!   zion-atomic-swap --config config/testnet.toml
//! ```
//!
//! # Startup sequence
//! 1. Load config (TOML file or defaults)
//! 2. Open SQLite DB
//! 3. Initialise `SwapExecutor` (validates escrow key → derives address)
//! 4. Start L1 block watcher background task
//! 5. Start auto-refund background task
//! 6. Start axum HTTP API

use axum::{routing::get, routing::post, Router};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use zion_atomic_swap::{
    config::SwapConfig,
    db::SwapDb,
    evm_watcher,
    executor::SwapExecutor,
    handlers::{self, AppState},
    watcher::{L1Watcher, RefundLoop},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Logging ───────────────────────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    // ── Config ────────────────────────────────────────────────────────────
    let config_path = std::env::args()
        .skip_while(|a| a != "--config")
        .nth(1)
        .unwrap_or_else(|| "config/atomic-swap.toml".to_string());

    let cfg = if std::path::Path::new(&config_path).exists() {
        info!("Loading config from {config_path}");
        SwapConfig::from_file(&config_path)?
    } else {
        info!("Config file not found — using built-in defaults (dev mode)");
        SwapConfig {
            swap: Default::default(),
            l1: Default::default(),
            database: Default::default(),
            api: Default::default(),
            refund: Default::default(),
            evm_watcher: None,
        }
    };

    // L2 security patch: fail-fast on mainnet if bearer_token / escrow key
    // are not set (C1 — open-access claim/refund endpoints are unsafe).
    cfg.validate_runtime()?;

    // M2: wall-clock sanity check. HTLC timelocks use Utc::now(); a badly
    // skewed system clock can trigger premature refunds or lock funds
    // indefinitely. Warn loudly if the clock looks wrong (year < 2024 or
    // > 2100). Full block-height-based timelocks are a future design change.
    let now_year = chrono::Utc::now()
        .format("%Y")
        .to_string()
        .parse::<u32>()
        .unwrap_or(0);
    if !(2024..=2100).contains(&now_year) {
        warn!(
            "⚠️ System clock reports year {} — HTLC timelocks depend on wall clock (M2). \
             Verify NTP sync before relying on refund/claim timing.",
            now_year
        );
    }

    let cfg = Arc::new(cfg);

    // ── Database ──────────────────────────────────────────────────────────
    let db_path = &cfg.database.path;
    info!("Opening database at {db_path}");
    let db = Arc::new(SwapDb::open(db_path)?);

    // ── Executor (validates escrow key) ───────────────────────────────────
    let executor = Arc::new(SwapExecutor::new(Arc::clone(&cfg))?);
    let escrow_address = executor.escrow_address.clone();
    info!("Escrow address: {escrow_address}");

    // ── Background: L1 watcher ────────────────────────────────────────────
    {
        let watcher = L1Watcher::new(
            Arc::clone(&cfg),
            Arc::clone(&db),
            Arc::clone(&executor),
            escrow_address.clone(),
        );
        tokio::spawn(async move {
            watcher.run().await;
        });
    }

    // ── Background: auto-refund loop ──────────────────────────────────────
    {
        let refund_loop = RefundLoop::new(Arc::clone(&cfg), Arc::clone(&db), Arc::clone(&executor));
        tokio::spawn(async move {
            refund_loop.run().await;
        });
    }
    // ── Background: EVM watcher (Base chain HTLC events) ──────────────
    if let Some(evm_cfg) = cfg.evm_watcher.clone() {
        if evm_cfg.enabled {
            info!("Starting EVM watcher for {}", evm_cfg.contract_addr);
            let swap_db = Arc::clone(&db);
            tokio::spawn(async move {
                let conn = swap_db.conn_for_evm_watcher();
                if let Err(e) = evm_watcher::run(evm_cfg, conn).await {
                    error!("EVM watcher exited with error: {e}");
                }
            });
        } else {
            info!("EVM watcher present in config but disabled");
        }
    }
    // ── HTTP API ──────────────────────────────────────────────────────────
    let state = AppState {
        db: Arc::clone(&db),
        executor: Arc::clone(&executor),
        escrow_address: escrow_address.clone(),
        bearer_token: cfg.api_bearer_token(),
    };

    let app = Router::new()
        .route("/health", get(handlers::health))
        .route("/swap/escrow-address", get(handlers::escrow_address))
        .route("/swap/pending", get(handlers::list_pending))
        .route("/swap/:hash", get(handlers::swap_status))
        .route("/swap/claim", post(handlers::claim))
        .route("/swap/refund", post(handlers::refund))
        .with_state(state);

    let bind = &cfg.api.bind;
    info!("🚀 Atomic Swap API listening on http://{bind}");

    // Use socket2 to set SO_REUSEADDR + SO_REUSEPORT so the daemon can
    // restart immediately without waiting for TIME_WAIT to expire.
    let addr: SocketAddr = bind.parse()?;
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    let listener = tokio::net::TcpListener::from_std(socket.into())?;

    axum::serve(listener, app).await?;

    Ok(())
}
