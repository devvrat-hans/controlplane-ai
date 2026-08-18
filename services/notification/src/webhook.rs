/// Generic webhook notification sender.
pub struct WebhookNotifier;

impl WebhookNotifier {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebhookNotifier {
    fn default() -> Self {
        Self::new()
    }
}
