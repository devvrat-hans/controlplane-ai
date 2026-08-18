use std::sync::Arc;

use arc_swap::ArcSwap;

/// Lock-free, hot-reloadable policy cache for fast-path checks.
/// Uses arc-swap so readers never block, and writers atomically swap in new rules.
#[derive(Clone)]
pub struct PolicyCache {
    inner: Arc<ArcSwap<FastPathRuleSet>>,
}

impl PolicyCache {
    pub fn new(rules: FastPathRuleSet) -> Self {
        Self {
            inner: Arc::new(ArcSwap::from_pointee(rules)),
        }
    }

    /// Atomically load the current rule set (lock-free read).
    pub fn load(&self) -> Arc<FastPathRuleSet> {
        self.inner.load_full()
    }

    /// Atomically swap in a new rule set (hot-reload).
    pub fn store(&self, rules: FastPathRuleSet) {
        self.inner.store(Arc::new(rules));
    }
}

impl Default for PolicyCache {
    fn default() -> Self {
        Self::new(FastPathRuleSet::default())
    }
}

/// The complete set of rules available to fast-path checks.
/// Loaded from PostgreSQL on a configurable interval and hot-swapped in.
#[derive(Clone, Debug)]
pub struct FastPathRuleSet {
    /// Per-request token output cap (None = no limit).
    pub max_tokens_per_request: Option<i32>,

    /// Retry detection: max requests in window before escalation.
    pub retry_max_count: u32,

    /// Retry detection: window duration in seconds.
    pub retry_window_seconds: u64,

    /// Custom unsafe content keywords (supplementing built-in patterns).
    pub unsafe_keywords: Vec<String>,

    /// Whether secret detection is enabled.
    pub secret_detection_enabled: bool,

    /// Whether cost cap check is enabled.
    pub cost_cap_enabled: bool,

    /// Whether retry detection is enabled.
    pub retry_detection_enabled: bool,

    /// Whether unsafe content check is enabled.
    pub unsafe_content_enabled: bool,
}

impl Default for FastPathRuleSet {
    fn default() -> Self {
        Self {
            max_tokens_per_request: None,
            retry_max_count: 5,
            retry_window_seconds: 60,
            unsafe_keywords: Vec::new(),
            secret_detection_enabled: true,
            cost_cap_enabled: true,
            retry_detection_enabled: true,
            unsafe_content_enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rules_are_sensible() {
        let rules = FastPathRuleSet::default();
        assert!(rules.max_tokens_per_request.is_none());
        assert_eq!(rules.retry_max_count, 5);
        assert_eq!(rules.retry_window_seconds, 60);
        assert!(rules.unsafe_keywords.is_empty());
        assert!(rules.secret_detection_enabled);
    }

    #[test]
    fn cache_load_returns_current_rules() {
        let cache = PolicyCache::new(FastPathRuleSet {
            max_tokens_per_request: Some(4096),
            ..FastPathRuleSet::default()
        });
        let loaded = cache.load();
        assert_eq!(loaded.max_tokens_per_request, Some(4096));
    }

    #[test]
    fn cache_store_atomically_updates() {
        let cache = PolicyCache::default();
        assert!(cache.load().max_tokens_per_request.is_none());

        cache.store(FastPathRuleSet {
            max_tokens_per_request: Some(2048),
            ..FastPathRuleSet::default()
        });

        assert_eq!(cache.load().max_tokens_per_request, Some(2048));
    }

    #[test]
    fn cache_is_clone_safe() {
        let cache = PolicyCache::default();
        let cache2 = cache.clone();

        cache.store(FastPathRuleSet {
            retry_max_count: 10,
            ..FastPathRuleSet::default()
        });

        // Both references see the same update
        assert_eq!(cache2.load().retry_max_count, 10);
    }
}
