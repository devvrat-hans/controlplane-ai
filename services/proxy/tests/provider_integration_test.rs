//! Integration tests for proxy handler with both Anthropic and Gemini providers.
//!
//! Tests verify that:
//! - Anthropic requests are forwarded correctly with API key in header
//! - Gemini requests are forwarded to the correct endpoint with API key as query param
//! - Token extraction works for both provider formats
//! - Fast-path still works regardless of provider
//! - Auth headers are not leaked upstream

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use axum::routing::any;

use controlplane_common::{create_provider, provider::ProviderKind};
use controlplane_fast_path::{FastPathEngine, FastPathRuleSet, PolicyCache};
use controlplane_platform::messaging::InProcessBus;
use controlplane_proxy::handler::{proxy_handler, ProxyState};

// =============================================================================
// Helpers
// =============================================================================

fn build_proxy_state(upstream_url: &str, provider_kind: ProviderKind) -> Arc<ProxyState> {
    let bus = Arc::new(InProcessBus::new());
    let policy_cache = PolicyCache::new(FastPathRuleSet::default());
    let fast_path = Arc::new(FastPathEngine::new(policy_cache));

    Arc::new(ProxyState {
        upstream_base_url: upstream_url.to_string(),
        upstream_api_key: "test-api-key-12345".to_string(),
        default_model: "test-model".to_string(),
        provider: create_provider(provider_kind),
        http_client: reqwest::Client::new(),
        fast_path,
        publisher: bus,
    })
}

fn anthropic_request_body() -> String {
    serde_json::to_string(&serde_json::json!({
        "model": "claude-3-sonnet",
        "messages": [{"role": "user", "content": "Hello"}]
    }))
    .unwrap()
}

fn gemini_request_body() -> String {
    serde_json::to_string(&serde_json::json!({
        "model": "gemini-2.0-flash",
        "contents": [{"role": "user", "parts": [{"text": "Hello"}]}]
    }))
    .unwrap()
}

fn opencode_request_body() -> String {
    serde_json::to_string(&serde_json::json!({
        "model": "mimo-v2.5-free",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 256,
        "stream": false
    }))
    .unwrap()
}

fn ollama_request_body() -> String {
    serde_json::to_string(&serde_json::json!({
        "model": "qwen2.5:1.5b",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 256,
        "stream": false
    }))
    .unwrap()
}

fn anthropic_upstream_response() -> serde_json::Value {
    serde_json::json!({
        "content": [{"type": "text", "text": "Response from upstream"}],
        "usage": {"input_tokens": 10, "output_tokens": 20}
    })
}

fn gemini_upstream_response() -> serde_json::Value {
    serde_json::json!({
        "candidates": [{"content": {"parts": [{"text": "Gemini response!"}]}}],
        "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 25}
    })
}

fn make_mock_upstream_handler(
    response_json: serde_json::Value,
) -> impl Fn(Request<Body>) -> std::pin::Pin<Box<dyn std::future::Future<Output = axum::Json<serde_json::Value>> + Send>> + Clone + Send + Sync + 'static {
    move |req: Request<Body>| {
        let resp = response_json.clone();
        Box::pin(async move {
            let headers = req.headers().clone();
            let query = req.uri().query().unwrap_or("").to_string();
            let api_key_count = headers.get_all("x-api-key").iter().count();
            let mut result = resp.clone();
            result["_meta"] = serde_json::json!({
                "api_key_count": api_key_count,
                "query": query,
                "has_key_in_query": query.contains("key="),
                "has_x_api_key_header": headers.get("x-api-key").is_some(),
                "has_anthropic_version": headers.get("anthropic-version").is_some(),
            });
            axum::Json(result)
        })
    }
}

// =============================================================================
// Anthropic Provider Tests
// =============================================================================

#[tokio::test]
async fn anthropic_forwards_api_key_as_header() {
    let upstream_json = anthropic_upstream_response();
    let handler = make_mock_upstream_handler(upstream_json);

    let upstream = tokio::spawn(async {
        let app = axum::Router::new().route("/{*path}", axum::routing::any(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (addr, handle)
    });

    let (addr, _handle) = upstream.await.unwrap();
    let state = build_proxy_state(&format!("http://{}", addr), ProviderKind::Anthropic);
    let app = axum::Router::new()
        .route("/v1/messages", any(proxy_handler))
        .with_state(state);

    let request = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .body(Body::from(anthropic_request_body()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let meta = &json["_meta"];

    assert_eq!(meta["has_x_api_key_header"], true, "Anthropic should set x-api-key header");
    assert_eq!(meta["has_anthropic_version"], true, "Anthropic should set anthropic-version header");
}

#[tokio::test]
async fn anthropic_does_not_duplicate_auth_headers() {
    let upstream_json = anthropic_upstream_response();
    let handler = make_mock_upstream_handler(upstream_json);

    let upstream = tokio::spawn(async {
        let app = axum::Router::new().route("/{*path}", axum::routing::any(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (addr, handle)
    });

    let (addr, _handle) = upstream.await.unwrap();
    let state = build_proxy_state(&format!("http://{}", addr), ProviderKind::Anthropic);
    let app = axum::Router::new()
        .route("/v1/messages", any(proxy_handler))
        .with_state(state);

    let request = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .body(Body::from(anthropic_request_body()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let body_bytes = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let meta = &json["_meta"];

    // Should have exactly 1 x-api-key (from apply_auth only)
    assert_eq!(meta["api_key_count"], 1, "Should have exactly 1 x-api-key header");
}

#[tokio::test]
async fn anthropic_fast_path_redacts_secrets() {
    let upstream_json = serde_json::json!({
        "content": [{"type": "text", "text": "Use this key: AKIAIOSFODNN7EXAMPLE to connect."}],
        "usage": {"input_tokens": 10, "output_tokens": 30}
    });
    let handler = make_mock_upstream_handler(upstream_json);

    let upstream = tokio::spawn(async {
        let app = axum::Router::new().route("/{*path}", axum::routing::any(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (addr, handle)
    });

    let (addr, _handle) = upstream.await.unwrap();
    let state = build_proxy_state(&format!("http://{}", addr), ProviderKind::Anthropic);
    let app = axum::Router::new()
        .route("/v1/messages", any(proxy_handler))
        .with_state(state);

    let request = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .body(Body::from(anthropic_request_body()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let body_bytes = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024).await.unwrap();
    let body_str = String::from_utf8_lossy(&body_bytes);

    assert!(!body_str.contains("AKIAIOSFODNN7EXAMPLE"), "AWS key should be redacted");
    assert!(body_str.contains("REDACTED"), "Redaction marker should be present");
}

// =============================================================================
// Gemini Provider Tests
// =============================================================================

#[tokio::test]
async fn gemini_forwards_api_key_as_query_param() {
    let upstream_json = gemini_upstream_response();
    let handler = make_mock_upstream_handler(upstream_json);

    let upstream = tokio::spawn(async {
        let app = axum::Router::new().route("/{*path}", axum::routing::any(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (addr, handle)
    });

    let (addr, _handle) = upstream.await.unwrap();
    let state = build_proxy_state(&format!("http://{}", addr), ProviderKind::Gemini);
    let app = axum::Router::new()
        .route("/v1/messages", any(proxy_handler))
        .with_state(state);

    let request = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .body(Body::from(gemini_request_body()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let body_bytes = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let meta = &json["_meta"];

    assert_eq!(meta["has_key_in_query"], true, "Gemini should use query param for auth");
    assert_eq!(meta["has_x_api_key_header"], false, "Gemini should NOT use x-api-key header");
}

#[tokio::test]
async fn gemini_fast_path_redacts_secrets() {
    let upstream_json = serde_json::json!({
        "candidates": [{"content": {"parts": [{"text": "Secret: sk-1234567890abcdef1234567890abcdef"}]}}],
        "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 30}
    });
    let handler = make_mock_upstream_handler(upstream_json);

    let upstream = tokio::spawn(async {
        let app = axum::Router::new().route("/{*path}", axum::routing::any(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (addr, handle)
    });

    let (addr, _handle) = upstream.await.unwrap();
    let state = build_proxy_state(&format!("http://{}", addr), ProviderKind::Gemini);
    let app = axum::Router::new()
        .route("/v1/messages", any(proxy_handler))
        .with_state(state);

    let request = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .body(Body::from(gemini_request_body()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let body_bytes = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024).await.unwrap();
    let body_str = String::from_utf8_lossy(&body_bytes);

    assert!(!body_str.contains("sk-1234567890abcdef1234567890abcdef"), "API key should be redacted");
}

// =============================================================================
// Cross-Provider Tests
// =============================================================================

#[tokio::test]
async fn both_providers_return_correlation_id() {
    for provider_kind in [ProviderKind::Anthropic, ProviderKind::Gemini] {
        let upstream_json = gemini_upstream_response();
        let handler = make_mock_upstream_handler(upstream_json);

        let upstream = tokio::spawn(async {
            let app = axum::Router::new().route("/{*path}", axum::routing::any(handler));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
            (addr, handle)
        });

        let (addr, _handle) = upstream.await.unwrap();
        let state = build_proxy_state(&format!("http://{}", addr), provider_kind);
        let app = axum::Router::new()
            .route("/v1/messages", any(proxy_handler))
            .with_state(state);

        let body = match provider_kind {
            ProviderKind::Anthropic => anthropic_request_body(),
            ProviderKind::Gemini => gemini_request_body(),
            ProviderKind::OpenCode => opencode_request_body(),
            ProviderKind::Ollama => ollama_request_body(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert!(
            response.headers().get("x-controlplane-correlation-id").is_some(),
            "Provider {:?} should set correlation ID header",
            provider_kind
        );
        let _ = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024).await;
    }
}

#[tokio::test]
async fn both_providers_return_latency_header() {
    for provider_kind in [ProviderKind::Anthropic, ProviderKind::Gemini] {
        let upstream_json = gemini_upstream_response();
        let handler = make_mock_upstream_handler(upstream_json);

        let upstream = tokio::spawn(async {
            let app = axum::Router::new().route("/{*path}", axum::routing::any(handler));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
            (addr, handle)
        });

        let (addr, _handle) = upstream.await.unwrap();
        let state = build_proxy_state(&format!("http://{}", addr), provider_kind);
        let app = axum::Router::new()
            .route("/v1/messages", any(proxy_handler))
            .with_state(state);

        let body = match provider_kind {
            ProviderKind::Anthropic => anthropic_request_body(),
            ProviderKind::Gemini => gemini_request_body(),
            ProviderKind::OpenCode => opencode_request_body(),
            ProviderKind::Ollama => ollama_request_body(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert!(
            response.headers().get("x-controlplane-latency-ms").is_some(),
            "Provider {:?} should set latency header",
            provider_kind
        );
        let _ = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024).await;
    }
}

#[tokio::test]
async fn both_providers_pass_through_clean_content() {
    for provider_kind in [ProviderKind::Anthropic, ProviderKind::Gemini] {
        let upstream_json = serde_json::json!({
            "candidates": [{"content": {"parts": [{"text": "All good here!"}]}}],
            "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 10}
        });
        let handler = make_mock_upstream_handler(upstream_json);

        let upstream = tokio::spawn(async {
            let app = axum::Router::new().route("/{*path}", axum::routing::any(handler));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
            (addr, handle)
        });

        let (addr, _handle) = upstream.await.unwrap();
        let state = build_proxy_state(&format!("http://{}", addr), provider_kind);
        let app = axum::Router::new()
            .route("/v1/messages", any(proxy_handler))
            .with_state(state);

        let body = match provider_kind {
            ProviderKind::Anthropic => anthropic_request_body(),
            ProviderKind::Gemini => gemini_request_body(),
            ProviderKind::OpenCode => opencode_request_body(),
            ProviderKind::Ollama => ollama_request_body(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "Provider {:?} should pass clean content", provider_kind);
        let _ = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024).await;
    }
}
