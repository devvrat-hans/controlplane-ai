use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::{error, info, warn};
use uuid::Uuid;

use controlplane_common::events::{subjects, EventEnvelope, VerdictPayload};
use controlplane_common::types::{AppId, Outcome, Resolution};
use controlplane_platform::messaging::{EventPublisher, EventSubscriber};

/// Repository for escalation case lifecycle management.
pub struct EscalationQueue {
    pool: PgPool,
    publisher: Arc<dyn EventPublisher>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EscalationRow {
    pub id: Uuid,
    pub verdict_id: Uuid,
    pub call_id: Uuid,
    pub app_id: Uuid,
    pub status: String,
    pub assigned_to: Option<Uuid>,
    pub resolution: Option<String>,
    pub resolution_reason: Option<String>,
    pub axis: String,
    pub confidence: f32,
    pub reason: String,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct EscalationFilters {
    pub status: Option<String>,
    pub app_id: Option<Uuid>,
    pub assigned_to: Option<Uuid>,
    pub limit: Option<i32>,
}

impl EscalationQueue {
    pub fn new(pool: PgPool, publisher: Arc<dyn EventPublisher>) -> Self {
        Self { pool, publisher }
    }

    /// Create a new escalation case from a verdict.
    pub async fn create_case(
        &self,
        verdict_id: Uuid,
        call_id: Uuid,
        app_id: AppId,
        axis: &str,
        confidence: f32,
        reason: &str,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::now_v7();

        sqlx::query(
            "INSERT INTO escalation_cases (id, verdict_id, call_id, app_id, status, axis, confidence, reason, created_at) \
             VALUES ($1, $2, $3, $4, 'open', $5, $6, $7, $8)"
        )
        .bind(id)
        .bind(verdict_id)
        .bind(call_id)
        .bind(app_id)
        .bind(axis)
        .bind(confidence)
        .bind(reason)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;

        info!(
            escalation_id = %id,
            call_id = %call_id,
            axis,
            "Escalation case created"
        );

        // Publish escalation event
        let payload = serde_json::json!({
            "escalation_id": id,
            "call_id": call_id,
            "app_id": app_id,
            "axis": axis,
            "confidence": confidence,
            "reason": reason,
        });
        let envelope = EventEnvelope::new(
            subjects::ESCALATION_CREATED,
            call_id,
            app_id,
            payload,
        );
        if let Ok(bytes) = envelope.to_bytes() {
            let _ = self.publisher.publish(subjects::ESCALATION_CREATED, &bytes).await;
        }

        Ok(id)
    }

    /// Get a single escalation case by ID.
    pub async fn get_case(&self, id: Uuid) -> Result<Option<EscalationRow>, sqlx::Error> {
        sqlx::query_as::<_, EscalationRow>(
            "SELECT id, verdict_id, call_id, app_id, status, assigned_to, resolution, \
             resolution_reason, axis, confidence, reason, created_at, resolved_at \
             FROM escalation_cases WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
    }

    /// List escalation cases with filters.
    pub async fn list_cases(&self, filters: &EscalationFilters) -> Result<Vec<EscalationRow>, sqlx::Error> {
        let mut query = String::from(
            "SELECT id, verdict_id, call_id, app_id, status, assigned_to, resolution, \
             resolution_reason, axis, confidence, reason, created_at, resolved_at \
             FROM escalation_cases WHERE 1=1"
        );

        let mut bind_idx = 1;

        if filters.status.is_some() {
            query.push_str(&format!(" AND status = ${bind_idx}"));
            bind_idx += 1;
        }
        if filters.app_id.is_some() {
            query.push_str(&format!(" AND app_id = ${bind_idx}"));
            bind_idx += 1;
        }
        if filters.assigned_to.is_some() {
            query.push_str(&format!(" AND assigned_to = ${bind_idx}"));
            bind_idx += 1;
        }

        query.push_str(" ORDER BY created_at DESC");
        query.push_str(&format!(" LIMIT ${bind_idx}"));
        let _ = bind_idx;

        let limit = filters.limit.unwrap_or(50).min(200) as i64;

        let mut db_query = sqlx::query_as::<_, EscalationRow>(&query);

        if let Some(status) = &filters.status {
            db_query = db_query.bind(status);
        }
        if let Some(app_id) = filters.app_id {
            db_query = db_query.bind(app_id);
        }
        if let Some(assigned_to) = filters.assigned_to {
            db_query = db_query.bind(assigned_to);
        }
        db_query = db_query.bind(limit);

        db_query.fetch_all(&self.pool).await
    }

    /// Transition a case to IN_REVIEW status.
    pub async fn start_review(&self, id: Uuid, reviewer_id: Uuid) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE escalation_cases SET status = 'in_review', assigned_to = $1 \
             WHERE id = $2 AND status = 'open'"
        )
        .bind(reviewer_id)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Resolve a case with a resolution action.
    ///
    /// Every resolution — confirm, override, or dismiss — is captured as a
    /// reviewer precedent in `reviewer_overrides` (the feedback/RAG learning
    /// store) so similar future calls can retrieve past human corrections.
    pub async fn resolve_case(
        &self,
        id: Uuid,
        resolution: Resolution,
        reason: &str,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE escalation_cases SET status = 'resolved', resolution = $1, \
             resolution_reason = $2, resolved_at = $3 \
             WHERE id = $4 AND status IN ('open', 'in_review')"
        )
        .bind(resolution.as_str())
        .bind(reason)
        .bind(Utc::now())
        .bind(id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() > 0 {
            info!(
                escalation_id = %id,
                resolution = resolution.as_str(),
                "Escalation case resolved"
            );

            // Capture the precedent for the retraining loop (fail-open: a capture
            // error must not fail the resolution itself).
            if let Err(e) = self.capture_precedent(id, resolution, reason).await {
                warn!(error = %e, escalation_id = %id, "Failed to capture reviewer precedent");
            }

            // If override, signal policy reload
            if resolution == Resolution::Override {
                let _ = self.publisher.publish("controlplane.policy.reload", b"escalation_override").await;
            }
        }

        Ok(result.rows_affected() > 0)
    }

    /// Store this resolution in the reviewer_overrides learning table and
    /// broadcast a feedback event.
    async fn capture_precedent(
        &self,
        case_id: Uuid,
        resolution: Resolution,
        reason: &str,
    ) -> Result<(), sqlx::Error> {
        let case = match self.get_case(case_id).await? {
            Some(c) => c,
            None => return Ok(()),
        };

        // Pull request/response context for similarity matching later
        let excerpts: Option<(Option<serde_json::Value>, Option<serde_json::Value>, Option<Uuid>)> =
            sqlx::query_as(
                "SELECT request_payload, response_payload, session_id FROM intercepted_calls WHERE id = $1"
            )
            .bind(case.call_id)
            .fetch_optional(&self.pool)
            .await?;

        let (request_excerpt, response_excerpt, session_id) = match excerpts {
            Some((req, resp, sid)) => (
                req.map(|v| truncate_excerpt(&v.to_string())),
                resp.map(|v| truncate_excerpt(&v.to_string())),
                sid,
            ),
            None => (None, None, None),
        };

        let precedent_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO reviewer_overrides (id, escalation_id, call_id, app_id, verdict_id, axis, \
             model_outcome, model_confidence, reviewer_action, reviewer_reason, \
             request_excerpt, response_excerpt, session_id, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, 'escalate', $7, $8, $9, $10, $11, $12, NOW())"
        )
        .bind(precedent_id)
        .bind(case.id)
        .bind(case.call_id)
        .bind(case.app_id)
        .bind(case.verdict_id)
        .bind(&case.axis)
        .bind(case.confidence)
        .bind(resolution.as_str())
        .bind(reason)
        .bind(&request_excerpt)
        .bind(&response_excerpt)
        .bind(session_id)
        .execute(&self.pool)
        .await?;

        // Broadcast so other services know new learning data exists
        let payload = serde_json::json!({
            "precedent_id": precedent_id,
            "escalation_id": case.id,
            "call_id": case.call_id,
            "app_id": case.app_id,
            "axis": case.axis,
            "reviewer_action": resolution.as_str(),
        });
        let envelope = EventEnvelope::new(
            controlplane_common::events::subjects::FEEDBACK_RECORDED,
            case.call_id,
            AppId::from(case.app_id),
            payload,
        );
        if let Ok(bytes) = envelope.to_bytes() {
            let _ = self.publisher.publish(controlplane_common::events::subjects::FEEDBACK_RECORDED, &bytes).await;
        }

        info!(precedent_id = %precedent_id, escalation_id = %case.id, "Reviewer precedent captured");
        Ok(())
    }

    /// Get open case count for dashboard stats.
    pub async fn open_count(&self) -> Result<i64, sqlx::Error> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM escalation_cases WHERE status = 'open'"
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(count.0)
    }
}

/// Truncate an excerpt to keep the trigram index lean.
fn truncate_excerpt(s: &str) -> String {
    s.chars().take(2000).collect()
}

/// Background subscriber that creates escalation cases from verdict events.
pub fn spawn_escalation_listener(
    pool: PgPool,
    subscriber: Arc<dyn EventSubscriber>,
    publisher: Arc<dyn EventPublisher>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let queue = EscalationQueue::new(pool, publisher);

    tokio::spawn(async move {
        // Subscribe to all verdicts (fast + shadow) to catch escalations from either path
        let mut receiver = match subscriber.subscribe("controlplane.verdict.*").await {
            Ok(rx) => rx,
            Err(e) => {
                error!(error = %e, "Escalation listener: failed to subscribe to verdicts");
                return;
            }
        };

        info!("Escalation listener: monitoring 'controlplane.verdict.*' (fast + shadow)");

        loop {
            tokio::select! {
                msg = receiver.recv() => {
                    match msg {
                        Some(payload) => {
                            // Try to parse as a verdict event
                            if let Ok(envelope) = serde_json::from_slice::<EventEnvelope<VerdictPayload>>(&payload) {
                                let verdict = &envelope.payload.verdict;
                                if verdict.outcome == Outcome::Escalate {
                                    // Look up app_id from intercepted_calls if envelope has nil
                                    let app_id = if envelope.app_id.is_nil() {
                                        match sqlx::query_scalar::<_, Uuid>(
                                            "SELECT app_id FROM intercepted_calls WHERE id = $1"
                                        )
                                        .bind(verdict.call_id)
                                        .fetch_optional(&queue.pool)
                                        .await
                                        {
                                            Ok(Some(id)) => id,
                                            _ => {
                                                error!(call_id = %verdict.call_id, "Cannot find app_id for call");
                                                continue;
                                            }
                                        }
                                    } else {
                                        envelope.app_id
                                    };

                                    // Alert fatigue mitigation: skip if duplicate open case
                                    // exists for same app + axis within the last hour
                                    let is_duplicate: bool = sqlx::query_scalar(
                                        "SELECT EXISTS(SELECT 1 FROM escalation_cases \
                                         WHERE app_id = $1 AND axis = $2 AND status = 'open' \
                                         AND created_at > NOW() - INTERVAL '1 hour')"
                                    )
                                    .bind(app_id)
                                    .bind(verdict.axis.as_str())
                                    .fetch_one(&queue.pool)
                                    .await
                                    .unwrap_or(false);

                                    if is_duplicate {
                                        info!(
                                            app_id = %app_id,
                                            axis = verdict.axis.as_str(),
                                            "Escalation dedup: skipping duplicate (same app+axis within 1h)"
                                        );
                                        continue;
                                    }

                                    info!(
                                        correlation_id = %envelope.correlation_id,
                                        verdict_id = %verdict.id,
                                        app_id = %app_id,
                                        "Escalation listener: creating case from escalated verdict"
                                    );
                                    // Retry with backoff (verdict row may not be persisted yet)
                                    let mut attempts: u64 = 0;
                                    loop {
                                        attempts += 1;
                                        match queue.create_case(
                                            verdict.id,
                                            verdict.call_id,
                                            app_id,
                                            verdict.axis.as_str(),
                                            verdict.confidence,
                                            &verdict.reason,
                                        ).await {
                                            Ok(_) => break,
                                            Err(e) if attempts < 3 => {
                                                warn!(error = %e, attempt = attempts, "Escalation case insert failed, retrying...");
                                                tokio::time::sleep(tokio::time::Duration::from_millis(150 * attempts)).await;
                                            }
                                            Err(e) => {
                                                error!(error = %e, "Failed to create escalation case after retries");
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        None => {
                            info!("Escalation listener subscription closed");
                            break;
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Escalation listener shutting down");
                        break;
                    }
                }
            }
        }
    });
}
