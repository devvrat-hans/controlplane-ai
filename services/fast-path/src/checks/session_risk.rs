use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use controlplane_common::types::{Axis, Outcome};

use crate::engine::FastPathVerdict;

/// Tracks accumulated risk events per session.
/// If a session accumulates too many non-pass verdicts,
/// the entire session is escalated for human review.
#[derive(Clone)]
pub struct SessionRiskAccumulator {
    state: Arc<Mutex<HashMap<u64, SessionRiskEntry>>>,
}

struct SessionRiskEntry {
    risk_events: u32,
    first_seen: Instant,
}

const SESSION_RISK_THRESHOLD: u32 = 3;
const SESSION_WINDOW_SECS: u64 = 3600; // 1 hour

impl Default for SessionRiskAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionRiskAccumulator {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Record that a non-pass verdict was issued for this session.
    pub fn record_risk_event(&self, session_key: u64) {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());

        let entry = state.entry(session_key).or_insert_with(|| SessionRiskEntry {
            risk_events: 0,
            first_seen: now,
        });

        if now.duration_since(entry.first_seen) > std::time::Duration::from_secs(SESSION_WINDOW_SECS) {
            entry.risk_events = 0;
            entry.first_seen = now;
        }

        entry.risk_events += 1;
    }

    /// Check if this session has accumulated enough risk to warrant escalation.
    pub fn check(&self, session_key: u64) -> Option<FastPathVerdict> {
        let start = std::time::Instant::now();
        let now = Instant::now();
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(entry) = state.get(&session_key) {
            if now.duration_since(entry.first_seen) <= std::time::Duration::from_secs(SESSION_WINDOW_SECS)
                && entry.risk_events >= SESSION_RISK_THRESHOLD
            {
                let duration_ms = start.elapsed().as_millis() as u32;
                return Some(FastPathVerdict {
                    axis: Axis::Responsibility,
                    check_name: "session_risk_accumulator".to_string(),
                    outcome: Outcome::Escalate,
                    confidence: 0.75,
                    reason: format!(
                        "Session accumulated {} risk events in the last hour (threshold: {})",
                        entry.risk_events, SESSION_RISK_THRESHOLD
                    ),
                    duration_ms,
                });
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_escalation_below_threshold() {
        let acc = SessionRiskAccumulator::new();
        acc.record_risk_event(42);
        acc.record_risk_event(42);
        assert!(acc.check(42).is_none());
    }

    #[test]
    fn escalates_at_threshold() {
        let acc = SessionRiskAccumulator::new();
        acc.record_risk_event(99);
        acc.record_risk_event(99);
        acc.record_risk_event(99);
        let verdict = acc.check(99);
        assert!(verdict.is_some());
        assert_eq!(verdict.unwrap().outcome, Outcome::Escalate);
    }

    #[test]
    fn different_sessions_independent() {
        let acc = SessionRiskAccumulator::new();
        acc.record_risk_event(1);
        acc.record_risk_event(1);
        acc.record_risk_event(1);
        acc.record_risk_event(2);
        assert!(acc.check(1).is_some());
        assert!(acc.check(2).is_none());
    }
}
