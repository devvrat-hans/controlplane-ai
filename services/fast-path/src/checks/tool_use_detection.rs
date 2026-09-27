use controlplane_common::types::{Axis, Outcome};

use crate::engine::FastPathVerdict;

/// Detects tool/function call patterns in model responses.
/// When tool use is detected, applies stricter confidence thresholds
/// because actions have higher downstream impact than pure text.
#[derive(Clone)]
pub struct ToolUseDetector;

/// Patterns that indicate the model is invoking tools or functions
const TOOL_PATTERNS: &[&str] = &[
    "function_call",
    "tool_calls",
    "tool_use",
    "\"type\": \"function\"",
    "\"type\":\"function\"",
    "\"name\":",
    "\"arguments\":",
];

/// Action directive patterns in text responses (agent-style outputs)
const ACTION_DIRECTIVE_PATTERNS: &[&str] = &[
    "I will now execute",
    "Executing command:",
    "Running tool:",
    "Calling API:",
    "DELETE FROM",
    "DROP TABLE",
    "rm -rf",
    "sudo ",
    "curl -X DELETE",
    "curl -X PUT",
];

impl Default for ToolUseDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolUseDetector {
    pub fn new() -> Self {
        Self
    }

    /// Check if the response contains tool use patterns.
    /// Returns a verdict only if dangerous action patterns are detected.
    pub fn check(&self, response_body: &str) -> ToolUseResult {
        let start = std::time::Instant::now();

        let has_structured_tool_use = TOOL_PATTERNS.iter()
            .any(|p| response_body.contains(p));

        let dangerous_directives: Vec<&str> = ACTION_DIRECTIVE_PATTERNS.iter()
            .filter(|p| response_body.to_lowercase().contains(&p.to_lowercase()))
            .copied()
            .collect();

        let has_dangerous_action = !dangerous_directives.is_empty();
        let has_tool_use = has_structured_tool_use || has_dangerous_action;

        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

        let verdict = if has_dangerous_action {
            Some(FastPathVerdict {
                axis: Axis::Responsibility,
                check_name: "tool_use_detection".to_string(),
                outcome: Outcome::Escalate,
                confidence: 0.8,
                reason: format!(
                    "Response contains dangerous action directives: {}",
                    dangerous_directives.join(", ")
                ),
                duration_ms,
            })
        } else {
            None
        };

        ToolUseResult {
            has_tool_use,
            has_structured_tool_use,
            has_dangerous_action,
            verdict,
        }
    }

    /// Apply action risk multiplier to a confidence score.
    /// Tool-using responses get 1.5x confidence weight because
    /// actions have higher downstream impact than text.
    pub fn apply_risk_multiplier(confidence: f32, has_tool_use: bool) -> f32 {
        if has_tool_use {
            (confidence * 1.5).min(1.0)
        } else {
            confidence
        }
    }
}

pub struct ToolUseResult {
    pub has_tool_use: bool,
    pub has_structured_tool_use: bool,
    pub has_dangerous_action: bool,
    pub verdict: Option<FastPathVerdict>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_structured_tool_calls() {
        let detector = ToolUseDetector::new();
        let response = r#"{"choices":[{"message":{"tool_calls":[{"type":"function","function":{"name":"get_weather"}}]}}]}"#;
        let result = detector.check(response);
        assert!(result.has_tool_use);
        assert!(result.has_structured_tool_use);
    }

    #[test]
    fn detects_function_call_format() {
        let detector = ToolUseDetector::new();
        let response = r#"{"choices":[{"message":{"function_call":{"name":"search","arguments":"{\"q\":\"hello\"}"}}}]}"#;
        let result = detector.check(response);
        assert!(result.has_tool_use);
        assert!(result.has_structured_tool_use);
    }

    #[test]
    fn detects_dangerous_action() {
        let detector = ToolUseDetector::new();
        let response = "I will now execute the deletion. Running tool: DELETE FROM users WHERE id = 5";
        let result = detector.check(response);
        assert!(result.has_tool_use);
        assert!(result.has_dangerous_action);
        assert!(result.verdict.is_some());
        assert_eq!(result.verdict.unwrap().outcome, Outcome::Escalate);
    }

    #[test]
    fn no_tool_use_in_normal_response() {
        let detector = ToolUseDetector::new();
        let response = "The weather today is sunny with a high of 75°F.";
        let result = detector.check(response);
        assert!(!result.has_tool_use);
        assert!(result.verdict.is_none());
    }

    #[test]
    fn risk_multiplier_increases_confidence() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-6;
        assert!(close(ToolUseDetector::apply_risk_multiplier(0.6, true), 0.9));
        assert!(close(ToolUseDetector::apply_risk_multiplier(0.6, false), 0.6));
        assert!(close(ToolUseDetector::apply_risk_multiplier(0.8, true), 1.0)); // capped at 1.0
    }
}
