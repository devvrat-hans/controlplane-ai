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
    pub duration_ms: f64,
}

/// Read an environment variable, trimmed, treating a blank value as absent.
///
/// Trimming matters more than it looks: the judge fails open, so a value that only
/// *looks* configured (a trailing space, a newline from a secret store) would leave the
/// judge silently doing nothing rather than reporting a bad configuration.
fn env_trimmed(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
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

    // ── Decision-model judge (Laya / Jev) ────────────────────────────────────────
    /// Master switch for the judge. **False by default** so the shadow path behaves
    /// exactly as it did before unless `DECISION_JUDGE` is set to `laya` or `jev`.
    pub decision_judge_enabled: bool,
    /// Base URL of the judge — `laya-serve` (Jev-compatible `/v1/systemone`).
    pub laya_url: Option<String>,
    /// Optional bearer token (`LAYA_API_KEY`, or `TYPESAFE_API_KEY` when using Jev).
    pub laya_api_key: Option<String>,
    /// Hard timeout for a single judge call, in milliseconds.
    pub laya_timeout_ms: u64,
    /// Explicit checkpoint override. `None` lets Laya's Router pick by language.
    pub laya_model: Option<String>,
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

            // Opt-in only: `DECISION_JUDGE=laya|jev` enables the judge. Anything else
            // (including unset) leaves the shadow path exactly as it was.
            decision_judge_enabled: matches!(
                std::env::var("DECISION_JUDGE")
                    .unwrap_or_default()
                    .trim()
                    .to_lowercase()
                    .as_str(),
                "laya" | "jev"
            ),
            // Every judge setting is normalised before use. These values arrive from
            // `.env` files and CI secrets, where a stray space or a trailing newline is
            // common — and because the judge fails open, a malformed URL, key or model
            // name would not surface as an error. It would silently leave the judge
            // scoring nothing at all, which is far harder to notice than a hard crash.
            laya_url: env_trimmed("LAYA_URL"),
            laya_api_key: env_trimmed("LAYA_API_KEY").or_else(|| env_trimmed("TYPESAFE_API_KEY")),
            laya_timeout_ms: env_trimmed("LAYA_TIMEOUT_MS")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(5000),
            // `auto` (the default) means: let the Router choose the checkpoint. Matched
            // case-insensitively for the same reason `DECISION_JUDGE` is, so `AUTO` — a
            // natural thing to type — is not forwarded as a literal model name.
            laya_model: env_trimmed("LAYA_MODEL")
                .filter(|model| !model.eq_ignore_ascii_case("auto")),
        }
    }
}
