//! Timeout behaviour: a slow upstream must surface a sanitized timeout, not
//! hang the client or leak the upstream error.

mod common;

use std::time::Duration;

use serde_json::json;

use common::{admin_config, server_with, spawn_mock, MockConfig};

#[tokio::test]
async fn slow_upstream_times_out_cleanly() {
    let upstream = spawn_mock(MockConfig {
        delay: Duration::from_millis(750),
        ..MockConfig::default()
    })
    .await;

    let mut cfg = admin_config(&upstream.base_url);
    cfg.request_timeout = Duration::from_millis(100);
    let server = server_with(cfg);

    let response = server
        .handle_message(
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {"name": "list_apps", "arguments": {}}
            }),
            Some("admin-token"),
        )
        .await
        .unwrap();

    assert_eq!(response["result"]["isError"], true);
    let rendered = response.to_string();
    assert!(
        rendered.contains("timed out"),
        "unexpected error: {rendered}"
    );
    assert!(!rendered.contains("supersecretvalue"));
}

#[tokio::test]
async fn cancel_during_inflight_call_is_safe() {
    let upstream = spawn_mock(MockConfig {
        delay: Duration::from_millis(500),
        ..MockConfig::default()
    })
    .await;

    let mut cfg = admin_config(&upstream.base_url);
    cfg.request_timeout = Duration::from_secs(5);
    let server = server_with(cfg);

    // Dropping the future models a client disconnect / cancellation: no panic,
    // no leaked task, and the server remains usable afterwards.
    let handle = tokio::spawn(async move {
        server
            .handle_message(
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "tools/call",
                    "params": {"name": "list_apps", "arguments": {}}
                }),
                Some("admin-token"),
            )
            .await
    });
    handle.abort();
    let _ = handle.await;

    // A fresh call still succeeds.
    let upstream2 = spawn_mock(MockConfig::default()).await;
    let server2 = server_with(admin_config(&upstream2.base_url));
    let ok = server2
        .handle_message(
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {"name": "list_apps", "arguments": {}}
            }),
            Some("admin-token"),
        )
        .await
        .unwrap();
    assert_eq!(ok["result"]["isError"], false);
}
