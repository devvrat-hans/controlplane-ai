//! Per-principal token-bucket rate limiting.
//!
//! The dashboard BFF has no rate limiting and the proxy's limiter is still a
//! scaffold, so the MCP server owns this concern for its own surface. The
//! bucket is keyed on the authenticated subject, falling back to `anonymous`.

use std::collections::HashMap;
use std::time::Instant;

use tokio::sync::Mutex;

use crate::error::McpError;

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

pub struct RateLimiter {
    per_min: f64,
    burst: f64,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    pub fn new(per_min: u32, burst: u32) -> Self {
        let per_min = per_min.max(1) as f64;
        let burst = burst.max(1) as f64;
        Self {
            per_min,
            burst,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Consume one token for `key`, returning a sanitized error when exhausted.
    pub async fn check(&self, key: &str) -> Result<(), McpError> {
        let refill_per_sec = self.per_min / 60.0;
        let mut buckets = self.buckets.lock().await;
        let now = Instant::now();

        let bucket = buckets.entry(key.to_string()).or_insert_with(|| Bucket {
            tokens: self.burst,
            last_refill: now,
        });

        let elapsed = now
            .saturating_duration_since(bucket.last_refill)
            .as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * refill_per_sec).min(self.burst);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let deficit = 1.0 - bucket.tokens;
            let retry_after_ms = (deficit / refill_per_sec * 1000.0).ceil() as u64;
            Err(McpError::rate_limited(retry_after_ms.max(1)))
        }
    }

    /// Bound memory growth: drop buckets that have fully refilled.
    pub async fn prune(&self) {
        let mut buckets = self.buckets.lock().await;
        let now = Instant::now();
        let refill_per_sec = self.per_min / 60.0;
        buckets.retain(|_, b| {
            let elapsed = now.saturating_duration_since(b.last_refill).as_secs_f64();
            (b.tokens + elapsed * refill_per_sec) < self.burst
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn burst_then_denied() {
        // 1/min refills a token every 60s, so the assertion cannot be undone by
        // scheduling delay on a loaded machine.
        let limiter = RateLimiter::new(1, 3);
        for _ in 0..3 {
            limiter.check("a").await.unwrap();
        }
        let err = limiter.check("a").await.unwrap_err();
        assert_eq!(err.code, crate::error::codes::RATE_LIMITED);
    }

    #[tokio::test]
    async fn keys_are_isolated() {
        let limiter = RateLimiter::new(1, 1);
        limiter.check("a").await.unwrap();
        assert!(limiter.check("b").await.is_ok());
    }

    #[tokio::test]
    async fn prune_keeps_depleted_and_drops_refilled_buckets() {
        // 60_000/min == 1_000 tokens/sec, so a few milliseconds fully refills it.
        let limiter = RateLimiter::new(60_000, 1);
        limiter.check("a").await.unwrap();
        limiter.prune().await;
        assert_eq!(
            limiter.buckets.lock().await.len(),
            1,
            "depleted bucket kept"
        );

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        limiter.prune().await;
        assert!(
            limiter.buckets.lock().await.is_empty(),
            "refilled bucket pruned"
        );
    }
}
