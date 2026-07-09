// Phase 6d — P2P security: rate limiting, blacklisting, escalating bans
//
// Constitutional reference (audit P1-10):
//   Escalating ban durations: 300s → 1800s → 7200s
//   Per-IP connection rate limiting
//   Global max connections

use std::collections::HashMap;
use std::net::IpAddr;

/// Default global connection limit.
pub const MAX_CONNECTIONS: usize = 128;

/// Maximum message rate per peer (messages per window).
pub const MAX_MESSAGES_PER_WINDOW: u32 = 100;

/// Rate-limit window in seconds.
pub const RATE_LIMIT_WINDOW_SECS: u64 = 60;

/// Escalating ban durations in seconds.
pub const BAN_DURATIONS: [u64; 3] = [300, 1800, 7200];

/// Maximum number of bans before permanent ban.
pub const MAX_BAN_STRIKES: u32 = BAN_DURATIONS.len() as u32;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Reasons a peer can be punished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BanReason {
    /// Exceeded message rate limit.
    RateLimitExceeded,
    /// Sent an invalid block.
    InvalidBlock,
    /// Sent an invalid transaction.
    InvalidTransaction,
    /// Protocol violation (unexpected message, malformed data).
    ProtocolViolation,
    /// Manual ban by operator.
    Manual,
}

/// State for a single tracked peer IP.
#[derive(Debug, Clone)]
struct PeerRecord {
    /// Number of messages in the current window.
    message_count: u32,
    /// Start of the current rate-limit window (epoch seconds).
    window_start: u64,
    /// Number of times this peer has been banned.
    ban_strikes: u32,
    /// If banned, the epoch second when the ban expires (None = not banned or permanent).
    ban_until: Option<u64>,
    /// Whether this peer is permanently banned.
    permanent: bool,
}

/// P2P security guard — rate limiter + blacklist + connection limiter.
#[derive(Debug)]
pub struct PeerSecurity {
    peers: HashMap<IpAddr, PeerRecord>,
    max_connections: usize,
    active_connections: usize,
}

impl PeerSecurity {
    pub fn new() -> Self {
        Self {
            peers: HashMap::new(),
            max_connections: MAX_CONNECTIONS,
            active_connections: 0,
        }
    }

    pub fn with_max_connections(max: usize) -> Self {
        Self {
            peers: HashMap::new(),
            max_connections: max,
            active_connections: 0,
        }
    }

    /// Check whether a peer is currently banned. Pass the current epoch seconds.
    pub fn is_banned(&self, ip: &IpAddr, now: u64) -> bool {
        if let Some(record) = self.peers.get(ip) {
            if record.permanent {
                return true;
            }
            if let Some(until) = record.ban_until {
                return now < until;
            }
        }
        false
    }

    /// Record a message from a peer. Returns `Err(BanReason)` if the peer
    /// exceeded the rate limit and was banned.
    pub fn record_message(&mut self, ip: IpAddr, now: u64) -> Result<(), BanReason> {
        let record = self.peers.entry(ip).or_insert_with(|| PeerRecord {
            message_count: 0,
            window_start: now,
            ban_strikes: 0,
            ban_until: None,
            permanent: false,
        });

        // Check existing ban.
        if record.permanent || record.ban_until.is_some_and(|u| now < u) {
            return Err(BanReason::RateLimitExceeded);
        }

        // Reset window if expired.
        if now >= record.window_start + RATE_LIMIT_WINDOW_SECS {
            record.message_count = 0;
            record.window_start = now;
        }

        record.message_count += 1;

        if record.message_count > MAX_MESSAGES_PER_WINDOW {
            self.apply_ban(ip, now, BanReason::RateLimitExceeded);
            return Err(BanReason::RateLimitExceeded);
        }

        Ok(())
    }

    /// Punish a peer for misbehavior with an escalating ban.
    pub fn punish(&mut self, ip: IpAddr, now: u64, reason: BanReason) {
        self.apply_ban(ip, now, reason);
    }

    /// Permanently ban a peer.
    pub fn ban_permanent(&mut self, ip: IpAddr) {
        let record = self.peers.entry(ip).or_insert_with(|| PeerRecord {
            message_count: 0,
            window_start: 0,
            ban_strikes: 0,
            ban_until: None,
            permanent: false,
        });
        record.permanent = true;
    }

    /// Unban a peer (remove all ban state).
    pub fn unban(&mut self, ip: &IpAddr) {
        if let Some(record) = self.peers.get_mut(ip) {
            record.ban_until = None;
            record.permanent = false;
            record.ban_strikes = 0;
        }
    }

    /// Try to accept a new inbound connection. Returns false if at limit.
    pub fn try_accept_connection(&mut self, ip: &IpAddr, now: u64) -> bool {
        if self.is_banned(ip, now) {
            return false;
        }
        if self.active_connections >= self.max_connections {
            return false;
        }
        self.active_connections += 1;
        true
    }

    /// Release a connection slot.
    pub fn release_connection(&mut self) {
        self.active_connections = self.active_connections.saturating_sub(1);
    }

    /// Current active connection count.
    pub fn active_connections(&self) -> usize {
        self.active_connections
    }

    /// Number of currently banned IPs (including expired bans not yet cleaned).
    pub fn banned_count(&self, now: u64) -> usize {
        self.peers
            .values()
            .filter(|r| r.permanent || r.ban_until.is_some_and(|u| now < u))
            .count()
    }

    /// Clean up expired bans and stale records.
    pub fn cleanup(&mut self, now: u64) {
        self.peers.retain(|_, r| {
            if r.permanent {
                return true;
            }
            if let Some(until) = r.ban_until {
                if now >= until {
                    // Ban expired; keep record if it still has strikes (for escalation).
                    r.ban_until = None;
                    return r.ban_strikes > 0;
                }
                return true;
            }
            // Keep if active in this window.
            now < r.window_start + RATE_LIMIT_WINDOW_SECS * 2
        });
    }

    // -- internal --

    fn apply_ban(&mut self, ip: IpAddr, now: u64, _reason: BanReason) {
        let record = self.peers.entry(ip).or_insert_with(|| PeerRecord {
            message_count: 0,
            window_start: now,
            ban_strikes: 0,
            ban_until: None,
            permanent: false,
        });
        let strike = record.ban_strikes as usize;
        if strike >= BAN_DURATIONS.len() {
            // Max strikes exceeded → permanent ban.
            record.permanent = true;
        } else {
            let duration = BAN_DURATIONS[strike];
            record.ban_until = Some(now + duration);
        }
        record.ban_strikes += 1;
    }
}

impl Default for PeerSecurity {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip(a: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, a))
    }

    #[test]
    fn test_not_banned_initially() {
        let sec = PeerSecurity::new();
        assert!(!sec.is_banned(&ip(1), 1000));
    }

    #[test]
    fn test_rate_limit_escalation() {
        let mut sec = PeerSecurity::new();
        let now = 1000;
        // Send MAX_MESSAGES_PER_WINDOW messages — all OK.
        for _ in 0..MAX_MESSAGES_PER_WINDOW {
            assert!(sec.record_message(ip(1), now).is_ok());
        }
        // One more → banned.
        assert!(sec.record_message(ip(1), now).is_err());
        assert!(sec.is_banned(&ip(1), now));
        // First ban = 300s.
        assert!(!sec.is_banned(&ip(1), now + 301));
    }

    #[test]
    fn test_escalating_ban_durations() {
        let mut sec = PeerSecurity::new();
        let now = 1000;
        // Strike 1: 300s.
        sec.punish(ip(1), now, BanReason::InvalidBlock);
        assert!(sec.is_banned(&ip(1), now));
        assert!(!sec.is_banned(&ip(1), now + 301));
        // Strike 2: 1800s.
        sec.punish(ip(1), now + 301, BanReason::InvalidBlock);
        assert!(sec.is_banned(&ip(1), now + 301));
        assert!(!sec.is_banned(&ip(1), now + 301 + 1801));
        // Strike 3: 7200s.
        sec.punish(ip(1), now + 2200, BanReason::InvalidBlock);
        assert!(sec.is_banned(&ip(1), now + 2200));
        assert!(!sec.is_banned(&ip(1), now + 2200 + 7201));
        // Strike 4: permanent.
        sec.punish(ip(1), now + 10000, BanReason::InvalidBlock);
        assert!(sec.is_banned(&ip(1), now + 10000));
        assert!(sec.is_banned(&ip(1), now + 999_999)); // permanent
    }

    #[test]
    fn test_permanent_ban() {
        let mut sec = PeerSecurity::new();
        sec.ban_permanent(ip(1));
        assert!(sec.is_banned(&ip(1), 0));
        assert!(sec.is_banned(&ip(1), u64::MAX));
    }

    #[test]
    fn test_unban() {
        let mut sec = PeerSecurity::new();
        sec.ban_permanent(ip(1));
        assert!(sec.is_banned(&ip(1), 1000));
        sec.unban(&ip(1));
        assert!(!sec.is_banned(&ip(1), 1000));
    }

    #[test]
    fn test_connection_limiter() {
        let mut sec = PeerSecurity::with_max_connections(2);
        assert!(sec.try_accept_connection(&ip(1), 0));
        assert!(sec.try_accept_connection(&ip(2), 0));
        assert!(!sec.try_accept_connection(&ip(3), 0)); // at limit
        sec.release_connection();
        assert!(sec.try_accept_connection(&ip(3), 0)); // slot freed
    }

    #[test]
    fn test_banned_peer_rejected_on_connect() {
        let mut sec = PeerSecurity::new();
        sec.ban_permanent(ip(1));
        assert!(!sec.try_accept_connection(&ip(1), 0));
    }

    #[test]
    fn test_cleanup_expired_bans() {
        let mut sec = PeerSecurity::new();
        sec.punish(ip(1), 1000, BanReason::InvalidBlock);
        assert_eq!(sec.banned_count(1000), 1);
        sec.cleanup(1000 + 301);
        assert_eq!(sec.banned_count(1000 + 301), 0);
    }

    #[test]
    fn test_rate_limit_window_resets() {
        let mut sec = PeerSecurity::new();
        let now = 1000;
        for _ in 0..MAX_MESSAGES_PER_WINDOW {
            sec.record_message(ip(1), now).unwrap();
        }
        // New window.
        let later = now + RATE_LIMIT_WINDOW_SECS + 1;
        assert!(sec.record_message(ip(1), later).is_ok());
    }

    #[test]
    fn test_constants() {
        assert_eq!(BAN_DURATIONS, [300, 1800, 7200]);
        assert_eq!(MAX_CONNECTIONS, 128);
        assert_eq!(MAX_MESSAGES_PER_WINDOW, 100);
    }
}
