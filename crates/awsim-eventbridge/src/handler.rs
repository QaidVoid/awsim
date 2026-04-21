use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{buses, events, rules, tags, targets};
use crate::state::EventBridgeState;

/// The EventBridge service handler.
pub struct EventBridgeService {
    store: AccountRegionStore<EventBridgeState>,
}

impl EventBridgeService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for EventBridgeService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for EventBridgeService {
    fn service_name(&self) -> &str {
        "events"
    }

    fn signing_name(&self) -> &str {
        "events"
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
        debug!(operation = %operation, "EventBridge operation");

        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            // Event Buses
            "CreateEventBus" => buses::create_event_bus(&state, &input, ctx),
            "DeleteEventBus" => buses::delete_event_bus(&state, &input, ctx),
            "DescribeEventBus" => buses::describe_event_bus(&state, &input, ctx),
            "ListEventBuses" => buses::list_event_buses(&state, &input, ctx),

            // Rules
            "PutRule" => rules::put_rule(&state, &input, ctx),
            "DeleteRule" => rules::delete_rule(&state, &input, ctx),
            "DescribeRule" => rules::describe_rule(&state, &input, ctx),
            "ListRules" => rules::list_rules(&state, &input, ctx),
            "EnableRule" => rules::enable_rule(&state, &input, ctx),
            "DisableRule" => rules::disable_rule(&state, &input, ctx),

            // Targets
            "PutTargets" => targets::put_targets(&state, &input, ctx),
            "RemoveTargets" => targets::remove_targets(&state, &input, ctx),
            "ListTargetsByRule" => targets::list_targets_by_rule(&state, &input, ctx),

            // Events
            "PutEvents" => events::put_events(&state, &input, ctx),

            // Tags
            "TagResource" => tags::tag_resource(&state, &input, ctx),
            "UntagResource" => tags::untag_resource(&state, &input, ctx),
            "ListTagsForResource" => tags::list_tags_for_resource(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
