/// Slack webhook notification sender.
pub struct SlackNotifier;

impl SlackNotifier {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SlackNotifier {
    fn default() -> Self {
        Self::new()
    }
}
