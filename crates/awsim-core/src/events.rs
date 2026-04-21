use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// An internal event emitted by a service to signal something happened.
///
/// Consumers (e.g. the background event router in `awsim`) subscribe to the
/// bus and perform cross-service fan-out (e.g. SNS → SQS delivery).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalEvent {
    /// Originating service: "s3", "sns", "eventbridge", etc.
    pub source: String,
    /// Fine-grained event type: "s3:ObjectCreated:Put", "sns:Publish", etc.
    pub event_type: String,
    /// AWS region the event occurred in.
    pub region: String,
    /// AWS account ID the event occurred in.
    pub account_id: String,
    /// Event-specific payload (free-form JSON).
    pub detail: serde_json::Value,
}

/// A cheap-to-clone handle to the shared broadcast channel used as an
/// internal event bus between services.
#[derive(Clone, Debug)]
pub struct EventBus {
    sender: broadcast::Sender<InternalEvent>,
}

impl EventBus {
    /// Create a new event bus with a buffer of 1 024 events.
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1024);
        Self { sender }
    }

    /// Publish an event to all current subscribers.
    ///
    /// Silently drops the event if there are no active subscribers (the
    /// broadcast channel returns `SendError` in that case, which we ignore).
    pub fn publish(&self, event: InternalEvent) {
        let _ = self.sender.send(event);
    }

    /// Subscribe to the event stream.  Each call returns an independent
    /// receiver that starts from the next event published after the call.
    pub fn subscribe(&self) -> broadcast::Receiver<InternalEvent> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
