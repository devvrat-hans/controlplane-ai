use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

const MAX_VERDICTS: usize = 500;

/// In-memory ring buffer for verdicts — used when no database is connected.
#[derive(Clone)]
pub struct InMemoryVerdictStore {
    inner: Arc<Mutex<VecDeque<VerdictRecord>>>,
}

#[derive(Clone, Serialize)]
pub struct VerdictRecord {
    pub id: String,
    pub call_id: String,
    pub app_id: Option<String>,
    pub axis: String,
    pub path: String,
    pub outcome: String,
    pub confidence: f32,
    pub reason: String,
    pub check_name: String,
    pub latency_ms: Option<i32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct OverviewStats {
    pub total_calls_24h: i64,
    pub total_verdicts_24h: i64,
    pub blocks_24h: i64,
    pub escalations_24h: i64,
    pub passes_24h: i64,
    pub open_escalations: i64,
    pub avg_fast_path_latency_ms: f64,
    pub top_blocked_axes: Vec<AxisCount>,
}

#[derive(Serialize)]
pub struct AxisCount {
    pub axis: String,
    pub count: i64,
}

impl InMemoryVerdictStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_VERDICTS))),
        }
    }

    /// Push a verdict into the ring buffer.
    pub fn push(&self, record: VerdictRecord) {
        if let Ok(mut store) = self.inner.lock() {
            if store.len() >= MAX_VERDICTS {
                store.pop_front();
            }
            store.push_back(record);
        }
    }

    /// Get recent verdicts (newest first), optionally filtered.
    pub fn recent(
        &self,
        limit: usize,
        app_id: Option<&str>,
        outcome: Option<&str>,
    ) -> Vec<VerdictRecord> {
        let store = match self.inner.lock() {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        store
            .iter()
            .rev()
            .filter(|r| {
                if let Some(aid) = app_id {
                    if r.app_id.as_deref() != Some(aid) {
                        return false;
                    }
                }
                if let Some(out) = outcome {
                    if r.outcome != out {
                        return false;
                    }
                }
                true
            })
            .take(limit)
            .cloned()
            .collect()
    }

    /// Compute overview stats from the in-memory ring buffer.
    pub fn overview(&self) -> OverviewStats {
        let store = match self.inner.lock() {
            Ok(s) => s,
            Err(_) => {
                return OverviewStats {
                    total_calls_24h: 0,
                    total_verdicts_24h: 0,
                    blocks_24h: 0,
                    escalations_24h: 0,
                    passes_24h: 0,
                    open_escalations: 0,
                    avg_fast_path_latency_ms: 0.0,
                    top_blocked_axes: vec![],
                };
            }
        };

        let cutoff = Utc::now() - Duration::hours(24);

        let recent: Vec<&VerdictRecord> = store
            .iter()
            .filter(|r| r.created_at > cutoff)
            .collect();

        let total_verdicts = recent.len() as i64;

        let mut unique_calls = std::collections::HashSet::new();
        for r in &recent {
            unique_calls.insert(r.call_id.clone());
        }
        let total_calls = unique_calls.len() as i64;

        let blocks = recent.iter().filter(|r| r.outcome == "block").count() as i64;
        let escalations = recent.iter().filter(|r| r.outcome == "escalate").count() as i64;
        let passes = recent.iter().filter(|r| r.outcome == "pass").count() as i64;

        let open_escalations = store
            .iter()
            .filter(|r| r.outcome == "escalate")
            .count() as i64;

        let fast_path_latencies: Vec<f64> = recent
            .iter()
            .filter(|r| r.path == "fast")
            .filter_map(|r| r.latency_ms.map(|v| v as f64))
            .collect();
        let avg_latency = if fast_path_latencies.is_empty() {
            0.0
        } else {
            fast_path_latencies.iter().sum::<f64>() / fast_path_latencies.len() as f64
        };

        let mut axis_counts = std::collections::HashMap::new();
        for r in recent.iter().filter(|r| r.outcome == "block") {
            *axis_counts.entry(r.axis.clone()).or_insert(0i64) += 1;
        }
        let mut top_blocked_axes: Vec<AxisCount> = axis_counts
            .into_iter()
            .map(|(axis, count)| AxisCount { axis, count })
            .collect();
        top_blocked_axes.sort_by(|a, b| b.count.cmp(&a.count));
        top_blocked_axes.truncate(5);

        OverviewStats {
            total_calls_24h: total_calls,
            total_verdicts_24h: total_verdicts,
            blocks_24h: blocks,
            escalations_24h: escalations,
            passes_24h: passes,
            open_escalations,
            avg_fast_path_latency_ms: avg_latency,
            top_blocked_axes,
        }
    }
}
