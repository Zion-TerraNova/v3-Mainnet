use anyhow::{anyhow, Context, Result};
use std::collections::{HashSet, VecDeque};
use std::fmt::Write as FmtWrite;
use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zion_core::{
    decode_p2p_message, decode_rpc_request,
    discovery::{DiscoveryCommand, DiscoveryEngine, DISCOVERY_PORT},
    encode_p2p_message, encode_rpc_response,
    ibd::{IbdCommand, IbdEngine},
    migration, node_protocol_version,
    p2p_security::PeerSecurity,
    peer_manager::{PeerAction, PeerDirection, PeerManager, MIN_OUTBOUND},
    propagation::{PropagationStats, SeenBlocks, SeenTransactions},
    rpc::{build_node_router, RpcRouter},
    websocket::WebSocketServer,
    AcceptedBlock, NodeConfig, NodeRuntime, P2pMessage, PeerEndpoint, SubmittedTransaction,
};

/// Acquire the PeerManager guard, recovering from poisoning instead of
/// panicking. PeerManager holds scoring/tracking bookkeeping whose loss
/// on a fresh recovered inner is not security-critical — recovering keeps
/// the daemon alive through a single panic in the P2P event loop rather
/// than amplifying it into a DOS on every subsequent peer connection.
/// (Audit finding F5.)
fn lock_peer_mgr(m: &Mutex<PeerManager>) -> MutexGuard<'_, PeerManager> {
    m.lock().unwrap_or_else(|poisoned| {
        eprintln!(
            "warning: peer_mgr mutex was poisoned by a panicking holder; \
             recovering inner state"
        );
        poisoned.into_inner()
    })
}

/// Acquire the PeerSecurity guard, recovering from poisoning instead of
/// panicking. PeerSecurity tracks rate-limit / ban bookkeeping; a panic
/// mid-mutation would otherwise take the whole daemon down on the very
/// next connection accept or message. (Audit finding F5.)
fn lock_peer_sec(m: &Mutex<PeerSecurity>) -> MutexGuard<'_, PeerSecurity> {
    m.lock().unwrap_or_else(|poisoned| {
        eprintln!(
            "warning: peer_sec mutex was poisoned by a panicking holder; \
             recovering inner state"
        );
        poisoned.into_inner()
    })
}

fn main() -> Result<()> {
    // Read migration height from env (set by edge-deploy config).
    // This tells the node which blocks are pre-migration (legacy 1e12 scale)
    // vs post-migration (new 1e6 scale). The RPC layer uses this to normalize
    // balance computations across the fork boundary.
    if let Ok(mh_str) = std::env::var("ZION_MIGRATION_HEIGHT") {
        if let Ok(mh) = mh_str.parse::<u64>() {
            if mh > 0 {
                migration::set_migration_height(mh);
                eprintln!("migration_height={mh} (pre-fork blocks use legacy 1e12 scale)");
            }
        }
    }

    // Account-model memo v1 hard-fork height override.
    // For a fresh chain the compile-time constant (0 = active from genesis) is
    // used. For a coordinated hard fork on an existing mainnet/testnet, set this
    // to a future height before starting all nodes.
    if let Ok(h_str) = std::env::var("ZION_ACCOUNT_TX_MEMO_V1_HEIGHT") {
        if let Ok(h) = h_str.parse::<u64>() {
            zion_cosmic_harmony::set_account_tx_memo_v1_activation_height(h);
            eprintln!("account_tx_memo_v1_activation_height={h} (runtime override for hard fork)");
        }
    }

    // F5: Account-model sender balance validation gate.
    // Active from genesis (0) by default. For existing chains where historical
    // blocks may contain TXs from addresses with insufficient balance, set
    // ZION_BALANCE_CHECK_HEIGHT to a future height before starting all nodes.
    if let Ok(h_str) = std::env::var("ZION_BALANCE_CHECK_HEIGHT") {
        if let Ok(h) = h_str.parse::<u64>() {
            zion_cosmic_harmony::set_balance_check_height(h);
            eprintln!("balance_check_activation_height={h} (runtime override for F5 hard fork)");
        }
    }

    // F4.7: Max transaction amount cap gate. Rejects any non-genesis,
    // non-coinbase TX whose amount exceeds TOTAL_SUPPLY. Disabled by default
    // (u64::MAX). Set ZION_MAX_TX_AMOUNT_HEIGHT to a future height (above the
    // 3.0.3 migration height) for a coordinated hard fork.
    if let Ok(h_str) = std::env::var("ZION_MAX_TX_AMOUNT_HEIGHT") {
        if let Ok(h) = h_str.parse::<u64>() {
            zion_cosmic_harmony::set_max_tx_amount_height(h);
            eprintln!("max_tx_amount_activation_height={h} (runtime override for F4.7 hard fork)");
        }
    }

    let config = NodeServerConfig::from_env()?;

    // Guard: migration height should be set for non-dev networks.
    let network_name = std::env::var("ZION_NETWORK").unwrap_or_else(|_| "mainnet".to_string());
    let is_production = network_name != "devnet" && network_name != "test";
    let migration_height = migration::migration_height();
    if migration_height == 0 && is_production {
        let has_existing_state = config
            .state_path
            .as_ref()
            .map(|p| std::path::Path::new(p).exists())
            .unwrap_or(false);
        if has_existing_state {
            return Err(anyhow!(
                "ZION_MIGRATION_HEIGHT is 0 or unset on a production network ({network_name}) \
                 with existing chain state. Set it to the correct migration height before starting the node."
            ));
        }
        eprintln!("warning: ZION_MIGRATION_HEIGHT is 0 on {network_name}; fresh chain assumed");
    }

    let miner_address = std::env::var("ZION_MINER_ADDRESS").unwrap_or_default();
    let humanitarian_address = std::env::var("ZION_HUMANITARIAN_WALLET").unwrap_or_default();
    let issobella_address = std::env::var("ZION_ISSOBELLA_WALLET").unwrap_or_default();
    // The pool-fee 1% slot is BURNED (never minted), so ZION_POOL_FEE_WALLET is
    // no longer required or used for the coinbase. Only humanitarian + issobella
    // need addresses; an empty string is passed for the (burned) pool-fee slot.
    let pool_fee_address = String::new();
    let has_any_fee_address = !humanitarian_address.is_empty() || !issobella_address.is_empty();
    let has_all_fee_addresses = !humanitarian_address.is_empty() && !issobella_address.is_empty();
    if has_any_fee_address && !has_all_fee_addresses {
        return Err(anyhow!(
            "ZION_HUMANITARIAN_WALLET and ZION_ISSOBELLA_WALLET must both be set together"
        ));
    }
    let runtime = Arc::new(Mutex::new({
        let mut rt = match config.state_path.as_deref() {
            Some(state_path) => NodeRuntime::with_chain_store(
                config.node_id.clone(),
                config.node_config.clone(),
                state_path,
            )
            .map_err(anyhow::Error::msg)?,
            None => NodeRuntime::new(config.node_id.clone(), config.node_config.clone()),
        };
        if !miner_address.is_empty() {
            rt.set_miner_address(miner_address.clone());
        }
        if has_all_fee_addresses {
            rt.set_fee_addresses(
                humanitarian_address.clone(),
                issobella_address.clone(),
                pool_fee_address.clone(),
            );
        }
        if config.block_retention > 0 {
            rt.set_block_retention(config.block_retention);
        }
        rt
    }));

    // Create WebSocket server
    let ws_server = Arc::new(WebSocketServer::new(Arc::clone(&runtime)));

    // Set WebSocket notifier in runtime
    {
        let mut rt = runtime.lock().expect("lock");
        rt.set_websocket_notifier(Arc::clone(&ws_server));
    }

    println!("ZION v3 node");
    println!("node_id={}", config.node_id);
    if !miner_address.is_empty() {
        println!("miner_address={}", miner_address);
    }
    if has_all_fee_addresses {
        println!("humanitarian_address={}", humanitarian_address);
        println!("issobella_address={}", issobella_address);
        println!("pool_fee=burned(1%)");
    }
    println!("protocol_version={}", node_protocol_version());
    println!("p2p_bind={}", config.node_config.p2p_bind.address());
    println!("rpc_bind={}", config.node_config.rpc_bind.address());
    println!(
        "block_retention={}",
        if config.block_retention == 0 {
            "unlimited".to_string()
        } else {
            config.block_retention.to_string()
        }
    );
    println!(
        "p2p_accept_limit={}",
        config
            .p2p_accept_limit
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unbounded".to_string())
    );
    println!(
        "rpc_accept_limit={}",
        config
            .rpc_accept_limit
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unbounded".to_string())
    );
    if let Some(state_path) = config.state_path.as_deref() {
        println!("state_path={state_path}");
    }

    // Run bootstrap sync in background so the node opens its ports immediately
    // while still attempting to catch up from seed peers.
    let runtime_clone = Arc::clone(&runtime);
    let batch_limit = config.sync_batch_limit;
    std::thread::spawn(move || {
        if let Err(e) = bootstrap_peer_sync(&runtime_clone, batch_limit) {
            eprintln!("bootstrap_peer_sync_err err={e}");
        }
        // Persist peers after bootstrap (captures any new peers learned)
        {
            let rt = runtime_clone.lock().expect("lock");
            if let Err(e) = rt.persist_peers() {
                eprintln!("peers_persist_err err={e}");
            }
        }
    });

    // Shared propagation state
    let seen_blocks = Arc::new(Mutex::new(SeenBlocks::new()));
    let seen_txs = Arc::new(Mutex::new(SeenTransactions::new()));
    let prop_stats = Arc::new(PropagationStats::new());

    // Peer manager — scoring, tracking, subnet diversity
    let peer_mgr = Arc::new(Mutex::new(PeerManager::new()));
    {
        let seeds: Vec<(IpAddr, u16)> = runtime
            .lock()
            .expect("lock")
            .known_peers()
            .iter()
            .filter_map(|p| {
                p.address().rsplit_once(':').and_then(|(h, port)| {
                    let ip: IpAddr = h.parse().ok()?;
                    let port: u16 = port.parse().ok()?;
                    Some((ip, port))
                })
            })
            .collect();
        lock_peer_mgr(&peer_mgr).add_seeds(&seeds);
    }

    // P2P security — rate limiting, banning, connection limits
    let peer_sec = Arc::new(Mutex::new(PeerSecurity::new()));

    // JSON-RPC 2.0 router (shared across all RPC client threads)
    let jsonrpc_router = Arc::new(build_node_router(Arc::clone(&runtime)));

    let p2p_listener = TcpListener::bind(config.node_config.p2p_bind.address())
        .context("failed to bind P2P listener")?;
    let rpc_listener = TcpListener::bind(config.node_config.rpc_bind.address())
        .context("failed to bind RPC listener")?;

    // ── Metrics HTTP server ────────────────────────────────────────────
    let metrics_bind =
        std::env::var("ZION_METRICS_BIND").unwrap_or_else(|_| "0.0.0.0:9115".to_string());
    println!("metrics_bind={metrics_bind}");
    let metrics_runtime = Arc::clone(&runtime);
    let _metrics_thread = thread::spawn(move || {
        if let Err(e) = serve_node_metrics(&metrics_bind, metrics_runtime) {
            eprintln!("metrics_server_err={e}");
        }
    });

    // ── WebSocket server ───────────────────────────────────────────────
    let ws_bind = config.node_config.websocket_bind.address();
    println!("websocket_bind={ws_bind}");
    let _ws_thread = thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async {
            if let Err(e) = ws_server.serve(&ws_bind).await {
                eprintln!("websocket_server_err={e}");
            }
        });
    });

    // ── Outbound peer thread ───────────────────────────────────────────
    let ob_runtime = Arc::clone(&runtime);
    let ob_seen = Arc::clone(&seen_blocks);
    let ob_stats = Arc::clone(&prop_stats);
    let ob_peer_mgr = Arc::clone(&peer_mgr);
    let ob_peer_sec = Arc::clone(&peer_sec);
    let ob_batch_limit = config.sync_batch_limit;
    let outbound_thread = thread::spawn(move || {
        outbound_peer_loop(
            &ob_runtime,
            &ob_seen,
            &ob_stats,
            &ob_peer_mgr,
            &ob_peer_sec,
            ob_batch_limit,
        );
    });

    // ── P2P accept loop ────────────────────────────────────────────────
    let p2p_runtime = Arc::clone(&runtime);
    let p2p_seen = Arc::clone(&seen_blocks);
    let p2p_seen_txs = Arc::clone(&seen_txs);
    let p2p_stats = Arc::clone(&prop_stats);
    let p2p_peer_mgr = Arc::clone(&peer_mgr);
    let p2p_peer_sec = Arc::clone(&peer_sec);
    let p2p_limit = config.p2p_accept_limit;
    let p2p_thread = thread::spawn(move || -> Result<()> {
        let mut handles = Vec::new();
        let mut accepted = 0u32;
        loop {
            if matches!(p2p_limit, Some(limit) if accepted >= limit) {
                break;
            }
            let (stream, peer_addr) = p2p_listener.accept().context("failed to accept P2P peer")?;

            // Security gate: check ban + connection limit
            let peer_ip = peer_addr.ip();
            let now_epoch = epoch_secs();
            {
                let mut sec = lock_peer_sec(&p2p_peer_sec);
                if !sec.try_accept_connection(&peer_ip, now_epoch) {
                    println!("p2p_rejected ip={peer_ip} (banned or at limit)");
                    drop(stream);
                    continue;
                }
            }

            println!("p2p_peer_addr={peer_addr}");
            let runtime = Arc::clone(&p2p_runtime);
            let seen = Arc::clone(&p2p_seen);
            let seen_txs = Arc::clone(&p2p_seen_txs);
            let stats = Arc::clone(&p2p_stats);
            let mgr = Arc::clone(&p2p_peer_mgr);
            let sec = Arc::clone(&p2p_peer_sec);
            let source = peer_addr.to_string();
            let source_ip = peer_ip;
            handles.push(thread::spawn(move || {
                let result = handle_p2p_stream(
                    stream, &runtime, &seen, &seen_txs, &stats, &source, &mgr, &sec, source_ip,
                );
                // Release connection slot when stream ends
                lock_peer_sec(&sec).release_connection();
                result
            }));
            accepted = accepted.saturating_add(1);
        }
        for handle in handles {
            handle
                .join()
                .map_err(|_| anyhow!("P2P client thread panicked"))??;
        }
        Ok(())
    });

    let rpc_runtime = Arc::clone(&runtime);
    let rpc_seen = Arc::clone(&seen_blocks);
    let rpc_seen_txs = Arc::clone(&seen_txs);
    let rpc_stats = Arc::clone(&prop_stats);
    let rpc_router = Arc::clone(&jsonrpc_router);
    let rpc_limit = config.rpc_accept_limit;
    let rpc_thread = thread::spawn(move || -> Result<()> {
        let mut handles = Vec::new();
        let mut accepted = 0u32;
        loop {
            if matches!(rpc_limit, Some(limit) if accepted >= limit) {
                break;
            }
            let (stream, peer_addr) = rpc_listener
                .accept()
                .context("failed to accept RPC client")?;
            println!("rpc_client_addr={peer_addr}");
            let runtime = Arc::clone(&rpc_runtime);
            let seen = Arc::clone(&rpc_seen);
            let seen_txs = Arc::clone(&rpc_seen_txs);
            let stats = Arc::clone(&rpc_stats);
            let router = Arc::clone(&rpc_router);
            handles.push(thread::spawn(move || {
                handle_rpc_stream(stream, &runtime, &seen, &seen_txs, &stats, &router)
            }));
            accepted = accepted.saturating_add(1);
        }
        for handle in handles {
            handle
                .join()
                .map_err(|_| anyhow!("RPC client thread panicked"))??;
        }
        Ok(())
    });

    p2p_thread
        .join()
        .map_err(|_| anyhow!("P2P thread panicked"))??;
    rpc_thread
        .join()
        .map_err(|_| anyhow!("RPC thread panicked"))??;
    let _ = outbound_thread.join();

    let status = runtime.lock().expect("node runtime lock poisoned").status();
    let snap = prop_stats.snapshot();
    println!("known_peers={}", status.known_peers.len());
    println!("blocks_relayed={}", snap.blocks_relayed);
    println!("relay_successes={}", snap.relay_successes);
    println!("relay_failures={}", snap.relay_failures);
    println!("txs_relayed={}", snap.txs_relayed);
    println!("tx_relay_successes={}", snap.tx_relay_successes);
    println!("tx_relay_failures={}", snap.tx_relay_failures);
    println!("revenue_total_usd={:.2}", status.revenue.total_earnings_usd);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_p2p_stream(
    stream: TcpStream,
    runtime: &Arc<Mutex<NodeRuntime>>,
    seen: &Arc<Mutex<SeenBlocks>>,
    seen_txs: &Arc<Mutex<SeenTransactions>>,
    stats: &Arc<PropagationStats>,
    source_addr: &str,
    peer_mgr: &Arc<Mutex<PeerManager>>,
    peer_sec: &Arc<Mutex<PeerSecurity>>,
    source_ip: IpAddr,
) -> Result<()> {
    // Set read timeout so idle connections are cleaned up
    stream.set_read_timeout(Some(Duration::from_secs(330))).ok();
    let reader_stream = stream.try_clone().context("failed to clone P2P stream")?;
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;

    // Peer ID assigned after Hello handshake
    let mut peer_id: Option<String> = None;

    // Transport-layer block-import allowlist (SEC-2026-07-02 F1 hardening).
    //
    // `ZION_P2P_ALLOWED_PEERS` is a comma-separated list of source IPs that
    // are permitted to announce blocks. When set, blocks from any other
    // source IP are rejected at the transport layer BEFORE consensus
    // validation. This is defense-in-depth on top of the firewall
    // (UFW/Tailscale) — if the firewall is ever misconfigured, an
    // unauthenticated peer still cannot inject blocks. Empty/unset = open
    // (any peer may announce blocks; consensus rules still apply).
    let block_import_allowlist: Vec<IpAddr> = std::env::var("ZION_P2P_ALLOWED_PEERS")
        .ok()
        .map(|raw| {
            raw.split(',')
                .filter_map(|entry| entry.trim().parse::<IpAddr>().ok())
                .collect()
        })
        .unwrap_or_default();

    loop {
        let line = match read_line(&mut reader) {
            Ok(line) => line,
            Err(_) => break, // Connection closed or timeout
        };

        // Rate limiting per message
        {
            let now_epoch = epoch_secs();
            let mut sec = lock_peer_sec(peer_sec);
            if let Err(_reason) = sec.record_message(source_ip, now_epoch) {
                eprintln!("p2p_rate_limited ip={source_ip}");
                break;
            }
        }

        println!("p2p_in={line}");
        let message = match decode_p2p_message(&line) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("p2p_decode_err source={source_addr} err={e}");
                // Punish for protocol violation
                lock_peer_sec(peer_sec).punish(
                    source_ip,
                    epoch_secs(),
                    zion_core::p2p_security::BanReason::ProtocolViolation,
                );
                break;
            }
        };

        // Register peer in PeerManager on Hello
        if let P2pMessage::Hello {
            ref node_id,
            ref listen_addr,
            ..
        } = message
        {
            let now = Instant::now();
            if let Some((host, port_str)) = listen_addr.rsplit_once(':') {
                if let (Ok(ip), Ok(port)) = (host.parse::<IpAddr>(), port_str.parse::<u16>()) {
                    lock_peer_mgr(peer_mgr).register_peer(
                        node_id,
                        ip,
                        port,
                        PeerDirection::Inbound,
                        now,
                    );
                    peer_id = Some(node_id.clone());
                }
            }
        }

        // Update last_seen in PeerManager
        if let Some(ref pid) = peer_id {
            let now = Instant::now();
            lock_peer_mgr(peer_mgr).record_message(pid, line.len() as u64, now);
        }

        // Detect AnnounceBlock / AnnounceTx for relay
        let is_announce = matches!(&message, P2pMessage::AnnounceBlock { .. });

        // Transport-layer block-import allowlist enforcement (F1 hardening).
        // Reject block announcements from source IPs that are not on the
        // configured allowlist. Consensus validation still runs for allowed
        // peers; this only blocks unauthorized sources from reaching it.
        if is_announce
            && !block_import_allowlist.is_empty()
            && !block_import_allowlist.contains(&source_ip)
        {
            eprintln!("p2p_block_rejected_unallowed_peer source={source_addr} ip={source_ip}");
            lock_peer_sec(peer_sec).punish(
                source_ip,
                epoch_secs(),
                zion_core::p2p_security::BanReason::ProtocolViolation,
            );
            break;
        }

        let announce_tx_info = match &message {
            P2pMessage::AnnounceTx { tx_id, transaction } => {
                Some((tx_id.clone(), transaction.clone()))
            }
            _ => None,
        };

        let response = match runtime
            .lock()
            .expect("node runtime lock poisoned")
            .handle_p2p_message(message)
        {
            Ok(r) => r,
            Err(reason) => {
                eprintln!("p2p_handle_err source={source_addr} reason={reason}");
                if let Some(ref pid) = peer_id {
                    lock_peer_mgr(peer_mgr)
                        .penalize(pid, zion_core::peer_manager::PENALTY_PROTOCOL_VIOLATION);
                }
                break;
            }
        };

        // Reward valid block imports
        if is_announce {
            if let Some(ref pid) = peer_id {
                lock_peer_mgr(peer_mgr).reward(pid, zion_core::peer_manager::REWARD_VALID_BLOCK);
            }
        }

        let response_line =
            encode_p2p_message(&response).context("failed to encode P2P response")?;
        if writer.write_all(response_line.as_bytes()).is_err() {
            break;
        }
        if writer.flush().is_err() {
            break;
        }
        println!("p2p_out={}", response_line.trim());

        // Relay newly accepted block to other peers (flood-fill)
        if is_announce {
            let rt = runtime.lock().expect("node runtime lock poisoned");
            if let Some(block) = rt.last_accepted_block().cloned() {
                let peers = rt.known_peers().to_vec();
                drop(rt);
                relay_block_to_peers(block, &peers, Some(source_addr), seen, stats);
            }
        }

        // Relay newly accepted transaction to other peers
        if let Some((tx_id, transaction)) = announce_tx_info {
            let peers = runtime.lock().expect("lock").known_peers().to_vec();
            relay_tx_to_peers(
                &tx_id,
                transaction,
                &peers,
                Some(source_addr),
                seen_txs,
                stats,
            );
        }
    }

    // Unregister peer when connection ends
    if let Some(ref pid) = peer_id {
        lock_peer_mgr(peer_mgr).unregister_peer(pid);
    }
    println!("p2p_disconnected source={source_addr}");

    Ok(())
}

fn handle_rpc_stream(
    stream: TcpStream,
    runtime: &Arc<Mutex<NodeRuntime>>,
    seen: &Arc<Mutex<SeenBlocks>>,
    seen_txs: &Arc<Mutex<SeenTransactions>>,
    stats: &Arc<PropagationStats>,
    jsonrpc_router: &Arc<RpcRouter>,
) -> Result<()> {
    // ── RPC hardening: read timeout + body size limit ──────────────────
    stream
        .set_read_timeout(Some(Duration::from_secs(RPC_READ_TIMEOUT_SECS)))
        .ok();
    let reader_stream = stream.try_clone().context("failed to clone RPC stream")?;
    let peer_addr = stream
        .peer_addr()
        .ok()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;

    let line = read_line_limited(&mut reader, RPC_MAX_REQUEST_BYTES)?;
    println!("rpc_in={line}");
    println!("rpc_audit peer={peer_addr} method={line}");

    // ── HTTP POST support for Electron/mobile clients ───────────────────
    // Desktop agent and mobile app send HTTP POST requests; detect and handle them.
    if line.starts_with("POST ") || line.starts_with("GET ") {
        return handle_rpc_http(
            &line,
            &mut reader,
            &mut writer,
            jsonrpc_router,
            runtime,
            seen_txs,
            stats,
        );
    }

    // Protocol detection: JSON-RPC 2.0 requests contain "jsonrpc" key
    let parsed: serde_json::Value = serde_json::from_str(&line).unwrap_or(serde_json::Value::Null);

    let is_jsonrpc = parsed.get("jsonrpc").is_some()
        || parsed
            .as_array()
            .is_some_and(|arr| arr.first().and_then(|v| v.get("jsonrpc")).is_some());

    if is_jsonrpc {
        // JSON-RPC 2.0 protocol
        let response_bytes = jsonrpc_router.handle_raw(line.as_bytes());
        writer
            .write_all(&response_bytes)
            .context("failed to write JSON-RPC response")?;
        writer.write_all(b"\n").context("failed to write newline")?;
        writer
            .flush()
            .context("failed to flush JSON-RPC response")?;
        let trimmed = String::from_utf8_lossy(&response_bytes);
        println!("jsonrpc_out={trimmed}");
        return Ok(());
    }

    // Simple line-delimited protocol (used by pool/miner)
    let request = decode_rpc_request(&line).context("failed to decode RPC request")?;

    // Check if this is a submit that might produce a new block
    let is_submit = matches!(&request, zion_core::RpcRequest::SubmitCandidate { .. });
    let submit_tx = match &request {
        zion_core::RpcRequest::SubmitTransaction { transaction } => {
            Some(SubmittedTransaction::Account(transaction.clone()))
        }
        _ => None,
    };
    let height_before = runtime.lock().expect("lock").chain_height();

    let response = runtime
        .lock()
        .expect("node runtime lock poisoned")
        .handle_rpc_request(request);
    let response_line = encode_rpc_response(&response).context("failed to encode RPC response")?;
    writer
        .write_all(response_line.as_bytes())
        .context("failed to write RPC response")?;
    writer.flush().context("failed to flush RPC response")?;
    println!("rpc_out={}", response_line.trim());

    // Relay newly mined block to all peers
    if is_submit {
        let rt = runtime.lock().expect("lock");
        if rt.chain_height() > height_before {
            if let Some(block) = rt.last_accepted_block().cloned() {
                let peers = rt.known_peers().to_vec();
                drop(rt);
                relay_block_to_peers(block, &peers, None, seen, stats);
            }
        }
    }

    // Relay accepted transaction to peers
    if let Some(submitted) = submit_tx {
        if let zion_core::RpcResponse::TransactionResult {
            accepted: true,
            ref tx_id,
            ..
        } = response
        {
            let peers = runtime.lock().expect("lock").known_peers().to_vec();
            relay_tx_to_peers(tx_id, submitted, &peers, None, seen_txs, stats);
        }
    }

    Ok(())
}

fn read_line(reader: &mut impl BufRead) -> Result<String> {
    let mut line = String::new();
    let read = reader.read_line(&mut line).context("failed to read line")?;
    if read == 0 {
        return Err(anyhow!("connection closed before message"));
    }
    Ok(line.trim().to_string())
}

/// Read a single line with a maximum byte limit to prevent OOM attacks.
fn read_line_limited(reader: &mut impl BufRead, max_bytes: usize) -> Result<String> {
    let mut line = String::new();
    let mut total = 0usize;
    loop {
        let available = reader.fill_buf().context("failed to fill buffer")?;
        if available.is_empty() {
            if total == 0 {
                return Err(anyhow!("connection closed before message"));
            }
            break;
        }
        if let Some(newline_pos) = available.iter().position(|&b| b == b'\n') {
            let chunk = &available[..=newline_pos];
            total += chunk.len();
            if total > max_bytes {
                return Err(anyhow!("RPC request exceeds {max_bytes} byte limit"));
            }
            line.push_str(&String::from_utf8_lossy(chunk));
            reader.consume(newline_pos + 1);
            break;
        } else {
            let len = available.len();
            total += len;
            if total > max_bytes {
                return Err(anyhow!("RPC request exceeds {max_bytes} byte limit"));
            }
            line.push_str(&String::from_utf8_lossy(available));
            reader.consume(len);
        }
    }
    Ok(line.trim().to_string())
}

/// Handle an HTTP POST/GET request on the RPC port.
/// Reads headers to find Content-Length, reads the JSON body, routes through
/// the JSON-RPC router, and writes back a proper HTTP response.
/// CORS headers are included so browser-based wallets can call directly.
/// If a transaction is accepted via submitTransaction, it is relayed to peers.
fn handle_rpc_http(
    first_line: &str,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    router: &Arc<RpcRouter>,
    runtime: &Arc<Mutex<NodeRuntime>>,
    seen_txs: &Arc<Mutex<SeenTransactions>>,
    stats: &Arc<PropagationStats>,
) -> Result<()> {
    // Read HTTP headers
    let mut content_length: usize = 0;
    loop {
        let header_line = {
            let mut buf = String::new();
            reader
                .read_line(&mut buf)
                .context("failed to read HTTP header")?;
            buf
        };
        let trimmed = header_line.trim();
        if trimmed.is_empty() {
            break; // end of headers
        }
        if let Some(value) = trimmed
            .strip_prefix("Content-Length:")
            .or_else(|| trimmed.strip_prefix("content-length:"))
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }

    let is_post = first_line.starts_with("POST ");

    if !is_post || content_length == 0 {
        // GET or empty POST — return health-style JSON
        let body = r#"{"status":"ok","service":"zion-v3-rpc","protocol":"jsonrpc-2.0"}"#;
        let http_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
            body.len(), body
        );
        writer.write_all(http_response.as_bytes())?;
        writer.flush()?;
        return Ok(());
    }

    // Enforce body size limit
    if content_length > RPC_MAX_REQUEST_BYTES {
        let err_body =
            r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"request too large"},"id":null}"#;
        let http_response = format!(
            "HTTP/1.1 413 Payload Too Large\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
            err_body.len(), err_body
        );
        writer.write_all(http_response.as_bytes())?;
        writer.flush()?;
        return Ok(());
    }

    // Read JSON body
    let mut body_buf = vec![0u8; content_length];
    reader
        .read_exact(&mut body_buf)
        .context("failed to read HTTP body")?;
    let body_str = String::from_utf8_lossy(&body_buf);
    println!("rpc_http_in={body_str}");

    // ── Audit log: extract RPC method for security monitoring ──────────
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body_str) {
        if let Some(method) = parsed.get("method").and_then(|m| m.as_str()) {
            let tx_id = parsed
                .get("params")
                .and_then(|p| p.get("transaction"))
                .and_then(|t| t.get("tx_id"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            println!("rpc_audit_http method={method} tx_id={tx_id}");
        }
    }

    // Check if this is a submitTransaction request to extract the transaction for relay
    let submit_tx = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body_str) {
        if let Some(method) = parsed.get("method").and_then(|m| m.as_str()) {
            if method == "submitTransaction"
                || method == "sendRawTransaction"
                || method == "submitAccountTransaction"
            {
                if let Some(params) = parsed.get("params") {
                    let tx_value = params
                        .get("transaction")
                        .cloned()
                        .unwrap_or_else(|| params.clone());
                    println!("rpc_http_tx_relay: attempting to parse transaction for relay");
                    match zion_core::SubmittedTransaction::parse_value(tx_value) {
                        Ok(transaction) => {
                            println!("rpc_http_tx_relay: transaction parsed successfully");
                            Some(transaction)
                        }
                        Err(e) => {
                            println!("rpc_http_tx_relay: failed to parse transaction: {}", e);
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Route through JSON-RPC router
    let response_bytes = router.handle_raw(&body_buf);
    let resp_str = String::from_utf8_lossy(&response_bytes);
    println!("rpc_http_out={resp_str}");

    // Relay accepted transaction to peers (same logic as line-delimited RPC)
    if let Some(submitted) = submit_tx {
        if let Ok(response) = serde_json::from_str::<serde_json::Value>(&resp_str) {
            if let Some(result) = response.get("result") {
                if result
                    .get("accepted")
                    .and_then(|a| a.as_bool())
                    .unwrap_or(false)
                {
                    if let Some(tx_id) = result.get("tx_id").and_then(|t| t.as_str()) {
                        let peers = runtime.lock().expect("lock").known_peers().to_vec();
                        relay_tx_to_peers(tx_id, submitted, &peers, None, seen_txs, stats);
                    }
                }
            }
        }
    }

    let http_response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n",
        response_bytes.len()
    );
    writer.write_all(http_response.as_bytes())?;
    writer.write_all(&response_bytes)?;
    writer.flush()?;
    Ok(())
}

#[derive(Debug, Clone)]
struct NodeServerConfig {
    node_id: String,
    p2p_accept_limit: Option<u32>,
    rpc_accept_limit: Option<u32>,
    sync_batch_limit: u16,
    state_path: Option<String>,
    block_retention: usize,
    #[allow(dead_code)]
    lmdb_map_size_mb: usize,
    node_config: NodeConfig,
}

impl NodeServerConfig {
    fn from_env() -> Result<Self> {
        let mut node_config = NodeConfig::mainnet();
        node_config.network = parse_network_env(node_config.network)?;

        if let Ok(value) = std::env::var("ZION_P2P_BIND") {
            node_config.p2p_bind = parse_endpoint_env(&value, "ZION_P2P_BIND")?;
        }
        if let Ok(value) = std::env::var("ZION_RPC_BIND") {
            node_config.rpc_bind = parse_endpoint_env(&value, "ZION_RPC_BIND")?;
        }
        if let Ok(value) = std::env::var("ZION_POOL_BIND") {
            node_config.pool_bind = parse_endpoint_env(&value, "ZION_POOL_BIND")?;
        }
        if let Ok(value) = std::env::var("ZION_WEBSOCKET_BIND") {
            node_config.websocket_bind = parse_endpoint_env(&value, "ZION_WEBSOCKET_BIND")?;
        }
        if let Ok(value) = std::env::var("ZION_SEED_PEERS") {
            node_config.seed_peers = parse_seed_peers_override(node_config.network, &value)?;
        }

        let shared_accept_limit = parse_accept_limit_env("ZION_ACCEPT_LIMIT", None)?;

        let block_retention = std::env::var("ZION_BLOCK_RETENTION")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(zion_core::DEFAULT_BLOCK_RETENTION);

        let lmdb_map_size_mb = std::env::var("ZION_LMDB_MAP_SIZE_MB")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0); // 0 = use default (10 GB)

        Ok(Self {
            node_id: env_or_default("ZION_NODE_ID", "v3-node-0"),
            p2p_accept_limit: parse_accept_limit_env("ZION_P2P_ACCEPT_LIMIT", shared_accept_limit)?,
            rpc_accept_limit: parse_accept_limit_env("ZION_RPC_ACCEPT_LIMIT", shared_accept_limit)?,
            sync_batch_limit: parse_sync_batch_limit_env()?,
            state_path: std::env::var("ZION_NODE_STATE_PATH").ok(),
            block_retention,
            lmdb_map_size_mb,
            node_config,
        })
    }
}

fn env_or_default(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn parse_network_env(default: zion_core::NetworkId) -> Result<zion_core::NetworkId> {
    match std::env::var("ZION_NETWORK") {
        Ok(value) => parse_network_name(&value),
        Err(_) => Ok(default),
    }
}

fn parse_network_name(value: &str) -> Result<zion_core::NetworkId> {
    match value.trim().to_ascii_lowercase().as_str() {
        "mainnet" => Ok(zion_core::NetworkId::Mainnet),
        "testnet" => Ok(zion_core::NetworkId::Testnet),
        "devnet" => Ok(zion_core::NetworkId::Devnet),
        other => Err(anyhow!(
            "invalid ZION_NETWORK: {other} (expected mainnet, testnet, or devnet)"
        )),
    }
}

fn parse_endpoint_env(value: &str, key: &str) -> Result<PeerEndpoint> {
    PeerEndpoint::parse(value).map_err(|reason| anyhow!("{key}: {reason}"))
}

fn parse_accept_limit_env(key: &str, default: Option<u32>) -> Result<Option<u32>> {
    match std::env::var(key) {
        Ok(value) => value
            .parse::<u32>()
            .map(Some)
            .with_context(|| format!("invalid u32 in {key}: {value}")),
        Err(_) => Ok(default),
    }
}

fn parse_sync_batch_limit_env() -> Result<u16> {
    match std::env::var("ZION_SYNC_BATCH_LIMIT") {
        Ok(value) => value
            .parse::<u16>()
            .with_context(|| format!("invalid u16 in ZION_SYNC_BATCH_LIMIT: {value}")),
        Err(_) => Ok(32),
    }
}

fn parse_seed_peers_env(value: &str) -> Result<Vec<PeerEndpoint>> {
    value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| parse_endpoint_env(entry, "ZION_SEED_PEERS"))
        .collect::<Result<Vec<_>>>()
}

fn parse_seed_peers_override(
    network: zion_core::NetworkId,
    value: &str,
) -> Result<Vec<PeerEndpoint>> {
    if value.eq_ignore_ascii_case("none") || value.eq_ignore_ascii_case("empty") {
        if network == zion_core::NetworkId::Mainnet {
            return Err(anyhow!(
                "ZION_SEED_PEERS=none|empty is not allowed on mainnet"
            ));
        }
        return Ok(Vec::new());
    }
    parse_seed_peers_env(value)
}

fn bootstrap_peer_sync(runtime: &Arc<Mutex<NodeRuntime>>, batch_limit: u16) -> Result<()> {
    let self_p2p_bind = {
        runtime
            .lock()
            .expect("node runtime lock poisoned")
            .config()
            .p2p_bind
            .address()
    };
    let mut pending = VecDeque::from(
        runtime
            .lock()
            .expect("node runtime lock poisoned")
            .known_peers()
            .to_vec(),
    );
    let mut seen = HashSet::new();

    while let Some(peer) = pending.pop_front() {
        let address = peer.address();
        if address == self_p2p_bind || !seen.insert(address.clone()) {
            continue;
        }
        match sync_from_peer(runtime, &peer, batch_limit.max(1)) {
            Ok(discovered) => {
                for peer in discovered {
                    let peer_address = peer.address();
                    if peer_address != self_p2p_bind && !seen.contains(&peer_address) {
                        pending.push_back(peer);
                    }
                }
            }
            Err(error) => {
                eprintln!("peer_sync_failed peer={address} reason={error}");
            }
        }
    }

    Ok(())
}

fn sync_from_peer(
    runtime: &Arc<Mutex<NodeRuntime>>,
    peer: &PeerEndpoint,
    batch_limit: u16,
) -> Result<Vec<PeerEndpoint>> {
    let hello = {
        runtime
            .lock()
            .expect("node runtime lock poisoned")
            .p2p_hello()
    };
    let welcome = p2p_roundtrip(peer, &hello)?;
    let discovered = match welcome {
        zion_core::P2pMessage::Welcome { peers, .. } => peers,
        other => return Err(anyhow!("unexpected hello response: {other:?}")),
    };

    {
        let mut runtime = runtime.lock().expect("node runtime lock poisoned");
        runtime.register_peer(peer.clone());
        runtime.register_peers(discovered.clone());
    }

    let status = match p2p_roundtrip(peer, &zion_core::P2pMessage::GetStatus)? {
        zion_core::P2pMessage::Status { status } => status,
        other => return Err(anyhow!("unexpected status response: {other:?}")),
    };

    {
        let mut runtime = runtime.lock().expect("node runtime lock poisoned");
        runtime.register_peers(status.known_peers.clone());
    }

    loop {
        let from_height = {
            let runtime = runtime.lock().expect("node runtime lock poisoned");
            if !runtime.needs_blocks_from(status.chain_height) {
                break;
            }
            runtime.chain_height()
        };

        let blocks = match p2p_roundtrip(
            peer,
            &zion_core::P2pMessage::GetBlocksSince {
                from_height,
                limit: batch_limit,
            },
        )? {
            zion_core::P2pMessage::Blocks { blocks } => blocks,
            other => return Err(anyhow!("unexpected block sync response: {other:?}")),
        };

        if blocks.is_empty() {
            return Err(anyhow!(
                "peer {} advertised height {} but returned no blocks after {}",
                peer.address(),
                status.chain_height,
                from_height
            ));
        }

        let imported = runtime
            .lock()
            .expect("node runtime lock poisoned")
            .import_peer_blocks(blocks)
            .map_err(anyhow::Error::msg)?;
        if imported == 0 {
            break;
        }
    }

    Ok(discovered)
}

fn p2p_roundtrip(
    peer: &PeerEndpoint,
    message: &zion_core::P2pMessage,
) -> Result<zion_core::P2pMessage> {
    let addr = peer
        .address()
        .to_socket_addrs()
        .with_context(|| format!("failed to resolve peer {}", peer.address()))?
        .next()
        .with_context(|| format!("no address for peer {}", peer.address()))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
        .with_context(|| format!("failed to connect to peer {}", peer.address()))?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let line = encode_p2p_message(message).context("failed to encode outbound P2P message")?;
    stream
        .write_all(line.as_bytes())
        .context("failed to write outbound P2P message")?;
    stream
        .flush()
        .context("failed to flush outbound P2P message")?;

    let mut reader = BufReader::new(stream);
    let response = read_line(&mut reader)?;
    decode_p2p_message(&response).context("failed to decode inbound P2P response")
}

/// Relay a newly accepted block to all eligible peers via flood-fill.
/// Spawns a background thread per peer so the caller is not blocked.
fn relay_block_to_peers(
    block: AcceptedBlock,
    peers: &[PeerEndpoint],
    source_addr: Option<&str>,
    seen: &Arc<Mutex<SeenBlocks>>,
    stats: &Arc<PropagationStats>,
) {
    use zion_core::propagation::plan_relay;

    let plan = {
        let mut seen_guard = seen.lock().expect("seen lock poisoned");
        plan_relay(
            &block.hash_hex,
            block.height,
            peers,
            source_addr,
            &mut seen_guard,
        )
    };

    let plan = match plan {
        Some(p) => p,
        None => {
            stats.record_duplicate();
            return;
        }
    };

    let target_count = plan.targets.len() as u64;
    stats.record_relay(target_count);
    println!(
        "relay_block height={} hash={:.16}… targets={}",
        plan.block_height, plan.block_hash, target_count
    );

    let mut handles: Vec<thread::JoinHandle<()>> = Vec::new();
    for target in plan.targets {
        // Cap concurrent relay threads
        if handles.len() >= MAX_RELAY_THREADS {
            let h = handles.remove(0);
            let _ = h.join();
        }
        let block = block.clone();
        let stats = Arc::clone(stats);
        handles.push(thread::spawn(move || {
            let msg = P2pMessage::AnnounceBlock { block };
            match p2p_roundtrip(&target.peer, &msg) {
                Ok(_) => {
                    println!("relay_ok peer={}", target.peer.address());
                    stats.record_success();
                }
                Err(e) => {
                    eprintln!("relay_err peer={} reason={e}", target.peer.address());
                    stats.record_failure();
                }
            }
        }));
    }
}

/// Relay a newly accepted transaction to all eligible peers via flood-fill.
fn relay_tx_to_peers(
    tx_id: &str,
    transaction: SubmittedTransaction,
    peers: &[PeerEndpoint],
    source_addr: Option<&str>,
    seen_txs: &Arc<Mutex<SeenTransactions>>,
    stats: &Arc<PropagationStats>,
) {
    use zion_core::propagation::plan_tx_relay;

    let plan = {
        let mut seen_guard = seen_txs.lock().expect("seen_txs lock poisoned");
        plan_tx_relay(tx_id, peers, source_addr, &mut seen_guard)
    };

    let plan = match plan {
        Some(p) => p,
        None => {
            stats.record_tx_duplicate();
            return;
        }
    };

    let target_count = plan.targets.len() as u64;
    stats.record_tx_relay(target_count);
    println!(
        "relay_tx tx_id={:.16}… targets={}",
        plan.tx_id, target_count
    );

    let mut handles: Vec<thread::JoinHandle<()>> = Vec::new();
    for target in plan.targets {
        // Cap concurrent relay threads
        if handles.len() >= MAX_RELAY_THREADS {
            let h = handles.remove(0);
            let _ = h.join();
        }
        let tx_id = tx_id.to_string();
        let transaction = transaction.clone();
        let stats = Arc::clone(stats);
        handles.push(thread::spawn(move || {
            let msg = P2pMessage::AnnounceTx {
                tx_id: tx_id.clone(),
                transaction,
            };
            match p2p_roundtrip(&target.peer, &msg) {
                Ok(_) => {
                    println!("tx_relay_ok peer={}", target.peer.address());
                    stats.record_tx_success();
                }
                Err(e) => {
                    eprintln!("tx_relay_err peer={} reason={e}", target.peer.address());
                    stats.record_tx_failure();
                }
            }
        }));
    }
}

/// Outbound peer loop: periodically connects to known peers, syncs new blocks,
/// sends heartbeat pings, discovers new peers via GetPeers, and runs
/// PeerManager maintenance.
const OUTBOUND_CYCLE_SECS: u64 = 30;
/// Maximum concurrent relay threads per block/tx announcement.
const MAX_RELAY_THREADS: usize = 16;
/// Run peer discovery (GetPeers) every N cycles (~5 min).
const DISCOVERY_EVERY_N_CYCLES: u64 = 10;
/// Persist known_peers to disk every N cycles (~5 min).
const PERSIST_PEERS_EVERY_N_CYCLES: u64 = 10;
/// How many consecutive sync failures at the same height triggers fork recovery.
const FORK_RECOVERY_THRESHOLD: u32 = 10;
/// Maximum RPC request body size (64 KiB — prevents OOM from oversized payloads).
const RPC_MAX_REQUEST_BYTES: usize = 65_536;
/// RPC connection read timeout (30 seconds).
const RPC_READ_TIMEOUT_SECS: u64 = 30;

fn outbound_peer_loop(
    runtime: &Arc<Mutex<NodeRuntime>>,
    seen: &Arc<Mutex<SeenBlocks>>,
    stats: &Arc<PropagationStats>,
    peer_mgr: &Arc<Mutex<PeerManager>>,
    peer_sec: &Arc<Mutex<PeerSecurity>>,
    batch_limit: u16,
) {
    let mut last_heartbeat = Instant::now();
    let mut last_cleanup = Instant::now();
    let mut cycle_count: u64 = 0;
    let mut sync_fail_count: u32 = 0;
    let mut sync_fail_height: u64 = 0;

    // ── Discovery engine setup ─────────────────────────────────────────
    let network_name = {
        let rt = runtime.lock().expect("lock");
        match rt.config().network {
            zion_core::NetworkId::Mainnet => "mainnet",
            zion_core::NetworkId::Testnet => "testnet",
            zion_core::NetworkId::Devnet => "devnet",
        }
        .to_string()
    };
    let mut discovery = DiscoveryEngine::new(&network_name);
    // Set self address so we don't try to connect to ourselves
    {
        let rt = runtime.lock().expect("lock");
        let bind = rt.config().p2p_bind.address();
        if let Some((host, port_str)) = bind.rsplit_once(':') {
            if let (Ok(ip), Ok(port)) = (host.parse::<IpAddr>(), port_str.parse::<u16>()) {
                discovery.set_self_addr(ip, port);
            }
        }
        let bootstrap_nodes: Vec<(IpAddr, u16)> = rt
            .known_peers()
            .iter()
            .filter_map(|peer| {
                let address = peer.address();
                let (host, port_str) = address.rsplit_once(':')?;
                let ip = host.parse::<IpAddr>().ok()?;
                let port = port_str.parse::<u16>().ok()?;
                Some((ip, port.saturating_add(2)))
            })
            .collect();
        discovery.set_bootstrap_nodes(bootstrap_nodes);
        if !rt.known_peers().is_empty() {
            discovery.set_dns_seeds(Vec::new());
        }
    }
    // Seed discovery with the current runtime peer list instead of hardcoded legacy hosts.
    for peer in runtime.lock().expect("lock").known_peers().iter() {
        if let Some((host, port_str)) = peer.address().rsplit_once(':') {
            if let (Ok(addr), Ok(port)) = (host.parse::<IpAddr>(), port_str.parse::<u16>()) {
                discovery.add_from_dns(addr, port, epoch_secs());
            }
        }
    }
    // Optional: bind UDP socket for announcements (non-blocking, best-effort)
    let udp_socket = UdpSocket::bind("0.0.0.0:0").ok().inspect(|s| {
        s.set_nonblocking(true).ok();
    });

    // ── IBD engine setup ───────────────────────────────────────────────
    let initial_height = runtime.lock().expect("lock").chain_height();
    let mut ibd = IbdEngine::new(initial_height);

    loop {
        thread::sleep(Duration::from_secs(OUTBOUND_CYCLE_SECS));
        cycle_count += 1;

        // Run PeerManager heartbeat
        let actions = {
            let now = Instant::now();
            if now.duration_since(last_heartbeat) >= Duration::from_secs(60) {
                last_heartbeat = now;
                lock_peer_mgr(peer_mgr).heartbeat(now)
            } else {
                Vec::new()
            }
        };

        for action in &actions {
            match action {
                PeerAction::Disconnect { peer_id, reason } => {
                    println!("peer_action_disconnect peer={peer_id} reason={reason}");
                }
                PeerAction::Ban { peer_id, reason } => {
                    println!("peer_action_ban peer={peer_id} reason={reason}");
                    // Propagate ban to PeerSecurity so inbound connections are rejected
                    if let Some(peer_state) = peer_mgr
                        .lock()
                        .expect("lock")
                        .peer_info()
                        .iter()
                        .find(|p| p.peer_id == *peer_id)
                    {
                        peer_sec.lock().expect("lock").punish(
                            peer_state.addr,
                            epoch_secs(),
                            zion_core::p2p_security::BanReason::Manual,
                        );
                    }
                }
                PeerAction::ConnectOutbound { addr, port } => {
                    println!("peer_action_connect_outbound addr={addr}:{port}");
                    let endpoint = match PeerEndpoint::parse(&format!("{addr}:{port}")) {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    // Try sync from this new peer
                    match sync_from_peer(runtime, &endpoint, batch_limit.max(1)) {
                        Ok(_) => println!("outbound_sync_ok peer={addr}:{port}"),
                        Err(e) => eprintln!("outbound_sync_err peer={addr}:{port} err={e}"),
                    }
                }
            }
        }

        // Proactive sync: check each known peer for new blocks
        let peers = runtime.lock().expect("lock").known_peers().to_vec();
        let our_height = runtime.lock().expect("lock").chain_height();
        let our_tip = runtime.lock().expect("lock").tip_hash_hex();

        // ── Fork detection: compare our tip with peer tips ─────────────
        let peers_ahead = 0u32;
        let peers_disagree_tip = 0u32;

        for peer in &peers {
            // Quick status check via Ping to keep connection alive (persistent)
            match p2p_roundtrip(
                peer,
                &P2pMessage::Ping {
                    nonce: epoch_secs(),
                },
            ) {
                Ok(P2pMessage::Pong { .. }) => {
                    // Peer alive — check if it has new blocks
                    if let Ok(P2pMessage::Status { status }) =
                        p2p_roundtrip(peer, &P2pMessage::GetStatus)
                    {
                        // Feed height into IBD engine
                        ibd.update_peer(&peer.address(), status.chain_height);
                        if status.chain_height > our_height {
                            println!(
                                "outbound_sync peer={} remote_height={} our_height={}",
                                peer.address(),
                                status.chain_height,
                                our_height,
                            );
                            match sync_from_peer(runtime, peer, batch_limit.max(1)) {
                                Ok(_) => {
                                    // Sync succeeded — reset failure counter
                                    sync_fail_count = 0;
                                }
                                Err(e) => {
                                    eprintln!("outbound_sync_err peer={} err={e}", peer.address());
                                    // Track consecutive failures at the same height
                                    let current_height =
                                        runtime.lock().expect("lock").chain_height();
                                    if current_height == sync_fail_height {
                                        sync_fail_count += 1;
                                    } else {
                                        sync_fail_height = current_height;
                                        sync_fail_count = 1;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(_) => {
                    // Peer unreachable — will be cleaned up by heartbeat timeout
                }
                _ => {}
            }
        }

        // ── Fork recovery: if sync is stuck at the same height for too
        //    many cycles and ALL reachable peers disagree with our tip,
        //    reset to genesis and re-IBD from the canonical chain. ──────
        if sync_fail_count >= FORK_RECOVERY_THRESHOLD
            && peers_ahead > 0
            && peers_disagree_tip == peers_ahead
        {
            eprintln!(
                "fork_detected height={} tip={} failures={} disagreeing_peers={}/{}",
                our_height, our_tip, sync_fail_count, peers_disagree_tip, peers_ahead,
            );
            let mut rt = runtime.lock().expect("lock");
            if let Err(e) = rt.reset_to_genesis() {
                eprintln!("fork_recovery_failed err={e}");
            } else {
                drop(rt);
                sync_fail_count = 0;
                sync_fail_height = 0;
                // Immediately try to resync from first available peer
                for peer in &peers {
                    match sync_from_peer(runtime, peer, batch_limit.max(1)) {
                        Ok(_) => {
                            eprintln!("fork_recovery_ibd_started peer={}", peer.address());
                            break;
                        }
                        Err(e) => {
                            eprintln!("fork_recovery_ibd_err peer={} err={e}", peer.address());
                        }
                    }
                }
            }
        }

        // ── Peer discovery: ask a peer for its known peers ─────────────
        if cycle_count.is_multiple_of(DISCOVERY_EVERY_N_CYCLES) && !peers.is_empty() {
            let idx = (cycle_count / DISCOVERY_EVERY_N_CYCLES) as usize % peers.len();
            let target = &peers[idx];
            match p2p_roundtrip(target, &P2pMessage::GetPeers) {
                Ok(P2pMessage::Peers { peers: discovered }) => {
                    let new_count = discovered.len();
                    if new_count > 0 {
                        let mut rt = runtime.lock().expect("lock");
                        let before = rt.peer_count();
                        rt.register_peers(discovered.clone());
                        let after = rt.peer_count();
                        drop(rt);
                        if after > before {
                            println!(
                                "peer_discovery from={} new={} total={}",
                                target.address(),
                                after - before,
                                after,
                            );
                            // Add new peers as seeds in PeerManager
                            let new_seeds: Vec<(IpAddr, u16)> = discovered
                                .iter()
                                .filter_map(|p| {
                                    p.address().rsplit_once(':').and_then(|(h, port)| {
                                        let ip: IpAddr = h.parse().ok()?;
                                        let port: u16 = port.parse().ok()?;
                                        Some((ip, port))
                                    })
                                })
                                .collect();
                            lock_peer_mgr(peer_mgr).add_seeds(&new_seeds);
                        }
                    }
                }
                Ok(_) => {} // unexpected response — ignore
                Err(e) => {
                    eprintln!("peer_discovery_err peer={} err={e}", target.address());
                }
            }
        }

        // ── Persist known_peers to disk ────────────────────────────────
        if cycle_count.is_multiple_of(PERSIST_PEERS_EVERY_N_CYCLES) {
            let rt = runtime.lock().expect("lock");
            if let Err(e) = rt.persist_peers() {
                eprintln!("peers_persist_err err={e}");
            }
        }

        // ── Cleanup expired bans in PeerSecurity (~every 5 min) ────────
        if last_cleanup.elapsed() >= Duration::from_secs(300) {
            last_cleanup = Instant::now();
            peer_sec.lock().expect("lock").cleanup(epoch_secs());
        }

        // ── Discovery engine tick ──────────────────────────────────────
        {
            let connected_peer_ids: Vec<String> = peer_mgr
                .lock()
                .expect("lock")
                .peer_info()
                .iter()
                .map(|p| p.peer_id.clone())
                .collect();
            let current_count = connected_peer_ids.len();
            let now = Instant::now();
            let now_secs = epoch_secs();
            let commands = discovery.tick(
                now,
                now_secs,
                &connected_peer_ids,
                current_count,
                MIN_OUTBOUND,
            );
            for cmd in commands {
                match cmd {
                    DiscoveryCommand::ResolveDns { hostname } => {
                        // Resolve DNS seed → feed results into discovery
                        let target = format!("{hostname}:{}", DISCOVERY_PORT.saturating_sub(1));
                        if let Ok(addrs) = target.to_socket_addrs() {
                            for addr in addrs {
                                discovery.add_from_dns(addr.ip(), addr.port(), now_secs);
                                // Also register in peer manager + runtime
                                let ep = PeerEndpoint::new(addr.ip().to_string(), addr.port());
                                runtime.lock().expect("lock").register_peer(ep);
                            }
                            println!("discovery_dns hostname={hostname}");
                        }
                    }
                    DiscoveryCommand::SendAnnounce { addr, port } => {
                        if let Some(ref sock) = udp_socket {
                            let rt = runtime.lock().expect("lock");
                            let height = rt.chain_height();
                            let version = node_protocol_version();
                            let bind = rt.config().p2p_bind.address();
                            drop(rt);
                            // Extract our host:port for the announcement
                            if let Some((host, port_str)) = bind.rsplit_once(':') {
                                if let Ok(p) = port_str.parse::<u16>() {
                                    let data = discovery
                                        .build_announcement(host, p, version, height, now_secs);
                                    let _ = sock.send_to(&data, (addr, port));
                                }
                            }
                        }
                    }
                    DiscoveryCommand::RequestPeers { peer_id } => {
                        // Find endpoint for this peer and request peers via P2P
                        let endpoint = peer_mgr
                            .lock()
                            .expect("lock")
                            .peer_info()
                            .iter()
                            .find(|p| p.peer_id == peer_id)
                            .map(|p| PeerEndpoint::new(p.addr.to_string(), p.port));
                        if let Some(ep) = endpoint {
                            if let Ok(P2pMessage::Peers { peers: found }) =
                                p2p_roundtrip(&ep, &P2pMessage::GetPeers)
                            {
                                for p in &found {
                                    if let Some((h, port_str)) = p.address().rsplit_once(':') {
                                        if let (Ok(ip), Ok(port)) =
                                            (h.parse::<IpAddr>(), port_str.parse::<u16>())
                                        {
                                            discovery
                                                .add_from_peer_exchange(ip, port, None, now_secs);
                                        }
                                    }
                                }
                                runtime.lock().expect("lock").register_peers(found);
                            }
                        }
                    }
                    DiscoveryCommand::TryConnect { addr, port } => {
                        let ep = PeerEndpoint::new(addr.to_string(), port);
                        match sync_from_peer(runtime, &ep, batch_limit.max(1)) {
                            Ok(_) => println!("discovery_connect_ok peer={addr}:{port}"),
                            Err(e) => eprintln!("discovery_connect_err peer={addr}:{port} err={e}"),
                        }
                    }
                }
            }
        }

        // ── IBD engine tick ────────────────────────────────────────────
        {
            let now = Instant::now();
            ibd.set_local_height(our_height);
            let ibd_commands = ibd.tick(now);
            for cmd in ibd_commands {
                match cmd {
                    IbdCommand::RequestBatch {
                        peer_id,
                        start_height,
                        count,
                    } => {
                        // Find peer endpoint from PeerManager
                        let endpoint = peer_mgr
                            .lock()
                            .expect("lock")
                            .peer_info()
                            .iter()
                            .find(|p| p.peer_id == peer_id)
                            .map(|p| PeerEndpoint::new(p.addr.to_string(), p.port));
                        if let Some(ep) = endpoint {
                            match p2p_roundtrip(
                                &ep,
                                &P2pMessage::GetBlocksSince {
                                    from_height: start_height,
                                    limit: count.min(u16::MAX as u64) as u16,
                                },
                            ) {
                                Ok(P2pMessage::Blocks { blocks }) => {
                                    ibd.batch_received(start_height);
                                    let imported = runtime
                                        .lock()
                                        .expect("lock")
                                        .import_peer_blocks(blocks)
                                        .unwrap_or(0);
                                    if imported > 0 {
                                        let new_height =
                                            runtime.lock().expect("lock").chain_height();
                                        ibd.blocks_applied(new_height);
                                        println!("ibd_batch start={start_height} imported={imported} height={new_height}");
                                    }
                                }
                                _ => {
                                    ibd.batch_received(start_height);
                                }
                            }
                        }
                    }
                    IbdCommand::DemotePeer { peer_id, reason } => {
                        println!("ibd_demote peer={peer_id} reason={reason}");
                        peer_mgr.lock().expect("lock").penalize(
                            &peer_id,
                            zion_core::peer_manager::PENALTY_PROTOCOL_VIOLATION,
                        );
                    }
                    IbdCommand::IbdComplete => {
                        println!("ibd_complete height={}", ibd.local_height());
                    }
                }
            }
        }

        // Relay our latest block to any peer that might be behind
        let rt = runtime.lock().expect("lock");
        if let Some(block) = rt.last_accepted_block().cloned() {
            let known = rt.known_peers().to_vec();
            drop(rt);
            relay_block_to_peers(block, &known, None, seen, stats);
        }
    }
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Metrics HTTP server ────────────────────────────────────────────────
// Serves Prometheus text exposition at /metrics and JSON health at /health.
// Reads NodeRuntime::status() on each request — no background thread needed.

fn serve_node_metrics(bind_addr: &str, runtime: Arc<Mutex<NodeRuntime>>) -> Result<()> {
    let listener = TcpListener::bind(bind_addr)
        .with_context(|| format!("failed to bind metrics listener on {bind_addr}"))?;

    for incoming in listener.incoming() {
        let mut stream = match incoming {
            Ok(s) => s,
            Err(e) => {
                eprintln!("metrics_accept_err={e}");
                continue;
            }
        };

        let mut reader = BufReader::new(&stream);
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            continue;
        }
        let path = request_line.split_whitespace().nth(1).unwrap_or("/metrics");

        let status = runtime.lock().expect("runtime lock poisoned").status();
        let peer_count = status.known_peers.len() as i64;

        let (code, content_type, body) = match path {
            "/health" => {
                let body = format!(
                    r#"{{"status":"ok","chain_height":{},"tip_hash":"{}","peer_count":{},"mempool_size":{}}}"#,
                    status.chain_height,
                    status.tip_hash_hex,
                    peer_count,
                    status.mempool_transactions,
                );
                ("200 OK", "application/json", body)
            }
            _ => {
                let mut out = String::with_capacity(2048);
                let _ = writeln!(out, "# HELP zion_chain_height Current chain tip height.");
                let _ = writeln!(out, "# TYPE zion_chain_height gauge");
                let _ = writeln!(out, "zion_chain_height {}", status.chain_height);
                let _ = writeln!(
                    out,
                    "# HELP zion_mempool_size Number of transactions in mempool."
                );
                let _ = writeln!(out, "# TYPE zion_mempool_size gauge");
                let _ = writeln!(out, "zion_mempool_size {}", status.mempool_transactions);
                let _ = writeln!(out, "# HELP zion_peer_count Total connected peers.");
                let _ = writeln!(out, "# TYPE zion_peer_count gauge");
                let _ = writeln!(out, "zion_peer_count {peer_count}");
                let _ = writeln!(
                    out,
                    "# HELP zion_blocks_accepted_total Total blocks accepted."
                );
                let _ = writeln!(out, "# TYPE zion_blocks_accepted_total gauge");
                let _ = writeln!(out, "zion_blocks_accepted_total {}", status.accepted_blocks);
                let _ = writeln!(
                    out,
                    "# HELP zion_template_height Active block template height."
                );
                let _ = writeln!(out, "# TYPE zion_template_height gauge");
                let _ = writeln!(
                    out,
                    "zion_template_height {}",
                    status.active_template_height
                );
                let _ = writeln!(
                    out,
                    "# HELP zion_template_txs Transactions in active template."
                );
                let _ = writeln!(out, "# TYPE zion_template_txs gauge");
                let _ = writeln!(
                    out,
                    "zion_template_txs {}",
                    status.active_template_transactions
                );
                let _ = writeln!(
                    out,
                    "# HELP zion_template_fees_zion Total fees in active template."
                );
                let _ = writeln!(out, "# TYPE zion_template_fees_zion gauge");
                let _ = writeln!(
                    out,
                    "zion_template_fees_zion {}",
                    status.active_template_total_fees_zion
                );
                let _ = writeln!(
                    out,
                    "# HELP zion_tip_hash_info Current chain tip hash (label)."
                );
                let _ = writeln!(out, "# TYPE zion_tip_hash_info gauge");
                let _ = writeln!(
                    out,
                    "zion_tip_hash_info{{tip_hash=\"{}\"}} 1",
                    status.tip_hash_hex
                );
                ("200 OK", "text/plain; version=0.0.4", out)
            }
        };

        let response = format!(
            "HTTP/1.1 {code}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        if let Err(e) = stream.write_all(response.as_bytes()) {
            eprintln!("metrics_write_err={e}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_network_name_accepts_testnet() {
        let network = parse_network_name("testnet").expect("network");
        assert_eq!(network, zion_core::NetworkId::Testnet);
    }

    #[test]
    fn parse_network_name_rejects_unknown_network() {
        let error = parse_network_name("weirdnet").unwrap_err();
        assert!(error.to_string().contains("invalid ZION_NETWORK"));
    }

    #[test]
    fn parse_seed_peers_override_rejects_none_on_mainnet() {
        let error = parse_seed_peers_override(zion_core::NetworkId::Mainnet, "none").unwrap_err();
        assert!(error.to_string().contains("not allowed on mainnet"));
    }

    #[test]
    fn parse_seed_peers_override_allows_none_on_testnet() {
        let peers = parse_seed_peers_override(zion_core::NetworkId::Testnet, "empty")
            .expect("testnet empty seed peers should be allowed");
        assert!(peers.is_empty());
    }

    #[test]
    fn parse_seed_peers_override_parses_explicit_list() {
        let peers = parse_seed_peers_override(
            zion_core::NetworkId::Mainnet,
            "127.0.0.1:8333,<LEGACY_EDGE>:8333",
        )
        .expect("explicit seed peer list should parse");
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].address(), "127.0.0.1:8333");
        assert_eq!(peers[1].address(), "<LEGACY_EDGE>:8333");
    }
}
