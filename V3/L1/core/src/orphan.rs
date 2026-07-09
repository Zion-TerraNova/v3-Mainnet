// Phase 6e — Orphan block buffer and chain-ID enforcement
//
// Orphan blocks: blocks whose parent is unknown. They are buffered and
// reconsidered when the missing parent arrives.
//
// Chain-ID: `zion-mainnet-1`. Peers that announce a different chain ID
// in their Hello message should be disconnected.

use std::collections::{HashMap, VecDeque};

/// The canonical mainnet chain identifier.
pub const CHAIN_ID: &str = "zion-mainnet-1";

/// Maximum number of orphan blocks kept in the buffer.
pub const MAX_ORPHAN_BLOCKS: usize = 200;

/// Maximum time (seconds) an orphan block is kept before expiry.
pub const ORPHAN_EXPIRY_SECS: u64 = 600;

// ---------------------------------------------------------------------------
// Orphan block buffer
// ---------------------------------------------------------------------------

/// A block waiting for its parent to be accepted.
#[derive(Debug, Clone)]
pub struct OrphanBlock {
    pub hash: [u8; 32],
    pub prev_hash: [u8; 32],
    pub height: u64,
    pub received_at: u64,
    /// Opaque serialized block data kept for later validation.
    pub data: Vec<u8>,
}

/// Buffer for orphan blocks, indexed by the missing parent hash.
#[derive(Debug)]
pub struct OrphanPool {
    /// parent_hash → list of orphans waiting for that parent.
    by_parent: HashMap<[u8; 32], Vec<OrphanBlock>>,
    /// Insertion order for FIFO eviction.
    order: VecDeque<[u8; 32]>,
    /// Total count.
    count: usize,
}

impl OrphanPool {
    pub fn new() -> Self {
        Self {
            by_parent: HashMap::new(),
            order: VecDeque::new(),
            count: 0,
        }
    }

    /// Number of orphan blocks in the buffer.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Insert an orphan block whose parent `prev_hash` is unknown.
    /// If the buffer is full, the oldest orphan is evicted.
    pub fn insert(&mut self, orphan: OrphanBlock) {
        // Evict if at capacity.
        while self.count >= MAX_ORPHAN_BLOCKS {
            self.evict_oldest();
        }
        let parent = orphan.prev_hash;
        self.order.push_back(orphan.hash);
        self.by_parent.entry(parent).or_default().push(orphan);
        self.count += 1;
    }

    /// Take all orphans that were waiting for `parent_hash`.
    /// Call this when a new block with that hash is accepted.
    pub fn take_children(&mut self, parent_hash: &[u8; 32]) -> Vec<OrphanBlock> {
        if let Some(orphans) = self.by_parent.remove(parent_hash) {
            self.count = self.count.saturating_sub(orphans.len());
            // Clean up order queue lazily — hashes of removed orphans will be
            // skipped in future evictions.
            orphans
        } else {
            Vec::new()
        }
    }

    /// Check whether there are orphans waiting for a given parent.
    pub fn has_children(&self, parent_hash: &[u8; 32]) -> bool {
        self.by_parent
            .get(parent_hash)
            .is_some_and(|v| !v.is_empty())
    }

    /// Remove expired orphans. `now` is epoch seconds.
    pub fn expire(&mut self, now: u64) {
        let mut expired_parents = Vec::new();
        for (parent, orphans) in &mut self.by_parent {
            let before = orphans.len();
            orphans.retain(|o| now.saturating_sub(o.received_at) < ORPHAN_EXPIRY_SECS);
            self.count = self.count.saturating_sub(before - orphans.len());
            if orphans.is_empty() {
                expired_parents.push(*parent);
            }
        }
        for p in expired_parents {
            self.by_parent.remove(&p);
        }
    }

    /// Clear the entire buffer.
    pub fn clear(&mut self) {
        self.by_parent.clear();
        self.order.clear();
        self.count = 0;
    }

    // -- internal --

    fn evict_oldest(&mut self) {
        while let Some(hash) = self.order.pop_front() {
            // Find and remove this orphan from its parent bucket.
            let mut found = false;
            for orphans in self.by_parent.values_mut() {
                if let Some(pos) = orphans.iter().position(|o| o.hash == hash) {
                    orphans.remove(pos);
                    self.count = self.count.saturating_sub(1);
                    found = true;
                    break;
                }
            }
            if found {
                // Clean up empty parent buckets.
                self.by_parent.retain(|_, v| !v.is_empty());
                break;
            }
            // Hash was already removed (e.g. via take_children); continue to next.
        }
    }
}

impl Default for OrphanPool {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Chain-ID enforcement
// ---------------------------------------------------------------------------

/// Validate that a peer's announced chain ID matches ours.
pub fn validate_chain_id(peer_chain_id: &str) -> bool {
    peer_chain_id == CHAIN_ID
}

/// Validate that a peer's announced network matches the expected one.
/// This is distinct from CHAIN_ID — it validates the `NetworkId` enum
/// value in the Hello message.
pub fn validate_network(expected: &str, got: &str) -> bool {
    expected == got
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(v: u8) -> [u8; 32] {
        let mut h = [0u8; 32];
        h[0] = v;
        h
    }

    fn orphan(h: u8, parent: u8, time: u64) -> OrphanBlock {
        OrphanBlock {
            hash: hash(h),
            prev_hash: hash(parent),
            height: h as u64,
            received_at: time,
            data: vec![h],
        }
    }

    #[test]
    fn test_insert_and_take_children() {
        let mut pool = OrphanPool::new();
        pool.insert(orphan(10, 5, 1000));
        pool.insert(orphan(11, 5, 1001));
        assert_eq!(pool.len(), 2);
        assert!(pool.has_children(&hash(5)));
        let children = pool.take_children(&hash(5));
        assert_eq!(children.len(), 2);
        assert_eq!(pool.len(), 0);
        assert!(!pool.has_children(&hash(5)));
    }

    #[test]
    fn test_no_children() {
        let pool = OrphanPool::new();
        assert!(!pool.has_children(&hash(99)));
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn test_eviction_at_capacity() {
        let mut pool = OrphanPool::new();
        for i in 0..MAX_ORPHAN_BLOCKS as u8 {
            pool.insert(orphan(i, 255, 1000));
        }
        assert_eq!(pool.len(), MAX_ORPHAN_BLOCKS);
        // One more triggers eviction.
        pool.insert(orphan(250, 254, 1000));
        assert_eq!(pool.len(), MAX_ORPHAN_BLOCKS);
    }

    #[test]
    fn test_expire() {
        let mut pool = OrphanPool::new();
        pool.insert(orphan(1, 0, 1000));
        pool.insert(orphan(2, 0, 2000));
        assert_eq!(pool.len(), 2);
        // Expire at 1000 + ORPHAN_EXPIRY_SECS: orphan 1 expires, orphan 2 survives.
        pool.expire(1000 + ORPHAN_EXPIRY_SECS);
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn test_clear() {
        let mut pool = OrphanPool::new();
        pool.insert(orphan(1, 0, 1000));
        pool.insert(orphan(2, 0, 1000));
        pool.clear();
        assert_eq!(pool.len(), 0);
        assert!(pool.is_empty());
    }

    #[test]
    fn test_chain_id_valid() {
        assert!(validate_chain_id("zion-mainnet-1"));
    }

    #[test]
    fn test_chain_id_invalid() {
        assert!(!validate_chain_id("zion-testnet-1"));
        assert!(!validate_chain_id("bitcoin-mainnet"));
        assert!(!validate_chain_id(""));
    }

    #[test]
    fn test_network_validation() {
        assert!(validate_network("Mainnet", "Mainnet"));
        assert!(!validate_network("Mainnet", "Testnet"));
    }

    #[test]
    fn test_constants() {
        assert_eq!(CHAIN_ID, "zion-mainnet-1");
        assert_eq!(MAX_ORPHAN_BLOCKS, 200);
        assert_eq!(ORPHAN_EXPIRY_SECS, 600);
    }

    #[test]
    fn test_multiple_parents() {
        let mut pool = OrphanPool::new();
        pool.insert(orphan(10, 5, 1000)); // waiting for parent 5
        pool.insert(orphan(20, 8, 1000)); // waiting for parent 8
        assert_eq!(pool.len(), 2);
        let c5 = pool.take_children(&hash(5));
        assert_eq!(c5.len(), 1);
        assert_eq!(pool.len(), 1);
        let c8 = pool.take_children(&hash(8));
        assert_eq!(c8.len(), 1);
        assert_eq!(pool.len(), 0);
    }
}
