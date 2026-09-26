use controlplane_common::types::{Axis, Outcome};

use crate::engine::FastPathVerdict;
use crate::policy_cache::FastPathRuleSet;

#[derive(Clone)]
pub struct CostCapCheck;

impl CostCapCheck {
    pub fn new() -> Self {
        Self
    }

    pub fn check(&self, token_count_output: Option<i32>, rules: &FastPathRuleSet) -> Option<FastPathVerdict> {
        let start = std::time::Instant::now();

        let max_tokens = rules.max_tokens_per_request?;

        let tokens = token_count_output?;

        let duration_ms = start.elapsed().as_millis() as u32;

        if tokens > max_tokens {
            Some(FastPathVerdict {
                axis: Axis::Cost,
                check_name: "cost_cap".to_string(),
                outcome: Outcome::Block,
                confidence: 0.99,
                reason: format!(
                    "Response token count ({}) exceeds per-request cap ({})",
                    tokens, max_tokens
                ),
                duration_ms,
            })
        } else if tokens as f64 > max_tokens as f64 * 0.9 {
            // Near-cap: escalate for human review (90-100% of cap)
            Some(FastPathVerdict {
                axis: Axis::Cost,
                check_name: "cost_cap".to_string(),
                outcome: Outcome::Escalate,
                confidence: 0.75,
                reason: format!(
                    "Token usage ({}) near cap ({}) — {}% utilization, flagged for review",
                    tokens, max_tokens, (tokens as f64 / max_tokens as f64 * 100.0) as i32
                ),
                duration_ms,
            })
        } else if tokens as f64 > max_tokens as f64 * 0.75 {
            // Approaching cap: edit/warn (75-90% of cap)
            Some(FastPathVerdict {
                axis: Axis::Cost,
                check_name: "cost_cap".to_string(),
                outcome: Outcome::Edit,
                confidence: 0.60,
                reason: format!(
                    "Token usage ({}) approaching cap ({}) — {}% utilization, response trimmed",
                    tokens, max_tokens, (tokens as f64 / max_tokens as f64 * 100.0) as i32
                ),
                duration_ms,
            })
        } else {
            None
        }
    }
}

impl Default for CostCapCheck {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules_with_cap(cap: i32) -> FastPathRuleSet {
        FastPathRuleSet {
            max_tokens_per_request: Some(cap),
            ..FastPathRuleSet::default()
        }
    }

    #[test]
    fn blocks_when_over_cap() {
        let check = CostCapCheck::new();
        let verdict = check.check(Some(5000), &rules_with_cap(4096));
        assert!(verdict.is_some());
        let v = verdict.unwrap();
        assert_eq!(v.outcome, Outcome::Block);
        assert!(v.reason.contains("5000"));
        assert!(v.reason.contains("4096"));
    }

    #[test]
    fn passes_when_well_under_cap() {
        let check = CostCapCheck::new();
        let verdict = check.check(Some(2000), &rules_with_cap(4096));
        assert!(verdict.is_none());
    }

    #[test]
    fn escalates_near_cap() {
        let check = CostCapCheck::new();
        // 3800/4096 = 92.7% → escalate
        let verdict = check.check(Some(3800), &rules_with_cap(4096));
        assert!(verdict.is_some());
        assert_eq!(verdict.unwrap().outcome, Outcome::Escalate);
    }

    #[test]
    fn edits_approaching_cap() {
        let check = CostCapCheck::new();
        // 3200/4096 = 78.1% → edit
        let verdict = check.check(Some(3200), &rules_with_cap(4096));
        assert!(verdict.is_some());
        assert_eq!(verdict.unwrap().outcome, Outcome::Edit);
    }

    #[test]
    fn passes_when_no_cap_configured() {
        let check = CostCapCheck::new();
        let rules = FastPathRuleSet::default();
        let verdict = check.check(Some(99999), &rules);
        assert!(verdict.is_none());
    }

    #[test]
    fn passes_when_no_token_count() {
        let check = CostCapCheck::new();
        let verdict = check.check(None, &rules_with_cap(4096));
        assert!(verdict.is_none());
    }
}
