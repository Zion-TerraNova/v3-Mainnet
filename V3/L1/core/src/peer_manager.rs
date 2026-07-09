// Phase 7d — Peer manager
//
// Audit reference: #26
//
// Tracks connected peers with scoring, subnet diversity, automatic banning
// for misbehavior, and dead peer detection. Pure state machine — no I/O.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

// ── Constants ──────────────────────────────────────────────────────────

/// Maximum connected peers.
pub const MAX_PEERS: usize = 128;

/// Minimum outbound peer connections to maintain.
pub const MIN_OUTBOUND: usize = 8;

/// Maximum peers per /16 subnet (IPv4) for diversity.
pub const MAX_PER_SUBNET: usize = 4;

/// How often to check for dead peers (seconds).
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);

/// Peer idle timeout — drop if no message for this long.
pub const PEER_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Initial score for a new peer.
pub const INITIAL_SCORE: i32 = 100;

/// Score at or below which a peer is automatically banned.
pub const BAN_THRESHOLD: i32 = -100;

/// Score penalty for sending an invalid block.
pub const PENALTY_INVALID_BLOCK: i32 = -50;

/// Score penalty for sending an invalid transaction.
pub const PENALTY_INVALID_TX: i32 = -10;

/// Score penalty for protocol violation.
pub const PENALTY_PROTOCOL_VIOLATION: i32 = -30;

/// Score bonus for providing a valid block.
pub const REWARD_VALID_BLOCK: i32 = 20;

/// Score bonus for providing valid transactions.
pub const REWARD_VALID_TX: i32 = 1;

/// Score bonus for responding to requests promptly.
pub const REWARD_FAST_RESPONSE: i32 = 5;

// ── Types ──────────────────────────────────────────────────────────────

/// Direction of the peer connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerDirection {
    Inbound,
    Outbound,
}

/// State of a tracked peer.
#[derive(Debug, Clone)]
pub struct PeerState {
    pub addr: IpAddr,
    pub port: u16,
    pub peer_id: String,
    pub direction: PeerDirection,
    pub score: i32,
    pub connected_at: Instant,
    pub last_seen: Instant,
    pub best_height: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub latency_ms: Option<u64>,
    pub banned: bool,
}

/// Actions the peer manager requests the node runtime to take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerAction {
    /// Disconnect this peer.
    Disconnect { peer_id: String, reason: String },
    /// Ban this peer.
    Ban { peer_id: String, reason: String },
    /// Connect to a new outbound peer.
    ConnectOutbound { addr: IpAddr, port: u16 },
}

// ── Peer Manager ───────────────────────────────────────────────────────

pub struct PeerManager {
    peers: HashMap<String, PeerState>,
    /// Seed addresses for outbound connections.
    seeds: Vec<(IpAddr, u16)>,
    /// Index for round-robin seed selection.
    seed_index: usize,
}

impl PeerManager {
    pub fn new() -> Self {
        Self {
            peers: HashMap::new(),
            seeds: Vec::new(),
            seed_index: 0,
        }
    }

    /// Add seed addresses for outbound connection attempts.
    pub fn add_seeds(&mut self, seeds: &[(IpAddr, u16)]) {
        for s in seeds {
            if !self.seeds.contains(s) {
                self.seeds.push(*s);
            }
        }
    }

    /// Total connected peer count.
    pub fn peer_count(&self) -> usize {
        self.peers.values().filter(|p| !p.banned).count()
    }

    /// Outbound peer count.
    pub fn outbound_count(&self) -> usize {
        self.peers
            .values()
            .filter(|p| !p.banned && p.direction == PeerDirection::Outbound)
            .count()
    }

    /// Count how many peers share a /16 subnet with the given IP.
    fn subnet_count(&self, addr: &IpAddr) -> usize {
        let prefix = subnet_prefix(addr);
        self.peers
            .values()
            .filter(|p| !p.banned && subnet_prefix(&p.addr) == prefix)
            .count()
    }

    /// Try to register a new peer. Returns false if rejected (full, subnet limit, etc.).
    pub fn register_peer(
        &mut self,
        peer_id: &str,
        addr: IpAddr,
        port: u16,
        direction: PeerDirection,
        now: Instant,
    ) -> bool {
        if self.peers.contains_key(peer_id) {
            return false; // duplicate
        }
        if self.peer_count() >= MAX_PEERS {
            return false;
        }
        if self.subnet_count(&addr) >= MAX_PER_SUBNET {
            return false;
        }

        self.peers.insert(
            peer_id.to_string(),
            PeerState {
                addr,
                port,
                peer_id: peer_id.to_string(),
                direction,
                score: INITIAL_SCORE,
                connected_at: now,
                last_seen: now,
                best_height: 0,
                bytes_sent: 0,
                bytes_received: 0,
                latency_ms: None,
                banned: false,
            },
        );
        true
    }

    /// Unregister a peer (disconnected).
    pub fn unregister_peer(&mut self, peer_id: &str) {
        self.peers.remove(peer_id);
    }

    /// Record that we received a message from this peer.
    pub fn record_message(&mut self, peer_id: &str, bytes: u64, now: Instant) {
        if let Some(p) = self.peers.get_mut(peer_id) {
            p.last_seen = now;
            p.bytes_received += bytes;
        }
    }

    /// Record that we sent a message to this peer.
    pub fn record_sent(&mut self, peer_id: &str, bytes: u64) {
        if let Some(p) = self.peers.get_mut(peer_id) {
            p.bytes_sent += bytes;
        }
    }

    /// Update a peer's best known height.
    pub fn update_height(&mut self, peer_id: &str, height: u64) {
        if let Some(p) = self.peers.get_mut(peer_id) {
            p.best_height = height;
        }
    }

    /// Record latency for this peer (e.g. from ping/pong).
    pub fn record_latency(&mut self, peer_id: &str, latency_ms: u64) {
        if let Some(p) = self.peers.get_mut(peer_id) {
            p.latency_ms = Some(latency_ms);
            if latency_ms < 500 {
                p.score = (p.score + REWARD_FAST_RESPONSE).min(500);
            }
        }
    }

    /// Apply a score penalty to a peer.
    pub fn penalize(&mut self, peer_id: &str, penalty: i32) {
        if let Some(p) = self.peers.get_mut(peer_id) {
            p.score -= penalty.abs();
        }
    }

    /// Apply a score reward to a peer.
    pub fn reward(&mut self, peer_id: &str, bonus: i32) {
        if let Some(p) = self.peers.get_mut(peer_id) {
            p.score = (p.score + bonus.abs()).min(500);
        }
    }

    /// Get a peer's current score.
    pub fn get_score(&self, peer_id: &str) -> Option<i32> {
        self.peers.get(peer_id).map(|p| p.score)
    }

    /// Get best known height across all peers.
    pub fn best_peer_height(&self) -> u64 {
        self.peers
            .values()
            .filter(|p| !p.banned)
            .map(|p| p.best_height)
            .max()
            .unwrap_or(0)
    }

    /// Get peers sorted by score (highest first) for block download.
    pub fn peers_by_score(&self) -> Vec<&PeerState> {
        let mut peers: Vec<&PeerState> = self.peers.values().filter(|p| !p.banned).collect();
        peers.sort_by(|a, b| b.score.cmp(&a.score));
        peers
    }

    /// Run periodic maintenance: detect dead peers, auto-ban low-score, suggest outbound connects.
    pub fn heartbeat(&mut self, now: Instant) -> Vec<PeerAction> {
        let mut actions = Vec::new();

        // Collect peer_ids to disconnect/ban (avoid borrow issues)
        let mut to_disconnect = Vec::new();
        let mut to_ban = Vec::new();

        for (peer_id, peer) in &self.peers {
            if peer.banned {
                continue;
            }

            // Dead peer detection
            if now.duration_since(peer.last_seen) >= PEER_IDLE_TIMEOUT {
                to_disconnect.push((peer_id.clone(), "idle timeout".to_string()));
                continue;
            }

            // Auto-ban low-score peers
            if peer.score <= BAN_THRESHOLD {
                to_ban.push((peer_id.clone(), format!("score too low: {}", peer.score)));
            }
        }

        for (peer_id, reason) in to_disconnect {
            actions.push(PeerAction::Disconnect {
                peer_id: peer_id.clone(),
                reason,
            });
            self.peers.remove(&peer_id);
        }

        for (peer_id, reason) in to_ban {
            if let Some(p) = self.peers.get_mut(&peer_id) {
                p.banned = true;
            }
            actions.push(PeerAction::Ban { peer_id, reason });
        }

        // Suggest outbound connections if below minimum
        if self.outbound_count() < MIN_OUTBOUND && !self.seeds.is_empty() {
            let deficit = MIN_OUTBOUND - self.outbound_count();
            for _ in 0..deficit {
                if self.seeds.is_empty() {
                    break;
                }
                let idx = self.seed_index % self.seeds.len();
                let (addr, port) = self.seeds[idx];
                self.seed_index += 1;

                // Skip if already connected to this address
                let already = self
                    .peers
                    .values()
                    .any(|p| p.addr == addr && p.port == port);
                if !already {
                    actions.push(PeerAction::ConnectOutbound { addr, port });
                }
            }
        }

        actions
    }

    /// Get a snapshot of all peer states (for RPC getPeerInfo).
    pub fn peer_info(&self) -> Vec<PeerState> {
        self.peers.values().cloned().collect()
    }
}

impl Default for PeerManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract /16 subnet prefix from an IP address for diversity checking.
fn subnet_prefix(addr: &IpAddr) -> u16 {
    match addr {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            ((octets[0] as u16) << 8) | octets[1] as u16
        }
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            segments[0]
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn now() -> Instant {
        Instant::now()
    }

    fn ip(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn register_and_count_peers() {
        let mut pm = PeerManager::new();
        let t = now();
        assert!(pm.register_peer("p1", ip(10, 0, 0, 1), 8333, PeerDirection::Outbound, t));
        assert!(pm.register_peer("p2", ip(10, 1, 0, 1), 8333, PeerDirection::Inbound, t));
        assert_eq!(pm.peer_count(), 2);
        assert_eq!(pm.outbound_count(), 1);
    }

    #[test]
    fn reject_duplicate_peer() {
        let mut pm = PeerManager::new();
        let t = now();
        assert!(pm.register_peer("p1", ip(10, 0, 0, 1), 8333, PeerDirection::Outbound, t));
        assert!(!pm.register_peer("p1", ip(10, 0, 0, 1), 8333, PeerDirection::Outbound, t));
    }

    #[test]
    fn subnet_diversity_limit() {
        let mut pm = PeerManager::new();
        let t = now();
        // All in 10.0.x.x /16 — should hit MAX_PER_SUBNET
        for i in 0..MAX_PER_SUBNET as u8 {
            assert!(pm.register_peer(
                &format!("p{i}"),
                ip(10, 0, i, 1),
                8333,
                PeerDirection::Inbound,
                t
            ));
        }
        // Next one from same subnet should be rejected
        assert!(!pm.register_peer("p_extra", ip(10, 0, 99, 1), 8333, PeerDirection::Inbound, t));
        // Different subnet should work
        assert!(pm.register_peer(
            "p_diff",
            ip(192, 168, 1, 1),
            8333,
            PeerDirection::Inbound,
            t
        ));
    }

    #[test]
    fn scoring_and_auto_ban() {
        let mut pm = PeerManager::new();
        let t = now();
        pm.register_peer("p1", ip(10, 0, 0, 1), 8333, PeerDirection::Outbound, t);
        assert_eq!(pm.get_score("p1"), Some(INITIAL_SCORE));

        // Penalize heavily
        pm.penalize("p1", 250);
        assert_eq!(pm.get_score("p1"), Some(INITIAL_SCORE - 250));
        assert!(pm.get_score("p1").unwrap() <= BAN_THRESHOLD);

        let actions = pm.heartbeat(t);
        let bans: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, PeerAction::Ban { .. }))
            .collect();
        assert_eq!(bans.len(), 1);
    }

    #[test]
    fn reward_increases_score() {
        let mut pm = PeerManager::new();
        let t = now();
        pm.register_peer("p1", ip(10, 0, 0, 1), 8333, PeerDirection::Outbound, t);
        pm.reward("p1", REWARD_VALID_BLOCK);
        assert_eq!(pm.get_score("p1"), Some(INITIAL_SCORE + REWARD_VALID_BLOCK));
    }

    #[test]
    fn score_capped_at_500() {
        let mut pm = PeerManager::new();
        let t = now();
        pm.register_peer("p1", ip(10, 0, 0, 1), 8333, PeerDirection::Outbound, t);
        pm.reward("p1", 1000);
        assert_eq!(pm.get_score("p1"), Some(500));
    }

    #[test]
    fn idle_timeout_disconnects() {
        let mut pm = PeerManager::new();
        let t = now();
        pm.register_peer("p1", ip(10, 0, 0, 1), 8333, PeerDirection::Outbound, t);

        let later = t + PEER_IDLE_TIMEOUT + Duration::from_secs(1);
        let actions = pm.heartbeat(later);
        let disconnects: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, PeerAction::Disconnect { .. }))
            .collect();
        assert_eq!(disconnects.len(), 1);
    }

    #[test]
    fn heartbeat_suggests_outbound_connections() {
        let mut pm = PeerManager::new();
        pm.add_seeds(&[(ip(8, 8, 8, 8), 8333), (ip(1, 1, 1, 1), 8333)]);
        let t = now();

        let actions = pm.heartbeat(t);
        let connects: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, PeerAction::ConnectOutbound { .. }))
            .collect();
        assert!(!connects.is_empty());
    }

    #[test]
    fn best_peer_height() {
        let mut pm = PeerManager::new();
        let t = now();
        pm.register_peer("p1", ip(10, 0, 0, 1), 8333, PeerDirection::Outbound, t);
        pm.register_peer("p2", ip(10, 1, 0, 1), 8333, PeerDirection::Outbound, t);
        pm.update_height("p1", 100);
        pm.update_height("p2", 200);
        assert_eq!(pm.best_peer_height(), 200);
    }

    #[test]
    fn peers_sorted_by_score() {
        let mut pm = PeerManager::new();
        let t = now();
        pm.register_peer("p1", ip(10, 0, 0, 1), 8333, PeerDirection::Outbound, t);
        pm.register_peer("p2", ip(10, 1, 0, 1), 8333, PeerDirection::Outbound, t);
        pm.reward("p2", 50);

        let sorted = pm.peers_by_score();
        assert_eq!(sorted[0].peer_id, "p2");
        assert_eq!(sorted[1].peer_id, "p1");
    }

    #[test]
    fn unregister_peer() {
        let mut pm = PeerManager::new();
        let t = now();
        pm.register_peer("p1", ip(10, 0, 0, 1), 8333, PeerDirection::Outbound, t);
        assert_eq!(pm.peer_count(), 1);
        pm.unregister_peer("p1");
        assert_eq!(pm.peer_count(), 0);
    }

    #[test]
    fn record_message_updates_last_seen() {
        let mut pm = PeerManager::new();
        let t = now();
        pm.register_peer("p1", ip(10, 0, 0, 1), 8333, PeerDirection::Outbound, t);

        let later = t + Duration::from_secs(30);
        pm.record_message("p1", 100, later);

        // Peer should not be idle-disconnected since we updated last_seen
        let check = later + Duration::from_secs(60);
        let actions = pm.heartbeat(check);
        assert!(actions
            .iter()
            .all(|a| !matches!(a, PeerAction::Disconnect { .. })));
    }

    #[test]
    fn latency_reward() {
        let mut pm = PeerManager::new();
        let t = now();
        pm.register_peer("p1", ip(10, 0, 0, 1), 8333, PeerDirection::Outbound, t);
        pm.record_latency("p1", 50);
        assert!(pm.get_score("p1").unwrap() > INITIAL_SCORE);
    }
}
