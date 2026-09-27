//! Sanitized error model for the MCP server.
//!
//! Every error that leaves this process is constructed here, so that upstream
//! response bodies, prompts, PII and internal implementation details can never
//! leak through an error message. The [`McpError::message`] is always a short,
//! operator-safe string; [`McpError::data`] may carry a stable machine code and
//! a correlation id, never payloads.

use serde_json::{json, Value};

/// JSON-RPC 2.0 + MCP error codes.
pub mod codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;

    // MCP-reserved / transport extension codes (outside the JSON-RPC range).
    pub const UNAUTHORIZED: i64 = -32001;
    pub const FORBIDDEN: i64 = -32002;
    pub const RATE_LIMITED: i64 = -32003;
    pub const UPSTREAM_UNAVAILABLE: i64 = -32004;
    pub const TIMEOUT: i64 = -32005;
    pub const NOT_FOUND: i64 = -32006;
    pub const PAYLOAD_TOO_LARGE: i64 = -32007;
}

/// A sanitized error that is safe to return to an MCP client.
#[derive(Debug, Clone)]
pub struct McpError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
    /// HTTP status used by the Streamable HTTP transport.
    pub http_status: u16,
}

impl McpError {
    pub fn new(code: i64, message: impl Into<String>, http_status: u16) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
            http_status,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    /// Attach the correlation id for the request that failed. Correlation ids are
    /// safe to return: they are join keys, not secrets.
    pub fn correlation(mut self, correlation_id: &str) -> Self {
        self.data = Some(match self.data.take() {
            Some(Value::Object(mut map)) => {
                map.insert("correlation_id".into(), json!(correlation_id));
                Value::Object(map)
            }
            _ => json!({ "correlation_id": correlation_id }),
        });
        self
    }

    pub fn parse_error() -> Self {
        Self::new(codes::PARSE_ERROR, "Invalid JSON-RPC payload", 400)
    }

    pub fn invalid_request(msg: impl Into<String>) -> Self {
        Self::new(codes::INVALID_REQUEST, msg, 400)
    }

    pub fn method_not_found(method: &str) -> Self {
        // Only the method name is echoed — it is caller-supplied, not sensitive.
        Self::new(
            codes::METHOD_NOT_FOUND,
            format!("Unknown method: {method}"),
            404,
        )
    }

    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self::new(codes::INVALID_PARAMS, msg, 400)
    }

    pub fn internal() -> Self {
        // Deliberately opaque: never surface internal error strings.
        Self::new(codes::INTERNAL_ERROR, "Internal server error", 500)
    }

    pub fn unauthorized() -> Self {
        Self::new(codes::UNAUTHORIZED, "Authentication required", 401)
    }

    pub fn forbidden(capability: &str) -> Self {
        Self::new(
            codes::FORBIDDEN,
            format!("Insufficient privileges for capability: {capability}"),
            403,
        )
    }

    pub fn rate_limited(retry_after_ms: u64) -> Self {
        Self::new(codes::RATE_LIMITED, "Rate limit exceeded", 429)
            .with_data(json!({ "retry_after_ms": retry_after_ms }))
    }

    pub fn upstream_unavailable() -> Self {
        Self::new(
            codes::UPSTREAM_UNAVAILABLE,
            "ControlPlane API unavailable",
            502,
        )
    }

    pub fn timeout() -> Self {
        Self::new(codes::TIMEOUT, "Upstream request timed out", 504)
    }

    pub fn not_found(resource: &str) -> Self {
        Self::new(codes::NOT_FOUND, format!("{resource} not found"), 404)
    }

    pub fn payload_too_large(limit: usize) -> Self {
        Self::new(
            codes::PAYLOAD_TOO_LARGE,
            format!("Payload exceeds the {limit}-byte limit"),
            413,
        )
    }

    /// Map an upstream HTTP status onto a sanitized error. The response body is
    /// never included.
    pub fn from_upstream_status(status: u16) -> Self {
        match status {
            400 => Self::new(codes::INVALID_PARAMS, "Upstream rejected the request", 400),
            401 => Self::new(codes::UNAUTHORIZED, "Upstream authentication failed", 401),
            403 => Self::new(codes::FORBIDDEN, "Upstream denied the request", 403),
            404 => Self::not_found("Requested resource"),
            413 => Self::new(
                codes::PAYLOAD_TOO_LARGE,
                "Upstream rejected the request as too large",
                413,
            ),
            429 => Self::rate_limited(1000),
            500..=599 => Self::upstream_unavailable(),
            _ => Self::upstream_unavailable(),
        }
    }

    /// Map a transport error onto a sanitized error.
    pub fn from_reqwest(err: &reqwest::Error) -> Self {
        if err.is_timeout() {
            Self::timeout()
        } else {
            Self::upstream_unavailable()
        }
    }
}

/// HTTP status for a JSON-RPC error code, used by the Streamable HTTP
/// transport so that transport-level concerns (auth, rate limiting) are
/// observable as status codes by generic HTTP tooling.
pub fn http_status_for_code(code: i64) -> u16 {
    match code {
        codes::PARSE_ERROR | codes::INVALID_REQUEST | codes::INVALID_PARAMS => 400,
        codes::UNAUTHORIZED => 401,
        codes::FORBIDDEN => 403,
        codes::NOT_FOUND | codes::METHOD_NOT_FOUND => 404,
        codes::PAYLOAD_TOO_LARGE => 413,
        codes::RATE_LIMITED => 429,
        codes::INTERNAL_ERROR => 500,
        codes::UPSTREAM_UNAVAILABLE => 502,
        codes::TIMEOUT => 504,
        _ => 200,
    }
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (code {})", self.message, self.code)
    }
}

impl std::error::Error for McpError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_error_is_opaque() {
        assert_eq!(McpError::internal().message, "Internal server error");
    }

    #[test]
    fn upstream_body_never_leaks() {
        let err = McpError::from_upstream_status(500);
        assert!(!err.message.contains("password"));
        assert_eq!(err.code, codes::UPSTREAM_UNAVAILABLE);
    }

    #[test]
    fn correlation_is_attached_safely() {
        let err = McpError::timeout().correlation("abc-123");
        assert_eq!(err.data.unwrap()["correlation_id"], "abc-123");
    }

    #[test]
    fn http_status_mapping() {
        assert_eq!(http_status_for_code(codes::UNAUTHORIZED), 401);
        assert_eq!(http_status_for_code(codes::FORBIDDEN), 403);
        assert_eq!(http_status_for_code(codes::RATE_LIMITED), 429);
        assert_eq!(http_status_for_code(codes::TIMEOUT), 504);
    }
}
