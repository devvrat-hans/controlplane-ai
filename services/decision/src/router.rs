use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{error, info};
use uuid::Uuid;

use controlplane_common::events::{subjects, DecisionPayload, EventEnvelope, VerdictPayload};
use controlplane_common::models::{Decision, Verdict};
use controlplane_common::types::AppId;
use controlplane_platform::messaging::{EventPublisher, EventSubscriber};

use crate::aggregator::VerdictAggregator;
use crate::policy::PolicyEngine;

pub struct DecisionServiceState {
    pub pool: PgPool,
    pub aggregator: VerdictAggregator,
    pub policy_engine: PolicyEngine,
    pub publisher: Arc<dyn EventPublisher>,
}

/// HTTP router for the decision service.
pub fn decision_router(state: Arc<DecisionServiceState>) -> Router {
    Router::new()
        .route("/api/v1/decision/aggregate", post(aggregate_verdicts))
        .route("/api/v1/decision/health", get(health))
        .with_state(state)
}

#[derive(Deserialize)]
struct AggregateRequest {
    call_id: Uuid,
    app_id: AppId,
    verdicts: Vec<VerdictInput>,
}

#[derive(Deserialize)]
struct VerdictInput {
    id: Uuid,
    axis: String,
    path: String,
    outcome: String,
    confidence: f32,
    reason: String,
    check_name: String,
    duration_ms: Option<i32>,
}

#[derive(Serialize)]
struct AggregateResponse {
    call_id: Uuid,
    final_outcome: String,
    primary_reason: String,
    contributing_verdict_count: usize,
    applied_policy_version: Option<i32>,
}

async fn aggregate_verdicts(
    State(state): State<Arc<DecisionServiceState>>,
    Json(request): Json<AggregateRequest>,
) -> Result<Json<AggregateResponse>, (StatusCode, String)> {
    let verdicts: Vec<Verdict> = request.verdicts.iter().map(|v| {
        let axis = controlplane_common::types::Axis::from_str_loose(&v.axis)
            .unwrap_or(controlplane_common::types::Axis::Responsibility);
        let path = controlplane_common::types::Path::from_str_loose(&v.path);
        let outcome = controlplane_common::types::Outcome::from_str_loose(&v.outcome)
            .unwrap_or(controlplane_common::types::Outcome::Pass);

        let mut verdict = Verdict::new(
            request.call_id,
            axis,
            path,
            outcome,
            v.confidence,
            &v.reason,
            &v.check_name,
        );
        verdict.id = v.id;
        if let Some(ms) = v.duration_ms {
            verdict = verdict.with_duration(ms);
        }
        verdict
    }).collect();

    // Load policy for this app
    let policy = state.policy_engine.load_policy(request.app_id).await;

    // Aggregate verdicts
    let result = state.aggregator.aggregate_with_reasoning(&verdicts);

    // Produce DecisionRecord
    let decision = Decision::from_verdicts(
        request.call_id,
        request.app_id,
        &verdicts,
        Some(policy.version),
    );

    // Persist verdicts to DB
    for verdict in &verdicts {
        if let Err(e) = persist_verdict(&state.pool, verdict).await {
            error!(error = %e, "Failed to persist verdict");
        }
    }

    // Publish decision to NATS
    let decision_payload = DecisionPayload { decision: decision.clone() };
    let envelope = EventEnvelope::new(
        subjects::DECISION_FINAL,
        request.call_id,
        request.app_id,
        decision_payload,
    );

    if let Ok(bytes) = envelope.to_bytes() {
        if let Err(e) = state.publisher.publish(subjects::DECISION_FINAL, &bytes).await {
            error!(error = %e, "Failed to publish decision");
        }
    }

    info!(
        call_id = %request.call_id,
        final_outcome = %result.final_outcome,
        policy_version = policy.version,
        "Decision rendered"
    );

    Ok(Json(AggregateResponse {
        call_id: request.call_id,
        final_outcome: result.final_outcome.as_str().to_string(),
        primary_reason: result.primary_reason,
        contributing_verdict_count: result.contributing_ids.len(),
        applied_policy_version: Some(policy.version),
    }))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "decision" }))
}

/// Persist a verdict to the verdicts table.
async fn persist_verdict(pool: &PgPool, verdict: &Verdict) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO verdicts (id, call_id, axis, path, outcome, confidence, reason, check_name, duration_ms, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(verdict.id)
    .bind(verdict.call_id)
    .bind(verdict.axis.as_str())
    .bind(verdict.path.as_str())
    .bind(verdict.outcome.as_str())
    .bind(verdict.confidence)
    .bind(&verdict.reason)
    .bind(&verdict.check_name)
    .bind(verdict.duration_ms)
    .bind(verdict.created_at)
    .execute(pool)
    .await?;

    Ok(())
}

/// Background subscriber that listens for verdicts on NATS
/// and automatically aggregates them when all paths report.
pub fn spawn_verdict_collector(
    state: Arc<DecisionServiceState>,
    subscriber: Arc<dyn EventSubscriber>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut receiver = match subscriber.subscribe("controlplane.verdict.*").await {
            Ok(rx) => rx,
            Err(e) => {
                error!(error = %e, "Decision service: failed to subscribe to verdicts");
                return;
            }
        };

        info!("Decision service: listening for verdicts on 'controlplane.verdict.*'");

        loop {
            tokio::select! {
                msg = receiver.recv() => {
                    match msg {
                        Some(payload) => {
                            if let Ok(envelope) = serde_json::from_slice::<EventEnvelope<VerdictPayload>>(&payload) {
                                let verdict = envelope.payload.verdict;
                                if let Err(e) = persist_verdict(&state.pool, &verdict).await {
                                    error!(error = %e, "Failed to persist collected verdict");
                                }
                            }
                        }
                        None => {
                            info!("Verdict collector subscription closed");
                            break;
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Verdict collector shutting down");
                        break;
                    }
                }
            }
        }
    });
}
