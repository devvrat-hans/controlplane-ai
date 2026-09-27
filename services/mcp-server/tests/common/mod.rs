#![allow(dead_code)]

//! Shared mock upstream for integration tests.
//!
//! Stands in for `controlplane-dashboard-api` and `controlplane-proxy` so tests
//! exercise the real MCP dispatch, validation, redaction and error mapping
//! without needing a database, NATS or a model provider.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

#[derive(Clone, Default)]
pub struct MockConfig {
    /// Artificial latency, used to exercise the MCP timeout path.
    pub delay: Duration,
    /// When set, every dashboard route returns this status.
    pub force_status: Option<u16>,
}

#[derive(Clone)]
struct MockState {
    cfg: MockConfig,
}

pub struct MockUpstream {
    pub base_url: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Drop for MockUpstream {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

pub async fn spawn_mock(cfg: MockConfig) -> MockUpstream {
    let state = MockState { cfg };

    let app = Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"status":"ok","version":"0.0.0"})) }),
        )
        .route("/ready", get(|| async { Json(json!({"status":"ready"})) }))
        .route("/api/v1/apps", get(apps))
        .route("/api/v1/requests/{call_id}", get(request_detail))
        .route("/api/v1/policies/{app_id}", get(policy))
        .route("/api/v1/profiles", get(profiles))
        .route("/api/v1/escalations", get(escalations))
        .route("/api/v1/audit/verify", get(verify))
        .route("/v1/messages", post(proxy_messages))
        .fallback(not_found)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();

    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });

    MockUpstream {
        base_url: format!("http://{addr}"),
        shutdown: Some(tx),
    }
}

async fn guard(state: &MockState) -> Result<(), Response> {
    if !state.cfg.delay.is_zero() {
        tokio::time::sleep(state.cfg.delay).await;
    }
    if let Some(status) = state.cfg.force_status {
        let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        // Body deliberately contains a secret to prove it never reaches the client.
        return Err((code, "password = supersecretvalue").into_response());
    }
    Ok(())
}

async fn apps(State(state): State<MockState>) -> Response {
    if let Err(r) = guard(&state).await {
        return r;
    }
    Json(json!([
        {"id": "10000000-0000-0000-0000-000000000001", "name": "ChatBot-Prod", "data_governance_level": "high"}
    ]))
    .into_response()
}

async fn request_detail(State(state): State<MockState>, Path(call_id): Path<String>) -> Response {
    if let Err(r) = guard(&state).await {
        return r;
    }
    Json(json!({
        "call": {
            "id": call_id,
            "correlation_id": "c-1",
            "app_id": "10000000-0000-0000-0000-000000000001",
            "model": "qwen2.5:1.5b",
            "request_payload": {"messages": [{"role": "user", "content": "hi a@b.com"}]},
            "response_payload": {"secret": "sk-live-should-not-leak"},
            "token_count_input": 10,
            "token_count_output": 20
        },
        "verdicts": [{
            "id": "v-1",
            "axis": "responsibility",
            "path": "fast",
            "outcome": "edit",
            "confidence": 0.97,
            "reason": "found AKIAIOSFODNN7EXAMPLE contact a@b.com",
            "check_name": "secret_detection",
            "latency_ms": 2,
            "created_at": "2026-09-26T10:00:00Z"
        }],
        "audit_records": [],
        "escalation": null
    }))
    .into_response()
}

async fn policy(State(state): State<MockState>, Path(app_id): Path<String>) -> Response {
    if let Err(r) = guard(&state).await {
        return r;
    }
    Json(json!({"app_id": app_id, "version": 3, "policies": [], "merged": {}})).into_response()
}

async fn profiles(State(state): State<MockState>) -> Response {
    if let Err(r) = guard(&state).await {
        return r;
    }
    Json(json!({"profiles": [{"id": "eu-financial", "name": "EU Financial Services"}]}))
        .into_response()
}

async fn escalations(State(state): State<MockState>) -> Response {
    if let Err(r) = guard(&state).await {
        return r;
    }
    Json(json!({"escalations": [], "total": 0})).into_response()
}

async fn verify(State(state): State<MockState>) -> Response {
    if let Err(r) = guard(&state).await {
        return r;
    }
    Json(json!({"valid": true, "records_checked": 10, "first_broken_at": null})).into_response()
}

async fn proxy_messages(State(state): State<MockState>, Json(body): Json<Value>) -> Response {
    if let Err(r) = guard(&state).await {
        return r;
    }
    let _ = body;
    Json(json!({
        "id": "msg-1",
        "choices": [{"message": {"role": "assistant", "content": "4"}}],
        "usage": {"input_tokens": 5, "output_tokens": 1}
    }))
    .into_response()
}

async fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": {"code": "not_found"}})),
    )
        .into_response()
}

/// Build an MCP server pointed at the mock, with a single admin token.
pub fn admin_config(base_url: &str) -> controlplane_mcp_server::McpConfig {
    controlplane_mcp_server::McpConfig {
        dashboard_url: base_url.to_string(),
        proxy_url: base_url.to_string(),
        auth_tokens: "admin-token:admin,reviewer-token:reviewer,viewer-token:viewer".into(),
        allow_anonymous: false,
        ..controlplane_mcp_server::McpConfig::default()
    }
}

pub fn server_with(
    cfg: controlplane_mcp_server::McpConfig,
) -> Arc<controlplane_mcp_server::McpServer> {
    Arc::new(controlplane_mcp_server::McpServer::new(cfg).unwrap())
}
