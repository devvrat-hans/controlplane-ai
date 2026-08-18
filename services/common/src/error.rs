use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ControlPlaneError {
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Messaging error: {0}")]
    Messaging(String),

    #[error("Proxy error: {0}")]
    Proxy(String),

    #[error("Upstream timeout: {0}")]
    UpstreamTimeout(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Rate limited: {0}")]
    RateLimited(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl ControlPlaneError {
    pub fn http_status_code(&self) -> u16 {
        match self {
            Self::NotFound(_) => 404,
            Self::Validation(_) => 400,
            Self::Unauthorized(_) => 401,
            Self::Forbidden(_) => 403,
            Self::Conflict(_) => 409,
            Self::RateLimited(_) => 429,
            Self::UpstreamTimeout(_) => 504,
            Self::Database(_) | Self::Messaging(_) | Self::Internal(_) => 500,
            Self::Proxy(_) => 502,
            Self::Config(_) => 500,
        }
    }

    pub fn error_code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::Validation(_) => "validation_error",
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::Conflict(_) => "conflict",
            Self::RateLimited(_) => "rate_limited",
            Self::UpstreamTimeout(_) => "upstream_timeout",
            Self::Database(_) => "database_error",
            Self::Messaging(_) => "messaging_error",
            Self::Proxy(_) => "proxy_error",
            Self::Config(_) => "config_error",
            Self::Internal(_) => "internal_error",
        }
    }
}

/// Standard API error response shape.
/// All endpoints return: `{ "error": { "code", "message", "details" } }`
#[derive(Debug, Serialize)]
pub struct ApiErrorResponse {
    pub error: ApiError,
}

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl From<ControlPlaneError> for ApiErrorResponse {
    fn from(err: ControlPlaneError) -> Self {
        ApiErrorResponse {
            error: ApiError {
                code: err.error_code().to_string(),
                message: err.to_string(),
                details: None,
            },
        }
    }
}

impl From<&ControlPlaneError> for ApiErrorResponse {
    fn from(err: &ControlPlaneError) -> Self {
        ApiErrorResponse {
            error: ApiError {
                code: err.error_code().to_string(),
                message: err.to_string(),
                details: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_status_codes() {
        assert_eq!(ControlPlaneError::NotFound("x".into()).http_status_code(), 404);
        assert_eq!(ControlPlaneError::Validation("x".into()).http_status_code(), 400);
        assert_eq!(ControlPlaneError::Unauthorized("x".into()).http_status_code(), 401);
        assert_eq!(ControlPlaneError::Forbidden("x".into()).http_status_code(), 403);
        assert_eq!(ControlPlaneError::RateLimited("x".into()).http_status_code(), 429);
        assert_eq!(ControlPlaneError::Internal("x".into()).http_status_code(), 500);
    }

    #[test]
    fn api_error_serialization() {
        let err = ControlPlaneError::NotFound("policy not found".into());
        let response: ApiErrorResponse = err.into();
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("not_found"));
        assert!(json.contains("policy not found"));
    }
}
