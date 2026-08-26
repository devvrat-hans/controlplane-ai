use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use controlplane_common::types::Resolution;

use crate::queue::{EscalationFilters, EscalationQueue, EscalationRow};

pub struct EscalationServiceState {
    pub queue: EscalationQueue,
}

/// HTTP router for escalation endpoints.
pub fn escalation_router(state: Arc<EscalationServiceState>) -> Router {
    Router::new()
        .route("/api/v1/escalations", get(list_escalations))
        .route("/api/v1/escalations/{id}", get(get_escalation))
        .route("/api/v1/escalations/{id}/review", post(start_review))
        .route("/api/v1/escalations/{id}/resolve", post(resolve_escalation))
        .route("/api/v1/escalations/stats", get(escalation_stats))
        .with_state(state)
}

// === Request/Response Types ===

#[derive(Deserialize)]
struct ListParams {
    status: Option<String>,
    app_id: Option<Uuid>,
    assigned_to: Option<Uuid>,
    limit: Option<i32>,
}

#[derive(Serialize)]
struct EscalationListResponse {
    escalations: Vec<EscalationResponse>,
    total: usize,
}

#[derive(Serialize)]
struct EscalationResponse {
    id: Uuid,
    verdict_id: Uuid,
    call_id: Uuid,
    app_id: Uuid,
    status: String,
    assigned_to: Option<Uuid>,
    resolution: Option<String>,
    resolution_reason: Option<String>,
    axis: String,
    confidence: f32,
    reason: String,
    created_at: DateTime<Utc>,
    resolved_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct StartReviewRequest {
    reviewer_id: Uuid,
}

#[derive(Deserialize)]
struct ResolveRequest {
    /// "confirm", "override", or "dismiss"
    action: String,
    reason: String,
}

#[derive(Serialize)]
struct ResolveResponse {
    success: bool,
    escalation_id: Uuid,
    resolution: String,
    message: String,
}

#[derive(Serialize)]
struct StatsResponse {
    open_count: i64,
}

// === Handlers ===

async fn list_escalations(
    State(state): State<Arc<EscalationServiceState>>,
    Query(params): Query<ListParams>,
) -> Result<Json<EscalationListResponse>, (StatusCode, String)> {
    let filters = EscalationFilters {
        status: params.status,
        app_id: params.app_id,
        assigned_to: params.assigned_to,
        limit: params.limit,
    };

    let rows = state.queue.list_cases(&filters).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total = rows.len();
    let escalations = rows.into_iter().map(row_to_response).collect();

    Ok(Json(EscalationListResponse { escalations, total }))
}

async fn get_escalation(
    State(state): State<Arc<EscalationServiceState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<EscalationResponse>, (StatusCode, String)> {
    let row = state.queue.get_case(id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Escalation not found".to_string()))?;

    Ok(Json(row_to_response(row)))
}

async fn start_review(
    State(state): State<Arc<EscalationServiceState>>,
    Path(id): Path<Uuid>,
    Json(request): Json<StartReviewRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let success = state.queue.start_review(id, request.reviewer_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    if success {
        Ok(Json(serde_json::json!({
            "success": true,
            "message": "Case moved to in_review"
        })))
    } else {
        Err((StatusCode::CONFLICT, "Case is not in 'open' status".to_string()))
    }
}

async fn resolve_escalation(
    State(state): State<Arc<EscalationServiceState>>,
    Path(id): Path<Uuid>,
    Json(request): Json<ResolveRequest>,
) -> Result<Json<ResolveResponse>, (StatusCode, String)> {
    let resolution = Resolution::from_str_loose(&request.action)
        .ok_or((StatusCode::BAD_REQUEST, format!("Invalid resolution action: '{}'. Must be confirm, override, or dismiss.", request.action)))?;

    let success = state.queue.resolve_case(id, resolution, &request.reason).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    if !success {
        return Err((StatusCode::CONFLICT, "Case cannot be resolved (already resolved or not found)".to_string()));
    }

    let message = match resolution {
        Resolution::Confirm => "Verdict confirmed. This feeds the reviewer-precedent learning loop.".to_string(),
        Resolution::Override => "Verdict overridden. Recorded as a reviewer precedent for future similar calls.".to_string(),
        Resolution::Dismiss => "Case dismissed as false positive. Recorded as a reviewer precedent.".to_string(),
    };

    Ok(Json(ResolveResponse {
        success: true,
        escalation_id: id,
        resolution: resolution.as_str().to_string(),
        message,
    }))
}

async fn escalation_stats(
    State(state): State<Arc<EscalationServiceState>>,
) -> Result<Json<StatsResponse>, (StatusCode, String)> {
    let open_count = state.queue.open_count().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(StatsResponse { open_count }))
}

fn row_to_response(row: EscalationRow) -> EscalationResponse {
    EscalationResponse {
        id: row.id,
        verdict_id: row.verdict_id,
        call_id: row.call_id,
        app_id: row.app_id,
        status: row.status,
        assigned_to: row.assigned_to,
        resolution: row.resolution,
        resolution_reason: row.resolution_reason,
        axis: row.axis,
        confidence: row.confidence,
        reason: row.reason,
        created_at: row.created_at,
        resolved_at: row.resolved_at,
    }
}
