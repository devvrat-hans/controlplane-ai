use std::sync::Arc;

use tracing::{debug, error, info, warn};

use controlplane_common::events::{
    subjects, EventEnvelope, ShadowAnalysisRequest, ShadowCheckRun, ShadowCheckStatus, ShadowCompletedPayload,
    VerdictPayload,
};
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

    // Launch every check as a timed task, or record why it was not started.
    // Disabled checks are still skipped entirely — they just leave a run record now.
    let groundedness: Launch<_> = if toggles.groundedness {
        let config = config.clone();
        let response = response_text.clone();
        let context = context_text.clone();
        Ok(spawn_timed(async move {
            GroundednessChecker::new(config.groundedness_threshold).check(&response, context.as_deref())
        }))
    } else { Err(DISABLED_BY_POLICY.to_string()) };

    let bias: Launch<_> = if toggles.bias_classification {
        let config = config.clone();
        let response = response_text.clone();
        Ok(spawn_timed(async move { BiasClassifier::new(config.bias_threshold).check(&response) }))
    } else { Err(DISABLED_BY_POLICY.to_string()) };

    let verbosity: Launch<_> = if toggles.verbosity {
        let config = config.clone();
        let response = response_text.clone();
        let prompt = prompt_text.clone();
        Ok(spawn_timed(async move {
            VerbosityChecker::new(config.verbosity_max_ratio, config.verbosity_min_density).check(&response, &prompt)
        }))
    } else { Err(DISABLED_BY_POLICY.to_string()) };

    // Prompt injection detection (runs on the INPUT prompt)
    let prompt_injection: Launch<_> = if toggles.prompt_injection {
        let config = config.clone();
        let prompt = prompt_text.clone();
        Ok(spawn_timed(async move {
            let detector = PromptInjectionDetector::new(&config);
            let result = detector.check(&prompt);
            detector.to_verdict(&result)
        }))
    } else { Err(DISABLED_BY_POLICY.to_string()) };

    let semantic_pii: Launch<_> = if toggles.semantic_pii {
        let config = config.clone();
        let response = response_text.clone();
        Ok(spawn_timed(async move {
            SemanticPiiDetector::new(config.semantic_pii_min_identifiers, config.semantic_pii_risk_threshold)
                .check(&response)
        }))
    } else { Err(DISABLED_BY_POLICY.to_string()) };

    // Guardrails sidecar gate: server switch, then per-app toggle, then a configured URL.
    let sidecar = |server_on: bool, toggle_on: bool| -> Result<GuardrailsClient, String> {
        if !server_on {
            return Err("disabled in server config".to_string());
        }
        if !toggle_on {
            return Err(DISABLED_BY_POLICY.to_string());
        }
        config
            .guardrails_url
            .as_deref()
            .map(|url| GuardrailsClient::with_timeout_ms(url, config.guardrails_timeout_ms))
            .ok_or_else(|| NO_SIDECAR.to_string())
    };

    // Guardrails checks on the RESPONSE (Presidio PII + LLM Guard toxicity). Bias is
    // input-only: on responses it over-fires (opinionated != biased).
    let pii: Launch<_> = sidecar(config.pii_enabled, toggles.pii_detection).map(|client| {
        let text = response_text.clone();
        spawn_timed(async move { client.scan_pii(&text).await })
    });

    let toxicity: Launch<_> = sidecar(config.toxicity_enabled, toggles.toxicity_detection).map(|client| {
        let text = response_text.clone();
        let prompt = prompt_text.clone();
        spawn_timed(async move { client.scan_toxicity(&text, Some(&prompt)).await })
    });

    // Also scan the INPUT prompt for toxicity/bias (catches inappropriate prompts)
    let input_toxicity: Launch<_> = sidecar(config.toxicity_enabled, toggles.toxicity_detection).and_then(|client| {
        if prompt_text.is_empty() {
            return Err(NO_PROMPT.to_string());
        }
        let text = prompt_text.clone();
        Ok(spawn_timed(async move { client.scan_toxicity(&text, None).await }))
    });

    let input_bias: Launch<_> = sidecar(config.bias_enabled, toggles.bias_detection).and_then(|client| {
        if prompt_text.is_empty() {
            return Err(NO_PROMPT.to_string());
        }
        let text = prompt_text.clone();
        Ok(spawn_timed(async move { client.scan_bias(&text, None).await }))
    });

    // Hallucination check (Laya), run on every request: against the grounding context
    // (a system message) when present, otherwise against the question itself. One small
    // call (2-3 questions). It only emits scores; the decision engine aggregates them.
    // Fails open.
    let hallucination: Launch<_> = if !toggles.hallucination {
        Err(DISABLED_BY_POLICY.to_string())
    } else {
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

                Ok(spawn_timed(async move {
                    let state = GovernanceState::new(&response, &prompt, context.as_deref());
                    client.evaluate_hallucination(&state).await
                }))
            }
            None => Err("Laya not configured (LAYA_URL unset)".to_string()),
        }
    };

    // Collect all results. Every check leaves exactly one run record; a verdict is
    // emitted only when a check found something. Verdict durations are the task's
    // wall time, measured the same way for every check.
    let mut verdicts: Vec<ShadowVerdict> = Vec::new();
    let mut runs: Vec<ShadowCheckRun> = Vec::new();

    if let Some((result, ms)) = finish("groundedness", groundedness, &mut runs).await {
        runs.push(ran("groundedness", ms));
        push_verdict(&mut verdicts, result.verdict, ms);
    }
    if let Some((result, ms)) = finish("bias_classification", bias, &mut runs).await {
        runs.push(ran("bias_classification", ms));
        push_verdict(&mut verdicts, result.verdict, ms);
    }
    if let Some((result, ms)) = finish("verbosity", verbosity, &mut runs).await {
        runs.push(ran("verbosity", ms));
        push_verdict(&mut verdicts, result.verdict, ms);
    }
    if let Some((verdict, ms)) = finish("prompt_injection", prompt_injection, &mut runs).await {
        runs.push(ran("prompt_injection", ms));
        push_verdict(&mut verdicts, verdict, ms);
    }
    if let Some((result, ms)) = finish("semantic_pii", semantic_pii, &mut runs).await {
        runs.push(ran("semantic_pii", ms));
        push_verdict(&mut verdicts, result.verdict, ms);
    }

    for (name, launch, input_label) in [
        ("pii", pii, None),
        ("toxicity", toxicity, None),
        ("input_toxicity", input_toxicity, Some("input-toxicity")),
        ("input_bias", input_bias, Some("input-bias")),
    ] {
        let Some((scan, ms)) = finish(name, launch, &mut runs).await else { continue };
        match scan {
            Ok(verdict) => {
                runs.push(ran(name, ms));
                // Input-side scans are renamed so they read as prompt findings.
                let verdict = verdict.map(|mut v| {
                    if let Some(label) = input_label {
                        v.check_name = label.to_string();
                        v.reason = format!("Input: {}", v.reason);
                    }
                    v
                });
                push_verdict(&mut verdicts, verdict, ms);
            }
            Err(e) => runs.push(run_record(name, ShadowCheckStatus::Error, Some(ms), Some(e))),
        }
    }

    // Laya hallucination verdicts (laya-hallucination / laya-groundedness, plus
    // sub-threshold evidence readings). A judge that never answered is an error.
    if let Some((judged, ms)) = finish("hallucination", hallucination, &mut runs).await {
        match judged {
            Ok(laya_verdicts) => {
                runs.push(ran("hallucination", ms));
                for v in laya_verdicts {
                    push_verdict(&mut verdicts, Some(v), ms);
                }
            }
            Err(e) => {
                warn!(error = %e, "Laya hallucination check unavailable — FAIL OPEN");
                runs.push(run_record("hallucination", ShadowCheckStatus::Error, Some(ms), Some(e)));
            }
        }
    }

    // Demote the keyword PII heuristic to a genuine fallback (plan §4.3).
    if let Some(dropped) = demote_semantic_pii(&mut verdicts) {
        debug!(
            correlation_id = %correlation_id,
            dropped,
            "Superseded keyword PII heuristic by a stronger PII detector"
        );
        if let Some(run) = runs.iter_mut().find(|r| r.check_name == "semantic_pii") {
            run.detail = Some("finding superseded by a stronger PII detector".to_string());
        }
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
        ).with_duration_precise(shadow_verdict.duration_ms);

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

    // Per-check run records (passes and skips included) for the request detail page.
    let checks_run = runs.iter().filter(|r| r.status == ShadowCheckStatus::Ran).count();
    let completed = EventEnvelope::new(
        subjects::SHADOW_COMPLETED,
        correlation_id,
        envelope.app_id,
        ShadowCompletedPayload { call_id: request.call_id, checks: runs },
    );
    if let Ok(bytes) = completed.to_bytes() {
        if let Err(e) = publisher.publish(subjects::SHADOW_COMPLETED, &bytes).await {
            warn!(error = %e, "Failed to publish shadow completion record");
        }
    }

    let worst_outcome = verdicts.iter()
        .map(|v| v.outcome)
        .fold(Outcome::Pass, Outcome::worst);

    info!(
        correlation_id = %correlation_id,
        checks_run,
        verdicts_produced = verdicts.len(),
        worst_outcome = %worst_outcome,
        "Shadow analysis complete"
    );
}

/// A started check (timed task) or the reason it was not started.
type Launch<T> = Result<tokio::task::JoinHandle<(T, f64)>, String>;

const DISABLED_BY_POLICY: &str = "disabled by policy";
const NO_SIDECAR: &str = "guardrails sidecar not configured";
const NO_PROMPT: &str = "no prompt text in request";

/// Spawn a check and measure its wall time in fractional milliseconds.
fn spawn_timed<F>(fut: F) -> tokio::task::JoinHandle<(F::Output, f64)>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(async move {
        let start = std::time::Instant::now();
        let out = fut.await;
        (out, start.elapsed().as_secs_f64() * 1000.0)
    })
}

fn run_record(name: &str, status: ShadowCheckStatus, ms: Option<f64>, detail: Option<String>) -> ShadowCheckRun {
    ShadowCheckRun {
        check_name: name.to_string(),
        status,
        duration_us: ms.map(|ms| (ms.max(0.0) * 1000.0).round() as i64),
        detail,
    }
}

fn ran(name: &str, ms: f64) -> ShadowCheckRun {
    run_record(name, ShadowCheckStatus::Ran, Some(ms), None)
}

/// Await a launch. Returns the output when the check ran; records a `Skipped`
/// (not started) or `Error` (task panicked) run otherwise.
async fn finish<T>(name: &str, launch: Launch<T>, runs: &mut Vec<ShadowCheckRun>) -> Option<(T, f64)> {
    match launch {
        Err(reason) => {
            runs.push(run_record(name, ShadowCheckStatus::Skipped, None, Some(reason)));
            None
        }
        Ok(handle) => match handle.await {
            Ok(out) => Some(out),
            Err(e) => {
                warn!(check = name, error = %e, "Shadow check task failed — FAIL OPEN");
                runs.push(run_record(name, ShadowCheckStatus::Error, None, Some("check task failed".to_string())));
                None
            }
        },
    }
}

/// Keep a finding, stamped with its check's measured wall time.
fn push_verdict(verdicts: &mut Vec<ShadowVerdict>, verdict: Option<ShadowVerdict>, ms: f64) {
    if let Some(mut v) = verdict {
        v.duration_ms = ms;
        verdicts.push(v);
    }
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
            duration_ms: 1.0,
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

    #[tokio::test]
    async fn skipped_check_records_its_reason() {
        let mut runs = Vec::new();
        let launch: Launch<u8> = Err(DISABLED_BY_POLICY.to_string());
        assert!(finish("groundedness", launch, &mut runs).await.is_none());
        assert_eq!(runs, vec![run_record("groundedness", ShadowCheckStatus::Skipped, None, Some(DISABLED_BY_POLICY.into()))]);
    }

    #[tokio::test]
    async fn completed_check_returns_output_and_wall_time() {
        let mut runs = Vec::new();
        let launch: Launch<u8> = Ok(spawn_timed(async {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            7u8
        }));
        let (out, ms) = finish("verbosity", launch, &mut runs).await.unwrap();
        assert_eq!(out, 7);
        assert!(ms >= 5.0, "measured {ms}ms");
        // `finish` leaves the success record to the caller (it may still be a scan error).
        assert!(runs.is_empty());
        assert_eq!(ran("verbosity", 1.2345).duration_us, Some(1235));
    }

    #[tokio::test]
    async fn panicking_check_is_recorded_as_error() {
        let mut runs = Vec::new();
        let launch: Launch<u8> = Ok(spawn_timed(async { panic!("boom") }));
        assert!(finish("bias_classification", launch, &mut runs).await.is_none());
        assert_eq!(runs[0].status, ShadowCheckStatus::Error);
    }

    #[test]
    fn verdict_takes_the_measured_duration() {
        let mut verdicts = Vec::new();
        push_verdict(&mut verdicts, Some(verdict("groundedness", Outcome::Escalate)), 3.25);
        push_verdict(&mut verdicts, None, 9.0);
        assert_eq!(verdicts.len(), 1);
        assert_eq!(verdicts[0].duration_ms, 3.25);
    }
}
