use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ledger::CostLedger;

pub struct CostServiceState {
    pub ledger: CostLedger,
}

/// HTTP router for cost analytics API.
pub fn cost_router(state: Arc<CostServiceState>) -> Router {
    Router::new()
        .route("/api/v1/cost/summary", get(get_summary))
        .route("/api/v1/cost/anomalies", get(get_anomalies))
        .route("/api/v1/cost/health", get(health))
        .with_state(state)
}

#[derive(Deserialize)]
struct SummaryParams {
    app_id: Uuid,
    /// Window: 1m, 5m, 15m, 1h, 6h, 1d, 7d, 30d
    window: Option<String>,
}

#[derive(Deserialize)]
struct AnomalyParams {
    app_id: Uuid,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct SummaryResponse {
    app_id: Uuid,
    window: String,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
    total_cost_usd: f64,
    request_count: i64,
    avg_tokens_per_request: f64,
    avg_cost_per_request: f64,
}

#[derive(Serialize)]
struct AnomalyResponse {
    app_id: Uuid,
    anomalies: Vec<AnomalyEntry>,
    checked_from: DateTime<Utc>,
    checked_to: DateTime<Utc>,
}

#[derive(Serialize)]
struct AnomalyEntry {
    metric: String,
    current_value: f64,
    baseline_value: f64,
    deviation_factor: f64,
    reason: String,
    detected_at: DateTime<Utc>,
}

async fn get_summary(
    State(state): State<Arc<CostServiceState>>,
    Query(params): Query<SummaryParams>,
) -> Result<Json<SummaryResponse>, (StatusCode, String)> {
    let window = params.window.unwrap_or_else(|| "1h".to_string());

    let summary = state.ledger.get_summary(params.app_id, &window).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_tokens = summary.total_input_tokens + summary.total_output_tokens;
    let avg_tokens = if summary.request_count > 0 {
        total_tokens as f64 / summary.request_count as f64
    } else {
        0.0
    };
    let avg_cost = if summary.request_count > 0 {
        summary.total_cost_usd / summary.request_count as f64
    } else {
        0.0
    };

    Ok(Json(SummaryResponse {
        app_id: params.app_id,
        window,
        window_start: summary.window_start,
        window_end: summary.window_end,
        total_input_tokens: summary.total_input_tokens,
        total_output_tokens: summary.total_output_tokens,
        total_tokens,
        total_cost_usd: summary.total_cost_usd,
        request_count: summary.request_count,
        avg_tokens_per_request: avg_tokens,
        avg_cost_per_request: avg_cost,
    }))
}

async fn get_anomalies(
    State(state): State<Arc<CostServiceState>>,
    Query(params): Query<AnomalyParams>,
) -> Result<Json<AnomalyResponse>, (StatusCode, String)> {
    let to = params.to.unwrap_or_else(Utc::now);
    let from = params.from.unwrap_or_else(|| to - chrono::Duration::hours(1));

    let anomalies = state.ledger.detect_anomalies(params.app_id, Some(from), Some(to)).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let entries: Vec<AnomalyEntry> = anomalies.into_iter().map(|a| AnomalyEntry {
        metric: a.metric,
        current_value: a.current_value,
        baseline_value: a.baseline_value,
        deviation_factor: a.deviation_factor,
        reason: a.reason,
        detected_at: a.detected_at,
    }).collect();

    Ok(Json(AnomalyResponse {
        app_id: params.app_id,
        anomalies: entries,
        checked_from: from,
        checked_to: to,
    }))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "cost-accounting" }))
}
