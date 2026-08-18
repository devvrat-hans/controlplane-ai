use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{AppId, Axis, CorrelationId, EscalationStatus, Outcome, Path, Resolution, UserRole, UserId};

// =============================================================================
// InterceptedCall
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterceptedCall {
    pub id: Uuid,
    pub correlation_id: CorrelationId,
    pub app_id: AppId,
    pub model: String,
    pub request_payload: Option<serde_json::Value>,
    pub response_payload: Option<serde_json::Value>,
    pub token_count_input: Option<i32>,
    pub token_count_output: Option<i32>,
    pub upstream_latency_ms: Option<i32>,
    pub fast_path_latency_ms: Option<i32>,
    pub created_at: DateTime<Utc>,
}

impl InterceptedCall {
    pub fn new(app_id: AppId, model: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            correlation_id: Uuid::now_v7(),
            app_id,
            model: model.into(),
            request_payload: None,
            response_payload: None,
            token_count_input: None,
            token_count_output: None,
            upstream_latency_ms: None,
            fast_path_latency_ms: None,
            created_at: now,
        }
    }

    pub fn total_tokens(&self) -> i32 {
        self.token_count_input.unwrap_or(0) + self.token_count_output.unwrap_or(0)
    }
}

// =============================================================================
// Verdict
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verdict {
    pub id: Uuid,
    pub call_id: Uuid,
    pub axis: Axis,
    pub path: Path,
    pub outcome: Outcome,
    pub confidence: f32,
    pub reason: String,
    pub check_name: String,
    pub duration_ms: Option<i32>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

impl Verdict {
    pub fn new(
        call_id: Uuid,
        axis: Axis,
        path: Path,
        outcome: Outcome,
        confidence: f32,
        reason: impl Into<String>,
        check_name: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            call_id,
            axis,
            path,
            outcome,
            confidence,
            reason: reason.into(),
            check_name: check_name.into(),
            duration_ms: None,
            metadata: None,
            created_at: Utc::now(),
        }
    }

    pub fn with_duration(mut self, ms: i32) -> Self {
        self.duration_ms = Some(ms);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

// =============================================================================
// Policy
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: Uuid,
    pub app_id: AppId,
    pub axis: Axis,
    pub threshold_config: serde_json::Value,
    pub version: i32,
    pub is_active: bool,
    pub updated_by: Option<UserId>,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

// =============================================================================
// AuditRecord
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub id: Uuid,
    pub call_id: Uuid,
    pub verdict_id: Uuid,
    pub app_id: AppId,
    pub action_taken: String,
    pub prev_hash: String,
    pub record_hash: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

// =============================================================================
// CostLedger
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostLedgerEntry {
    pub id: Uuid,
    pub app_id: AppId,
    pub window_start: DateTime<Utc>,
    pub window_end: DateTime<Utc>,
    pub total_tokens_input: i64,
    pub total_tokens_output: i64,
    pub total_cost_cents: i64,
    pub request_count: i32,
    pub avg_tokens_per_req: Option<i32>,
    pub baseline_avg: Option<f32>,
    pub baseline_deviation: Option<f32>,
    pub created_at: DateTime<Utc>,
}

impl CostLedgerEntry {
    pub fn total_tokens(&self) -> i64 {
        self.total_tokens_input + self.total_tokens_output
    }

    pub fn cost_dollars(&self) -> f64 {
        self.total_cost_cents as f64 / 100.0
    }

    pub fn is_anomalous(&self, threshold_std_devs: f32) -> bool {
        self.baseline_deviation
            .map(|d| d.abs() > threshold_std_devs)
            .unwrap_or(false)
    }
}

// =============================================================================
// EscalationCase
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationCase {
    pub id: Uuid,
    pub verdict_id: Uuid,
    pub call_id: Uuid,
    pub app_id: AppId,
    pub status: EscalationStatus,
    pub assigned_to: Option<UserId>,
    pub resolution: Option<Resolution>,
    pub resolution_reason: Option<String>,
    pub axis: Axis,
    pub confidence: f32,
    pub reason: String,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

impl EscalationCase {
    pub fn new(verdict: &Verdict, app_id: AppId) -> Self {
        Self {
            id: Uuid::now_v7(),
            verdict_id: verdict.id,
            call_id: verdict.call_id,
            app_id,
            status: EscalationStatus::Open,
            assigned_to: None,
            resolution: None,
            resolution_reason: None,
            axis: verdict.axis,
            confidence: verdict.confidence,
            reason: verdict.reason.clone(),
            created_at: Utc::now(),
            resolved_at: None,
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self.status, EscalationStatus::Open | EscalationStatus::InReview)
    }
}

// =============================================================================
// User
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: UserRole,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
}

// =============================================================================
// App
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub id: Uuid,
    pub name: String,
    pub team_id: Option<Uuid>,
    #[serde(skip_serializing)]
    pub api_key_hash: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// =============================================================================
// Decision (aggregated final outcome for a call)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub call_id: Uuid,
    pub app_id: AppId,
    pub final_outcome: Outcome,
    pub contributing_verdicts: Vec<Uuid>,
    pub applied_policy_version: Option<i32>,
    pub created_at: DateTime<Utc>,
}

impl Decision {
    pub fn from_verdicts(call_id: Uuid, app_id: AppId, verdicts: &[Verdict], policy_version: Option<i32>) -> Self {
        let final_outcome = verdicts
            .iter()
            .map(|v| v.outcome)
            .fold(Outcome::Pass, Outcome::worst);

        let contributing_verdicts = verdicts.iter().map(|v| v.id).collect();

        Self {
            call_id,
            app_id,
            final_outcome,
            contributing_verdicts,
            applied_policy_version: policy_version,
            created_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intercepted_call_total_tokens() {
        let mut call = InterceptedCall::new(Uuid::nil(), "claude-sonnet-4-20250514");
        call.token_count_input = Some(100);
        call.token_count_output = Some(250);
        assert_eq!(call.total_tokens(), 350);
    }

    #[test]
    fn cost_ledger_anomaly_detection() {
        let entry = CostLedgerEntry {
            id: Uuid::nil(),
            app_id: Uuid::nil(),
            window_start: Utc::now(),
            window_end: Utc::now(),
            total_tokens_input: 1000,
            total_tokens_output: 2000,
            total_cost_cents: 450,
            request_count: 10,
            avg_tokens_per_req: Some(300),
            baseline_avg: Some(200.0),
            baseline_deviation: Some(3.5),
            created_at: Utc::now(),
        };
        assert!(entry.is_anomalous(2.0));
        assert!(!entry.is_anomalous(4.0));
    }

    #[test]
    fn decision_from_verdicts_picks_worst() {
        let call_id = Uuid::now_v7();
        let app_id = Uuid::now_v7();
        let verdicts = vec![
            Verdict::new(call_id, Axis::Cost, Path::Fast, Outcome::Pass, 0.1, "ok", "cost_cap"),
            Verdict::new(call_id, Axis::Responsibility, Path::Fast, Outcome::Edit, 0.95, "secret found", "secret_detection"),
            Verdict::new(call_id, Axis::Performance, Path::Shadow, Outcome::Escalate, 0.5, "low groundedness", "groundedness"),
        ];
        let decision = Decision::from_verdicts(call_id, app_id, &verdicts, Some(1));
        assert_eq!(decision.final_outcome, Outcome::Edit);
        assert_eq!(decision.contributing_verdicts.len(), 3);
    }

    #[test]
    fn escalation_case_from_verdict() {
        let call_id = Uuid::now_v7();
        let verdict = Verdict::new(call_id, Axis::Responsibility, Path::Shadow, Outcome::Escalate, 0.65, "bias detected", "bias_classifier");
        let case = EscalationCase::new(&verdict, Uuid::now_v7());
        assert!(case.is_open());
        assert_eq!(case.confidence, 0.65);
        assert_eq!(case.axis, Axis::Responsibility);
    }
}
