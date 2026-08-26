use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sqlx::PgPool;
use tracing::{info, warn};

use controlplane_platform::messaging::EventPublisher;

use crate::types::ShadowVerdict;

/// Configuration for pattern promotion.
#[derive(Debug, Clone)]
pub struct PromotionConfig {
    /// Number of recurring occurrences before a pattern is promoted to fast-path.
    pub promotion_threshold: u32,
    /// Whether to persist tracking to PostgreSQL (false = in-memory only for demo).
    pub persist_to_db: bool,
}

impl Default for PromotionConfig {
    fn default() -> Self {
        Self {
            promotion_threshold: 5,
            persist_to_db: true,
        }
    }
}

/// Tracks recurring patterns from shadow verdicts and promotes them
/// to fast-path rules when they recur enough times.
#[derive(Clone)]
pub struct PatternPromoter {
    config: PromotionConfig,
    /// In-memory tracker: pattern_key → occurrence count.
    tracker: Arc<Mutex<HashMap<String, PatternEntry>>>,
    pool: Option<PgPool>,
    publisher: Option<Arc<dyn EventPublisher>>,
}

#[derive(Debug, Clone)]
struct PatternEntry {
    count: u32,
    check_name: String,
    sample_reason: String,
    promoted: bool,
}

impl PatternPromoter {
    pub fn new(config: PromotionConfig, pool: Option<PgPool>, publisher: Option<Arc<dyn EventPublisher>>) -> Self {
        Self {
            config,
            tracker: Arc::new(Mutex::new(HashMap::new())),
            pool,
            publisher,
        }
    }

    /// Track a shadow verdict for potential promotion.
    /// Returns true if this call triggered a promotion.
    pub async fn track_verdict(&self, verdict: &ShadowVerdict) -> bool {
        let pattern_key = derive_pattern_key(verdict);
        let should_promote;

        {
            let mut tracker = self.tracker.lock().unwrap_or_else(|e| e.into_inner());
            let entry = tracker.entry(pattern_key.clone()).or_insert_with(|| PatternEntry {
                count: 0,
                check_name: verdict.check_name.clone(),
                sample_reason: verdict.reason.clone(),
                promoted: false,
            });

            entry.count += 1;
            entry.sample_reason = verdict.reason.clone();

            should_promote = !entry.promoted && entry.count >= self.config.promotion_threshold;
            if should_promote {
                entry.promoted = true;
            }
        }

        // Persist to database
        if self.config.persist_to_db {
            if let Some(pool) = &self.pool {
                self.upsert_pattern_in_db(pool, &pattern_key, verdict, should_promote).await;
            }
        }

        // If promoted, signal policy reload
        if should_promote {
            info!(
                check = &verdict.check_name,
                pattern_key = &pattern_key,
                "Pattern promoted to fast-path after {} occurrences",
                self.config.promotion_threshold,
            );

            if let Some(publisher) = &self.publisher {
                let _ = publisher.publish("controlplane.policy.reload", b"promoted").await;
            }
        }

        should_promote
    }

    /// Get the current promotion state (for diagnostics / dashboard).
    pub fn get_tracked_patterns(&self) -> Vec<TrackedPattern> {
        let tracker = self.tracker.lock().unwrap_or_else(|e| e.into_inner());
        tracker.iter().map(|(key, entry)| TrackedPattern {
            pattern_key: key.clone(),
            check_name: entry.check_name.clone(),
            occurrence_count: entry.count,
            promoted: entry.promoted,
            sample_reason: entry.sample_reason.clone(),
        }).collect()
    }

    async fn upsert_pattern_in_db(&self, pool: &PgPool, pattern_key: &str, verdict: &ShadowVerdict, promoted: bool) {
        let promoted_at: Option<chrono::DateTime<chrono::Utc>> = if promoted {
            Some(chrono::Utc::now())
        } else {
            None
        };

        let promoted_rule: Option<serde_json::Value> = if promoted {
            Some(build_promoted_rule(verdict))
        } else {
            None
        };

        let result = sqlx::query(
            r#"
            INSERT INTO pattern_promotions (check_name, pattern_key, occurrence_count, promoted, promoted_at, sample_reason, promoted_rule, last_seen_at)
            VALUES ($1, $2, 1, $3, $4, $5, $6, NOW())
            ON CONFLICT (check_name, pattern_key) DO UPDATE SET
                occurrence_count = pattern_promotions.occurrence_count + 1,
                last_seen_at = NOW(),
                sample_reason = $5,
                promoted = COALESCE($3, pattern_promotions.promoted),
                promoted_at = COALESCE($4, pattern_promotions.promoted_at),
                promoted_rule = COALESCE($6, pattern_promotions.promoted_rule)
            "#
        )
        .bind(&verdict.check_name)
        .bind(pattern_key)
        .bind(promoted)
        .bind(promoted_at)
        .bind(&verdict.reason)
        .bind(promoted_rule)
        .execute(pool)
        .await;

        if let Err(e) = result {
            warn!(error = %e, "Failed to upsert pattern promotion in DB");
        }
    }
}

/// A summary of a tracked pattern (for API/dashboard).
#[derive(Debug, Clone)]
pub struct TrackedPattern {
    pub pattern_key: String,
    pub check_name: String,
    pub occurrence_count: u32,
    pub promoted: bool,
    pub sample_reason: String,
}

/// Derive a stable key for a pattern from a shadow verdict.
/// Groups similar verdicts together by check_name + normalized signal.
fn derive_pattern_key(verdict: &ShadowVerdict) -> String {
    match verdict.check_name.as_str() {
        "bias_classification" => {
            // Group by the bias category detected
            if let Some(cats) = extract_categories_from_reason(&verdict.reason) {
                format!("bias:{}", cats)
            } else {
                "bias:general".to_string()
            }
        }
        "unsafe_content" => {
            // Group by the keyword that triggered
            if let Some(keyword) = extract_keyword_from_reason(&verdict.reason) {
                format!("unsafe:{}", keyword)
            } else {
                "unsafe:general".to_string()
            }
        }
        "groundedness" => {
            "groundedness:low_score".to_string()
        }
        "verbosity" => {
            "verbosity:excessive".to_string()
        }
        "semantic_pii" => {
            "semantic_pii:reidentification".to_string()
        }
        other => {
            format!("{}:general", other)
        }
    }
}

/// Build a rule to be added to fast-path when a pattern is promoted.
fn build_promoted_rule(verdict: &ShadowVerdict) -> serde_json::Value {
    serde_json::json!({
        "source": "pattern_promotion",
        "check_name": verdict.check_name,
        "axis": verdict.axis.as_str(),
        "outcome": verdict.outcome.as_str(),
        "reason_sample": verdict.reason,
        "confidence": verdict.confidence,
    })
}

fn extract_categories_from_reason(reason: &str) -> Option<String> {
    // "Bias detected ... in categories: gender, race"
    if let Some(idx) = reason.find("categories:") {
        let after = &reason[idx + 12..];
        let end = after.find(')').unwrap_or(after.len());
        return Some(after[..end].trim().to_string());
    }
    None
}

fn extract_keyword_from_reason(reason: &str) -> Option<String> {
    // "Blocked: content matches unsafe keyword policy (\"keyword\")"
    if let Some(start) = reason.find('"') {
        if let Some(end) = reason[start + 1..].find('"') {
            return Some(reason[start + 1..start + 1 + end].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use controlplane_common::types::{Axis, Outcome};

    fn make_verdict(check_name: &str, reason: &str) -> ShadowVerdict {
        ShadowVerdict {
            axis: Axis::Responsibility,
            check_name: check_name.to_string(),
            outcome: Outcome::Escalate,
            confidence: 0.8,
            reason: reason.to_string(),
            duration_ms: 5,
        }
    }

    #[tokio::test]
    async fn tracks_and_promotes_after_threshold() {
        let config = PromotionConfig {
            promotion_threshold: 3,
            persist_to_db: false,
        };
        let promoter = PatternPromoter::new(config, None, None);
        let verdict = make_verdict("bias_classification", "Bias detected in categories: gender");

        assert!(!promoter.track_verdict(&verdict).await); // 1
        assert!(!promoter.track_verdict(&verdict).await); // 2
        assert!(promoter.track_verdict(&verdict).await);  // 3 → promoted!
        assert!(!promoter.track_verdict(&verdict).await); // 4 → already promoted, no repeat
    }

    #[tokio::test]
    async fn different_patterns_tracked_separately() {
        let config = PromotionConfig {
            promotion_threshold: 3,
            persist_to_db: false,
        };
        let promoter = PatternPromoter::new(config, None, None);
        let v1 = make_verdict("bias_classification", "Bias detected in categories: gender");
        let v2 = make_verdict("bias_classification", "Bias detected in categories: race");

        promoter.track_verdict(&v1).await;
        promoter.track_verdict(&v1).await;
        promoter.track_verdict(&v2).await;

        let patterns = promoter.get_tracked_patterns();
        assert_eq!(patterns.len(), 2);
    }

    #[test]
    fn derive_pattern_key_bias() {
        let v = make_verdict("bias_classification", "Bias detected in categories: gender, race");
        let key = derive_pattern_key(&v);
        assert!(key.starts_with("bias:"));
        assert!(key.contains("gender"));
    }

    #[test]
    fn derive_pattern_key_unsafe() {
        let v = make_verdict("unsafe_content", r#"Blocked: content matches unsafe keyword policy ("forbidden_word")"#);
        let key = derive_pattern_key(&v);
        assert_eq!(key, "unsafe:forbidden_word");
    }

    #[test]
    fn derive_pattern_key_groundedness() {
        let v = make_verdict("groundedness", "Low groundedness score");
        let key = derive_pattern_key(&v);
        assert_eq!(key, "groundedness:low_score");
    }

    #[test]
    fn build_promoted_rule_json() {
        let v = make_verdict("bias_classification", "Bias in categories: gender");
        let rule = build_promoted_rule(&v);
        assert_eq!(rule["source"], "pattern_promotion");
        assert_eq!(rule["check_name"], "bias_classification");
    }
}
