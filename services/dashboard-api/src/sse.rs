use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use tokio::sync::broadcast;
use tracing::{info, warn};

use controlplane_common::events::{EventEnvelope, VerdictPayload};
use controlplane_platform::messaging::EventSubscriber;

use crate::in_memory_store::{InMemoryVerdictStore, VerdictRecord};

/// Shared state for SSE broadcasting.
/// Receives verdicts from NATS and broadcasts to all connected SSE clients.
#[derive(Clone)]
pub struct SseBroadcaster {
    tx: broadcast::Sender<String>,
}

impl SseBroadcaster {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Subscribe to the SSE stream (called per-client connection).
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    /// Publish a verdict event to all connected SSE clients.
    pub fn publish(&self, event_json: String) {
        let _ = self.tx.send(event_json);
    }
}

/// Spawn a background task that bridges NATS verdicts to the SSE broadcaster.
pub fn spawn_sse_bridge(
    broadcaster: SseBroadcaster,
    subscriber: Arc<dyn EventSubscriber>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    verdict_store: Option<InMemoryVerdictStore>,
) {
    tokio::spawn(async move {
        let mut receiver = match subscriber.subscribe("controlplane.verdict.*").await {
            Ok(rx) => rx,
            Err(e) => {
                warn!(error = %e, "SSE bridge: failed to subscribe to verdicts");
                return;
            }
        };

        info!("SSE bridge: forwarding verdicts to connected clients");

        loop {
            tokio::select! {
                msg = receiver.recv() => {
                    match msg {
                        Some(payload) => {
                            if let Ok(envelope) = serde_json::from_slice::<EventEnvelope<VerdictPayload>>(&payload) {
                                let event = serde_json::json!({
                                    "type": "verdict",
                                    "correlation_id": envelope.correlation_id,
                                    "app_id": envelope.app_id,
                                    "timestamp": envelope.timestamp,
                                    "verdict": {
                                        "id": envelope.payload.verdict.id,
                                        "call_id": envelope.payload.verdict.call_id,
                                        "axis": envelope.payload.verdict.axis.as_str(),
                                        "path": envelope.payload.verdict.path.as_str(),
                                        "outcome": envelope.payload.verdict.outcome.as_str(),
                                        "confidence": envelope.payload.verdict.confidence,
                                        "reason": envelope.payload.verdict.reason,
                                        "check_name": envelope.payload.verdict.check_name,
                                    }
                                });
                                broadcaster.publish(event.to_string());

                                // Also store in memory for the dashboard API
                                if let Some(ref store) = verdict_store {
                                    store.push(VerdictRecord {
                                        id: envelope.payload.verdict.id.to_string(),
                                        call_id: envelope.payload.verdict.call_id.to_string(),
                                        app_id: Some(envelope.app_id.to_string()),
                                        axis: envelope.payload.verdict.axis.as_str().to_string(),
                                        path: envelope.payload.verdict.path.as_str().to_string(),
                                        outcome: envelope.payload.verdict.outcome.as_str().to_string(),
                                        confidence: envelope.payload.verdict.confidence,
                                        reason: envelope.payload.verdict.reason,
                                        check_name: envelope.payload.verdict.check_name,
                                        latency_ms: None,
                                        created_at: envelope.timestamp,
                                    });
                                }
                            }
                        }
                        None => {
                            info!("SSE bridge: subscription closed");
                            break;
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("SSE bridge shutting down");
                        break;
                    }
                }
            }
        }
    });
}

/// SSE handler: streams live verdicts to connected clients.
pub async fn verdict_stream_handler(
    broadcaster: SseBroadcaster,
) -> impl IntoResponse {
    let rx = broadcaster.subscribe();

    let stream = async_stream::stream! {
        let mut rx = rx;
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    yield Ok::<_, Infallible>(Event::default().data(msg));
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    let warning = serde_json::json!({
                        "type": "warning",
                        "message": format!("Skipped {} events (slow client)", n)
                    });
                    yield Ok(Event::default().data(warning.to_string()));
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
