//! End-to-end tests for the Streamable HTTP transport: auth headers, health and
//! readiness, batch requests, and the body-size limit.

mod common;

use serde_json::json;

use controlplane_mcp_server::transport::http;

use common::{admin_config, server_with, spawn_mock, MockConfig};

struct HttpServer {
    base_url: String,
    _shutdown: tokio::sync::oneshot::Sender<()>,
}

async fn spawn_http(
    server: std::sync::Arc<controlplane_mcp_server::McpServer>,
    max_body: usize,
) -> HttpServer {
    let app = http::router(server, max_body);
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
    HttpServer {
        base_url: format!("http://{addr}"),
        _shutdown: tx,
    }
}

#[tokio::test]
async fn initialize_over_http_with_bearer_token() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let http_server = spawn_http(server, 64 * 1024).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/mcp", http_server.base_url))
        .bearer_auth("admin-token")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize"}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["result"]["protocolVersion"], "2024-11-05");
}

#[tokio::test]
async fn http_without_token_is_unauthorized() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let http_server = spawn_http(server, 64 * 1024).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/mcp", http_server.base_url))
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(
        body["error"]["code"],
        controlplane_mcp_server::error::codes::UNAUTHORIZED
    );
}

#[tokio::test]
async fn api_key_header_is_accepted() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let http_server = spawn_http(server, 64 * 1024).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/mcp", http_server.base_url))
        .header("x-api-key", "viewer-token")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn health_and_ready_endpoints() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let http_server = spawn_http(server, 64 * 1024).await;
    let client = reqwest::Client::new();

    let health = client
        .get(format!("{}/health", http_server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status(), 200);
    let body: serde_json::Value = health.json().await.unwrap();
    assert_eq!(body["status"], "ok");

    let ready = client
        .get(format!("{}/ready", http_server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(ready.status(), 200);
}

#[tokio::test]
async fn batch_requests_return_an_array() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let http_server = spawn_http(server, 64 * 1024).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/mcp", http_server.base_url))
        .bearer_auth("admin-token")
        .json(&json!([
            {"jsonrpc":"2.0","id":1,"method":"ping"},
            {"jsonrpc":"2.0","id":2,"method":"tools/list"}
        ]))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn oversized_body_is_rejected() {
    let upstream = spawn_mock(MockConfig::default()).await;
    let server = server_with(admin_config(&upstream.base_url));
    let http_server = spawn_http(server, 1024).await;
    let client = reqwest::Client::new();

    let big = "x".repeat(4096);
    let response = client
        .post(format!("{}/mcp", http_server.base_url))
        .bearer_auth("admin-token")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"ping","params":{"pad": big}}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 413);
}
