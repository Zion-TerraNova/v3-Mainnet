use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::revenue_journal::{JournalPayload, RevenueJournal};

// ── Constants ──────────────────────────────────────────────────────
pub const ZION_ALLOCATION: f64 = 0.50;
pub const MULTI_ALGO_ALLOCATION: f64 = 0.25;
pub const NCL_ALLOCATION: f64 = 0.25;
pub const MIN_ZION_ALLOCATION: f64 = 0.50;

pub const MERGED_MINING_FEE: f64 = 0.05;
pub const PROFIT_SWITCH_FEE: f64 = 0.02;
pub const BLAKE3_EXTERNAL_FEE: f64 = 0.02;
pub const NCL_FEE: f64 = 0.10;

/// Protocol fee split for canonical ZION blocks (percentages).
///
/// WARNING: These values must stay in sync with `zion_core::emission`.
/// If you change any value here, you MUST also update:
///   1. V3/L1/core/src/emission.rs  (MINER_PCT, HUMANITARIAN_PCT, ISSOBELLA_PCT, POOL_FEE_PCT)
///   2. V3/L1/pool/src/pplns.rs     (FeeConfig::default)
///   3. V3/L1/pool/src/bin/server.rs (parse_env_u64 fallbacks)
///   4. V3/docs/MAINNET_CONSTANTS.md
///   5. docs/WP-Mainet/ whitepapers
pub const ZION_MINER_PCT: u64 = 89;
pub const ZION_HUMANITARIAN_PCT: u64 = 5;
pub const ZION_ISSOBELLA_PCT: u64 = 5;
pub const ZION_POOL_PCT: u64 = 1;

/// Circuit breaker threshold: consecutive failures before opening.
pub const CIRCUIT_BREAKER_THRESHOLD: u32 = 10;
/// Seconds before a tripped circuit breaker can be retried.
pub const CIRCUIT_BREAKER_RESET_SECS: u64 = 60;

/// Maximum number of recently-seen block heights retained for idempotence.
/// The set is pruned to keep heights within the most-recent window, which is
/// far larger than any plausible re-org or journal replay distance but keeps
/// long-running pools from growing memory unbounded.
pub const SEEN_HEIGHTS_WINDOW: u64 = 100_000;

// ── RevenueSource ──────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RevenueSource {
    Zion,
    KeccakBonus,
    Sha3Bonus,
    ProfitSwitch,
    /// Revenue from Blake3-compatible external coins (DCR, ALPH).
    /// Same fee rate as ProfitSwitch (2 %) since our algo already uses Blake3
    /// internally and the hash function is shared infrastructure.
    Blake3External,
    /// Revenue from kHeavyHash external coins (KAS).
    KHeavyHashExternal,
    /// Revenue from Ethash / Etchash external coins (ETC, EVR, MEWC).
    EthashExternal,
    /// Revenue from KawPow / ProgPow external coins (RVN, CLORE).
    KawPowExternal,
    /// Revenue from Autolykos v2 external coins (ERG).
    AutolykosExternal,
    /// Revenue from RandomX external coins (XMR).
    RandomXExternal,
    /// Revenue from ZelHash / Equihash external coins (FLUX).
    ZelHashExternal,
    /// Revenue from DeekshaLite v1 simplified mining (GCN-friendly).
    DeekshaLite,
    /// Revenue from DeekshaLite Fire thermal-intensive mining (winter heating).
    ThermalBonus,
    /// Revenue from AI / NCL compute layer.
    NclAi,
}

impl RevenueSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Zion => "zion",
            Self::KeccakBonus => "keccak_bonus",
            Self::Sha3Bonus => "sha3_bonus",
            Self::ProfitSwitch => "profit_switch",
            Self::Blake3External => "blake3_external",
            Self::KHeavyHashExternal => "kheavyhash_external",
            Self::EthashExternal => "ethash_external",
            Self::KawPowExternal => "kawpow_external",
            Self::AutolykosExternal => "autolykos_external",
            Self::RandomXExternal => "randomx_external",
            Self::ZelHashExternal => "zelhash_external",
            Self::DeekshaLite => "deeksha_lite",
            Self::ThermalBonus => "thermal_bonus",
            Self::NclAi => "ncl_ai",
        }
    }

    pub fn fee_rate(self) -> f64 {
        match self {
            Self::Zion | Self::KeccakBonus | Self::Sha3Bonus => MERGED_MINING_FEE,
            Self::ProfitSwitch
            | Self::Blake3External
            | Self::KHeavyHashExternal
            | Self::EthashExternal
            | Self::KawPowExternal
            | Self::AutolykosExternal
            | Self::RandomXExternal
            | Self::ZelHashExternal => BLAKE3_EXTERNAL_FEE,
            Self::DeekshaLite | Self::ThermalBonus => MERGED_MINING_FEE,
            Self::NclAi => NCL_FEE,
        }
    }
}

// ── RevenueEvent ───────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevenueEvent {
    pub source: RevenueSource,
    pub value_usd: f64,
    pub qualifies: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    /// Optional per-coin ticker for granular multi-algo revenue tracking.
    /// Examples: "DCR", "ALPH", "KAS", "ETC", "RVN", "ERG", "XMR", "FLUX".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_coin: Option<String>,
}

impl RevenueEvent {
    pub fn new(source: RevenueSource, value_usd: f64, qualifies: bool) -> Self {
        Self {
            source,
            value_usd,
            qualifies,
            timestamp: None,
            block_height: None,
            tx_hash: None,
            external_coin: None,
        }
    }

    pub fn with_height(mut self, height: u64) -> Self {
        self.block_height = Some(height);
        self
    }

    pub fn with_tx_hash(mut self, tx_hash: impl Into<String>) -> Self {
        self.tx_hash = Some(tx_hash.into());
        self
    }

    pub fn with_external_coin(mut self, coin: impl Into<String>) -> Self {
        self.external_coin = Some(coin.into());
        self
    }
}

// ── RevenueHealth ──────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevenueHealth {
    pub source: RevenueSource,
    pub last_success_ts: Option<String>,
    pub consecutive_failures: u32,
    pub total_events: u64,
    pub circuit_open: bool,
    /// Wall-clock time at which the circuit breaker most recently tripped.
    /// Used by `maybe_auto_reset` to enforce `CIRCUIT_BREAKER_RESET_SECS`
    /// cooldown independently of `last_success_ts` (which may be missing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circuit_opened_ts: Option<String>,
}

impl RevenueHealth {
    pub fn new(source: RevenueSource) -> Self {
        Self {
            source,
            last_success_ts: None,
            consecutive_failures: 0,
            total_events: 0,
            circuit_open: false,
            circuit_opened_ts: None,
        }
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.last_success_ts = Some(Utc::now().to_rfc3339());
        self.total_events += 1;
        self.circuit_open = false;
        self.circuit_opened_ts = None;
    }

    /// Records a failure. Returns `true` if the circuit breaker just opened.
    pub fn record_failure(&mut self) -> bool {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= CIRCUIT_BREAKER_THRESHOLD && !self.circuit_open {
            self.circuit_open = true;
            self.circuit_opened_ts = Some(Utc::now().to_rfc3339());
            return true;
        }
        false
    }

    /// Check whether the circuit breaker should auto-reset after the cooldown
    /// period has elapsed since it opened.  Call this before deciding to skip
    /// an event due to an open circuit.
    pub fn maybe_auto_reset(&mut self) {
        if !self.circuit_open {
            return;
        }
        // Prefer the explicit trip timestamp; fall back to last_success_ts for
        // backwards compatibility with serialized state that predates
        // `circuit_opened_ts`.
        let reference = self
            .circuit_opened_ts
            .as_ref()
            .or(self.last_success_ts.as_ref());
        if let Some(ts) = reference {
            if let Ok(opened) = ts.parse::<chrono::DateTime<chrono::Utc>>() {
                let elapsed = Utc::now().signed_duration_since(opened).num_seconds();
                if elapsed >= CIRCUIT_BREAKER_RESET_SECS as i64 {
                    self.reset();
                }
            } else {
                // Unparseable timestamp — reset rather than wedge the circuit
                // open forever.
                self.reset();
            }
        } else {
            // No timestamp at all (e.g. state created before this field
            // existed).  Reset so the source gets another chance.
            self.reset();
        }
    }

    pub fn reset(&mut self) {
        self.consecutive_failures = 0;
        self.circuit_open = false;
        self.circuit_opened_ts = None;
    }
}

// ── NclStats ───────────────────────────────────────────────────────
/// Snapshot of Neural Compute Layer (NCL) task activity.  Tracks the
/// 25 % AI-inference revenue stream end-to-end: how many tasks were
/// dispatched to the Hiran gateway, how many succeeded, latency, and
/// the USD-denominated revenue earned (gross — fees are already in
/// `RevenueStats::zion_fees_usd`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NclStats {
    pub tasks_total: u64,
    pub tasks_succeeded: u64,
    pub tasks_failed: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub total_latency_ms: u64,
    pub total_value_usd: f64,
    /// RFC3339 timestamp of the most recent successful task.
    pub last_success_ts: Option<String>,
}

impl NclStats {
    pub fn success_rate(&self) -> f64 {
        if self.tasks_total == 0 {
            0.0
        } else {
            self.tasks_succeeded as f64 / self.tasks_total as f64
        }
    }

    pub fn avg_latency_ms(&self) -> f64 {
        if self.tasks_succeeded == 0 {
            0.0
        } else {
            self.total_latency_ms as f64 / self.tasks_succeeded as f64
        }
    }

    pub fn avg_tokens_out(&self) -> f64 {
        if self.tasks_succeeded == 0 {
            0.0
        } else {
            self.tokens_out as f64 / self.tasks_succeeded as f64
        }
    }
}

// ── RevenueStats ───────────────────────────────────────────────────
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RevenueStats {
    pub total_earnings_usd: f64,
    pub zion_fees_usd: f64,
    pub miner_payout_usd: f64,
    pub by_source: HashMap<String, f64>,
    // ZION-denominated tracking for canonical Deeksha mining (flowers).
    pub total_zion: u64,
    pub zion_fees_zion: u64,
    pub humanitarian_zion: u64,
    pub issobella_zion: u64,
    pub miner_payout_zion: u64,
    pub blocks_found: u64,
    // Audit fields.
    pub last_block_height: u64,
    pub last_block_ts: Option<String>,
}

// ── RevenueCollector ───────────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct RevenueCollector {
    stats: Arc<RwLock<RevenueStats>>,
    ncl_stats: Arc<RwLock<NclStats>>,
    pending_fees_usd: Arc<RwLock<f64>>,
    pending_fees_zion: Arc<RwLock<u64>>,
    seen_heights: Arc<RwLock<HashSet<u64>>>,
    health: Arc<RwLock<HashMap<RevenueSource, RevenueHealth>>>,
    journal: Option<Arc<RevenueJournal>>,
}

impl Default for RevenueCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl RevenueCollector {
    pub fn new() -> Self {
        Self {
            stats: Arc::new(RwLock::new(RevenueStats::default())),
            ncl_stats: Arc::new(RwLock::new(NclStats::default())),
            pending_fees_usd: Arc::new(RwLock::new(0.0)),
            pending_fees_zion: Arc::new(RwLock::new(0)),
            seen_heights: Arc::new(RwLock::new(HashSet::new())),
            health: Arc::new(RwLock::new(HashMap::new())),
            journal: None,
        }
    }

    pub fn with_journal(journal: RevenueJournal) -> Self {
        let mut collector = Self::new();
        collector.journal = Some(Arc::new(journal));
        collector
    }

    /// Enable journaling from environment defaults.
    pub fn with_env_journal() -> Self {
        Self::with_journal(RevenueJournal::from_env_or_default())
    }

    // ── Tracking ─────────────────────────────────────────────────────

    /// Track a multi-chain revenue event (denominated in USD).
    pub fn track_event(&self, event: RevenueEvent) {
        if !event.qualifies {
            self.update_health_failure(event.source);
            return;
        }

        let fee = Self::calculate_fee(event.source, event.value_usd);
        let miner_share = event.value_usd - fee;

        let mut stats = self.stats.write().expect("revenue stats lock poisoned");
        stats.total_earnings_usd += event.value_usd;
        stats.zion_fees_usd += fee;
        stats.miner_payout_usd += miner_share;
        *stats
            .by_source
            .entry(event.source.as_str().to_string())
            .or_insert(0.0) += event.value_usd;

        let mut pending = self
            .pending_fees_usd
            .write()
            .expect("revenue pending-fees lock poisoned");
        *pending += fee;

        drop(stats);
        drop(pending);

        self.update_health_success(event.source);

        if let Some(ref journal) = self.journal {
            if let Err(e) = journal.append(JournalPayload::Event {
                source: event.source.as_str().to_string(),
                value_usd: event.value_usd,
                qualifies: event.qualifies,
                block_height: event.block_height,
            }) {
                eprintln!("revenue_journal_append_error: {}", e);
            }
        }
    }

    /// Track a canonical ZION Deeksha block reward (denominated in flowers).
    /// Uses the protocol fee split: 89 % miner / 5 % humanitarian / 5 % issobella / 1 % pool.
    /// Idempotent: the same `height` is ignored if already seen.
    pub fn track_zion_block(
        &self,
        height: u64,
        subsidy: u64,
        pool_fee_pct: u64,
        tx_hash: Option<String>,
    ) {
        {
            let mut seen = self
                .seen_heights
                .write()
                .expect("seen_heights lock poisoned");
            if !seen.insert(height) {
                // Already recorded — deduplicate.
                return;
            }
            // Prune anything older than the retention window so the set
            // cannot grow without bound on a long-running pool.
            if height > SEEN_HEIGHTS_WINDOW {
                let cutoff = height - SEEN_HEIGHTS_WINDOW;
                seen.retain(|h| *h >= cutoff);
            }
        }

        // Derive fee percentages from pool_fee_pct, falling back to defaults
        // when the caller passes 0 (for backward compatibility).
        let pool_pct = if pool_fee_pct == 0 {
            ZION_POOL_PCT
        } else {
            pool_fee_pct
        };
        // Humanitarian and issobella are protocol-level constants; only the
        // pool-fee share is configurable at call time.
        let humanitarian_pct = ZION_HUMANITARIAN_PCT;
        let issobella_pct = ZION_ISSOBELLA_PCT;
        let miner_pct = 100u64
            .saturating_sub(humanitarian_pct)
            .saturating_sub(issobella_pct)
            .saturating_sub(pool_pct);

        let miner_share = subsidy * miner_pct / 100;
        let humanitarian = subsidy * humanitarian_pct / 100;
        let issobella = subsidy * issobella_pct / 100;
        let pool_fee = subsidy * pool_pct / 100;

        // Adjust for rounding so total equals subsidy.
        let sum = miner_share + humanitarian + issobella + pool_fee;
        let miner_share = if sum < subsidy {
            miner_share + (subsidy - sum)
        } else {
            miner_share
        };

        let now = Utc::now().to_rfc3339();

        {
            let mut stats = self.stats.write().expect("revenue stats lock poisoned");
            stats.total_zion += subsidy;
            stats.zion_fees_zion += pool_fee;
            stats.humanitarian_zion += humanitarian;
            stats.issobella_zion += issobella;
            stats.miner_payout_zion += miner_share;
            stats.blocks_found += 1;
            stats.last_block_height = height;
            stats.last_block_ts = Some(now.clone());
            *stats
                .by_source
                .entry("zion_canonical".to_string())
                .or_insert(0.0) += subsidy as f64;

            let mut pending = self
                .pending_fees_zion
                .write()
                .expect("revenue pending-fees-zion lock poisoned");
            *pending += pool_fee;
        }

        self.update_health_success(RevenueSource::Zion);

        if let Some(ref journal) = self.journal {
            if let Err(e) = journal.append(JournalPayload::ZionBlock {
                height,
                subsidy,
                pool_fee,
                humanitarian,
                issobella,
                miner: miner_share,
                tx_hash,
            }) {
                eprintln!("revenue_journal_append_error: {}", e);
            }
        }
    }

    pub fn track_ncl_task(&self, value_usd: f64) {
        self.track_event(RevenueEvent {
            source: RevenueSource::NclAi,
            value_usd,
            qualifies: true,
            timestamp: Some(Utc::now().to_rfc3339()),
            block_height: None,
            tx_hash: None,
            external_coin: None,
        });
        // Bookkeeping-only update — full detail goes through track_ncl_task_detailed.
        let mut s = self.ncl_stats.write().expect("ncl_stats lock poisoned");
        s.tasks_total = s.tasks_total.saturating_add(1);
        if value_usd > 0.0 {
            s.tasks_succeeded = s.tasks_succeeded.saturating_add(1);
            s.total_value_usd += value_usd;
            s.last_success_ts = Some(Utc::now().to_rfc3339());
        } else {
            s.tasks_failed = s.tasks_failed.saturating_add(1);
        }
    }

    /// Record a fully-attributed NCL task: revenue + per-task telemetry
    /// (tokens, latency).  This is the canonical entrypoint used by the
    /// pool-side NCL gateway dispatcher.
    pub fn track_ncl_task_detailed(
        &self,
        value_usd: f64,
        tokens_in: u64,
        tokens_out: u64,
        latency_ms: u64,
        success: bool,
    ) {
        // Revenue side first (mirrors existing track_event semantics —
        // failed tasks contribute zero revenue but are counted in stats).
        if success {
            self.track_event(RevenueEvent {
                source: RevenueSource::NclAi,
                value_usd,
                qualifies: true,
                timestamp: Some(Utc::now().to_rfc3339()),
                block_height: None,
                tx_hash: None,
                external_coin: None,
            });
        } else {
            self.update_health_failure(RevenueSource::NclAi);
        }

        // Telemetry side.
        let mut s = self.ncl_stats.write().expect("ncl_stats lock poisoned");
        s.tasks_total = s.tasks_total.saturating_add(1);
        s.tokens_in = s.tokens_in.saturating_add(tokens_in);
        if success {
            s.tasks_succeeded = s.tasks_succeeded.saturating_add(1);
            s.tokens_out = s.tokens_out.saturating_add(tokens_out);
            s.total_latency_ms = s.total_latency_ms.saturating_add(latency_ms);
            s.total_value_usd += value_usd;
            s.last_success_ts = Some(Utc::now().to_rfc3339());
        } else {
            s.tasks_failed = s.tasks_failed.saturating_add(1);
        }
    }

    pub fn ncl_stats(&self) -> NclStats {
        self.ncl_stats
            .read()
            .expect("ncl_stats lock poisoned")
            .clone()
    }

    /// Track a Deeksha stream-telemetry bundle.
    ///
    /// Each step in the Deeksha pipeline is mapped to a revenue stream and
    /// recorded as a fractional revenue event. The total `value_usd` is split
    /// across streams proportionally to their work-unit weights.
    ///
    /// This is used by the pool to do granular per-stream accounting when a
    /// share or block is accepted.
    pub fn track_deeksha_streams(
        &self,
        telemetry: &crate::stream_layers::DeekshaStreamTelemetry,
        total_value_usd: f64,
        block_height: Option<u64>,
    ) {
        if telemetry.total_work == 0 || total_value_usd <= 0.0 {
            return;
        }
        for (source_name, units) in &telemetry.stream_breakdown {
            let source = match source_name.as_str() {
                "zion" => RevenueSource::Zion,
                "keccak_bonus" => RevenueSource::KeccakBonus,
                "sha3_bonus" => RevenueSource::Sha3Bonus,
                "ncl_ai" => RevenueSource::NclAi,
                _ => continue,
            };
            let share = total_value_usd * (*units as f64 / telemetry.total_work as f64);
            self.track_event(RevenueEvent {
                source,
                value_usd: share,
                qualifies: true,
                timestamp: Some(Utc::now().to_rfc3339()),
                block_height,
                tx_hash: None,
                external_coin: None,
            });
        }
    }

    // ── Replay (for startup recovery) ────────────────────────────────

    pub fn replay_zion_block(
        &self,
        height: u64,
        subsidy: u64,
        pool_fee: u64,
        humanitarian: u64,
        issobella: u64,
        miner: u64,
    ) {
        self.replay_zion_block_with_ts(
            height,
            subsidy,
            pool_fee,
            humanitarian,
            issobella,
            miner,
            None,
        );
    }

    /// Replay a Zion block while preserving the original journal timestamp.
    #[allow(clippy::too_many_arguments)]
    pub fn replay_zion_block_with_ts(
        &self,
        height: u64,
        subsidy: u64,
        pool_fee: u64,
        humanitarian: u64,
        issobella: u64,
        miner: u64,
        journal_ts: Option<String>,
    ) {
        let mut seen = self
            .seen_heights
            .write()
            .expect("seen_heights lock poisoned");
        if !seen.insert(height) {
            return;
        }
        drop(seen);

        let mut stats = self.stats.write().expect("revenue stats lock poisoned");
        stats.total_zion += subsidy;
        stats.zion_fees_zion += pool_fee;
        stats.humanitarian_zion += humanitarian;
        stats.issobella_zion += issobella;
        stats.miner_payout_zion += miner;
        stats.blocks_found += 1;
        // Track the highest height we have ever seen, not the last-replayed one,
        // so out-of-order journal entries cannot rewind the cursor.
        if height >= stats.last_block_height {
            stats.last_block_height = height;
            // Prefer the journal-supplied timestamp; fall back to current time
            // only if the entry was malformed or missing.
            stats.last_block_ts = journal_ts.or_else(|| Some(Utc::now().to_rfc3339()));
        }
        *stats
            .by_source
            .entry("zion_canonical".to_string())
            .or_insert(0.0) += subsidy as f64;

        let mut pending = self
            .pending_fees_zion
            .write()
            .expect("revenue pending-fees-zion lock poisoned");
        *pending += pool_fee;
    }

    pub fn replay_event(&self, source: RevenueSource, value_usd: f64, qualifies: bool) {
        if !qualifies {
            return;
        }
        let fee = Self::calculate_fee(source, value_usd);
        let miner_share = value_usd - fee;

        let mut stats = self.stats.write().expect("revenue stats lock poisoned");
        stats.total_earnings_usd += value_usd;
        stats.zion_fees_usd += fee;
        stats.miner_payout_usd += miner_share;
        *stats
            .by_source
            .entry(source.as_str().to_string())
            .or_insert(0.0) += value_usd;

        let mut pending = self
            .pending_fees_usd
            .write()
            .expect("revenue pending-fees lock poisoned");
        *pending += fee;
    }

    // ── Health ───────────────────────────────────────────────────────

    fn update_health_success(&self, source: RevenueSource) {
        let mut health = self.health.write().expect("health lock poisoned");
        health
            .entry(source)
            .or_insert_with(|| RevenueHealth::new(source))
            .record_success();
    }

    fn update_health_failure(&self, source: RevenueSource) {
        let mut health = self.health.write().expect("health lock poisoned");
        let h = health
            .entry(source)
            .or_insert_with(|| RevenueHealth::new(source));
        h.maybe_auto_reset();
        if h.record_failure() {
            // Circuit breaker just tripped — surface this so SREs can react.
            eprintln!(
                "revenue_circuit_open source={} consecutive_failures={}",
                source.as_str(),
                h.consecutive_failures
            );
        }
    }

    pub fn health_for(&self, source: RevenueSource) -> RevenueHealth {
        let health = self.health.read().expect("health lock poisoned");
        health
            .get(&source)
            .cloned()
            .unwrap_or_else(|| RevenueHealth::new(source))
    }

    pub fn all_health(&self) -> Vec<RevenueHealth> {
        let health = self.health.read().expect("health lock poisoned");
        health.values().cloned().collect()
    }

    // ── Getters / Payouts ──────────────────────────────────────────

    pub fn get_stats(&self) -> RevenueStats {
        self.stats
            .read()
            .expect("revenue stats lock poisoned")
            .clone()
    }

    pub fn get_pending_fees(&self) -> f64 {
        *self
            .pending_fees_usd
            .read()
            .expect("revenue pending-fees lock poisoned")
    }

    pub fn get_pending_fees_zion(&self) -> u64 {
        *self
            .pending_fees_zion
            .read()
            .expect("revenue pending-fees-zion lock poisoned")
    }

    pub fn process_payout(&self) -> f64 {
        let mut pending = self
            .pending_fees_usd
            .write()
            .expect("revenue pending-fees lock poisoned");
        let amount = *pending;
        *pending = 0.0;
        // Avoid journaling no-op payouts that would only add noise/IO.
        if amount > 0.0 {
            if let Some(ref journal) = self.journal {
                if let Err(e) = journal.append(JournalPayload::Payout { amount_usd: amount }) {
                    eprintln!("revenue_journal_append_error: {}", e);
                }
            }
        }
        amount
    }

    pub fn process_payout_zion(&self) -> u64 {
        let mut pending = self
            .pending_fees_zion
            .write()
            .expect("revenue pending-fees-zion lock poisoned");
        let amount = *pending;
        *pending = 0;
        if amount > 0 {
            if let Some(ref journal) = self.journal {
                if let Err(e) = journal.append(JournalPayload::PayoutZion { amount }) {
                    eprintln!("revenue_journal_append_error: {}", e);
                }
            }
        }
        amount
    }

    /// Replay all persisted Zion blocks and events from the journal into
    /// this collector.  Called automatically by `CoreRuntime::new_with_journal_replay`.
    pub fn replay(&self) {
        let Some(ref journal) = self.journal else {
            return;
        };
        match journal.replay_zion_blocks() {
            Ok(blocks) => {
                for block in &blocks {
                    self.replay_zion_block_with_ts(
                        block.height,
                        block.subsidy,
                        block.pool_fee,
                        block.humanitarian,
                        block.issobella,
                        block.miner,
                        Some(block.ts.clone()),
                    );
                }
                eprintln!("revenue_replay_zion_blocks loaded={}", blocks.len());
            }
            Err(e) => {
                eprintln!("revenue_replay_zion_blocks error: {}", e);
            }
        }
        match journal.replay_events() {
            Ok(events) => {
                for event in &events {
                    self.replay_event(
                        match event.source.as_str() {
                            "zion" => RevenueSource::Zion,
                            "keccak_bonus" => RevenueSource::KeccakBonus,
                            "sha3_bonus" => RevenueSource::Sha3Bonus,
                            "profit_switch" => RevenueSource::ProfitSwitch,
                            "blake3_external" => RevenueSource::Blake3External,
                            "kheavyhash_external" => RevenueSource::KHeavyHashExternal,
                            "ethash_external" => RevenueSource::EthashExternal,
                            "kawpow_external" => RevenueSource::KawPowExternal,
                            "autolykos_external" => RevenueSource::AutolykosExternal,
                            "randomx_external" => RevenueSource::RandomXExternal,
                            "zelhash_external" => RevenueSource::ZelHashExternal,
                            "ncl_ai" => RevenueSource::NclAi,
                            _ => continue,
                        },
                        event.value_usd,
                        event.qualifies,
                    );
                }
                eprintln!("revenue_replay_events loaded={}", events.len());
            }
            Err(e) => {
                eprintln!("revenue_replay_events error: {}", e);
            }
        }
    }

    pub fn calculate_fee(source: RevenueSource, value_usd: f64) -> f64 {
        value_usd * source.fee_rate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_mining_fee_rate_is_preserved() {
        let collector = RevenueCollector::new();
        collector.track_event(RevenueEvent {
            source: RevenueSource::KeccakBonus,
            value_usd: 10.0,
            qualifies: true,
            timestamp: None,
            block_height: None,
            tx_hash: None,
            external_coin: None,
        });

        let stats = collector.get_stats();
        assert_eq!(stats.total_earnings_usd, 10.0);
        assert!((stats.zion_fees_usd - 0.5).abs() < 0.001);
    }

    #[test]
    fn profit_switch_uses_lower_fee() {
        let fee = RevenueCollector::calculate_fee(RevenueSource::ProfitSwitch, 100.0);
        assert!((fee - 2.0).abs() < 0.001);
    }

    #[test]
    fn blake3_external_uses_same_fee_as_profit_switch() {
        let fee = RevenueCollector::calculate_fee(RevenueSource::Blake3External, 100.0);
        assert!((fee - 2.0).abs() < 0.001);
    }

    #[test]
    fn non_qualifying_revenue_is_ignored() {
        let collector = RevenueCollector::new();
        collector.track_event(RevenueEvent {
            source: RevenueSource::Zion,
            value_usd: 12.5,
            qualifies: false,
            timestamp: None,
            block_height: None,
            tx_hash: None,
            external_coin: None,
        });

        let stats = collector.get_stats();
        assert_eq!(stats.total_earnings_usd, 0.0);
        assert_eq!(stats.zion_fees_usd, 0.0);
    }

    #[test]
    fn track_zion_block_records_subsidy_and_split() {
        let collector = RevenueCollector::new();
        let subsidy = 5_400_067_000_000_000_u64;
        collector.track_zion_block(42, subsidy, 1, None);

        let stats = collector.get_stats();
        let expected_pool_fee = subsidy * ZION_POOL_PCT / 100;
        let expected_humanitarian = subsidy * ZION_HUMANITARIAN_PCT / 100;
        let expected_issobella = subsidy * ZION_ISSOBELLA_PCT / 100;
        let expected_miner =
            subsidy - expected_pool_fee - expected_humanitarian - expected_issobella;

        assert_eq!(stats.total_zion, subsidy);
        assert_eq!(stats.zion_fees_zion, expected_pool_fee);
        assert_eq!(stats.humanitarian_zion, expected_humanitarian);
        assert_eq!(stats.issobella_zion, expected_issobella);
        assert_eq!(stats.miner_payout_zion, expected_miner);
        assert_eq!(stats.blocks_found, 1);
        assert_eq!(stats.last_block_height, 42);
        assert_eq!(stats.total_earnings_usd, 0.0); // USD side untouched
        assert_eq!(collector.get_pending_fees_zion(), expected_pool_fee);
    }

    #[test]
    fn track_zion_block_zero_pool_fee_gives_all_to_miner() {
        let collector = RevenueCollector::new();
        let subsidy = 1_000_000_u64;
        collector.track_zion_block(1, subsidy, 0, None);

        let stats = collector.get_stats();
        assert_eq!(stats.total_zion, subsidy);
        assert_eq!(stats.zion_fees_zion, subsidy * ZION_POOL_PCT / 100);
        assert_eq!(
            stats.miner_payout_zion,
            subsidy - stats.zion_fees_zion - stats.humanitarian_zion - stats.issobella_zion
        );
    }

    #[test]
    fn idempotence_guard_prevents_double_counting() {
        let collector = RevenueCollector::new();
        let subsidy = 1_000_000_u64;
        collector.track_zion_block(100, subsidy, 1, None);
        collector.track_zion_block(100, subsidy, 1, None); // duplicate

        let stats = collector.get_stats();
        assert_eq!(stats.blocks_found, 1);
        assert_eq!(stats.total_zion, subsidy);
    }

    #[test]
    fn health_tracks_success_and_failure() {
        let collector = RevenueCollector::new();
        let source = RevenueSource::Blake3External;

        collector.track_event(RevenueEvent::new(source, 10.0, true));
        let h = collector.health_for(source);
        assert_eq!(h.consecutive_failures, 0);
        assert_eq!(h.total_events, 1);
        assert!(!h.circuit_open);

        collector.track_event(RevenueEvent::new(source, 5.0, false));
        let h = collector.health_for(source);
        assert_eq!(h.consecutive_failures, 1);
        assert!(!h.circuit_open);
    }

    #[test]
    fn circuit_breaker_auto_resets_after_cooldown() {
        let mut health = RevenueHealth::new(RevenueSource::Blake3External);
        // Trip the circuit breaker.
        for _ in 0..CIRCUIT_BREAKER_THRESHOLD {
            health.record_failure();
        }
        assert!(health.circuit_open);
        assert!(health.circuit_opened_ts.is_some());

        // Within the cooldown window the breaker must stay open.
        health.maybe_auto_reset();
        assert!(
            health.circuit_open,
            "must not reset before cooldown elapses"
        );

        // Back-date the trip timestamp past the cooldown and verify reset.
        let past = Utc::now() - chrono::Duration::seconds(CIRCUIT_BREAKER_RESET_SECS as i64 + 5);
        health.circuit_opened_ts = Some(past.to_rfc3339());
        health.maybe_auto_reset();
        assert!(!health.circuit_open);
        assert_eq!(health.consecutive_failures, 0);
        assert!(health.circuit_opened_ts.is_none());
    }

    #[test]
    fn circuit_breaker_stays_open_within_cooldown() {
        let mut health = RevenueHealth::new(RevenueSource::EthashExternal);
        for _ in 0..CIRCUIT_BREAKER_THRESHOLD {
            health.record_failure();
        }
        assert!(health.circuit_open);
        // Multiple auto-reset calls inside the cooldown must NOT clear it.
        for _ in 0..5 {
            health.maybe_auto_reset();
            assert!(health.circuit_open);
        }
    }

    #[test]
    fn pool_fee_pct_is_used_when_nonzero() {
        let collector = RevenueCollector::new();
        let subsidy = 1_000_000_u64;
        // Pass pool_fee_pct=2 instead of default 1.
        collector.track_zion_block(200, subsidy, 2, None);

        let stats = collector.get_stats();
        assert_eq!(stats.zion_fees_zion, subsidy * 2 / 100); // 2% pool fee
        assert_eq!(stats.humanitarian_zion, subsidy * 5 / 100); // 5% humanitarian
        assert_eq!(stats.issobella_zion, subsidy * 5 / 100); // 5% issobella
        assert_eq!(stats.miner_payout_zion, subsidy * 88 / 100); // 88% miner
                                                                 // Total must equal subsidy.
        assert_eq!(
            stats.miner_payout_zion
                + stats.humanitarian_zion
                + stats.issobella_zion
                + stats.zion_fees_zion,
            subsidy
        );
    }

    #[test]
    fn revenue_event_builder_works() {
        let e = RevenueEvent::new(RevenueSource::Zion, 1.0, true)
            .with_height(99)
            .with_tx_hash("abc123");
        assert_eq!(e.block_height, Some(99));
        assert_eq!(e.tx_hash, Some("abc123".to_string()));
    }

    #[test]
    fn track_deeksha_streams_splits_value_proportionally() {
        let collector = RevenueCollector::new();

        // Build synthetic telemetry matching a full Deeksha pipeline.
        let mut telemetry = crate::stream_layers::DeekshaStreamTelemetry::default();
        telemetry
            .steps
            .push((crate::stream_layers::DeekshaStep::Keccak256, 5));
        telemetry
            .steps
            .push((crate::stream_layers::DeekshaStep::Sha3_512, 5));
        telemetry
            .steps
            .push((crate::stream_layers::DeekshaStep::GoldenMatrix, 10));
        telemetry
            .steps
            .push((crate::stream_layers::DeekshaStep::MemoryHard, 55));
        telemetry
            .steps
            .push((crate::stream_layers::DeekshaStep::NpuMix, 15));
        telemetry
            .steps
            .push((crate::stream_layers::DeekshaStep::CosmicFusion, 10));
        telemetry.total_work = 100;
        telemetry.stream_breakdown.insert("zion".to_string(), 75);
        telemetry
            .stream_breakdown
            .insert("keccak_bonus".to_string(), 5);
        telemetry
            .stream_breakdown
            .insert("sha3_bonus".to_string(), 5);
        telemetry.stream_breakdown.insert("ncl_ai".to_string(), 15);

        collector.track_deeksha_streams(&telemetry, 100.0, Some(123));

        let stats = collector.get_stats();
        // 100 USD split: 75 ZION, 5 Keccak, 5 SHA3, 15 NCL
        // Fees: ZION 5% = 3.75, Keccak 5% = 0.25, SHA3 5% = 0.25, NCL 10% = 1.5
        // Total fees = 5.75
        assert!((stats.total_earnings_usd - 100.0).abs() < 0.001);
        assert!((stats.zion_fees_usd - 5.75).abs() < 0.1);
        assert!((stats.miner_payout_usd - 94.25).abs() < 0.1);
    }
}
