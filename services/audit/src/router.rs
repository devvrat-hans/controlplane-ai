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

use controlplane_common::events::{EventEnvelope, VerdictPayload};
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
        let mut receiver = match subscriber.subscribe("controlplane.verdict.*").await {
            Ok(rx) => rx,
            Err(e) => {
                error!(error = %e, "Audit subscriber: failed to subscribe to verdicts");
                return;
            }
        };

        info!("Audit subscriber: listening on 'controlplane.verdict.*' (fast + shadow)");

        loop {
            tokio::select! {
                msg = receiver.recv() => {
                    match msg {
                        Some(payload) => {
                            match serde_json::from_slice::<EventEnvelope<VerdictPayload>>(&payload) {
                                Ok(envelope) => {
                                    let app_id = envelope.app_id;
                                    let verdict = &envelope.payload.verdict;
                                    let action = verdict.outcome.as_str().to_string();
                                    let metadata = serde_json::json!({
                                        "axis": verdict.axis.as_str(),
                                        "check_name": verdict.check_name,
                                        "confidence": verdict.confidence,
                                    });
                                    let call_id = verdict.call_id;
                                    let verdict_id = verdict.id;

                                    // Retry up to 3 times with backoff (verdict row may not exist yet)
                                    let mut attempts: u64 = 0;
                                    loop {
                                        attempts += 1;
                                        match repository.append(
                                            call_id,
                                            verdict_id,
                                            app_id,
                                            &action,
                                            Some(metadata.clone()),
                                        ).await {
                                            Ok(_) => break,
                                            Err(e) if attempts < 3 => {
                                                warn!(error = %e, attempt = attempts, "Audit insert failed, retrying...");
                                                tokio::time::sleep(tokio::time::Duration::from_millis(100 * attempts)).await;
                                            }
                                            Err(e) => {
                                                error!(error = %e, "Failed to append audit record after retries");
                                                break;
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(error = %e, "Failed to deserialize verdict payload");
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
