use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use controlplane_common::types::{Axis, Outcome};

use crate::engine::FastPathVerdict;
use crate::policy_cache::FastPathRuleSet;

/// Sliding-window retry/loop detection per session key.
/// Uses an in-memory LRU-style map (no DB hit on hot path).
#[derive(Clone)]
pub struct RetryDetector {
    state: Arc<Mutex<HashMap<u64, WindowEntry>>>,
}

struct WindowEntry {
    count: u32,
    window_start: Instant,
}

impl RetryDetector {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if this request is part of a retry storm.
    /// `session_key` is a hash of the prompt/session identifier.
    pub fn check(&self, session_key: u64, rules: &FastPathRuleSet) -> Option<FastPathVerdict> {
        let start = std::time::Instant::now();
        let now = Instant::now();
        let window_duration = std::time::Duration::from_secs(rules.retry_window_seconds);
        let max_count = rules.retry_max_count;

        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());

        let entry = state.entry(session_key).or_insert_with(|| WindowEntry {
            count: 0,
            window_start: now,
        });

        // Reset window if expired
        if now.duration_since(entry.window_start) > window_duration {
            entry.count = 0;
            entry.window_start = now;
        }

        entry.count += 1;
        let current_count = entry.count;

        let duration_ms = start.elapsed().as_millis() as u32;

        if current_count > max_count {
            Some(FastPathVerdict {
                axis: Axis::Cost,
                check_name: "retry_detection".to_string(),
                outcome: Outcome::Escalate,
                confidence: 0.85,
                reason: format!(
                    "Retry storm detected: {} requests in {}s window (limit: {})",
                    current_count, rules.retry_window_seconds, max_count
                ),
                duration_ms,
            })
        } else {
            None
        }
    }

    /// Remove stale entries to prevent unbounded memory growth.
    pub fn cleanup(&self, max_age_secs: u64) {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.retain(|_, entry| {
            now.duration_since(entry.window_start).as_secs() < max_age_secs
        });
    }

    /// Compute a session key from a prompt string (fast hash).
    pub fn hash_prompt(prompt: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        prompt.hash(&mut hasher);
        hasher.finish()
    }
}

impl Default for RetryDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_rules() -> FastPathRuleSet {
        FastPathRuleSet {
            retry_window_seconds: 60,
            retry_max_count: 3,
            ..FastPathRuleSet::default()
        }
    }

    #[test]
    fn allows_requests_under_threshold() {
        let detector = RetryDetector::new();
        let rules = default_rules();
        let key = RetryDetector::hash_prompt("hello world");

        assert!(detector.check(key, &rules).is_none()); // 1st
        assert!(detector.check(key, &rules).is_none()); // 2nd
        assert!(detector.check(key, &rules).is_none()); // 3rd (at limit)
    }

    #[test]
    fn detects_retry_storm() {
        let detector = RetryDetector::new();
        let rules = default_rules();
        let key = RetryDetector::hash_prompt("hello world");

        for _ in 0..3 {
            let _ = detector.check(key, &rules);
        }

        // 4th request should trigger
        let verdict = detector.check(key, &rules);
        assert!(verdict.is_some());
        let v = verdict.unwrap();
        assert_eq!(v.outcome, Outcome::Escalate);
        assert!(v.reason.contains("Retry storm"));
    }

    #[test]
    fn different_keys_tracked_separately() {
        let detector = RetryDetector::new();
        let rules = default_rules();
        let key1 = RetryDetector::hash_prompt("query A");
        let key2 = RetryDetector::hash_prompt("query B");

        for _ in 0..3 {
            let _ = detector.check(key1, &rules);
        }

        // key1 is at limit, key2 is fresh
        assert!(detector.check(key1, &rules).is_some()); // over
        assert!(detector.check(key2, &rules).is_none()); // fresh
    }

    #[test]
    fn hash_prompt_is_deterministic() {
        let h1 = RetryDetector::hash_prompt("same input");
        let h2 = RetryDetector::hash_prompt("same input");
        assert_eq!(h1, h2);

        let h3 = RetryDetector::hash_prompt("different input");
        assert_ne!(h1, h3);
    }
}
