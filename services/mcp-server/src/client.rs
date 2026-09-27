//! HTTP client for the existing ControlPlane surfaces.
//!
//! The MCP server does not re-implement governance logic. It calls the same
//! dashboard BFF and proxy endpoints the dashboard uses. This module adds the
//! cross-cutting concerns the raw endpoints lack: correlation-id propagation,
//! hard timeouts, request/response size limits, and sanitized errors.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Method};
use serde_json::Value;

use crate::error::McpError;
use crate::redact;

const CORRELATION_HEADER: &str = "x-controlplane-correlation-id";
const REQUEST_ID_HEADER: &str = "x-request-id";

#[derive(Debug, Clone)]
pub struct ControlPlaneClient {
    http: Client,
    dashboard_url: String,
    proxy_url: String,
    proxy_api_key: Option<String>,
    max_response_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct UpstreamResponse {
    pub status: u16,
    pub body: Value,
    /// Governed fast-path latency, when the proxy reports it.
    pub governance_latency_ms: Option<i64>,
    /// Correlation id echoed by the proxy, when present.
    pub upstream_correlation_id: Option<String>,
}

impl ControlPlaneClient {
    pub fn new(
        dashboard_url: impl Into<String>,
        proxy_url: impl Into<String>,
        proxy_api_key: Option<String>,
        timeout: Duration,
        max_response_bytes: usize,
    ) -> Result<Self, McpError> {
        let http = Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_millis(3_000))
            .user_agent(concat!("controlplane-mcp/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| McpError::internal())?;

        Ok(Self {
            http,
            dashboard_url: dashboard_url.into().trim_end_matches('/').to_string(),
            proxy_url: proxy_url.into().trim_end_matches('/').to_string(),
            proxy_api_key,
            max_response_bytes,
        })
    }

    fn headers(&self, correlation_id: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(REQUEST_ID_HEADER.as_bytes()),
            HeaderValue::from_str(correlation_id),
        ) {
            headers.insert(name, value);
        }
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(CORRELATION_HEADER.as_bytes()),
            HeaderValue::from_str(correlation_id),
        ) {
            headers.insert(name, value);
        }
        headers
    }

    async fn send(
        &self,
        method: Method,
        url: String,
        body: Option<Value>,
        correlation_id: &str,
        extra_headers: HeaderMap,
    ) -> Result<UpstreamResponse, McpError> {
        let mut headers = self.headers(correlation_id);
        for (k, v) in extra_headers.iter() {
            headers.insert(k.clone(), v.clone());
        }

        let mut request = self.http.request(method, &url).headers(headers);
        if let Some(body) = body {
            request = request.json(&body);
        }

        let response = request
            .send()
            .await
            .map_err(|e| McpError::from_reqwest(&e))?;
        let status = response.status().as_u16();
        let governance_latency_ms = response
            .headers()
            .get("x-controlplane-latency-ms")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<i64>().ok());
        let upstream_correlation_id = response
            .headers()
            .get(CORRELATION_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        // Bound the response before buffering it in full.
        if let Some(len) = response.content_length() {
            if len as usize > self.max_response_bytes {
                return Err(McpError::payload_too_large(self.max_response_bytes));
            }
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| McpError::from_reqwest(&e))?;
        if bytes.len() > self.max_response_bytes {
            return Err(McpError::payload_too_large(self.max_response_bytes));
        }

        if !(200..300).contains(&status) {
            return Err(McpError::from_upstream_status(status));
        }

        // Empty bodies (204, DELETE) are represented as null.
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).map_err(|_| {
                // A non-JSON upstream response is an implementation detail we do
                // not surface; the caller only learns the call failed.
                McpError::upstream_unavailable()
            })?
        };

        Ok(UpstreamResponse {
            status,
            body,
            governance_latency_ms,
            upstream_correlation_id,
        })
    }

    pub async fn dashboard_get(
        &self,
        path: &str,
        query: &[(String, String)],
        correlation_id: &str,
    ) -> Result<Value, McpError> {
        let url = self.build_url(&self.dashboard_url, path, query);
        Ok(self
            .send(Method::GET, url, None, correlation_id, HeaderMap::new())
            .await?
            .body)
    }

    pub async fn dashboard_post(
        &self,
        path: &str,
        body: Value,
        correlation_id: &str,
    ) -> Result<Value, McpError> {
        let url = self.build_url(&self.dashboard_url, path, &[]);
        Ok(self
            .send(
                Method::POST,
                url,
                Some(body),
                correlation_id,
                HeaderMap::new(),
            )
            .await?
            .body)
    }

    pub async fn dashboard_put(
        &self,
        path: &str,
        body: Value,
        correlation_id: &str,
    ) -> Result<Value, McpError> {
        let url = self.build_url(&self.dashboard_url, path, &[]);
        Ok(self
            .send(
                Method::PUT,
                url,
                Some(body),
                correlation_id,
                HeaderMap::new(),
            )
            .await?
            .body)
    }

    /// Forward a governed call through the proxy (`POST /v1/messages`).
    pub async fn proxy_messages(
        &self,
        body: Value,
        correlation_id: &str,
        caller_api_key: Option<&str>,
    ) -> Result<UpstreamResponse, McpError> {
        let url = self.build_url(&self.proxy_url, "/v1/messages", &[]);
        let mut headers = HeaderMap::new();
        if let Some(key) = caller_api_key.or(self.proxy_api_key.as_deref()) {
            if let Ok(value) = HeaderValue::from_str(key) {
                headers.insert(HeaderName::from_static("x-api-key"), value);
            }
        }
        self.send(Method::POST, url, Some(body), correlation_id, headers)
            .await
    }

    /// POST to an explicitly configured absolute base URL.
    ///
    /// Used only by the opt-in internal-scan adapter; the caller is responsible
    /// for validating that `base` comes from trusted configuration.
    pub async fn absolute_post(
        &self,
        base: &str,
        path: &str,
        body: Value,
        correlation_id: &str,
    ) -> Result<Value, McpError> {
        let url = self.build_url(base.trim_end_matches('/'), path, &[]);
        Ok(self
            .send(
                Method::POST,
                url,
                Some(body),
                correlation_id,
                HeaderMap::new(),
            )
            .await?
            .body)
    }

    pub async fn absolute_get(
        &self,
        base: &str,
        path: &str,
        correlation_id: &str,
    ) -> Result<Value, McpError> {
        let url = self.build_url(base.trim_end_matches('/'), path, &[]);
        Ok(self
            .send(Method::GET, url, None, correlation_id, HeaderMap::new())
            .await?
            .body)
    }

    pub async fn dashboard_health(&self, correlation_id: &str) -> Result<Value, McpError> {
        self.dashboard_get("/health", &[], correlation_id).await
    }

    pub async fn dashboard_ready(&self, correlation_id: &str) -> Result<Value, McpError> {
        self.dashboard_get("/ready", &[], correlation_id).await
    }

    pub fn proxy_configured(&self) -> bool {
        !self.proxy_url.is_empty()
    }

    fn build_url(&self, base: &str, path: &str, query: &[(String, String)]) -> String {
        let mut url = format!("{base}{path}");
        let encoded: Vec<String> = query
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect();
        if !encoded.is_empty() {
            url.push('?');
            url.push_str(&encoded.join("&"));
        }
        url
    }
}

/// Minimal percent-encoding for query values, avoiding a new dependency.
fn encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Sanitize a raw response for inclusion in tool output.
pub fn sanitize_response(value: &Value) -> Value {
    redact::sanitize_value(value, 2_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_encoded() {
        let client =
            ControlPlaneClient::new("http://x", "http://y", None, Duration::from_secs(1), 1024)
                .unwrap();
        let url = client.build_url(
            "http://x",
            "/api/v1/requests",
            &[
                ("search".into(), "a b&c".into()),
                ("limit".into(), "5".into()),
            ],
        );
        assert!(url.contains("search=a%20b%26c"));
        assert!(url.contains("limit=5"));
    }

    #[test]
    fn empty_query_values_are_dropped() {
        let client =
            ControlPlaneClient::new("http://x", "http://y", None, Duration::from_secs(1), 1024)
                .unwrap();
        let url = client.build_url("http://x", "/p", &[("app_id".into(), "".into())]);
        assert_eq!(url, "http://x/p");
    }

    #[test]
    fn response_sanitization_redacts_credentials() {
        let v = serde_json::json!({"reason": "token = supersecretvalue", "api_key": "x"});
        let out = sanitize_response(&v);
        assert_eq!(out["api_key"], "[redacted]");
    }
}
