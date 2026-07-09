//! Simple per-IP fixed-window rate limiter for OASIS API.
//!
//! Default: 30 requests per 60 seconds per IP.

use axum::{
    extract::{ConnectInfo, Request},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Fixed-window bucket for a single IP.
#[derive(Debug, Clone)]
struct Window {
    count: u32,
    reset_at: Instant,
}

/// Shared rate-limit state.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    max_requests: u32,
    window_secs: u64,
    state: Arc<Mutex<HashMap<String, Window>>>,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self {
            max_requests,
            window_secs,
            state: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if `ip` is allowed to proceed. Returns `true` if under limit.
    fn check(&self, ip: &str) -> bool {
        let mut map = self.state.lock().unwrap();
        let now = Instant::now();
        let window_dur = Duration::from_secs(self.window_secs);

        match map.get_mut(ip) {
            Some(window) => {
                if now > window.reset_at {
                    // Window expired, reset
                    window.count = 1;
                    window.reset_at = now + window_dur;
                    true
                } else if window.count < self.max_requests {
                    window.count += 1;
                    true
                } else {
                    false
                }
            }
            None => {
                map.insert(
                    ip.to_string(),
                    Window {
                        count: 1,
                        reset_at: now + window_dur,
                    },
                );
                true
            }
        }
    }
}

/// Axum middleware: reject requests over the per-IP limit.
/// Skips limiting if ConnectInfo is missing (e.g. unit tests via oneshot).
pub async fn rate_limit_middleware(
    addr: Option<ConnectInfo<SocketAddr>>,
    req: Request,
    next: Next,
) -> Response {
    let ip = addr.map(|a| a.0.ip().to_string());
    if let Some(ip) = ip {
        let limiter = req
            .extensions()
            .get::<RateLimiter>()
            .cloned()
            .unwrap_or_else(|| RateLimiter::new(30, 60));
        if !limiter.check(&ip) {
            return (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response();
        }
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_allows_under_cap() {
        let rl = RateLimiter::new(3, 60);
        assert!(rl.check("1.2.3.4"));
        assert!(rl.check("1.2.3.4"));
        assert!(rl.check("1.2.3.4"));
    }

    #[test]
    fn test_rate_limit_blocks_over_cap() {
        let rl = RateLimiter::new(2, 60);
        assert!(rl.check("1.2.3.4"));
        assert!(rl.check("1.2.3.4"));
        assert!(!rl.check("1.2.3.4"));
    }

    #[test]
    fn test_rate_limit_per_ip_isolation() {
        let rl = RateLimiter::new(1, 60);
        assert!(rl.check("1.2.3.4"));
        assert!(rl.check("5.6.7.8"));
    }
}
