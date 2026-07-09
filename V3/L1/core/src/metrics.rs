// Phase 7e — Metrics and Prometheus-compatible export
//
// Audit reference: #27
//
// Atomic counters and gauges for runtime observability.
// Export in Prometheus text exposition format — no external crate needed.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

// ── Metric definitions ─────────────────────────────────────────────────

/// Runtime metrics for the ZION node.
pub struct NodeMetrics {
    // Counters (monotonically increasing)
    pub blocks_accepted: AtomicU64,
    pub blocks_rejected: AtomicU64,
    pub txs_processed: AtomicU64,
    pub txs_rejected: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    pub reorgs_executed: AtomicU64,

    // Gauges (can go up or down)
    pub chain_height: AtomicU64,
    pub mempool_size: AtomicI64,
    pub mempool_bytes: AtomicI64,
    pub peer_count: AtomicI64,
    pub inbound_peers: AtomicI64,
    pub outbound_peers: AtomicI64,
    pub difficulty: AtomicU64,
    pub last_block_time_secs: AtomicU64,
    pub ibd_progress_pct: AtomicU64, // 0–10000 (two decimal places × 100)

    // Phase 9c — enhanced diagnostics (ported from TREE_NODES health/monitoring)
    pub checkpoint_height: AtomicU64,
    pub discovered_peers: AtomicU64,
    pub udp_announcements: AtomicU64,
    pub uptime_secs: AtomicU64,
    pub network_hashrate: AtomicU64,
    pub best_peer_height: AtomicU64,
}

impl NodeMetrics {
    pub fn new() -> Self {
        Self {
            blocks_accepted: AtomicU64::new(0),
            blocks_rejected: AtomicU64::new(0),
            txs_processed: AtomicU64::new(0),
            txs_rejected: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            reorgs_executed: AtomicU64::new(0),
            chain_height: AtomicU64::new(0),
            mempool_size: AtomicI64::new(0),
            mempool_bytes: AtomicI64::new(0),
            peer_count: AtomicI64::new(0),
            inbound_peers: AtomicI64::new(0),
            outbound_peers: AtomicI64::new(0),
            difficulty: AtomicU64::new(0),
            last_block_time_secs: AtomicU64::new(0),
            ibd_progress_pct: AtomicU64::new(0),
            checkpoint_height: AtomicU64::new(0),
            discovered_peers: AtomicU64::new(0),
            udp_announcements: AtomicU64::new(0),
            uptime_secs: AtomicU64::new(0),
            network_hashrate: AtomicU64::new(0),
            best_peer_height: AtomicU64::new(0),
        }
    }

    // ── Counter increments ─────────────────────────────────────────────

    pub fn inc_blocks_accepted(&self) {
        self.blocks_accepted.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_blocks_rejected(&self) {
        self.blocks_rejected.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_txs_processed(&self, n: u64) {
        self.txs_processed.fetch_add(n, Ordering::Relaxed);
    }

    pub fn inc_txs_rejected(&self) {
        self.txs_rejected.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_bytes_sent(&self, n: u64) {
        self.bytes_sent.fetch_add(n, Ordering::Relaxed);
    }

    pub fn add_bytes_received(&self, n: u64) {
        self.bytes_received.fetch_add(n, Ordering::Relaxed);
    }

    pub fn inc_reorgs(&self) {
        self.reorgs_executed.fetch_add(1, Ordering::Relaxed);
    }

    // ── Gauge setters ──────────────────────────────────────────────────

    pub fn set_chain_height(&self, h: u64) {
        self.chain_height.store(h, Ordering::Relaxed);
    }

    pub fn set_mempool_size(&self, n: i64) {
        self.mempool_size.store(n, Ordering::Relaxed);
    }

    pub fn set_mempool_bytes(&self, n: i64) {
        self.mempool_bytes.store(n, Ordering::Relaxed);
    }

    pub fn set_peer_count(&self, total: i64, inbound: i64, outbound: i64) {
        self.peer_count.store(total, Ordering::Relaxed);
        self.inbound_peers.store(inbound, Ordering::Relaxed);
        self.outbound_peers.store(outbound, Ordering::Relaxed);
    }

    pub fn set_difficulty(&self, d: u64) {
        self.difficulty.store(d, Ordering::Relaxed);
    }

    pub fn set_last_block_time(&self, t: u64) {
        self.last_block_time_secs.store(t, Ordering::Relaxed);
    }

    pub fn set_ibd_progress(&self, pct_x100: u64) {
        self.ibd_progress_pct.store(pct_x100, Ordering::Relaxed);
    }

    pub fn set_checkpoint_height(&self, h: u64) {
        self.checkpoint_height.store(h, Ordering::Relaxed);
    }

    pub fn set_discovered_peers(&self, n: u64) {
        self.discovered_peers.store(n, Ordering::Relaxed);
    }

    pub fn inc_udp_announcements(&self) {
        self.udp_announcements.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_uptime(&self, secs: u64) {
        self.uptime_secs.store(secs, Ordering::Relaxed);
    }

    pub fn set_network_hashrate(&self, h: u64) {
        self.network_hashrate.store(h, Ordering::Relaxed);
    }

    pub fn set_best_peer_height(&self, h: u64) {
        self.best_peer_height.store(h, Ordering::Relaxed);
    }

    // ── Prometheus text exposition ─────────────────────────────────────

    /// Render all metrics in Prometheus text exposition format.
    pub fn render_prometheus(&self) -> String {
        let mut out = String::with_capacity(2048);

        // Helper macro
        macro_rules! counter {
            ($name:expr, $help:expr, $field:expr) => {
                out.push_str(&format!(
                    "# HELP {} {}\n# TYPE {} counter\n{} {}\n",
                    $name,
                    $help,
                    $name,
                    $name,
                    $field.load(Ordering::Relaxed)
                ));
            };
        }
        macro_rules! gauge {
            ($name:expr, $help:expr, $field:expr, $ty:ty) => {
                out.push_str(&format!(
                    "# HELP {} {}\n# TYPE {} gauge\n{} {}\n",
                    $name,
                    $help,
                    $name,
                    $name,
                    $field.load(Ordering::Relaxed) as $ty
                ));
            };
        }

        counter!(
            "zion_blocks_accepted_total",
            "Total blocks accepted",
            self.blocks_accepted
        );
        counter!(
            "zion_blocks_rejected_total",
            "Total blocks rejected",
            self.blocks_rejected
        );
        counter!(
            "zion_txs_processed_total",
            "Total transactions processed",
            self.txs_processed
        );
        counter!(
            "zion_txs_rejected_total",
            "Total transactions rejected",
            self.txs_rejected
        );
        counter!("zion_bytes_sent_total", "Total bytes sent", self.bytes_sent);
        counter!(
            "zion_bytes_received_total",
            "Total bytes received",
            self.bytes_received
        );
        counter!(
            "zion_reorgs_total",
            "Total chain reorganizations",
            self.reorgs_executed
        );

        gauge!(
            "zion_chain_height",
            "Current chain tip height",
            self.chain_height,
            u64
        );
        gauge!(
            "zion_mempool_size",
            "Number of transactions in mempool",
            self.mempool_size,
            i64
        );
        gauge!(
            "zion_mempool_bytes",
            "Total bytes of transactions in mempool",
            self.mempool_bytes,
            i64
        );
        gauge!(
            "zion_peer_count",
            "Total connected peers",
            self.peer_count,
            i64
        );
        gauge!(
            "zion_inbound_peers",
            "Inbound peer connections",
            self.inbound_peers,
            i64
        );
        gauge!(
            "zion_outbound_peers",
            "Outbound peer connections",
            self.outbound_peers,
            i64
        );
        gauge!(
            "zion_difficulty",
            "Current mining difficulty",
            self.difficulty,
            u64
        );
        gauge!(
            "zion_last_block_time",
            "Timestamp of the last accepted block",
            self.last_block_time_secs,
            u64
        );
        gauge!(
            "zion_ibd_progress_pct",
            "IBD progress in percent x100 (0-10000)",
            self.ibd_progress_pct,
            u64
        );

        gauge!(
            "zion_checkpoint_height",
            "Highest verified checkpoint height",
            self.checkpoint_height,
            u64
        );
        gauge!(
            "zion_discovered_peers",
            "Number of peers in discovery pool",
            self.discovered_peers,
            u64
        );
        counter!(
            "zion_udp_announcements_total",
            "Total UDP announcements processed",
            self.udp_announcements
        );
        gauge!(
            "zion_uptime_seconds",
            "Node uptime in seconds",
            self.uptime_secs,
            u64
        );
        gauge!(
            "zion_network_hashrate",
            "Estimated network hash rate",
            self.network_hashrate,
            u64
        );
        gauge!(
            "zion_best_peer_height",
            "Best known peer chain height",
            self.best_peer_height,
            u64
        );

        out
    }

    /// Render a comprehensive health check JSON.
    pub fn health_check(&self) -> String {
        let height = self.chain_height.load(Ordering::Relaxed);
        let peers = self.peer_count.load(Ordering::Relaxed);
        let mempool = self.mempool_size.load(Ordering::Relaxed);
        let difficulty = self.difficulty.load(Ordering::Relaxed);
        let uptime = self.uptime_secs.load(Ordering::Relaxed);
        let best_peer = self.best_peer_height.load(Ordering::Relaxed);
        let checkpoint = self.checkpoint_height.load(Ordering::Relaxed);
        let discovered = self.discovered_peers.load(Ordering::Relaxed);
        let hashrate = self.network_hashrate.load(Ordering::Relaxed);
        let ibd_pct = self.ibd_progress_pct.load(Ordering::Relaxed);
        let last_block = self.last_block_time_secs.load(Ordering::Relaxed);

        let sync_lag = best_peer.saturating_sub(height);
        let sync_status = if ibd_pct < 10_000 && sync_lag > 50 {
            "ibd"
        } else if sync_lag > 0 {
            "syncing"
        } else {
            "synced"
        };

        format!(
            concat!(
                r#"{{"status":"ok","chain_height":{},"peer_count":{},"mempool_size":{},"#,
                r#""difficulty":{},"uptime_secs":{},"best_peer_height":{},"sync_lag":{},"#,
                r#""sync_status":"{}","checkpoint_height":{},"discovered_peers":{},"#,
                r#""network_hashrate":{},"last_block_time":{}}}"#,
            ),
            height,
            peers,
            mempool,
            difficulty,
            uptime,
            best_peer,
            sync_lag,
            sync_status,
            checkpoint,
            discovered,
            hashrate,
            last_block
        )
    }
}

impl Default for NodeMetrics {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_values_are_zero() {
        let m = NodeMetrics::new();
        assert_eq!(m.blocks_accepted.load(Ordering::Relaxed), 0);
        assert_eq!(m.chain_height.load(Ordering::Relaxed), 0);
        assert_eq!(m.peer_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn counter_increments() {
        let m = NodeMetrics::new();
        m.inc_blocks_accepted();
        m.inc_blocks_accepted();
        m.inc_blocks_rejected();
        m.inc_txs_processed(5);
        assert_eq!(m.blocks_accepted.load(Ordering::Relaxed), 2);
        assert_eq!(m.blocks_rejected.load(Ordering::Relaxed), 1);
        assert_eq!(m.txs_processed.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn gauge_updates() {
        let m = NodeMetrics::new();
        m.set_chain_height(1000);
        m.set_mempool_size(42);
        m.set_peer_count(10, 6, 4);
        m.set_difficulty(5000);

        assert_eq!(m.chain_height.load(Ordering::Relaxed), 1000);
        assert_eq!(m.mempool_size.load(Ordering::Relaxed), 42);
        assert_eq!(m.peer_count.load(Ordering::Relaxed), 10);
        assert_eq!(m.inbound_peers.load(Ordering::Relaxed), 6);
        assert_eq!(m.outbound_peers.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn prometheus_output_format() {
        let m = NodeMetrics::new();
        m.inc_blocks_accepted();
        m.set_chain_height(500);

        let output = m.render_prometheus();
        assert!(output.contains("# TYPE zion_blocks_accepted_total counter"));
        assert!(output.contains("zion_blocks_accepted_total 1"));
        assert!(output.contains("# TYPE zion_chain_height gauge"));
        assert!(output.contains("zion_chain_height 500"));
    }

    #[test]
    fn prometheus_has_all_metrics() {
        let m = NodeMetrics::new();
        let output = m.render_prometheus();

        let expected = [
            "zion_blocks_accepted_total",
            "zion_blocks_rejected_total",
            "zion_txs_processed_total",
            "zion_txs_rejected_total",
            "zion_bytes_sent_total",
            "zion_bytes_received_total",
            "zion_reorgs_total",
            "zion_chain_height",
            "zion_mempool_size",
            "zion_mempool_bytes",
            "zion_peer_count",
            "zion_inbound_peers",
            "zion_outbound_peers",
            "zion_difficulty",
            "zion_last_block_time",
            "zion_ibd_progress_pct",
            "zion_checkpoint_height",
            "zion_discovered_peers",
            "zion_udp_announcements_total",
            "zion_uptime_seconds",
            "zion_network_hashrate",
            "zion_best_peer_height",
        ];

        for name in expected {
            assert!(output.contains(name), "missing metric: {name}");
        }
    }

    #[test]
    fn health_check_json() {
        let m = NodeMetrics::new();
        m.set_chain_height(100);
        m.set_peer_count(5, 3, 2);
        m.set_mempool_size(10);
        m.set_difficulty(5000);
        m.set_uptime(3600);
        m.set_best_peer_height(100);

        let json = m.health_check();
        assert!(json.contains(r#""status":"ok""#));
        assert!(json.contains(r#""chain_height":100"#));
        assert!(json.contains(r#""peer_count":5"#));
        assert!(json.contains(r#""mempool_size":10"#));
        assert!(json.contains(r#""difficulty":5000"#));
        assert!(json.contains(r#""uptime_secs":3600"#));
        assert!(json.contains(r#""sync_status":"synced""#));
    }

    #[test]
    fn bytes_tracking() {
        let m = NodeMetrics::new();
        m.add_bytes_sent(100);
        m.add_bytes_sent(200);
        m.add_bytes_received(500);

        assert_eq!(m.bytes_sent.load(Ordering::Relaxed), 300);
        assert_eq!(m.bytes_received.load(Ordering::Relaxed), 500);
    }

    #[test]
    fn reorg_counter() {
        let m = NodeMetrics::new();
        m.inc_reorgs();
        m.inc_reorgs();
        assert_eq!(m.reorgs_executed.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn ibd_progress() {
        let m = NodeMetrics::new();
        m.set_ibd_progress(5050); // 50.50%
        assert_eq!(m.ibd_progress_pct.load(Ordering::Relaxed), 5050);
    }

    #[test]
    fn concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let m = Arc::new(NodeMetrics::new());
        let mut handles = vec![];

        for _ in 0..4 {
            let m = Arc::clone(&m);
            handles.push(thread::spawn(move || {
                for _ in 0..1000 {
                    m.inc_blocks_accepted();
                    m.inc_txs_processed(1);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(m.blocks_accepted.load(Ordering::Relaxed), 4000);
        assert_eq!(m.txs_processed.load(Ordering::Relaxed), 4000);
    }
}
