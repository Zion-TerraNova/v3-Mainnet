// Phase 9b — Peer discovery via UDP announcements and DNS seeds
//
// Ported from TREE_NODES/discovery/node_discovery.py into V3 Rust.
//
// Provides:
//   - `DiscoveredPeer`: metadata about a peer found through discovery
//   - `DiscoveryEngine`: pure state machine that manages a pool of discovered
//     peers from multiple sources (DNS seeds, peer exchange, UDP announcements)
//   - UDP announcement protocol: JSON-encoded node_announce messages on port 8335
//   - Peer expiry, deduplication, and source tracking
//
// This module is a pure state machine — no I/O. The node runtime is responsible
// for actually sending/receiving UDP packets, resolving DNS, and connecting.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

// ── Constants ──────────────────────────────────────────────────────────

/// UDP port for node announcements.
pub const DISCOVERY_PORT: u16 = 8335;

/// How often to run the discovery cycle (seconds).
pub const DISCOVERY_INTERVAL: Duration = Duration::from_secs(300);

/// Maximum number of discovered peers to track.
pub const MAX_DISCOVERED: usize = 1000;

/// Peers not seen for this long are pruned.
pub const PEER_EXPIRY: Duration = Duration::from_secs(24 * 3600);

/// Maximum age (in seconds) of a UDP announcement before discarding.
pub const MAX_ANNOUNCE_AGE: u64 = 600;

/// DNS seed hostnames for initial peer discovery.
///
/// Single-server 3.0.4 topology — configure additional seeds via ZION_SEED_PEERS env var.
pub const DNS_SEEDS: &[&str] = &[];

/// Well-known bootstrap nodes for UDP announcements.
pub const BOOTSTRAP_NODES: &[(&str, u16)] = &[
    // 3.0.4 canonical mainnet server (old Edge <LEGACY_EDGE> decommissioned 2026-07-07).
    ("<ZION_SEED_PEER>", 8335),
];

// ── Types ──────────────────────────────────────────────────────────────

/// How a peer was discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoverySource {
    /// Resolved from a DNS seed hostname.
    Dns,
    /// Received via peer exchange (getaddr/addr protocol messages).
    PeerExchange,
    /// Received via UDP announcement on port 8335.
    UdpAnnounce,
    /// Loaded from a bootstrap/seed list.
    Bootstrap,
    /// Loaded from a persisted discovered_peers.json file.
    Persisted,
}

/// A peer discovered through the discovery subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPeer {
    /// IP address of the peer.
    pub addr: IpAddr,
    /// P2P port of the peer.
    pub port: u16,
    /// When this peer was first seen.
    pub first_seen_secs: u64,
    /// When this peer was last seen (UNIX seconds for serialization).
    pub last_seen_secs: u64,
    /// How the peer was discovered.
    pub source: DiscoverySource,
    /// Self-reported protocol version (if available).
    pub version: Option<String>,
    /// Self-reported chain height (if available).
    pub height: Option<u64>,
    /// Self-reported service flags (if available).
    pub services: Option<u64>,
}

/// A UDP node_announce message (JSON encoded).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAnnounce {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub height: Option<u64>,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default)]
    pub timestamp: Option<u64>,
}

/// Commands produced by the discovery engine for the node runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryCommand {
    /// Resolve this DNS seed hostname and report results back.
    ResolveDns { hostname: String },
    /// Send a UDP announcement to this address.
    SendAnnounce { addr: IpAddr, port: u16 },
    /// Request peer addresses from this connected peer.
    RequestPeers { peer_id: String },
    /// Try connecting to this discovered peer.
    TryConnect { addr: IpAddr, port: u16 },
}

// ── Discovery Engine ───────────────────────────────────────────────────

/// Pure state machine for peer discovery. Manages a pool of discovered
/// peers and produces commands for the node runtime to execute.
pub struct DiscoveryEngine {
    /// Discovered peers keyed by "addr:port".
    peers: HashMap<String, DiscoveredPeer>,
    /// DNS seed index for round-robin resolution.
    dns_index: usize,
    /// DNS seeds used for round-robin resolution.
    dns_seeds: Vec<String>,
    /// Last time we ran a full discovery cycle.
    last_cycle: Option<Instant>,
    /// Our own external address (if known), to avoid self-connection.
    self_addr: Option<(IpAddr, u16)>,
    /// Bootstrap peers to receive UDP announcements / initial discovery nudges.
    bootstrap_nodes: Vec<(IpAddr, u16)>,
    /// Network identifier for filtering announcements (e.g., "mainnet", "testnet").
    network: String,
}

impl DiscoveryEngine {
    /// Create a new discovery engine for the given network.
    pub fn new(network: &str) -> Self {
        Self {
            peers: HashMap::new(),
            dns_index: 0,
            dns_seeds: DNS_SEEDS.iter().map(|s| (*s).to_string()).collect(),
            last_cycle: None,
            self_addr: None,
            bootstrap_nodes: Vec::new(),
            network: network.to_string(),
        }
    }

    /// Set our own address to avoid self-connection.
    pub fn set_self_addr(&mut self, addr: IpAddr, port: u16) {
        self.self_addr = Some((addr, port));
    }

    /// Replace the current bootstrap node list used for UDP announcements.
    pub fn set_bootstrap_nodes(&mut self, nodes: Vec<(IpAddr, u16)>) {
        self.bootstrap_nodes = nodes;
    }

    /// Replace the current DNS seed list.
    pub fn set_dns_seeds(&mut self, seeds: Vec<String>) {
        self.dns_seeds = seeds;
        if self.dns_index >= self.dns_seeds.len() {
            self.dns_index = 0;
        }
    }

    /// Number of tracked discovered peers.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Get all discovered peers, sorted by most recently seen.
    pub fn peers_by_recency(&self) -> Vec<&DiscoveredPeer> {
        let mut peers: Vec<&DiscoveredPeer> = self.peers.values().collect();
        peers.sort_by(|a, b| b.last_seen_secs.cmp(&a.last_seen_secs));
        peers
    }

    /// Get N peers suitable for connection attempts (most recently seen, not self).
    pub fn peers_for_connection(&self, count: usize) -> Vec<(&IpAddr, u16)> {
        self.peers_by_recency()
            .into_iter()
            .filter(|p| {
                if let Some((self_ip, self_port)) = &self.self_addr {
                    !(p.addr == *self_ip && p.port == *self_port)
                } else {
                    true
                }
            })
            .take(count)
            .map(|p| (&p.addr, p.port))
            .collect()
    }

    /// Add or update a peer from DNS resolution.
    pub fn add_from_dns(&mut self, addr: IpAddr, port: u16, now_secs: u64) {
        self.upsert(addr, port, DiscoverySource::Dns, None, None, now_secs);
    }

    /// Add or update a peer from peer exchange (getaddr response).
    pub fn add_from_peer_exchange(
        &mut self,
        addr: IpAddr,
        port: u16,
        height: Option<u64>,
        now_secs: u64,
    ) {
        self.upsert(
            addr,
            port,
            DiscoverySource::PeerExchange,
            None,
            height,
            now_secs,
        );
    }

    /// Process an incoming UDP node_announce message.
    /// Returns true if the announcement was accepted.
    pub fn process_announcement(&mut self, data: &[u8], now_secs: u64) -> bool {
        // Parse JSON
        let announce: NodeAnnounce = match serde_json::from_slice(data) {
            Ok(a) => a,
            Err(_) => return false,
        };

        // Validate message type
        if announce.msg_type != "node_announce" {
            return false;
        }

        // Validate network if specified
        if let Some(ref net) = announce.network {
            if net != &self.network {
                return false;
            }
        }

        // Validate timestamp freshness (reject stale/future announcements)
        if let Some(ts) = announce.timestamp {
            if now_secs > ts && now_secs - ts > MAX_ANNOUNCE_AGE {
                return false;
            }
            if ts > now_secs + 60 {
                return false;
            }
        }

        // Parse host address
        let addr: IpAddr = match announce.host.parse() {
            Ok(a) => a,
            Err(_) => return false,
        };

        // Reject if it's our own announcement
        if let Some((self_ip, self_port)) = &self.self_addr {
            if addr == *self_ip && announce.port == *self_port {
                return false;
            }
        }

        self.upsert(
            addr,
            announce.port,
            DiscoverySource::UdpAnnounce,
            announce.version,
            announce.height,
            now_secs,
        );

        true
    }

    /// Build a UDP announcement message for ourselves.
    pub fn build_announcement(
        &self,
        host: &str,
        port: u16,
        version: &str,
        height: u64,
        now_secs: u64,
    ) -> Vec<u8> {
        let announce = NodeAnnounce {
            msg_type: "node_announce".to_string(),
            host: host.to_string(),
            port,
            version: Some(version.to_string()),
            height: Some(height),
            network: Some(self.network.clone()),
            timestamp: Some(now_secs),
        };
        serde_json::to_vec(&announce).unwrap_or_default()
    }

    /// Run a discovery tick. Produces commands for the node runtime.
    /// `connected_peers`: list of currently connected peer IDs for peer exchange.
    /// `current_peer_count`: how many peers we currently have connected.
    /// `min_peers`: minimum connected peers we want.
    pub fn tick(
        &mut self,
        now: Instant,
        now_secs: u64,
        connected_peers: &[String],
        current_peer_count: usize,
        min_peers: usize,
    ) -> Vec<DiscoveryCommand> {
        let mut commands = Vec::new();

        // Check if it's time for a discovery cycle
        let should_cycle = match self.last_cycle {
            None => true,
            Some(last) => now.duration_since(last) >= DISCOVERY_INTERVAL,
        };

        if !should_cycle && current_peer_count >= min_peers {
            return commands;
        }

        if should_cycle {
            self.last_cycle = Some(now);

            // Prune expired peers
            self.prune(now_secs);

            // Resolve next DNS seed
            if self.dns_index < self.dns_seeds.len() {
                commands.push(DiscoveryCommand::ResolveDns {
                    hostname: self.dns_seeds[self.dns_index].clone(),
                });
                self.dns_index = (self.dns_index + 1) % self.dns_seeds.len();
            }

            // Request peers from a few connected peers
            for peer_id in connected_peers.iter().take(3) {
                commands.push(DiscoveryCommand::RequestPeers {
                    peer_id: peer_id.clone(),
                });
            }

            // Send UDP announcement to configured bootstrap nodes.
            for &(addr, port) in &self.bootstrap_nodes {
                commands.push(DiscoveryCommand::SendAnnounce { addr, port });
            }
        }

        // If we need more peers, suggest connections from discovered pool
        if current_peer_count < min_peers {
            let deficit = min_peers - current_peer_count;
            let candidates = self.peers_for_connection(deficit);
            for (addr, port) in candidates {
                commands.push(DiscoveryCommand::TryConnect { addr: *addr, port });
            }
        }

        commands
    }

    /// Load previously persisted peers from JSON.
    pub fn load_persisted(&mut self, json: &str) -> Result<usize, String> {
        let peers: Vec<DiscoveredPeer> =
            serde_json::from_str(json).map_err(|e| format!("invalid peers JSON: {e}"))?;
        let count = peers.len();
        for mut p in peers {
            p.source = DiscoverySource::Persisted;
            let key = format!("{}:{}", p.addr, p.port);
            if !self.peers.contains_key(&key) && self.peers.len() < MAX_DISCOVERED {
                self.peers.insert(key, p);
            }
        }
        Ok(count)
    }

    /// Export current peers as JSON for persistence.
    pub fn export_json(&self) -> String {
        let peers: Vec<&DiscoveredPeer> = self.peers.values().collect();
        serde_json::to_string_pretty(&peers).unwrap_or_else(|_| "[]".to_string())
    }

    // ── Internal helpers ───────────────────────────────────────────────

    fn upsert(
        &mut self,
        addr: IpAddr,
        port: u16,
        source: DiscoverySource,
        version: Option<String>,
        height: Option<u64>,
        now_secs: u64,
    ) {
        let key = format!("{addr}:{port}");

        if let Some(existing) = self.peers.get_mut(&key) {
            existing.last_seen_secs = now_secs;
            if let Some(v) = version {
                existing.version = Some(v);
            }
            if let Some(h) = height {
                existing.height = Some(h);
            }
        } else if self.peers.len() < MAX_DISCOVERED {
            self.peers.insert(
                key,
                DiscoveredPeer {
                    addr,
                    port,
                    first_seen_secs: now_secs,
                    last_seen_secs: now_secs,
                    source,
                    version,
                    height,
                    services: None,
                },
            );
        }
    }

    fn prune(&mut self, now_secs: u64) {
        let expiry_secs = PEER_EXPIRY.as_secs();
        self.peers
            .retain(|_, p| now_secs.saturating_sub(p.last_seen_secs) < expiry_secs);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn new_engine_is_empty() {
        let engine = DiscoveryEngine::new("testnet");
        assert_eq!(engine.peer_count(), 0);
    }

    #[test]
    fn add_from_dns() {
        let mut engine = DiscoveryEngine::new("testnet");
        engine.add_from_dns(ip(10, 0, 0, 1), 8334, 1000);
        assert_eq!(engine.peer_count(), 1);

        let peers = engine.peers_by_recency();
        assert_eq!(peers[0].addr, ip(10, 0, 0, 1));
        assert_eq!(peers[0].source, DiscoverySource::Dns);
    }

    #[test]
    fn add_from_peer_exchange() {
        let mut engine = DiscoveryEngine::new("testnet");
        engine.add_from_peer_exchange(ip(10, 0, 0, 1), 8334, Some(1000), 2000);
        assert_eq!(engine.peer_count(), 1);

        let peers = engine.peers_by_recency();
        assert_eq!(peers[0].height, Some(1000));
        assert_eq!(peers[0].source, DiscoverySource::PeerExchange);
    }

    #[test]
    fn upsert_updates_last_seen() {
        let mut engine = DiscoveryEngine::new("testnet");
        engine.add_from_dns(ip(10, 0, 0, 1), 8334, 1000);
        engine.add_from_dns(ip(10, 0, 0, 1), 8334, 2000);
        assert_eq!(engine.peer_count(), 1);

        let peers = engine.peers_by_recency();
        assert_eq!(peers[0].last_seen_secs, 2000);
        assert_eq!(peers[0].first_seen_secs, 1000); // unchanged
    }

    #[test]
    fn process_valid_announcement() {
        let mut engine = DiscoveryEngine::new("testnet");
        let announce = serde_json::json!({
            "type": "node_announce",
            "host": "10.0.0.1",
            "port": 8334,
            "version": "zion-v3-node/0.1",
            "height": 500,
            "network": "testnet",
            "timestamp": 1000
        });
        let data = serde_json::to_vec(&announce).unwrap();
        assert!(engine.process_announcement(&data, 1000));
        assert_eq!(engine.peer_count(), 1);
    }

    #[test]
    fn reject_wrong_network() {
        let mut engine = DiscoveryEngine::new("testnet");
        let announce = serde_json::json!({
            "type": "node_announce",
            "host": "10.0.0.1",
            "port": 8334,
            "network": "mainnet",
            "timestamp": 1000
        });
        let data = serde_json::to_vec(&announce).unwrap();
        assert!(!engine.process_announcement(&data, 1000));
        assert_eq!(engine.peer_count(), 0);
    }

    #[test]
    fn reject_stale_announcement() {
        let mut engine = DiscoveryEngine::new("testnet");
        let announce = serde_json::json!({
            "type": "node_announce",
            "host": "10.0.0.1",
            "port": 8334,
            "network": "testnet",
            "timestamp": 100
        });
        let data = serde_json::to_vec(&announce).unwrap();
        // now_secs = 100 + MAX_ANNOUNCE_AGE + 1 = too old
        assert!(!engine.process_announcement(&data, 100 + MAX_ANNOUNCE_AGE + 1));
    }

    #[test]
    fn reject_own_announcement() {
        let mut engine = DiscoveryEngine::new("testnet");
        engine.set_self_addr(ip(10, 0, 0, 1), 8334);
        let announce = serde_json::json!({
            "type": "node_announce",
            "host": "10.0.0.1",
            "port": 8334,
            "network": "testnet",
            "timestamp": 1000
        });
        let data = serde_json::to_vec(&announce).unwrap();
        assert!(!engine.process_announcement(&data, 1000));
    }

    #[test]
    fn max_discovered_limit() {
        let mut engine = DiscoveryEngine::new("testnet");
        for i in 0..MAX_DISCOVERED + 10 {
            let a = ((i >> 8) & 0xFF) as u8;
            let b = (i & 0xFF) as u8;
            engine.add_from_dns(ip(10, 0, a, b), 8334, i as u64);
        }
        assert_eq!(engine.peer_count(), MAX_DISCOVERED);
    }

    #[test]
    fn prune_expired_peers() {
        let mut engine = DiscoveryEngine::new("testnet");
        engine.add_from_dns(ip(10, 0, 0, 1), 8334, 1000);
        engine.add_from_dns(ip(10, 0, 0, 2), 8334, 2000);

        let expiry = PEER_EXPIRY.as_secs();
        engine.prune(1000 + expiry - 1); // both peers are still within expiry
        assert_eq!(engine.peer_count(), 2);

        engine.prune(1000 + expiry + 1); // peer at 1000 is expired, peer at 2000 is not
        assert_eq!(engine.peer_count(), 1);
    }

    #[test]
    fn build_announcement_format() {
        let engine = DiscoveryEngine::new("testnet");
        let data = engine.build_announcement("10.0.0.1", 8334, "zion-v3/0.1", 100, 5000);
        let parsed: NodeAnnounce = serde_json::from_slice(&data).unwrap();
        assert_eq!(parsed.msg_type, "node_announce");
        assert_eq!(parsed.host, "10.0.0.1");
        assert_eq!(parsed.port, 8334);
        assert_eq!(parsed.height, Some(100));
        assert_eq!(parsed.network.as_deref(), Some("testnet"));
    }

    #[test]
    fn tick_produces_dns_and_announce_commands() {
        // The production `DNS_SEEDS` slice is currently empty (mainnet has not
        // committed to a public DNS seed surface yet). The test seeds the
        // engine explicitly so it exercises the DNS-resolve branch deterministically
        // — without this `set_dns_seeds` call the assertion below would
        // depend on whether anyone has populated `DNS_SEEDS`, which was the
        // root cause of the historical "flaky DNS test" on `main`.
        let mut engine = DiscoveryEngine::new("testnet");
        engine.set_dns_seeds(vec!["rehearsal-seed.invalid".into()]);
        engine.set_bootstrap_nodes(vec![(ip(10, 0, 0, 1), DISCOVERY_PORT)]);
        let now = Instant::now();
        let cmds = engine.tick(now, 1000, &["peer1".into()], 0, 8);

        let dns: Vec<_> = cmds
            .iter()
            .filter(|c| matches!(c, DiscoveryCommand::ResolveDns { .. }))
            .collect();
        assert!(
            !dns.is_empty(),
            "tick() must emit at least one ResolveDns command when dns_seeds is non-empty"
        );

        let announces: Vec<_> = cmds
            .iter()
            .filter(|c| matches!(c, DiscoveryCommand::SendAnnounce { .. }))
            .collect();
        assert!(
            !announces.is_empty(),
            "tick() must emit at least one SendAnnounce command when bootstrap_nodes is non-empty"
        );
    }

    /// Mirrors the historical default (`DNS_SEEDS = &[]`) and pins the
    /// invariant that `tick()` must NOT emit `ResolveDns` when there are no
    /// configured DNS seeds. This complements `tick_produces_dns_and_announce_commands`
    /// so future contributors do not accidentally re-introduce the empty-seed
    /// failure that the explicit `set_dns_seeds` call above hides.
    #[test]
    fn tick_emits_no_dns_when_seeds_empty() {
        let mut engine = DiscoveryEngine::new("testnet");
        engine.set_bootstrap_nodes(vec![(ip(10, 0, 0, 1), DISCOVERY_PORT)]);
        // Default `DNS_SEEDS` is empty; do NOT call `set_dns_seeds`.
        assert_eq!(engine.dns_seeds.len(), DNS_SEEDS.len());
        let now = Instant::now();
        let cmds = engine.tick(now, 1000, &["peer1".into()], 0, 8);
        let dns: Vec<_> = cmds
            .iter()
            .filter(|c| matches!(c, DiscoveryCommand::ResolveDns { .. }))
            .collect();
        assert!(
            dns.is_empty(),
            "tick() must not emit ResolveDns when dns_seeds is empty"
        );
    }

    #[test]
    fn tick_suggests_connections_when_below_min() {
        let mut engine = DiscoveryEngine::new("testnet");
        engine.add_from_dns(ip(10, 0, 0, 1), 8334, 1000);
        engine.add_from_dns(ip(10, 0, 0, 2), 8334, 1000);

        let now = Instant::now();
        let cmds = engine.tick(now, 1000, &[], 0, 8);

        let connects: Vec<_> = cmds
            .iter()
            .filter(|c| matches!(c, DiscoveryCommand::TryConnect { .. }))
            .collect();
        assert!(!connects.is_empty());
    }

    #[test]
    fn json_roundtrip() {
        let mut engine = DiscoveryEngine::new("testnet");
        engine.add_from_dns(ip(10, 0, 0, 1), 8334, 1000);
        engine.add_from_dns(ip(10, 0, 0, 2), 8334, 2000);

        let json = engine.export_json();

        let mut engine2 = DiscoveryEngine::new("testnet");
        let loaded = engine2.load_persisted(&json).unwrap();
        assert_eq!(loaded, 2);
        assert_eq!(engine2.peer_count(), 2);
    }

    #[test]
    fn peers_for_connection_excludes_self() {
        let mut engine = DiscoveryEngine::new("testnet");
        engine.set_self_addr(ip(10, 0, 0, 1), 8334);
        engine.add_from_dns(ip(10, 0, 0, 1), 8334, 1000); // self
        engine.add_from_dns(ip(10, 0, 0, 2), 8334, 1000);

        let candidates = engine.peers_for_connection(10);
        assert_eq!(candidates.len(), 1);
        assert_eq!(*candidates[0].0, ip(10, 0, 0, 2));
    }
}
