use std::time::Instant;

use controlplane_common::types::{Axis, Outcome};

use crate::checks::{CostCapCheck, RetryDetector, SecretDetector, SessionRiskAccumulator, ToolUseDetector, UnsafeContentCheck};
use crate::policy_cache::PolicyCache;

/// The total fast-path budget. If checks exceed this, remaining checks are skipped.
const FAST_PATH_BUDGET_MS: u128 = 50;

#[derive(Clone)]
pub struct FastPathEngine {
    pub policy_cache: PolicyCache,
    secret_detector: SecretDetector,
    cost_cap: CostCapCheck,
    retry_detector: RetryDetector,
    unsafe_check: UnsafeContentCheck,
    session_risk: SessionRiskAccumulator,
    tool_use_detector: ToolUseDetector,
}

impl FastPathEngine {
    pub fn new(policy_cache: PolicyCache) -> Self {
        Self {
            policy_cache,
            secret_detector: SecretDetector::new(),
            cost_cap: CostCapCheck::new(),
            retry_detector: RetryDetector::new(),
            unsafe_check: UnsafeContentCheck::new(),
            session_risk: SessionRiskAccumulator::new(),
            tool_use_detector: ToolUseDetector::new(),
        }
    }

    /// Run all fast-path checks sequentially. Short-circuits on block.
    /// Enforces total budget: if cumulative time exceeds budget, skip remaining checks.
    pub fn evaluate(&self, response_body: &str) -> FastPathResult {
        self.evaluate_with_context(response_body, None, None)
    }

    /// Full evaluation with optional token count and session key for retry detection.
    pub fn evaluate_with_context(
        &self,
        response_body: &str,
        token_count_output: Option<i32>,
        session_key: Option<u64>,
    ) -> FastPathResult {
        let start = Instant::now();
        let rules = self.policy_cache.load();
        let mut verdicts: Vec<FastPathVerdict> = Vec::new();
        let mut edits: Vec<ResponseEdit> = Vec::new();
        let mut worst_outcome = Outcome::Pass;

        // --- Check 1: Unsafe content (cheapest, most critical — check first) ---
        if let Some(verdict) = self.unsafe_check.check(response_body, &rules) {
            worst_outcome = Outcome::worst(worst_outcome, verdict.outcome);
            if verdict.outcome == Outcome::Block {
                verdicts.push(verdict);
                return FastPathResult { outcome: worst_outcome, edits, verdicts, has_tool_use: false };
            }
            verdicts.push(verdict);
        }

        if start.elapsed().as_millis() > FAST_PATH_BUDGET_MS {
            return FastPathResult { outcome: worst_outcome, edits, verdicts, has_tool_use: false };
        }

        // --- Check 2: Secret/PII detection ---
        let secret_result = self.secret_detector.check(response_body);
        let (secret_verdict, secret_edits) = self.secret_detector.to_verdict_and_edits(&secret_result);
        if let Some(verdict) = secret_verdict {
            worst_outcome = Outcome::worst(worst_outcome, verdict.outcome);
            verdicts.push(verdict);
            edits.extend(secret_edits);
        }

        if start.elapsed().as_millis() > FAST_PATH_BUDGET_MS {
            return FastPathResult { outcome: worst_outcome, edits, verdicts, has_tool_use: false };
        }

        // --- Check 3: Cost cap enforcement ---
        if let Some(verdict) = self.cost_cap.check(token_count_output, &rules) {
            worst_outcome = Outcome::worst(worst_outcome, verdict.outcome);
            if verdict.outcome == Outcome::Block {
                verdicts.push(verdict);
                return FastPathResult { outcome: worst_outcome, edits, verdicts, has_tool_use: false };
            }
            verdicts.push(verdict);
        }

        if start.elapsed().as_millis() > FAST_PATH_BUDGET_MS {
            return FastPathResult { outcome: worst_outcome, edits, verdicts, has_tool_use: false };
        }

        // --- Check 4: Retry/loop detection ---
        if let Some(key) = session_key {
            if let Some(verdict) = self.retry_detector.check(key, &rules) {
                worst_outcome = Outcome::worst(worst_outcome, verdict.outcome);
                verdicts.push(verdict);
            }
        }

        // --- Check 5: Tool/function call detection (agent risk) ---
        let tool_result = self.tool_use_detector.check(response_body);
        let has_tool_use = tool_result.has_tool_use;
        if let Some(verdict) = tool_result.verdict {
            worst_outcome = Outcome::worst(worst_outcome, verdict.outcome);
            verdicts.push(verdict);
        }

        // --- Check 6: Session risk accumulator (multi-turn compounding risk) ---
        if let Some(key) = session_key {
            // Record risk event if any non-pass verdict was issued
            if worst_outcome != Outcome::Pass {
                self.session_risk.record_risk_event(key);
            }
            // Check if session has accumulated too many risk events
            if let Some(verdict) = self.session_risk.check(key) {
                worst_outcome = Outcome::worst(worst_outcome, verdict.outcome);
                verdicts.push(verdict);
            }
        }

        // Apply action risk multiplier: if tool use is detected, boost all
        // non-pass verdict confidences by 1.5x (actions have higher downstream impact)
        if has_tool_use {
            for v in &mut verdicts {
                v.confidence = ToolUseDetector::apply_risk_multiplier(v.confidence, true);
            }
        }

        FastPathResult { outcome: worst_outcome, edits, verdicts, has_tool_use }
    }

    /// Periodic cleanup of retry detection state.
    pub fn cleanup_retry_state(&self) {
        self.retry_detector.cleanup(300); // Remove entries older than 5 minutes
    }
}

pub struct FastPathResult {
    pub outcome: Outcome,
    pub edits: Vec<ResponseEdit>,
    pub verdicts: Vec<FastPathVerdict>,
    pub has_tool_use: bool,
}

#[derive(Clone)]
pub struct ResponseEdit {
    pub original: String,
    pub replacement: String,
    pub reason: String,
}

pub struct FastPathVerdict {
    pub axis: Axis,
    pub check_name: String,
    pub outcome: Outcome,
    pub confidence: f32,
    pub reason: String,
    pub duration_ms: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy_cache::FastPathRuleSet;

    fn engine_with_defaults() -> FastPathEngine {
        FastPathEngine::new(PolicyCache::default())
    }

    fn engine_with_cap(cap: i32) -> FastPathEngine {
        let rules = FastPathRuleSet {
            max_tokens_per_request: Some(cap),
            ..FastPathRuleSet::default()
        };
        FastPathEngine::new(PolicyCache::new(rules))
    }

    #[test]
    fn passes_clean_response() {
        let engine = engine_with_defaults();
        let result = engine.evaluate("Hello! The answer is 42. Have a great day!");
        assert_eq!(result.outcome, Outcome::Pass);
        assert!(result.edits.is_empty());
        assert!(result.verdicts.is_empty());
    }

    #[test]
    fn detects_and_redacts_secret() {
        let engine = engine_with_defaults();
        let result = engine.evaluate("Your key is AKIAIOSFODNN7EXAMPLE, use it wisely.");
        assert_eq!(result.outcome, Outcome::Edit);
        assert!(!result.edits.is_empty());
        assert!(result.edits[0].replacement.contains("REDACTED"));
        assert_eq!(result.verdicts[0].check_name, "secret_detection");
    }

    #[test]
    fn blocks_unsafe_content() {
        let engine = engine_with_defaults();
        let result = engine.evaluate("Sure! Here's how to hack into the server...");
        assert_eq!(result.outcome, Outcome::Block);
        assert_eq!(result.verdicts[0].check_name, "unsafe_content");
    }

    #[test]
    fn blocks_cost_cap_exceeded() {
        let engine = engine_with_cap(1000);
        let result = engine.evaluate_with_context("Normal response.", Some(2000), None);
        assert_eq!(result.outcome, Outcome::Block);
        assert!(result.verdicts.iter().any(|v| v.check_name == "cost_cap"));
    }

    #[test]
    fn detects_retry_storm() {
        let engine = engine_with_defaults();
        let key = Some(12345u64);

        // Within limit (default retry_max_count is 5)
        for _ in 0..5 {
            let _ = engine.evaluate_with_context("Hello", None, key);
        }

        // Over limit — 6th request should trigger
        let result = engine.evaluate_with_context("Hello", None, key);
        assert_eq!(result.outcome, Outcome::Escalate);
        assert!(result.verdicts.iter().any(|v| v.check_name == "retry_detection"));
    }

    #[test]
    fn short_circuits_on_block() {
        let engine = engine_with_defaults();
        // Unsafe content should block immediately — no secret detection runs after
        let body = "Kill yourself and here's the key AKIAIOSFODNN7EXAMPLE";
        let result = engine.evaluate(body);
        assert_eq!(result.outcome, Outcome::Block);
        // Only unsafe_content verdict, not secret_detection (short-circuited)
        assert_eq!(result.verdicts.len(), 1);
        assert_eq!(result.verdicts[0].check_name, "unsafe_content");
    }

    #[test]
    fn worst_outcome_wins() {
        let engine = engine_with_cap(1000);
        // Secret (Edit) + Cost cap exceeded (Block) = Block
        let body = "Key: AKIAIOSFODNN7EXAMPLE";
        let result = engine.evaluate_with_context(body, Some(2000), None);
        assert_eq!(result.outcome, Outcome::Block);
    }

    #[test]
    fn completes_within_budget() {
        let engine = engine_with_defaults();
        let body = "A".repeat(10000); // 10KB of text
        let start = Instant::now();
        let _ = engine.evaluate(&body);
        let elapsed = start.elapsed().as_millis();
        assert!(elapsed < 50, "Fast path took {}ms, budget is 25ms", elapsed);
    }
}
