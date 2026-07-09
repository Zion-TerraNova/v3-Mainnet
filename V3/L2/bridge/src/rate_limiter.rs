//! Rate limiter for bridge operations.
//!
//! Enforces `security.max_ops_per_hour` from [`BridgeConfig`] as a sliding-window
//! counter. Also provides per-address throttling to prevent a single actor from
//! consuming the full hourly quota.
//!
//! Thread-safe: all state is behind a [`std::sync::Mutex`] so the relayer can
//! call `check_*` from any Tokio task.

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

/// Sliding-window rate limiter.
///
/// Keeps a bounded history of operation timestamps and rejects new ops when
/// the window limit is reached.
#[derive(Debug)]
pub struct RateLimiter {
    /// Maximum global operations per window.
    max_ops: u32,
    /// Maximum operations per single address per window.
    max_per_address: u32,
    /// Window duration.
    window: Duration,
    /// Internal mutable state.
    inner: Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {
    /// Global timestamps (oldest first).
    global: VecDeque<SystemTime>,
    /// Per-address timestamps (key = lowercase address string).
    per_address: std::collections::HashMap<String, VecDeque<SystemTime>>,
}

/// Rate-limit check result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitResult {
    /// Operation is allowed.
    Allowed,
    /// Rejected: global hourly limit reached.
    GlobalLimitReached { current: u32, max: u32 },
    /// Rejected: per-address limit reached.
    AddressLimitReached {
        address: String,
        current: u32,
        max: u32,
    },
}

impl RateLimiter {
    /// Acquire the inner guard, recovering from poisoning instead of
    /// panicking. The rate-limit state is bookkeeping (sliding-window
    /// counters of `Instant`s); recovering the inner state keeps the
    /// bridge daemon alive through a single panic rather than amplifying
    /// it into a DOS on every subsequent relayer call. (Audit finding F5.)
    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| {
            eprintln!(
                "warning: rate limiter mutex was poisoned by a panicking holder; \
                 recovering inner state"
            );
            poisoned.into_inner()
        })
    }

    /// Create a new rate limiter.
    ///
    /// - `max_ops_per_hour`: global cap from `security.max_ops_per_hour`
    /// - Per-address cap defaults to `max_ops_per_hour / 4` (min 5).
    pub fn new(max_ops_per_hour: u32) -> Self {
        let max_per_address = (max_ops_per_hour / 4).max(5);
        Self {
            max_ops: max_ops_per_hour,
            max_per_address,
            window: Duration::from_secs(3600),
            inner: Mutex::new(Inner {
                global: VecDeque::new(),
                per_address: std::collections::HashMap::new(),
            }),
        }
    }

    /// Check and, if allowed, record an operation for the given address.
    ///
    /// Returns `RateLimitResult::Allowed` on success and records the
    /// timestamp, or one of the rejection variants.
    pub fn check_and_record(&self, address: &str) -> RateLimitResult {
        let now = SystemTime::now();
        let cutoff = now - self.window;
        let addr_key = address.to_ascii_lowercase();

        let mut inner = self.inner();

        // Prune expired entries — global
        while inner.global.front().is_some_and(|&t| t < cutoff) {
            inner.global.pop_front();
        }

        // Check global limit
        if inner.global.len() as u32 >= self.max_ops {
            return RateLimitResult::GlobalLimitReached {
                current: inner.global.len() as u32,
                max: self.max_ops,
            };
        }

        // Prune and check per-address limit
        {
            let addr_queue = inner.per_address.entry(addr_key.clone()).or_default();
            while addr_queue.front().is_some_and(|&t| t < cutoff) {
                addr_queue.pop_front();
            }
            if addr_queue.len() as u32 >= self.max_per_address {
                return RateLimitResult::AddressLimitReached {
                    address: addr_key,
                    current: addr_queue.len() as u32,
                    max: self.max_per_address,
                };
            }
        }

        // Record — both borrows are now independent
        inner.global.push_back(now);
        inner
            .per_address
            .entry(addr_key)
            .or_default()
            .push_back(now);

        RateLimitResult::Allowed
    }

    /// Peek at current global count without recording.
    pub fn current_count(&self) -> u32 {
        let now = SystemTime::now();
        let cutoff = now - self.window;
        let mut inner = self.inner();
        while inner.global.front().is_some_and(|&t| t < cutoff) {
            inner.global.pop_front();
        }
        inner.global.len() as u32
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_allow() {
        let rl = RateLimiter::new(10);
        assert_eq!(
            rl.check_and_record("0xabcdef1234567890abcdef1234567890abcdef12"),
            RateLimitResult::Allowed,
        );
        assert_eq!(rl.current_count(), 1);
    }

    #[test]
    fn test_global_limit() {
        let rl = RateLimiter::new(3);
        for i in 0..3 {
            let addr = format!("0xaddr{}", i);
            assert_eq!(rl.check_and_record(&addr), RateLimitResult::Allowed);
        }
        // 4th should be rejected
        match rl.check_and_record("0xaddr_new") {
            RateLimitResult::GlobalLimitReached { current: 3, max: 3 } => {}
            other => panic!("Expected GlobalLimitReached, got {:?}", other),
        }
    }

    #[test]
    fn test_per_address_limit() {
        // max_ops=100, per_address = 100/4 = 25
        let rl = RateLimiter::new(100);
        let addr = "0xSingleUser";
        for _ in 0..25 {
            assert_eq!(rl.check_and_record(addr), RateLimitResult::Allowed);
        }
        // 26th from same address should be rejected
        match rl.check_and_record(addr) {
            RateLimitResult::AddressLimitReached {
                current: 25,
                max: 25,
                ..
            } => {}
            other => panic!("Expected AddressLimitReached, got {:?}", other),
        }
        // Different address should still work
        assert_eq!(rl.check_and_record("0xOtherUser"), RateLimitResult::Allowed);
    }

    #[test]
    fn test_case_insensitive() {
        let rl = RateLimiter::new(100);
        rl.check_and_record("0xABCDEF");
        assert_eq!(rl.current_count(), 1);
        // Same address, different case, should count as same
        rl.check_and_record("0xabcdef");
        assert_eq!(rl.current_count(), 2);
    }
}
