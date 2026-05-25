pub mod error;
mod operations;
pub mod state;

use async_trait::async_trait;
use awsim_core::events::{API_CALL_EVENT_TYPE, EventBus};
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::{debug, warn};

use state::CloudTrailState;

pub struct CloudTrailService {
    store: AccountRegionStore<CloudTrailState>,
}

impl CloudTrailService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    /// Per-account/region state store. Exposed so the gateway can wire
    /// the event-bus subscriber (see [`spawn_event_subscriber`]).
    pub fn store(&self) -> AccountRegionStore<CloudTrailState> {
        self.store.clone()
    }
}

/// Subscribe to the cross-service [`EventBus`] and append every
/// API-call event to the matching per-account/region CloudTrail event
/// log. Runs until the bus is dropped.
///
/// Call once at startup with the gateway's `state.event_bus` and the
/// `cloudtrail.store()` handle. The returned `JoinHandle` is detached;
/// dropping it does not cancel the task.
pub fn spawn_event_subscriber(
    bus: &EventBus,
    store: AccountRegionStore<CloudTrailState>,
) -> tokio::task::JoinHandle<()> {
    let mut rx = bus.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) if event.event_type == API_CALL_EVENT_TYPE => {
                    if let Ok(detail) =
                        serde_json::from_value::<awsim_core::events::ApiCallDetail>(event.detail)
                    {
                        let state = store.get(&event.account_id, &event.region);
                        state.record_event(detail);
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(skipped, "CloudTrail subscriber lagged; events dropped");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    })
}

impl Default for CloudTrailService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for CloudTrailService {
    fn service_name(&self) -> &str {
        "cloudtrail"
    }

    fn signing_name(&self) -> &str {
        "cloudtrail"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_1
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation = %operation, "CloudTrail operation");
        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            "CreateTrail" => operations::trails::create_trail(&state, &input, ctx),
            "DescribeTrails" => operations::trails::describe_trails(&state, &input, ctx),
            "DeleteTrail" => operations::trails::delete_trail(&state, &input, ctx),
            "UpdateTrail" => operations::trails::update_trail(&state, &input, ctx),
            "StartLogging" => operations::trails::start_logging(&state, &input, ctx),
            "StopLogging" => operations::trails::stop_logging(&state, &input, ctx),
            "GetTrailStatus" => operations::trails::get_trail_status(&state, &input, ctx),
            "GetEventSelectors" => operations::selectors::get_event_selectors(&state, &input, ctx),
            "PutEventSelectors" => operations::selectors::put_event_selectors(&state, &input, ctx),
            "ListTrails" => operations::trails::list_trails(&state, &input, ctx),
            "LookupEvents" => operations::trails::lookup_events(&state, &input, ctx),
            "PutInsightSelectors" => {
                operations::selectors::put_insight_selectors(&state, &input, ctx)
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use awsim_core::events::{ApiCallDetail, EventBus, InternalEvent};
    use serde_json::json;

    fn detail(name: &str) -> ApiCallDetail {
        ApiCallDetail {
            event_id: format!("evt-{name}"),
            event_source: "s3.amazonaws.com".into(),
            event_name: name.into(),
            event_time_epoch: 1.0,
            source_ip: None,
            user_agent: None,
            user_identity_arn: Some("arn:aws:iam::000000000000:user/alice".into()),
            user_identity_account: Some("000000000000".into()),
            request_parameters: None,
            response_elements: None,
            error_code: None,
            error_message: None,
            http_status: 200,
        }
    }

    #[tokio::test]
    async fn subscriber_records_api_call_events_into_state() {
        let svc = CloudTrailService::new();
        let bus = EventBus::new();
        spawn_event_subscriber(&bus, svc.store());

        bus.publish(InternalEvent {
            source: "s3".into(),
            event_type: API_CALL_EVENT_TYPE.into(),
            region: "us-east-1".into(),
            account_id: "000000000000".into(),
            detail: serde_json::to_value(detail("CreateBucket")).unwrap(),
        });

        // Yield until the subscriber task drains the event.
        for _ in 0..50 {
            tokio::task::yield_now().await;
            let state = svc.store().get("000000000000", "us-east-1");
            if !state.event_log.lock().unwrap().is_empty() {
                return;
            }
        }
        panic!("subscriber did not record the event");
    }

    #[tokio::test]
    async fn lookup_events_filters_by_event_name() {
        let svc = CloudTrailService::new();
        let state = svc.store().get("000000000000", "us-east-1");
        state.record_event(detail("CreateBucket"));
        state.record_event(detail("DeleteBucket"));
        state.record_event(detail("PutObject"));

        let ctx = RequestContext::new("cloudtrail", "us-east-1");
        let response = operations::trails::lookup_events(
            &state,
            &json!({
                "LookupAttributes": [
                    {"AttributeKey": "EventName", "AttributeValue": "CreateBucket"}
                ]
            }),
            &ctx,
        )
        .unwrap();
        let events = response["Events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["EventName"], "CreateBucket");
    }

    #[test]
    fn event_log_drops_oldest_at_capacity() {
        let state = CloudTrailState::default();
        for i in 0..(state::EVENT_LOG_CAPACITY + 5) {
            state.record_event(detail(&format!("Op{i}")));
        }
        let log = state.event_log.lock().unwrap();
        assert_eq!(log.len(), state::EVENT_LOG_CAPACITY);
        // Newest at front.
        assert_eq!(
            log.front().unwrap().event_name,
            format!("Op{}", state::EVENT_LOG_CAPACITY + 4)
        );
    }
}
