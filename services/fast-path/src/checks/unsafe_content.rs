use controlplane_common::types::{Axis, Outcome};

use crate::engine::FastPathVerdict;
use crate::policy_cache::FastPathRuleSet;

/// Keyword/pattern matching for severe, unambiguous unsafe content categories.
/// Only matches high-confidence, clear-cut violations — borderline cases go to shadow-path.
#[derive(Clone)]
pub struct UnsafeContentCheck;

/// Built-in severe unsafe patterns (supplemented by configurable blocklist in policy cache).
const DEFAULT_UNSAFE_PATTERNS: &[(&str, &str)] = &[
    ("how to make a bomb", "instructions for weapons/explosives"),
    ("how to synthesize", "instructions for controlled substances"),
    ("kill yourself", "self-harm encouragement"),
    ("here's how to hack into", "unauthorized access instructions"),
    ("child exploitation", "CSAM-related content"),
    ("here is the malware code", "malware distribution"),
];

impl UnsafeContentCheck {
    pub fn new() -> Self {
        Self
    }

    pub fn check(&self, response_body: &str, rules: &FastPathRuleSet) -> Option<FastPathVerdict> {
        let start = std::time::Instant::now();
        let body_lower = response_body.to_lowercase();

        // Check built-in severe patterns
        for (pattern, category) in DEFAULT_UNSAFE_PATTERNS {
            if body_lower.contains(pattern) {
                let duration_ms = start.elapsed().as_millis() as u32;
                return Some(FastPathVerdict {
                    axis: Axis::Responsibility,
                    check_name: "unsafe_content".to_string(),
                    outcome: Outcome::Block,
                    confidence: 0.95,
                    reason: format!("Blocked: severe unsafe content detected ({})", category),
                    duration_ms,
                });
            }
        }

        // Check configurable blocklist from policy cache
        for keyword in &rules.unsafe_keywords {
            if body_lower.contains(&keyword.to_lowercase()) {
                let duration_ms = start.elapsed().as_millis() as u32;
                return Some(FastPathVerdict {
                    axis: Axis::Responsibility,
                    check_name: "unsafe_content".to_string(),
                    outcome: Outcome::Block,
                    confidence: 0.90,
                    reason: format!("Blocked: content matches unsafe keyword policy (\"{}\")", keyword),
                    duration_ms,
                });
            }
        }

        None
    }
}

impl Default for UnsafeContentCheck {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_severe_content() {
        let check = UnsafeContentCheck::new();
        let rules = FastPathRuleSet::default();
        let body = "Sure, here's how to hack into the company server...";
        let verdict = check.check(body, &rules);
        assert!(verdict.is_some());
        let v = verdict.unwrap();
        assert_eq!(v.outcome, Outcome::Block);
        assert!(v.reason.contains("unauthorized access"));
    }

    #[test]
    fn blocks_custom_keywords() {
        let check = UnsafeContentCheck::new();
        let rules = FastPathRuleSet {
            unsafe_keywords: vec!["forbidden_topic".to_string(), "banned_phrase".to_string()],
            ..FastPathRuleSet::default()
        };
        let body = "Let me tell you about the forbidden_topic in detail...";
        let verdict = check.check(body, &rules);
        assert!(verdict.is_some());
        assert_eq!(verdict.unwrap().outcome, Outcome::Block);
    }

    #[test]
    fn passes_safe_content() {
        let check = UnsafeContentCheck::new();
        let rules = FastPathRuleSet::default();
        let body = "The weather today is sunny. Please remember to stay hydrated!";
        let verdict = check.check(body, &rules);
        assert!(verdict.is_none());
    }

    #[test]
    fn case_insensitive_matching() {
        let check = UnsafeContentCheck::new();
        let rules = FastPathRuleSet::default();
        let body = "KILL YOURSELF is something you should never say.";
        let verdict = check.check(body, &rules);
        assert!(verdict.is_some());
    }
}
