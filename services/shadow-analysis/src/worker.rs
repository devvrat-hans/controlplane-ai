use std::sync::Arc;

use tracing::{debug, error, info, warn};

use controlplane_common::events::{subjects, EventEnvelope, ShadowAnalysisRequest, VerdictPayload};
use controlplane_common::models::Verdict;
use controlplane_common::{create_provider, provider::UpstreamProvider};
use controlplane_common::types::{Outcome, Path as VerdictPath};
use controlplane_platform::messaging::{EventPublisher, EventSubscriber};

use crate::bias::BiasClassifier;
use crate::calibration::CalibrationStore;
use crate::governance_questions::GovernanceState;
use crate::groundedness::GroundednessChecker;
use crate::guardrails_client::GuardrailsClient;
use crate::laya_client::LayaClient;
use crate::prompt_injection::PromptInjectionDetector;
use crate::semantic_pii::SemanticPiiDetector;
use crate::toggles::ToggleStore;
use crate::types::{ShadowConfig, ShadowVerdict};
use crate::verbosity::VerbosityChecker;

/// `check_name` of the stronger, span-producing PII detector (Presidio sidecar).
pub(crate) const PRESIDIO_PII_CHECK: &str = "presidio-pii";
/// `check_name` of the judge's contextual re-identification detector.
pub(crate) const LAYA_PII_CHECK: &str = "laya-semantic-pii";
/// `check_name` of the keyword heuristic that is being demoted to a fallback.
pub(crate) const KEYWORD_PII_CHECK: &str = "semantic_pii";

/// Shadow-path worker: subscribes to NATS, runs all async checks in parallel,
/// publishes verdicts back.
pub struct ShadowWorker {
    subscriber: Arc<dyn EventSubscriber>,
    publisher: Arc<dyn EventPublisher>,
    config: ShadowConfig,
    /// Hot-reloadable check toggles from the policy engine (Policies page switches).
    toggles: ToggleStore,
    /// Fitted detector calibration (temperature scaling). Inert unless a fit exists.
    calibration: CalibrationStore,
}

impl ShadowWorker {
    pub fn new(
        subscriber: Arc<dyn EventSubscriber>,
        publisher: Arc<dyn EventPublisher>,
        config: ShadowConfig,
    ) -> Self {
        Self {
            subscriber,
            publisher,
            config,
            toggles: ToggleStore::default(),
            calibration: CalibrationStore::new(),
        }
    }

    /// Attach a shared toggle store so Policies page switches take effect live.
    pub fn with_toggles(mut self, toggles: ToggleStore) -> Self {
        self.toggles = toggles;
        self
    }

    /// Attach the shared calibration store so a re-fit takes effect without a restart.
    pub fn with_calibration(mut self, calibration: CalibrationStore) -> Self {
        self.calibration = calibration;
        self
    }

    /// Start the shadow worker loop. Runs until the shutdown signal.
    pub async fn run(self, mut shutdown_rx: tokio::sync::watch::Receiver<bool>) {
        let mut receiver = match self.subscriber.subscribe(subjects::INTERCEPT_SHADOW).await {
            Ok(rx) => rx,
            Err(e) => {
                error!(error = %e, "Shadow worker failed to subscribe");
                return;
            }
        };

        info!("Shadow analysis worker started, listening on '{}'", subjects::INTERCEPT_SHADOW);

        loop {
            tokio::select! {
                msg = receiver.recv() => {
                    match msg {
                        Some(payload) => {
                            let publisher = self.publisher.clone();
                            let config = self.config.clone();
                            let toggles = self.toggles.clone();
                            let calibration = self.calibration.clone();
                            tokio::spawn(async move {
                                process_message(&payload, &publisher, &config, &toggles, &calibration).await;
                            });
                        }
                        None => {
                            warn!("Shadow worker subscription closed");
                            break;
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Shadow worker shutting down");
                        break;
                    }
                }
            }
        }
    }
}

async fn process_message(
    payload: &[u8],
    publisher: &Arc<dyn EventPublisher>,
    config: &ShadowConfig,
    toggles: &ToggleStore,
    calibration: &CalibrationStore,
) {
    // Snapshot the current check toggles (Policies page switches)
    let toggles = toggles.load();
    // Snapshot the fitted calibration. Inert unless a fit has been written.
    let calibration = calibration.load();
    let envelope: EventEnvelope<ShadowAnalysisRequest> = match serde_json::from_slice(payload) {
        Ok(env) => env,
        Err(e) => {
            warn!(error = %e, "Failed to deserialize shadow analysis request");
            return;
        }
    };

    let correlation_id = envelope.correlation_id;
    let request = &envelope.payload;

    debug!(correlation_id = %correlation_id, "Processing shadow analysis");

    // Build the appropriate provider for parsing
    let provider: Box<dyn UpstreamProvider> = create_provider(config.provider);

    let response_text = request.response_payload
        .as_ref()
        .and_then(|v| {
            let bytes = serde_json::to_vec(v).ok()?;
            provider.extract_response_text(&bytes)
        })
        .unwrap_or_default();

    let prompt_text = request.request_payload
        .as_ref()
        .and_then(|v| {
            let bytes = serde_json::to_vec(v).ok()?;
            provider.extract_request_prompt(&bytes)
        })
        .unwrap_or_default();

    let context_text = request.request_payload
        .as_ref()
        .and_then(|v| {
            let bytes = serde_json::to_vec(v).ok()?;
            provider.extract_context(&bytes)
        });

    // Run all enabled checks in parallel (disabled checks are skipped entirely)
    let config_clone = config.clone();
    let response_clone = response_text.clone();
    let context_clone = context_text.clone();
    let groundedness_handle = if toggles.groundedness {
        Some(tokio::spawn(async move {
            let checker = GroundednessChecker::new(config_clone.groundedness_threshold);
            checker.check(&response_clone, context_clone.as_deref())
        }))
    } else { None };

    let config_clone = config.clone();
    let response_clone = response_text.clone();
    let bias_handle = if toggles.bias_classification {
        Some(tokio::spawn(async move {
            let classifier = BiasClassifier::new(config_clone.bias_threshold);
            classifier.check(&response_clone)
        }))
    } else { None };

    let config_clone = config.clone();
    let response_clone = response_text.clone();
    let prompt_clone = prompt_text.clone();
    let verbosity_handle = if toggles.verbosity {
        Some(tokio::spawn(async move {
            let checker = VerbosityChecker::new(config_clone.verbosity_max_ratio, config_clone.verbosity_min_density);
            checker.check(&response_clone, &prompt_clone)
        }))
    } else { None };

    // Prompt injection detection (runs on the INPUT prompt)
    let config_clone = config.clone();
    let prompt_clone = prompt_text.clone();
    let prompt_injection_handle = if toggles.prompt_injection {
        Some(tokio::spawn(async move {
            let detector = PromptInjectionDetector::new(&config_clone);
            let result = detector.check(&prompt_clone);
            detector.to_verdict(&result)
        }))
    } else { None };

    let config_clone = config.clone();
    let response_clone = response_text.clone();
    let semantic_pii_handle = if toggles.semantic_pii {
        Some(tokio::spawn(async move {
            let detector = SemanticPiiDetector::new(
                config_clone.semantic_pii_min_identifiers,
                config_clone.semantic_pii_risk_threshold,
            );
            detector.check(&response_clone)
        }))
    } else { None };

    // Run guardrails sidecar checks on RESPONSE (Presidio PII + LLM Guard Toxicity/Bias)
    let guardrails_pii_handle = if config.pii_enabled && toggles.pii_detection {
        if let Some(ref url) = config.guardrails_url {
            let client = GuardrailsClient::new(url);
            let text = response_text.clone();
            Some(tokio::spawn(async move { client.scan_pii(&text).await }))
        } else { None }
    } else { None };

    let guardrails_toxicity_handle = if config.toxicity_enabled && toggles.toxicity_detection {
        if let Some(ref url) = config.guardrails_url {
            let client = GuardrailsClient::new(url);
            let text = response_text.clone();
            let prompt = prompt_text.clone();
            Some(tokio::spawn(async move { client.scan_toxicity(&text, Some(&prompt)).await }))
        } else { None }
    } else { None };

    // Bias on response text produces too many false positives (opinionated != biased).
    // Only scan inputs for bias; response bias is caught by the input_bias_handle below.
    let guardrails_bias_handle: Option<tokio::task::JoinHandle<Option<ShadowVerdict>>> = None;

    // DeepEval hallucination check (compares response against context)
    let guardrails_hallucination_handle = if toggles.hallucination {
        if let Some(ref ctx) = context_text {
        if let Some(ref url) = config.guardrails_url {
            let client = GuardrailsClient::new(url);
            let text = response_text.clone();
            let context = ctx.clone();
            Some(tokio::spawn(async move { client.scan_hallucination(&text, Some(&context)).await }))
        } else { None }
    } else { None }
    } else { None };

    // Also scan the INPUT prompt for toxicity/bias (catches inappropriate prompts)
    let input_toxicity_handle = if config.toxicity_enabled && toggles.toxicity_detection && !prompt_text.is_empty() {
        if let Some(ref url) = config.guardrails_url {
            let client = GuardrailsClient::new(url);
            let text = prompt_text.clone();
            Some(tokio::spawn(async move { client.scan_toxicity(&text, None).await }))
        } else { None }
    } else { None };

    let input_bias_handle = if config.bias_enabled && toggles.bias_detection && !prompt_text.is_empty() {
        if let Some(ref url) = config.guardrails_url {
            let client = GuardrailsClient::new(url);
            let text = prompt_text.clone();
            Some(tokio::spawn(async move { client.scan_bias(&text, None).await }))
        } else { None }
    } else { None };

    // Decision-model judge (Laya / Jev): every governance question in ONE batched call.
    //
    // The process-level master switch is `DECISION_JUDGE=laya|jev`; when it is unset (the
    // default) no HTTP call is made at all and the shadow path behaves exactly as before.
    // A per-app policy may additionally opt out via `checks.decision_judge_enabled = false`.
    //
    // The judge never blocks delivery and never makes the final decision — it only emits
    // per-check scores that the decision engine aggregates. Any failure yields no
    // verdicts (absence of a shadow verdict means pass).
    let laya_handle = if config.decision_judge_enabled && toggles.decision_judge {
        match config.laya_url.as_ref() {
            Some(url) => {
                let client = LayaClient::new(
                    url,
                    config.laya_timeout_ms,
                    config.laya_model.clone(),
                    config.laya_api_key.clone(),
                )
                // Fitted temperatures, so the thresholds downstream are statistically
                // meaningful rather than raw over-confident scores. Inert by default.
                .with_calibration((*calibration).clone());
                let response = response_text.clone();
                let prompt = prompt_text.clone();
                let context = context_text.clone();

                Some(tokio::spawn(async move {
                    let state = GovernanceState::new(&response, &prompt, context.as_deref());
                    client.evaluate(&state).await
                }))
            }
            None => {
                warn!("Decision judge enabled but LAYA_URL is not set — skipping the judge");
                None
            }
        }
    } else {
        None
    };

    // Collect all results
    let mut verdicts: Vec<ShadowVerdict> = Vec::new();

    if let Some(handle) = groundedness_handle {
        if let Ok(result) = handle.await {
            if let Some(v) = result.verdict {
                verdicts.push(v);
            }
        }
    }

    if let Some(handle) = bias_handle {
        if let Ok(result) = handle.await {
            if let Some(v) = result.verdict {
                verdicts.push(v);
            }
        }
    }

    if let Some(handle) = verbosity_handle {
        if let Ok(result) = handle.await {
            if let Some(v) = result.verdict {
                verdicts.push(v);
            }
        }
    }

    if let Some(handle) = prompt_injection_handle {
        if let Ok(Some(v)) = handle.await {
            verdicts.push(v);
        }
    }

    if let Some(handle) = semantic_pii_handle {
        if let Ok(result) = handle.await {
            if let Some(v) = result.verdict {
                verdicts.push(v);
            }
        }
    }

    // Collect guardrails sidecar results
    if let Some(handle) = guardrails_pii_handle {
        if let Ok(Some(v)) = handle.await {
            verdicts.push(v);
        }
    }

    if let Some(handle) = guardrails_toxicity_handle {
        if let Ok(Some(v)) = handle.await {
            verdicts.push(v);
        }
    }

    if let Some(handle) = guardrails_bias_handle {
        if let Ok(Some(v)) = handle.await {
            verdicts.push(v);
        }
    }

    if let Some(handle) = guardrails_hallucination_handle {
        if let Ok(Some(v)) = handle.await {
            verdicts.push(v);
        }
    }

    // Collect input-side guardrails results (rename check for clarity)
    if let Some(handle) = input_toxicity_handle {
        if let Ok(Some(mut v)) = handle.await {
            v.check_name = "input-toxicity".to_string();
            v.reason = format!("Input: {}", v.reason);
            verdicts.push(v);
        }
    }

    if let Some(handle) = input_bias_handle {
        if let Ok(Some(mut v)) = handle.await {
            v.check_name = "input-bias".to_string();
            v.reason = format!("Input: {}", v.reason);
            verdicts.push(v);
        }
    }

    // Decision-model judge verdicts. A failed or timed-out task contributes nothing.
    if let Some(handle) = laya_handle {
        match handle.await {
            Ok(laya_verdicts) => verdicts.extend(laya_verdicts),
            Err(e) => warn!(error = %e, "Laya judge task failed — FAIL OPEN"),
        }
    }

    // Demote the keyword PII heuristic to a genuine fallback (plan §4.3).
    if let Some(dropped) = demote_semantic_pii(&mut verdicts) {
        debug!(
            correlation_id = %correlation_id,
            dropped,
            "Superseded keyword PII heuristic by a stronger PII detector"
        );
    }

    // Publish each verdict
    for shadow_verdict in &verdicts {
        let verdict = Verdict::new(
            request.call_id,
            shadow_verdict.axis,
            VerdictPath::Shadow,
            shadow_verdict.outcome,
            shadow_verdict.confidence,
            &shadow_verdict.reason,
            &shadow_verdict.check_name,
        ).with_duration(shadow_verdict.duration_ms as i32);

        let payload = VerdictPayload { verdict };
        let envelope = EventEnvelope::new(
            subjects::VERDICT_SHADOW,
            correlation_id,
            envelope.app_id,
            payload,
        );

        if let Ok(bytes) = envelope.to_bytes() {
            if let Err(e) = publisher.publish(subjects::VERDICT_SHADOW, &bytes).await {
                warn!(
                    error = %e,
                    check = &shadow_verdict.check_name,
                    "Failed to publish shadow verdict"
                );
            }
        }
    }

    let worst_outcome = verdicts.iter()
        .map(|v| v.outcome)
        .fold(Outcome::Pass, Outcome::worst);

    info!(
        correlation_id = %correlation_id,
        checks_run = 4,
        verdicts_produced = verdicts.len(),
        worst_outcome = %worst_outcome,
        "Shadow analysis complete"
    );
}

/// Demote the `semantic_pii` keyword heuristic to a fallback.
///
/// Plan §4.3: the heuristic is *strictly dominated* on this job. Presidio finds the
/// actual entities and returns character offsets that can be redacted; the judge reasons
/// about inference-based re-identification that Presidio structurally cannot see. Running
/// the keyword list *alongside* them adds a third, correlated signal that over-fires on
/// the word "email" — the classic way an ensemble inflates confidence without adding
/// information.
///
/// So: when a stronger PII detector actually reported on this response, the keyword
/// verdict is dropped. When neither did — the sidecar is down, or the judge is off — the
/// heuristic survives as the fail-open fallback it was always meant to be.
///
/// Returns `Some(dropped_count)` when the heuristic was superseded, `None` otherwise.
pub(crate) fn demote_semantic_pii(verdicts: &mut Vec<ShadowVerdict>) -> Option<usize> {
    let stronger_pii_reported = verdicts.iter().any(|v| {
        v.check_name == PRESIDIO_PII_CHECK
            || (v.check_name == LAYA_PII_CHECK && v.outcome != Outcome::Pass)
    });

    if !stronger_pii_reported {
        return None;
    }

    let before = verdicts.len();
    verdicts.retain(|v| v.check_name != KEYWORD_PII_CHECK);
    let dropped = before - verdicts.len();

    if dropped == 0 {
        None
    } else {
        Some(dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use controlplane_common::types::Axis;

    fn verdict(check_name: &str, outcome: Outcome) -> ShadowVerdict {
        ShadowVerdict {
            axis: Axis::Responsibility,
            check_name: check_name.to_string(),
            outcome,
            confidence: 0.8,
            reason: "test".to_string(),
            duration_ms: 1,
        }
    }

    #[test]
    fn keyword_pii_is_dropped_when_presidio_reported() {
        let mut verdicts = vec![
            verdict(PRESIDIO_PII_CHECK, Outcome::Edit),
            verdict(KEYWORD_PII_CHECK, Outcome::Escalate),
        ];

        assert_eq!(demote_semantic_pii(&mut verdicts), Some(1));
        assert_eq!(verdicts.len(), 1);
        assert_eq!(verdicts[0].check_name, PRESIDIO_PII_CHECK);
    }

    #[test]
    fn keyword_pii_is_dropped_when_the_judge_reported() {
        let mut verdicts = vec![
            verdict(LAYA_PII_CHECK, Outcome::Escalate),
            verdict(KEYWORD_PII_CHECK, Outcome::Escalate),
        ];

        assert_eq!(demote_semantic_pii(&mut verdicts), Some(1));
    }

    #[test]
    fn keyword_pii_survives_when_the_sidecar_is_down() {
        // This is the whole point of keeping the heuristic: it is the fallback when
        // neither the sidecar nor the judge produced anything.
        let mut verdicts = vec![
            verdict(KEYWORD_PII_CHECK, Outcome::Escalate),
            verdict("prompt_injection", Outcome::Escalate),
        ];

        assert_eq!(demote_semantic_pii(&mut verdicts), None);
        assert_eq!(verdicts.len(), 2);
    }

    #[test]
    fn a_pre_judge_laya_evidence_verdict_does_not_supersede_the_heuristic() {
        let mut verdicts = vec![
            verdict(&format!("{LAYA_PII_CHECK}-evidence"), Outcome::Pass),
            verdict(KEYWORD_PII_CHECK, Outcome::Escalate),
        ];

        assert_eq!(demote_semantic_pii(&mut verdicts), None);
        assert_eq!(verdicts.len(), 2);
    }

    #[test]
    fn demotion_is_a_no_op_without_a_keyword_verdict() {
        let mut verdicts = vec![verdict(PRESIDIO_PII_CHECK, Outcome::Edit)];

        assert_eq!(demote_semantic_pii(&mut verdicts), None);
        assert_eq!(verdicts.len(), 1);
    }
}


