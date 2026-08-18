/// Notification worker: subscribes to decision events, dispatches alerts.
pub struct NotificationWorker;

impl NotificationWorker {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NotificationWorker {
    fn default() -> Self {
        Self::new()
    }
}
