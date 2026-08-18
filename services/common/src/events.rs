use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{Decision, Verdict};
use crate::types::{AppId, CorrelationId, Outcome};

// =============================================================================
// Event envelope — wraps every NATS/in-process message
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope<T> {
    pub id: Uuid,
    pub subject: String,
    pub correlation_id: CorrelationId,
    pub app_id: AppId,
    pub timestamp: DateTime<Utc>,
    pub payload: T,
}

impl<T: Serialize> EventEnvelope<T> {
    pub fn new(subject: impl Into<String>, correlation_id: CorrelationId, app_id: AppId, payload: T) -> Self {
        Self {
            id: Uuid::now_v7(),
            subject: subject.into(),
            correlation_id,
            app_id,
            timestamp: Utc::now(),
            payload,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}

impl<T: for<'de> Deserialize<'de>> EventEnvelope<T> {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

// =============================================================================
// NATS subjects
// =============================================================================

pub mod subjects {
    pub const INTERCEPT_CAPTURED: &str = "controlplane.intercept.captured";
    pub const INTERCEPT_SHADOW: &str = "controlplane.intercept.shadow";
    pub const VERDICT_FAST: &str = "controlplane.verdict.fast";
    pub const VERDICT_SHADOW: &str = "controlplane.verdict.shadow";
    pub const DECISION_FINAL: &str = "controlplane.decision.final";
    pub const ESCALATION_CREATED: &str = "controlplane.escalation.created";
    pub const POLICY_UPDATED: &str = "controlplane.policy.updated";
}

// =============================================================================
// Typed event payloads
// =============================================================================

/// Published by the proxy after capturing a request/response pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterceptCapturedPayload {
    pub call_id: Uuid,
    pub model: String,
    pub token_count_input: Option<i32>,
    pub token_count_output: Option<i32>,
    pub upstream_latency_ms: Option<i32>,
}

/// Published by the proxy to trigger shadow-path analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowAnalysisRequest {
    pub call_id: Uuid,
    pub request_payload: Option<serde_json::Value>,
    pub response_payload: Option<serde_json::Value>,
    pub model: String,
    pub token_count_output: Option<i32>,
}

/// Published by fast-path or shadow-path when a verdict is produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerdictPayload {
    pub verdict: Verdict,
}

/// Published by the decision engine with the final aggregated outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionPayload {
    pub decision: Decision,
}

/// Published when an escalation case is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationCreatedPayload {
    pub escalation_id: Uuid,
    pub verdict_id: Uuid,
    pub call_id: Uuid,
    pub axis: String,
    pub confidence: f32,
    pub reason: String,
}

/// Published when a policy is updated (triggers fast-path cache reload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyUpdatedPayload {
    pub policy_id: Uuid,
    pub app_id: AppId,
    pub axis: String,
    pub new_version: i32,
}

// =============================================================================
// Notification payload (consumed by notification service)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPayload {
    pub app_name: String,
    pub outcome: Outcome,
    pub axis: String,
    pub reason: String,
    pub correlation_id: CorrelationId,
    pub dashboard_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Axis, Path};

    #[test]
    fn envelope_roundtrip() {
        let verdict = Verdict {
            id: Uuid::now_v7(),
            call_id: Uuid::now_v7(),
            axis: Axis::Responsibility,
            path: Path::Fast,
            outcome: Outcome::Edit,
            confidence: 0.95,
            reason: "Secret detected".into(),
            check_name: "secret_detection".into(),
            duration_ms: Some(3),
            metadata: None,
            created_at: Utc::now(),
        };

        let envelope = EventEnvelope::new(
            subjects::VERDICT_FAST,
            Uuid::now_v7(),
            Uuid::now_v7(),
            VerdictPayload { verdict: verdict.clone() },
        );

        let bytes = envelope.to_bytes().unwrap();
        let parsed: EventEnvelope<VerdictPayload> = EventEnvelope::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.subject, subjects::VERDICT_FAST);
        assert_eq!(parsed.payload.verdict.outcome, Outcome::Edit);
        assert_eq!(parsed.payload.verdict.check_name, "secret_detection");
    }

    use crate::models::Verdict;
}
