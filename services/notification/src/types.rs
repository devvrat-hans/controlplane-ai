use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Unified notification payload sent to all channels (Slack, webhook, etc.)
#[derive(Debug, Clone)]
pub struct NotificationPayload {
    pub call_id: Uuid,
    pub app_id: Uuid,
    pub app_name: Option<String>,
    pub outcome: String,
    pub axis: String,
    pub confidence: f32,
    pub reason: String,
    pub timestamp: DateTime<Utc>,
}
