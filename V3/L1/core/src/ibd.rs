// Phase 7b — Initial Block Download (IBD) state machine
//
// Audit reference: #25
//
// Tracks sync state: how far behind the network we are, whether we're in
// IBD mode, stall detection, and batch download orchestration.
//
// This is a pure state machine — it does not perform I/O itself but
// produces commands that the node runtime executes.

use std::collections::HashMap;
use std::time::{Duration, Instant};

// ── Constants ──────────────────────────────────────────────────────────

/// If our tip is more than this many blocks behind the best known peer
/// height, we enter IBD mode.
pub const IBD_THRESHOLD: u64 = 50;

/// Number of blocks to request per batch during IBD.
pub const IBD_BATCH_SIZE: u64 = 500;

/// How long to wait for a batch response before considering the peer stalled.
pub const IBD_STALL_TIMEOUT: Duration = Duration::from_secs(120);

/// Maximum retries before demoting a stalled peer.
pub const IBD_MAX_RETRIES: u32 = 3;

/// Maximum concurrent pending batches.
pub const IBD_MAX_INFLIGHT: usize = 4;

// ── Types ──────────────────────────────────────────────────────────────

/// Overall sync status reported to the rest of the node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    /// Initial block download — far behind the network.
    Ibd,
    /// Catching up — less than IBD_THRESHOLD behind but not fully synced.
    Syncing,
    /// Fully synced — tip matches or exceeds best known peer height.
    Synced,
}

/// A batch request issued to a peer.
#[derive(Debug, Clone)]
pub struct BatchRequest {
    /// Peer that should serve this batch.
    pub peer_id: String,
    /// Starting height (inclusive).
    pub start_height: u64,
    /// Number of blocks requested.
    pub count: u64,
    /// When this request was issued.
    pub issued_at: Instant,
    /// How many times we've retried this range.
    pub retries: u32,
}

/// Commands the IBD engine produces for the node runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IbdCommand {
    /// Request a batch of blocks from a specific peer.
    RequestBatch {
        peer_id: String,
        start_height: u64,
        count: u64,
    },
    /// Demote or disconnect a stalled/misbehaving peer.
    DemotePeer { peer_id: String, reason: String },
    /// IBD is complete — transition to normal sync.
    IbdComplete,
}

/// Peer info as seen by the IBD engine.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub peer_id: String,
    pub best_height: u64,
    pub available: bool,
}

// ── IBD State Machine ──────────────────────────────────────────────────

/// The IBD state machine. Deterministic, clock-injected for testability.
pub struct IbdEngine {
    /// Our local tip height.
    local_height: u64,
    /// Best known height across all peers.
    best_peer_height: u64,
    /// Current sync status.
    status: SyncStatus,
    /// Inflight batch requests keyed by start_height.
    inflight: HashMap<u64, BatchRequest>,
    /// Next height to request.
    next_request_height: u64,
    /// Available peers for batch requests.
    peers: Vec<PeerInfo>,
    /// Round-robin index into peers.
    peer_index: usize,
    /// Set when transitioning to Synced; cleared after emitting IbdComplete.
    ibd_complete_pending: bool,
}

impl IbdEngine {
    /// Create a new IBD engine starting from the given local tip height.
    pub fn new(local_height: u64) -> Self {
        Self {
            local_height,
            best_peer_height: local_height,
            status: SyncStatus::Synced,
            inflight: HashMap::new(),
            next_request_height: local_height + 1,
            peers: Vec::new(),
            peer_index: 0,
            ibd_complete_pending: false,
        }
    }

    /// Current sync status.
    pub fn status(&self) -> SyncStatus {
        self.status
    }

    /// Local tip height.
    pub fn local_height(&self) -> u64 {
        self.local_height
    }

    /// Best known peer height.
    pub fn best_peer_height(&self) -> u64 {
        self.best_peer_height
    }

    /// Number of inflight batch requests.
    pub fn inflight_count(&self) -> usize {
        self.inflight.len()
    }

    /// Update our local tip height (called after blocks are applied).
    pub fn set_local_height(&mut self, height: u64) {
        self.local_height = height;
        self.recalculate_status();
    }

    /// Update a peer's best known height.
    pub fn update_peer(&mut self, peer_id: &str, best_height: u64) {
        if best_height > self.best_peer_height {
            self.best_peer_height = best_height;
        }
        if let Some(p) = self.peers.iter_mut().find(|p| p.peer_id == peer_id) {
            p.best_height = best_height;
            p.available = true;
        } else {
            self.peers.push(PeerInfo {
                peer_id: peer_id.to_string(),
                best_height,
                available: true,
            });
        }
        self.recalculate_status();
    }

    /// Remove a peer entirely.
    pub fn remove_peer(&mut self, peer_id: &str) {
        self.peers.retain(|p| p.peer_id != peer_id);
        // Cancel any inflight requests to this peer
        self.inflight.retain(|_, req| req.peer_id != peer_id);
        // Recalculate best_peer_height from remaining peers
        self.best_peer_height = self
            .peers
            .iter()
            .map(|p| p.best_height)
            .max()
            .unwrap_or(self.local_height);
        self.recalculate_status();
    }

    /// Produce the next set of commands. Called periodically by the node runtime.
    /// `now` is injected for testability.
    pub fn tick(&mut self, now: Instant) -> Vec<IbdCommand> {
        let mut commands = Vec::new();

        // Check for stalled requests
        let stalled: Vec<u64> = self
            .inflight
            .iter()
            .filter(|(_, req)| now.duration_since(req.issued_at) >= IBD_STALL_TIMEOUT)
            .map(|(h, _)| *h)
            .collect();

        for height in stalled {
            if let Some(req) = self.inflight.remove(&height) {
                if req.retries >= IBD_MAX_RETRIES {
                    commands.push(IbdCommand::DemotePeer {
                        peer_id: req.peer_id.clone(),
                        reason: format!(
                            "stalled after {} retries for batch at height {}",
                            IBD_MAX_RETRIES, height
                        ),
                    });
                    // Mark peer unavailable
                    if let Some(p) = self.peers.iter_mut().find(|p| p.peer_id == req.peer_id) {
                        p.available = false;
                    }
                } else {
                    // Re-insert with incremented retry and pick a different peer
                    if let Some(peer_id) = self.pick_peer() {
                        let new_req = BatchRequest {
                            peer_id: peer_id.clone(),
                            start_height: height,
                            count: req.count,
                            issued_at: now,
                            retries: req.retries + 1,
                        };
                        commands.push(IbdCommand::RequestBatch {
                            peer_id,
                            start_height: height,
                            count: req.count,
                        });
                        self.inflight.insert(height, new_req);
                    }
                }
            }
        }

        // Issue new batch requests if we need more blocks and have capacity
        if self.status == SyncStatus::Ibd || self.status == SyncStatus::Syncing {
            while self.inflight.len() < IBD_MAX_INFLIGHT
                && self.next_request_height <= self.best_peer_height
            {
                let count =
                    IBD_BATCH_SIZE.min(self.best_peer_height - self.next_request_height + 1);
                if count == 0 {
                    break;
                }

                if let Some(peer_id) = self.pick_peer() {
                    commands.push(IbdCommand::RequestBatch {
                        peer_id: peer_id.clone(),
                        start_height: self.next_request_height,
                        count,
                    });
                    self.inflight.insert(
                        self.next_request_height,
                        BatchRequest {
                            peer_id,
                            start_height: self.next_request_height,
                            count,
                            issued_at: now,
                            retries: 0,
                        },
                    );
                    self.next_request_height += count;
                } else {
                    break; // No available peers
                }
            }
        }

        // Emit IbdComplete if we transitioned to Synced
        if self.ibd_complete_pending {
            self.ibd_complete_pending = false;
            commands.push(IbdCommand::IbdComplete);
        }

        commands
    }

    /// Acknowledge that a batch starting at `start_height` has been received.
    pub fn batch_received(&mut self, start_height: u64) {
        self.inflight.remove(&start_height);
    }

    /// Acknowledge that blocks have been applied up to `new_height`.
    pub fn blocks_applied(&mut self, new_height: u64) {
        self.set_local_height(new_height);
    }

    fn recalculate_status(&mut self) {
        let old = self.status;
        if self.best_peer_height <= self.local_height {
            if self.inflight.is_empty() {
                self.status = SyncStatus::Synced;
            }
        } else if self.best_peer_height - self.local_height > IBD_THRESHOLD {
            self.status = SyncStatus::Ibd;
        } else {
            self.status = SyncStatus::Syncing;
        }
        if old != SyncStatus::Synced && self.status == SyncStatus::Synced {
            self.ibd_complete_pending = true;
        }
    }

    fn pick_peer(&mut self) -> Option<String> {
        let available: Vec<&PeerInfo> = self
            .peers
            .iter()
            .filter(|p| p.available && p.best_height > self.local_height)
            .collect();
        if available.is_empty() {
            return None;
        }
        self.peer_index %= available.len();
        let peer_id = available[self.peer_index].peer_id.clone();
        self.peer_index = (self.peer_index + 1) % available.len();
        Some(peer_id)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_synced_with_no_peers() {
        let engine = IbdEngine::new(100);
        assert_eq!(engine.status(), SyncStatus::Synced);
        assert_eq!(engine.local_height(), 100);
    }

    #[test]
    fn enters_ibd_when_far_behind() {
        let mut engine = IbdEngine::new(0);
        engine.update_peer("peer1", 1000);
        assert_eq!(engine.status(), SyncStatus::Ibd);
    }

    #[test]
    fn enters_syncing_when_slightly_behind() {
        let mut engine = IbdEngine::new(990);
        engine.update_peer("peer1", 1000);
        assert_eq!(engine.status(), SyncStatus::Syncing);
    }

    #[test]
    fn synced_when_at_peer_height() {
        let mut engine = IbdEngine::new(1000);
        engine.update_peer("peer1", 1000);
        assert_eq!(engine.status(), SyncStatus::Synced);
    }

    #[test]
    fn tick_issues_batch_requests() {
        let mut engine = IbdEngine::new(0);
        engine.update_peer("peer1", 2000);
        let now = Instant::now();

        let cmds = engine.tick(now);
        // Should issue up to IBD_MAX_INFLIGHT batch requests
        let requests: Vec<_> = cmds
            .iter()
            .filter(|c| matches!(c, IbdCommand::RequestBatch { .. }))
            .collect();
        assert_eq!(requests.len(), IBD_MAX_INFLIGHT);
        assert_eq!(engine.inflight_count(), IBD_MAX_INFLIGHT);
    }

    #[test]
    fn batch_received_clears_inflight() {
        let mut engine = IbdEngine::new(0);
        engine.update_peer("peer1", 5000);
        let now = Instant::now();
        engine.tick(now);
        assert_eq!(engine.inflight_count(), IBD_MAX_INFLIGHT);

        // Acknowledge the first batch
        engine.batch_received(1);
        assert_eq!(engine.inflight_count(), IBD_MAX_INFLIGHT - 1);
    }

    #[test]
    fn blocks_applied_updates_height() {
        let mut engine = IbdEngine::new(0);
        engine.update_peer("peer1", 100);
        engine.blocks_applied(100);
        assert_eq!(engine.local_height(), 100);
        assert_eq!(engine.status(), SyncStatus::Synced);
    }

    #[test]
    fn stall_detection_retries() {
        let mut engine = IbdEngine::new(0);
        engine.update_peer("peer1", 1000);
        engine.update_peer("peer2", 1000);
        let now = Instant::now();
        engine.tick(now);

        // Simulate time passing beyond stall timeout
        let later = now + IBD_STALL_TIMEOUT + Duration::from_secs(1);
        let cmds = engine.tick(later);

        // Should have retry requests
        let retries: Vec<_> = cmds
            .iter()
            .filter(|c| matches!(c, IbdCommand::RequestBatch { .. }))
            .collect();
        assert!(!retries.is_empty());
    }

    #[test]
    fn stall_detection_demotes_after_max_retries() {
        let mut engine = IbdEngine::new(0);
        engine.update_peer("peer1", 1000);
        let now = Instant::now();

        // Issue initial requests
        engine.tick(now);

        // Simulate repeated stalls (build up retries to MAX)
        let mut t = now;
        for _ in 0..IBD_MAX_RETRIES {
            t += IBD_STALL_TIMEOUT + Duration::from_secs(1);
            engine.tick(t);
        }

        // After MAX_RETRIES, the peer should be demoted
        let t_final = t + IBD_STALL_TIMEOUT + Duration::from_secs(1);
        let cmds = engine.tick(t_final);
        let demotions: Vec<_> = cmds
            .iter()
            .filter(|c| matches!(c, IbdCommand::DemotePeer { .. }))
            .collect();
        assert!(!demotions.is_empty());
    }

    #[test]
    fn ibd_complete_signal() {
        let mut engine = IbdEngine::new(0);
        engine.update_peer("peer1", 10);
        let now = Instant::now();

        // Request and acknowledge all
        engine.tick(now);
        engine.batch_received(1);
        engine.blocks_applied(10);

        let cmds = engine.tick(now);
        assert!(cmds.contains(&IbdCommand::IbdComplete));
        assert_eq!(engine.status(), SyncStatus::Synced);
    }

    #[test]
    fn remove_peer_cancels_inflight() {
        let mut engine = IbdEngine::new(0);
        engine.update_peer("peer1", 1000);
        let now = Instant::now();
        engine.tick(now);
        let before = engine.inflight_count();
        assert!(before > 0);

        engine.remove_peer("peer1");
        assert_eq!(engine.inflight_count(), 0);
    }

    #[test]
    fn no_requests_when_synced() {
        let mut engine = IbdEngine::new(1000);
        engine.update_peer("peer1", 1000);
        let now = Instant::now();

        let cmds = engine.tick(now);
        assert!(cmds.is_empty());
    }

    #[test]
    fn batch_size_clamped_at_end() {
        let mut engine = IbdEngine::new(0);
        // Only 3 blocks behind — batch should be clamped
        engine.update_peer("peer1", 3);
        let now = Instant::now();
        let cmds = engine.tick(now);

        if let Some(IbdCommand::RequestBatch { count, .. }) = cmds.first() {
            assert_eq!(*count, 3);
        }
    }
}
