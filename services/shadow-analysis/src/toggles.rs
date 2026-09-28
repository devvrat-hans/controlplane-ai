use std::sync::Arc;

use arc_swap::ArcSwap;
use sqlx::PgPool;
use tracing::{error, info, warn};

use controlplane_platform::messaging::EventSubscriber;

/// Runtime on/off switches for every shadow-path check.
/// Loaded from the `policies` table (canonical `checks` object) and hot-reloaded
/// whenever a `controlplane.policy.updated` event arrives.
#[derive(Debug, Clone)]
pub struct CheckToggles {
    // Shadow-path native checks
    pub groundedness: bool,
    pub bias_classification: bool,
    pub verbosity: bool,
    pub prompt_injection: bool,
    pub semantic_pii: bool,
    // Guardrails sidecar checks
    pub pii_detection: bool,
    pub toxicity_detection: bool,
    pub bias_detection: bool,
    // Hallucination check (Laya decision model, needs grounding context)
    pub hallucination: bool,
}

impl Default for CheckToggles {
    fn default() -> Self {
        Self {
            groundedness: true,
            bias_classification: true,
            verbosity: true,
            prompt_injection: true,
            semantic_pii: true,
            pii_detection: true,
            toxicity_detection: true,
            bias_detection: true,
            hallucination: true,
        }
    }
}

impl CheckToggles {
    fn parse_checks_obj(obj: &serde_json::Value, t: &mut CheckToggles) {
        let get = |k: &str| obj.get(k).and_then(|v| v.as_bool());
        if let Some(v) = get("groundedness_enabled") { t.groundedness = v; }
        if let Some(v) = get("hallucination_detection_enabled") { t.hallucination = v; }
        if let Some(v) = get("verbosity_enabled") { t.verbosity = v; }
        if let Some(v) = get("prompt_injection_enabled") { t.prompt_injection = v; }
        if let Some(v) = get("semantic_pii_enabled") { t.semantic_pii = v; }
        if let Some(v) = get("pii_detection") { t.pii_detection = v; }
        if let Some(v) = get("toxicity_detection") { t.toxicity_detection = v; }
        if let Some(v) = get("bias_detection") { t.bias_classification = v; t.bias_detection = v; }
    }

    /// Apply legacy flat keys too (config written before the `checks` object existed).
    fn apply_flat(t: &mut CheckToggles, config: &serde_json::Value) {
        if let Some(b) = config.get("groundedness_enabled").and_then(|v| v.as_bool()) { t.groundedness = b; }
        if let Some(b) = config.get("verbosity_enabled").and_then(|v| v.as_bool()) { t.verbosity = b; }
        if let Some(b) = config.get("prompt_injection_enabled").and_then(|v| v.as_bool()) { t.prompt_injection = b; }
        if let Some(b) = config.get("semantic_pii_enabled").and_then(|v| v.as_bool()) { t.semantic_pii = b; }
        if let Some(b) = config.get("pii_detection").and_then(|v| v.as_bool()) { t.pii_detection = b; }
        if let Some(b) = config.get("toxicity_detection").and_then(|v| v.as_bool()) { t.toxicity_detection = b; }
        if let Some(b) = config.get("bias_detection").and_then(|v| v.as_bool()) { t.bias_classification = b; t.bias_detection = b; }
    }
}

/// Shared, lock-free toggle store read on every shadow analysis request.
#[derive(Clone)]
pub struct ToggleStore {
    inner: Arc<ArcSwap<CheckToggles>>,
}

impl Default for ToggleStore {
    fn default() -> Self {
        Self::new(CheckToggles::default())
    }
}

impl ToggleStore {
    pub fn new(toggles: CheckToggles) -> Self {
        Self { inner: Arc::new(ArcSwap::from_pointee(toggles)) }
    }

    pub fn load(&self) -> Arc<CheckToggles> {
        self.inner.load_full()
    }

    pub fn store(&self, toggles: CheckToggles) {
        self.inner.store(Arc::new(toggles));
    }
}

/// Load check toggles from the active policy rows in PostgreSQL.
pub async fn load_toggles_from_db(pool: &PgPool) -> Result<CheckToggles, sqlx::Error> {
    let rows: Vec<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT axis, threshold_config FROM policies WHERE is_active = TRUE"
    )
    .fetch_all(pool)
    .await?;

    Ok(toggles_from_rows(&rows))
}

fn toggles_from_rows(rows: &[(String, serde_json::Value)]) -> CheckToggles {
    let mut t = CheckToggles::default();
    for (axis, config) in rows {
        match axis.as_str() {
            "performance" => {
                if let Some(obj) = config.get("checks").and_then(|v| v.as_object()) {
                    CheckToggles::parse_checks_obj(&serde_json::Value::Object(obj.clone()), &mut t);
                }
                CheckToggles::apply_flat(&mut t, config);
            }
            "responsibility" => {
                if let Some(obj) = config.get("checks").and_then(|v| v.as_object()) {
                    CheckToggles::parse_checks_obj(&serde_json::Value::Object(obj.clone()), &mut t);
                }
                // Engine-format actions double as kill-switches
                if let Some(a) = config.get("pii_action").and_then(|v| v.as_str()) {
                    if a == "off" { t.pii_detection = false; t.semantic_pii = false; }
                }
                if let Some(a) = config.get("unsafe_action").and_then(|v| v.as_str()) {
                    if a == "off" && config.get("checks").is_none() {
                        // Only treat as global off in legacy configs without explicit checks
                        warn!("Legacy unsafe_action=off without checks object");
                    }
                }
                CheckToggles::apply_flat(&mut t, config);
            }
            _ => {}
        }
    }
    t
}

/// Background task: initial load + reload whenever a POLICY_UPDATED event arrives.
pub fn spawn_toggle_reloader(
    pool: PgPool,
    store: ToggleStore,
    subscriber: Arc<dyn EventSubscriber>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        // Initial load at startup
        match load_toggles_from_db(&pool).await {
            Ok(t) => {
                store.store(t);
                info!("Shadow check toggles loaded from policies table");
            }
            Err(e) => warn!(error = %e, "Initial toggle load failed — using defaults"),
        }

        let mut receiver = match subscriber.subscribe(controlplane_common::events::subjects::POLICY_UPDATED).await {
            Ok(rx) => rx,
            Err(e) => {
                warn!(error = %e, "Toggle reloader: failed to subscribe to policy updates");
                return;
            }
        };

        loop {
            tokio::select! {
                msg = receiver.recv() => {
                    if msg.is_none() {
                        info!("Toggle reloader subscription closed");
                        break;
                    }
                    match load_toggles_from_db(&pool).await {
                        Ok(t) => {
                            store.store(t);
                            info!("Shadow check toggles reloaded after policy update");
                        }
                        Err(e) => error!(error = %e, "Failed to reload toggles"),
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Toggle reloader shutting down");
                        break;
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rows_give_all_enabled() {
        let t = toggles_from_rows(&[]);
        assert!(t.groundedness && t.prompt_injection && t.semantic_pii);
    }

    #[test]
    fn checks_object_disables_toggles() {
        let rows = vec![
            ("performance".to_string(), serde_json::json!({
                "checks": { "groundedness_enabled": false, "verbosity_enabled": false }
            })),
            ("responsibility".to_string(), serde_json::json!({
                "checks": { "prompt_injection_enabled": false, "semantic_pii_enabled": true }
            })),
        ];
        let t = toggles_from_rows(&rows);
        assert!(!t.groundedness);
        assert!(!t.verbosity);
        assert!(!t.prompt_injection);
        assert!(t.semantic_pii);
        assert!(t.hallucination); // untouched stays default
    }

    #[test]
    fn legacy_decision_judge_key_is_ignored() {
        // The decision judge was removed; stored policies may still carry the key.
        let rows = vec![(
            "responsibility".to_string(),
            serde_json::json!({ "checks": { "decision_judge_enabled": false } }),
        )];
        let t = toggles_from_rows(&rows);
        assert!(t.groundedness && t.prompt_injection && t.hallucination);
    }

    #[test]
    fn engine_action_off_disables_pii_family() {
        let rows = vec![
            ("responsibility".to_string(), serde_json::json!({ "pii_action": "off" })),
        ];
        let t = toggles_from_rows(&rows);
        assert!(!t.pii_detection);
        assert!(!t.semantic_pii);
    }
}
