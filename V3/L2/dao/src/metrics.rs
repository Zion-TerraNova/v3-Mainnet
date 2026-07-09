//! DAO Prometheus metrics.
//!
//! Exposes DAO governance counters in Prometheus text exposition format
//! via `GET /metrics` on the same HTTP port as the REST API.
//!
//! Counters are updated by API handlers through the shared `Arc<DaoMetrics>`.
//!
//! ## Prometheus endpoint
//!
//! ```text
//! GET http://localhost:8080/metrics
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// DAO governance runtime metrics.
///
/// All fields are atomic — safe to share across Tokio tasks via `Arc<DaoMetrics>`.
#[derive(Debug)]
pub struct DaoMetrics {
    start_time: Instant,

    // Proposal counters
    pub proposals_created: AtomicU64,
    pub proposals_executed: AtomicU64,
    pub proposals_rejected: AtomicU64,
    pub proposals_expired: AtomicU64,

    // Voting
    pub votes_cast: AtomicU64,
    pub votes_yes: AtomicU64,
    pub votes_no: AtomicU64,
    pub votes_abstain: AtomicU64,

    // Treasury
    pub treasury_operations_submitted: AtomicU64,
    pub treasury_operations_executed: AtomicU64,
    pub treasury_total_disbursed_zion: AtomicU64, // suma v celých ZION (bez decimals)

    // Emergency
    pub emergency_actions_executed: AtomicU64,

    // Guardian
    pub guardian_signatures_collected: AtomicU64,

    // L1 scanner
    pub l1_blocks_scanned: AtomicU64,
    pub l1_governance_memos_found: AtomicU64,

    // API
    pub api_requests_total: AtomicU64,
    pub api_errors_total: AtomicU64,
}

impl DaoMetrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            start_time: Instant::now(),
            proposals_created: AtomicU64::new(0),
            proposals_executed: AtomicU64::new(0),
            proposals_rejected: AtomicU64::new(0),
            proposals_expired: AtomicU64::new(0),
            votes_cast: AtomicU64::new(0),
            votes_yes: AtomicU64::new(0),
            votes_no: AtomicU64::new(0),
            votes_abstain: AtomicU64::new(0),
            treasury_operations_submitted: AtomicU64::new(0),
            treasury_operations_executed: AtomicU64::new(0),
            treasury_total_disbursed_zion: AtomicU64::new(0),
            emergency_actions_executed: AtomicU64::new(0),
            guardian_signatures_collected: AtomicU64::new(0),
            l1_blocks_scanned: AtomicU64::new(0),
            l1_governance_memos_found: AtomicU64::new(0),
            api_requests_total: AtomicU64::new(0),
            api_errors_total: AtomicU64::new(0),
        })
    }

    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Increment a u64 counter by 1.
    #[inline]
    pub fn inc(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Render all metrics in Prometheus text exposition format (v0.0.4).
    pub fn render_prometheus(&self) -> String {
        let mut out = String::with_capacity(2048);

        // Helper macro to emit a single metric line
        macro_rules! emit {
            ($kind:expr, $name:expr, $help:expr, $val:expr) => {
                out.push_str(&format!("# HELP {} {}\n", $name, $help));
                out.push_str(&format!("# TYPE {} {}\n", $name, $kind));
                out.push_str(&format!("{} {}\n", $name, $val));
            };
        }

        emit!(
            "gauge",
            "zion_dao_uptime_seconds",
            "DAO daemon uptime in seconds",
            self.uptime_secs()
        );

        // Proposals
        emit!(
            "counter",
            "zion_dao_proposals_created_total",
            "Total governance proposals created",
            self.proposals_created.load(Ordering::Relaxed)
        );
        emit!(
            "counter",
            "zion_dao_proposals_executed_total",
            "Total proposals successfully executed",
            self.proposals_executed.load(Ordering::Relaxed)
        );
        emit!(
            "counter",
            "zion_dao_proposals_rejected_total",
            "Total proposals rejected (quorum not met or voted down)",
            self.proposals_rejected.load(Ordering::Relaxed)
        );
        emit!(
            "counter",
            "zion_dao_proposals_expired_total",
            "Total proposals that expired without reaching quorum",
            self.proposals_expired.load(Ordering::Relaxed)
        );

        // Voting
        emit!(
            "counter",
            "zion_dao_votes_cast_total",
            "Total votes cast across all proposals",
            self.votes_cast.load(Ordering::Relaxed)
        );
        emit!(
            "counter",
            "zion_dao_votes_yes_total",
            "Total YES votes cast",
            self.votes_yes.load(Ordering::Relaxed)
        );
        emit!(
            "counter",
            "zion_dao_votes_no_total",
            "Total NO votes cast",
            self.votes_no.load(Ordering::Relaxed)
        );
        emit!(
            "counter",
            "zion_dao_votes_abstain_total",
            "Total ABSTAIN votes cast",
            self.votes_abstain.load(Ordering::Relaxed)
        );

        // Treasury
        emit!(
            "counter",
            "zion_dao_treasury_ops_submitted_total",
            "Total treasury operations submitted for multisig",
            self.treasury_operations_submitted.load(Ordering::Relaxed)
        );
        emit!(
            "counter",
            "zion_dao_treasury_ops_executed_total",
            "Total treasury operations executed",
            self.treasury_operations_executed.load(Ordering::Relaxed)
        );
        emit!(
            "counter",
            "zion_dao_treasury_disbursed_zion_total",
            "Total ZION disbursed from treasury (whole coins, no decimals)",
            self.treasury_total_disbursed_zion.load(Ordering::Relaxed)
        );

        // Emergency
        emit!(
            "counter",
            "zion_dao_emergency_actions_total",
            "Total emergency actions executed",
            self.emergency_actions_executed.load(Ordering::Relaxed)
        );

        // Guardian
        emit!(
            "counter",
            "zion_dao_guardian_signatures_total",
            "Total guardian signatures collected for multisig operations",
            self.guardian_signatures_collected.load(Ordering::Relaxed)
        );

        // L1 scanner
        emit!(
            "counter",
            "zion_dao_l1_blocks_scanned_total",
            "Total L1 blocks scanned for governance memos",
            self.l1_blocks_scanned.load(Ordering::Relaxed)
        );
        emit!(
            "counter",
            "zion_dao_l1_governance_memos_total",
            "Total L1 governance memo transactions found",
            self.l1_governance_memos_found.load(Ordering::Relaxed)
        );

        // API
        emit!(
            "counter",
            "zion_dao_api_requests_total",
            "Total HTTP API requests processed",
            self.api_requests_total.load(Ordering::Relaxed)
        );
        emit!(
            "counter",
            "zion_dao_api_errors_total",
            "Total HTTP API requests that returned an error",
            self.api_errors_total.load(Ordering::Relaxed)
        );

        out
    }
}

impl Default for DaoMetrics {
    fn default() -> Self {
        Self {
            start_time: Instant::now(),
            proposals_created: AtomicU64::new(0),
            proposals_executed: AtomicU64::new(0),
            proposals_rejected: AtomicU64::new(0),
            proposals_expired: AtomicU64::new(0),
            votes_cast: AtomicU64::new(0),
            votes_yes: AtomicU64::new(0),
            votes_no: AtomicU64::new(0),
            votes_abstain: AtomicU64::new(0),
            treasury_operations_submitted: AtomicU64::new(0),
            treasury_operations_executed: AtomicU64::new(0),
            treasury_total_disbursed_zion: AtomicU64::new(0),
            emergency_actions_executed: AtomicU64::new(0),
            guardian_signatures_collected: AtomicU64::new(0),
            l1_blocks_scanned: AtomicU64::new(0),
            l1_governance_memos_found: AtomicU64::new(0),
            api_requests_total: AtomicU64::new(0),
            api_errors_total: AtomicU64::new(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_new() {
        let m = DaoMetrics::new();
        assert_eq!(m.proposals_created.load(Ordering::Relaxed), 0);
        assert_eq!(m.votes_cast.load(Ordering::Relaxed), 0);
        assert_eq!(m.api_requests_total.load(Ordering::Relaxed), 0);
        assert!(m.uptime_secs() < 5);
    }

    #[test]
    fn test_metrics_inc() {
        let m = DaoMetrics::new();
        DaoMetrics::inc(&m.proposals_created);
        DaoMetrics::inc(&m.proposals_created);
        DaoMetrics::inc(&m.votes_cast);
        assert_eq!(m.proposals_created.load(Ordering::Relaxed), 2);
        assert_eq!(m.votes_cast.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_render_prometheus() {
        let m = DaoMetrics::new();
        DaoMetrics::inc(&m.proposals_created);
        DaoMetrics::inc(&m.votes_yes);
        DaoMetrics::inc(&m.votes_yes);
        let txt = m.render_prometheus();

        assert!(txt.contains("# HELP zion_dao_proposals_created_total"));
        assert!(txt.contains("# TYPE zion_dao_proposals_created_total counter"));
        assert!(txt.contains("zion_dao_proposals_created_total 1"));
        assert!(txt.contains("zion_dao_votes_yes_total 2"));
        assert!(txt.contains("zion_dao_uptime_seconds"));
    }

    #[test]
    fn test_render_prometheus_format() {
        let m = DaoMetrics::new();
        let txt = m.render_prometheus();
        // Every metric must have HELP, TYPE, and value lines
        for line in txt.lines() {
            if line.starts_with("# HELP") || line.starts_with("# TYPE") || line.is_empty() {
                continue;
            }
            // Value line should be: name value
            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            assert_eq!(parts.len(), 2, "Bad format: {line}");
            assert!(parts[1].parse::<u64>().is_ok(), "Non-integer value: {line}");
        }
    }
}
