use controlplane_common::provider::ProviderKind;
use controlplane_common::types::{Axis, Outcome};

/// A verdict produced by a shadow-path check.
#[derive(Debug, Clone)]
pub struct ShadowVerdict {
    pub axis: Axis,
    pub check_name: String,
    pub outcome: Outcome,
    pub confidence: f32,
    pub reason: String,
    pub duration_ms: u32,
}

/// Configuration for shadow-path analysis thresholds.
#[derive(Debug, Clone)]
pub struct ShadowConfig {
    pub groundedness_threshold: f32,
    pub bias_threshold: f32,
    pub verbosity_max_ratio: f32,
    pub verbosity_min_density: f32,
    pub semantic_pii_min_identifiers: usize,
    pub semantic_pii_risk_threshold: f32,
    pub prompt_injection_threshold: f32,
    pub provider: ProviderKind,
    pub guardrails_url: Option<String>,
    pub pii_enabled: bool,
    pub toxicity_enabled: bool,
    pub bias_enabled: bool,
}

impl Default for ShadowConfig {
    fn default() -> Self {
        Self {
            groundedness_threshold: 0.6,
            bias_threshold: 0.7,
            verbosity_max_ratio: 10.0,
            verbosity_min_density: 0.3,
            semantic_pii_min_identifiers: 3,
            semantic_pii_risk_threshold: 0.5,
            prompt_injection_threshold: 0.5,
            provider: ProviderKind::Anthropic,
            guardrails_url: std::env::var("GUARDRAILS_URL").ok(),
            pii_enabled: true,
            toxicity_enabled: true,
            bias_enabled: true,
        }
    }
}
