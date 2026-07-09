//! Bridge relay entry point.
//!
//! Usage:
//!   zion-bridge --config config/bridge.toml
//!
//! Or with env vars:
//!   ZION_BRIDGE_CONFIG=config/bridge.toml zion-bridge
//!
//! ## Startup recovery
//!
//! On startup the relay reads `l1_locks` and `evm_burns` rows whose status is
//! `Pending`, `Confirmed`, or `Failed` (with retry_count < MAX_RELAY_RETRIES)
//! from the SQLite database and re-injects them into the processing channels
//! so they are handled even if the process crashed before finishing them.
//!
//! ## Watchdog
//!
//! A background task checks every `WATCHDOG_INTERVAL_SECS` seconds whether a
//! new L1 block has arrived within `security.l1_block_timeout_secs`. If not,
//! the watchdog logs an error. When `security.auto_pause_on_anomaly = true` the
//! bridge will refuse to process new events until an operator resumes it.
//!
//! ## Graceful shutdown
//!
//! Both `SIGTERM` and `Ctrl-C` are handled. The relay waits up to
//! `SHUTDOWN_GRACE_SECS` seconds for in-flight operations to complete.

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use zion_bridge::config::BridgeConfig;
use zion_bridge::db::BridgeDb;
use zion_bridge::evm_watcher::EvmWatcher;
use zion_bridge::l1_watcher::L1Watcher;
use zion_bridge::metrics::{serve_metrics, BridgeMetrics};
use zion_bridge::relayer::Relayer;

/// Max relay retries before a lock/burn is considered permanently failed.
const MAX_RELAY_RETRIES: u32 = 5;

/// How long to wait for in-flight tasks during graceful shutdown (seconds).
const SHUTDOWN_GRACE_SECS: u64 = 30;

/// Watchdog check interval (seconds).
const WATCHDOG_INTERVAL_SECS: u64 = 60;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    info!("🌉 ZION Bridge Relay v{}", env!("CARGO_PKG_VERSION"));
    info!("   L1 ↔ EVM cross-chain bridge");

    // Post-3.0.3 safety: crash loud if old binary (12-decimal) is loaded
    const {
        assert!(
            zion_bridge::types::FLOWERS_PER_ZION == 1_000_000,
            "FATAL: FLOWERS_PER_ZION != 1_000_000 — old pre-3.0.3 binary detected. \
         Update to v3.0.3+ before running bridge relay."
        )
    };
    const {
        assert!(
            zion_bridge::types::FLOWERS_TO_WEI_FACTOR == 1_000_000_000_000,
            "FATAL: FLOWERS_TO_WEI_FACTOR != 1e12 — old pre-3.0.3 binary detected."
        )
    };

    // Load config
    let config_path = std::env::var("ZION_BRIDGE_CONFIG").unwrap_or_else(|_| {
        std::env::args()
            .nth(2)
            .unwrap_or_else(|| "config/bridge.toml".into())
    });

    let config = BridgeConfig::load(&config_path)?;
    config.validate_runtime()?;
    let config = Arc::new(config);

    info!("📋 Network: {}", config.bridge.network);
    info!("📋 L1 RPC: {}", config.l1.rpc_url);
    if let Some(ref backup) = config.l1.rpc_url_backup {
        info!("📋 L1 RPC backup: {}", backup);
    }
    info!("📋 Bridge address: {}", config.l1.bridge_address);
    info!(
        "📋 Validator threshold: {}/{}",
        config.validator.threshold, config.validator.total_validators
    );

    for chain in config.active_chains() {
        info!(
            "📋 EVM chain: {} (ID: {}) — wZION: {}, Bridge: {}",
            chain.name, chain.evm_chain_id, chain.wzion_address, chain.bridge_contract_address,
        );
    }

    // Open database
    let db = Arc::new(BridgeDb::open(&config.database.path)?);
    let last_l1_height = db.get_last_l1_height()?;
    let last_l1_height = if last_l1_height == 0 {
        config.l1.start_block_height.unwrap_or(0)
    } else {
        last_l1_height
    };

    // Initialize metrics (BridgeMetrics::new() already returns Arc<Self>)
    let metrics = BridgeMetrics::new();

    // Start Prometheus metrics endpoint
    let metrics_port = config.metrics.port;
    tokio::spawn(serve_metrics(Arc::clone(&metrics), metrics_port));

    // Create channels (buffer large enough to hold recovery events)
    let (lock_tx, lock_rx) = mpsc::channel(500);
    let (burn_tx, burn_rx) = mpsc::channel(500);

    // ── Startup recovery: re-inject pending + retryable failed events ───────
    let recovered_locks = recover_pending_locks(&db, &lock_tx).await;
    let recovered_burns = recover_pending_burns(&db, &burn_tx).await;
    info!(
        "🔄 Startup recovery: {} pending lock(s), {} pending burn(s) re-queued",
        recovered_locks, recovered_burns,
    );

    // ── Shutdown flag ────────────────────────────────────────────────────────
    let shutdown = Arc::new(AtomicBool::new(false));

    // ── Start L1 watcher ─────────────────────────────────────────────────────
    let l1_config = config.l1.clone();
    let l1_metrics = Arc::clone(&metrics);
    let l1_shutdown = Arc::clone(&shutdown);
    let l1_lock_tx = lock_tx.clone();
    let _l1_handle = tokio::spawn(async move {
        let mut watcher =
            L1Watcher::new(l1_config, Some(last_l1_height)).with_metrics(Arc::clone(&l1_metrics));
        loop {
            if l1_shutdown.load(Ordering::Relaxed) {
                info!("L1 watcher: shutdown signal received");
                break;
            }
            if let Err(e) = watcher.run(l1_lock_tx.clone()).await {
                error!("L1 watcher crashed: {:?}", e);
                l1_metrics.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    // ── Start EVM watchers (one per active chain) ─────────────────────────
    let mut evm_handles = vec![];
    for chain in config.active_chains() {
        let chain_config = chain.clone();
        let start_block = chain_config.start_block;
        let ankr_config = config.ankr.clone();
        let burn_tx_clone = burn_tx.clone();
        let evm_metrics = Arc::clone(&metrics);
        let evm_shutdown = Arc::clone(&shutdown);
        let handle = tokio::spawn(async move {
            loop {
                if evm_shutdown.load(Ordering::Relaxed) {
                    info!(
                        "[{}] EVM watcher: shutdown signal received",
                        chain_config.name
                    );
                    break;
                }
                let mut watcher =
                    EvmWatcher::new(chain_config.clone(), ankr_config.clone(), start_block);
                if let Err(e) = watcher
                    .run(burn_tx_clone.clone(), Arc::clone(&evm_metrics))
                    .await
                {
                    error!("[{}] EVM watcher crashed: {:?}", chain_config.name, e);
                    evm_metrics.errors.fetch_add(1, Ordering::Relaxed);
                    // Brief pause before restart
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                }
            }
        });
        evm_handles.push(handle);
    }
    drop(burn_tx); // Drop extra sender so channel closes when all watchers stop

    // ── Start relayer ────────────────────────────────────────────────────────
    let relayer_config = Arc::clone(&config);
    let relayer_metrics = Arc::clone(&metrics);
    let relayer_db = Arc::clone(&db);
    let relayer_shutdown = Arc::clone(&shutdown);
    let _relayer_handle = tokio::spawn(async move {
        if relayer_shutdown.load(Ordering::Relaxed) {
            info!("Relayer: shutdown signal received");
            return;
        }
        let relayer = Relayer::new(
            Arc::clone(&relayer_config),
            Arc::clone(&relayer_metrics),
            Arc::clone(&relayer_db),
        );
        if let Err(e) = relayer.run(lock_rx, burn_rx).await {
            error!("Relayer crashed: {:?}", e);
            relayer_metrics.errors.fetch_add(1, Ordering::Relaxed);
        }
        // Relayer channels are consumed on first run — restart not easily possible
        // without recreating channels.
    });

    // ── Watchdog task ────────────────────────────────────────────────────────
    let watchdog_metrics = Arc::clone(&metrics);
    let watchdog_config = Arc::clone(&config);
    let watchdog_shutdown = Arc::clone(&shutdown);
    let _watchdog_handle = tokio::spawn(async move {
        run_watchdog(watchdog_metrics, watchdog_config, watchdog_shutdown).await;
    });

    info!("🟢 Bridge relay running — waiting for shutdown signal (SIGTERM / Ctrl-C)");

    // Wait for shutdown signal (Ctrl-C or SIGTERM)
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => { info!("🛑 Ctrl-C received"); }
            _ = sigterm.recv() => { info!("🛑 SIGTERM received"); }
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        info!("🛑 Ctrl-C received");
    }

    // Signal all tasks to stop
    shutdown.store(true, Ordering::SeqCst);
    info!(
        "🛑 Graceful shutdown — waiting up to {}s for in-flight operations…",
        SHUTDOWN_GRACE_SECS
    );
    tokio::time::sleep(std::time::Duration::from_secs(SHUTDOWN_GRACE_SECS)).await;
    info!("✅ Bridge relay stopped");

    Ok(())
}

/// Re-inject pending / retryable locks from DB into the lock channel at startup.
/// Returns the number of events re-queued.
async fn recover_pending_locks(
    db: &Arc<BridgeDb>,
    lock_tx: &mpsc::Sender<zion_bridge::types::L1LockEvent>,
) -> usize {
    let mut count = 0usize;

    // Pending + Confirmed + Executing (stuck mid-processing)
    match db.get_pending_locks() {
        Ok(locks) => {
            for lock in locks {
                info!(
                    "🔄 Recovery: re-queueing pending lock {} ({} wZION wei, status={:?})",
                    lock.l1_tx_hash, lock.amount_wzion_wei, lock.status,
                );
                // Reset to Pending so the relayer re-processes it cleanly
                let _ = db.update_lock_status(
                    &lock.l1_tx_hash,
                    zion_bridge::types::BridgeStatus::Pending,
                );
                if lock_tx.send(lock).await.is_err() {
                    error!("Recovery: lock channel closed during startup");
                    break;
                }
                count += 1;
            }
        }
        Err(e) => error!("Recovery: failed to load pending locks: {}", e),
    }

    // Failed but still retryable
    match db.get_retryable_locks(MAX_RELAY_RETRIES) {
        Ok(locks) => {
            for lock in locks {
                info!(
                    "🔄 Recovery: re-queueing failed lock {} for retry",
                    lock.l1_tx_hash
                );
                // Reset to Pending so the relayer re-processes it
                let _ = db.update_lock_status(
                    &lock.l1_tx_hash,
                    zion_bridge::types::BridgeStatus::Pending,
                );
                if lock_tx.send(lock).await.is_err() {
                    error!("Recovery: lock channel closed during startup");
                    break;
                }
                count += 1;
            }
        }
        Err(e) => error!("Recovery: failed to load retryable locks: {}", e),
    }

    count
}

/// Re-inject pending / retryable burns from DB into the burn channel at startup.
async fn recover_pending_burns(
    db: &Arc<BridgeDb>,
    burn_tx: &mpsc::Sender<zion_bridge::types::EvmBurnEvent>,
) -> usize {
    let mut count = 0usize;

    match db.get_pending_burns() {
        Ok(burns) => {
            for burn in burns {
                info!(
                    "🔄 Recovery: re-queueing pending burn {} ({} wZION wei, status={:?})",
                    burn.burn_id, burn.amount_wzion_wei, burn.status,
                );
                // Reset to Pending so the relayer re-processes it cleanly
                let _ =
                    db.update_burn_status(&burn.burn_id, zion_bridge::types::BridgeStatus::Pending);
                if burn_tx.send(burn).await.is_err() {
                    error!("Recovery: burn channel closed during startup");
                    break;
                }
                count += 1;
            }
        }
        Err(e) => error!("Recovery: failed to load pending burns: {}", e),
    }

    match db.get_retryable_burns(MAX_RELAY_RETRIES) {
        Ok(burns) => {
            for burn in burns {
                info!(
                    "🔄 Recovery: re-queueing failed burn {} for retry",
                    burn.burn_id
                );
                let _ =
                    db.update_burn_status(&burn.burn_id, zion_bridge::types::BridgeStatus::Pending);
                if burn_tx.send(burn).await.is_err() {
                    error!("Recovery: burn channel closed during startup");
                    break;
                }
                count += 1;
            }
        }
        Err(e) => error!("Recovery: failed to load retryable burns: {}", e),
    }

    count
}

/// Watchdog: checks that L1 blocks are arriving within the configured timeout.
/// Logs an error (and optionally pauses the bridge) if blocks stop.
async fn run_watchdog(
    metrics: Arc<BridgeMetrics>,
    config: Arc<BridgeConfig>,
    shutdown: Arc<AtomicBool>,
) {
    let timeout_secs = config.security.l1_block_timeout_secs;
    let auto_pause = config.security.auto_pause_on_anomaly;
    info!(
        "🐕 Watchdog started — L1 block timeout: {}s, auto_pause: {}",
        timeout_secs, auto_pause
    );

    let mut last_known_block: u64 = 0;
    let mut stall_reported = false;

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(WATCHDOG_INTERVAL_SECS)).await;

        if shutdown.load(Ordering::Relaxed) {
            info!("Watchdog: shutdown signal received");
            break;
        }

        let current_block = metrics.last_l1_block.load(Ordering::Relaxed);

        if current_block == 0 {
            // L1Watcher hasn't reported any block yet — skip until we have data
            continue;
        }

        if current_block > last_known_block {
            // New blocks arriving — reset stall state
            if stall_reported {
                info!(
                    "🐕 Watchdog: L1 block progress resumed at height {} (was stalled at {})",
                    current_block, last_known_block
                );
                stall_reported = false;
            }
            last_known_block = current_block;
            metrics.set_watchdog_ok();
        } else {
            // No new blocks since last check
            let stall_cycles = WATCHDOG_INTERVAL_SECS;
            if stall_cycles >= timeout_secs && !stall_reported {
                error!(
                    "🚨 Watchdog: L1 block STALL detected! Last block: {}, no new blocks for ≥{}s",
                    last_known_block, timeout_secs,
                );
                metrics.watchdog_stalls.fetch_add(1, Ordering::Relaxed);
                stall_reported = true;

                if auto_pause {
                    warn!(
                        "🚨 auto_pause_on_anomaly=true — bridge is pausing until L1 resumes. \
                         Operator must verify L1 node health."
                    );
                    metrics.set_bridge_paused(true);
                }
            }
        }
    }
}
