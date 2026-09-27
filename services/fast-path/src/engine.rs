use std::time::Instant;

use controlplane_common::types::{Axis, Outcome};

use crate::checks::{CostCapCheck, RetryDetector, SecretDetector, SessionRiskAccumulator, ToolUseDetector, UnsafeContentCheck};
use crate::policy_cache::PolicyCache;

/// The total fast-path budget. If checks exceed this, remaining checks are skipped.
const FAST_PATH_BUDGET_MS: u128 = 50;

/// Fractional milliseconds elapsed since `start` (microsecond resolution).
fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

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
        let engine = Self {
            policy_cache,
            secret_detector: SecretDetector::new(),
            cost_cap: CostCapCheck::new(),
            retry_detector: RetryDetector::new(),
            unsafe_check: UnsafeContentCheck::new(),
            session_risk: SessionRiskAccumulator::new(),
            tool_use_detector: ToolUseDetector::new(),
        };
        engine.warm_up();
        engine
    }

    /// Run the stateless checks once so their lazily-compiled regexes are built at
    /// startup. Otherwise the first request pays for compilation (~90 ms for the
    /// secret patterns in a debug build), overruns the proxy's 50 ms fast-path
    /// budget, fails open — and that response goes out unredacted.
    /// Stateful checks (retry, session risk) are skipped so no state is recorded.
    fn warm_up(&self) {
        const SAMPLE: &str = "warm-up AKIAIOSFODNN7EXAMPLE api_key=abcdefghijklmnopqrstuvwx             a@b.example 123-45-6789 4111 1111 1111 1111 +1 555 123 4567             {\"tool_calls\":[{\"function\":{\"name\":\"x\"}}]}";
        let rules = self.policy_cache.load();
        let _ = self.unsafe_check.check(SAMPLE, &rules);
        let found = self.secret_detector.check(SAMPLE);
        let _ = self.secret_detector.to_verdict_and_edits(&found);
        let _ = self.tool_use_detector.check(SAMPLE);
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
        self.evaluate_with_app_context(response_body, token_count_output, session_key, None)
    }

    /// Full evaluation with per-app cost cap override.
    pub fn evaluate_with_app_context(
        &self,
        response_body: &str,
        token_count_output: Option<i32>,
        session_key: Option<u64>,
        app_max_tokens: Option<i32>,
    ) -> FastPathResult {
        let start = Instant::now();
        let base_rules = self.policy_cache.load();
        let rules = if let Some(cap) = app_max_tokens {
            let mut r = (*base_rules).clone();
            r.max_tokens_per_request = Some(cap);
            std::sync::Arc::new(r)
        } else {
            base_rules
        };
        let mut verdicts: Vec<FastPathVerdict> = Vec::new();
        let mut edits: Vec<ResponseEdit> = Vec::new();
        let mut worst_outcome = Outcome::Pass;
        // Wall time of every check that ran, pass or not, keyed by check name.
        let mut timings: Vec<CheckTiming> = Vec::new();
        macro_rules! done {
            ($has_tool_use:expr) => {
                return FastPathResult {
                    outcome: worst_outcome,
                    edits,
                    verdicts,
                    has_tool_use: $has_tool_use,
                    check_timings: timings,
                }
            };
        }
        // Run one check, record its timing, and stamp the timing on its verdict.
        macro_rules! timed {
            ($name:literal, $body:expr) => {{
                let t = Instant::now();
                let out = $body;
                let ms = elapsed_ms(t);
                timings.push(CheckTiming { check_name: $name, duration_ms: ms });
                out.map(|mut v: FastPathVerdict| {
                    v.duration_ms = ms;
                    v
                })
            }};
        }

        // --- Check 1: Unsafe content (cheapest, most critical — check first) ---
        if let Some(verdict) = timed!("unsafe_content", self.unsafe_check.check(response_body, &rules)) {
            worst_outcome = Outcome::worst(worst_outcome, verdict.outcome);
            if verdict.outcome == Outcome::Block {
                verdicts.push(verdict);
                done!(false);
            }
            verdicts.push(verdict);
        }

        if start.elapsed().as_millis() > FAST_PATH_BUDGET_MS {
            done!(false);
        }

        // --- Check 2: Secret/PII detection ---
        let secret_edits;
        let secret_verdict = timed!("secret_detection", {
            let secret_result = self.secret_detector.check(response_body);
            let (verdict, found_edits) = self.secret_detector.to_verdict_and_edits(&secret_result);
            secret_edits = found_edits;
            verdict
        });
        if let Some(verdict) = secret_verdict {
            worst_outcome = Outcome::worst(worst_outcome, verdict.outcome);
            verdicts.push(verdict);
            edits.extend(secret_edits);
        }

        if start.elapsed().as_millis() > FAST_PATH_BUDGET_MS {
            done!(false);
        }

        // --- Check 3: Cost cap enforcement ---
        if let Some(verdict) = timed!("cost_cap", self.cost_cap.check(token_count_output, &rules)) {
            worst_outcome = Outcome::worst(worst_outcome, verdict.outcome);
            if verdict.outcome == Outcome::Block {
                verdicts.push(verdict);
                done!(false);
            }
            verdicts.push(verdict);
        }

        if start.elapsed().as_millis() > FAST_PATH_BUDGET_MS {
            done!(false);
        }

        // --- Check 4: Retry/loop detection ---
        if let Some(key) = session_key {
            if let Some(verdict) = timed!("retry_detection", self.retry_detector.check(key, &rules)) {
                worst_outcome = Outcome::worst(worst_outcome, verdict.outcome);
                verdicts.push(verdict);
            }
        }

        // --- Check 5: Tool/function call detection (agent risk) ---
        let has_tool_use;
        let tool_verdict = timed!("tool_use_detection", {
            let tool_result = self.tool_use_detector.check(response_body);
            has_tool_use = tool_result.has_tool_use;
            tool_result.verdict
        });
        if let Some(verdict) = tool_verdict {
            worst_outcome = Outcome::worst(worst_outcome, verdict.outcome);
            verdicts.push(verdict);
        }

        // --- Check 6: Session risk accumulator (multi-turn compounding risk) ---
        if let Some(key) = session_key {
            let session_verdict = timed!("session_risk", {
                // Record risk event if any non-pass verdict was issued
                if worst_outcome != Outcome::Pass {
                    self.session_risk.record_risk_event(key);
                }
                // Check if session has accumulated too many risk events
                self.session_risk.check(key)
            });
            if let Some(verdict) = session_verdict {
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

        FastPathResult { outcome: worst_outcome, edits, verdicts, has_tool_use, check_timings: timings }
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
    /// Duration of every check that ran (including passes), in run order.
    pub check_timings: Vec<CheckTiming>,
}

/// Wall time of one fast-path check. Names match the dashboard's check list.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckTiming {
    pub check_name: &'static str,
    /// Fractional milliseconds (microsecond resolution).
    pub duration_ms: f64,
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
    pub duration_ms: f64,
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

    #[test]
    fn times_every_check_including_passes() {
        let engine = engine_with_defaults();
        let result = engine.evaluate_with_context("Hello! The answer is 42.", Some(10), Some(7));
        assert_eq!(result.outcome, Outcome::Pass);
        assert!(result.verdicts.is_empty());
        let names: Vec<_> = result.check_timings.iter().map(|t| t.check_name).collect();
        assert_eq!(
            names,
            ["unsafe_content", "secret_detection", "cost_cap", "retry_detection", "tool_use_detection", "session_risk"]
        );
        assert!(result.check_timings.iter().all(|t| t.duration_ms >= 0.0 && t.duration_ms < 50.0));
    }

    #[test]
    fn verdict_carries_its_check_timing() {
        let engine = engine_with_cap(100);
        let result = engine.evaluate_with_context("Hello", Some(500), None);
        let verdict = result.verdicts.iter().find(|v| v.check_name == "cost_cap").unwrap();
        let timing = result.check_timings.iter().find(|t| t.check_name == "cost_cap").unwrap();
        assert_eq!(verdict.duration_ms, timing.duration_ms);
    }
}
