use std::collections::HashMap;

use dashmap::DashMap;

/// An EventBridge target attached to a rule.
#[derive(Debug, Clone)]
pub struct Target {
    pub id: String,
    pub arn: String,
    pub input: Option<String>,
    pub input_path: Option<String>,
    /// Optional InputTransformer (`{InputPathsMap, InputTemplate}`).
    /// Mutually exclusive with `input` and `input_path`. Stored on
    /// PutTargets but not yet applied at fan-out — see NEW_PLAN §10.4.
    #[allow(dead_code)]
    pub input_transformer: Option<InputTransformer>,
    /// AWS Batch-specific submission overrides. Stored verbatim and
    /// echoed back from ListTargetsByRule so SDKs that round-trip
    /// target configuration see the same shape they sent.
    pub batch_parameters: Option<serde_json::Value>,
    /// SQS queue ARN where EventBridge would publish events the target
    /// failed to deliver. Validated for shape at PutTargets; the actual
    /// delivery path remains stubby (see NEW_PLAN §10.4).
    pub dead_letter_arn: Option<String>,
    /// Retry policy. `(MaximumEventAgeInSeconds, MaximumRetryAttempts)`
    /// bounded by AWS at 60..=86400 and 0..=185 respectively.
    pub retry_policy: Option<(u32, u32)>,
    /// IAM role EventBridge assumes when invoking this target. AWS
    /// validates the shape at PutTargets and, for cross-account
    /// ARNs, that the role actually exists in the target account.
    pub role_arn: Option<String>,
}

/// EventBridge `InputTransformer` shape — stored verbatim and applied
/// at fan-out time. AWS requires `InputTemplate`; `InputPathsMap` is
/// optional but every key it declares must appear at least once in
/// the template as `<key>`.
///
/// The fields are populated and validated on PutTargets but not yet
/// consulted during fan-out (the EventBridge target invocation path
/// is itself stubby — see NEW_PLAN §10.4). The `allow(dead_code)`
/// keeps `#![deny(warnings)]` happy until that work lands.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct InputTransformer {
    pub input_paths_map: HashMap<String, String>,
    pub input_template: String,
}

/// An EventBridge rule on a bus.
#[derive(Debug, Clone)]
pub struct Rule {
    pub name: String,
    pub arn: String,
    pub event_bus_name: String,
    pub event_pattern: Option<String>,
    pub schedule_expression: Option<String>,
    pub state: String, // "ENABLED" or "DISABLED"
    pub description: String,
    pub targets: Vec<Target>,
}

/// A single EventBridge event bus.
#[derive(Debug)]
pub struct EventBus {
    pub name: String,
    pub arn: String,
    /// rule_name → Rule
    pub rules: HashMap<String, Rule>,
    pub tags: HashMap<String, String>,
    /// Resource policy attached to the bus. Authorizes cross-account
    /// PutEvents callers — AWS denies cross-account writes when no
    /// statement grants `events:PutEvents` to the calling principal.
    pub policy: Option<String>,
}

impl EventBus {
    pub fn new(name: String, arn: String) -> Self {
        Self {
            name,
            arn,
            rules: HashMap::new(),
            tags: HashMap::new(),
            policy: None,
        }
    }
}

/// A stored event (for debugging/inspection; cross-service delivery is future work).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StoredEvent {
    pub event_id: String,
    pub source: String,
    pub detail_type: String,
    pub detail: String,
    pub event_bus_name: String,
    pub resources: Vec<String>,
    /// Names of rules that matched this event.
    pub matched_rules: Vec<String>,
}

/// An EventBridge event archive.
#[derive(Debug, Clone)]
pub struct Archive {
    pub name: String,
    pub arn: String,
    pub event_source_arn: String,
    pub description: String,
    pub event_pattern: Option<String>,
    pub retention_days: u32,
    pub state: String,
    pub creation_time: String,
}

/// An API destination connection (auth config).
#[derive(Debug, Clone)]
pub struct Connection {
    pub name: String,
    pub arn: String,
    pub description: String,
    pub auth_type: String,
    pub auth_parameters: serde_json::Value,
    pub state: String,
    pub creation_time: String,
    pub last_modified_time: String,
}

/// An HTTP API destination.
#[derive(Debug, Clone)]
pub struct ApiDestination {
    pub name: String,
    pub arn: String,
    pub description: String,
    pub connection_arn: String,
    pub invocation_endpoint: String,
    pub http_method: String,
    pub invocation_rate_limit_per_second: u32,
    pub state: String,
    pub creation_time: String,
    pub last_modified_time: String,
}

/// An event replay.
#[derive(Debug, Clone)]
pub struct Replay {
    pub name: String,
    pub arn: String,
    pub description: String,
    pub event_source_arn: String,
    pub destination: serde_json::Value,
    pub event_start_time: String,
    pub event_end_time: String,
    pub state: String,
    pub state_reason: Option<String>,
    pub replay_start_time: Option<String>,
    pub replay_end_time: Option<String>,
}

/// Per-account/region EventBridge state.
#[derive(Debug, Default)]
pub struct EventBridgeState {
    /// bus_name → EventBus
    pub event_buses: DashMap<String, EventBus>,
    /// Recent events for debugging
    pub recent_events: DashMap<String, StoredEvent>,
    /// archive_name → Archive
    pub archives: DashMap<String, Archive>,
    /// connection_name → Connection
    pub connections: DashMap<String, Connection>,
    /// api_destination_name → ApiDestination
    pub api_destinations: DashMap<String, ApiDestination>,
    /// replay_name → Replay
    pub replays: DashMap<String, Replay>,
}
