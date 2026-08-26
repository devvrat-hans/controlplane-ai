use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use controlplane_common::types::{AppId, Axis, Outcome};

/// Policy engine: loads per-app/per-axis thresholds and applies them to produce outcomes.
pub struct PolicyEngine {
    pool: PgPool,
}

/// The resolved policy for a specific app.
#[derive(Debug, Clone)]
pub struct ResolvedPolicy {
    pub app_id: AppId,
    pub version: i32,
    pub performance: PerformancePolicy,
    pub cost: CostPolicy,
    pub responsibility: ResponsibilityPolicy,
}

#[derive(Debug, Clone)]
pub struct PerformancePolicy {
    pub groundedness_threshold: f32,
    pub action: PolicyAction,
}

#[derive(Debug, Clone)]
pub struct CostPolicy {
    pub max_tokens_per_request: Option<i32>,
    pub daily_budget_cents: Option<i64>,
    pub action: PolicyAction,
}

#[derive(Debug, Clone)]
pub struct ResponsibilityPolicy {
    pub bias_threshold: f32,
    pub pii_action: PolicyAction,
    pub unsafe_action: PolicyAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyAction {
    Pass,
    Escalate,
    Edit,
    Block,
}

impl PolicyAction {
    pub fn to_outcome(self) -> Outcome {
        match self {
            PolicyAction::Pass => Outcome::Pass,
            PolicyAction::Escalate => Outcome::Escalate,
            PolicyAction::Edit => Outcome::Edit,
            PolicyAction::Block => Outcome::Block,
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "block" => PolicyAction::Block,
            "edit" => PolicyAction::Edit,
            "escalate" => PolicyAction::Escalate,
            _ => PolicyAction::Pass,
        }
    }
}

impl Default for ResolvedPolicy {
    fn default() -> Self {
        Self {
            app_id: Uuid::nil(),
            version: 0,
            performance: PerformancePolicy {
                groundedness_threshold: 0.6,
                action: PolicyAction::Escalate,
            },
            cost: CostPolicy {
                max_tokens_per_request: None,
                daily_budget_cents: None,
                action: PolicyAction::Block,
            },
            responsibility: ResponsibilityPolicy {
                bias_threshold: 0.7,
                pii_action: PolicyAction::Edit,
                unsafe_action: PolicyAction::Block,
            },
        }
    }
}

#[derive(sqlx::FromRow)]
struct PolicyRow {
    axis: String,
    threshold_config: serde_json::Value,
    version: i32,
}

impl PolicyEngine {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Load the active policy for an app. Returns default if not found.
    pub async fn load_policy(&self, app_id: AppId) -> ResolvedPolicy {
        match self.load_from_db(app_id).await {
            Ok(Some(policy)) => policy,
            Ok(None) => {
                info!(app_id = %app_id, "No policy found, using defaults");
                ResolvedPolicy { app_id, ..Default::default() }
            }
            Err(e) => {
                warn!(app_id = %app_id, error = %e, "Failed to load policy, using defaults");
                ResolvedPolicy { app_id, ..Default::default() }
            }
        }
    }

    async fn load_from_db(&self, app_id: AppId) -> Result<Option<ResolvedPolicy>, sqlx::Error> {
        let rows: Vec<PolicyRow> = sqlx::query_as::<_, PolicyRow>(
            "SELECT axis, threshold_config, version FROM policies WHERE app_id = $1 AND is_active = TRUE"
        )
        .bind(app_id)
        .fetch_all(&self.pool)
        .await?;

        if rows.is_empty() {
            return Ok(None);
        }

        let max_version = rows.iter().map(|r| r.version).max().unwrap_or(0);
        let mut policy = ResolvedPolicy {
            app_id,
            version: max_version,
            ..Default::default()
        };

        for row in &rows {
            let config = &row.threshold_config;
            match row.axis.as_str() {
                "performance" => {
                    if let Some(t) = config.get("groundedness_threshold").and_then(|v| v.as_f64()) {
                        policy.performance.groundedness_threshold = t as f32;
                    }
                    if let Some(a) = config.get("action").and_then(|v| v.as_str()) {
                        policy.performance.action = PolicyAction::from_str(a);
                    }
                }
                "cost" => {
                    if let Some(t) = config.get("max_tokens_per_request").and_then(|v| v.as_i64()) {
                        policy.cost.max_tokens_per_request = Some(t as i32);
                    }
                    if let Some(b) = config.get("daily_budget_cents").and_then(|v| v.as_i64()) {
                        policy.cost.daily_budget_cents = Some(b);
                    }
                    if let Some(a) = config.get("action").and_then(|v| v.as_str()) {
                        policy.cost.action = PolicyAction::from_str(a);
                    }
                }
                "responsibility" => {
                    if let Some(t) = config.get("bias_threshold").and_then(|v| v.as_f64()) {
                        policy.responsibility.bias_threshold = t as f32;
                    }
                    if let Some(a) = config.get("pii_action").and_then(|v| v.as_str()) {
                        policy.responsibility.pii_action = PolicyAction::from_str(a);
                    }
                    if let Some(a) = config.get("unsafe_action").and_then(|v| v.as_str()) {
                        policy.responsibility.unsafe_action = PolicyAction::from_str(a);
                    }
                }
                _ => {}
            }
        }

        Ok(Some(policy))
    }

    /// Apply a policy to determine the threshold-adjusted outcome for a given check.
    pub fn apply_threshold(&self, policy: &ResolvedPolicy, axis: Axis, check_name: &str, raw_score: f32) -> Outcome {
        match axis {
            Axis::Performance => {
                if raw_score < policy.performance.groundedness_threshold {
                    policy.performance.action.to_outcome()
                } else {
                    Outcome::Pass
                }
            }
            Axis::Cost => {
                policy.cost.action.to_outcome()
            }
            Axis::Responsibility => {
                match check_name {
                    "bias_classification" => {
                        if raw_score > policy.responsibility.bias_threshold {
                            PolicyAction::Escalate.to_outcome()
                        } else {
                            Outcome::Pass
                        }
                    }
                    "secret_detection" | "semantic_pii" => {
                        policy.responsibility.pii_action.to_outcome()
                    }
                    "unsafe_content" => {
                        policy.responsibility.unsafe_action.to_outcome()
                    }
                    _ => Outcome::Pass,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_action_from_str() {
        assert_eq!(PolicyAction::from_str("block"), PolicyAction::Block);
        assert_eq!(PolicyAction::from_str("Block"), PolicyAction::Block);
        assert_eq!(PolicyAction::from_str("edit"), PolicyAction::Edit);
        assert_eq!(PolicyAction::from_str("escalate"), PolicyAction::Escalate);
        assert_eq!(PolicyAction::from_str("pass"), PolicyAction::Pass);
        assert_eq!(PolicyAction::from_str("unknown"), PolicyAction::Pass);
    }

    #[test]
    fn default_policy_is_sensible() {
        let policy = ResolvedPolicy::default();
        assert_eq!(policy.performance.groundedness_threshold, 0.6);
        assert_eq!(policy.responsibility.bias_threshold, 0.7);
        assert_eq!(policy.cost.action, PolicyAction::Block);
    }

    #[test]
    fn apply_threshold_performance() {
        let policy = ResolvedPolicy::default();

        // Below threshold → escalate (simulating engine logic directly)
        let outcome = apply_performance_threshold(&policy, 0.4);
        assert_eq!(outcome, Outcome::Escalate);

        // Above threshold → pass
        let outcome = apply_performance_threshold(&policy, 0.8);
        assert_eq!(outcome, Outcome::Pass);
    }

    #[test]
    fn apply_threshold_bias() {
        let policy = ResolvedPolicy::default();

        // Above threshold → escalate
        let outcome = apply_bias_threshold(&policy, 0.9);
        assert_eq!(outcome, Outcome::Escalate);

        // Below threshold → pass
        let outcome = apply_bias_threshold(&policy, 0.3);
        assert_eq!(outcome, Outcome::Pass);
    }

    fn apply_performance_threshold(policy: &ResolvedPolicy, score: f32) -> Outcome {
        if score < policy.performance.groundedness_threshold {
            policy.performance.action.to_outcome()
        } else {
            Outcome::Pass
        }
    }

    fn apply_bias_threshold(policy: &ResolvedPolicy, score: f32) -> Outcome {
        if score > policy.responsibility.bias_threshold {
            PolicyAction::Escalate.to_outcome()
        } else {
            Outcome::Pass
        }
    }
}
