use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize)]
pub struct RequestEvent {
    pub id: String,
    pub ts: f64,
    pub method: String,
    pub path: String,
    pub service: String,
    pub operation: Option<String>,
    pub account_id: String,
    pub region: String,
    pub principal_arn: Option<String>,
    pub status_code: u16,
    pub duration_ms: f64,
    pub request_size: u64,
    pub response_size: u64,
    pub error_code: Option<String>,
    /// Lambda-style memory size in MB, populated when the responding
    /// service sets the `X-Awsim-Memory-MB` header on its response.
    /// Used by the billing meter for accurate GB-second compute cost
    /// (otherwise it falls back to the 128 MB minimum).
    pub memory_mb: Option<u32>,
    /// Number of state transitions executed by the responding service
    /// for this request. Step Functions emits this so the meter can
    /// charge per-transition (the actual AWS billing unit) instead of
    /// per-StartExecution call. None for non-stateful services.
    pub state_transitions: Option<u32>,
    /// Number of characters in the request payload. Polly /
    /// Comprehend / Translate emit this so the meter can charge
    /// per-character (the AWS billing unit for these services). None
    /// for services that don't bill per character.
    pub character_count: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct RequestEventBus {
    sender: broadcast::Sender<RequestEvent>,
}

impl RequestEventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self { sender }
    }

    pub fn publish(&self, event: RequestEvent) {
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RequestEvent> {
        self.sender.subscribe()
    }

    pub fn sender(&self) -> &broadcast::Sender<RequestEvent> {
        &self.sender
    }
}

impl Default for RequestEventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn broadcast_round_trip() {
        let bus = RequestEventBus::new();
        let mut rx = bus.subscribe();
        let event = RequestEvent {
            id: "req-1".to_string(),
            ts: 1735041600.123,
            method: "POST".to_string(),
            path: "/".to_string(),
            service: "s3".to_string(),
            operation: Some("PutObject".to_string()),
            account_id: "000000000000".to_string(),
            region: "us-east-1".to_string(),
            principal_arn: None,
            status_code: 200,
            duration_ms: 12.5,
            request_size: 1024,
            response_size: 256,
            error_code: None,
            memory_mb: None,
            state_transitions: None,
            character_count: None,
        };
        bus.publish(event.clone());
        let received = rx.recv().await.expect("receive event");
        assert_eq!(received.id, event.id);
        assert_eq!(received.service, "s3");
        assert_eq!(received.operation.as_deref(), Some("PutObject"));
        assert_eq!(received.status_code, 200);
    }
}
