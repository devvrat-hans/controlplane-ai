use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use sqlx::PgPool;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use controlplane_common::events::{EventEnvelope, ShadowAnalysisRequest, VerdictPayload};
use controlplane_common::models::Verdict;
use controlplane_common::provider::UpstreamProvider;
use controlplane_common::types::{Outcome, Path as VerdictPath};
use controlplane_common::events::subjects;
use controlplane_fast_path::FastPathEngine;
use controlplane_platform::messaging::EventPublisher;

pub struct ProxyState {
    pub upstream_base_url: String,
    pub upstream_api_key: String,
    pub default_model: String,
    pub provider: Box<dyn UpstreamProvider>,
    pub http_client: reqwest::Client,
    pub fast_path: Arc<FastPathEngine>,
    pub publisher: Arc<dyn EventPublisher>,
    pub pool: Option<PgPool>,
    pub default_app_id: Uuid,
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

    // Extract model from request body (provider-aware)
    let model_in_body = state.provider.extract_model(&request_body)
        .unwrap_or_else(|| state.default_model.clone());

    // Extract session_id for multi-turn tracking (R2.1)
    // Priority: explicit "session_id" field > X-Session-Id header > derived from messages hash
    let session_id: Option<Uuid> = extract_session_id(&request_body, &headers);

    // Extract app_id: explicit "app_id" in body > X-App-Id header > default
    let app_id: Uuid = extract_app_id(&request_body, &headers)
        .unwrap_or(state.default_app_id);

    // Extract profile_id (0-5) for Agent-Internal (App1) regulatory profile override
    let profile_id: Option<u8> = extract_profile_id(&request_body);
    if let Some(pid) = profile_id {
        info!(
            correlation_id = %correlation_id,
            profile_id = pid,
            profile_name = profile_id_to_name(pid),
            "Using regulatory profile override"
        );
    }

    // Forward to upstream
    let upstream_path = state.provider.rewrite_path(&path, &model_in_body);
    let upstream_url = format!("{}{}", state.upstream_base_url, upstream_path);
    let upstream_start = Instant::now();

    let mut upstream_request = state.http_client.request(method.clone(), &upstream_url);

    // Forward relevant headers (content-type, etc.) — skip auth/host/transfer
    for (key, value) in headers.iter() {
        let key_str = key.as_str().to_lowercase();
        match key_str.as_str() {
            "host" | "connection" | "transfer-encoding" | "content-length"
            | "x-api-key" | "anthropic-version" => continue,
            _ => {
                upstream_request = upstream_request.header(key.clone(), value.clone());
            }
        }
    }

    // Apply provider-specific auth
    upstream_request = state.provider.apply_auth(upstream_request, &state.upstream_api_key);

    upstream_request = upstream_request.body(request_body.clone());

    let upstream_response = match upstream_request.send().await {
        Ok(resp) => resp,
        Err(e) => {
            // Strip the URL: some providers (Gemini) carry the API key as a query
            // parameter, and reqwest's error text embeds the full request URL.
            let e = e.without_url();
            error!(
                correlation_id = %correlation_id,
                error = %e,
                "Upstream request failed"
            );
            return (StatusCode::BAD_GATEWAY, format!("Upstream error: {e}")).into_response();
        }
    };

    // Fractional ms for exact display; the integer column keeps whole ms for older readers.
    let upstream_elapsed_ms = upstream_start.elapsed().as_secs_f64() * 1000.0;
    let upstream_latency_ms = upstream_elapsed_ms as i32;
    let upstream_status = upstream_response.status();
    let upstream_headers = upstream_response.headers().clone();

    // Read response body
    let response_body = match upstream_response.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            let e = e.without_url();
            error!(
                correlation_id = %correlation_id,
                error = %e,
                "Failed to read upstream response body"
            );
            return (StatusCode::BAD_GATEWAY, "Failed to read upstream response").into_response();
        }
    };

    // Extract token usage from response (provider-aware)
    let (input_tokens, output_tokens) = state.provider.extract_token_usage(&response_body);

    // Look up per-app cost cap from DB policies (or from profile if profile_id provided for App1)
    let app_max_tokens: Option<i32> = if let Some(pool) = state.pool.as_ref() {
        if let Some(pid) = profile_id {
            // profile_id provided — look up from policy_profiles table
            let profile_name = profile_id_to_name(pid);
            sqlx::query_scalar::<_, i32>(
                "SELECT (default_thresholds->'cost'->>'max_tokens_per_request')::int FROM policy_profiles WHERE id = $1 LIMIT 1"
            )
            .bind(profile_name)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()
        } else {
            // No profile_id — use app's own policy (which may link to a default profile)
            sqlx::query_scalar::<_, i32>(
                "SELECT (threshold_config->>'max_tokens_per_request')::int FROM policies WHERE app_id = $1 AND axis = 'cost' AND is_active = true LIMIT 1"
            )
            .bind(app_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()
        }
    } else {
        None
    };

    // --- Fast-path checks (synchronous, must complete before delivery) ---
    let fast_path_start = Instant::now();
    let fast_path_result = run_fast_path_safe(
        &state.fast_path, &response_body, output_tokens, &request_body, correlation_id, app_max_tokens,
    ).await;
    let fast_path_elapsed_ms = fast_path_start.elapsed().as_secs_f64() * 1000.0;
    let fast_path_latency_ms = fast_path_elapsed_ms as i32;

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

    // Persist intercepted_call to DB (must happen before verdict events)
    let has_tool_use = fast_path_result.has_tool_use;
    if let Some(ref pool) = state.pool {
        let req_json: Option<serde_json::Value> = serde_json::from_slice(&request_body).ok();
        let resp_json: Option<serde_json::Value> = serde_json::from_slice(&response_body).ok();
        if let Err(e) = sqlx::query(
            "INSERT INTO intercepted_calls \
             (id, correlation_id, app_id, model, request_payload, response_payload, \
              token_count_input, token_count_output, upstream_latency_ms, fast_path_latency_ms, session_id, has_tool_use, \
              upstream_latency_us, fast_path_latency_us, fast_path_check_timings_us, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, NOW()) \
             ON CONFLICT (id) DO NOTHING"
        )
        .bind(correlation_id)
        .bind(correlation_id)
        .bind(app_id)
        .bind(&model_in_body)
        .bind(&req_json)
        .bind(&resp_json)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(upstream_latency_ms)
        .bind(fast_path_latency_ms)
        .bind(session_id)
        .bind(has_tool_use)
        .bind(ms_to_us(upstream_elapsed_ms))
        .bind(ms_to_us(fast_path_elapsed_ms))
        .bind(&fast_path_result.check_timings_us)
        .execute(pool)
        .await {
            warn!(error = %e, correlation_id = %correlation_id, "Failed to persist intercepted_call");
        }
    }

    // Publish verdicts to SSE (for all outcomes including pass/block)
    let publisher_for_verdicts = state.publisher.clone();
    let verdicts_for_stream: Vec<Verdict> = if fast_path_result.verdicts.is_empty() {
        // No individual check triggered — create pass verdicts for each axis
        // so statistics are distributed evenly across responsibility/performance/cost
        use controlplane_common::types::Axis;
        vec![
            Verdict::new(
                correlation_id, Axis::Responsibility, VerdictPath::Fast,
                Outcome::Pass, 1.0, "Responsibility checks passed", "fast-path-summary",
            ).with_duration_precise(fast_path_elapsed_ms),
            Verdict::new(
                correlation_id, Axis::Performance, VerdictPath::Fast,
                Outcome::Pass, 1.0, "Performance checks passed", "fast-path-summary",
            ).with_duration(0),
            Verdict::new(
                correlation_id, Axis::Cost, VerdictPath::Fast,
                Outcome::Pass, 1.0, "Cost checks passed", "fast-path-summary",
            ).with_duration(0),
        ]
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
                    app_id,
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
            &state.publisher, correlation_id, app_id, &model_in_body, &request_body, &response_body,
            input_tokens, output_tokens, upstream_latency_ms, fast_path_latency_ms,
        ).await;

        return (StatusCode::FORBIDDEN, axum::Json(block_body)).into_response();
    }

    // --- Async: publish to shadow-path and audit (non-blocking) ---
    let publisher = state.publisher.clone();
    let req_body_clone = request_body.clone();
    let resp_body_clone = response_body.clone();
    let model_for_capture = model_in_body.clone();

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
            app_id,
            shadow_req,
        );

        if let Ok(bytes) = envelope.to_bytes() {
            if let Err(e) = publisher.publish(subjects::INTERCEPT_SHADOW, &bytes).await {
                warn!(error = %e, "Failed to publish shadow analysis request");
            }
        }

        // Captured-call event for cost accounting (blocked calls publish theirs above).
        publish_call_async(
            &publisher, correlation_id, app_id, &model_for_capture, &req_body_clone, &resp_body_clone,
            input_tokens, output_tokens, upstream_latency_ms, fast_path_latency_ms,
        ).await;
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
    call_id: Uuid,
    app_max_tokens: Option<i32>,
) -> FastPathSafeResult {
    let body_str = String::from_utf8_lossy(response_body);
    // Hash only the user message content + session_id so retry detection works
    // (identical messages in the same session produce the same key)
    let session_key = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        if let Ok(body) = serde_json::from_slice::<serde_json::Value>(request_body) {
            if let Some(sid) = body.get("session_id").and_then(|v| v.as_str()) {
                sid.hash(&mut hasher);
            }
            if let Some(messages) = body.get("messages").and_then(|v| v.as_array()) {
                if let Some(last_msg) = messages.last() {
                    if let Some(content) = last_msg.get("content").and_then(|c| c.as_str()) {
                        content.hash(&mut hasher);
                    }
                }
            }
        } else {
            request_body.hash(&mut hasher);
        }
        Some(hasher.finish())
    };

    let timeout_result = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        tokio::task::spawn_blocking({
            let engine = engine.clone();
            let body = body_str.to_string();
            move || engine.evaluate_with_app_context(&body, output_tokens, session_key, app_max_tokens)
        }),
    )
    .await;

    match timeout_result {
        Ok(Ok(result)) => {
            let verdicts: Vec<Verdict> = result.verdicts.iter().map(|v| {
                Verdict::new(
                    call_id,
                    v.axis,
                    VerdictPath::Fast,
                    v.outcome,
                    v.confidence,
                    &v.reason,
                    &v.check_name,
                ).with_duration_precise(v.duration_ms)
            }).collect();
            let check_timings_us = serde_json::Value::Object(
                result.check_timings.iter()
                    .map(|t| (t.check_name.to_string(), serde_json::json!(ms_to_us(t.duration_ms))))
                    .collect(),
            );

            FastPathSafeResult {
                outcome: result.outcome,
                edits: result.edits,
                verdicts,
                block_reason: result.verdicts.iter()
                    .find(|v| v.outcome == Outcome::Block)
                    .map(|v| v.reason.clone()),
                has_tool_use: result.has_tool_use,
                check_timings_us: Some(check_timings_us),
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
    has_tool_use: bool,
    /// `{check_name: microseconds}` for every fast-path check that ran; `None`
    /// when the fast path failed open (panic/timeout) and nothing was measured.
    check_timings_us: Option<serde_json::Value>,
}

/// Fractional milliseconds → whole microseconds.
fn ms_to_us(ms: f64) -> i64 {
    (ms.max(0.0) * 1000.0).round() as i64
}

impl FastPathSafeResult {
    fn pass() -> Self {
        Self {
            outcome: Outcome::Pass,
            edits: Vec::new(),
            verdicts: Vec::new(),
            block_reason: None,
            has_tool_use: false,
            check_timings_us: None,
        }
    }
}

/// Extract session_id for multi-turn conversation tracking.
/// Checks: 1) explicit "session_id" in request body, 2) X-Session-Id header,
/// 3) derives a stable ID from the first user message (for repeat conversations).
fn extract_session_id(request_body: &[u8], headers: &axum::http::HeaderMap) -> Option<Uuid> {
    // 1. Check for explicit session_id in request body
    if let Ok(body) = serde_json::from_slice::<serde_json::Value>(request_body) {
        if let Some(sid) = body.get("session_id").and_then(|v| v.as_str()) {
            if let Ok(parsed) = Uuid::parse_str(sid) {
                return Some(parsed);
            }
        }
    }

    // 2. Check X-Session-Id header
    if let Some(header_val) = headers.get("x-session-id") {
        if let Ok(s) = header_val.to_str() {
            if let Ok(parsed) = Uuid::parse_str(s) {
                return Some(parsed);
            }
        }
    }

    // 3. If messages array has >1 message, derive session from first user message hash
    // This groups multi-turn conversations that share the same opening message
    if let Ok(body) = serde_json::from_slice::<serde_json::Value>(request_body) {
        if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
            if messages.len() > 1 {
                if let Some(first_content) = messages.first()
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    first_content.hash(&mut hasher);
                    let hash = hasher.finish();
                    let bytes = hash.to_le_bytes();
                    let mut uuid_bytes = [0u8; 16];
                    uuid_bytes[..8].copy_from_slice(&bytes);
                    uuid_bytes[8..].copy_from_slice(&bytes);
                    uuid_bytes[6] = (uuid_bytes[6] & 0x0F) | 0x40; // version 4
                    uuid_bytes[8] = (uuid_bytes[8] & 0x3F) | 0x80; // variant
                    return Some(Uuid::from_bytes(uuid_bytes));
                }
            }
        }
    }

    None
}

/// Extract profile_id (0-5) from request body for regulatory profile selection.
/// Only meaningful for Agent-Internal (App1).
fn extract_profile_id(request_body: &[u8]) -> Option<u8> {
    if let Ok(body) = serde_json::from_slice::<serde_json::Value>(request_body) {
        if let Some(pid) = body.get("profile_id").and_then(|v| v.as_u64()) {
            if pid <= 5 {
                return Some(pid as u8);
            }
        }
    }
    None
}

/// Map profile_id (0-5) to the database profile name.
fn profile_id_to_name(id: u8) -> &'static str {
    match id {
        0 => "us-financial",
        1 => "eu-financial",
        2 => "us-healthcare",
        3 => "india-general",
        4 => "eu-general",
        5 => "global-internal",
        _ => "eu-financial", // fallback to default
    }
}

/// Extract app_id from the request.
/// Checks: 1) explicit "app_id" in request body, 2) X-App-Id header.
fn extract_app_id(request_body: &[u8], headers: &axum::http::HeaderMap) -> Option<Uuid> {
    // 1. Check for explicit app_id in request body
    if let Ok(body) = serde_json::from_slice::<serde_json::Value>(request_body) {
        if let Some(aid) = body.get("app_id").and_then(|v| v.as_str()) {
            if let Ok(parsed) = Uuid::parse_str(aid) {
                return Some(parsed);
            }
        }
    }

    // 2. Check X-App-Id header
    if let Some(header_val) = headers.get("x-app-id") {
        if let Ok(s) = header_val.to_str() {
            if let Ok(parsed) = Uuid::parse_str(s) {
                return Some(parsed);
            }
        }
    }

    None
}

async fn publish_call_async(
    publisher: &Arc<dyn EventPublisher>,
    correlation_id: Uuid,
    app_id: Uuid,
    model: &str,
    _request_body: &[u8],
    _response_body: &[u8],
    _input_tokens: Option<i32>,
    _output_tokens: Option<i32>,
    _upstream_latency_ms: i32,
    _fast_path_latency_ms: i32,
) {
    let payload = controlplane_common::events::InterceptCapturedPayload {
        call_id: correlation_id,
        model: model.to_string(),
        token_count_input: _input_tokens,
        token_count_output: _output_tokens,
        upstream_latency_ms: Some(_upstream_latency_ms),
    };

    let envelope = EventEnvelope::new(
        subjects::INTERCEPT_CAPTURED,
        correlation_id,
        app_id,
        payload,
    );

    if let Ok(bytes) = envelope.to_bytes() {
        let _ = publisher.publish(subjects::INTERCEPT_CAPTURED, &bytes).await;
    }
}
