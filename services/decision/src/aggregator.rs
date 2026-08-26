use uuid::Uuid;

use controlplane_common::models::{Decision, Verdict};
use controlplane_common::types::{AppId, Outcome};

/// Aggregates verdicts from fast-path and shadow-path,
/// applies confidence weighting, produces final decision.
pub struct VerdictAggregator;

impl VerdictAggregator {
    pub fn new() -> Self {
        Self
    }

    /// Aggregate a set of verdicts into a final Decision.
    ///
    /// Logic:
    /// - Final outcome = worst verdict (block > edit > escalate > pass)
    /// - If conflicting verdicts at the same severity, the one with higher confidence wins
    /// - Contributing verdicts are all non-pass verdicts
    pub fn aggregate(
        &self,
        call_id: Uuid,
        app_id: AppId,
        verdicts: &[Verdict],
        policy_version: Option<i32>,
    ) -> Decision {
        Decision::from_verdicts(call_id, app_id, verdicts, policy_version)
    }

    /// Aggregate with confidence-weighted tie-breaking and intersection escalation.
    /// Returns (final_outcome, primary_reason, contributing_verdict_ids).
    ///
    /// Intersection escalation: when 2+ different axes produce non-pass verdicts,
    /// escalate even if individual confidences are below threshold. This handles
    /// overlapping risks (e.g., hallucination + privacy = compound risk).
    pub fn aggregate_with_reasoning(
        &self,
        verdicts: &[Verdict],
    ) -> AggregationResult {
        if verdicts.is_empty() {
            return AggregationResult {
                final_outcome: Outcome::Pass,
                primary_reason: "No verdicts received".to_string(),
                contributing_ids: Vec::new(),
                confidence: 1.0,
                compound_risk: false,
                triggered_axes: Vec::new(),
            };
        }

        let worst = verdicts.iter()
            .map(|v| v.outcome)
            .fold(Outcome::Pass, Outcome::worst);

        // Collect unique axes that produced non-pass verdicts
        let mut triggered_axes: Vec<String> = verdicts.iter()
            .filter(|v| v.outcome != Outcome::Pass)
            .map(|v| v.axis.as_str().to_string())
            .collect();
        triggered_axes.sort();
        triggered_axes.dedup();

        let compound_risk = triggered_axes.len() >= 2;

        // Intersection escalation: if 2+ axes fire, escalate even if individual
        // outcomes were just "edit" or low-confidence "escalate"
        let final_outcome = if compound_risk && worst == Outcome::Pass {
            Outcome::Escalate
        } else if compound_risk && worst == Outcome::Edit {
            // Compound risk upgrades "edit" to "escalate"
            Outcome::Escalate
        } else {
            worst
        };

        // Find the verdict(s) at the worst level with highest confidence
        let contributing: Vec<&Verdict> = verdicts.iter()
            .filter(|v| v.outcome != Outcome::Pass)
            .collect();
        let worst_level: Vec<&Verdict> = verdicts.iter()
            .filter(|v| v.outcome == final_outcome)
            .collect();
        // Fall back to all contributing verdicts when the final outcome came from
        // intersection escalation rather than any single verdict's level.
        let pool: Vec<&Verdict> = if worst_level.is_empty() { contributing.clone() } else { worst_level };

        let primary = if pool.is_empty() {
            verdicts.first().unwrap()
        } else {
            pool.iter()
                .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap()
        };

        let primary_reason = if compound_risk {
            format!(
                "Compound risk: {} axes triggered ({}). {}",
                triggered_axes.len(),
                triggered_axes.join(", "),
                primary.reason
            )
        } else {
            primary.reason.clone()
        };

        let all_non_pass: Vec<Uuid> = verdicts.iter()
            .filter(|v| v.outcome != Outcome::Pass)
            .map(|v| v.id)
            .collect();

        AggregationResult {
            final_outcome,
            primary_reason,
            contributing_ids: all_non_pass,
            confidence: primary.confidence,
            compound_risk,
            triggered_axes,
        }
    }
}

impl Default for VerdictAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct AggregationResult {
    pub final_outcome: Outcome,
    pub primary_reason: String,
    pub contributing_ids: Vec<Uuid>,
    pub confidence: f32,
    pub compound_risk: bool,
    pub triggered_axes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use controlplane_common::types::{Axis, Path as VerdictPath};

    fn make_verdict(outcome: Outcome, confidence: f32, reason: &str) -> Verdict {
        Verdict::new(
            Uuid::nil(),
            Axis::Responsibility,
            VerdictPath::Fast,
            outcome,
            confidence,
            reason,
            "test_check",
        )
    }

    #[test]
    fn worst_outcome_wins() {
        let aggregator = VerdictAggregator::new();
        let verdicts = vec![
            make_verdict(Outcome::Pass, 1.0, "All good"),
            make_verdict(Outcome::Escalate, 0.8, "Bias detected"),
            make_verdict(Outcome::Edit, 0.9, "PII found"),
        ];

        let result = aggregator.aggregate_with_reasoning(&verdicts);
        // Edit > Escalate > Pass
        assert_eq!(result.final_outcome, Outcome::Edit);
    }

    #[test]
    fn block_overrides_everything() {
        let aggregator = VerdictAggregator::new();
        let verdicts = vec![
            make_verdict(Outcome::Edit, 0.95, "PII found"),
            make_verdict(Outcome::Block, 0.7, "Unsafe content"),
            make_verdict(Outcome::Escalate, 0.9, "Bias"),
        ];

        let result = aggregator.aggregate_with_reasoning(&verdicts);
        assert_eq!(result.final_outcome, Outcome::Block);
        assert!(result.primary_reason.contains("Unsafe"));
    }

    #[test]
    fn highest_confidence_is_primary() {
        let aggregator = VerdictAggregator::new();
        let verdicts = vec![
            make_verdict(Outcome::Block, 0.7, "Low confidence block"),
            make_verdict(Outcome::Block, 0.95, "High confidence block"),
        ];

        let result = aggregator.aggregate_with_reasoning(&verdicts);
        assert_eq!(result.confidence, 0.95);
        assert!(result.primary_reason.contains("High confidence"));
    }

    #[test]
    fn empty_verdicts_pass() {
        let aggregator = VerdictAggregator::new();
        let result = aggregator.aggregate_with_reasoning(&[]);
        assert_eq!(result.final_outcome, Outcome::Pass);
    }

    #[test]
    fn all_pass_gives_pass() {
        let aggregator = VerdictAggregator::new();
        let verdicts = vec![
            make_verdict(Outcome::Pass, 1.0, "OK"),
            make_verdict(Outcome::Pass, 1.0, "Also OK"),
        ];

        let result = aggregator.aggregate_with_reasoning(&verdicts);
        assert_eq!(result.final_outcome, Outcome::Pass);
        assert!(result.contributing_ids.is_empty());
    }

    #[test]
    fn decision_from_verdicts() {
        let aggregator = VerdictAggregator::new();
        let app_id = Uuid::now_v7();
        let call_id = Uuid::now_v7();
        let verdicts = vec![
            make_verdict(Outcome::Escalate, 0.8, "Issue"),
        ];

        let decision = aggregator.aggregate(call_id, app_id, &verdicts, Some(3));
        assert_eq!(decision.call_id, call_id);
        assert_eq!(decision.app_id, app_id);
        assert_eq!(decision.final_outcome, Outcome::Escalate);
        assert_eq!(decision.applied_policy_version, Some(3));
    }
}
