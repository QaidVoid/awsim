use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use serde_json::Value;

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// A single log event.
#[derive(Debug, Clone)]
pub struct LogEvent {
    pub timestamp: u64,
    pub message: String,
    pub ingestion_time: u64,
}

/// A log stream within a log group.
#[derive(Debug)]
pub struct LogStream {
    pub name: String,
    pub arn: String,
    pub creation_time: u64,
    pub first_event_timestamp: Option<u64>,
    pub last_event_timestamp: Option<u64>,
    pub last_ingestion_time: Option<u64>,
    pub upload_sequence_token: Arc<AtomicU64>,
    pub events: RwLock<Vec<LogEvent>>,
}

impl LogStream {
    pub fn new(name: String, arn: String) -> Self {
        Self {
            name,
            arn,
            creation_time: now_millis(),
            first_event_timestamp: None,
            last_event_timestamp: None,
            last_ingestion_time: None,
            upload_sequence_token: Arc::new(AtomicU64::new(1)),
            events: RwLock::new(Vec::new()),
        }
    }

    pub fn next_sequence_token(&self) -> u64 {
        self.upload_sequence_token.fetch_add(1, Ordering::SeqCst)
    }
}

/// A log group.
#[derive(Debug)]
pub struct LogGroup {
    pub name: String,
    pub arn: String,
    pub creation_time: u64,
    pub retention_in_days: Option<u32>,
    pub stored_bytes: u64,
    pub tags: HashMap<String, String>,
    pub streams: DashMap<String, LogStream>,
}

impl LogGroup {
    pub fn new(name: String, arn: String, tags: HashMap<String, String>) -> Self {
        Self {
            name,
            arn,
            creation_time: now_millis(),
            retention_in_days: None,
            stored_bytes: 0,
            tags,
            streams: DashMap::new(),
        }
    }
}

/// A subscription filter on a log group.
#[derive(Debug, Clone)]
pub struct SubscriptionFilter {
    pub filter_name: String,
    pub log_group_name: String,
    pub filter_pattern: String,
    pub destination_arn: String,
    pub creation_time: u64,
}

/// A metric filter on a log group.
#[derive(Debug, Clone)]
pub struct MetricFilter {
    pub filter_name: String,
    pub log_group_name: String,
    pub filter_pattern: String,
    pub metric_transformations: Vec<Value>,
    pub creation_time: u64,
}

/// A saved CloudWatch Insights query definition.
#[derive(Debug, Clone)]
pub struct QueryDefinition {
    pub query_definition_id: String,
    pub name: String,
    pub query_string: String,
    pub log_group_names: Vec<String>,
}

/// A running or completed Insights query.
#[derive(Debug, Clone)]
pub struct InsightsQuery {
    pub query_id: String,
    pub status: String,
}

/// Per-account/region CloudWatch Logs state.
#[derive(Debug, Default)]
pub struct LogsState {
    /// logGroupName → LogGroup
    pub log_groups: DashMap<String, LogGroup>,
    /// (logGroupName, filterName) → SubscriptionFilter
    pub subscription_filters: DashMap<(String, String), SubscriptionFilter>,
    /// (logGroupName, filterName) → MetricFilter
    pub metric_filters: DashMap<(String, String), MetricFilter>,
    /// queryDefinitionId → QueryDefinition
    pub query_definitions: DashMap<String, QueryDefinition>,
    /// queryId → InsightsQuery
    pub insights_queries: DashMap<String, InsightsQuery>,
}
