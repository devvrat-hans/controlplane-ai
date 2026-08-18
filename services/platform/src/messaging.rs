use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, info};

// =============================================================================
// Event bus trait — one interface, two implementations
// =============================================================================

#[async_trait]
pub trait EventPublisher: Send + Sync + 'static {
    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), anyhow::Error>;
}

#[async_trait]
pub trait EventSubscriber: Send + Sync + 'static {
    async fn subscribe(&self, subject: &str) -> Result<EventReceiver, anyhow::Error>;
}

pub struct EventReceiver {
    inner: broadcast::Receiver<(String, Vec<u8>)>,
    subject_filter: String,
}

impl EventReceiver {
    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        loop {
            match self.inner.recv().await {
                Ok((subject, payload)) => {
                    if subject_matches(&subject, &self.subject_filter) {
                        return Some(payload);
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(lagged = n, "Event receiver lagged, skipping messages");
                    continue;
                }
            }
        }
    }
}

fn subject_matches(subject: &str, filter: &str) -> bool {
    if filter.ends_with(".*") {
        let prefix = &filter[..filter.len() - 2];
        subject.starts_with(prefix)
    } else {
        subject == filter
    }
}

// =============================================================================
// In-process event bus (for local dev / demo without NATS)
// =============================================================================

#[derive(Clone)]
pub struct InProcessBus {
    sender: broadcast::Sender<(String, Vec<u8>)>,
}

impl InProcessBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(4096);
        info!("In-process event bus initialized (capacity: 4096)");
        Self { sender }
    }
}

impl Default for InProcessBus {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventPublisher for InProcessBus {
    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), anyhow::Error> {
        debug!(subject, bytes = payload.len(), "Publishing event (in-process)");
        let _ = self.sender.send((subject.to_string(), payload.to_vec()));
        Ok(())
    }
}

#[async_trait]
impl EventSubscriber for InProcessBus {
    async fn subscribe(&self, subject: &str) -> Result<EventReceiver, anyhow::Error> {
        debug!(subject, "Subscribing (in-process)");
        Ok(EventReceiver {
            inner: self.sender.subscribe(),
            subject_filter: subject.to_string(),
        })
    }
}

// =============================================================================
// NATS event bus
// =============================================================================

#[derive(Clone)]
pub struct NatsBus {
    client: async_nats::Client,
    subscriptions: Arc<Mutex<HashMap<String, broadcast::Sender<(String, Vec<u8>)>>>>,
}

impl NatsBus {
    pub async fn connect(url: &str) -> Result<Self, anyhow::Error> {
        info!(url, "Connecting to NATS");
        let client = async_nats::connect(url).await?;
        info!("NATS connected");
        Ok(Self {
            client,
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn check_health(&self) -> bool {
        self.client.connection_state() == async_nats::connection::State::Connected
    }
}

#[async_trait]
impl EventPublisher for NatsBus {
    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), anyhow::Error> {
        debug!(subject, bytes = payload.len(), "Publishing event (NATS)");
        self.client
            .publish(subject.to_string(), payload.to_vec().into())
            .await?;
        Ok(())
    }
}

#[async_trait]
impl EventSubscriber for NatsBus {
    async fn subscribe(&self, subject: &str) -> Result<EventReceiver, anyhow::Error> {
        debug!(subject, "Subscribing (NATS)");
        let mut subs = self.subscriptions.lock().await;

        let sender = subs
            .entry(subject.to_string())
            .or_insert_with(|| {
                let (tx, _) = broadcast::channel(4096);
                let client = self.client.clone();
                let subj = subject.to_string();
                let tx_clone = tx.clone();

                tokio::spawn(async move {
                    let mut subscriber = match client.subscribe(subj.clone()).await {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!(error = %e, subject = %subj, "Failed to subscribe to NATS");
                            return;
                        }
                    };

                    while let Some(msg) = subscriber.next().await {
                        let _ = tx_clone.send((msg.subject.to_string(), msg.payload.to_vec()));
                    }
                });

                tx
            })
            .clone();

        Ok(EventReceiver {
            inner: sender.subscribe(),
            subject_filter: subject.to_string(),
        })
    }
}

// =============================================================================
// Factory: create the appropriate bus based on config
// =============================================================================

pub enum EventBus {
    InProcess(InProcessBus),
    Nats(NatsBus),
}

impl EventBus {
    pub fn publisher(&self) -> Arc<dyn EventPublisher> {
        match self {
            EventBus::InProcess(bus) => Arc::new(bus.clone()),
            EventBus::Nats(bus) => Arc::new(bus.clone()),
        }
    }

    pub fn subscriber(&self) -> Arc<dyn EventSubscriber> {
        match self {
            EventBus::InProcess(bus) => Arc::new(bus.clone()),
            EventBus::Nats(bus) => Arc::new(bus.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_matching() {
        assert!(subject_matches("controlplane.verdict.fast", "controlplane.verdict.fast"));
        assert!(subject_matches("controlplane.verdict.fast", "controlplane.verdict.*"));
        assert!(subject_matches("controlplane.verdict.shadow", "controlplane.verdict.*"));
        assert!(!subject_matches("controlplane.decision.final", "controlplane.verdict.*"));
    }

    #[tokio::test]
    async fn inprocess_bus_pubsub() {
        let bus = InProcessBus::new();
        let mut receiver = bus.subscribe("test.subject").await.unwrap();

        let payload = b"hello world";
        bus.publish("test.subject", payload).await.unwrap();

        let received = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            receiver.recv(),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(received, payload);
    }

    #[tokio::test]
    async fn inprocess_bus_wildcard_filter() {
        let bus = InProcessBus::new();
        let mut receiver = bus.subscribe("controlplane.verdict.*").await.unwrap();

        bus.publish("controlplane.verdict.fast", b"fast").await.unwrap();
        bus.publish("controlplane.decision.final", b"decision").await.unwrap();
        bus.publish("controlplane.verdict.shadow", b"shadow").await.unwrap();

        let msg1 = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            receiver.recv(),
        ).await.unwrap().unwrap();
        assert_eq!(msg1, b"fast");

        let msg2 = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            receiver.recv(),
        ).await.unwrap().unwrap();
        assert_eq!(msg2, b"shadow");
    }
}
