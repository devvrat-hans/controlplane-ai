use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use sqlx::PgPool;
use tracing::{debug, error, info, warn};

use crate::policy_cache::{FastPathRuleSet, PolicyCache};

/// Metrics exposed by the policy reloader.
#[derive(Debug, Default)]
pub struct ReloadMetrics {
    pub reload_count: AtomicU64,
    pub last_reload_latency_ms: AtomicU64,
    pub last_version_loaded: AtomicU64,
    pub error_count: AtomicU64,
}

/// Configuration for the policy reload background task.
#[derive(Clone, Debug)]
pub struct ReloadConfig {
    /// How often to poll PostgreSQL for changes (seconds).
    pub poll_interval_secs: u64,
    /// Optional: only load policies for a specific app_id.
    pub app_id_filter: Option<uuid::Uuid>,
}

impl Default for ReloadConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: 30,
            app_id_filter: None,
        }
    }
}

/// Spawns a background tokio task that polls PostgreSQL for policy changes
/// and hot-swaps the new rule set into the PolicyCache.
///
/// Returns a handle to the metrics and a shutdown sender.
pub fn spawn_policy_reloader(
    pool: PgPool,
    cache: PolicyCache,
    config: ReloadConfig,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Arc<ReloadMetrics> {
    let metrics = Arc::new(ReloadMetrics::default());
    let metrics_clone = metrics.clone();

    tokio::spawn(async move {
        info!(
            poll_interval_secs = config.poll_interval_secs,
            "Policy reloader started"
        );

        let mut last_max_version: i32 = 0;

        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(config.poll_interval_secs)) => {}
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Policy reloader shutting down");
                        return;
                    }
                }
            }

            let start = Instant::now();

            match load_policies_from_db(&pool, config.app_id_filter).await {
                Ok((rule_set, max_version)) => {
                    let latency_ms = start.elapsed().as_millis() as u64;
                    metrics_clone.last_reload_latency_ms.store(latency_ms, Ordering::Relaxed);

                    if max_version > last_max_version {
                        cache.store(rule_set);
                        last_max_version = max_version;
                        metrics_clone.reload_count.fetch_add(1, Ordering::Relaxed);
                        metrics_clone.last_version_loaded.store(max_version as u64, Ordering::Relaxed);

                        info!(
                            version = max_version,
                            latency_ms,
                            "Policy cache reloaded with new version"
                        );
                    } else {
                        debug!(
                            version = max_version,
                            latency_ms,
                            "Policy unchanged, skipping reload"
                        );
                    }
                }
                Err(e) => {
                    metrics_clone.error_count.fetch_add(1, Ordering::Relaxed);
                    error!(error = %e, "Failed to reload policies from PostgreSQL");
                }
            }
        }
    });

    metrics
}

/// Spawns a NATS subscriber that triggers immediate reload on signal.
pub fn spawn_nats_reload_trigger(
    cache: PolicyCache,
    pool: PgPool,
    subscriber: Arc<dyn controlplane_platform::messaging::EventSubscriber>,
    metrics: Arc<ReloadMetrics>,
    app_id_filter: Option<uuid::Uuid>,
) {
    tokio::spawn(async move {
        let subject = "controlplane.policy.reload";
        let mut receiver = match subscriber.subscribe(subject).await {
            Ok(rx) => rx,
            Err(e) => {
                warn!(error = %e, "Failed to subscribe to policy reload signal");
                return;
            }
        };

        info!("Listening for NATS policy reload signals on '{}'", subject);

        loop {
            match receiver.recv().await {
                Some(_) => {
                    info!("Received NATS policy reload signal — triggering immediate reload");
                    let start = Instant::now();

                    match load_policies_from_db(&pool, app_id_filter).await {
                        Ok((rule_set, max_version)) => {
                            cache.store(rule_set);
                            let latency_ms = start.elapsed().as_millis() as u64;
                            metrics.reload_count.fetch_add(1, Ordering::Relaxed);
                            metrics.last_reload_latency_ms.store(latency_ms, Ordering::Relaxed);
                            metrics.last_version_loaded.store(max_version as u64, Ordering::Relaxed);
                            info!(version = max_version, latency_ms, "Policy cache force-reloaded via signal");
                        }
                        Err(e) => {
                            metrics.error_count.fetch_add(1, Ordering::Relaxed);
                            error!(error = %e, "Force reload from signal failed");
                        }
                    }
                }
                None => {
                    warn!("NATS reload subscription closed");
                    break;
                }
            }
        }
    });
}

/// Row shape for a policy read from PostgreSQL.
#[derive(sqlx::FromRow)]
struct PolicyRow {
    pub axis: String,
    pub threshold_config: serde_json::Value,
    pub version: i32,
}

/// Load active policies from PostgreSQL and merge into a single FastPathRuleSet.
async fn load_policies_from_db(
    pool: &PgPool,
    app_id_filter: Option<uuid::Uuid>,
) -> Result<(FastPathRuleSet, i32), sqlx::Error> {
    let rows: Vec<PolicyRow> = if let Some(app_id) = app_id_filter {
        sqlx::query_as::<_, PolicyRow>(
            "SELECT axis, threshold_config, version FROM policies WHERE app_id = $1 AND is_active = TRUE ORDER BY version DESC"
        )
        .bind(app_id)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, PolicyRow>(
            "SELECT axis, threshold_config, version FROM policies WHERE is_active = TRUE ORDER BY version DESC"
        )
        .fetch_all(pool)
        .await?
    };

    let max_version = rows.iter().map(|r| r.version).max().unwrap_or(0);
    let rule_set = merge_policies_into_rules(&rows);

    Ok((rule_set, max_version))
}

/// Merge all active policy rows into a single unified FastPathRuleSet.
fn merge_policies_into_rules(rows: &[PolicyRow]) -> FastPathRuleSet {
    let mut rules = FastPathRuleSet::default();

    for row in rows {
        let config = &row.threshold_config;

        match row.axis.as_str() {
            "cost" => {
                if let Some(max_tokens) = config.get("max_tokens_per_request").and_then(|v| v.as_i64()) {
                    // Use the most restrictive cap across all loaded policies
                    let current = rules.max_tokens_per_request.unwrap_or(i32::MAX);
                    rules.max_tokens_per_request = Some(current.min(max_tokens as i32));
                }
                if let Some(retry_max) = config.get("retry_max").and_then(|v| v.as_u64()) {
                    rules.retry_max_count = rules.retry_max_count.min(retry_max as u32);
                }
                if let Some(retry_window) = config.get("retry_window_seconds").and_then(|v| v.as_u64()) {
                    rules.retry_window_seconds = rules.retry_window_seconds.min(retry_window);
                }
            }
            "responsibility" => {
                if let Some(keywords) = config.get("unsafe_keywords").and_then(|v| v.as_array()) {
                    for kw in keywords {
                        if let Some(s) = kw.as_str() {
                            if !rules.unsafe_keywords.contains(&s.to_string()) {
                                rules.unsafe_keywords.push(s.to_string());
                            }
                        }
                    }
                }
                if let Some(pii_action) = config.get("pii_action").and_then(|v| v.as_str()) {
                    rules.secret_detection_enabled = pii_action != "off";
                }
                if let Some(unsafe_action) = config.get("unsafe_action").and_then(|v| v.as_str()) {
                    rules.unsafe_content_enabled = unsafe_action != "off";
                }
            }
            _ => {} // "performance" axis is handled by shadow-path
        }
    }

    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_empty_gives_defaults() {
        let rules = merge_policies_into_rules(&[]);
        assert!(rules.max_tokens_per_request.is_none());
        assert_eq!(rules.retry_max_count, 5);
        assert!(rules.secret_detection_enabled);
    }

    #[test]
    fn merge_cost_policy_extracts_cap() {
        let rows = vec![PolicyRow {
            axis: "cost".to_string(),
            threshold_config: serde_json::json!({
                "max_tokens_per_request": 4096,
                "retry_max": 3,
                "retry_window_seconds": 30
            }),
            version: 1,
        }];

        let rules = merge_policies_into_rules(&rows);
        assert_eq!(rules.max_tokens_per_request, Some(4096));
        assert_eq!(rules.retry_max_count, 3);
        assert_eq!(rules.retry_window_seconds, 30);
    }

    #[test]
    fn merge_responsibility_policy_extracts_keywords() {
        let rows = vec![PolicyRow {
            axis: "responsibility".to_string(),
            threshold_config: serde_json::json!({
                "pii_action": "edit",
                "unsafe_action": "block",
                "unsafe_keywords": ["dangerous_thing", "banned_term"]
            }),
            version: 1,
        }];

        let rules = merge_policies_into_rules(&rows);
        assert!(rules.secret_detection_enabled);
        assert!(rules.unsafe_content_enabled);
        assert_eq!(rules.unsafe_keywords.len(), 2);
        assert!(rules.unsafe_keywords.contains(&"dangerous_thing".to_string()));
    }

    #[test]
    fn merge_multiple_cost_policies_uses_most_restrictive() {
        let rows = vec![
            PolicyRow {
                axis: "cost".to_string(),
                threshold_config: serde_json::json!({ "max_tokens_per_request": 8192 }),
                version: 1,
            },
            PolicyRow {
                axis: "cost".to_string(),
                threshold_config: serde_json::json!({ "max_tokens_per_request": 4096 }),
                version: 2,
            },
        ];

        let rules = merge_policies_into_rules(&rows);
        assert_eq!(rules.max_tokens_per_request, Some(4096));
    }

    #[test]
    fn merge_disables_pii_if_off() {
        let rows = vec![PolicyRow {
            axis: "responsibility".to_string(),
            threshold_config: serde_json::json!({
                "pii_action": "off",
                "unsafe_action": "off"
            }),
            version: 1,
        }];

        let rules = merge_policies_into_rules(&rows);
        assert!(!rules.secret_detection_enabled);
        assert!(!rules.unsafe_content_enabled);
    }
}
