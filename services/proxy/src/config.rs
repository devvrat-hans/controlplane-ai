use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub listen_addr: String,
    pub upstream: UpstreamConfig,
    pub limits: LimitsConfig,
    pub timeouts: TimeoutConfig,
    pub rate_limit: RateLimitConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamConfig {
    pub base_url: String,
    pub tls_verify: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfig {
    pub max_request_body_bytes: usize,
    pub max_response_body_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    pub upstream_timeout_ms: u64,
    pub fast_path_budget_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub requests_per_minute: u32,
    pub burst_size: u32,
}

impl ProxyConfig {
    pub fn upstream_timeout(&self) -> Duration {
        Duration::from_millis(self.timeouts.upstream_timeout_ms)
    }

    pub fn fast_path_budget(&self) -> Duration {
        Duration::from_millis(self.timeouts.fast_path_budget_ms)
    }
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0:8900".to_string(),
            upstream: UpstreamConfig {
                base_url: "https://api.anthropic.com".to_string(),
                tls_verify: true,
            },
            limits: LimitsConfig {
                max_request_body_bytes: 10 * 1024 * 1024, // 10 MB
                max_response_body_bytes: 10 * 1024 * 1024,
            },
            timeouts: TimeoutConfig {
                upstream_timeout_ms: 30_000, // 30s
                fast_path_budget_ms: 25,     // 25ms hard budget
            },
            rate_limit: RateLimitConfig {
                enabled: true,
                requests_per_minute: 600,
                burst_size: 50,
            },
        }
    }
}

impl ProxyConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(addr) = std::env::var("PROXY_LISTEN_ADDR") {
            config.listen_addr = addr;
        }
        if let Ok(url) = std::env::var("UPSTREAM_BASE_URL") {
            config.upstream.base_url = url;
        }
        if let Ok(timeout) = std::env::var("UPSTREAM_TIMEOUT_MS") {
            if let Ok(ms) = timeout.parse() {
                config.timeouts.upstream_timeout_ms = ms;
            }
        }
        if let Ok(budget) = std::env::var("FAST_PATH_BUDGET_MS") {
            if let Ok(ms) = budget.parse() {
                config.timeouts.fast_path_budget_ms = ms;
            }
        }
        if let Ok(max) = std::env::var("MAX_REQUEST_BODY_BYTES") {
            if let Ok(bytes) = max.parse() {
                config.limits.max_request_body_bytes = bytes;
            }
        }
        if let Ok(rpm) = std::env::var("RATE_LIMIT_RPM") {
            if let Ok(val) = rpm.parse() {
                config.rate_limit.requests_per_minute = val;
            }
        }
        if std::env::var("RATE_LIMIT_DISABLED").is_ok() {
            config.rate_limit.enabled = false;
        }

        config
    }
}
