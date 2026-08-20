use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{get, put};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use crate::auth::auth_middleware;
use crate::sse::{verdict_stream_handler, SseBroadcaster};

/// Shared state for the dashboard API.
#[derive(Clone)]
pub struct DashboardState {
    pub pool: PgPool,
    pub broadcaster: SseBroadcaster,
}

/// Build the full dashboard API router with CORS and auth.
pub fn dashboard_router(state: DashboardState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_routes = Router::new()
        // SSE live stream
        .route("/api/v1/verdicts/stream", get(sse_handler))
        // REST endpoints
        .route("/api/v1/verdicts/recent", get(recent_verdicts))
        .route("/api/v1/stats/overview", get(stats_overview))
        .route("/api/v1/apps", get(list_apps))
        // Policies
        .route("/api/v1/policies/{app_id}", get(get_policy).put(update_policy))
        // Escalations
        .route("/api/v1/escalations", get(list_escalations))
        .route("/api/v1/escalations/{id}/resolve", axum::routing::post(resolve_escalation))
        // Health
        .route("/health", get(health))
        .route("/ready", get(ready))
        .with_state(Arc::new(state))
        .layer(middleware::from_fn(auth_middleware))
        .layer(cors);

    api_routes
}

// === Types ===

#[derive(Deserialize)]
struct RecentVerdictsParams {
    limit: Option<i64>,
    app_id: Option<Uuid>,
    outcome: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
struct VerdictRow {
    id: Uuid,
    call_id: Uuid,
    app_id: Option<Uuid>,
    axis: String,
    path: String,
    outcome: String,
    confidence: f32,
    reason: String,
    check_name: String,
    latency_ms: Option<i32>,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct RecentVerdictsResponse {
    verdicts: Vec<VerdictRow>,
    total: usize,
}

#[derive(Serialize)]
struct StatsOverview {
    total_calls_24h: i64,
    total_verdicts_24h: i64,
    blocks_24h: i64,
    escalations_24h: i64,
    passes_24h: i64,
    open_escalations: i64,
    avg_fast_path_latency_ms: f64,
    top_blocked_axes: Vec<AxisCount>,
}

#[derive(Serialize, sqlx::FromRow)]
struct AxisCount {
    axis: String,
    count: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct AppRow {
    id: Uuid,
    name: String,
    team_id: Uuid,
    created_at: DateTime<Utc>,
}

// === Handlers ===

async fn sse_handler(
    State(state): State<Arc<DashboardState>>,
) -> impl axum::response::IntoResponse {
    verdict_stream_handler(state.broadcaster.clone()).await
}

async fn recent_verdicts(
    State(state): State<Arc<DashboardState>>,
    Query(params): Query<RecentVerdictsParams>,
) -> Result<Json<RecentVerdictsResponse>, (StatusCode, String)> {
    let limit = params.limit.unwrap_or(50).min(200);

    let verdicts = if let Some(app_id) = params.app_id {
        if let Some(outcome) = &params.outcome {
            sqlx::query_as::<_, VerdictRow>(
                "SELECT id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, latency_ms, created_at \
                 FROM verdicts WHERE app_id = $1 AND outcome = $2 ORDER BY created_at DESC LIMIT $3"
            )
            .bind(app_id)
            .bind(outcome)
            .bind(limit)
            .fetch_all(&state.pool)
            .await
        } else {
            sqlx::query_as::<_, VerdictRow>(
                "SELECT id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, latency_ms, created_at \
                 FROM verdicts WHERE app_id = $1 ORDER BY created_at DESC LIMIT $2"
            )
            .bind(app_id)
            .bind(limit)
            .fetch_all(&state.pool)
            .await
        }
    } else if let Some(outcome) = &params.outcome {
        sqlx::query_as::<_, VerdictRow>(
            "SELECT id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, latency_ms, created_at \
             FROM verdicts WHERE outcome = $1 ORDER BY created_at DESC LIMIT $2"
        )
        .bind(outcome)
        .bind(limit)
        .fetch_all(&state.pool)
        .await
    } else {
        sqlx::query_as::<_, VerdictRow>(
            "SELECT id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, latency_ms, created_at \
             FROM verdicts ORDER BY created_at DESC LIMIT $1"
        )
        .bind(limit)
        .fetch_all(&state.pool)
        .await
    }.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total = verdicts.len();
    Ok(Json(RecentVerdictsResponse { verdicts, total }))
}

async fn stats_overview(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<StatsOverview>, (StatusCode, String)> {
    let now_minus_24h = Utc::now() - chrono::Duration::hours(24);

    let total_calls: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM intercepted_calls WHERE created_at > $1"
    )
    .bind(now_minus_24h)
    .fetch_one(&state.pool)
    .await
    .unwrap_or((0,));

    let total_verdicts: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM verdicts WHERE created_at > $1"
    )
    .bind(now_minus_24h)
    .fetch_one(&state.pool)
    .await
    .unwrap_or((0,));

    let blocks: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM verdicts WHERE outcome = 'block' AND created_at > $1"
    )
    .bind(now_minus_24h)
    .fetch_one(&state.pool)
    .await
    .unwrap_or((0,));

    let escalations: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM verdicts WHERE outcome = 'escalate' AND created_at > $1"
    )
    .bind(now_minus_24h)
    .fetch_one(&state.pool)
    .await
    .unwrap_or((0,));

    let passes: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM verdicts WHERE outcome = 'pass' AND created_at > $1"
    )
    .bind(now_minus_24h)
    .fetch_one(&state.pool)
    .await
    .unwrap_or((0,));

    let open_escalations: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM escalation_cases WHERE status = 'open'"
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or((0,));

    let avg_latency: (Option<f64>,) = sqlx::query_as(
        "SELECT AVG(latency_ms::double precision) FROM verdicts WHERE path = 'fast' AND created_at > $1"
    )
    .bind(now_minus_24h)
    .fetch_one(&state.pool)
    .await
    .unwrap_or((None,));

    let top_blocked_axes: Vec<AxisCount> = sqlx::query_as(
        "SELECT axis, COUNT(*) as count FROM verdicts \
         WHERE outcome = 'block' AND created_at > $1 \
         GROUP BY axis ORDER BY count DESC LIMIT 5"
    )
    .bind(now_minus_24h)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    Ok(Json(StatsOverview {
        total_calls_24h: total_calls.0,
        total_verdicts_24h: total_verdicts.0,
        blocks_24h: blocks.0,
        escalations_24h: escalations.0,
        passes_24h: passes.0,
        open_escalations: open_escalations.0,
        avg_fast_path_latency_ms: avg_latency.0.unwrap_or(0.0),
        top_blocked_axes,
    }))
}

async fn list_apps(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<Vec<AppRow>>, (StatusCode, String)> {
    let apps: Vec<AppRow> = sqlx::query_as(
        "SELECT id, name, team_id, created_at FROM apps ORDER BY name"
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(apps))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "dashboard-api"
    }))
}

async fn ready(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    sqlx::query("SELECT 1")
        .execute(&state.pool)
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, format!("DB not ready: {e}")))?;

    Ok(Json(serde_json::json!({
        "status": "ready",
        "service": "dashboard-api"
    })))
}

// === Policy Handlers ===

#[derive(Deserialize)]
struct PolicyUpdate {
    block_threshold: Option<f64>,
    escalate_threshold: Option<f64>,
    max_tokens_per_request: Option<i32>,
    retry_max_count: Option<i32>,
    unsafe_keywords: Option<Vec<String>>,
}

async fn get_policy(
    State(state): State<Arc<DashboardState>>,
    Path(app_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let parsed_id = Uuid::parse_str(&app_id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid app_id".to_string()))?;

    let rows: Vec<(Uuid, String, serde_json::Value, bool)> = sqlx::query_as(
        "SELECT id, axis, threshold_config, is_active FROM policies WHERE app_id = $1 AND is_active = true"
    )
    .bind(parsed_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    Ok(Json(serde_json::json!({ "policies": rows.iter().map(|(id, axis, config, active)| {
        serde_json::json!({ "id": id, "axis": axis, "config": config, "is_active": active })
    }).collect::<Vec<_>>() })))
}

async fn update_policy(
    State(state): State<Arc<DashboardState>>,
    Path(app_id): Path<String>,
    Json(body): Json<PolicyUpdate>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let parsed_id = Uuid::parse_str(&app_id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid app_id".to_string()))?;

    let config = serde_json::json!({
        "block_threshold": body.block_threshold.unwrap_or(0.9),
        "escalate_threshold": body.escalate_threshold.unwrap_or(0.6),
        "max_tokens_per_request": body.max_tokens_per_request.unwrap_or(4000),
        "retry_max_count": body.retry_max_count.unwrap_or(3),
        "unsafe_keywords": body.unsafe_keywords.unwrap_or_default(),
    });

    let axes = ["performance", "cost", "responsibility"];
    for axis in axes {
        sqlx::query(
            "INSERT INTO policies (id, app_id, axis, threshold_config, version, is_active, created_at) \
             VALUES ($1, $2, $3, $4, 1, true, NOW()) \
             ON CONFLICT ON CONSTRAINT policies_app_id_axis_version_key \
             DO UPDATE SET threshold_config = $4, updated_at = NOW()"
        )
        .bind(Uuid::now_v7())
        .bind(parsed_id)
        .bind(axis)
        .bind(&config)
        .execute(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error saving policy for axis {}: {e}", axis)))?;
    }

    Ok(Json(serde_json::json!({ "status": "saved", "app_id": app_id })))
}

// === Escalation Handlers ===

#[derive(Deserialize)]
struct EscalationListParams {
    status: Option<String>,
    limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
struct EscalationRow {
    id: Uuid,
    call_id: Uuid,
    verdict_id: Uuid,
    app_id: Uuid,
    reason: String,
    status: String,
    assigned_to: Option<String>,
    resolution: Option<String>,
    resolution_reason: Option<String>,
    created_at: DateTime<Utc>,
    resolved_at: Option<DateTime<Utc>>,
}

async fn list_escalations(
    State(state): State<Arc<DashboardState>>,
    Query(params): Query<EscalationListParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let limit = params.limit.unwrap_or(50).min(200);
    let status = params.status.unwrap_or_else(|| "open".to_string());

    let cases: Vec<EscalationRow> = sqlx::query_as(
        "SELECT id, call_id, verdict_id, app_id, reason, status, assigned_to, resolution, resolution_reason, created_at, resolved_at \
         FROM escalation_cases WHERE status = $1 ORDER BY created_at DESC LIMIT $2"
    )
    .bind(&status)
    .bind(limit)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(serde_json::json!({ "cases": cases, "total": cases.len() })))
}

#[derive(Deserialize)]
struct ResolveBody {
    action: String,
    reason: Option<String>,
}

async fn resolve_escalation(
    State(state): State<Arc<DashboardState>>,
    Path(id): Path<String>,
    Json(body): Json<ResolveBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let parsed_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid escalation id".to_string()))?;

    sqlx::query(
        "UPDATE escalation_cases SET status = 'resolved', resolution = $1, resolution_reason = $2, resolved_at = NOW() WHERE id = $3"
    )
    .bind(&body.action)
    .bind(&body.reason)
    .bind(parsed_id)
    .execute(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(serde_json::json!({ "status": "resolved", "id": id, "action": body.action })))
}
