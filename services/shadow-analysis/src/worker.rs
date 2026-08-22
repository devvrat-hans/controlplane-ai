use std::sync::Arc;

use tracing::{debug, error, info, warn};

use controlplane_common::events::{subjects, EventEnvelope, ShadowAnalysisRequest, VerdictPayload};
use controlplane_common::models::Verdict;
use controlplane_common::{create_provider, provider::UpstreamProvider};
use controlplane_common::types::{Outcome, Path as VerdictPath};
use controlplane_platform::messaging::{EventPublisher, EventSubscriber};

use crate::bias::BiasClassifier;
use crate::groundedness::GroundednessChecker;
use crate::semantic_pii::SemanticPiiDetector;
use crate::types::{ShadowConfig, ShadowVerdict};
use crate::verbosity::VerbosityChecker;

/// Shadow-path worker: subscribes to NATS, runs all async checks in parallel,
/// publishes verdicts back.
pub struct ShadowWorker {
    subscriber: Arc<dyn EventSubscriber>,
    publisher: Arc<dyn EventPublisher>,
    config: ShadowConfig,
}

impl ShadowWorker {
    pub fn new(
        subscriber: Arc<dyn EventSubscriber>,
        publisher: Arc<dyn EventPublisher>,
        config: ShadowConfig,
    ) -> Self {
        Self { subscriber, publisher, config }
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
                            tokio::spawn(async move {
                                process_message(&payload, &publisher, &config).await;
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
) {
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

    // Run all checks in parallel
    let config_clone = config.clone();
    let response_clone = response_text.clone();
    let context_clone = context_text.clone();
    let groundedness_handle = tokio::spawn(async move {
        let checker = GroundednessChecker::new(config_clone.groundedness_threshold);
        checker.check(&response_clone, context_clone.as_deref())
    });

    let config_clone = config.clone();
    let response_clone = response_text.clone();
    let bias_handle = tokio::spawn(async move {
        let classifier = BiasClassifier::new(config_clone.bias_threshold);
        classifier.check(&response_clone)
    });

    let config_clone = config.clone();
    let response_clone = response_text.clone();
    let prompt_clone = prompt_text.clone();
    let verbosity_handle = tokio::spawn(async move {
        let checker = VerbosityChecker::new(config_clone.verbosity_max_ratio, config_clone.verbosity_min_density);
        checker.check(&response_clone, &prompt_clone)
    });

    let config_clone = config.clone();
    let response_clone = response_text.clone();
    let semantic_pii_handle = tokio::spawn(async move {
        let detector = SemanticPiiDetector::new(
            config_clone.semantic_pii_min_identifiers,
            config_clone.semantic_pii_risk_threshold,
        );
        detector.check(&response_clone)
    });

    // Collect all results
    let mut verdicts: Vec<ShadowVerdict> = Vec::new();

    if let Ok(result) = groundedness_handle.await {
        if let Some(v) = result.verdict {
            verdicts.push(v);
        }
    }

    if let Ok(result) = bias_handle.await {
        if let Some(v) = result.verdict {
            verdicts.push(v);
        }
    }

    if let Ok(result) = verbosity_handle.await {
        if let Some(v) = result.verdict {
            verdicts.push(v);
        }
    }

    if let Ok(result) = semantic_pii_handle.await {
        if let Some(v) = result.verdict {
            verdicts.push(v);
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


