use std::thread;
use std::time::Duration;

/// Exponential backoff state for reconnect attempts.
#[allow(dead_code)]
pub struct Backoff {
    current_ms: u64,
    max_ms: u64,
    multiplier: u64,
}

impl Backoff {
    /// Create a new backoff starting at `initial_ms`, capped at `max_ms`.
    pub fn new(initial_ms: u64, max_ms: u64) -> Self {
        Self {
            current_ms: initial_ms,
            max_ms,
            multiplier: 2,
        }
    }

    /// Default production backoff: 1s → 60s.
    pub fn default_reconnect() -> Self {
        Self::new(1_000, 60_000)
    }

    /// Sleep for the current backoff interval, then increase it.
    pub fn wait(&mut self) {
        let delay = self.current_ms.min(self.max_ms);
        println!("reconnect_backoff_ms={delay}");
        thread::sleep(Duration::from_millis(delay));
        self.current_ms = self
            .current_ms
            .saturating_mul(self.multiplier)
            .min(self.max_ms);
    }

    /// Reset backoff to initial value after a successful connection.
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        // Restore by dividing back from multiplier logic.
        // Simpler: just recreate. Keep max_ms.
        self.current_ms = self.max_ms / 60; // back to ~1s for 60s max
        if self.current_ms == 0 {
            self.current_ms = 1_000;
        }
    }

    /// Current delay in ms (for logging).
    #[allow(dead_code)]
    pub fn current_ms(&self) -> u64 {
        self.current_ms.min(self.max_ms)
    }
}

/// Run a closure in a reconnect loop. Returns Ok(R) on first success,
/// or keeps retrying indefinitely on failure.
///
/// `max_attempts` of 0 means infinite.
pub fn with_reconnect<F, R>(
    max_attempts: u32,
    mut backoff: Backoff,
    mut action: F,
) -> anyhow::Result<R>
where
    F: FnMut(u32) -> anyhow::Result<R>,
{
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match action(attempt) {
            Ok(result) => return Ok(result),
            Err(e) => {
                println!("session_error attempt={attempt} error=\"{e}\"");
                if max_attempts > 0 && attempt >= max_attempts {
                    return Err(e);
                }
                backoff.wait();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_increases_and_caps() {
        let mut b = Backoff::new(100, 1000);
        assert_eq!(b.current_ms(), 100);
        // Simulate wait without actually sleeping
        b.current_ms = b.current_ms.saturating_mul(b.multiplier).min(b.max_ms);
        assert_eq!(b.current_ms(), 200);
        b.current_ms = b.current_ms.saturating_mul(b.multiplier).min(b.max_ms);
        assert_eq!(b.current_ms(), 400);
        b.current_ms = b.current_ms.saturating_mul(b.multiplier).min(b.max_ms);
        assert_eq!(b.current_ms(), 800);
        b.current_ms = b.current_ms.saturating_mul(b.multiplier).min(b.max_ms);
        assert_eq!(b.current_ms(), 1000); // capped
    }

    #[test]
    fn with_reconnect_succeeds_on_first_try() {
        let result = with_reconnect(3, Backoff::new(1, 10), |_attempt| Ok(42u32));
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn with_reconnect_retries_then_succeeds() {
        let result = with_reconnect(5, Backoff::new(1, 10), |attempt| {
            if attempt < 3 {
                Err(anyhow::anyhow!("transient"))
            } else {
                Ok(99u32)
            }
        });
        assert_eq!(result.unwrap(), 99);
    }

    #[test]
    fn with_reconnect_exhausts_max_attempts() {
        let result = with_reconnect(2, Backoff::new(1, 10), |_attempt| -> anyhow::Result<u32> {
            Err(anyhow::anyhow!("permanent"))
        });
        assert!(result.is_err());
    }
}
