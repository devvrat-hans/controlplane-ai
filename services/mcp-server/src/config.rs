//! Environment-driven configuration.
//!
//! Configuration is read once at startup. All limits have conservative
//! defaults so the server is safe to run with an empty environment.

use std::time::Duration;

use crate::error::McpError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    Stdio,
    Http,
}

#[derive(Debug, Clone)]
pub struct McpConfig {
    pub transport: Transport,
    /// Base URL of the dashboard BFF (`controlplane-dashboard-api`).
    pub dashboard_url: String,
    /// Base URL of the governance proxy (`controlplane-proxy`).
    pub proxy_url: String,
    /// Optional upstream credential forwarded to the proxy for `evaluate_prompt`.
    pub proxy_api_key: Option<String>,
    /// Base URL of the internal guardrails sidecar. Only used when
    /// `enable_internal_scans` is true.
    pub guardrails_url: Option<String>,
    /// Opt-in switch for the curated scanner adapter. **Off by default** because
    /// the guardrails sidecar is internal-only infrastructure.
    pub enable_internal_scans: bool,
    /// `token:role[:app_id|app_id]` entries, comma separated.
    pub auth_tokens: String,
    /// When true and no credentials are configured, requests are treated as
    /// anonymous `viewer` principals. Defaults to false (fail closed).
    pub allow_anonymous: bool,
    /// Role assumed for the stdio transport when `MCP_TOKEN` is not set.
    /// Empty means "unset" — the stdio transport then refuses to start rather
    /// than serving unauthenticated read access.
    pub stdio_role: String,
    /// Bind address for the Streamable HTTP transport.
    pub http_addr: String,
    pub request_timeout: Duration,
    pub max_payload_bytes: usize,
    pub max_response_bytes: usize,
    pub rate_limit_per_min: u32,
    pub rate_limit_burst: u32,
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().eq_ignore_ascii_case("true") || v.trim() == "1")
        .unwrap_or(default)
}

impl McpConfig {
    pub fn from_env() -> Result<Self, McpError> {
        let transport = match std::env::var("MCP_TRANSPORT")
            .unwrap_or_else(|_| "stdio".into())
            .trim()
            .to_lowercase()
            .as_str()
        {
            "stdio" => Transport::Stdio,
            "http" | "streamable-http" => Transport::Http,
            other => {
                return Err(McpError::invalid_request(format!(
                    "Invalid MCP_TRANSPORT: {other}"
                )))
            }
        };

        Ok(Self {
            transport,
            dashboard_url: std::env::var("CONTROLPLANE_API_URL")
                .unwrap_or_else(|_| "http://localhost:8080".into())
                .trim_end_matches('/')
                .to_string(),
            proxy_url: std::env::var("CONTROLPLANE_PROXY_URL")
                .unwrap_or_else(|_| "http://localhost:8900".into())
                .trim_end_matches('/')
                .to_string(),
            proxy_api_key: std::env::var("CONTROLPLANE_PROXY_API_KEY")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            guardrails_url: std::env::var("CONTROLPLANE_GUARDRAILS_URL")
                .ok()
                .map(|v| v.trim().trim_end_matches('/').to_string())
                .filter(|v| !v.is_empty()),
            enable_internal_scans: env_bool("MCP_ENABLE_INTERNAL_SCANS", false),
            auth_tokens: std::env::var("MCP_AUTH_TOKENS").unwrap_or_default(),
            allow_anonymous: env_bool("MCP_ALLOW_ANONYMOUS", false),
            stdio_role: std::env::var("MCP_ROLE")
                .unwrap_or_default()
                .trim()
                .to_lowercase(),
            http_addr: std::env::var("MCP_HTTP_ADDR").unwrap_or_else(|_| "127.0.0.1:8090".into()),
            request_timeout: Duration::from_millis(
                env_usize("MCP_REQUEST_TIMEOUT_MS", 15_000) as u64
            ),
            max_payload_bytes: env_usize("MCP_MAX_PAYLOAD_BYTES", 256 * 1024),
            max_response_bytes: env_usize("MCP_MAX_RESPONSE_BYTES", 2 * 1024 * 1024),
            rate_limit_per_min: env_u32("MCP_RATE_LIMIT_PER_MIN", 120),
            rate_limit_burst: env_u32("MCP_RATE_LIMIT_BURST", 30),
        })
    }
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            transport: Transport::Stdio,
            dashboard_url: "http://localhost:8080".into(),
            proxy_url: "http://localhost:8900".into(),
            proxy_api_key: None,
            guardrails_url: None,
            enable_internal_scans: false,
            auth_tokens: String::new(),
            allow_anonymous: false,
            stdio_role: String::new(),
            http_addr: "127.0.0.1:8090".into(),
            request_timeout: Duration::from_millis(15_000),
            max_payload_bytes: 256 * 1024,
            max_response_bytes: 2 * 1024 * 1024,
            rate_limit_per_min: 120,
            rate_limit_burst: 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// Environment variables are process-wide, so tests that mutate them must
    /// not run concurrently with each other.
    fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn defaults_are_fail_closed() {
        let cfg = McpConfig::default();
        assert!(!cfg.allow_anonymous);
        assert_eq!(cfg.transport, Transport::Stdio);
    }

    #[test]
    fn env_overrides_are_applied() {
        let _guard = env_lock();
        std::env::set_var("MCP_TRANSPORT", "http");
        std::env::set_var("MCP_ALLOW_ANONYMOUS", "true");
        std::env::set_var("MCP_RATE_LIMIT_PER_MIN", "7");
        let cfg = McpConfig::from_env().unwrap();
        assert_eq!(cfg.transport, Transport::Http);
        assert!(cfg.allow_anonymous);
        assert_eq!(cfg.rate_limit_per_min, 7);
        std::env::remove_var("MCP_TRANSPORT");
        std::env::remove_var("MCP_ALLOW_ANONYMOUS");
        std::env::remove_var("MCP_RATE_LIMIT_PER_MIN");
    }

    #[test]
    fn invalid_transport_is_rejected() {
        let _guard = env_lock();
        std::env::set_var("MCP_TRANSPORT", "carrier-pigeon");
        let result = McpConfig::from_env();
        std::env::remove_var("MCP_TRANSPORT");
        assert!(result.is_err());
    }
}
