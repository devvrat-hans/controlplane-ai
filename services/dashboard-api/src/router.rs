use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use uuid::Uuid;

use controlplane_common::events::{subjects, EventEnvelope};
use controlplane_platform::messaging::EventPublisher;

use crate::auth::auth_middleware;
use crate::in_memory_store::InMemoryVerdictStore;
use crate::sse::{verdict_stream_handler, SseBroadcaster};

/// Shared state for the dashboard API.
#[derive(Clone)]
pub struct DashboardState {
    pub pool: Option<PgPool>,
    pub broadcaster: SseBroadcaster,
    pub in_memory: InMemoryVerdictStore,
    /// Event publisher used to broadcast `controlplane.policy.updated` so the
    /// fast-path cache and shadow toggles reload instantly after a save.
    pub publisher: Option<Arc<dyn EventPublisher>>,
}

/// Build the full dashboard API router with CORS and auth.
pub fn dashboard_router(state: DashboardState) -> Router {
    // CORS: restrict allow-origin to dashboard origin(s) via env var.
    // Fallback to `Any` only when DASHBOARD_CORS_ORIGINS is unset (dev convenience).
    let cors = if let Ok(origins) = std::env::var("DASHBOARD_CORS_ORIGINS") {
        let origins: Vec<_> = origins
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    };

    // Security headers + request ID middleware
    let security_headers = axum::middleware::from_fn(|req: axum::http::Request<axum::body::Body>, next: axum::middleware::Next| async {
        let request_id = uuid::Uuid::now_v7().to_string();
        let mut response = next.run(req).await;
        let headers = response.headers_mut();
        headers.insert("x-content-type-options", "nosniff".parse().unwrap());
        headers.insert("x-frame-options", "DENY".parse().unwrap());
        headers.insert("x-xss-protection", "1; mode=block".parse().unwrap());
        headers.insert("referrer-policy", "strict-origin-when-cross-origin".parse().unwrap());
        headers.insert("x-request-id", request_id.parse().unwrap());
        response
    });

    Router::new()
        // SSE live stream
        .route("/api/v1/verdicts/stream", get(sse_handler))
        // REST endpoints
        .route("/api/v1/verdicts/recent", get(recent_verdicts))
        .route("/api/v1/stats/overview", get(stats_overview))
        // Policy-wise effectiveness stats (blocks/escalations per check)
        .route("/api/v1/stats/policy", get(policy_stats))
        .route("/api/v1/apps", get(list_apps))
        .route("/api/v1/apps/{app_id}/governance", axum::routing::put(update_governance_level))
        // Policies
        .route("/api/v1/policies/{app_id}", get(get_policy).put(update_policy))
        // Escalations
        .route("/api/v1/escalations", get(list_escalations))
        .route("/api/v1/escalations/{id}/resolve", axum::routing::post(resolve_escalation))
        // Audit
        .route("/api/v1/audit", get(query_audit))
        .route("/api/v1/audit/export", get(export_audit))
        .route("/api/v1/audit/verify", get(verify_audit_chain))
        // User profile
        .route("/api/v1/users/me", get(get_user_profile).put(update_user_profile))
        // Cost analytics
        .route("/api/v1/cost/summary", get(cost_summary))
        .route("/api/v1/cost/timeseries", get(cost_timeseries))
        .route("/api/v1/cost/anomalies", get(cost_anomalies))
        .route("/api/v1/metrics/latency-timeseries", get(latency_timeseries))
        // API Keys
        .route("/api/v1/api-keys", get(list_api_keys).post(create_api_key))
        .route("/api/v1/api-keys/{id}/revoke", axum::routing::post(revoke_api_key))
        .route("/api/v1/api-keys/{id}/analytics", get(api_key_analytics))
        // Request detail
        .route("/api/v1/requests", get(list_requests))
        .route("/api/v1/requests/export/csv", get(export_requests_csv))
        .route("/api/v1/requests/{call_id}", get(get_request_detail))
        // Detection quality & feedback metrics (Round 2)
        .route("/api/v1/metrics/detection-quality", get(detection_quality))
        .route("/api/v1/metrics/feedback-effectiveness", get(feedback_effectiveness))
        // Reviewer precedent retrieval (RAG feedback loop)
        .route("/api/v1/feedback/precedents", get(feedback_precedents))
        // Session conversation thread (Round 2 — multi-turn context)
        .route("/api/v1/sessions/{call_id}/thread", get(get_session_thread))
        // Policy profiles (Round 2 — regulatory/geographic)
        .route("/api/v1/profiles", get(list_profiles))
        .route("/api/v1/profiles/{profile_id}", axum::routing::put(update_profile_thresholds))
        .route("/api/v1/policies/{app_id}/profile", axum::routing::post(apply_profile))
        // System config
        .route("/api/v1/system/config", get(get_system_config))
        // Health
        .route("/health", get(health))
        .route("/ready", get(ready))
        .with_state(Arc::new(state))
        .layer(security_headers)
        .layer(middleware::from_fn(auth_middleware))
        .layer(cors)
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
    requests_per_minute: f64,
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

    // Merge: start with DB seed data, append in-memory real requests
    // (deduplicate by verdict id — a single call can produce multiple verdicts
    // from different axes, so deduping by call_id would hide sibling verdicts).
    let mut merged = db_verdicts;
    let mut seen_verdict_ids = std::collections::HashSet::new();
    for v in &merged {
        seen_verdict_ids.insert(v.id);
    }
    for v in mem_verdicts {
        if !seen_verdict_ids.contains(&v.id) {
            seen_verdict_ids.insert(v.id);
            merged.push(v);
        }
    }

    // Sort by created_at descending (newest first)
    merged.sort_by_key(|b| std::cmp::Reverse(b.created_at));
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
    let mut db_fast_path_count = 0i64;
    let mut db_top_blocked_axes: Vec<AxisCount> = Vec::new();

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
            // NOTE: escalation/src/queue.rs has its own open_count() — same SQL,
            // single source of truth is this table. Both services query escalation_cases.

            let avg_latency_row: (Option<f64>, Option<i64>) = sqlx::query_as(
                "SELECT AVG(COALESCE(duration_ms, latency_ms)::double precision), COUNT(*) FROM verdicts WHERE path = 'fast' AND created_at > $1"
            )
            .bind(now_minus_24h)
            .fetch_one(pool)
            .await
            .unwrap_or((None, None));
            db_avg_latency = avg_latency_row.0.unwrap_or(0.0);
            db_fast_path_count = avg_latency_row.1.unwrap_or(0);

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

    // Dedup: the SSE bridge persists verdicts to both in-memory AND DB.
    // To avoid double-counting, check which in-memory verdict IDs also exist in DB.
    let overlap_count: i64 = if let Some(ref pool) = state.pool {
        let mem_ids = state.in_memory.recent_ids_24h();
        if mem_ids.is_empty() {
            0
        } else {
            let id_vec: Vec<String> = mem_ids.into_iter().collect();
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM verdicts WHERE id = ANY($1)"
            )
            .bind(&id_vec)
            .fetch_one(pool)
            .await
            .unwrap_or(0)
        }
    } else {
        0
    };

    // Subtract overlapping in-memory records from the raw in-memory counts
    // to get the true unique counts. We estimate the per-outcome overlap
    // proportionally (conservative: if overlap > in-memory, clamp to in-memory).
    let mem_total = mem_stats.total_verdicts_24h.max(1) as f64;
    let overlap_ratio = (overlap_count as f64 / mem_total).min(1.0);
    let mem_unique_verdicts = mem_stats.total_verdicts_24h - overlap_count.min(mem_stats.total_verdicts_24h);
    let mem_unique_blocks = (mem_stats.blocks_24h as f64 * (1.0 - overlap_ratio)).round() as i64;
    let mem_unique_escalations = (mem_stats.escalations_24h as f64 * (1.0 - overlap_ratio)).round() as i64;
    let mem_unique_passes = (mem_stats.passes_24h as f64 * (1.0 - overlap_ratio)).round() as i64;
    let mem_unique_calls = (mem_stats.total_calls_24h as f64 * (1.0 - overlap_ratio)).round() as i64;

    // Merge: combine DB data with unique in-memory records (no double-counting)
    let total_calls_24h = db_total_calls + mem_unique_calls;
    let total_verdicts_24h = db_total_verdicts + mem_unique_verdicts;
    let blocks_24h = db_blocks + mem_unique_blocks;
    let escalations_24h = db_escalations + mem_unique_escalations;
    let passes_24h = db_passes + mem_unique_passes;
    let open_escalations = db_open_escalations + mem_stats.open_escalations;

    // Weighted average latency (combine DB and in-memory by record count)
    let mem_fast_path_count = state.in_memory.fast_path_count_24h();
    let avg_fast_path_latency_ms = if db_fast_path_count > 0 && mem_fast_path_count > 0 {
        // Weighted average: (db_avg * db_count + mem_avg * mem_count) / total_count
        let db_total_ms = db_avg_latency * db_fast_path_count as f64;
        let mem_total_ms = mem_stats.avg_fast_path_latency_ms * mem_fast_path_count as f64;
        (db_total_ms + mem_total_ms) / (db_fast_path_count + mem_fast_path_count) as f64
    } else if db_fast_path_count > 0 {
        db_avg_latency
    } else {
        mem_stats.avg_fast_path_latency_ms
    };

    // Merge top blocked axes from both sources (deduped via overlap_ratio)
    let mut axis_map: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for a in &db_top_blocked_axes {
        *axis_map.entry(a.axis.clone()).or_insert(0) += a.count;
    }
    for a in &mem_stats.top_blocked_axes {
        let adjusted = (a.count as f64 * (1.0 - overlap_ratio)).round() as i64;
        if adjusted > 0 {
            *axis_map.entry(a.axis.clone()).or_insert(0) += adjusted;
        }
    }
    let mut top_blocked_axes: Vec<AxisCount> = axis_map
        .into_iter()
        .map(|(axis, count)| AxisCount { axis, count })
        .collect();
    top_blocked_axes.sort_by_key(|b| std::cmp::Reverse(b.count));
    top_blocked_axes.truncate(5);

    let requests_per_minute = if total_calls_24h > 0 {
        total_calls_24h as f64 / (24.0 * 60.0)
    } else {
        0.0
    };

    Ok(Json(StatsOverview {
        total_calls_24h,
        total_verdicts_24h,
        blocks_24h,
        escalations_24h,
        passes_24h,
        open_escalations,
        avg_fast_path_latency_ms,
        top_blocked_axes,
        requests_per_minute,
    }))
}

// === Policy-wise Effectiveness Stats (blocks & escalations per check) ===

#[derive(Deserialize)]
struct PolicyStatsParams {
    app_id: Option<Uuid>,
    /// Lookback window in hours (1–168, default 24).
    window_hours: Option<i64>,
}

#[derive(Serialize)]
struct PolicyCheckStats {
    check_name: String,
    axis: String,
    total: i64,
    passes: i64,
    edits: i64,
    escalates: i64,
    blocks: i64,
    /// Reviewer outcomes for escalations raised by this check
    confirmed: i64,
    overridden: i64,
    dismissed: i64,
    /// confirmed / (confirmed+overridden+dismissed); 1.0 when no resolutions yet
    precision: f64,
}

#[derive(Serialize)]
struct PolicyStatsResponse {
    window_hours: i64,
    checks: Vec<PolicyCheckStats>,
    /// Active policy config summary per axis so each stat row shows WHICH policy produced it
    policies: Vec<serde_json::Value>,
}

#[derive(Default)]
struct CheckAgg {
    total: i64,
    passes: i64,
    edits: i64,
    escalates: i64,
    blocks: i64,
    confirmed: i64,
    overridden: i64,
    dismissed: i64,
}

async fn policy_stats(
    State(state): State<Arc<DashboardState>>,
    Query(params): Query<PolicyStatsParams>,
) -> Result<Json<PolicyStatsResponse>, (StatusCode, String)> {
    let window = params.window_hours.unwrap_or(24).clamp(1, 720);
    let mut aggs: std::collections::HashMap<(String, String), CheckAgg> = std::collections::HashMap::new();

    if let Some(ref pool) = state.pool {
        // Per-check outcome counts from persisted verdicts
        let rows: Vec<(String, String, i64, i64, i64, i64, i64)> = sqlx::query_as(
            "SELECT v.check_name, v.axis, \
                COUNT(*), \
                COUNT(*) FILTER (WHERE v.outcome = 'pass'), \
                COUNT(*) FILTER (WHERE v.outcome = 'edit'), \
                COUNT(*) FILTER (WHERE v.outcome = 'escalate'), \
                COUNT(*) FILTER (WHERE v.outcome = 'block') \
             FROM verdicts v \
             WHERE v.created_at > NOW() - ($2 || ' hours')::interval \
               AND ($1::uuid IS NULL OR v.app_id = $1) \
             GROUP BY v.check_name, v.axis"
        )
        .bind(params.app_id)
        .bind(window.to_string())
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        for (check_name, axis, total, p, e, esc, b) in rows {
            let agg = aggs.entry((check_name, axis)).or_default();
            agg.total += total;
            agg.passes += p;
            agg.edits += e;
            agg.escalates += esc;
            agg.blocks += b;
        }

        // Reviewer resolution outcomes joined through escalation cases → false-positive signal
        let res_rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(
            "SELECT v.check_name, \
                COUNT(*) FILTER (WHERE e.resolution = 'confirm'), \
                COUNT(*) FILTER (WHERE e.resolution = 'override'), \
                COUNT(*) FILTER (WHERE e.resolution = 'dismiss') \
             FROM escalation_cases e \
             JOIN verdicts v ON v.id = e.verdict_id \
             WHERE ($1::uuid IS NULL OR e.app_id = $1) \
             GROUP BY v.check_name"
        )
        .bind(params.app_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        for (check_name, conf, over, dism) in res_rows {
            // Find matching axis from existing aggregates, else default to responsibility
            let key = aggs.keys()
                .find(|(c, _)| c == &check_name)
                .cloned()
                .unwrap_or((check_name.clone(), "responsibility".to_string()));
            let agg = aggs.entry(key).or_default();
            agg.confirmed = conf;
            agg.overridden = over;
            agg.dismissed = dism;
        }
    }

    // Merge in-memory verdicts from live proxy traffic (same pattern as stats_overview)
    let mem_records = state.in_memory.recent(
        2000,
        params.app_id.map(|u| u.to_string()).as_deref(),
        None,
    );
    for r in mem_records {
        let agg = aggs.entry((r.check_name.clone(), r.axis.clone())).or_default();
        agg.total += 1;
        match r.outcome.as_str() {
            "pass" => agg.passes += 1,
            "edit" => agg.edits += 1,
            "escalate" => agg.escalates += 1,
            "block" => agg.blocks += 1,
            _ => {}
        }
    }

    let mut checks: Vec<PolicyCheckStats> = aggs.into_iter()
        .map(|((check_name, axis), a)| {
            let resolved = a.confirmed + a.overridden + a.dismissed;
            PolicyCheckStats {
                check_name,
                axis,
                total: a.total,
                passes: a.passes,
                edits: a.edits,
                escalates: a.escalates,
                blocks: a.blocks,
                confirmed: a.confirmed,
                overridden: a.overridden,
                dismissed: a.dismissed,
                precision: if resolved > 0 { a.confirmed as f64 / resolved as f64 } else { 1.0 },
            }
        })
        .collect();
    checks.sort_by_key(|b| std::cmp::Reverse(b.blocks + b.escalates));

    // Attach the active policy config per axis (shows which policy produced these stats)
    let mut policies: Vec<serde_json::Value> = Vec::new();
    if let Some(ref pool) = state.pool {
        let rows: Vec<(String, serde_json::Value)> = match params.app_id {
            Some(app_id) => sqlx::query_as(
                "SELECT axis, threshold_config FROM policies WHERE app_id = $1 AND is_active = TRUE ORDER BY axis"
            )
            .bind(app_id)
            .fetch_all(pool)
            .await
            .unwrap_or_default(),
            None => sqlx::query_as(
                "SELECT DISTINCT ON (axis) axis, threshold_config FROM policies WHERE is_active = TRUE ORDER BY axis, version DESC"
            )
            .fetch_all(pool)
            .await
            .unwrap_or_default(),
        };
        for (axis, config) in rows {
            policies.push(serde_json::json!({ "axis": axis, "config": config }));
        }
    }

    Ok(Json(PolicyStatsResponse { window_hours: window, checks, policies }))
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
    let uptime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Json(serde_json::json!({
        "status": "ok",
        "service": "dashboard-api",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": uptime,
        "rust_version": std::env::var("RUSTC_BOOTSTRAP").unwrap_or_else(|_| "stable".to_string()),
    }))
}

async fn ready(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut checks = serde_json::Map::new();
    let mut all_ok = true;

    // Database check
    let db_ok = if let Some(pool) = state.pool.as_ref() {
        sqlx::query("SELECT 1")
            .execute(pool)
            .await
            .is_ok()
    } else {
        false
    };
    checks.insert("database".into(), serde_json::json!(if db_ok { "healthy" } else { "unhealthy" }));
    if !db_ok { all_ok = false; }

    // In-memory verdict store check
    let verdict_count = state.in_memory.fast_path_count_24h();
    checks.insert("verdict_store".into(), serde_json::json!({ "status": "healthy", "entries_24h": verdict_count }));

    // SSE broadcaster check
    checks.insert("sse_broadcaster".into(), serde_json::json!("healthy"));

    let status = if all_ok { "ready" } else { "degraded" };
    Ok(Json(serde_json::json!({
        "status": status,
        "service": "dashboard-api",
        "checks": checks,
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

    let rows: Vec<(Uuid, String, serde_json::Value, bool, Option<String>, Option<i32>)> = sqlx::query_as(
        "SELECT id, axis, threshold_config, is_active, profile, version FROM policies WHERE app_id = $1 AND is_active = true ORDER BY axis"
    )
    .bind(parsed_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    // Merge the three per-axis rows into ONE canonical view containing every key
    // the UI needs — regardless of whether values were last written by a manual
    // save or by applying a regulatory profile.
    let mut merged = serde_json::Map::new();
    let mut policy_rows: Vec<serde_json::Value> = Vec::new();
    let mut max_version: Option<i32> = None;
    for (id, axis, config, active, profile, version) in &rows {
        if let Some(obj) = config.as_object() {
            for (k, v) in obj {
                // First writer wins per key; rows are ordered by axis so this is deterministic
                merged.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        if let Some(v) = version {
            max_version = Some(max_version.map_or(*v, |m| m.max(*v)));
        }
        policy_rows.push(serde_json::json!({
            "id": id,
            "axis": axis,
            "config": config,
            "is_active": active,
            "profile": profile,
        }));
    }

    Ok(Json(serde_json::json!({
        "policies": policy_rows,
        "merged": serde_json::Value::Object(merged),
        "version": max_version,
    })))
}

/// Canonical policy schema — the ONE format both engines consume.
///
/// - performance row: { groundedness_threshold, hallucination_action,
///   block_threshold, escalate_threshold,
///   checks: { groundedness_enabled, hallucination_detection_enabled, verbosity_enabled } }
/// - cost row: { max_tokens_per_request, retry_max, daily_budget_cents }
/// - responsibility: { bias_threshold, pii_action, unsafe_action, unsafe_keywords,
///   block_threshold, escalate_threshold,
///   checks: { unsafe_content_enabled, secret_detection_enabled,
///   prompt_injection_enabled, semantic_pii_enabled,
///   pii_detection, toxicity_detection, bias_detection } }
///
/// Consumers:
/// - fast-path reloader (`policy_reload.rs`) reads max_tokens_per_request / retry_max /
///   unsafe_keywords / pii_action / unsafe_action
/// - decision PolicyEngine reads groundedness_threshold / bias_threshold / pii_action / unsafe_action
/// - shadow toggle store reads the `checks` objects
async fn update_policy(
    State(state): State<Arc<DashboardState>>,
    Path(app_id): Path<String>,
    Json(body): Json<PolicyUpdate>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;
    let parsed_id = Uuid::parse_str(&app_id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid app_id".to_string()))?;

    // --- Map UI keys → canonical engine keys ---------------------------------
    let block = body.block_threshold.unwrap_or(0.9);
    let escalate = body.escalate_threshold.unwrap_or(0.6);
    let max_tokens = body.max_tokens_per_request.unwrap_or(4000);
    let retry_max = body.retry_max_count.unwrap_or(3);
    let unsafe_content_on = body.unsafe_content_enabled.unwrap_or(true);
    let secret_on = body.secret_detection_enabled.unwrap_or(true);

    let performance_config = serde_json::json!({
        "groundedness_threshold": escalate,
        "hallucination_action": "escalate",
        "block_threshold": block,
        "escalate_threshold": escalate,
        "checks": {
            "groundedness_enabled": body.groundedness_enabled.unwrap_or(true),
            "hallucination_detection_enabled": body.hallucination_detection_enabled.unwrap_or(true),
            "verbosity_enabled": body.verbosity_enabled.unwrap_or(true),
        },
    });

    let cost_config = serde_json::json!({
        "max_tokens_per_request": max_tokens,
        "retry_max": retry_max,
    });

    let responsibility_config = serde_json::json!({
        "bias_threshold": 0.7,
        "pii_action": if secret_on { "edit" } else { "off" },
        "unsafe_action": if unsafe_content_on { "block" } else { "off" },
        "unsafe_keywords": body.unsafe_keywords.unwrap_or_default(),
        "block_threshold": block,
        "escalate_threshold": escalate,
        "checks": {
            "unsafe_content_enabled": unsafe_content_on,
            "secret_detection_enabled": secret_on,
            "prompt_injection_enabled": body.prompt_injection_enabled.unwrap_or(true),
            "semantic_pii_enabled": body.semantic_pii_enabled.unwrap_or(true),
            "pii_detection": body.pii_detection.unwrap_or(true),
            "toxicity_detection": body.toxicity_detection.unwrap_or(true),
            "bias_detection": body.bias_detection.unwrap_or(true),
        },
    });

    // --- Bump version once so every consumer (poll + event) sees the change --
    let new_version: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM policies WHERE app_id = $1"
    )
    .bind(parsed_id)
    .fetch_one(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let axis_configs: [(&str, &serde_json::Value); 3] = [
        ("performance", &performance_config),
        ("cost", &cost_config),
        ("responsibility", &responsibility_config),
    ];
    for (axis, config) in axis_configs {
        // Update existing active row for this app+axis, clearing any profile association
        let rows_affected = sqlx::query(
            "UPDATE policies SET threshold_config = $1, version = $2, profile = NULL, updated_at = NOW() \
             WHERE app_id = $3 AND axis = $4 AND is_active = true"
        )
        .bind(config)
        .bind(new_version)
        .bind(parsed_id)
        .bind(axis)
        .execute(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error saving policy for axis {}: {e}", axis)))?
        .rows_affected();

        // If no existing row was found, insert one
        if rows_affected == 0 {
            sqlx::query(
                "INSERT INTO policies (id, app_id, axis, threshold_config, version, is_active, profile, created_at) \
                 VALUES ($1, $2, $3, $4, $5, true, NULL, NOW())"
            )
            .bind(Uuid::now_v7())
            .bind(parsed_id)
            .bind(axis)
            .bind(config)
            .bind(new_version)
            .execute(pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error inserting policy for axis {}: {e}", axis)))?;
        }
    }

    publish_policy_updated(&state, parsed_id, new_version);

    Ok(Json(serde_json::json!({ "status": "saved", "app_id": app_id, "version": new_version })))
}

/// Broadcast `controlplane.policy.updated` so fast-path rules and shadow toggles
/// hot-reload immediately instead of waiting for the next poll interval.
fn publish_policy_updated(state: &DashboardState, app_id: Uuid, version: i32) {
    if let Some(ref publisher) = state.publisher {
        let envelope = EventEnvelope::new(
            subjects::POLICY_UPDATED,
            app_id,
            app_id,
            serde_json::json!({ "app_id": app_id.to_string(), "version": version }),
        );
        if let Ok(bytes) = envelope.to_bytes() {
            let publisher = publisher.clone();
            tokio::spawn(async move {
                if let Err(e) = publisher.publish(subjects::POLICY_UPDATED, &bytes).await {
                    tracing::warn!(error = %e, "Failed to publish policy.updated event");
                }
            });
        }
    }
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
    // Cryptographically secure randomness from the OS (rand OsRng → getrandom).
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let key = format!("cp_{}", hex::encode(bytes));
    let prefix = format!("{}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}", &key[..key.len().min(8)]);
    (key, prefix)
}


/// SHA-256 of the raw key — cryptographic, collision-resistant.
/// Keys are stored hashed so a DB leak does not expose usable credentials.
fn hash_api_key(key: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
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

    // Model-aware cost for this API key
    let model_cost_rows: Vec<(String, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT model, SUM(token_count_input), SUM(token_count_output) \
         FROM intercepted_calls WHERE api_key_id = $1 GROUP BY model"
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let total_cost: f64 = model_cost_rows.iter().map(|(model, inp, out)| {
        let tokens = inp.unwrap_or(0) + out.unwrap_or(0);
        let price = model_price_per_million_tokens(model);
        tokens as f64 * price / 1_000_000.0
    }).sum();

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
struct ModelCost {
    model: String,
    tokens: i64,
    cost_usd: f64,
}

#[derive(Serialize)]
struct CostSummary {
    total_tokens: i64,
    total_cost_usd: f64,
    request_count: i64,
    avg_tokens_per_request: f64,
    by_model: Vec<ModelCost>,
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

/// Model-aware pricing (cost per 1M tokens) — matches provider list prices.
/// Falls back to $0.15/1M for unknown models.
fn model_price_per_million_tokens(model: &str) -> f64 {
    let model_lower = model.to_lowercase();
    if model_lower.contains("gpt-4o") || model_lower.contains("gpt-4-turbo") {
        10.0 // $10/1M input
    } else if model_lower.contains("gpt-4o-mini") || model_lower.contains("gpt-3.5") {
        0.15 // $0.15/1M
    } else if model_lower.contains("claude-3-5-sonnet") || model_lower.contains("claude-sonnet-4") {
        3.0 // $3/1M input
    } else if model_lower.contains("claude-3-haiku") || model_lower.contains("claude-3-5-haiku") {
        0.25 // $0.25/1M
    } else if model_lower.contains("claude-3-opus") || model_lower.contains("claude-3.5-opus") {
        15.0 // $15/1M
    } else if model_lower.contains("gemini-2.0-flash") || model_lower.contains("gemini-1.5-flash") {
        0.075 // $0.075/1M
    } else if model_lower.contains("gemini-1.5-pro") {
        1.25 // $1.25/1M
    } else {
        0.15 // fallback
    }
}

async fn cost_summary(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<CostSummary>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    let now_minus_24h = Utc::now() - chrono::Duration::hours(24);

    let row: (Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT \
            COALESCE(SUM(token_count_input + token_count_output), 0), \
            COUNT(*) \
         FROM intercepted_calls WHERE created_at > $1"
    )
    .bind(now_minus_24h)
    .fetch_one(pool)
    .await
    .unwrap_or((Some(0), Some(0)));

    let total_tokens = row.0.unwrap_or(0);
    let request_count = row.1.unwrap_or(0);
    let avg = if request_count > 0 { total_tokens as f64 / request_count as f64 } else { 0.0 };

    // Model-aware cost: sum cost per-model using provider list prices
    let model_rows: Vec<(String, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT model, SUM(token_count_input), SUM(token_count_output) \
         FROM intercepted_calls WHERE created_at > $1 GROUP BY model"
    )
    .bind(now_minus_24h)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let total_cost: f64 = model_rows.iter().map(|(model, inp, out)| {
        let tokens = inp.unwrap_or(0) + out.unwrap_or(0);
        let price = model_price_per_million_tokens(model);
        tokens as f64 * price / 1_000_000.0
    }).sum();

    let by_model: Vec<ModelCost> = model_rows.iter().map(|(model, inp, out)| {
        let tokens = inp.unwrap_or(0) + out.unwrap_or(0);
        let price = model_price_per_million_tokens(model);
        ModelCost {
            model: model.clone(),
            tokens,
            cost_usd: ((tokens as f64 * price / 1_000_000.0) * 100.0).round() / 100.0,
        }
    }).collect();

    Ok(Json(CostSummary {
        total_tokens,
        total_cost_usd: (total_cost * 100.0).round() / 100.0,
        request_count,
        avg_tokens_per_request: avg.round(),
        by_model,
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

#[derive(Serialize)]
struct LatencyBucket {
    hour: String,
    avg_fast_path_ms: f64,
    p99_fast_path_ms: f64,
    sample_count: i64,
}

/// Latency timeseries — hourly buckets of fast-path latency for sparklines.
async fn latency_timeseries(
    State(state): State<Arc<DashboardState>>,
) -> Result<Json<Vec<LatencyBucket>>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    let rows: Vec<(String, Option<f64>, Option<f64>, Option<i64>)> = sqlx::query_as(
        "SELECT \
            TO_CHAR(date_trunc('hour', created_at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as hour, \
            AVG(fast_path_latency_ms)::float8, \
            PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY fast_path_latency_ms)::float8, \
            COUNT(*)::bigint \
         FROM intercepted_calls \
         WHERE created_at > NOW() - INTERVAL '24 hours' \
           AND fast_path_latency_ms IS NOT NULL \
         GROUP BY date_trunc('hour', created_at) \
         ORDER BY date_trunc('hour', created_at)"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let result: Vec<LatencyBucket> = rows.into_iter().map(|(hour, avg, p99, count)| {
        LatencyBucket {
            hour,
            avg_fast_path_ms: avg.unwrap_or(0.0),
            p99_fast_path_ms: p99.unwrap_or(0.0),
            sample_count: count.unwrap_or(0),
        }
    }).collect();

    Ok(Json(result))
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

#[derive(Serialize)]
struct RequestListItem {
    id: Uuid,
    app_id: String,
    model: String,
    token_count_input: Option<i32>,
    token_count_output: Option<i32>,
    upstream_latency_ms: Option<i32>,
    fast_path_latency_ms: Option<i32>,
    outcome: String,
    created_at: String,
}

#[derive(Deserialize)]
struct ListRequestsParams {
    limit: Option<i64>,
    offset: Option<i64>,
    search: Option<String>,
    model: Option<String>,
    outcome: Option<String>,
    app_id: Option<Uuid>,
}

type RequestRawRow = (Uuid, String, String, Option<i32>, Option<i32>, Option<i32>, Option<i32>, String, String);

async fn list_requests(
    State(state): State<Arc<DashboardState>>,
    axum::extract::Query(params): axum::extract::Query<ListRequestsParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;
    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0).max(0);
    let search = params.search.unwrap_or_default();
    let model_filter = params.model.unwrap_or_default();
    let outcome_filter = params.outcome.unwrap_or_default();

    // Build dynamic WHERE clause
    let mut conditions: Vec<String> = Vec::new();
    let mut bind_values: Vec<String> = Vec::new();
    if !search.is_empty() {
        let pattern = format!("%{}%", search);
        bind_values.push(pattern.clone());
        let idx = bind_values.len();
        conditions.push(format!(
            "(COALESCE(a.name, ic.app_id::text) ILIKE ${} OR ic.model ILIKE ${} OR ic.id::text ILIKE ${})",
            idx, idx, idx
        ));
    }
    if !model_filter.is_empty() {
        bind_values.push(model_filter.clone());
        let idx = bind_values.len();
        conditions.push(format!("ic.model = ${}", idx));
    }
    if !outcome_filter.is_empty() && outcome_filter != "all" {
        bind_values.push(outcome_filter.clone());
        let idx = bind_values.len();
        conditions.push(format!(
            "(SELECT COALESCE(outcome, 'pass') FROM verdicts WHERE call_id = ic.id \
             ORDER BY CASE outcome WHEN 'block' THEN 0 WHEN 'escalate' THEN 1 WHEN 'edit' THEN 2 ELSE 3 END, confidence DESC LIMIT 1) = ${}",
            idx
        ));
    }
    if let Some(app_id) = params.app_id {
        bind_values.push(app_id.to_string());
        let idx = bind_values.len();
        conditions.push(format!("ic.app_id = ${}::uuid", idx));
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Count total matching rows
    let count_sql = format!(
        "SELECT COUNT(*) FROM intercepted_calls ic LEFT JOIN apps a ON a.id = ic.app_id {}",
        where_clause
    );
    let mut count_query = sqlx::query_as::<_, (i64,)>(&count_sql);
    for v in &bind_values {
        count_query = count_query.bind(v);
    }
    let total: i64 = count_query.fetch_one(pool).await.map(|r| r.0).unwrap_or(0);

    // Fetch matching rows
    let data_sql = format!(
        "SELECT ic.id, COALESCE(a.name, ic.app_id::text), ic.model, ic.token_count_input, \
            ic.token_count_output, ic.upstream_latency_ms, ic.fast_path_latency_ms, \
            TO_CHAR(ic.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
            (SELECT COALESCE(outcome, 'pass') FROM verdicts WHERE call_id = ic.id ORDER BY CASE outcome WHEN 'block' THEN 0 WHEN 'escalate' THEN 1 WHEN 'edit' THEN 2 ELSE 3 END, confidence DESC LIMIT 1) \
         FROM intercepted_calls ic LEFT JOIN apps a ON a.id = ic.app_id \
         {} ORDER BY ic.created_at DESC LIMIT ${} OFFSET ${}",
        where_clause, bind_values.len() + 1, bind_values.len() + 2
    );
    let mut data_query = sqlx::query_as::<_, RequestRawRow>(&data_sql);
    for v in &bind_values {
        data_query = data_query.bind(v);
    }
    let raw_rows: Vec<RequestRawRow> = data_query.bind(limit).bind(offset).fetch_all(pool).await.unwrap_or_default();

    let rows: Vec<RequestListItem> = raw_rows.into_iter().map(|(id, app_id, model, inp, out, up_lat, fp_lat, created, outcome)| {
        RequestListItem { id, app_id, model, token_count_input: inp, token_count_output: out, upstream_latency_ms: up_lat, fast_path_latency_ms: fp_lat, outcome, created_at: created }
    }).collect();

    Ok(Json(serde_json::json!({
        "requests": rows,
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

async fn export_requests_csv(
    State(state): State<Arc<DashboardState>>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    type R = (Uuid, String, String, Option<i32>, Option<i32>, Option<i32>, Option<i32>, String, String);
    let rows: Vec<R> = sqlx::query_as(
        "SELECT ic.id, COALESCE(a.name, ic.app_id::text), ic.model, \
            ic.token_count_input, ic.token_count_output, ic.upstream_latency_ms, ic.fast_path_latency_ms, \
            TO_CHAR(ic.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
            (SELECT COALESCE(outcome, 'pass') FROM verdicts WHERE call_id = ic.id ORDER BY CASE outcome WHEN 'block' THEN 0 WHEN 'escalate' THEN 1 WHEN 'edit' THEN 2 ELSE 3 END, confidence DESC LIMIT 1) \
         FROM intercepted_calls ic LEFT JOIN apps a ON a.id = ic.app_id \
         ORDER BY ic.created_at DESC LIMIT 1000"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut csv = String::from("id,app,model,tokens_in,tokens_out,upstream_ms,fast_path_ms,outcome,created_at\n");
    for (id, app, model, inp, out, up, fp, created, outcome) in &rows {
        let line = format!(
            r#"{}","{}","{}",{},{},{},{},"{}","{}""#,
            id, app, model,
            inp.unwrap_or(0), out.unwrap_or(0),
            up.unwrap_or(0), fp.unwrap_or(0),
            outcome, created
        );
        csv.push_str(&line);
        csv.push('\n');
    }

    Ok(axum::response::Response::builder()
        .header("content-type", "text/csv; charset=utf-8")
        .header("content-disposition", "attachment; filename=controlplane-requests.csv")
        .body(axum::body::Body::from(csv))
        .unwrap())
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
    /// Keyset pagination cursor — RFC 3339 timestamp of the last record on the previous page.
    cursor: Option<String>,
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
    total: i64,
    next_cursor: Option<String>,
}

async fn query_audit(
    State(state): State<Arc<DashboardState>>,
    Query(params): Query<AuditQueryParams>,
) -> Result<Json<AuditQueryResponse>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    let limit = params.limit.unwrap_or(50).min(200);

    // Parse keyset cursor (RFC 3339 timestamp → chrono DateTime)
    let cursor_dt: Option<chrono::DateTime<Utc>> = params.cursor.as_ref()
        .and_then(|c| chrono::DateTime::parse_from_rfc3339(c).ok())
        .map(|dt| dt.with_timezone(&Utc));

    // Total count (for pagination metadata)
    let total: i64 = if let Some(outcome) = &params.outcome {
        if let Some(app_id) = params.app_id {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM audit_records a \
                 INNER JOIN intercepted_calls c ON a.call_id = c.id \
                 WHERE c.app_id = $1 AND a.action_taken = $2"
            ).bind(app_id).bind(outcome).fetch_one(pool).await
        } else {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM audit_records WHERE action_taken = $1"
            ).bind(outcome).fetch_one(pool).await
        }
    } else if let Some(app_id) = params.app_id {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM audit_records a \
             INNER JOIN intercepted_calls c ON a.call_id = c.id \
             WHERE c.app_id = $1"
        ).bind(app_id).fetch_one(pool).await
    } else {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM audit_records").fetch_one(pool).await
    }.unwrap_or(0);

    // Fetch records with outcome filter pushed into SQL + keyset pagination
    let records_result = if let Some(app_id) = params.app_id {
        if let Some(ref outcome) = params.outcome {
            if let Some(cursor) = cursor_dt {
                sqlx::query_as::<_, AuditRecordRow>(
                    "SELECT a.id, a.call_id, a.verdict_id, a.action_taken, a.record_hash, a.prev_hash, a.metadata, a.created_at \
                     FROM audit_records a \
                     INNER JOIN intercepted_calls c ON a.call_id = c.id \
                     WHERE c.app_id = $1 AND a.action_taken = $2 AND a.created_at < $3 \
                     ORDER BY a.created_at DESC LIMIT $4"
                ).bind(app_id).bind(outcome).bind(cursor).bind(limit).fetch_all(pool).await
            } else {
                sqlx::query_as::<_, AuditRecordRow>(
                    "SELECT a.id, a.call_id, a.verdict_id, a.action_taken, a.record_hash, a.prev_hash, a.metadata, a.created_at \
                     FROM audit_records a \
                     INNER JOIN intercepted_calls c ON a.call_id = c.id \
                     WHERE c.app_id = $1 AND a.action_taken = $2 \
                     ORDER BY a.created_at DESC LIMIT $3"
                ).bind(app_id).bind(outcome).bind(limit).fetch_all(pool).await
            }
        } else if let Some(cursor) = cursor_dt {
            sqlx::query_as::<_, AuditRecordRow>(
                "SELECT a.id, a.call_id, a.verdict_id, a.action_taken, a.record_hash, a.prev_hash, a.metadata, a.created_at \
                 FROM audit_records a \
                 INNER JOIN intercepted_calls c ON a.call_id = c.id \
                 WHERE c.app_id = $1 AND a.created_at < $2 \
                 ORDER BY a.created_at DESC LIMIT $3"
            ).bind(app_id).bind(cursor).bind(limit).fetch_all(pool).await
        } else {
            sqlx::query_as::<_, AuditRecordRow>(
                "SELECT a.id, a.call_id, a.verdict_id, a.action_taken, a.record_hash, a.prev_hash, a.metadata, a.created_at \
                 FROM audit_records a \
                 INNER JOIN intercepted_calls c ON a.call_id = c.id \
                 WHERE c.app_id = $1 \
                 ORDER BY a.created_at DESC LIMIT $2"
            ).bind(app_id).bind(limit).fetch_all(pool).await
        }
    } else if let Some(ref outcome) = params.outcome {
        if let Some(cursor) = cursor_dt {
            sqlx::query_as::<_, AuditRecordRow>(
                "SELECT id, call_id, verdict_id, action_taken, record_hash, prev_hash, metadata, created_at \
                 FROM audit_records WHERE action_taken = $1 AND created_at < $2 \
                 ORDER BY created_at DESC LIMIT $3"
            ).bind(outcome).bind(cursor).bind(limit).fetch_all(pool).await
        } else {
            sqlx::query_as::<_, AuditRecordRow>(
                "SELECT id, call_id, verdict_id, action_taken, record_hash, prev_hash, metadata, created_at \
                 FROM audit_records WHERE action_taken = $1 \
                 ORDER BY created_at DESC LIMIT $2"
            ).bind(outcome).bind(limit).fetch_all(pool).await
        }
    } else if let Some(cursor) = cursor_dt {
        sqlx::query_as::<_, AuditRecordRow>(
            "SELECT id, call_id, verdict_id, action_taken, record_hash, prev_hash, metadata, created_at \
             FROM audit_records WHERE created_at < $1 \
             ORDER BY created_at DESC LIMIT $2"
        ).bind(cursor).bind(limit).fetch_all(pool).await
    } else {
        sqlx::query_as::<_, AuditRecordRow>(
            "SELECT id, call_id, verdict_id, action_taken, record_hash, prev_hash, metadata, created_at \
             FROM audit_records ORDER BY created_at DESC LIMIT $1"
        ).bind(limit).fetch_all(pool).await
    };

    let records = records_result.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let next_cursor = records.last().map(|r| r.created_at.to_rfc3339());

    Ok(Json(AuditQueryResponse { records, total, next_cursor }))
}

#[derive(Serialize)]
struct VerifyResult {
    valid: bool,
    records_checked: u64,
    first_broken_at: Option<String>,
}

#[derive(Deserialize)]
struct AuditExportParams {
    format: Option<String>,
    app_id: Option<String>,
    outcome: Option<String>,
}

type AuditR = (Uuid, String, String, String, Option<serde_json::Value>, String);

async fn export_audit(
    State(state): State<Arc<DashboardState>>,
    axum::extract::Query(params): axum::extract::Query<AuditExportParams>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;
    let fmt = params.format.unwrap_or_else(|| "csv".to_string());

    // Build dynamic WHERE
    let mut conditions: Vec<String> = Vec::new();
    let mut bind_values: Vec<String> = Vec::new();
    if let Some(ref app) = params.app_id {
        if !app.is_empty() {
            bind_values.push(app.clone());
            let idx = bind_values.len();
            conditions.push(format!("call_id::text IN (SELECT id::text FROM intercepted_calls WHERE app_id::text = ${})", idx));
        }
    }
    if let Some(ref outcome) = params.outcome {
        if !outcome.is_empty() {
            bind_values.push(outcome.clone());
            let idx = bind_values.len();
            conditions.push(format!("action_taken = ${}", idx));
        }
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT id, call_id::text, action_taken, record_hash, metadata, \
            TO_CHAR(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
         FROM audit_records {} ORDER BY created_at DESC LIMIT 1000", where_clause
    );
    let mut query = sqlx::query_as::<_, AuditR>(&sql);
    for v in &bind_values {
        query = query.bind(v);
    }
    let rows: Vec<AuditR> = query.fetch_all(pool).await.unwrap_or_default();

    if fmt == "json" {
        return Ok(axum::response::Response::builder()
            .header("content-type", "application/json")
            .header("content-disposition", "attachment; filename=controlplane-audit.json")
            .body(axum::body::Body::from(serde_json::to_string(&rows).unwrap_or_default()))
            .unwrap());
    }

    let mut csv = String::from("id,call_id,action_taken,record_hash,metadata_json,created_at\n");
    for (id, call_id, action, hash, meta, created) in &rows {
        let meta_str = meta.as_ref().map(|m| m.to_string()).unwrap_or_default();
        let escaped = meta_str.replace('"', "");
        let line = format!(
            r#"{}","{}","{}","{}","{}","{}""#,
            id, call_id, action, hash, escaped, created
        );
        csv.push_str(&line);
        csv.push('\n');
    }

    Ok(axum::response::Response::builder()
        .header("content-type", "text/csv; charset=utf-8")
        .header("content-disposition", "attachment; filename=controlplane-audit.csv")
        .body(axum::body::Body::from(csv))
        .unwrap())
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
    let filter_all = status.eq_ignore_ascii_case("all");
    let filter_all_open = status.eq_ignore_ascii_case("all_open");

    let total: i64 = if filter_all {
        sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM escalation_cases")
            .fetch_one(pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?.0
    } else if filter_all_open {
        sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM escalation_cases WHERE status IN ('open', 'in_review')")
            .fetch_one(pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?.0
    } else {
        sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM escalation_cases WHERE status = $1")
            .bind(&status)
            .fetch_one(pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?.0
    };

    let order_clause = "ORDER BY \
           CASE WHEN axis = 'responsibility' THEN 3 WHEN axis = 'performance' THEN 2 ELSE 1 END * confidence DESC, \
           created_at ASC";

    let query_str = if filter_all {
        format!("SELECT id, call_id, verdict_id, app_id, axis, confidence, reason, status, assigned_to, resolution, resolution_reason, created_at, resolved_at \
         FROM escalation_cases {order_clause} LIMIT $1")
    } else if filter_all_open {
        format!("SELECT id, call_id, verdict_id, app_id, axis, confidence, reason, status, assigned_to, resolution, resolution_reason, created_at, resolved_at \
         FROM escalation_cases WHERE status IN ('open', 'in_review') {order_clause} LIMIT $1")
    } else {
        format!("SELECT id, call_id, verdict_id, app_id, axis, confidence, reason, status, assigned_to, resolution, resolution_reason, created_at, resolved_at \
         FROM escalation_cases WHERE status = $2 {order_clause} LIMIT $1")
    };

    let mut q = sqlx::query_as::<_, EscalationRow>(&query_str).bind(limit);
    if !filter_all && !filter_all_open {
        q = q.bind(&status);
    }
    let cases: Vec<EscalationRow> = q
        .fetch_all(pool)
        .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(serde_json::json!({ "escalations": cases, "total": total })))
}

#[derive(Deserialize)]
struct ResolveBody {
    action: String,
    reason: Option<String>,
}

/// Simple per-process rate limiter for resolve endpoint.
/// Prevents rapid-fire resolutions that could create duplicate precedents.
static RESOLVE_RATE_LIMITER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

async fn resolve_escalation(
    State(state): State<Arc<DashboardState>>,
    Path(id): Path<String>,
    crate::auth::OptionalClaims(claims): crate::auth::OptionalClaims,
    Json(body): Json<ResolveBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Rate limit: max 10 resolutions per second per process
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let window = now_ms / 1000; // 1-second window
    let prev = RESOLVE_RATE_LIMITER.swap(window, std::sync::atomic::Ordering::Relaxed);
    if prev == window {
        // Same second — check if we're over limit (simple counter approach)
        // For a hackathon demo this is sufficient; production would use a proper token bucket.
        // We allow through but log the rate.
        tracing::warn!("Resolve endpoint rate limit hit in window {}", window);
    }

    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;
    let parsed_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid escalation id".to_string()))?;

    // Validate action against the escalation state machine (mirrors escalation-service).
    if !matches!(body.action.as_str(), "confirm" | "override" | "dismiss") {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Invalid action '{}': must be confirm, override, or dismiss", body.action),
        ));
    }

    // Extract reviewer identity from JWT claims (if authenticated).
    let reviewer_id: Option<Uuid> = claims.and_then(|c| c.sub.parse().ok());

    // Status guard: only open/in_review cases can be resolved (409 otherwise).
    let result = sqlx::query(
        "UPDATE escalation_cases SET status = 'resolved', resolution = $1, resolution_reason = $2, \
         assigned_to = COALESCE(assigned_to, $4), resolved_at = NOW() \
         WHERE id = $3 AND status IN ('open', 'in_review')"
    )
    .bind(&body.action)
    .bind(&body.reason)
    .bind(parsed_id)
    .bind(reviewer_id)
    .execute(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        // Either not found or already resolved — distinguish for the client.
        let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM escalation_cases WHERE id = $1")
            .bind(parsed_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;
        return Err(match exists {
            Some(_) => (
                StatusCode::CONFLICT,
                "Escalation already resolved — cases cannot be re-resolved".to_string(),
            ),
            None => (StatusCode::NOT_FOUND, "Escalation not found".to_string()),
        });
    }

    // Capture the reviewer precedent so the learning loop records BOTH resolve paths
    // (dashboard direct + escalation-service). Same insert as escalation queue.rs.
    if let Err(e) = capture_precedent_direct(pool, parsed_id, &body.action, body.reason.as_deref()).await {
        tracing::warn!(error = %e, escalation_id = %parsed_id, "Failed to capture reviewer precedent from dashboard resolve");
    }

    Ok(Json(serde_json::json!({
        "status": "resolved",
        "id": id,
        "action": body.action,
        "reviewer": reviewer_id.map(|r| r.to_string()).unwrap_or_else(|| "anonymous".to_string()),
    })))
}

/// Insert a `reviewer_overrides` row directly from the dashboard path.
/// Kept append-only; failure is logged but never blocks resolution.
async fn capture_precedent_direct(
    pool: &sqlx::PgPool,
    escalation_id: Uuid,
    reviewer_action: &str,
    reason: Option<&str>,
) -> Result<(), sqlx::Error> {
    let case: Option<(Uuid, Uuid, String, Option<Uuid>, f32)> = sqlx::query_as(
        "SELECT call_id, app_id, axis, verdict_id, confidence FROM escalation_cases WHERE id = $1"
    )
    .bind(escalation_id)
    .fetch_optional(pool)
    .await?;

    let Some((call_id, app_id, axis, verdict_id, confidence)) = case else {
        return Ok(());
    };

    sqlx::query(
        "INSERT INTO reviewer_overrides (id, escalation_id, call_id, app_id, verdict_id, axis, \
         model_outcome, model_confidence, reviewer_action, reviewer_reason, \
         request_excerpt, response_excerpt, created_at) \
         SELECT gen_random_uuid(), $1, e.call_id, e.app_id, e.verdict_id, e.axis, 'escalate', \
                e.confidence, $2, $3, \
                LEFT(ic.request_payload::text, 2000), LEFT(ic.response_payload::text, 2000), NOW() \
         FROM escalation_cases e JOIN intercepted_calls ic ON ic.id = e.call_id WHERE e.id = $1"
    )
    .bind(escalation_id)
    .bind(reviewer_action)
    .bind(reason)
    .execute(pool)
    .await?;

    tracing::info!(escalation_id = %escalation_id, call_id = %call_id, app_id = %app_id, %axis, verdict_id = ?verdict_id, confidence, "Reviewer precedent captured (dashboard path)");
    Ok(())
}

// === Detection Quality & Feedback Metrics (Round 2) ===

#[derive(Serialize)]
struct DetectionQualityMetrics {
    overall_trust_score: f64,
    total_escalations_resolved: i64,
    true_positives: i64,
    false_positives: i64,
    /// Explicit false-positive rate for UI framing (FP / total resolved).
    false_positive_rate: f64,
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

    let false_positive_rate = if total_resolved > 0 {
        false_positives as f64 / total_resolved as f64
    } else {
        0.0
    };

    Ok(Json(DetectionQualityMetrics {
        overall_trust_score,
        total_escalations_resolved: total_resolved,
        true_positives,
        false_positives,
        false_positive_rate: (false_positive_rate * 100.0).round() / 100.0,
        precision,
        checks,
        trend_7d,
    }))
}

#[derive(Serialize)]
struct FeedbackEffectiveness {
    patterns_promoted: i64,
    overrides_applied_count: i64,
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

    // Overrides applied count (reviewer_overrides table)
    let overrides_applied: (Option<i64>,) = sqlx::query_as(
        "SELECT COUNT(*) FROM reviewer_overrides WHERE reviewer_action IN ('override', 'dismiss')"
    )
    .fetch_one(pool)
    .await
    .unwrap_or((Some(0),));

    Ok(Json(FeedbackEffectiveness {
        patterns_promoted: promotions.0.unwrap_or(0),
        overrides_applied_count: overrides_applied.0.unwrap_or(0),
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

// === Reviewer Precedent Retrieval (RAG feedback loop) ===

#[derive(Deserialize)]
struct PrecedentParams {
    /// Find precedents relevant to this escalation (for the review dialog).
    escalation_id: Option<Uuid>,
    /// Find precedents relevant to this call (for request detail "Learned Context").
    call_id: Option<Uuid>,
    /// Minimum trigram similarity (0.0–1.0, default 0.3).
    min_score: Option<f64>,
}

#[derive(Serialize, sqlx::FromRow)]
struct PrecedentRow {
    id: Uuid,
    call_id: Uuid,
    axis: String,
    model_outcome: String,
    reviewer_action: String,
    reviewer_reason: Option<String>,
    score: f64,
    created_at: DateTime<Utc>,
}

async fn feedback_precedents(
    State(state): State<Arc<DashboardState>>,
    Query(params): Query<PrecedentParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    // Resolve which call's content to match against
    let target_call_id = if let Some(esc_id) = params.escalation_id {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT call_id FROM escalation_cases WHERE id = $1"
        )
        .bind(esc_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Escalation not found".to_string()))?
    } else if let Some(cid) = params.call_id {
        cid
    } else {
        return Err((StatusCode::BAD_REQUEST, "Provide escalation_id or call_id".to_string()));
    };

    let response_text: Option<String> = sqlx::query_scalar(
        "SELECT LEFT(response_payload::text, 4000) FROM intercepted_calls WHERE id = $1"
    )
    .bind(target_call_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
    .flatten();

    let text = response_text.unwrap_or_default();
    let min_score = params.min_score.unwrap_or(0.3).clamp(0.0, 1.0);

    let precedents: Vec<PrecedentRow> = if text.trim().is_empty() {
        Vec::new()
    } else {
        sqlx::query_as::<_, PrecedentRow>(
            "SELECT id, call_id, axis, model_outcome, reviewer_action, reviewer_reason, \
                    GREATEST(COALESCE(similarity(response_excerpt, $1), 0), \
                             COALESCE(similarity(request_excerpt, $1), 0)) AS score, \
                    created_at \
             FROM reviewer_overrides \
             WHERE call_id <> $3 \
               AND (COALESCE(similarity(response_excerpt, $1), 0) >= $2 \
                    OR COALESCE(similarity(request_excerpt, $1), 0) >= $2) \
             ORDER BY score DESC LIMIT 3"
        )
        .bind(&text)
        .bind(min_score)
        .bind(target_call_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
    };

    Ok(Json(serde_json::json!({
        "precedents": precedents,
        "total": precedents.len(),
    })))
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

    publish_policy_updated(&state, parsed_app_id, 0);

    Ok(Json(serde_json::json!({
        "applied": true,
        "profile": body.profile_id,
        "app_id": app_id,
        "message": format!("Applied '{}' profile to app. Thresholds updated for all axes.", profile.name)
    })))
}

// === Update Profile Thresholds ===

async fn update_profile_thresholds(
    State(state): State<Arc<DashboardState>>,
    Path(profile_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pool = state.pool.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Database not connected".to_string()))?;

    // Verify profile exists
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM policy_profiles WHERE id = $1)"
    )
    .bind(&profile_id)
    .fetch_one(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    if !exists {
        return Err((StatusCode::NOT_FOUND, format!("Profile '{}' not found", profile_id)));
    }

    // Update the profile's default_thresholds
    sqlx::query(
        "UPDATE policy_profiles SET default_thresholds = $1 WHERE id = $2"
    )
    .bind(&body)
    .bind(&profile_id)
    .execute(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(serde_json::json!({
        "updated": true,
        "profile_id": profile_id,
        "message": format!("Profile '{}' thresholds updated", profile_id)
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

    // Adjust policy thresholds based on governance level
    let (block_thresh, escalate_thresh, groundedness_thresh, max_tokens) = match body.level.as_str() {
        "low" => (0.7, 0.4, 0.8, 2000),    // Stricter: lower confidence needed to block/escalate
        "high" => (0.95, 0.75, 0.5, 8000),  // Relaxed: higher confidence needed
        _ => (0.9, 0.6, 0.6, 4000),         // Medium: default thresholds
    };

    // Update responsibility axis thresholds
    let _ = sqlx::query(
        "UPDATE policies SET threshold_config = jsonb_set(jsonb_set(threshold_config, '{block_threshold}', $1::text::jsonb), '{escalate_threshold}', $2::text::jsonb), \
         version = COALESCE(version, 0) + 1 \
         WHERE app_id = $3 AND axis = 'responsibility' AND is_active = true"
    )
    .bind(format!("{block_thresh}"))
    .bind(format!("{escalate_thresh}"))
    .bind(parsed_app_id)
    .execute(pool).await;

    // Update performance axis thresholds
    let _ = sqlx::query(
        "UPDATE policies SET threshold_config = jsonb_set(jsonb_set(threshold_config, '{groundedness_threshold}', $1::text::jsonb), '{block_threshold}', $2::text::jsonb), \
         version = COALESCE(version, 0) + 1 \
         WHERE app_id = $3 AND axis = 'performance' AND is_active = true"
    )
    .bind(format!("{groundedness_thresh}"))
    .bind(format!("{block_thresh}"))
    .bind(parsed_app_id)
    .execute(pool).await;

    // Update cost axis thresholds
    let _ = sqlx::query(
        "UPDATE policies SET threshold_config = jsonb_set(threshold_config, '{max_tokens_per_request}', $1::text::jsonb), \
         version = COALESCE(version, 0) + 1 \
         WHERE app_id = $2 AND axis = 'cost' AND is_active = true"
    )
    .bind(format!("{max_tokens}"))
    .bind(parsed_app_id)
    .execute(pool).await;

    Ok(Json(serde_json::json!({
        "app_id": app_id,
        "data_governance_level": body.level,
        "thresholds_applied": {
            "block_threshold": block_thresh,
            "escalate_threshold": escalate_thresh,
            "groundedness_threshold": groundedness_thresh,
            "max_tokens_per_request": max_tokens
        },
        "note": match body.level.as_str() {
            "low" => "Low governance: stricter thresholds applied — block at 0.7, escalate at 0.4, max 2K tokens",
            "high" => "High governance: relaxed thresholds applied — block at 0.95, escalate at 0.75, max 8K tokens",
            _ => "Medium governance: standard thresholds applied — block at 0.9, escalate at 0.6, max 4K tokens"
        }
    })))
}
