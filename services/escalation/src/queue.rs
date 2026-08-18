use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::{error, info};
use uuid::Uuid;

use controlplane_common::events::{subjects, DecisionPayload, EventEnvelope};
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

            // If override, signal policy reload
            if resolution == Resolution::Override {
                let _ = self.publisher.publish("controlplane.policy.reload", b"escalation_override").await;
            }
        }

        Ok(result.rows_affected() > 0)
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

/// Background subscriber that creates escalation cases from decision events.
pub fn spawn_escalation_listener(
    pool: PgPool,
    subscriber: Arc<dyn EventSubscriber>,
    publisher: Arc<dyn EventPublisher>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let queue = EscalationQueue::new(pool, publisher);

    tokio::spawn(async move {
        let mut receiver = match subscriber.subscribe(subjects::DECISION_FINAL).await {
            Ok(rx) => rx,
            Err(e) => {
                error!(error = %e, "Escalation listener: failed to subscribe");
                return;
            }
        };

        info!("Escalation listener: monitoring '{}'", subjects::DECISION_FINAL);

        loop {
            tokio::select! {
                msg = receiver.recv() => {
                    match msg {
                        Some(payload) => {
                            if let Ok(envelope) = serde_json::from_slice::<EventEnvelope<DecisionPayload>>(&payload) {
                                let decision = &envelope.payload.decision;
                                if decision.final_outcome == Outcome::Escalate {
                                    // Look up the verdict that caused escalation
                                    // For now, create case from the decision metadata
                                    if let Err(e) = queue.create_case(
                                        decision.contributing_verdicts.first().copied().unwrap_or(Uuid::nil()),
                                        decision.call_id,
                                        decision.app_id,
                                        "unknown", // axis from verdict
                                        0.0,
                                        "Escalated by decision engine",
                                    ).await {
                                        error!(error = %e, "Failed to create escalation case");
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
