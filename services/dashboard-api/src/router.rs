use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use crate::auth::auth_middleware;
use crate::in_memory_store::InMemoryVerdictStore;
use crate::sse::{verdict_stream_handler, SseBroadcaster};

/// Shared state for the dashboard API.
#[derive(Clone)]
pub struct DashboardState {
    pub pool: Option<PgPool>,
    pub broadcaster: SseBroadcaster,
    pub in_memory: InMemoryVerdictStore,
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
        .route("/api/v1/apps/{app_id}/governance", axum::routing::put(update_governance_level))
        // Policies
        .route("/api/v1/policies/{app_id}", get(get_policy).put(update_policy))
        // Escalations
        .route("/api/v1/escalations", get(list_escalations))
        .route("/api/v1/escalations/{id}/resolve", axum::routing::post(resolve_escalation))
        // Audit
        .route("/api/v1/audit", get(query_audit))
        .route("/api/v1/audit/verify", get(verify_audit_chain))
        // User profile
        .route("/api/v1/users/me", get(get_user_profile).put(update_user_profile))
        // Cost analytics
        .route("/api/v1/cost/summary", get(cost_summary))
        .route("/api/v1/cost/timeseries", get(cost_timeseries))
        .route("/api/v1/cost/anomalies", get(cost_anomalies))
        // API Keys
        .route("/api/v1/api-keys", get(list_api_keys).post(create_api_key))
        .route("/api/v1/api-keys/{id}/revoke", axum::routing::post(revoke_api_key))
        .route("/api/v1/api-keys/{id}/analytics", get(api_key_analytics))
        // Request detail
        .route("/api/v1/requests/{call_id}", get(get_request_detail))
        // Detection quality & feedback metrics (Round 2)
        .route("/api/v1/metrics/detection-quality", get(detection_quality))
        .route("/api/v1/metrics/feedback-effectiveness", get(feedback_effectiveness))
        // Session conversation thread (Round 2 — multi-turn context)
        .route("/api/v1/sessions/{call_id}/thread", get(get_session_thread))
        // Policy profiles (Round 2 — regulatory/geographic)
        .route("/api/v1/profiles", get(list_profiles))
        .route("/api/v1/policies/{app_id}/profile", axum::routing::post(apply_profile))
        // System config
        .route("/api/v1/system/config", get(get_system_config))
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
    team_id: Option<Uuid>,
    data_governance_level: String,
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
    let limit = params.limit.unwrap_or(50).min(200) as usize;
    let app_id_str = params.app_id.map(|u| u.to_string());
    let outcome_str = params.outcome.as_deref();

    // Collect DB verdicts (seed data) if available
    let mut db_verdicts: Vec<VerdictRow> = Vec::new();
    if let Some(ref pool) = state.pool {
        let db_result = if let Some(app_id) = params.app_id {
            if let Some(outcome) = &params.outcome {
                sqlx::query_as::<_, VerdictRow>(
                    "SELECT id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, COALESCE(duration_ms, latency_ms) as latency_ms, created_at \
                     FROM verdicts WHERE app_id = $1 AND outcome = $2 ORDER BY created_at DESC LIMIT $3"
                )
                .bind(app_id)
                .bind(outcome)
                .bind(limit as i64)
                .fetch_all(pool)
                .await
            } else {
                sqlx::query_as::<_, VerdictRow>(
                    "SELECT id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, COALESCE(duration_ms, latency_ms) as latency_ms, created_at \
                     FROM verdicts WHERE app_id = $1 ORDER BY created_at DESC LIMIT $2"
                )
                .bind(app_id)
                .bind(limit as i64)
                .fetch_all(pool)
                .await
            }
        } else if let Some(outcome) = &params.outcome {
            sqlx::query_as::<_, VerdictRow>(
                "SELECT id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, COALESCE(duration_ms, latency_ms) as latency_ms, created_at \
                 FROM verdicts WHERE outcome = $1 ORDER BY created_at DESC LIMIT $2"
            )
            .bind(outcome)
            .bind(limit as i64)
            .fetch_all(pool)
            .await
        } else {
            sqlx::query_as::<_, VerdictRow>(
                "SELECT id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, COALESCE(duration_ms, latency_ms) as latency_ms, created_at \
                 FROM verdicts ORDER BY created_at DESC LIMIT $1"
            )
            .bind(limit as i64)
            .fetch_all(pool)
            .await
        };

        match db_result {
            Ok(v) => db_verdicts = v,
            Err(e) => tracing::warn!(error = %e, "DB query failed, using in-memory only"),
        }
    }

    // Collect in-memory verdicts (real proxy requests)
    let mem_records = state.in_memory.recent(limit, app_id_str.as_deref(), outcome_str);
    let mem_verdicts: Vec<VerdictRow> = mem_records.into_iter().map(|r| VerdictRow {
        id: uuid::Uuid::parse_str(&r.id).unwrap_or_default(),
        call_id: uuid::Uuid::parse_str(&r.call_id).unwrap_or_default(),
        app_id: r.app_id.as_deref().and_then(|s| uuid::Uuid::parse_str(s).ok()),
        axis: r.axis,
        path: r.path,
        outcome: r.outcome,
        confidence: r.confidence,
        reason: r.reason,
        check_name: r.check_name,
        latency_ms: r.latency_ms,
        created_at: r.created_at,
    }).collect();

    // Merge: start with DB seed data, append in-memory real requests (deduplicate by call_id)
    let mut merged = db_verdicts;
    let mut seen_call_ids = std::collections::HashSet::new();
    for v in &merged {
        seen_call_ids.insert(v.call_id);
    }
    for v in mem_verdicts {
        if !seen_call_ids.contains(&v.call_id) {
            seen_call_ids.insert(v.call_id);
            merged.push(v);
        }
    }

    // Sort by created_at descending (newest first)
    merged.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    merged.truncate(limit);

    let total = merged.len();
    Ok(Json(RecentVerdictsResponse { verdicts: merged, total }))
}

async fn stats_overview(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<StatsOverview>, (StatusCode, String)> {
    // Collect DB stats (seed data)
    let mut db_total_calls = 0i64;
    let mut db_total_verdicts = 0i64;
    let mut db_blocks = 0i64;
    let mut db_escalations = 0i64;
    let mut db_passes = 0i64;
    let mut db_open_escalations = 0i64;
    let mut db_avg_latency = 0.0f64;
    let mut db_top_blocked_axes: Vec<AxisCount> = Vec::new();
    let mut has_db_data = false;

    if let Some(ref pool) = state.pool {
        let now_minus_24h = Utc::now() - chrono::Duration::hours(24);

        let total_verdicts: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM verdicts WHERE created_at > $1"
        )
        .bind(now_minus_24h)
        .fetch_one(pool)
        .await
        .unwrap_or((0,));

        if total_verdicts.0 > 0 {
            has_db_data = true;
            db_total_verdicts = total_verdicts.0;

            let total_calls: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM intercepted_calls WHERE created_at > $1"
            )
            .bind(now_minus_24h)
            .fetch_one(pool)
            .await
            .unwrap_or((0,));
            db_total_calls = total_calls.0;

            let blocks: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM verdicts WHERE outcome = 'block' AND created_at > $1"
            )
            .bind(now_minus_24h)
            .fetch_one(pool)
            .await
            .unwrap_or((0,));
            db_blocks = blocks.0;

            let escalations: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM verdicts WHERE outcome = 'escalate' AND created_at > $1"
            )
            .bind(now_minus_24h)
            .fetch_one(pool)
            .await
            .unwrap_or((0,));
            db_escalations = escalations.0;

            let passes: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM verdicts WHERE outcome = 'pass' AND created_at > $1"
            )
            .bind(now_minus_24h)
            .fetch_one(pool)
            .await
            .unwrap_or((0,));
            db_passes = passes.0;

            let open_esc: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM escalation_cases WHERE status = 'open'"
            )
            .fetch_one(pool)
            .await
            .unwrap_or((0,));
            db_open_escalations = open_esc.0;

            let avg_latency: (Option<f64>,) = sqlx::query_as(
                "SELECT AVG(COALESCE(duration_ms, latency_ms)::double precision) FROM verdicts WHERE path = 'fast' AND created_at > $1"
            )
            .bind(now_minus_24h)
            .fetch_one(pool)
            .await
            .unwrap_or((None,));
            db_avg_latency = avg_latency.0.unwrap_or(0.0);

            db_top_blocked_axes = sqlx::query_as::<_, AxisCount>(
                "SELECT axis, COUNT(*) as count FROM verdicts \
                 WHERE outcome = 'block' AND created_at > $1 \
                 GROUP BY axis ORDER BY count DESC LIMIT 5"
            )
            .bind(now_minus_24h)
            .fetch_all(pool)
            .await
            .unwrap_or_default();
        }
    }

    // Collect in-memory stats (real proxy requests)
    let mem_stats = state.in_memory.overview();

    // Merge: combine DB seed data with in-memory real request data
    let total_calls_24h = db_total_calls + mem_stats.total_calls_24h;
    let total_verdicts_24h = db_total_verdicts + mem_stats.total_verdicts_24h;
    let blocks_24h = db_blocks + mem_stats.blocks_24h;
    let escalations_24h = db_escalations + mem_stats.escalations_24h;
    let passes_24h = db_passes + mem_stats.passes_24h;
    let open_escalations = db_open_escalations + mem_stats.open_escalations;

    // Weighted average latency (combine DB and in-memory)
    let avg_fast_path_latency_ms = if has_db_data && mem_stats.avg_fast_path_latency_ms > 0.0 {
        // Simple average of the two sources
        (db_avg_latency + mem_stats.avg_fast_path_latency_ms) / 2.0
    } else if has_db_data {
        db_avg_latency
    } else {
        mem_stats.avg_fast_path_latency_ms
    };

    // Merge top blocked axes from both sources
    let mut axis_map: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for a in &db_top_blocked_axes {
        *axis_map.entry(a.axis.clone()).or_insert(0) += a.count;
    }
    for a in &mem_stats.top_blocked_axes {
        *axis_map.entry(a.axis.clone()).or_insert(0) += a.count;
    }
    let mut top_blocked_axes: Vec<AxisCount> = axis_map
        .into_iter()
        .map(|(axis, count)| AxisCount { axis, count })
        .collect();
    top_blocked_axes.sort_by(|a, b| b.count.cmp(&a.count));
    top_blocked_axes.truncate(5);

    Ok(Json(StatsOverview {
        total_calls_24h,
        total_verdicts_24h,
        blocks_24h,
        escalations_24h,
        passes_24h,
        open_escalations,
        avg_fast_path_latency_ms,
        top_blocked_axes,
    }))
}

async fn list_apps(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<Vec<AppRow>>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;
    let apps: Vec<AppRow> = sqlx::query_as(
        "SELECT id, name, team_id, COALESCE(data_governance_level, 'medium') as data_governance_level, created_at FROM apps ORDER BY name"
    )
    .fetch_all(pool)
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
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;
    sqlx::query("SELECT 1")
        .execute(pool)
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
    // Guardrails sidecar toggles
    pii_detection: Option<bool>,
    toxicity_detection: Option<bool>,
    bias_detection: Option<bool>,
    // Fast-path toggles
    unsafe_content_enabled: Option<bool>,
    secret_detection_enabled: Option<bool>,
    // Shadow-path toggles
    prompt_injection_enabled: Option<bool>,
    hallucination_detection_enabled: Option<bool>,
    groundedness_enabled: Option<bool>,
    verbosity_enabled: Option<bool>,
    semantic_pii_enabled: Option<bool>,
}

async fn get_policy(
    State(state): State<Arc<DashboardState>>,
    Path(app_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;
    let parsed_id = Uuid::parse_str(&app_id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid app_id".to_string()))?;

    let rows: Vec<(Uuid, String, serde_json::Value, bool)> = sqlx::query_as(
        "SELECT id, axis, threshold_config, is_active FROM policies WHERE app_id = $1 AND is_active = true"
    )
    .bind(parsed_id)
    .fetch_all(pool)
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
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;
    let parsed_id = Uuid::parse_str(&app_id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid app_id".to_string()))?;

    let config = serde_json::json!({
        "block_threshold": body.block_threshold.unwrap_or(0.9),
        "escalate_threshold": body.escalate_threshold.unwrap_or(0.6),
        "max_tokens_per_request": body.max_tokens_per_request.unwrap_or(4000),
        "retry_max_count": body.retry_max_count.unwrap_or(3),
        "unsafe_keywords": body.unsafe_keywords.unwrap_or_default(),
        "pii_detection": body.pii_detection.unwrap_or(true),
        "toxicity_detection": body.toxicity_detection.unwrap_or(true),
        "bias_detection": body.bias_detection.unwrap_or(true),
        "unsafe_content_enabled": body.unsafe_content_enabled.unwrap_or(true),
        "secret_detection_enabled": body.secret_detection_enabled.unwrap_or(true),
        "prompt_injection_enabled": body.prompt_injection_enabled.unwrap_or(true),
        "hallucination_detection_enabled": body.hallucination_detection_enabled.unwrap_or(true),
        "groundedness_enabled": body.groundedness_enabled.unwrap_or(true),
        "verbosity_enabled": body.verbosity_enabled.unwrap_or(true),
        "semantic_pii_enabled": body.semantic_pii_enabled.unwrap_or(true),
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
        .execute(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error saving policy for axis {}: {e}", axis)))?;
    }

    Ok(Json(serde_json::json!({ "status": "saved", "app_id": app_id })))
}

// === System Config Handler ===

#[derive(Serialize)]
struct SystemConfig {
    api_port: u16,
    proxy_addr: String,
    upstream_provider: String,
    upstream_model: String,
    upstream_base_url: String,
    event_bus_mode: String,
    database_connected: bool,
    database_engine: String,
    database_name: String,
}

async fn get_system_config(
    State(state): State<Arc<DashboardState>>,
) -> Json<SystemConfig> {
    let db_connected = state.pool.is_some();
    let db_name = if db_connected {
        sqlx::query_scalar::<_, String>("SELECT current_database()")
            .fetch_one(state.pool.as_ref().unwrap())
            .await.unwrap_or_default()
    } else { String::new() };
    let db_version = if db_connected {
        sqlx::query_scalar::<_, String>("SELECT version()")
            .fetch_one(state.pool.as_ref().unwrap())
            .await.unwrap_or_default()
    } else { String::new() };

    Json(SystemConfig {
        api_port: std::env::var("DASHBOARD_API_PORT")
            .unwrap_or_else(|_| "8080".into()).parse().unwrap_or(8080),
        proxy_addr: std::env::var("PROXY_LISTEN_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8900".into()),
        upstream_provider: std::env::var("UPSTREAM_PROVIDER")
            .unwrap_or_else(|_| "ollama".into()),
        upstream_model: std::env::var("UPSTREAM_MODEL")
            .unwrap_or_else(|_| "qwen2.5:1.5b".into()),
        upstream_base_url: std::env::var("UPSTREAM_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:11434".into()),
        event_bus_mode: std::env::var("EVENT_BUS")
            .unwrap_or_else(|_| "inproc".into()),
        database_connected: db_connected,
        database_engine: db_version,
        database_name: db_name,
    })
}

// === API Key Handlers ===


#[derive(Serialize, sqlx::FromRow)]
struct ApiKeyRecord {
    id: Uuid,
    name: String,
    key_prefix: String,
    scopes: Vec<String>,
    status: String,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct ApiKeyListResponse {
    keys: Vec<ApiKeyRecord>,
    total: usize,
}

#[derive(Deserialize)]
struct CreateApiKeyRequest {
    name: String,
    scopes: Vec<String>,
}

fn generate_api_key() -> (String, String) {
    use std::io::Read;
    let mut rng = std::fs::File::open("/dev/urandom").ok();
    let mut bytes = [0u8; 32];
    if let Some(ref mut f) = rng {
        let _ = f.read(&mut bytes);
    } else {
        for b in bytes.iter_mut() {
            *b = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() & 0xff) as u8;
        }
    }
    let key = format!("cp_{}", base36_encode(&bytes));
    let prefix = format!("{}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}", &key[..key.len().min(8)]);
    (key, prefix)
}

fn base36_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut result = String::new();
    for &byte in data {
        result.push(CHARS[(byte % 36) as usize] as char);
    }
    result
}

fn hash_api_key(key: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

async fn list_api_keys(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<ApiKeyListResponse>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    let keys: Vec<ApiKeyRecord> = sqlx::query_as(
        "SELECT id, name, key_prefix, scopes, status, created_at, last_used_at
         FROM api_keys ORDER BY created_at DESC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let total = keys.len();
    Ok(Json(ApiKeyListResponse { keys, total }))
}

async fn create_api_key(
    State(state): State<Arc<DashboardState>>,
    Json(body): Json<CreateApiKeyRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    let (full_key, prefix) = generate_api_key();
    let key_hash = hash_api_key(&full_key);

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO api_keys (name, key_hash, key_prefix, scopes) VALUES ($1, $2, $3, $4) RETURNING id"
    )
    .bind(&body.name)
    .bind(&key_hash)
    .bind(&prefix)
    .bind(&body.scopes)
    .fetch_one(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "created",
        "id": id,
        "key": full_key,
        "key_prefix": prefix,
        "message": "Copy this key now - it won't be shown again"
    })))
}

async fn revoke_api_key(
    State(state): State<Arc<DashboardState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    sqlx::query("UPDATE api_keys SET status = 'revoked', revoked_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({ "status": "revoked", "id": id })))
}

// === API Key Analytics ===

#[derive(Serialize)]
struct ApiKeyAnalytics {
    api_key_id: Uuid,
    name: String,
    total_requests: i64,
    total_tokens: i64,
    total_cost_usd: f64,
    verdicts_by_outcome: Vec<OutcomeCount>,
    recent_activity: Vec<RecentActivity>,
}

#[derive(Serialize, sqlx::FromRow)]
struct OutcomeCount {
    outcome: String,
    count: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct RecentActivity {
    call_id: Uuid,
    model: String,
    outcome: Option<String>,
    tokens: Option<i64>,
    created_at: DateTime<Utc>,
}

async fn api_key_analytics(
    State(state): State<Arc<DashboardState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiKeyAnalytics>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    // Get key info
    let key_info: (Uuid, String) = sqlx::query_as(
        "SELECT id, name FROM api_keys WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or((StatusCode::NOT_FOUND, "API key not found".to_string()))?;

    // Total requests and tokens
    let stats: (Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(token_count_input + token_count_output), 0)
         FROM intercepted_calls WHERE api_key_id = $1"
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let total_requests = stats.0.unwrap_or(0);
    let total_tokens = stats.1.unwrap_or(0);
    let total_cost = total_tokens as f64 * 0.00000015;

    // Verdicts by outcome
    let verdicts_by_outcome: Vec<OutcomeCount> = sqlx::query_as(
        "SELECT v.outcome, COUNT(*) as count
         FROM verdicts v
         JOIN intercepted_calls ic ON v.call_id = ic.id
         WHERE ic.api_key_id = $1
         GROUP BY v.outcome"
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Recent activity
    let recent_activity: Vec<RecentActivity> = sqlx::query_as(
        "SELECT ic.id as call_id, ic.model, v.outcome,
                (ic.token_count_input + ic.token_count_output) as tokens,
                ic.created_at
         FROM intercepted_calls ic
         LEFT JOIN verdicts v ON v.call_id = ic.id
         WHERE ic.api_key_id = $1
         ORDER BY ic.created_at DESC LIMIT 20"
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ApiKeyAnalytics {
        api_key_id: key_info.0,
        name: key_info.1,
        total_requests,
        total_tokens,
        total_cost_usd: (total_cost * 100.0).round() / 100.0,
        verdicts_by_outcome,
        recent_activity,
    }))
}


// === Cost Analytics Handlers ===

#[derive(Serialize)]
struct CostSummary {
    total_tokens: i64,
    total_cost_usd: f64,
    request_count: i64,
    avg_tokens_per_request: f64,
}

#[derive(Serialize, sqlx::FromRow)]
struct CostTimeseriesRow {
    hour: String,
    tokens: i64,
    requests: i64,
}

#[derive(Serialize)]
struct CostAnomaly {
    app_id: String,
    metric: String,
    current_value: f64,
    baseline_value: f64,
    deviation_pct: f64,
}

async fn cost_summary(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<CostSummary>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    let now_minus_24h = Utc::now() - chrono::Duration::hours(24);

    let row: (Option<i64>, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT \
            COALESCE(SUM(token_count_input + token_count_output), 0), \
            COUNT(*), \
            COALESCE(SUM(token_count_input + token_count_output), 0) \
         FROM intercepted_calls WHERE created_at > $1"
    )
    .bind(now_minus_24h)
    .fetch_one(pool)
    .await
    .unwrap_or((Some(0), Some(0), Some(0)));

    let total_tokens = row.0.unwrap_or(0);
    let request_count = row.1.unwrap_or(0);
    let avg = if request_count > 0 { total_tokens as f64 / request_count as f64 } else { 0.0 };
    // Rough cost estimate: $0.15 per 1M tokens (varies by model)
    let total_cost = total_tokens as f64 * 0.00000015;

    Ok(Json(CostSummary {
        total_tokens,
        total_cost_usd: (total_cost * 100.0).round() / 100.0,
        request_count,
        avg_tokens_per_request: avg.round(),
    }))
}

async fn cost_timeseries(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<Vec<CostTimeseriesRow>>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    let rows: Vec<CostTimeseriesRow> = sqlx::query_as(
        "SELECT \
            TO_CHAR(date_trunc('hour', created_at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as hour, \
            COALESCE(SUM(token_count_input + token_count_output), 0) as tokens, \
            COUNT(*) as requests \
         FROM intercepted_calls \
         WHERE created_at > NOW() - INTERVAL '24 hours' \
         GROUP BY date_trunc('hour', created_at) \
         ORDER BY date_trunc('hour', created_at)"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    Ok(Json(rows))
}

async fn cost_anomalies(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<Vec<CostAnomaly>>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    // Compare last hour vs per-hour average of previous 24 hours
    let rows: Vec<CostAnomaly> = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT COALESCE(a.name, ic.app_id::text) as app_name, \
            COALESCE(SUM(ic.token_count_input + ic.token_count_output), 0) as recent_tokens, \
            GREATEST(COALESCE((SELECT SUM(token_count_input + token_count_output) / GREATEST(COUNT(DISTINCT date_trunc('hour', created_at)), 1) \
                FROM intercepted_calls \
                WHERE app_id = ic.app_id AND created_at BETWEEN NOW() - INTERVAL '24 hours' AND NOW() - INTERVAL '1 hour'), 1), 1) as hourly_avg \
         FROM intercepted_calls ic \
         LEFT JOIN apps a ON a.id = ic.app_id \
         WHERE ic.created_at > NOW() - INTERVAL '1 hour' \
         GROUP BY ic.app_id, a.name"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .filter(|(_, recent, avg)| *avg > 0 && (*recent as f64 / *avg as f64) > 2.0)
    .map(|(app_name, recent, avg)| CostAnomaly {
        app_id: app_name,
        metric: "Token usage surge".to_string(),
        current_value: recent as f64,
        baseline_value: avg as f64,
        deviation_pct: ((recent as f64 / avg as f64 - 1.0) * 100.0).round(),
    })
    .collect();

    Ok(Json(rows))
}

// === User Profile Handlers ===

#[derive(Serialize, sqlx::FromRow)]
struct UserProfile {
    id: Uuid,
    email: String,
    name: Option<String>,
    role: String,
    created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct UpdateProfileRequest {
    name: String,
}

// === Request Detail Handler ===

#[derive(Serialize)]
struct RequestDetail {
    call: CallDetail,
    verdicts: Vec<VerdictDetail>,
    audit_records: Vec<AuditDetail>,
    escalation: Option<EscalationDetail>,
}

#[derive(Serialize, sqlx::FromRow)]
struct CallDetail {
    id: Uuid,
    correlation_id: Uuid,
    app_id: Uuid,
    model: String,
    token_count_input: Option<i32>,
    token_count_output: Option<i32>,
    upstream_latency_ms: Option<i32>,
    fast_path_latency_ms: Option<i32>,
    request_payload: Option<serde_json::Value>,
    response_payload: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
struct VerdictDetail {
    id: Uuid,
    axis: String,
    path: String,
    outcome: String,
    confidence: f32,
    reason: String,
    check_name: String,
    latency_ms: Option<i32>,
    created_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
struct AuditDetail {
    id: Uuid,
    action_taken: String,
    record_hash: String,
    prev_hash: String,
    metadata: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
struct EscalationDetail {
    id: Uuid,
    status: String,
    axis: String,
    confidence: f32,
    reason: String,
    assigned_to: Option<Uuid>,
    resolution: Option<String>,
    resolution_reason: Option<String>,
    created_at: DateTime<Utc>,
    resolved_at: Option<DateTime<Utc>>,
}

async fn get_request_detail(
    State(state): State<Arc<DashboardState>>,
    Path(call_id): Path<Uuid>,
) -> Result<Json<RequestDetail>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    // Fetch the intercepted call
    let call: CallDetail = sqlx::query_as(
        "SELECT id, correlation_id, app_id, model, token_count_input, token_count_output,
                upstream_latency_ms, fast_path_latency_ms, request_payload, response_payload, created_at
         FROM intercepted_calls WHERE id = $1"
    )
    .bind(call_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or((StatusCode::NOT_FOUND, "Request not found".to_string()))?;

    // Fetch all verdicts for this call
    let verdicts: Vec<VerdictDetail> = sqlx::query_as(
        "SELECT id, axis, path, outcome, confidence, reason, check_name, \
                COALESCE(duration_ms, latency_ms) as latency_ms, created_at
         FROM verdicts WHERE call_id = $1 ORDER BY created_at ASC"
    )
    .bind(call_id)
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Fetch audit records for this call
    let audit_records: Vec<AuditDetail> = sqlx::query_as(
        "SELECT id, action_taken, record_hash, prev_hash, metadata, created_at
         FROM audit_records WHERE call_id = $1 ORDER BY created_at ASC"
    )
    .bind(call_id)
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Fetch escalation case for this call (if any)
    let escalation: Option<EscalationDetail> = sqlx::query_as(
        "SELECT id, status, axis, confidence, reason, assigned_to, resolution,
                resolution_reason, created_at, resolved_at
         FROM escalation_cases WHERE call_id = $1 LIMIT 1"
    )
    .bind(call_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(RequestDetail {
        call,
        verdicts,
        audit_records,
        escalation,
    }))
}

async fn get_user_profile(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<UserProfile>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    // In demo mode, return the admin user
    let profile = sqlx::query_as::<_, UserProfile>(
        "SELECT id, email, name, role, created_at FROM users WHERE email = $1"
    )
    .bind("admin@controlplane.ai")
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    match profile {
        Some(p) => Ok(Json(p)),
        None => Err((StatusCode::NOT_FOUND, "User not found".to_string())),
    }
}

async fn update_user_profile(
    State(state): State<Arc<DashboardState>>,
    Json(body): Json<UpdateProfileRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Name cannot be empty".to_string()));
    }

    sqlx::query(
        "UPDATE users SET name = $1, updated_at = NOW() WHERE email = $2"
    )
    .bind(&name)
    .bind("admin@controlplane.ai")
    .execute(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(serde_json::json!({
        "status": "updated",
        "name": name,
    })))
}

// === Audit Handlers ===

#[derive(Deserialize)]
#[allow(dead_code)]
struct AuditQueryParams {
    app_id: Option<Uuid>,
    outcome: Option<String>,
    axis: Option<String>,
    limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
struct AuditRecordRow {
    id: Uuid,
    call_id: Uuid,
    verdict_id: Uuid,
    action_taken: String,
    record_hash: String,
    prev_hash: String,
    metadata: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct AuditQueryResponse {
    records: Vec<AuditRecordRow>,
    total: usize,
    next_cursor: Option<String>,
}

async fn query_audit(
    State(state): State<Arc<DashboardState>>,
    Query(params): Query<AuditQueryParams>,
) -> Result<Json<AuditQueryResponse>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    let limit = params.limit.unwrap_or(50).min(200);

    // Fetch from DB with optional app_id filter
    let db_result = if let Some(app_id) = params.app_id {
        sqlx::query_as::<_, AuditRecordRow>(
            "SELECT a.id, a.call_id, a.verdict_id, a.action_taken, a.record_hash, a.prev_hash, a.metadata, a.created_at \
             FROM audit_records a \
             INNER JOIN intercepted_calls c ON a.call_id = c.id \
             WHERE c.app_id = $1 \
             ORDER BY a.created_at DESC LIMIT $2"
        )
        .bind(app_id)
        .bind(limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, AuditRecordRow>(
            "SELECT id, call_id, verdict_id, action_taken, record_hash, prev_hash, metadata, created_at \
             FROM audit_records ORDER BY created_at DESC LIMIT $1"
        )
        .bind(limit)
        .fetch_all(pool)
        .await
    };

    let all_records = db_result.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    // Apply outcome filter in Rust to avoid complex dynamic SQL
    let filtered: Vec<AuditRecordRow> = all_records.into_iter().filter(|r| {
        if let Some(ref outcome) = params.outcome {
            r.action_taken == *outcome
        } else {
            true
        }
    }).collect();

    let next_cursor = filtered.last().map(|r| r.created_at.to_rfc3339());
    let total = filtered.len();

    Ok(Json(AuditQueryResponse { records: filtered, total, next_cursor }))
}

#[derive(Serialize)]
struct VerifyResult {
    valid: bool,
    records_checked: u64,
    first_broken_at: Option<String>,
}

async fn verify_audit_chain(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<VerifyResult>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    let records: Vec<AuditRecordRow> = sqlx::query_as::<_, AuditRecordRow>(
        "SELECT id, call_id, verdict_id, action_taken, record_hash, prev_hash, metadata, created_at \
         FROM audit_records ORDER BY created_at ASC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    if records.is_empty() {
        return Ok(Json(VerifyResult {
            valid: true,
            records_checked: 0,
            first_broken_at: None,
        }));
    }

    let mut checked = 0u64;
    for window in records.windows(2) {
        let prev = &window[0];
        let curr = &window[1];
        if curr.prev_hash != prev.record_hash {
            return Ok(Json(VerifyResult {
                valid: false,
                records_checked: checked,
                first_broken_at: Some(curr.id.to_string()),
            }));
        }
        checked += 1;
    }

    Ok(Json(VerifyResult {
        valid: true,
        records_checked: checked + 1,
        first_broken_at: None,
    }))
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
    axis: String,
    confidence: f32,
    reason: String,
    status: String,
    assigned_to: Option<Uuid>,
    resolution: Option<String>,
    resolution_reason: Option<String>,
    created_at: DateTime<Utc>,
    resolved_at: Option<DateTime<Utc>>,
}

async fn list_escalations(
    State(state): State<Arc<DashboardState>>,
    Query(params): Query<EscalationListParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;
    let limit = params.limit.unwrap_or(50).min(200);
    let status = params.status.unwrap_or_else(|| "open".to_string());

    // Priority scoring: higher confidence + responsibility axis + older = higher priority
    let cases: Vec<EscalationRow> = sqlx::query_as(
        "SELECT id, call_id, verdict_id, app_id, axis, confidence, reason, status, assigned_to, resolution, resolution_reason, created_at, resolved_at \
         FROM escalation_cases WHERE status = $1 \
         ORDER BY \
           CASE WHEN axis = 'responsibility' THEN 3 WHEN axis = 'performance' THEN 2 ELSE 1 END * confidence DESC, \
           created_at ASC \
         LIMIT $2"
    )
    .bind(&status)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(serde_json::json!({ "escalations": cases, "total": cases.len() })))
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
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;
    let parsed_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid escalation id".to_string()))?;

    sqlx::query(
        "UPDATE escalation_cases SET status = 'resolved', resolution = $1, resolution_reason = $2, resolved_at = NOW() WHERE id = $3"
    )
    .bind(&body.action)
    .bind(&body.reason)
    .bind(parsed_id)
    .execute(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(serde_json::json!({ "status": "resolved", "id": id, "action": body.action })))
}

// === Detection Quality & Feedback Metrics (Round 2) ===

#[derive(Serialize)]
struct DetectionQualityMetrics {
    overall_trust_score: f64,
    total_escalations_resolved: i64,
    true_positives: i64,
    false_positives: i64,
    precision: f64,
    checks: Vec<CheckQuality>,
    trend_7d: Vec<DailyQuality>,
}

#[derive(Serialize, sqlx::FromRow)]
struct CheckQuality {
    axis: String,
    total_flagged: i64,
    confirmed: i64,
    overridden: i64,
    dismissed: i64,
    precision: f64,
}

#[derive(Serialize, sqlx::FromRow)]
struct DailyQuality {
    day: String,
    confirmed: i64,
    overridden: i64,
    dismissed: i64,
    precision: f64,
}

async fn detection_quality(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<DetectionQualityMetrics>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    // Overall resolution counts
    let overall: (Option<i64>, Option<i64>, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT \
            COUNT(*) FILTER (WHERE status = 'resolved'), \
            COUNT(*) FILTER (WHERE resolution = 'confirm'), \
            COUNT(*) FILTER (WHERE resolution = 'override'), \
            COUNT(*) FILTER (WHERE resolution = 'dismiss') \
         FROM escalation_cases"
    )
    .fetch_one(pool)
    .await
    .unwrap_or((Some(0), Some(0), Some(0), Some(0)));

    let total_resolved = overall.0.unwrap_or(0);
    let true_positives = overall.1.unwrap_or(0);
    let overridden = overall.2.unwrap_or(0);
    let dismissed = overall.3.unwrap_or(0);
    let false_positives = overridden + dismissed;
    let precision = if total_resolved > 0 {
        true_positives as f64 / total_resolved as f64
    } else {
        1.0
    };

    // Per-axis quality breakdown
    let checks: Vec<CheckQuality> = sqlx::query_as(
        "SELECT \
            axis, \
            COUNT(*) as total_flagged, \
            COUNT(*) FILTER (WHERE resolution = 'confirm') as confirmed, \
            COUNT(*) FILTER (WHERE resolution = 'override') as overridden, \
            COUNT(*) FILTER (WHERE resolution = 'dismiss') as dismissed, \
            CASE WHEN COUNT(*) FILTER (WHERE status = 'resolved') > 0 \
                THEN COUNT(*) FILTER (WHERE resolution = 'confirm')::float / \
                     COUNT(*) FILTER (WHERE status = 'resolved')::float \
                ELSE 1.0 \
            END as precision \
         FROM escalation_cases \
         GROUP BY axis \
         ORDER BY axis"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    // 7-day trend
    let trend_7d: Vec<DailyQuality> = sqlx::query_as(
        "SELECT \
            TO_CHAR(resolved_at, 'YYYY-MM-DD') as day, \
            COUNT(*) FILTER (WHERE resolution = 'confirm') as confirmed, \
            COUNT(*) FILTER (WHERE resolution = 'override') as overridden, \
            COUNT(*) FILTER (WHERE resolution = 'dismiss') as dismissed, \
            CASE WHEN COUNT(*) > 0 \
                THEN COUNT(*) FILTER (WHERE resolution = 'confirm')::float / COUNT(*)::float \
                ELSE 1.0 \
            END as precision \
         FROM escalation_cases \
         WHERE status = 'resolved' AND resolved_at > NOW() - INTERVAL '7 days' \
         GROUP BY TO_CHAR(resolved_at, 'YYYY-MM-DD') \
         ORDER BY day"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    // Trust score: weighted precision (higher weight for more critical axes)
    let overall_trust_score = (precision * 100.0).round() / 100.0;

    Ok(Json(DetectionQualityMetrics {
        overall_trust_score,
        total_escalations_resolved: total_resolved,
        true_positives,
        false_positives,
        precision,
        checks,
        trend_7d,
    }))
}

#[derive(Serialize)]
struct FeedbackEffectiveness {
    patterns_promoted: i64,
    threshold_adjustments: i64,
    avg_resolution_time_hours: f64,
    resolution_distribution: ResolutionDistribution,
    improvement_indicators: ImprovementIndicators,
}

#[derive(Serialize)]
struct ResolutionDistribution {
    confirm_pct: f64,
    override_pct: f64,
    dismiss_pct: f64,
}

#[derive(Serialize)]
struct ImprovementIndicators {
    escalation_rate_trend: String,
    repeat_flag_rate: f64,
    reviewer_agreement_rate: f64,
}

async fn feedback_effectiveness(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<FeedbackEffectiveness>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    // Pattern promotions count
    let promotions: (Option<i64>,) = sqlx::query_as(
        "SELECT COUNT(*) FROM pattern_promotions"
    )
    .fetch_one(pool)
    .await
    .unwrap_or((Some(0),));

    // Resolution distribution
    let dist: (Option<i64>, Option<i64>, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT \
            COUNT(*) FILTER (WHERE status = 'resolved'), \
            COUNT(*) FILTER (WHERE resolution = 'confirm'), \
            COUNT(*) FILTER (WHERE resolution = 'override'), \
            COUNT(*) FILTER (WHERE resolution = 'dismiss') \
         FROM escalation_cases"
    )
    .fetch_one(pool)
    .await
    .unwrap_or((Some(0), Some(0), Some(0), Some(0)));

    let total = dist.0.unwrap_or(0).max(1) as f64;
    let confirm = dist.1.unwrap_or(0) as f64;
    let overridden = dist.2.unwrap_or(0) as f64;
    let dismissed = dist.3.unwrap_or(0) as f64;

    // Average resolution time
    let avg_time: (Option<f64>,) = sqlx::query_as(
        "SELECT AVG(EXTRACT(EPOCH FROM (resolved_at - created_at)) / 3600.0) \
         FROM escalation_cases WHERE status = 'resolved'"
    )
    .fetch_one(pool)
    .await
    .unwrap_or((None,));

    // Escalation rate trend: compare last 7 days vs previous 7 days
    let recent: (Option<i64>,) = sqlx::query_as(
        "SELECT COUNT(*) FROM escalation_cases WHERE created_at > NOW() - INTERVAL '7 days'"
    )
    .fetch_one(pool)
    .await
    .unwrap_or((Some(0),));

    let previous: (Option<i64>,) = sqlx::query_as(
        "SELECT COUNT(*) FROM escalation_cases \
         WHERE created_at BETWEEN NOW() - INTERVAL '14 days' AND NOW() - INTERVAL '7 days'"
    )
    .fetch_one(pool)
    .await
    .unwrap_or((Some(0),));

    let recent_count = recent.0.unwrap_or(0) as f64;
    let previous_count = previous.0.unwrap_or(1).max(1) as f64;
    let trend = if recent_count < previous_count * 0.8 {
        "improving".to_string()
    } else if recent_count > previous_count * 1.2 {
        "worsening".to_string()
    } else {
        "stable".to_string()
    };

    // Repeat flag rate: how often same call_id gets multiple escalations
    let repeat_rate: (Option<f64>,) = sqlx::query_as(
        "SELECT CASE WHEN COUNT(DISTINCT call_id) > 0 \
            THEN 1.0 - (COUNT(DISTINCT call_id)::float / COUNT(*)::float) \
            ELSE 0.0 END \
         FROM escalation_cases"
    )
    .fetch_one(pool)
    .await
    .unwrap_or((Some(0.0),));

    // Reviewer agreement = confirm rate (reviewer agrees with system)
    let agreement = confirm / total;

    Ok(Json(FeedbackEffectiveness {
        patterns_promoted: promotions.0.unwrap_or(0),
        threshold_adjustments: dist.2.unwrap_or(0),
        avg_resolution_time_hours: (avg_time.0.unwrap_or(0.0) * 10.0).round() / 10.0,
        resolution_distribution: ResolutionDistribution {
            confirm_pct: (confirm / total * 100.0).round(),
            override_pct: (overridden / total * 100.0).round(),
            dismiss_pct: (dismissed / total * 100.0).round(),
        },
        improvement_indicators: ImprovementIndicators {
            escalation_rate_trend: trend,
            repeat_flag_rate: (repeat_rate.0.unwrap_or(0.0) * 100.0).round() / 100.0,
            reviewer_agreement_rate: (agreement * 100.0).round() / 100.0,
        },
    }))
}

// === Session Conversation Thread (Round 2 — Multi-Turn Context) ===

#[derive(Serialize, sqlx::FromRow)]
struct SessionTurn {
    id: Uuid,
    model: String,
    request_payload: Option<serde_json::Value>,
    response_payload: Option<serde_json::Value>,
    token_count_input: Option<i32>,
    token_count_output: Option<i32>,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct SessionThread {
    session_id: Option<Uuid>,
    turns: Vec<SessionTurn>,
    total_turns: usize,
}

async fn get_session_thread(
    State(state): State<Arc<DashboardState>>,
    Path(call_id): Path<Uuid>,
) -> Result<Json<SessionThread>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    // First, find the session_id for this call
    let session_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT session_id FROM intercepted_calls WHERE id = $1"
    )
    .bind(call_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .flatten();

    let turns = if let Some(sid) = session_id {
        // Fetch all turns in the same session, ordered chronologically
        sqlx::query_as::<_, SessionTurn>(
            "SELECT id, model, request_payload, response_payload, \
                    token_count_input, token_count_output, created_at \
             FROM intercepted_calls WHERE session_id = $1 \
             ORDER BY created_at ASC LIMIT 50"
        )
        .bind(sid)
        .fetch_all(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    } else {
        // No session — just return this single call
        sqlx::query_as::<_, SessionTurn>(
            "SELECT id, model, request_payload, response_payload, \
                    token_count_input, token_count_output, created_at \
             FROM intercepted_calls WHERE id = $1"
        )
        .bind(call_id)
        .fetch_all(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };

    let total_turns = turns.len();
    Ok(Json(SessionThread { session_id, turns, total_turns }))
}

// === Policy Profiles (Round 2 — Regulatory/Geographic) ===

#[derive(Serialize, sqlx::FromRow)]
struct PolicyProfile {
    id: String,
    name: String,
    description: String,
    geography: String,
    industry: String,
    risk_appetite: String,
    default_thresholds: serde_json::Value,
    regulations: Vec<String>,
}

async fn list_profiles(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    let profiles: Vec<PolicyProfile> = sqlx::query_as(
        "SELECT id, name, description, geography, industry, risk_appetite, default_thresholds, regulations \
         FROM policy_profiles ORDER BY geography, industry"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(serde_json::json!({ "profiles": profiles })))
}

#[derive(Deserialize)]
struct ApplyProfileBody {
    profile_id: String,
}

async fn apply_profile(
    State(state): State<Arc<DashboardState>>,
    Path(app_id): Path<String>,
    Json(body): Json<ApplyProfileBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    let parsed_app_id: Uuid = app_id.parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid app_id".to_string()))?;

    // Verify profile exists and get its thresholds
    let profile: Option<PolicyProfile> = sqlx::query_as(
        "SELECT id, name, description, geography, industry, risk_appetite, default_thresholds, regulations \
         FROM policy_profiles WHERE id = $1"
    )
    .bind(&body.profile_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let profile = profile.ok_or((StatusCode::NOT_FOUND, format!("Profile '{}' not found", body.profile_id)))?;

    // Update all policies for this app to use the profile and apply its thresholds
    let axes = ["performance", "cost", "responsibility"];
    for axis in &axes {
        let threshold = profile.default_thresholds.get(axis).cloned()
            .unwrap_or(serde_json::json!({}));

        sqlx::query(
            "UPDATE policies SET threshold_config = $1, profile = $2, version = version + 1, updated_at = NOW() \
             WHERE app_id = $3 AND axis = $4"
        )
        .bind(&threshold)
        .bind(&body.profile_id)
        .bind(parsed_app_id)
        .bind(axis)
        .execute(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;
    }

    Ok(Json(serde_json::json!({
        "applied": true,
        "profile": body.profile_id,
        "app_id": app_id,
        "message": format!("Applied '{}' profile to app. Thresholds updated for all axes.", profile.name)
    })))
}

// === Data Source Governance (Round 2, Task R2.9) ===

#[derive(Deserialize)]
struct UpdateGovernanceBody {
    level: String,
}

async fn update_governance_level(
    State(state): State<Arc<DashboardState>>,
    Path(app_id): Path<String>,
    Json(body): Json<UpdateGovernanceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    let parsed_app_id: Uuid = app_id.parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid app_id".to_string()))?;

    if !["high", "medium", "low"].contains(&body.level.as_str()) {
        return Err((StatusCode::BAD_REQUEST, "Level must be 'high', 'medium', or 'low'".to_string()));
    }

    sqlx::query("UPDATE apps SET data_governance_level = $1 WHERE id = $2")
        .bind(&body.level)
        .bind(parsed_app_id)
        .execute(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(serde_json::json!({
        "app_id": app_id,
        "data_governance_level": body.level,
        "note": match body.level.as_str() {
            "low" => "Low governance = stricter groundedness/hallucination thresholds applied",
            "high" => "High governance = standard thresholds (well-governed data sources)",
            _ => "Medium governance = moderate thresholds"
        }
    })))
}
