use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// An internal event emitted by a service to signal something happened.
///
/// Consumers (e.g. the background event router in `awsim`, CloudTrail's
/// event store, EventBridge's `aws.*` auto-emission, AWS Config's
/// configuration-item recorder) subscribe to the bus and perform
/// cross-service fan-out (e.g. SNS to SQS delivery, or recording the
/// call for later `LookupEvents`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalEvent {
    /// Originating service: "s3", "sns", "eventbridge", etc.
    pub source: String,
    /// Fine-grained event type: "s3:ObjectCreated:Put", "sns:Publish",
    /// or the canonical [`API_CALL_EVENT_TYPE`] for every API call the
    /// gateway dispatches.
    pub event_type: String,
    /// AWS region the event occurred in.
    pub region: String,
    /// AWS account ID the event occurred in.
    pub account_id: String,
    /// Event-specific payload (free-form JSON).
    pub detail: serde_json::Value,
}

/// Reserved [`InternalEvent::event_type`] for the per-API-call event
/// the gateway publishes after every dispatched request.
///
/// Subscribers that want CloudTrail-style records key off this value
/// and parse `detail` as [`ApiCallDetail`].
pub const API_CALL_EVENT_TYPE: &str = "awsim:ApiCall";

/// Canonical detail payload for the per-API-call event the gateway
/// publishes after every dispatch (success or error).
///
/// Field naming follows the CloudTrail record shape so subscribers
/// can render it directly without re-mapping. Sensitive request /
/// response fields are scrubbed by the publisher (passwords, KMS
/// ciphertexts, signed-URL bodies); subscribers should treat
/// `request_parameters` and `response_elements` as already redacted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiCallDetail {
    pub event_id: String,
    pub event_source: String,
    pub event_name: String,
    pub event_time_epoch: f64,
    pub source_ip: Option<String>,
    pub user_agent: Option<String>,
    pub user_identity_arn: Option<String>,
    pub user_identity_account: Option<String>,
    pub request_parameters: Option<serde_json::Value>,
    pub response_elements: Option<serde_json::Value>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub http_status: u16,
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

    /// Publish a canonical per-API-call event.
    ///
    /// Convenience wrapper around [`Self::publish`] that builds the
    /// `InternalEvent` envelope with the reserved
    /// [`API_CALL_EVENT_TYPE`] and serialises `detail` for
    /// CloudTrail-style consumers.
    pub fn publish_api_call(&self, region: String, account_id: String, detail: ApiCallDetail) {
        let payload = match serde_json::to_value(&detail) {
            Ok(v) => v,
            Err(_) => return,
        };
        self.publish(InternalEvent {
            source: detail.event_source.clone(),
            event_type: API_CALL_EVENT_TYPE.to_string(),
            region,
            account_id,
            detail: payload,
        });
    }

    /// Subscribe to the event stream.  Each call returns an independent
    /// receiver that starts from the next event published after the call.
    pub fn subscribe(&self) -> broadcast::Receiver<InternalEvent> {
        self.sender.subscribe()
    }

    /// Number of currently subscribed receivers — surfaced by the
    /// `/_awsim/debug/objects` diagnostic to detect leaked SSE
    /// subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
