use std::collections::HashMap;

use dashmap::DashMap;

/// An EventBridge target attached to a rule.
#[derive(Debug, Clone)]
pub struct Target {
    pub id: String,
    pub arn: String,
    pub input: Option<String>,
    pub input_path: Option<String>,
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
}

impl EventBus {
    pub fn new(name: String, arn: String) -> Self {
        Self {
            name,
            arn,
            rules: HashMap::new(),
            tags: HashMap::new(),
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

/// Per-account/region EventBridge state.
#[derive(Debug, Default)]
pub struct EventBridgeState {
    /// bus_name → EventBus
    pub event_buses: DashMap<String, EventBus>,
    /// Recent events for debugging
    pub recent_events: DashMap<String, StoredEvent>,
}
