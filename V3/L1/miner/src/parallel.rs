use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use zion_core::{BlockCandidate, MiningJob, MiningSolution};
use zion_cosmic_harmony::{cosmic_harmony_with_height, deeksha_lite, deeksha_lite_fire};

/// Hash function selector for dual-algo support.
pub fn hash_candidate(candidate: &BlockCandidate, algorithm: &str) -> [u8; 32] {
    match algorithm {
        "deeksha_lite_v1" => {
            deeksha_lite::deeksha_lite(&candidate.header.to_bytes(), candidate.nonce)
        }
        "deeksha_lite_fire" => {
            deeksha_lite_fire::deeksha_lite_fire(&candidate.header.to_bytes(), candidate.nonce)
        }
        _ => {
            cosmic_harmony_with_height(
                &candidate.header.to_bytes(),
                candidate.nonce,
                candidate.height,
            )
            .data
        }
    }
}

/// Multi-threaded nonce scan using rayon thread pool.
///
/// Divides `job.nonce_count` into `threads` equal chunks, scans each in
/// parallel, and returns the first solution found (cancelling others via
/// an `AtomicBool` flag).
pub fn parallel_scan_nonce_range(
    job: MiningJob,
    threads: usize,
    algorithm: &str,
) -> Option<MiningSolution> {
    let threads = threads.max(1);
    if threads == 1 {
        return sequential_scan(job, &AtomicBool::new(false), algorithm);
    }

    let chunk_size = job.nonce_count / threads as u64;
    if chunk_size == 0 {
        return sequential_scan(job, &AtomicBool::new(false), algorithm);
    }

    let cancelled = Arc::new(AtomicBool::new(false));

    let result: Option<MiningSolution> = (0..threads).into_par_iter().find_map_any(|thread_idx| {
        let start = job.start_nonce.wrapping_add(thread_idx as u64 * chunk_size);
        let count = if thread_idx == threads - 1 {
            // Last thread gets the remainder
            job.nonce_count - (thread_idx as u64 * chunk_size)
        } else {
            chunk_size
        };

        let sub_job = MiningJob {
            job_id: job.job_id,
            header: job.header,
            target: job.target,
            start_nonce: start,
            nonce_count: count,
            height: job.height,
        };

        let sol = sequential_scan(sub_job, &cancelled, algorithm);
        if sol.is_some() {
            cancelled.store(true, Ordering::Relaxed);
        }
        sol
    });

    result
}

/// Sequential single-thread scan respecting a cancellation flag.
fn sequential_scan(
    job: MiningJob,
    cancelled: &AtomicBool,
    algorithm: &str,
) -> Option<MiningSolution> {
    for offset in 0..job.nonce_count {
        if offset % 4096 == 0 && cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let nonce = job.start_nonce.wrapping_add(offset);
        let candidate = BlockCandidate {
            header: job.header,
            nonce,
            height: job.height,
        };
        let hash = hash_candidate(&candidate, algorithm);
        if job.target.allows(&hash) {
            return Some(MiningSolution {
                job_id: job.job_id,
                candidate,
                hash,
            });
        }
    }
    None
}

/// Detect optimal thread count from env or CPU cores.
pub fn detect_threads() -> usize {
    match std::env::var("ZION_THREADS") {
        Ok(v) => v.parse::<usize>().unwrap_or(1).max(1),
        Err(_) => num_cpus::get().max(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zion_core::{DifficultyTarget, MiningHeader};

    fn test_header() -> MiningHeader {
        MiningHeader {
            version: 3,
            previous_hash: [0x11; 32],
            merkle_root: [0x22; 32],
            timestamp: 1_762_000_200,
            difficulty_bits: 0x1f00ffff,
        }
    }

    #[test]
    fn parallel_scan_finds_same_as_sequential() {
        let job = MiningJob {
            job_id: 1,
            header: test_header(),
            target: DifficultyTarget::MAX,
            start_nonce: 0,
            nonce_count: 100,
            height: 0,
        };

        let seq = sequential_scan(
            job,
            &AtomicBool::new(false),
            "cosmic_harmony_ekam_deeksha_v2",
        );
        let par = parallel_scan_nonce_range(job, 4, "cosmic_harmony_ekam_deeksha_v2");

        assert!(seq.is_some());
        assert!(par.is_some());
        // Both must find nonce 0 (MAX target accepts any hash)
        assert_eq!(seq.unwrap().candidate.nonce, 0);
        assert_eq!(par.unwrap().candidate.nonce, 0);
    }

    #[test]
    fn parallel_scan_with_one_thread_equals_sequential() {
        let job = MiningJob {
            job_id: 1,
            header: test_header(),
            target: DifficultyTarget::MAX,
            start_nonce: 42,
            nonce_count: 10,
            height: 0,
        };

        let seq = sequential_scan(
            job,
            &AtomicBool::new(false),
            "cosmic_harmony_ekam_deeksha_v2",
        );
        let par = parallel_scan_nonce_range(job, 1, "cosmic_harmony_ekam_deeksha_v2");

        assert_eq!(seq.unwrap().candidate.nonce, par.unwrap().candidate.nonce,);
    }

    #[test]
    fn parallel_scan_respects_cancellation() {
        let cancelled = AtomicBool::new(true);
        let job = MiningJob {
            job_id: 1,
            header: test_header(),
            target: DifficultyTarget::MAX,
            start_nonce: 0,
            nonce_count: 1_000_000,
            height: 0,
        };

        // With cancelled=true up front, should return None quickly
        let result = sequential_scan(job, &cancelled, "cosmic_harmony_ekam_deeksha_v2");
        assert!(result.is_none());
    }

    #[test]
    fn deeksha_lite_produces_different_hashes() {
        let header = test_header();
        let nonce = 42u64;

        let hash_ekam =
            zion_cosmic_harmony::cosmic_harmony_ekam_deeksha_v3(&header.to_bytes(), nonce, 0).data;
        let hash_lite = deeksha_lite::deeksha_lite(&header.to_bytes(), nonce);

        assert_ne!(
            hash_ekam, hash_lite,
            "DeekshaLite must produce different hashes than ekam_v3"
        );
    }
}
