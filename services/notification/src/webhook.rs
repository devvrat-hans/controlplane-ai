use reqwest::Client;
use serde::Serialize;
use tracing::{error, info};

use crate::types::NotificationPayload;

/// Generic webhook notification sender.
/// Posts JSON payloads to one or more configured webhook URLs.
pub struct WebhookNotifier {
    client: Client,
    endpoints: Vec<WebhookEndpoint>,
}

#[derive(Clone)]
pub struct WebhookEndpoint {
    pub url: String,
    pub secret: Option<String>,
}

#[derive(Serialize)]
struct WebhookBody {
    event: String,
    call_id: String,
    app_id: String,
    app_name: Option<String>,
    outcome: String,
    axis: String,
    confidence: f32,
    reason: String,
    timestamp: String,
}

impl WebhookNotifier {
    pub fn new() -> Self {
        let endpoints = Self::load_endpoints_from_env();

        Self {
            client: Client::new(),
            endpoints,
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.endpoints.is_empty()
    }

    fn load_endpoints_from_env() -> Vec<WebhookEndpoint> {
        let mut endpoints = Vec::new();

        // Support up to 5 webhook endpoints via env vars
        for i in 0..5 {
            let url_key = if i == 0 {
                "WEBHOOK_URL".to_string()
            } else {
                format!("WEBHOOK_URL_{i}")
            };

            if let Ok(url) = std::env::var(&url_key) {
                let secret_key = if i == 0 {
                    "WEBHOOK_SECRET".to_string()
                } else {
                    format!("WEBHOOK_SECRET_{i}")
                };

                endpoints.push(WebhookEndpoint {
                    url,
                    secret: std::env::var(&secret_key).ok(),
                });
            }
        }

        endpoints
    }

    pub async fn send(&self, payload: &NotificationPayload) -> Result<(), String> {
        if self.endpoints.is_empty() {
            info!(call_id = %payload.call_id, "No webhooks configured, skipping");
            return Ok(());
        }

        let body = WebhookBody {
            event: format!("controlplane.{}", payload.outcome),
            call_id: payload.call_id.to_string(),
            app_id: payload.app_id.to_string(),
            app_name: payload.app_name.clone(),
            outcome: payload.outcome.clone(),
            axis: payload.axis.clone(),
            confidence: payload.confidence,
            reason: payload.reason.clone(),
            timestamp: payload.timestamp.to_rfc3339(),
        };

        let mut errors = Vec::new();

        for endpoint in &self.endpoints {
            let mut request = self.client.post(&endpoint.url).json(&body);

            if let Some(secret) = &endpoint.secret {
                request = request.header("X-ControlPlane-Secret", secret.as_str());
            }

            match request.send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        info!(url = %endpoint.url, call_id = %payload.call_id, "Webhook delivered");
                    } else {
                        let status = resp.status();
                        let msg = format!("Webhook {} returned {}", endpoint.url, status);
                        error!("{msg}");
                        errors.push(msg);
                    }
                }
                Err(e) => {
                    let msg = format!("Webhook {} failed: {}", endpoint.url, e);
                    error!("{msg}");
                    errors.push(msg);
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

impl Default for WebhookNotifier {
    fn default() -> Self {
        Self::new()
    }
}
