//! Streamable HTTP transport.
//!
//! Exposes `POST /mcp` (JSON-RPC, single or batch), plus `GET /health` and
//! `GET /ready`. Authentication accepts either `Authorization: Bearer <token>`
//! or `X-API-Key: <token>`. The request body is bounded by
//! `MCP_MAX_PAYLOAD_BYTES`; the server never echoes request bodies in errors.
//!
//! SSE streaming (`GET /mcp`) is intentionally **not** implemented; this server
//! only needs request/response semantics. Clients that require server-initiated
//! messages should use the stdio transport.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::McpError;
use crate::server::McpServer;

pub fn router(server: Arc<McpServer>, max_body_bytes: usize) -> Router {
    Router::new()
        .route("/mcp", post(mcp_handler))
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .with_state(server)
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = value.strip_prefix("Bearer ") {
            let token = token.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

async fn mcp_handler(
    State(server): State<Arc<McpServer>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let token = bearer_token(&headers);

    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            let err = McpError::parse_error();
            return (
                StatusCode::BAD_REQUEST,
                Json(crate::protocol::error(Value::Null, &err)),
            )
                .into_response();
        }
    };

    match parsed {
        Value::Array(messages) => {
            if messages.is_empty() {
                let err = McpError::invalid_request("empty batch");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(crate::protocol::error(Value::Null, &err)),
                )
                    .into_response();
            }
            let mut responses = Vec::with_capacity(messages.len());
            for message in messages {
                if let Some(response) = server.handle_message(message, token.as_deref()).await {
                    responses.push(response);
                }
            }
            if responses.is_empty() {
                StatusCode::ACCEPTED.into_response()
            } else {
                (StatusCode::OK, Json(Value::Array(responses))).into_response()
            }
        }
        single => match server.handle_message(single, token.as_deref()).await {
            Some(response) => {
                // Transport-level concerns surface as HTTP statuses so generic
                // HTTP tooling (load balancers, clients) can react correctly.
                let status = response
                    .get("error")
                    .and_then(|e| e.get("code"))
                    .and_then(Value::as_i64)
                    .and_then(|code| {
                        StatusCode::from_u16(crate::error::http_status_for_code(code)).ok()
                    })
                    .unwrap_or(StatusCode::OK);
                (status, Json(response)).into_response()
            }
            None => StatusCode::ACCEPTED.into_response(),
        },
    }
}

async fn health_handler(State(server): State<Arc<McpServer>>) -> Response {
    let _ = server;
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "version": crate::protocol::SERVER_VERSION,
            "transport": "http"
        })),
    )
        .into_response()
}

async fn ready_handler(State(server): State<Arc<McpServer>>) -> Response {
    // Readiness reflects whether the upstream ControlPlane API answers. Any
    // failure reason is sanitized to a stable code.
    let correlation_id = Uuid::now_v7().to_string();
    match server.client.dashboard_health(&correlation_id).await {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"status": "ready", "upstream": "ok"})),
        )
            .into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "degraded", "upstream": "unavailable"})),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{McpConfig, Transport};

    fn server() -> Arc<McpServer> {
        Arc::new(
            McpServer::new(McpConfig {
                transport: Transport::Http,
                auth_tokens: "secret:viewer".into(),
                ..McpConfig::default()
            })
            .unwrap(),
        )
    }

    #[tokio::test]
    async fn bearer_token_is_extracted() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer abc".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("abc".to_string()));
    }

    #[tokio::test]
    async fn api_key_header_is_extracted() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "abc".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("abc".to_string()));
    }

    #[tokio::test]
    async fn missing_token_is_none() {
        assert!(bearer_token(&HeaderMap::new()).is_none());
    }

    #[test]
    fn router_builds() {
        let _ = router(server(), 1024);
    }
}
