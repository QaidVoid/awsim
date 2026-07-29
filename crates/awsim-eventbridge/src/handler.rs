use std::sync::Arc;

use awsim_core::{
    AccountRegionStore, AwsError, PrincipalLookup, Protocol, RequestContext, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use crate::operations::{
    api_destinations, archives, buses, connections, event_sources, events, replays, rules, tags,
    targets,
};
use crate::state::EventBridgeState;
use crate::util::now_epoch;

/// The EventBridge service handler.
pub struct EventBridgeService {
    store: AccountRegionStore<EventBridgeState>,
    /// IAM principal lookup used to validate cross-account `RoleArn`s
    /// at PutTargets. `None` keeps PutTargets working in standalone
    /// test setups that don't wire IAM state.
    iam_lookup: Option<Arc<dyn PrincipalLookup>>,
}

impl EventBridgeService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
            iam_lookup: None,
        }
    }

    /// Plug in the IAM principal lookup so PutTargets can verify that
    /// a cross-account `RoleArn` actually points at an existing role.
    pub fn with_iam_lookup(mut self, lookup: Arc<dyn PrincipalLookup>) -> Self {
        self.iam_lookup = Some(lookup);
        self
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
            "PutTargets" => targets::put_targets(&state, &input, ctx, self.iam_lookup.as_deref()),
            "RemoveTargets" => targets::remove_targets(&state, &input, ctx),
            "ListTargetsByRule" => targets::list_targets_by_rule(&state, &input, ctx),

            // Events
            "PutEvents" => events::put_events(&state, &input, ctx),
            "TestEventPattern" => events::test_event_pattern(&input),

            // Resource policy
            "PutPermission" => buses::put_permission(&state, &input, ctx),
            "RemovePermission" => buses::remove_permission(&state, &input, ctx),

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

    /// Expire archives whose retention window has elapsed. Computes the
    /// current epoch once and sweeps every account/region state. The
    /// sweep is absolute-time based and idempotent, so repeated ticks
    /// (or missed ones) converge on the same result.
    async fn tick(&self) {
        let now = now_epoch();
        for (_, state) in self.store.iter_all() {
            state.sweep_expired_archives(now);
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut buckets = Vec::new();
        for ((account_id, region), state) in self.store.iter_all() {
            buckets.push(EbBucketSnapshot {
                account_id,
                region,
                event_buses: state
                    .event_buses
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
                archives: state
                    .archives
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
                connections: state
                    .connections
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
                api_destinations: state
                    .api_destinations
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
                replays: state
                    .replays
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
            });
        }
        serde_json::to_vec(&EbSnapshot { buckets }).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: EbSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        for bucket in snapshot.buckets {
            let state = self.store.get(&bucket.account_id, &bucket.region);
            state.event_buses.clear();
            state.archives.clear();
            state.connections.clear();
            state.api_destinations.clear();
            state.replays.clear();
            for (k, v) in bucket.event_buses {
                state.event_buses.insert(k, v);
            }
            for (k, v) in bucket.archives {
                state.archives.insert(k, v);
            }
            for (k, v) in bucket.connections {
                state.connections.insert(k, v);
            }
            for (k, v) in bucket.api_destinations {
                state.api_destinations.insert(k, v);
            }
            for (k, v) in bucket.replays {
                state.replays.insert(k, v);
            }
        }
        Ok(())
    }
}

/// One (account, region) pair's EventBridge state on disk.
///
/// `recent_events` is deliberately excluded: it is a debugging ring, not
/// durable state, and persisting it would grow snapshots without giving
/// anything back on restore.
#[derive(serde::Serialize, serde::Deserialize)]
struct EbBucketSnapshot {
    account_id: String,
    region: String,
    event_buses: Vec<(String, crate::state::EventBus)>,
    archives: Vec<(String, crate::state::Archive)>,
    connections: Vec<(String, crate::state::Connection)>,
    api_destinations: Vec<(String, crate::state::ApiDestination)>,
    replays: Vec<(String, crate::state::Replay)>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct EbSnapshot {
    buckets: Vec<EbBucketSnapshot>,
}
