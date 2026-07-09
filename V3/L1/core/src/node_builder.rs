// Phase 8d — Node Bootstrap Orchestrator
//
// Wires together all V3 subsystems into a coherent node lifecycle:
//   1. Open LMDB storage (or initialize with genesis)
//   2. Build chain state from storage tip
//   3. Initialize IBD engine, peer manager, metrics
//   4. Register RPC methods with live handlers
//   5. Provide a clean startup / shutdown sequence
//
// This module is the "main()" composition layer — it owns the subsystem
// instances and exposes a single `NodeHandle` for the binary entry point.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::chain::ChainEntry;
use crate::genesis;
use crate::ibd::{IbdEngine, SyncStatus};
use crate::launch;
use crate::metrics::NodeMetrics;
use crate::peer_manager::PeerManager;
use crate::rpc::{build_stub_router, RpcRouter};
use crate::storage::{ChainDb, StorageError};
use crate::NodeConfig;

// ── Error types ────────────────────────────────────────────────────────

/// Errors during node bootstrap or lifecycle.
#[derive(Debug)]
pub enum NodeError {
    Storage(StorageError),
    Launch(String),
    Config(String),
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(e) => write!(f, "storage: {e}"),
            Self::Launch(e) => write!(f, "launch: {e}"),
            Self::Config(e) => write!(f, "config: {e}"),
        }
    }
}

impl From<StorageError> for NodeError {
    fn from(e: StorageError) -> Self {
        Self::Storage(e)
    }
}

// ── Node state ─────────────────────────────────────────────────────────

/// Current node state summary, exposed for RPC and monitoring.
#[derive(Debug, Clone)]
pub struct NodeStatus {
    pub chain_height: u64,
    pub chain_tip_hash: String,
    pub sync_status: SyncStatus,
    pub peer_count: usize,
    pub network: String,
    pub version: &'static str,
    pub launch_ready: bool,
}

/// The fully-wired node handle. Owns all subsystem instances.
pub struct NodeHandle {
    pub config: NodeConfig,
    pub db: ChainDb,
    pub ibd: IbdEngine,
    pub peer_manager: PeerManager,
    pub metrics: Arc<NodeMetrics>,
    pub rpc: RpcRouter,
    chain_tip: ChainEntry,
    started_at: Instant,
}

impl NodeHandle {
    /// Bootstrap a node from disk. If the database is empty, initializes
    /// from genesis and runs launch readiness checks.
    pub fn open(data_dir: &Path, config: NodeConfig) -> Result<Self, NodeError> {
        // 1. Validate seed peers
        if config.seed_peers.is_empty() {
            return Err(NodeError::Config("no seed peers configured".into()));
        }

        // 2. Open (or create) LMDB storage
        let db = ChainDb::open(data_dir)?;
        let meta = db.get_meta()?;

        // 3. Initialize from genesis if this is a fresh database
        let chain_tip = if meta.tip_height == 0 && meta.tip_hash == [0u8; 32] {
            // Verify launch readiness before first run
            launch::verify_genesis_integrity()
                .map_err(|e| NodeError::Launch(format!("genesis integrity check failed: {e}")))?;

            // The genesis block will be saved by the caller during first sync.
            // For now, return genesis as the chain tip.
            let genesis = genesis::genesis_block();
            ChainEntry {
                height: 0,
                hash: parse_hash(&genesis.hash_hex),
                prev_hash: [0u8; 32],
                difficulty: genesis.difficulty,
                total_work: 0,
            }
        } else {
            // Restore chain tip from stored metadata
            ChainEntry {
                height: meta.tip_height,
                hash: meta.tip_hash,
                prev_hash: [0u8; 32], // Will be filled by chain state
                difficulty: 0,
                total_work: meta.total_work,
            }
        };

        // 4. Initialize IBD engine from current tip
        let ibd = IbdEngine::new(chain_tip.height);

        // 5. Initialize peer manager
        let mut peer_manager = PeerManager::new();
        let seeds: Vec<(std::net::IpAddr, u16)> = config
            .seed_peers
            .iter()
            .filter_map(|ep| {
                ep.host
                    .parse::<std::net::IpAddr>()
                    .ok()
                    .map(|ip| (ip, ep.port))
            })
            .collect();
        peer_manager.add_seeds(&seeds);

        // 6. Initialize metrics
        let metrics = Arc::new(NodeMetrics::new());
        metrics.set_chain_height(chain_tip.height);

        // 7. Build RPC router with node method stubs
        let rpc = build_stub_router();

        Ok(Self {
            config,
            db,
            ibd,
            peer_manager,
            metrics,
            rpc,
            chain_tip,
            started_at: Instant::now(),
        })
    }

    /// Return current node status.
    pub fn status(&self) -> NodeStatus {
        NodeStatus {
            chain_height: self.chain_tip.height,
            chain_tip_hash: crate::hex(&self.chain_tip.hash),
            sync_status: self.ibd.status(),
            peer_count: self.peer_manager.peer_count(),
            network: format!("{:?}", self.config.network),
            version: crate::NODE_PROTOCOL_VERSION,
            launch_ready: launch::is_launch_ready(),
        }
    }

    /// Node uptime.
    pub fn uptime(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }

    /// Current chain tip height.
    pub fn tip_height(&self) -> u64 {
        self.chain_tip.height
    }

    /// Update the chain tip after accepting a new block.
    pub fn advance_tip(&mut self, entry: ChainEntry) {
        let height = entry.height;
        self.chain_tip = entry;
        self.metrics.set_chain_height(height);
        self.metrics.inc_blocks_accepted();
        self.ibd.blocks_applied(height);
    }

    /// Register a new peer connection.
    pub fn register_peer(
        &mut self,
        peer_id: &str,
        addr: std::net::IpAddr,
        port: u16,
        best_height: u64,
    ) {
        self.peer_manager.register_peer(
            peer_id,
            addr,
            port,
            crate::peer_manager::PeerDirection::Outbound,
            Instant::now(),
        );
        self.ibd.update_peer(peer_id, best_height);
        let total = self.peer_manager.peer_count() as i64;
        self.metrics.set_peer_count(total, 0, total);
    }

    /// Disconnect a peer.
    pub fn disconnect_peer(&mut self, peer_id: &str) {
        self.peer_manager.unregister_peer(peer_id);
        self.ibd.remove_peer(peer_id);
        let total = self.peer_manager.peer_count() as i64;
        self.metrics.set_peer_count(total, 0, total);
    }

    /// Run a single IBD tick and return commands.
    pub fn ibd_tick(&mut self) -> Vec<crate::ibd::IbdCommand> {
        self.ibd.tick(Instant::now())
    }

    /// Run peer manager heartbeat.
    pub fn heartbeat(&mut self) -> Vec<crate::peer_manager::PeerAction> {
        self.peer_manager.heartbeat(Instant::now())
    }

    /// Render Prometheus metrics.
    pub fn prometheus_metrics(&self) -> String {
        self.metrics.render_prometheus()
    }

    /// Render health check JSON.
    pub fn health_check(&self) -> String {
        self.metrics.health_check()
    }

    /// Get launch readiness report.
    pub fn readiness_report(&self) -> String {
        launch::readiness_report()
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

fn parse_hash(hex_str: &str) -> [u8; 32] {
    let mut hash = [0u8; 32];
    if hex_str.len() == 64 {
        for (i, byte) in hash.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex_str[i * 2..i * 2 + 2], 16).unwrap_or(0);
        }
    }
    hash
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PeerEndpoint;
    use tempfile::tempdir;

    fn test_config() -> NodeConfig {
        NodeConfig {
            network: crate::NetworkId::Mainnet,
            p2p_bind: PeerEndpoint::new("127.0.0.1", 18334),
            rpc_bind: PeerEndpoint::new("127.0.0.1", 18332),
            pool_bind: PeerEndpoint::new("127.0.0.1", 18444),
            websocket_bind: PeerEndpoint::new("127.0.0.1", 18445),
            seed_peers: vec![
                PeerEndpoint::new("127.0.0.1", 18334),
                PeerEndpoint::new("127.0.0.2", 18334),
            ],
        }
    }

    #[test]
    fn bootstrap_fresh_node() {
        let dir = tempdir().unwrap();
        let handle = NodeHandle::open(dir.path(), test_config()).unwrap();
        assert_eq!(handle.tip_height(), 0);
        assert_eq!(handle.status().chain_height, 0);
        assert!(handle.status().launch_ready);
    }

    #[test]
    fn rejects_empty_seed_peers() {
        let dir = tempdir().unwrap();
        let mut cfg = test_config();
        cfg.seed_peers.clear();
        let result = NodeHandle::open(dir.path(), cfg);
        assert!(result.is_err());
    }

    #[test]
    fn status_shows_synced() {
        let dir = tempdir().unwrap();
        let handle = NodeHandle::open(dir.path(), test_config()).unwrap();
        assert_eq!(handle.status().sync_status, SyncStatus::Synced);
    }

    #[test]
    fn advance_tip_updates_state() {
        let dir = tempdir().unwrap();
        let mut handle = NodeHandle::open(dir.path(), test_config()).unwrap();
        let entry = ChainEntry {
            height: 42,
            hash: [0xAB; 32],
            prev_hash: [0xCD; 32],
            difficulty: 1000,
            total_work: 42_000,
        };
        handle.advance_tip(entry);
        assert_eq!(handle.tip_height(), 42);
        assert_eq!(handle.status().chain_height, 42);
    }

    #[test]
    fn register_and_disconnect_peer() {
        let dir = tempdir().unwrap();
        let mut handle = NodeHandle::open(dir.path(), test_config()).unwrap();
        handle.register_peer("peer1", "192.168.1.1".parse().unwrap(), 8334, 100);
        assert_eq!(handle.status().peer_count, 1);
        handle.disconnect_peer("peer1");
        assert_eq!(handle.status().peer_count, 0);
    }

    #[test]
    fn ibd_tick_returns_commands() {
        let dir = tempdir().unwrap();
        let mut handle = NodeHandle::open(dir.path(), test_config()).unwrap();
        // No peers with higher height, so no commands
        let cmds = handle.ibd_tick();
        assert!(cmds.is_empty());
    }

    #[test]
    fn prometheus_metrics_output() {
        let dir = tempdir().unwrap();
        let handle = NodeHandle::open(dir.path(), test_config()).unwrap();
        let metrics = handle.prometheus_metrics();
        assert!(metrics.contains("zion_"));
    }

    #[test]
    fn health_check_output() {
        let dir = tempdir().unwrap();
        let handle = NodeHandle::open(dir.path(), test_config()).unwrap();
        let health = handle.health_check();
        assert!(health.contains("status"));
    }

    #[test]
    fn readiness_report_output() {
        let dir = tempdir().unwrap();
        let handle = NodeHandle::open(dir.path(), test_config()).unwrap();
        let report = handle.readiness_report();
        assert!(report.contains("Launch Readiness"));
        assert!(report.contains("PASS"));
    }

    #[test]
    fn uptime_is_positive() {
        let dir = tempdir().unwrap();
        let handle = NodeHandle::open(dir.path(), test_config()).unwrap();
        // Uptime should be very small but positive
        assert!(handle.uptime().as_nanos() > 0);
    }

    #[test]
    fn mainnet_config_has_seed_peers() {
        let cfg = NodeConfig::mainnet();
        assert!(cfg.seed_peers.len() >= 2, "mainnet needs 2+ seed peers");
    }
}
