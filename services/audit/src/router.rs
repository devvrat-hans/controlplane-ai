use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use controlplane_common::events::{subjects, DecisionPayload, EventEnvelope};
use controlplane_platform::messaging::EventSubscriber;
use tracing::{error, info, warn};

use crate::repository::{AuditQueryFilters, AuditRepository, VerificationResult};

pub struct AuditServiceState {
    pub repository: AuditRepository,
}

/// HTTP router for audit endpoints.
pub fn audit_router(state: Arc<AuditServiceState>) -> Router {
    Router::new()
        .route("/api/v1/audit", get(query_audit))
        .route("/api/v1/audit/verify", get(verify_chain))
        .route("/api/v1/audit/export", get(export_audit))
        .with_state(state)
}

// === Query Params ===

#[derive(Deserialize)]
struct AuditQueryParams {
    app_id: Option<Uuid>,
    action: Option<String>,
    axis: Option<String>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    cursor: Option<DateTime<Utc>>,
    limit: Option<i32>,
}

#[derive(Deserialize)]
struct ExportParams {
    app_id: Option<Uuid>,
    action: Option<String>,
    axis: Option<String>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    /// "csv" or "json" (default: json)
    format: Option<String>,
}

#[derive(Deserialize)]
struct VerifyParams {
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
}

// === Response Types ===

#[derive(Serialize)]
struct AuditQueryResponse {
    records: Vec<AuditRecordResponse>,
    next_cursor: Option<String>,
    total_returned: usize,
}

#[derive(Serialize)]
struct AuditRecordResponse {
    id: Uuid,
    call_id: Uuid,
    verdict_id: Uuid,
    action_taken: String,
    record_hash: String,
    metadata: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
}

// === Handlers ===

async fn query_audit(
    State(state): State<Arc<AuditServiceState>>,
    Query(params): Query<AuditQueryParams>,
) -> Result<Json<AuditQueryResponse>, (StatusCode, String)> {
    let filters = AuditQueryFilters {
        app_id: params.app_id,
        action: params.action,
        axis: params.axis,
        from: params.from,
        to: params.to,
        cursor: params.cursor,
        limit: params.limit,
    };

    let records = state.repository.query(&filters).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let next_cursor = records.last().map(|r| r.created_at.to_rfc3339());
    let total_returned = records.len();

    let records: Vec<AuditRecordResponse> = records.into_iter().map(|r| AuditRecordResponse {
        id: r.id,
        call_id: r.call_id,
        verdict_id: r.verdict_id,
        action_taken: r.action_taken,
        record_hash: r.record_hash,
        metadata: r.metadata,
        created_at: r.created_at,
    }).collect();

    Ok(Json(AuditQueryResponse { records, next_cursor, total_returned }))
}

async fn verify_chain(
    State(state): State<Arc<AuditServiceState>>,
    Query(params): Query<VerifyParams>,
) -> Result<Json<VerificationResult>, (StatusCode, String)> {
    let result = state.repository.verify_chain(params.from, params.to).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(result))
}

async fn export_audit(
    State(state): State<Arc<AuditServiceState>>,
    Query(params): Query<ExportParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let filters = AuditQueryFilters {
        app_id: params.app_id,
        action: params.action,
        axis: params.axis,
        from: params.from,
        to: params.to,
        cursor: None,
        limit: Some(10000), // Export max
    };

    let records = state.repository.query(&filters).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let format = params.format.as_deref().unwrap_or("json");

    match format {
        "csv" => {
            let mut csv = String::from("id,call_id,verdict_id,action_taken,record_hash,created_at\n");
            for r in &records {
                csv.push_str(&format!(
                    "{},{},{},{},{},{}\n",
                    r.id, r.call_id, r.verdict_id, r.action_taken, r.record_hash, r.created_at.to_rfc3339()
                ));
            }

            Ok((
                [(axum::http::header::CONTENT_TYPE, "text/csv"),
                 (axum::http::header::CONTENT_DISPOSITION, "attachment; filename=\"audit_export.csv\"")],
                csv,
            ).into_response())
        }
        _ => {
            let json_records: Vec<AuditRecordResponse> = records.into_iter().map(|r| AuditRecordResponse {
                id: r.id,
                call_id: r.call_id,
                verdict_id: r.verdict_id,
                action_taken: r.action_taken,
                record_hash: r.record_hash,
                metadata: r.metadata,
                created_at: r.created_at,
            }).collect();

            Ok((
                [(axum::http::header::CONTENT_TYPE, "application/json"),
                 (axum::http::header::CONTENT_DISPOSITION, "attachment; filename=\"audit_export.json\"")],
                serde_json::to_string_pretty(&json_records).unwrap_or_default(),
            ).into_response())
        }
    }
}

/// Background subscriber: listens for decisions and auto-creates audit records.
pub fn spawn_audit_subscriber(
    pool: PgPool,
    subscriber: Arc<dyn EventSubscriber>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let repository = AuditRepository::new(pool);

    tokio::spawn(async move {
        let mut receiver = match subscriber.subscribe(subjects::DECISION_FINAL).await {
            Ok(rx) => rx,
            Err(e) => {
                error!(error = %e, "Audit subscriber: failed to subscribe to decisions");
                return;
            }
        };

        info!("Audit subscriber: listening on '{}'", subjects::DECISION_FINAL);

        loop {
            tokio::select! {
                msg = receiver.recv() => {
                    match msg {
                        Some(payload) => {
                            match serde_json::from_slice::<EventEnvelope<DecisionPayload>>(&payload) {
                                Ok(envelope) => {
                                    let decision = &envelope.payload.decision;
                                    let action = decision.final_outcome.as_str();

                                    // Create audit record for each contributing verdict
                                    if decision.contributing_verdicts.is_empty() {
                                        // Still log the decision even with no contributing verdicts
                                        let metadata = serde_json::json!({
                                            "app_id": decision.app_id,
                                            "policy_version": decision.applied_policy_version,
                                        });
                                        if let Err(e) = repository.append(
                                            decision.call_id,
                                            Uuid::nil(), // no specific verdict
                                            action,
                                            Some(metadata),
                                        ).await {
                                            error!(error = %e, "Failed to append audit record");
                                        }
                                    } else {
                                        for &verdict_id in &decision.contributing_verdicts {
                                            let metadata = serde_json::json!({
                                                "app_id": decision.app_id,
                                                "policy_version": decision.applied_policy_version,
                                            });
                                            if let Err(e) = repository.append(
                                                decision.call_id,
                                                verdict_id,
                                                action,
                                                Some(metadata),
                                            ).await {
                                                error!(error = %e, "Failed to append audit record");
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(error = %e, "Failed to deserialize decision payload");
                                }
                            }
                        }
                        None => {
                            info!("Audit subscriber: subscription closed");
                            break;
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Audit subscriber shutting down");
                        break;
                    }
                }
            }
        }
    });
}
