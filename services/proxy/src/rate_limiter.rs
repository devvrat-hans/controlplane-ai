use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;

use crate::config::RateLimitConfig;

/// Simple sliding-window rate limiter per key (app_id or IP).
#[derive(Clone)]
pub struct RateLimiter {
    config: RateLimitConfig,
    state: Arc<Mutex<HashMap<String, WindowState>>>,
}

struct WindowState {
    tokens: u32,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Returns true if the request is allowed, false if rate-limited.
    pub async fn check(&self, key: &str) -> bool {
        if !self.config.enabled {
            return true;
        }

        let mut state = self.state.lock().await;
        let now = Instant::now();

        let window = state.entry(key.to_string()).or_insert_with(|| WindowState {
            tokens: self.config.burst_size,
            last_refill: now,
        });

        // Refill tokens based on time elapsed
        let elapsed_ms = now.duration_since(window.last_refill).as_millis() as u64;
        let refill_rate_ms = 60_000 / self.config.requests_per_minute as u64;
        let new_tokens = (elapsed_ms / refill_rate_ms) as u32;

        if new_tokens > 0 {
            window.tokens = (window.tokens + new_tokens).min(self.config.burst_size);
            window.last_refill = now;
        }

        if window.tokens > 0 {
            window.tokens -= 1;
            true
        } else {
            false
        }
    }

    /// Remove stale entries (call periodically to prevent unbounded memory growth).
    pub async fn cleanup(&self, max_idle_seconds: u64) {
        let mut state = self.state.lock().await;
        let now = Instant::now();
        state.retain(|_, v| {
            now.duration_since(v.last_refill).as_secs() < max_idle_seconds
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allows_within_burst() {
        let limiter = RateLimiter::new(RateLimitConfig {
            enabled: true,
            requests_per_minute: 60,
            burst_size: 5,
        });

        for _ in 0..5 {
            assert!(limiter.check("app1").await);
        }
        // 6th request should be rate-limited
        assert!(!limiter.check("app1").await);
    }

    #[tokio::test]
    async fn disabled_allows_all() {
        let limiter = RateLimiter::new(RateLimitConfig {
            enabled: false,
            requests_per_minute: 1,
            burst_size: 1,
        });

        for _ in 0..100 {
            assert!(limiter.check("app1").await);
        }
    }

    #[tokio::test]
    async fn separate_keys_have_separate_limits() {
        let limiter = RateLimiter::new(RateLimitConfig {
            enabled: true,
            requests_per_minute: 60,
            burst_size: 2,
        });

        assert!(limiter.check("app1").await);
        assert!(limiter.check("app1").await);
        assert!(!limiter.check("app1").await);

        // app2 has its own bucket
        assert!(limiter.check("app2").await);
        assert!(limiter.check("app2").await);
        assert!(!limiter.check("app2").await);
    }
}
