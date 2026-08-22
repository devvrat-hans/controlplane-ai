use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use controlplane_common::events::{EventEnvelope, ShadowAnalysisRequest, VerdictPayload};
use controlplane_common::models::Verdict;
use controlplane_common::types::{Outcome, Path as VerdictPath};
use controlplane_common::events::subjects;
use controlplane_fast_path::FastPathEngine;
use controlplane_platform::messaging::EventPublisher;

use crate::token_counter::extract_token_usage;

pub struct ProxyState {
    pub upstream_base_url: String,
    pub http_client: reqwest::Client,
    pub fast_path: Arc<FastPathEngine>,
    pub publisher: Arc<dyn EventPublisher>,
}

pub async fn proxy_handler(
    State(state): State<Arc<ProxyState>>,
    request: Request<Body>,
) -> impl IntoResponse {
    let correlation_id = Uuid::now_v7();
    let start = Instant::now();

    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let headers = request.headers().clone();

    debug!(
        correlation_id = %correlation_id,
        method = %method,
        path = %path,
        "Proxy: intercepting request"
    );

    // Extract request body
    let request_body = match axum::body::to_bytes(request.into_body(), 10 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(e) => {
            error!(error = %e, "Failed to read request body");
            return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
        }
    };

    // Forward to upstream
    let upstream_url = format!("{}{}", state.upstream_base_url, path);
    let upstream_start = Instant::now();

    let mut upstream_request = state.http_client.request(method, &upstream_url);

    // Forward relevant headers (auth, content-type, anthropic-version)
    for (key, value) in headers.iter() {
        let key_str = key.as_str().to_lowercase();
        match key_str.as_str() {
            "host" | "connection" | "transfer-encoding" | "content-length" => continue,
            _ => {
                upstream_request = upstream_request.header(key.clone(), value.clone());
            }
        }
    }

    upstream_request = upstream_request.body(request_body.clone());

    let upstream_response = match upstream_request.send().await {
        Ok(resp) => resp,
        Err(e) => {
            error!(
                correlation_id = %correlation_id,
                error = %e,
                "Upstream request failed"
            );
            return (StatusCode::BAD_GATEWAY, format!("Upstream error: {e}")).into_response();
        }
    };

    let upstream_latency_ms = upstream_start.elapsed().as_millis() as i32;
    let upstream_status = upstream_response.status();
    let upstream_headers = upstream_response.headers().clone();

    // Read response body
    let response_body = match upstream_response.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            error!(
                correlation_id = %correlation_id,
                error = %e,
                "Failed to read upstream response body"
            );
            return (StatusCode::BAD_GATEWAY, "Failed to read upstream response").into_response();
        }
    };

    // Extract token usage from response
    let (input_tokens, output_tokens) = extract_token_usage(&response_body);

    // --- Fast-path checks (synchronous, must complete before delivery) ---
    let fast_path_start = Instant::now();
    let fast_path_result = run_fast_path_safe(
        &state.fast_path, &response_body, output_tokens, &request_body,
    ).await;
    let fast_path_latency_ms = fast_path_start.elapsed().as_millis() as i32;

    // Apply edits if any
    let final_response_body = if !fast_path_result.edits.is_empty() {
        let mut body_str = String::from_utf8_lossy(&response_body).to_string();
        for edit in &fast_path_result.edits {
            body_str = body_str.replace(&edit.original, &edit.replacement);
        }
        Bytes::from(body_str)
    } else {
        response_body.clone()
    };

    // Publish verdicts to SSE (for all outcomes including pass/block)
    let publisher_for_verdicts = state.publisher.clone();
    let verdicts_for_stream: Vec<Verdict> = if fast_path_result.verdicts.is_empty() {
        vec![Verdict::new(
            correlation_id,
            controlplane_common::types::Axis::Responsibility,
            VerdictPath::Fast,
            fast_path_result.outcome,
            1.0,
            match fast_path_result.outcome {
                Outcome::Pass => "All checks passed",
                Outcome::Block => fast_path_result.block_reason.as_deref().unwrap_or("Blocked"),
                _ => "Processed",
            },
            "fast-path-summary",
        ).with_duration(fast_path_latency_ms)]
    } else {
        fast_path_result.verdicts.clone()
    };

    tokio::spawn({
        let verdicts = verdicts_for_stream;
        async move {
            for verdict in verdicts {
                let envelope = EventEnvelope::new(
                    subjects::VERDICT_FAST,
                    correlation_id,
                    Uuid::nil(),
                    VerdictPayload { verdict },
                );
                if let Ok(bytes) = envelope.to_bytes() {
                    let _ = publisher_for_verdicts.publish(subjects::VERDICT_FAST, &bytes).await;
                }
            }
        }
    });

    // Check if blocked
    if fast_path_result.outcome == Outcome::Block {
        info!(
            correlation_id = %correlation_id,
            reason = fast_path_result.block_reason.as_deref().unwrap_or("unknown"),
            "Request BLOCKED by fast-path"
        );

        let block_body = serde_json::json!({
            "error": {
                "code": "blocked_by_policy",
                "message": fast_path_result.block_reason.as_deref().unwrap_or("Blocked by ControlPlane policy"),
                "correlation_id": correlation_id.to_string()
            }
        });

        publish_call_async(
            &state.publisher, correlation_id, &request_body, &response_body,
            input_tokens, output_tokens, upstream_latency_ms, fast_path_latency_ms,
        ).await;

        return (StatusCode::FORBIDDEN, axum::Json(block_body)).into_response();
    }

    // --- Async: publish to shadow-path and audit (non-blocking) ---
    let publisher = state.publisher.clone();
    let req_body_clone = request_body.clone();
    let resp_body_clone = response_body.clone();

    tokio::spawn(async move {
        let shadow_req = ShadowAnalysisRequest {
            call_id: correlation_id,
            request_payload: serde_json::from_slice(&req_body_clone).ok(),
            response_payload: serde_json::from_slice(&resp_body_clone).ok(),
            model: "unknown".to_string(),
            token_count_output: output_tokens,
        };

        let envelope = EventEnvelope::new(
            subjects::INTERCEPT_SHADOW,
            correlation_id,
            Uuid::nil(),
            shadow_req,
        );

        if let Ok(bytes) = envelope.to_bytes() {
            if let Err(e) = publisher.publish(subjects::INTERCEPT_SHADOW, &bytes).await {
                warn!(error = %e, "Failed to publish shadow analysis request");
            }
        }
    });

    let total_latency_ms = start.elapsed().as_millis();
    info!(
        correlation_id = %correlation_id,
        upstream_latency_ms,
        fast_path_latency_ms,
        total_added_ms = fast_path_latency_ms,
        total_latency_ms = %total_latency_ms,
        outcome = %fast_path_result.outcome,
        "Proxy: request completed"
    );

    // Build response with original headers + correlation_id
    let mut response = Response::builder().status(upstream_status);

    for (key, value) in upstream_headers.iter() {
        let key_str = key.as_str().to_lowercase();
        if key_str != "transfer-encoding" && key_str != "content-length" {
            response = response.header(key.clone(), value.clone());
        }
    }

    response = response.header(
        "X-ControlPlane-Correlation-Id",
        HeaderValue::from_str(&correlation_id.to_string()).unwrap(),
    );
    response = response.header(
        "X-ControlPlane-Latency-Ms",
        HeaderValue::from_str(&fast_path_latency_ms.to_string()).unwrap(),
    );

    response
        .body(Body::from(final_response_body))
        .unwrap()
        .into_response()
}

/// Run fast-path with fail-open guarantee: if it panics or times out, pass through.
async fn run_fast_path_safe(
    engine: &FastPathEngine,
    response_body: &[u8],
    output_tokens: Option<i32>,
    request_body: &[u8],
) -> FastPathSafeResult {
    let body_str = String::from_utf8_lossy(response_body);
    let session_key = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        request_body.hash(&mut hasher);
        Some(hasher.finish())
    };

    let timeout_result = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        tokio::task::spawn_blocking({
            let engine = engine.clone();
            let body = body_str.to_string();
            move || engine.evaluate_with_context(&body, output_tokens, session_key)
        }),
    )
    .await;

    match timeout_result {
        Ok(Ok(result)) => {
            let verdicts: Vec<Verdict> = result.verdicts.iter().map(|v| {
                Verdict::new(
                    Uuid::nil(), // call_id filled later
                    v.axis,
                    VerdictPath::Fast,
                    v.outcome,
                    v.confidence,
                    &v.reason,
                    &v.check_name,
                ).with_duration(v.duration_ms as i32)
            }).collect();

            FastPathSafeResult {
                outcome: result.outcome,
                edits: result.edits,
                verdicts,
                block_reason: result.verdicts.iter()
                    .find(|v| v.outcome == Outcome::Block)
                    .map(|v| v.reason.clone()),
            }
        }
        Ok(Err(e)) => {
            warn!(error = %e, "Fast-path panicked — FAIL OPEN");
            FastPathSafeResult::pass()
        }
        Err(_) => {
            warn!("Fast-path timed out (>50ms) — FAIL OPEN");
            FastPathSafeResult::pass()
        }
    }
}

struct FastPathSafeResult {
    outcome: Outcome,
    edits: Vec<controlplane_fast_path::engine::ResponseEdit>,
    verdicts: Vec<Verdict>,
    block_reason: Option<String>,
}

impl FastPathSafeResult {
    fn pass() -> Self {
        Self {
            outcome: Outcome::Pass,
            edits: Vec::new(),
            verdicts: Vec::new(),
            block_reason: None,
        }
    }
}

async fn publish_call_async(
    publisher: &Arc<dyn EventPublisher>,
    correlation_id: Uuid,
    _request_body: &[u8],
    _response_body: &[u8],
    _input_tokens: Option<i32>,
    _output_tokens: Option<i32>,
    _upstream_latency_ms: i32,
    _fast_path_latency_ms: i32,
) {
    let payload = controlplane_common::events::InterceptCapturedPayload {
        call_id: correlation_id,
        model: "unknown".to_string(),
        token_count_input: _input_tokens,
        token_count_output: _output_tokens,
        upstream_latency_ms: Some(_upstream_latency_ms),
    };

    let envelope = EventEnvelope::new(
        subjects::INTERCEPT_CAPTURED,
        correlation_id,
        Uuid::nil(),
        payload,
    );

    if let Ok(bytes) = envelope.to_bytes() {
        let _ = publisher.publish(subjects::INTERCEPT_CAPTURED, &bytes).await;
    }
}
