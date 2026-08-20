use reqwest::Client;
use serde::Serialize;
use tracing::{error, info};

use crate::types::NotificationPayload;

/// Slack webhook notification sender.
/// Posts rich Block Kit messages to a configured Slack webhook URL.
pub struct SlackNotifier {
    client: Client,
    webhook_url: Option<String>,
    dashboard_base_url: String,
}

#[derive(Serialize)]
struct SlackMessage {
    blocks: Vec<SlackBlock>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum SlackBlock {
    #[serde(rename = "header")]
    Header { text: SlackText },
    #[serde(rename = "section")]
    Section { text: SlackText },
    #[serde(rename = "divider")]
    Divider,
    #[serde(rename = "context")]
    Context { elements: Vec<SlackText> },
}

#[derive(Serialize)]
struct SlackText {
    #[serde(rename = "type")]
    text_type: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    emoji: Option<bool>,
}

impl SlackText {
    fn plain(text: impl Into<String>) -> Self {
        Self { text_type: "plain_text".into(), text: text.into(), emoji: Some(true) }
    }

    fn mrkdwn(text: impl Into<String>) -> Self {
        Self { text_type: "mrkdwn".into(), text: text.into(), emoji: None }
    }
}

impl SlackNotifier {
    pub fn new() -> Self {
        let webhook_url = std::env::var("SLACK_WEBHOOK_URL").ok();
        let dashboard_base_url = std::env::var("DASHBOARD_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:3000".to_string());

        Self {
            client: Client::new(),
            webhook_url,
            dashboard_base_url,
        }
    }

    pub fn is_configured(&self) -> bool {
        self.webhook_url.is_some()
    }

    pub async fn send(&self, payload: &NotificationPayload) -> Result<(), String> {
        let webhook_url = match &self.webhook_url {
            Some(url) => url,
            None => {
                info!(
                    call_id = %payload.call_id,
                    "Slack not configured, skipping notification"
                );
                return Ok(());
            }
        };

        let message = self.build_message(payload);

        match self.client.post(webhook_url).json(&message).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    info!(call_id = %payload.call_id, "Slack notification sent");
                    Ok(())
                } else {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    error!(call_id = %payload.call_id, %status, body, "Slack webhook failed");
                    Err(format!("Slack returned {status}: {body}"))
                }
            }
            Err(e) => {
                error!(call_id = %payload.call_id, error = %e, "Slack request failed");
                Err(format!("Request error: {e}"))
            }
        }
    }

    fn build_message(&self, payload: &NotificationPayload) -> SlackMessage {
        let emoji = match payload.outcome.as_str() {
            "block" => "🚫",
            "escalate" => "⚠️",
            _ => "ℹ️",
        };

        let title = format!("{emoji} AI Response {}", payload.outcome.to_uppercase());

        let detail = format!(
            "*App:* {}\n*Axis:* {}\n*Confidence:* {:.0}%\n*Reason:* {}",
            payload.app_name.as_deref().unwrap_or("Unknown"),
            payload.axis,
            payload.confidence * 100.0,
            payload.reason,
        );

        let dashboard_link = format!(
            "<{}/stream?call_id={}|View in Dashboard>",
            self.dashboard_base_url, payload.call_id
        );

        SlackMessage {
            blocks: vec![
                SlackBlock::Header { text: SlackText::plain(title) },
                SlackBlock::Section { text: SlackText::mrkdwn(detail) },
                SlackBlock::Divider,
                SlackBlock::Context {
                    elements: vec![
                        SlackText::mrkdwn(dashboard_link),
                        SlackText::mrkdwn(format!("Call ID: `{}`", payload.call_id)),
                    ],
                },
            ],
        }
    }
}

impl Default for SlackNotifier {
    fn default() -> Self {
        Self::new()
    }
}
