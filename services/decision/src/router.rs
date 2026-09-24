use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{debug, error, info};
use uuid::Uuid;

use controlplane_common::events::{subjects, DecisionPayload, EventEnvelope, VerdictPayload};
use controlplane_common::models::{Decision, Verdict};
use controlplane_common::types::{AppId, Outcome};
use controlplane_platform::messaging::{EventPublisher, EventSubscriber};

use crate::aggregator::{AxisFusion, FusionConfig, FusionResult, VerdictAggregator};
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
    /// Precedent IDs from the reviewer-override learning store that were
    /// consulted for this decision (auditable explainability).
    consulted_precedents: Vec<Uuid>,
    /// Calibrated-fusion detail. `applied: false` means no calibration fit exists yet
    /// and the pre-existing aggregator produced the outcome.
    fusion: FusionSummary,
}

/// Explainability for the calibration-aware path (plan §5.5 / §5.6).
#[derive(Serialize, Default)]
struct FusionSummary {
    applied: bool,
    calibration_version: Option<i32>,
    /// True when the judge and the heuristics materially disagreed on some axis and the
    /// case was routed to a human for that reason.
    disagreement: bool,
    disagreement_reason: Option<String>,
    axes: Vec<AxisSummary>,
}

#[derive(Serialize)]
struct AxisSummary {
    axis: String,
    /// Fused probability for this axis (weighted noisy-OR).
    p: f32,
    outcome: String,
    detectors: Vec<DetectorSummary>,
}

#[derive(Serialize)]
struct DetectorSummary {
    check_name: String,
    p: f32,
    /// `None` when the detector is outside the fit and was passed through unchanged.
    weight: Option<f32>,
    calibrated: bool,
}

impl From<&FusionResult> for FusionSummary {
    fn from(fused: &FusionResult) -> Self {
        Self {
            applied: true,
            calibration_version: fused.calibration_version,
            disagreement: fused.disagreement,
            disagreement_reason: fused.disagreement_reason.clone(),
            axes: fused
                .axes
                .iter()
                .map(|AxisFusion { axis, p, outcome, detectors }| AxisSummary {
                    axis: axis.as_str().to_string(),
                    p: *p,
                    outcome: outcome.as_str().to_string(),
                    detectors: detectors
                        .iter()
                        .map(|d| DetectorSummary {
                            check_name: d.check_name.clone(),
                            p: d.p,
                            weight: d.weight,
                            calibrated: d.calibrated,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
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

    // Aggregate verdicts.
    //
    // The calibrated fusion only engages when a fit exists in `detector_calibration`;
    // otherwise `fuse_evidence` returns `None` and the pre-existing aggregator runs,
    // so an un-fitted deployment keeps its exact previous behaviour (plan §7).
    let fusion_config = load_fusion_config(&state.pool).await;
    let fused = state.aggregator.fuse_evidence(&verdicts, &fusion_config);
    let fusion_summary = fused
        .as_ref()
        .map(FusionSummary::from)
        .unwrap_or_default();

    if fusion_summary.applied {
        info!(
            call_id = %request.call_id,
            calibration_version = ?fusion_summary.calibration_version,
            disagreement = fusion_summary.disagreement,
            "Calibrated fusion applied"
        );
    }

    let mut result = match fused {
        Some(fused) => fused.aggregation,
        None => state.aggregator.aggregate_with_reasoning(&verdicts),
    };

    // --- Feedback RAG: retrieve similar past reviewer decisions and suppress/annotate ---
    let mut consulted_precedents: Vec<Uuid> = Vec::new();
    if let Some(response_text) =
        fetch_call_response_text(&state.pool, request.call_id).await
    {
        let precedents = crate::feedback::find_similar_precedents(
            &state.pool,
            Some(request.app_id),
            &response_text,
            3,
            0.3,
        )
        .await;

        // Auto-suppress: if a strongly similar case (>=60%) was dismissed/overridden,
        // downgrade escalate/edit to pass. This is the active feedback loop.
        let dominated_by_dismiss = precedents.iter().any(|p| {
            p.score >= 0.6
                && (p.reviewer_action == "dismiss" || p.reviewer_action == "override")
        });
        if dominated_by_dismiss
            && (result.final_outcome == Outcome::Escalate || result.final_outcome == Outcome::Edit)
        {
            info!(
                call_id = %request.call_id,
                "Feedback loop: suppressing {} -> pass (similar case was dismissed/overridden)",
                result.final_outcome
            );
            result.final_outcome = Outcome::Pass;
            result.primary_reason = format!(
                "Auto-suppressed by feedback loop: {}. Similar past case was dismissed by a reviewer.",
                result.primary_reason
            );
        }

        let (annotation, ids) =
            crate::feedback::annotate_from_precedents(result.final_outcome.as_str(), &precedents);
        consulted_precedents = ids;
        if !annotation.is_empty() {
            result.primary_reason.push_str(&annotation);
        }
    }

    // Produce DecisionRecord
    let decision = Decision::from_verdicts(
        request.call_id,
        request.app_id,
        &verdicts,
        Some(policy.version),
    );

    // Persist verdicts to DB
    for verdict in &verdicts {
        if let Err(e) = persist_verdict(&state.pool, verdict, request.app_id).await {
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
        consulted_precedents,
        fusion: fusion_summary,
    }))
}

/// Load the fitted fusion parameters from `detector_calibration`.
///
/// Only rows with `calibrated = TRUE` are used, so the table's documented seed defaults
/// (which ship `calibrated = FALSE`) leave the fusion switched off. Any database error
/// also leaves it off — the decision service must never fail because its calibration
/// could not be read; it simply falls back to the deterministic aggregator.
///
/// Thresholds come from the same store as the weights so a policy can tighten them
/// without a redeploy; the `JUDGE_*` environment variables provide the process default.
async fn load_fusion_config(pool: &PgPool) -> FusionConfig {
    let mut config = FusionConfig {
        escalate_threshold: env_f64("JUDGE_ESCALATE_THRESHOLD", FusionConfig::default().escalate_threshold),
        edit_threshold: env_f64("JUDGE_EDIT_THRESHOLD", FusionConfig::default().edit_threshold),
        evidence_threshold: env_f64("JUDGE_EVIDENCE_THRESHOLD", FusionConfig::default().evidence_threshold),
        disagreement_delta: env_f64("JUDGE_DISAGREEMENT_DELTA", FusionConfig::default().disagreement_delta),
        ..FusionConfig::default()
    };

    let rows: Result<Vec<(String, f64, i32)>, sqlx::Error> = sqlx::query_as(
        r#"
        SELECT DISTINCT ON (detector, primitive, option_count)
               detector, weight, version
        FROM detector_calibration
        WHERE calibrated = TRUE
        ORDER BY detector, primitive, option_count, version DESC
        "#,
    )
    .fetch_all(pool)
    .await;

    let rows = match rows {
        Ok(rows) => rows,
        Err(e) => {
            debug!(error = %e, "Could not load detector calibration — using the deterministic aggregator");
            return FusionConfig::default();
        }
    };

    for (detector, weight, version) in rows {
        config.weights.insert(detector, weight.clamp(0.0, 1.0));
        config.calibration_version = Some(match config.calibration_version {
            Some(current) => current.max(version),
            None => version,
        });
    }

    config
}

/// Parse a positive `f64` from the environment, falling back to the built-in default.
fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(default)
}

/// Fetch a text representation of this call's response payload for similarity
/// matching. Fails open (None) on any error.
async fn fetch_call_response_text(pool: &PgPool, call_id: Uuid) -> Option<String> {
    let payload: Option<Option<serde_json::Value>> = sqlx::query_scalar(
        "SELECT response_payload FROM intercepted_calls WHERE id = $1"
    )
    .bind(call_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    payload.flatten().map(|v| {
        let s = v.to_string();
        s.chars().take(4000).collect()
    })
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "decision" }))
}

/// Persist a verdict to the verdicts table.
async fn persist_verdict(pool: &PgPool, verdict: &Verdict, app_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, duration_ms, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(verdict.id)
    .bind(verdict.call_id)
    .bind(app_id)
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
                                let app_id = envelope.app_id;
                                let verdict = envelope.payload.verdict;
                                if let Err(e) = persist_verdict(&state.pool, &verdict, app_id).await {
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
