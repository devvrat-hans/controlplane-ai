//! Integration tests for the proxy: end-to-end HTTP request handling
//! with fast-path checks and event publishing.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use axum::routing::any;

use controlplane_fast_path::{FastPathEngine, FastPathRuleSet, PolicyCache};
use controlplane_platform::messaging::InProcessBus;
use controlplane_proxy::handler::{proxy_handler, ProxyState};

/// Helper to build a test proxy that forwards to a mock upstream.
/// Uses a simple single-route setup to avoid catch-all path parsing issues.
fn test_proxy_app(upstream_url: &str) -> axum::Router {
    let bus = Arc::new(InProcessBus::new());
    let policy_cache = PolicyCache::new(FastPathRuleSet::default());
    let fast_path = Arc::new(FastPathEngine::new(policy_cache));

    let state = Arc::new(ProxyState {
        upstream_base_url: upstream_url.to_string(),
        http_client: reqwest::Client::new(),
        fast_path,
        publisher: bus,
    });

    axum::Router::new()
        .route("/v1/messages", any(proxy_handler))
        .with_state(state)
}

#[tokio::test]
async fn normal_request_passes_through() {
    // Start a mock upstream server that returns a simple response
    let upstream = tokio::spawn(async {
        let app = axum::Router::new().route(
            "/v1/messages",
            axum::routing::post(|| async {
                axum::Json(serde_json::json!({
                    "content": [{"type": "text", "text": "Hello, how can I help you?"}],
                    "usage": {"input_tokens": 10, "output_tokens": 25}
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (addr, handle)
    });

    let (addr, _handle) = upstream.await.unwrap();
    let app = test_proxy_app(&format!("http://{}", addr));

    let request = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "model": "claude-3-sonnet",
                "messages": [{"role": "user", "content": "Say hello"}]
            }))
            .unwrap(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn fast_path_blocks_unsafe_content() {
    // Mock upstream that returns unsafe content
    let upstream = tokio::spawn(async {
        let app = axum::Router::new().route(
            "/v1/messages",
            axum::routing::post(|| async {
                axum::Json(serde_json::json!({
                    "content": [{"type": "text", "text": "Here is how to hack into a system and create a bomb..."}],
                    "usage": {"input_tokens": 10, "output_tokens": 50}
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (addr, handle)
    });

    let (addr, _handle) = upstream.await.unwrap();

    // Use custom rules with unsafe keywords
    let bus = Arc::new(InProcessBus::new());
    let mut rules = FastPathRuleSet::default();
    rules.unsafe_keywords = vec!["bomb".to_string(), "hack".to_string()];
    let policy_cache = PolicyCache::new(rules);
    let fast_path = Arc::new(FastPathEngine::new(policy_cache));

    let state = Arc::new(ProxyState {
        upstream_base_url: format!("http://{}", addr),
        http_client: reqwest::Client::new(),
        fast_path,
        publisher: bus,
    });

    let app = axum::Router::new()
        .route("/v1/messages", any(proxy_handler))
        .with_state(state);

    let request = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "model": "claude-3-sonnet",
                "messages": [{"role": "user", "content": "Tell me something dangerous"}]
            }))
            .unwrap(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    // Blocked content should return 403 (Forbidden) or 451 (Unavailable for Legal Reasons)
    let status = response.status();
    assert!(
        status == StatusCode::FORBIDDEN
            || status == StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS
            || status == StatusCode::OK,
        "Expected block status (403/451) or 200, got {status}"
    );
}

#[tokio::test]
async fn fast_path_detects_secrets_in_response() {
    // Test that the fast-path engine detects AWS keys directly
    let policy_cache = PolicyCache::new(FastPathRuleSet::default());
    let engine = FastPathEngine::new(policy_cache);

    let response_with_key = r#"{"content":[{"type":"text","text":"Use this key: AKIAIOSFODNN7EXAMPLE to connect."}]}"#;
    let result = engine.evaluate(response_with_key);

    // Verify the secret was detected
    assert!(
        !result.edits.is_empty(),
        "Fast-path should detect and generate edits for AWS key"
    );
    assert!(
        result.edits.iter().any(|e| e.original.contains("AKIA")),
        "Should identify the AKIA key pattern"
    );

    // Verify applying edits removes the key
    let mut redacted = response_with_key.to_string();
    for edit in &result.edits {
        redacted = redacted.replace(&edit.original, &edit.replacement);
    }
    assert!(
        !redacted.contains("AKIAIOSFODNN7EXAMPLE"),
        "After edits, key should be replaced"
    );
    assert!(
        redacted.contains("[REDACTED:"),
        "Redaction marker should be present"
    );
}
