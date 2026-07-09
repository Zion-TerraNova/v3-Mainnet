//! ZION Revenue Proxy — standalone Stratum proxy for external multi-algo pools.
//!
//! Accepts connections from GPU miners and forwards them transparently to
//! external pools (2miners, MoneroOcean, ZPool, etc.), substituting the
//! operator wallet so the 25% multi-algo revenue stream is live.
//!
//! Usage:
//!   ZION_PROXY_COINS=KAS,ETC ZION_PROXY_WALLET=YOUR_BTC_WALLET cargo run --bin revenue-proxy
//!
//! Each coin listens on a dedicated port (default base 9000).

use std::sync::Arc;
use tokio::signal;
use tracing::{error, info};
use zion_cosmic_harmony::profit_router::{CoinProfile, ExternalCoin, PoolPreference};
use zion_pool::revenue_proxy::{ExternalPoolStats, ProxyListener};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let coins_env = std::env::var("ZION_PROXY_COINS").unwrap_or_else(|_| "KAS".to_string());
    let wallet = std::env::var("ZION_PROXY_WALLET").unwrap_or_default();
    let worker = std::env::var("ZION_PROXY_WORKER").unwrap_or_else(|_| "zion_pool".to_string());
    let region = std::env::var("ZION_PROXY_REGION").unwrap_or_else(|_| "eu".to_string());
    let preference = std::env::var("ZION_PROXY_PREFERENCE")
        .ok()
        .map(|s| PoolPreference::from_str_loose(&s))
        .unwrap_or(PoolPreference::Default);
    let base_port: u16 = std::env::var("ZION_PROXY_BASE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9000);

    if wallet.is_empty() {
        eprintln!("Error: ZION_PROXY_WALLET is required (BTC payout address).");
        std::process::exit(1);
    }

    let coin_names: Vec<&str> = coins_env.split(',').map(|s| s.trim()).collect();
    let mut handles = vec![];

    for (idx, name) in coin_names.iter().enumerate() {
        let Some(coin) = ExternalCoin::from_str_loose(name) else {
            error!("Unknown coin '{}', skipping", name);
            continue;
        };
        let profile = CoinProfile::for_preference(coin, preference, &region);
        let listen_port = base_port + idx as u16;
        let listen_addr = format!("0.0.0.0:{}", listen_port);
        let stats = Arc::new(ExternalPoolStats::default());

        let proxy = Arc::new(ProxyListener::new(
            &listen_addr,
            profile.pool_address(),
            &wallet,
            &worker,
            stats,
        ));

        info!(
            "Starting proxy for {} → {} (algo={}) on {}",
            coin.ticker(),
            profile.pool_address(),
            profile.algorithm,
            listen_addr
        );

        handles.push(tokio::spawn(async move {
            if let Err(e) = proxy.run().await {
                error!("[{}] Proxy listener error: {}", coin.ticker(), e);
            }
        }));
    }

    if handles.is_empty() {
        eprintln!("No valid coins configured. Set ZION_PROXY_COINS (e.g. KAS,ETC,ALPH).");
        std::process::exit(1);
    }

    info!("Revenue proxy running. Press Ctrl-C to stop.");
    signal::ctrl_c().await?;
    info!("Shutdown signal received.");

    // Tasks are detached; they'll exit when the process terminates.
    Ok(())
}
