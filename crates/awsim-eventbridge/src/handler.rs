use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{
    api_destinations, archives, buses, connections, event_sources, events, replays, rules, tags,
    targets,
};
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
            "TestEventPattern" => events::test_event_pattern(&input),

            // Tags
            "TagResource" => tags::tag_resource(&state, &input, ctx),
            "UntagResource" => tags::untag_resource(&state, &input, ctx),
            "ListTagsForResource" => tags::list_tags_for_resource(&state, &input, ctx),

            // Event Sources (stubs)
            "DescribeEventSource" => event_sources::describe_event_source(&state, &input, ctx),
            "ListEventSources" => event_sources::list_event_sources(&state, &input, ctx),
            "PutPartnerEventSource" => event_sources::put_partner_event_source(&state, &input, ctx),

            // Archives
            "CreateArchive" => archives::create_archive(&state, &input, ctx),
            "DeleteArchive" => archives::delete_archive(&state, &input, ctx),
            "DescribeArchive" => archives::describe_archive(&state, &input, ctx),
            "ListArchives" => archives::list_archives(&state, &input, ctx),

            // Connections
            "CreateConnection" => connections::create_connection(&state, &input, ctx),
            "DeleteConnection" => connections::delete_connection(&state, &input, ctx),
            "DescribeConnection" => connections::describe_connection(&state, &input, ctx),
            "ListConnections" => connections::list_connections(&state, &input, ctx),

            // API Destinations
            "CreateApiDestination" => api_destinations::create_api_destination(&state, &input, ctx),
            "DeleteApiDestination" => api_destinations::delete_api_destination(&state, &input, ctx),
            "DescribeApiDestination" => {
                api_destinations::describe_api_destination(&state, &input, ctx)
            }
            "ListApiDestinations" => api_destinations::list_api_destinations(&state, &input, ctx),

            // Replays
            "StartReplay" => replays::start_replay(&state, &input, ctx),
            "CancelReplay" => replays::cancel_replay(&state, &input, ctx),
            "DescribeReplay" => replays::describe_replay(&state, &input, ctx),
            "ListReplays" => replays::list_replays(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
