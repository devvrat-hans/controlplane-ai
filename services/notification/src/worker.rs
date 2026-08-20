use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tracing::{error, info, warn};
use uuid::Uuid;

use controlplane_common::events::{subjects, DecisionPayload, EventEnvelope};
use controlplane_common::types::Outcome;
use controlplane_platform::messaging::EventSubscriber;

use crate::slack::SlackNotifier;
use crate::types::NotificationPayload;
use crate::webhook::WebhookNotifier;

/// Rate limiter to prevent notification spam during burst events.
/// Aggregates notifications per app within a time window.
struct RateLimiter {
    window: Duration,
    max_per_window: usize,
    /// Tracks (app_id) -> list of notification timestamps in current window
    buckets: HashMap<Uuid, Vec<Instant>>,
}

impl RateLimiter {
    fn new(window_secs: u64, max_per_window: usize) -> Self {
        Self {
            window: Duration::from_secs(window_secs),
            max_per_window,
            buckets: HashMap::new(),
        }
    }

    /// Returns true if the notification should be sent (not rate-limited).
    fn allow(&mut self, app_id: Uuid) -> bool {
        let now = Instant::now();
        let timestamps = self.buckets.entry(app_id).or_default();

        // Remove expired entries
        timestamps.retain(|t| now.duration_since(*t) < self.window);

        if timestamps.len() >= self.max_per_window {
            false
        } else {
            timestamps.push(now);
            true
        }
    }

    /// Periodic cleanup of stale entries.
    fn cleanup(&mut self) {
        let now = Instant::now();
        self.buckets.retain(|_, timestamps| {
            timestamps.retain(|t| now.duration_since(*t) < self.window);
            !timestamps.is_empty()
        });
    }
}

/// Notification worker: subscribes to decision events, dispatches alerts.
pub struct NotificationWorker {
    slack: SlackNotifier,
    webhook: WebhookNotifier,
    rate_limiter: Arc<Mutex<RateLimiter>>,
}

impl NotificationWorker {
    pub fn new() -> Self {
        let rate_window = std::env::var("NOTIFICATION_RATE_WINDOW_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60u64);
        let rate_max = std::env::var("NOTIFICATION_RATE_MAX_PER_WINDOW")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5usize);

        Self {
            slack: SlackNotifier::new(),
            webhook: WebhookNotifier::new(),
            rate_limiter: Arc::new(Mutex::new(RateLimiter::new(rate_window, rate_max))),
        }
    }

    /// Process a single decision event and send notifications if needed.
    async fn handle_decision(&self, envelope: EventEnvelope<DecisionPayload>) {
        let decision = &envelope.payload.decision;

        // Only notify on block or escalate
        if decision.final_outcome == Outcome::Pass {
            return;
        }

        let app_id = decision.app_id;

        // Rate limiting check
        {
            let mut limiter = self.rate_limiter.lock().await;
            if !limiter.allow(app_id) {
                warn!(
                    app_id = %app_id,
                    outcome = decision.final_outcome.as_str(),
                    "Notification rate-limited"
                );
                return;
            }
        }

        let payload = NotificationPayload {
            call_id: decision.call_id,
            app_id,
            app_name: None,
            outcome: decision.final_outcome.as_str().to_string(),
            axis: "governance".to_string(),
            confidence: 1.0,
            reason: format!(
                "Decision outcome: {} ({} contributing verdicts)",
                decision.final_outcome.as_str(),
                decision.contributing_verdicts.len()
            ),
            timestamp: envelope.timestamp,
        };

        // Send to all configured channels in parallel
        let (slack_result, webhook_result) = tokio::join!(
            self.slack.send(&payload),
            self.webhook.send(&payload),
        );

        if let Err(e) = slack_result {
            error!(call_id = %payload.call_id, error = %e, "Slack notification failed");
        }
        if let Err(e) = webhook_result {
            error!(call_id = %payload.call_id, error = %e, "Webhook notification failed");
        }
    }
}

impl Default for NotificationWorker {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawn the notification background listener.
pub fn spawn_notification_worker(
    subscriber: Arc<dyn EventSubscriber>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let worker = NotificationWorker::new();

    tokio::spawn(async move {
        let mut receiver = match subscriber.subscribe(subjects::DECISION_FINAL).await {
            Ok(rx) => rx,
            Err(e) => {
                error!(error = %e, "Notification worker: failed to subscribe");
                return;
            }
        };

        info!(
            slack_configured = worker.slack.is_configured(),
            webhook_configured = worker.webhook.is_configured(),
            "Notification worker: listening on '{}'", subjects::DECISION_FINAL,
        );

        // Periodic cleanup timer
        let mut cleanup_interval = tokio::time::interval(Duration::from_secs(300));
        cleanup_interval.tick().await; // skip immediate first tick

        loop {
            tokio::select! {
                msg = receiver.recv() => {
                    match msg {
                        Some(payload) => {
                            match serde_json::from_slice::<EventEnvelope<DecisionPayload>>(&payload) {
                                Ok(envelope) => {
                                    worker.handle_decision(envelope).await;
                                }
                                Err(e) => {
                                    warn!(error = %e, "Failed to parse decision event");
                                }
                            }
                        }
                        None => {
                            info!("Notification worker: subscription closed");
                            break;
                        }
                    }
                }
                _ = cleanup_interval.tick() => {
                    let mut limiter = worker.rate_limiter.lock().await;
                    limiter.cleanup();
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Notification worker shutting down");
                        break;
                    }
                }
            }
        }
    });
}
