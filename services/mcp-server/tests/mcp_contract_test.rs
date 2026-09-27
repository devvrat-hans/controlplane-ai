//! Contract tests for the MCP surface: the tool/resource catalogue, successful
//! calls, redaction guarantees, and upstream failure mapping.

mod common;

use serde_json::{json, Value};
use std::sync::Arc;

use controlplane_mcp_server::McpServer;

use common::{admin_config, server_with, spawn_mock, MockConfig};

async fn call(server: &Arc<McpServer>, method: &str, id: i64, params: Option<Value>) -> Value {
    let mut message = json!({ "jsonrpc": "2.0", "id": id, "method": method });
    if let Some(params) = params {
        message["params"] = params;
    }
    server
        .handle_message(message, Some("admin-token"))
        .await
        .expect("expected a response")
}

async fn call_tool(server: &Arc<McpServer>, name: &str, args: Value) -> Value {
    call(
        server,
        "tools/call",
        2,
        Some(json!({ "name": name, "arguments": args })),
    )
    .await
}

#[tokio::test]
async fn initialize_handshake_is_well_formed() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let response = call(&server, "initialize", 1, None).await;
    assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(response["result"]["serverInfo"]["name"], "controlplane-mcp");
    assert!(response["result"]["capabilities"]["tools"].is_object());
}

#[tokio::test]
async fn tools_catalogue_contains_the_documented_surface() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let response = call(&server, "tools/list", 1, None).await;
    let tools = response["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    for expected in [
        "list_apps",
        "get_policy",
        "list_requests",
        "get_request",
        "get_detection_quality",
        "get_judge_agreement",
        "list_escalations",
        "verify_audit_chain",
        "get_system_config",
        "scan_content",
        "evaluate_prompt",
        "resolve_escalation",
        "update_policy",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }
    // Every tool must carry a typed input schema.
    for tool in tools {
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
}

#[tokio::test]
async fn list_apps_succeeds() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let response = call_tool(&server, "list_apps", json!({})).await;
    assert_eq!(response["result"]["isError"], false);
    let structured = &response["result"]["structuredContent"];
    assert_eq!(structured[0]["name"], "ChatBot-Prod");
}

#[tokio::test]
async fn get_request_redacts_secrets_pii_and_payloads() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let call_id = "10000000-0000-0000-0000-0000000000aa";
    let response = call_tool(&server, "get_request", json!({ "call_id": call_id })).await;
    assert_eq!(response["result"]["isError"], false);

    let rendered = response.to_string();
    assert!(!rendered.contains("AKIAIOSFODNN7EXAMPLE"), "aws key leaked");
    assert!(!rendered.contains("a@b.com"), "email leaked");
    assert!(
        !rendered.contains("sk-live-should-not-leak"),
        "secret payload leaked"
    );
    assert!(
        rendered.contains("[REDACTED:AWS_KEY]"),
        "expected redaction marker"
    );
    assert!(
        rendered.contains("[redacted]"),
        "expected payload suppression"
    );
}

#[tokio::test]
async fn resources_can_be_listed_and_read() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));

    let list = call(&server, "resources/list", 1, None).await;
    let uris: Vec<&str> = list["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["uri"].as_str())
        .collect();
    assert!(uris.contains(&"controlplane://apps"));

    let templates = call(&server, "resources/templates/list", 2, None).await;
    assert!(!templates["result"]["resourceTemplates"]
        .as_array()
        .unwrap()
        .is_empty());

    let read = call(
        &server,
        "resources/read",
        3,
        Some(json!({ "uri": "controlplane://apps" })),
    )
    .await;
    assert!(read["result"]["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("ChatBot-Prod"));
}

#[tokio::test]
async fn unknown_resource_is_a_clean_error() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let response = call(
        &server,
        "resources/read",
        1,
        Some(json!({ "uri": "controlplane://does-not-exist" })),
    )
    .await;
    assert_eq!(
        response["error"]["code"],
        controlplane_mcp_server::error::codes::NOT_FOUND
    );
}

#[tokio::test]
async fn invalid_tool_arguments_produce_a_tool_error() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let response = call_tool(&server, "get_request", json!({ "call_id": "not-a-uuid" })).await;
    assert_eq!(response["result"]["isError"], true);
}

#[tokio::test]
async fn upstream_failure_is_sanitized() {
    let upstream = spawn_mock(MockConfig {
        force_status: Some(500),
        ..MockConfig::default()
    })
    .await;
    let server = server_with(admin_config(&upstream.base_url));
    let response = call_tool(&server, "list_apps", json!({})).await;

    assert_eq!(response["result"]["isError"], true);
    let rendered = response.to_string();
    assert!(
        !rendered.contains("supersecretvalue"),
        "upstream body leaked"
    );
    assert!(rendered.contains("ControlPlane API unavailable"));
}

#[tokio::test]
async fn evaluate_prompt_returns_governance_metadata() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let response = call_tool(
        &server,
        "evaluate_prompt",
        json!({ "messages": [{ "role": "user", "content": "What is 2+2?" }], "max_tokens": 16 }),
    )
    .await;

    assert_eq!(response["result"]["isError"], false);
    let structured = &response["result"]["structuredContent"];
    assert!(structured["governance"]["correlation_id"].is_string());
    assert_eq!(structured["response"]["usage"]["output_tokens"], 1);
}

#[tokio::test]
async fn system_config_omits_credentials() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    // The mock has no /system/config route, so this exercises the 404 path.
    let response = call_tool(&server, "get_system_config", json!({})).await;
    assert_eq!(response["result"]["isError"], true);
    let rendered = response.to_string();
    assert!(!rendered.contains("supersecretvalue"));
}
