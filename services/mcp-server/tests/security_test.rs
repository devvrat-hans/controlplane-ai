//! Security tests: authentication, capability authorization, app-scoped
//! isolation, payload limits, and the opt-in scanner gate.

mod common;

use serde_json::{json, Value};
use std::sync::Arc;

use controlplane_mcp_server::error::codes;
use controlplane_mcp_server::{McpConfig, McpServer};

use common::{admin_config, server_with, spawn_mock, MockConfig};

async fn call_with(server: &Arc<McpServer>, token: Option<&str>, message: Value) -> Value {
    server
        .handle_message(message, token)
        .await
        .expect("expected a response")
}

fn tool_call(id: i64, name: &str, args: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": args }
    })
}

#[tokio::test]
async fn unknown_token_is_rejected() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let response = call_with(
        &server,
        Some("not-a-real-token"),
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    )
    .await;
    assert_eq!(response["error"]["code"], codes::UNAUTHORIZED);
}

#[tokio::test]
async fn missing_token_is_rejected_when_anonymous_disabled() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let response = call_with(
        &server,
        None,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    )
    .await;
    assert_eq!(response["error"]["code"], codes::UNAUTHORIZED);
}

#[tokio::test]
async fn viewer_cannot_resolve_escalations() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let response = call_with(
        &server,
        Some("viewer-token"),
        tool_call(
            1,
            "resolve_escalation",
            json!({
                "escalation_id": "10000000-0000-0000-0000-0000000000bb",
                "action": "dismiss"
            }),
        ),
    )
    .await;
    assert_eq!(response["error"]["code"], codes::FORBIDDEN);
}

#[tokio::test]
async fn reviewer_cannot_change_policies() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let response = call_with(
        &server,
        Some("reviewer-token"),
        tool_call(
            1,
            "update_policy",
            json!({
                "app_id": "10000000-0000-0000-0000-000000000001",
                "policy": {"block_threshold": 0.9}
            }),
        ),
    )
    .await;
    assert_eq!(response["error"]["code"], codes::FORBIDDEN);
}

#[tokio::test]
async fn app_scoped_token_cannot_reach_other_apps() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let mut cfg = admin_config(&upstream.base_url);
    cfg.auth_tokens = "scoped:admin:10000000-0000-0000-0000-000000000001".into();
    let server = server_with(cfg);

    // Allowed app.
    let ok = call_with(
        &server,
        Some("scoped"),
        tool_call(
            1,
            "get_policy",
            json!({"app_id": "10000000-0000-0000-0000-000000000001"}),
        ),
    )
    .await;
    assert!(ok["error"].is_null(), "expected success for in-scope app");

    // Out-of-scope app.
    let denied = call_with(
        &server,
        Some("scoped"),
        tool_call(
            2,
            "get_policy",
            json!({"app_id": "10000000-0000-0000-0000-000000000002"}),
        ),
    )
    .await;
    assert_eq!(denied["error"]["code"], codes::FORBIDDEN);
}

#[tokio::test]
async fn oversized_evaluate_payload_is_rejected() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let mut cfg = admin_config(&upstream.base_url);
    cfg.max_payload_bytes = 512;
    let server = server_with(cfg);

    let response = call_with(
        &server,
        Some("admin-token"),
        tool_call(
            1,
            "evaluate_prompt",
            json!({"messages": [{"role": "user", "content": "x".repeat(4000)}]}),
        ),
    )
    .await;
    assert_eq!(response["error"]["code"], codes::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn internal_scanner_is_disabled_by_default() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let response = call_with(
        &server,
        Some("admin-token"),
        tool_call(1, "scan_content", json!({"kind": "pii", "text": "a@b.com"})),
    )
    .await;
    assert_eq!(response["error"]["code"], codes::FORBIDDEN);
}

#[tokio::test]
async fn path_traversal_in_profile_id_is_rejected() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let response = call_with(
        &server,
        Some("admin-token"),
        tool_call(
            1,
            "apply_profile",
            json!({
                "app_id": "10000000-0000-0000-0000-000000000001",
                "profile_id": "../../admin"
            }),
        ),
    )
    .await;
    // Rejected as a validation failure before any request is issued.
    assert_eq!(response["result"]["isError"], true);
    assert!(!response.to_string().contains("../../admin"));
}

#[tokio::test]
async fn anonymous_reads_only_when_explicitly_enabled() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let mut cfg = McpConfig {
        allow_anonymous: true,
        ..admin_config(&upstream.base_url)
    };
    cfg.auth_tokens = String::new();
    let server = server_with(cfg);

    let read = call_with(
        &server,
        None,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    )
    .await;
    assert!(read["error"].is_null());

    let write = call_with(
        &server,
        None,
        tool_call(
            2,
            "update_policy",
            json!({
                "app_id": "10000000-0000-0000-0000-000000000001",
                "policy": {"block_threshold": 0.9}
            }),
        ),
    )
    .await;
    assert_eq!(write["error"]["code"], codes::FORBIDDEN);
}
