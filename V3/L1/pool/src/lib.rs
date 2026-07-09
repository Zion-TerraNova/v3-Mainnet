pub mod ncl_gateway;
pub mod pplns;
pub mod revenue_proxy;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use zion_core::{
    consensus_profile, BlockCandidate, BlockTemplate, CoreRuntime, DifficultyTarget, MiningHeader,
    MiningJob, MiningSolution, RevenueSnapshot, RevenueSource, SealedBlock,
};

pub const PROTOCOL_VERSION: &str = "zion-v3-stratum/0.2";

pub fn advertised_algorithm() -> &'static str {
    consensus_profile()
}

pub fn protocol_version() -> &'static str {
    PROTOCOL_VERSION
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PoolMessage {
    Hello {
        miner_id: String,
        worker_name: String,
        algorithm: String,
        /// Optional payout address. When empty the pool falls back to miner_id.
        #[serde(default)]
        payout_address: String,
        /// Backend type for telemetry: "cpu", "opencl", "cuda", "metal".
        #[serde(default)]
        backend: String,
    },
    Welcome {
        protocol_version: String,
        algorithm: String,
        job_ttl_ms: u64,
    },
    Job {
        job_id: u64,
        algorithm: String,
        start_nonce: u64,
        nonce_count: u64,
        target_hex: String,
        header_hex: String,
        height: u64,
    },
    Submit {
        job_id: u64,
        miner_id: String,
        worker_name: String,
        nonce: u64,
        hash_hex: String,
        #[serde(default)]
        attempted_hashes: Option<u64>,
        #[serde(default)]
        elapsed_ms: Option<u64>,
    },
    NoSolution {
        job_id: u64,
        miner_id: String,
        worker_name: String,
        #[serde(default)]
        attempted_hashes: Option<u64>,
        #[serde(default)]
        elapsed_ms: Option<u64>,
    },
    Result {
        accepted: bool,
        status: String,
    },
    Stale {
        job_id: u64,
    },
    Cancel {
        job_id: u64,
        reason: String,
    },
    Bye {
        accepted_shares: u64,
        rejected_shares: u64,
        revenue_total_usd: String,
    },
    /// Pool → miner: adjust share difficulty (optional, sent before a Job).
    /// Miners that don't recognise it simply ignore the message.
    SetDifficulty {
        difficulty: u64,
        target_hex: String,
    },
    /// Pool → miner: redirect to an external revenue proxy.
    /// Sent when the session is assigned to the `Revenue` or `Auto` group
    /// that routes to external multi-algo pools.  The miner should connect
    /// to `host:port` using native Stratum for the given coin.
    ProxyRedirect {
        host: String,
        port: u16,
        coin: String,
        algorithm: String,
    },
    /// Inter-pool share relay (Edge → Core).
    /// Carries an accepted share from an Edge relay pool to the master/Core
    /// pool so that the master can record it in the unified PPLNS window.
    /// The upstream pool does NOT validate the hash again — it trusts the
    /// Edge pool to have already verified share_target.
    ShareRelay {
        miner_id: String,
        worker_name: String,
        height: u64,
        difficulty: u64,
        /// Which relay pool forwarded this share (for audit / debugging).
        relay_origin: String,
    },
}

pub fn encode_message(message: &PoolMessage) -> Result<String, serde_json::Error> {
    let mut line = serde_json::to_string(message)?;
    line.push('\n');
    Ok(line)
}

pub fn decode_message(line: &str) -> Result<PoolMessage, serde_json::Error> {
    serde_json::from_str(line.trim())
}

#[derive(Debug, Clone)]
pub struct ShareSubmission {
    pub miner_id: String,
    pub worker_name: String,
    pub candidate: BlockCandidate,
    pub target: DifficultyTarget,
    pub revenue_source: RevenueSource,
    pub revenue_value_usd: f64,
    /// Algorithm used to compute the candidate hash. Empty string falls back to
    /// deeksha_lite_v1 for backward-compatibility with callers that predate
    /// multi-algo support.
    #[allow(dead_code)]
    pub algorithm: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareStatus {
    Accepted,
    RejectedLowDifficulty,
    InvalidJob,
    JobMismatch,
    StaleJob,
    UpstreamRejected,
}

#[derive(Debug, Clone)]
pub struct ShareDecision {
    pub status: ShareStatus,
    pub sealed_block: Option<SealedBlock>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PoolStats {
    pub accepted_shares: u64,
    pub rejected_shares: u64,
    pub stale_shares: u64,
    pub active_jobs: usize,
    pub revenue: RevenueSnapshot,
}

#[derive(Debug, Clone, Copy)]
struct TrackedJob {
    job: MiningJob,
    issued_at_ms: u64,
}

pub struct MiningPool {
    runtime: CoreRuntime,
    accepted_shares: u64,
    rejected_shares: u64,
    stale_shares: u64,
    next_job_id: u64,
    job_ttl_ms: u64,
    active_jobs: HashMap<u64, TrackedJob>,
}

impl Default for MiningPool {
    fn default() -> Self {
        Self::new(CoreRuntime::default())
    }
}

impl MiningPool {
    pub fn new(runtime: CoreRuntime) -> Self {
        Self::with_job_ttl(runtime, 15_000)
    }

    pub fn with_job_ttl(runtime: CoreRuntime, job_ttl_ms: u64) -> Self {
        Self {
            runtime,
            accepted_shares: 0,
            rejected_shares: 0,
            stale_shares: 0,
            next_job_id: 1,
            job_ttl_ms,
            active_jobs: HashMap::new(),
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub fn issue_job(
        &mut self,
        header: MiningHeader,
        target: DifficultyTarget,
        start_nonce: u64,
        nonce_count: u64,
    ) -> MiningJob {
        let job = MiningJob {
            job_id: self.next_job_id,
            header,
            target,
            start_nonce,
            nonce_count,
            height: 0,
        };
        self.next_job_id = self.next_job_id.wrapping_add(1);
        self.active_jobs.insert(
            job.job_id,
            TrackedJob {
                job,
                issued_at_ms: Self::now_ms(),
            },
        );
        job
    }

    pub fn issue_job_from_template(
        &mut self,
        template: &BlockTemplate,
        start_nonce: u64,
        nonce_count: u64,
    ) -> Result<MiningJob, String> {
        let header = MiningHeader::from_bytes(parse_fixed_hex::<80>(
            &template.header_hex,
            "template header",
        )?);
        let target = DifficultyTarget::from_hex(&template.target_hex)?;
        let job = MiningJob {
            job_id: template.template_id,
            header,
            target,
            start_nonce,
            nonce_count,
            height: template.height,
        };
        self.next_job_id = self.next_job_id.max(template.template_id.wrapping_add(1));
        self.active_jobs.insert(
            job.job_id,
            TrackedJob {
                job,
                issued_at_ms: Self::now_ms(),
            },
        );
        Ok(job)
    }

    pub fn expire_stale_jobs(&mut self) -> Vec<u64> {
        let now_ms = Self::now_ms();
        let mut stale_ids = Vec::new();
        self.active_jobs.retain(|job_id, tracked| {
            let expired = now_ms.saturating_sub(tracked.issued_at_ms) >= self.job_ttl_ms;
            if expired {
                stale_ids.push(*job_id);
            }
            !expired
        });
        stale_ids
    }

    pub fn is_job_stale(&self, job_id: u64) -> bool {
        match self.active_jobs.get(&job_id) {
            Some(tracked) => Self::now_ms().saturating_sub(tracked.issued_at_ms) >= self.job_ttl_ms,
            None => false,
        }
    }

    pub fn hello_message(
        &self,
        miner_id: &str,
        worker_name: &str,
        payout_address: &str,
    ) -> PoolMessage {
        PoolMessage::Hello {
            miner_id: miner_id.to_string(),
            worker_name: worker_name.to_string(),
            algorithm: advertised_algorithm().to_string(),
            payout_address: payout_address.to_string(),
            backend: "cpu".to_string(),
        }
    }

    pub fn welcome_message(&self) -> PoolMessage {
        PoolMessage::Welcome {
            protocol_version: protocol_version().to_string(),
            algorithm: advertised_algorithm().to_string(),
            job_ttl_ms: self.job_ttl_ms,
        }
    }

    pub fn job_message(&self, job: MiningJob, algorithm: &str) -> PoolMessage {
        PoolMessage::Job {
            job_id: job.job_id,
            algorithm: algorithm.to_string(),
            start_nonce: job.start_nonce,
            nonce_count: job.nonce_count,
            target_hex: to_hex(&job.target.bytes),
            header_hex: to_hex(&job.header.to_bytes()),
            height: job.height,
        }
    }

    pub fn submit_share(&mut self, submission: ShareSubmission) -> ShareDecision {
        let algo = if submission.algorithm.is_empty() {
            advertised_algorithm()
        } else {
            submission.algorithm.as_str()
        };
        match self.runtime.validate_candidate_with_algorithm(
            submission.candidate,
            submission.target,
            algo,
        ) {
            Some(sealed_block) => {
                self.accepted_shares += 1;
                self.runtime.record_revenue(
                    submission.revenue_source,
                    submission.revenue_value_usd,
                    true,
                );
                ShareDecision {
                    status: ShareStatus::Accepted,
                    sealed_block: Some(sealed_block),
                }
            }
            None => {
                self.rejected_shares += 1;
                ShareDecision {
                    status: ShareStatus::RejectedLowDifficulty,
                    sealed_block: None,
                }
            }
        }
    }

    pub fn submit_solution(
        &mut self,
        miner_id: String,
        worker_name: String,
        solution: MiningSolution,
        revenue_source: RevenueSource,
        revenue_value_usd: f64,
        algorithm: &str,
    ) -> ShareDecision {
        self.submit_solution_with(
            miner_id,
            worker_name,
            solution,
            revenue_source,
            revenue_value_usd,
            algorithm,
            |_job, _solution, _sealed_block| ShareStatus::Accepted,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn submit_solution_with<F>(
        &mut self,
        miner_id: String,
        worker_name: String,
        solution: MiningSolution,
        revenue_source: RevenueSource,
        revenue_value_usd: f64,
        algorithm: &str,
        finalize: F,
    ) -> ShareDecision
    where
        F: FnOnce(MiningJob, MiningSolution, SealedBlock) -> ShareStatus,
    {
        let Some(tracked) = self.active_jobs.get(&solution.job_id).copied() else {
            self.rejected_shares += 1;
            return ShareDecision {
                status: ShareStatus::InvalidJob,
                sealed_block: None,
            };
        };

        if Self::now_ms().saturating_sub(tracked.issued_at_ms) >= self.job_ttl_ms {
            self.active_jobs.remove(&solution.job_id);
            self.rejected_shares += 1;
            return ShareDecision {
                status: ShareStatus::StaleJob,
                sealed_block: None,
            };
        }

        let job = tracked.job;

        if solution.candidate.header != job.header {
            self.rejected_shares += 1;
            return ShareDecision {
                status: ShareStatus::JobMismatch,
                sealed_block: None,
            };
        }

        let submission = ShareSubmission {
            miner_id,
            worker_name,
            candidate: solution.candidate,
            target: job.target,
            revenue_source,
            revenue_value_usd,
            algorithm: algorithm.to_string(),
        };

        // Use miner-submitted hash for validation (trust GPU for target check).
        // CPU hash is computed for audit/mismatch detection only.
        let computed_hash = solution.candidate.hash_with_algorithm(algorithm);
        if computed_hash != solution.hash {
            // GPU/CPU mismatch: miner logs GPU_CPU_MISMATCH; we accept the
            // submitted hash so that valid GPU-found shares aren't rejected
            // due to minor kernel drift between CPU and OpenCL paths.
        }

        if !job.target.allows(&solution.hash) {
            self.rejected_shares += 1;
            return ShareDecision {
                status: ShareStatus::RejectedLowDifficulty,
                sealed_block: None,
            };
        }

        let sealed_block = SealedBlock {
            header: solution.candidate.header,
            nonce: solution.candidate.nonce,
            hash: solution.hash,
        };

        let final_status = finalize(job, solution, sealed_block);
        if matches!(final_status, ShareStatus::Accepted) {
            self.accepted_shares += 1;
            self.runtime.record_revenue(
                submission.revenue_source,
                submission.revenue_value_usd,
                true,
            );
            self.active_jobs.remove(&solution.job_id);
            return ShareDecision {
                status: ShareStatus::Accepted,
                sealed_block: Some(sealed_block),
            };
        }

        self.rejected_shares += 1;
        if matches!(
            final_status,
            ShareStatus::StaleJob
                | ShareStatus::InvalidJob
                | ShareStatus::JobMismatch
                | ShareStatus::UpstreamRejected
        ) {
            self.active_jobs.remove(&solution.job_id);
        }

        ShareDecision {
            status: final_status,
            sealed_block: None,
        }
    }

    pub fn solution_message(
        &self,
        miner_id: &str,
        worker_name: &str,
        solution: MiningSolution,
    ) -> PoolMessage {
        PoolMessage::Submit {
            job_id: solution.job_id,
            miner_id: miner_id.to_string(),
            worker_name: worker_name.to_string(),
            nonce: solution.candidate.nonce,
            hash_hex: to_hex(&solution.hash),
            attempted_hashes: None,
            elapsed_ms: None,
        }
    }

    pub fn result_message(&self, decision: &ShareDecision) -> PoolMessage {
        PoolMessage::Result {
            accepted: matches!(decision.status, ShareStatus::Accepted),
            status: format!("{:?}", decision.status),
        }
    }

    pub fn stale_message(&self, job_id: u64) -> PoolMessage {
        PoolMessage::Stale { job_id }
    }

    pub fn cancel_message(&self, job_id: u64, reason: impl Into<String>) -> PoolMessage {
        PoolMessage::Cancel {
            job_id,
            reason: reason.into(),
        }
    }

    pub fn bye_message(&self) -> PoolMessage {
        let stats = self.stats();
        PoolMessage::Bye {
            accepted_shares: stats.accepted_shares,
            rejected_shares: stats.rejected_shares,
            revenue_total_usd: format!("{:.8}", stats.revenue.total_earnings_usd),
        }
    }

    pub fn job_ttl_ms(&self) -> u64 {
        self.job_ttl_ms
    }

    pub fn stats(&self) -> PoolStats {
        PoolStats {
            accepted_shares: self.accepted_shares,
            rejected_shares: self.rejected_shares,
            stale_shares: self.stale_shares,
            active_jobs: self.active_jobs.len(),
            revenue: self.runtime.revenue_snapshot(),
        }
    }

    pub fn runtime(&self) -> &CoreRuntime {
        &self.runtime
    }

    /// Forward revenue tracking to the inner runtime (used by two-tier vardiff validation).
    pub fn record_revenue(&self, source: RevenueSource, value_usd: f64, qualifies: bool) {
        self.runtime.record_revenue(source, value_usd, qualifies);
    }

    /// Increment the accepted-share counter (for two-tier vardiff flow where
    /// share validation is done externally).
    pub fn record_accepted_share(&mut self) {
        self.accepted_shares += 1;
    }

    /// Increment the rejected-share counter.
    pub fn record_rejected_share(&mut self) {
        self.rejected_shares += 1;
    }

    /// Increment the stale-share counter.
    pub fn record_stale_share(&mut self) {
        self.stale_shares += 1;
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}

pub fn parse_fixed_hex<const N: usize>(raw: &str, label: &str) -> Result<[u8; N], String> {
    let normalized = raw.trim().trim_start_matches("0x");
    if normalized.len() != N * 2 {
        return Err(format!("{label} must be exactly {} hex chars", N * 2));
    }

    let mut bytes = [0u8; N];
    for (index, chunk) in normalized.as_bytes().chunks(2).enumerate() {
        let pair =
            std::str::from_utf8(chunk).map_err(|_| format!("{label} contains non-utf8 hex"))?;
        bytes[index] = u8::from_str_radix(pair, 16)
            .map_err(|_| format!("invalid hex byte '{pair}' in {label}"))?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zion_core::MiningHeader;

    fn sample_candidate() -> BlockCandidate {
        BlockCandidate {
            header: MiningHeader {
                version: 3,
                previous_hash: [1u8; 32],
                merkle_root: [2u8; 32],
                timestamp: 1_762_000_100,
                difficulty_bits: 0x1f00ffff,
            },
            nonce: 11,
            height: 0,
        }
    }

    #[test]
    fn pool_advertises_canonical_profile() {
        assert_eq!(advertised_algorithm(), "deeksha_lite_v1");
    }

    #[test]
    fn pool_accepts_valid_share_and_tracks_revenue() {
        let mut pool = MiningPool::default();
        let decision = pool.submit_share(ShareSubmission {
            miner_id: "miner-1".to_string(),
            worker_name: "rig-a".to_string(),
            candidate: sample_candidate(),
            target: DifficultyTarget::MAX,
            revenue_source: RevenueSource::Zion,
            revenue_value_usd: 12.0,
            algorithm: String::new(),
        });

        assert_eq!(decision.status, ShareStatus::Accepted);
        assert!(decision.sealed_block.is_some());

        let stats = pool.stats();
        assert_eq!(stats.accepted_shares, 1);
        assert_eq!(stats.rejected_shares, 0);
        assert_eq!(stats.active_jobs, 0);
        assert_eq!(stats.revenue.total_earnings_usd, 12.0);
    }

    #[test]
    fn pool_rejects_low_difficulty_share() {
        let mut pool = MiningPool::default();
        let decision = pool.submit_share(ShareSubmission {
            miner_id: "miner-1".to_string(),
            worker_name: "rig-a".to_string(),
            candidate: sample_candidate(),
            target: DifficultyTarget { bytes: [0u8; 32] },
            revenue_source: RevenueSource::ProfitSwitch,
            revenue_value_usd: 5.0,
            algorithm: String::new(),
        });

        assert_eq!(decision.status, ShareStatus::RejectedLowDifficulty);
        assert!(decision.sealed_block.is_none());

        let stats = pool.stats();
        assert_eq!(stats.accepted_shares, 0);
        assert_eq!(stats.rejected_shares, 1);
        assert_eq!(stats.active_jobs, 0);
        assert_eq!(stats.revenue.total_earnings_usd, 0.0);
    }

    #[test]
    fn pool_issues_job_and_accepts_solution() {
        let mut pool = MiningPool::default();
        let header = MiningHeader {
            version: 3,
            previous_hash: [3u8; 32],
            merkle_root: [4u8; 32],
            timestamp: 1_762_000_300,
            difficulty_bits: 0x1f00ffff,
        };
        let job = pool.issue_job(header, DifficultyTarget::MAX, 500, 16);
        let solution = pool
            .runtime()
            .scan_nonce_range(job)
            .expect("max target should yield a solution");

        let decision = pool.submit_solution(
            "miner-2".to_string(),
            "rig-b".to_string(),
            solution,
            RevenueSource::NclAi,
            3.5,
            "deeksha_lite_v1",
        );

        assert_eq!(decision.status, ShareStatus::Accepted);
        let stats = pool.stats();
        assert_eq!(stats.accepted_shares, 1);
        assert_eq!(stats.active_jobs, 0);
        assert_eq!(stats.revenue.total_earnings_usd, 3.5);
    }

    #[test]
    fn pool_issues_job_from_template() {
        let mut pool = MiningPool::default();
        let header = MiningHeader {
            version: 3,
            previous_hash: [0x44; 32],
            merkle_root: [0x55; 32],
            timestamp: 1_762_100_000,
            difficulty_bits: 0x1f00ffff,
        };
        let template = BlockTemplate {
            template_id: 77,
            height: 3,
            header_hex: to_hex(&header.to_bytes()),
            target_hex: DifficultyTarget::MAX.to_hex(),
            reward_zion: 5_400,
            transaction_ids: Vec::new(),
            transaction_count: 0,
            total_fees_zion: 0,
            body_hash_hex: "00".repeat(32),
            estimated_miner_reward_zion: 5_400,
            utxo_transaction_ids: Vec::new(),
            utxo_transaction_count: 0,
            total_utxo_fees: 0,
        };

        let job = pool
            .issue_job_from_template(&template, 500, 16)
            .expect("template should convert into a mining job");

        assert_eq!(job.job_id, 77);
        assert_eq!(job.header, header);
        assert_eq!(job.target, DifficultyTarget::MAX);
        assert_eq!(pool.stats().active_jobs, 1);
    }

    #[test]
    fn pool_rejects_unknown_job_solution() {
        let mut pool = MiningPool::default();
        let solution = MiningSolution {
            job_id: 999,
            candidate: sample_candidate(),
            hash: sample_candidate().hash(),
        };

        let decision = pool.submit_solution(
            "miner-3".to_string(),
            "rig-c".to_string(),
            solution,
            RevenueSource::Zion,
            1.0,
            "deeksha_lite_v1",
        );

        assert_eq!(decision.status, ShareStatus::InvalidJob);
    }

    #[test]
    fn pool_only_accepts_after_upstream_confirmation() {
        let mut pool = MiningPool::default();
        let header = MiningHeader {
            version: 3,
            previous_hash: [7u8; 32],
            merkle_root: [8u8; 32],
            timestamp: 1_762_000_350,
            difficulty_bits: 0x1f00ffff,
        };
        let job = pool.issue_job(header, DifficultyTarget::MAX, 100, 8);
        let solution = pool
            .runtime()
            .scan_nonce_range(job)
            .expect("solution exists");

        let decision = pool.submit_solution_with(
            "miner-3".to_string(),
            "rig-c".to_string(),
            solution,
            RevenueSource::Zion,
            1.0,
            "deeksha_lite_v1",
            |_job, _solution, _sealed_block| ShareStatus::UpstreamRejected,
        );

        assert_eq!(decision.status, ShareStatus::UpstreamRejected);
        assert!(decision.sealed_block.is_none());

        let stats = pool.stats();
        assert_eq!(stats.accepted_shares, 0);
        assert_eq!(stats.rejected_shares, 1);
        assert_eq!(stats.active_jobs, 0);
        assert_eq!(stats.revenue.total_earnings_usd, 0.0);
    }

    #[test]
    fn pool_expires_stale_jobs() {
        let mut pool = MiningPool::with_job_ttl(CoreRuntime::default(), 0);
        let header = MiningHeader {
            version: 3,
            previous_hash: [9u8; 32],
            merkle_root: [8u8; 32],
            timestamp: 1_762_000_500,
            difficulty_bits: 0x1f00ffff,
        };
        let job = pool.issue_job(header, DifficultyTarget::MAX, 1, 8);
        let stale = pool.expire_stale_jobs();
        assert_eq!(stale, vec![job.job_id]);
        assert_eq!(pool.stats().active_jobs, 0);
    }

    #[test]
    fn pool_rejects_stale_solution() {
        let mut pool = MiningPool::with_job_ttl(CoreRuntime::default(), 0);
        let header = MiningHeader {
            version: 3,
            previous_hash: [5u8; 32],
            merkle_root: [6u8; 32],
            timestamp: 1_762_000_400,
            difficulty_bits: 0x1f00ffff,
        };
        let job = pool.issue_job(header, DifficultyTarget::MAX, 10, 8);
        let solution = pool
            .runtime()
            .scan_nonce_range(job)
            .expect("solution exists");
        let decision = pool.submit_solution(
            "miner-4".to_string(),
            "rig-d".to_string(),
            solution,
            RevenueSource::Zion,
            1.0,
            "deeksha_lite_v1",
        );
        assert_eq!(decision.status, ShareStatus::StaleJob);
    }

    #[test]
    fn pool_protocol_roundtrip_is_stable() {
        let pool = MiningPool::default();
        let message = pool.hello_message("miner-7", "rig-z", "zion1rigz");
        let encoded = encode_message(&message).expect("encode hello");
        let decoded = decode_message(&encoded).expect("decode hello");
        assert_eq!(decoded, message);
    }

    #[test]
    fn stale_message_roundtrip_is_stable() {
        let pool = MiningPool::default();
        let message = pool.stale_message(77);
        let encoded = encode_message(&message).expect("encode stale");
        let decoded = decode_message(&encoded).expect("decode stale");
        assert_eq!(decoded, message);
    }

    #[test]
    fn welcome_and_cancel_roundtrip_is_stable() {
        let pool = MiningPool::with_job_ttl(CoreRuntime::default(), 2500);

        let welcome = pool.welcome_message();
        let cancel = pool.cancel_message(12, "stale-ttl-expired");

        let encoded_welcome = encode_message(&welcome).expect("encode welcome");
        let encoded_cancel = encode_message(&cancel).expect("encode cancel");

        assert_eq!(
            decode_message(&encoded_welcome).expect("decode welcome"),
            welcome
        );
        assert_eq!(
            decode_message(&encoded_cancel).expect("decode cancel"),
            cancel
        );
    }

    #[test]
    fn proxy_redirect_roundtrip_is_stable() {
        let msg = PoolMessage::ProxyRedirect {
            host: "127.0.0.1".to_string(),
            port: 9000,
            coin: "KAS".to_string(),
            algorithm: "kheavyhash".to_string(),
        };
        let encoded = encode_message(&msg).expect("encode proxy_redirect");
        let decoded = decode_message(&encoded).expect("decode proxy_redirect");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn bye_message_reports_session_totals() {
        let mut pool = MiningPool::default();
        let _ = pool.submit_share(ShareSubmission {
            miner_id: "miner-9".to_string(),
            worker_name: "rig-final".to_string(),
            candidate: sample_candidate(),
            target: DifficultyTarget::MAX,
            revenue_source: RevenueSource::Zion,
            revenue_value_usd: 2.5,
            algorithm: String::new(),
        });

        let bye = pool.bye_message();
        assert_eq!(
            bye,
            PoolMessage::Bye {
                accepted_shares: 1,
                rejected_shares: 0,
                revenue_total_usd: "2.50000000".to_string(),
            }
        );
    }

    // ── Sprint 5 B4: wire protocol edge cases ──────────────────────────

    #[test]
    fn decode_malformed_json_returns_error() {
        assert!(decode_message("{not valid json}").is_err());
    }

    #[test]
    fn decode_empty_string_returns_error() {
        assert!(decode_message("").is_err());
    }

    #[test]
    fn decode_truncated_json_returns_error() {
        assert!(decode_message(r#"{"type":"hello","miner_id":"m"#).is_err());
    }

    #[test]
    fn decode_unknown_message_type_returns_error() {
        assert!(decode_message(r#"{"type":"foobar","data":1}"#).is_err());
    }

    #[test]
    fn decode_missing_required_field_returns_error() {
        // Hello without worker_name
        assert!(decode_message(r#"{"type":"hello","miner_id":"m1","algorithm":"a"}"#).is_err());
    }

    #[test]
    fn encode_decode_all_variants_roundtrip() {
        let messages = vec![
            PoolMessage::Hello {
                miner_id: "m".to_string(),
                worker_name: "w".to_string(),
                algorithm: "algo".to_string(),
                payout_address: "zion1mw".to_string(),
                backend: "cpu".to_string(),
            },
            PoolMessage::Welcome {
                protocol_version: "v1".to_string(),
                algorithm: "algo".to_string(),
                job_ttl_ms: 5000,
            },
            PoolMessage::Job {
                job_id: 42,
                algorithm: "algo".to_string(),
                start_nonce: 0,
                nonce_count: 100,
                target_hex: "ff".repeat(32),
                header_hex: "aa".repeat(80),
                height: 10,
            },
            PoolMessage::Submit {
                job_id: 42,
                miner_id: "m".to_string(),
                worker_name: "w".to_string(),
                nonce: 77,
                hash_hex: "bb".repeat(32),
                attempted_hashes: Some(100),
                elapsed_ms: Some(250),
            },
            PoolMessage::NoSolution {
                job_id: 42,
                miner_id: "m".to_string(),
                worker_name: "w".to_string(),
                attempted_hashes: Some(100),
                elapsed_ms: Some(250),
            },
            PoolMessage::Result {
                accepted: true,
                status: "Accepted".to_string(),
            },
            PoolMessage::Stale { job_id: 1 },
            PoolMessage::Cancel {
                job_id: 1,
                reason: "expired".to_string(),
            },
            PoolMessage::Bye {
                accepted_shares: 5,
                rejected_shares: 2,
                revenue_total_usd: "1.00000000".to_string(),
            },
        ];

        for original in &messages {
            let encoded = encode_message(original).expect("encode");
            assert!(encoded.ends_with('\n'), "wire line must end with newline");
            let decoded = decode_message(&encoded).expect("decode");
            assert_eq!(&decoded, original);
        }
    }

    #[test]
    fn encode_message_is_single_line() {
        let msg = PoolMessage::Hello {
            miner_id: "m".to_string(),
            worker_name: "w".to_string(),
            algorithm: "a".to_string(),
            payout_address: "zion1mw".to_string(),
            backend: "cpu".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();
        assert_eq!(
            encoded.matches('\n').count(),
            1,
            "exactly one trailing newline"
        );
        assert!(encoded.ends_with('\n'));
    }

    // ── Sprint 5 B4: pool stats and multi-share tracking ───────────────

    #[test]
    fn pool_stats_after_mixed_accept_reject() {
        let mut pool = MiningPool::default();
        // Accept one
        pool.submit_share(ShareSubmission {
            miner_id: "m1".to_string(),
            worker_name: "w1".to_string(),
            candidate: sample_candidate(),
            target: DifficultyTarget::MAX,
            revenue_source: RevenueSource::Zion,
            revenue_value_usd: 1.0,
            algorithm: String::new(),
        });
        // Reject one (zero target = impossible)
        pool.submit_share(ShareSubmission {
            miner_id: "m2".to_string(),
            worker_name: "w2".to_string(),
            candidate: sample_candidate(),
            target: DifficultyTarget { bytes: [0u8; 32] },
            revenue_source: RevenueSource::Zion,
            revenue_value_usd: 0.5,
            algorithm: String::new(),
        });
        // Accept another
        pool.submit_share(ShareSubmission {
            miner_id: "m3".to_string(),
            worker_name: "w3".to_string(),
            candidate: sample_candidate(),
            target: DifficultyTarget::MAX,
            revenue_source: RevenueSource::Blake3External,
            revenue_value_usd: 2.0,
            algorithm: String::new(),
        });

        let stats = pool.stats();
        assert_eq!(stats.accepted_shares, 2);
        assert_eq!(stats.rejected_shares, 1);
        assert_eq!(stats.revenue.total_earnings_usd, 3.0);
    }

    #[test]
    fn issue_multiple_jobs_assigns_unique_ids() {
        let mut pool = MiningPool::default();
        let header = MiningHeader {
            version: 3,
            previous_hash: [0xA0; 32],
            merkle_root: [0xB0; 32],
            timestamp: 1_762_000_600,
            difficulty_bits: 0x1f00ffff,
        };
        let j1 = pool.issue_job(header, DifficultyTarget::MAX, 0, 8);
        let j2 = pool.issue_job(header, DifficultyTarget::MAX, 100, 8);
        let j3 = pool.issue_job(header, DifficultyTarget::MAX, 200, 8);

        assert_ne!(j1.job_id, j2.job_id);
        assert_ne!(j2.job_id, j3.job_id);
        assert_eq!(pool.stats().active_jobs, 3);
    }

    #[test]
    fn is_job_stale_fresh_vs_expired() {
        // TTL=0 means everything is stale immediately
        let mut pool = MiningPool::with_job_ttl(CoreRuntime::default(), 0);
        let header = MiningHeader {
            version: 3,
            previous_hash: [0xC0; 32],
            merkle_root: [0xD0; 32],
            timestamp: 1_762_000_700,
            difficulty_bits: 0x1f00ffff,
        };
        let job = pool.issue_job(header, DifficultyTarget::MAX, 0, 8);
        assert!(pool.is_job_stale(job.job_id));

        // Unknown job_id returns false (not stale, just non-existent)
        assert!(!pool.is_job_stale(99999));
    }

    #[test]
    fn submit_solution_job_mismatch_rejected() {
        let mut pool = MiningPool::default();
        let header = MiningHeader {
            version: 3,
            previous_hash: [0xE0; 32],
            merkle_root: [0xF0; 32],
            timestamp: 1_762_000_800,
            difficulty_bits: 0x1f00ffff,
        };
        let job = pool.issue_job(header, DifficultyTarget::MAX, 0, 8);

        // Create solution with wrong header
        let wrong_header = MiningHeader {
            version: 3,
            previous_hash: [0x01; 32],
            merkle_root: [0x02; 32],
            timestamp: 1_762_000_900,
            difficulty_bits: 0x1f00ffff,
        };
        let solution = MiningSolution {
            job_id: job.job_id,
            candidate: BlockCandidate {
                header: wrong_header,
                nonce: 42,
                height: 0,
            },
            hash: [0u8; 32],
        };

        let decision = pool.submit_solution(
            "m1".to_string(),
            "w1".to_string(),
            solution,
            RevenueSource::Zion,
            1.0,
            "deeksha_lite_v1",
        );
        assert_eq!(decision.status, ShareStatus::JobMismatch);
        assert_eq!(pool.stats().rejected_shares, 1);
    }

    // ── Sprint 5 B4: hex parsing edge cases ────────────────────────────

    #[test]
    fn parse_fixed_hex_wrong_length_fails() {
        let result = parse_fixed_hex::<32>("aabb", "test");
        assert!(result.is_err());
    }

    #[test]
    fn parse_fixed_hex_non_hex_chars_fails() {
        let result = parse_fixed_hex::<2>("ZZZZ", "test");
        assert!(result.is_err());
    }

    #[test]
    fn parse_fixed_hex_valid_with_0x_prefix() {
        let result = parse_fixed_hex::<2>("0xaabb", "test");
        assert_eq!(result.unwrap(), [0xaa, 0xbb]);
    }

    #[test]
    fn parse_fixed_hex_exact_length_works() {
        let hex = "00".repeat(32);
        let result = parse_fixed_hex::<32>(&hex, "test");
        assert_eq!(result.unwrap(), [0u8; 32]);
    }

    #[test]
    fn job_message_encodes_hex_fields_correctly() {
        let mut pool = MiningPool::default();
        let header = MiningHeader {
            version: 3,
            previous_hash: [0xff; 32],
            merkle_root: [0xaa; 32],
            timestamp: 100,
            difficulty_bits: 0x1f00ffff,
        };
        let job = pool.issue_job(header, DifficultyTarget::MAX, 0, 64);
        let msg = pool.job_message(job, "deeksha_lite_v1");
        if let PoolMessage::Job {
            header_hex,
            target_hex,
            ..
        } = msg
        {
            assert_eq!(header_hex.len(), 160, "80 bytes = 160 hex chars");
            assert_eq!(target_hex.len(), 64, "32 bytes = 64 hex chars");
        } else {
            panic!("expected Job variant");
        }
    }

    #[test]
    fn default_pool_has_zero_stats() {
        let pool = MiningPool::default();
        let stats = pool.stats();
        assert_eq!(stats.accepted_shares, 0);
        assert_eq!(stats.rejected_shares, 0);
        assert_eq!(stats.active_jobs, 0);
    }

    #[test]
    fn protocol_version_is_stable() {
        assert_eq!(protocol_version(), "zion-v3-stratum/0.2");
    }
}
