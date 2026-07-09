//! Simple metrics for zion-free-world.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct FreeWorldMetrics {
    pub blocks_scanned: AtomicU64,
    pub grants_pending: AtomicU64,
    pub grants_approved: AtomicU64,
    pub grants_disbursed: AtomicU64,
    pub projects_active: AtomicU64,
    pub total_accumulated_zion: AtomicU64,
    pub total_disbursed_zion: AtomicU64,
}

impl FreeWorldMetrics {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Serve Prometheus-style metrics text.
pub fn serve_metrics_text(metrics: &FreeWorldMetrics) -> String {
    format!(
        "# HELP zion_free_world_blocks_scanned Total L1 blocks scanned\n\
         # TYPE zion_free_world_blocks_scanned counter\n\
         zion_free_world_blocks_scanned {}\n\n\
         # HELP zion_free_world_grants_pending Number of pending grants\n\
         # TYPE zion_free_world_grants_pending gauge\n\
         zion_free_world_grants_pending {}\n\n\
         # HELP zion_free_world_grants_approved Number of approved grants\n\
         # TYPE zion_free_world_grants_approved gauge\n\
         zion_free_world_grants_approved {}\n\n\
         # HELP zion_free_world_grants_disbursed Number of disbursed grants\n\
         # TYPE zion_free_world_grants_disbursed gauge\n\
         zion_free_world_grants_disbursed {}\n\n\
         # HELP zion_free_world_projects_active Number of active projects\n\
         # TYPE zion_free_world_projects_active gauge\n\
         zion_free_world_projects_active {}\n\n\
         # HELP zion_free_world_total_accumulated_zion Total ZION accumulated\n\
         # TYPE zion_free_world_total_accumulated_zion gauge\n\
         zion_free_world_total_accumulated_zion {}\n\n\
         # HELP zion_free_world_total_disbursed_zion Total ZION disbursed\n\
         # TYPE zion_free_world_total_disbursed_zion gauge\n\
         zion_free_world_total_disbursed_zion {}\n",
        metrics.blocks_scanned.load(Ordering::Relaxed),
        metrics.grants_pending.load(Ordering::Relaxed),
        metrics.grants_approved.load(Ordering::Relaxed),
        metrics.grants_disbursed.load(Ordering::Relaxed),
        metrics.projects_active.load(Ordering::Relaxed),
        metrics.total_accumulated_zion.load(Ordering::Relaxed),
        metrics.total_disbursed_zion.load(Ordering::Relaxed),
    )
}
