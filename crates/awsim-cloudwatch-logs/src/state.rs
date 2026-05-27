use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{BodyStore, Snapshottable};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// A log stream within a log group. Events themselves live in the
/// shared `SqliteStore` (off the `LogsState`); the stream struct
/// only carries its name + bookkeeping metadata.
#[derive(Debug)]
pub struct LogStream {
    pub name: String,
    pub arn: String,
    pub creation_time: u64,
    pub first_event_timestamp: Option<u64>,
    pub last_event_timestamp: Option<u64>,
    pub last_ingestion_time: Option<u64>,
    pub upload_sequence_token: Arc<AtomicU64>,
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
    /// `STANDARD` or `INFREQUENT_ACCESS`; surfaced in DescribeLogGroups.
    pub log_group_class: String,
    /// AWS exposes this via PutDataProtectionPolicy / PutLogGroupClass.
    /// When `ENABLED`, DeleteLogGroup must refuse until a separate
    /// PutDataProtectionPolicy turns it back off.
    pub deletion_protection: String,
    /// KMS key used to encrypt log events at rest. Validated when the
    /// group is created (`arn:aws:kms:` prefix) and surfaced by
    /// DescribeLogGroups for downstream tooling.
    pub kms_key_id: Option<String>,
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
            log_group_class: "STANDARD".to_string(),
            deletion_protection: "DISABLED".to_string(),
            kms_key_id: None,
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
    pub body_store: OnceLock<Arc<BodyStore>>,
    /// Shared SQLite store backing all log events. Set by the
    /// service on first `get_state` so per-region stores all see the
    /// same database.
    pub sqlite: OnceLock<Arc<crate::SqliteStore>>,
}

impl LogsState {
    pub fn body_store(&self) -> Option<&Arc<BodyStore>> {
        self.body_store.get()
    }

    pub fn set_body_store(&self, store: Arc<BodyStore>) {
        let _ = self.body_store.set(store);
    }

    pub fn sqlite(&self) -> Option<&Arc<crate::SqliteStore>> {
        self.sqlite.get()
    }

    pub fn set_sqlite(&self, store: Arc<crate::SqliteStore>) {
        let _ = self.sqlite.set(store);
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LogStreamSnapshot {
    pub name: String,
    pub arn: String,
    pub creation_time: u64,
    pub first_event_timestamp: Option<u64>,
    pub last_event_timestamp: Option<u64>,
    pub last_ingestion_time: Option<u64>,
    pub upload_sequence_token: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LogGroupSnapshot {
    pub name: String,
    pub arn: String,
    pub creation_time: u64,
    pub retention_in_days: Option<u32>,
    pub stored_bytes: u64,
    pub tags: HashMap<String, String>,
    pub streams: Vec<LogStreamSnapshot>,
    #[serde(default = "default_log_group_class")]
    pub log_group_class: String,
    #[serde(default = "default_deletion_protection")]
    pub deletion_protection: String,
    #[serde(default)]
    pub kms_key_id: Option<String>,
}

fn default_log_group_class() -> String {
    "STANDARD".to_string()
}

fn default_deletion_protection() -> String {
    "DISABLED".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LogsRegionSnapshot {
    pub account_id: String,
    pub region: String,
    pub log_groups: Vec<LogGroupSnapshot>,
    #[serde(default)]
    pub subscription_filters: Vec<SubscriptionFilter>,
    #[serde(default)]
    pub metric_filters: Vec<MetricFilter>,
    #[serde(default)]
    pub query_definitions: Vec<QueryDefinition>,
    #[serde(default)]
    pub insights_queries: Vec<InsightsQuery>,
}

impl Serialize for SubscriptionFilter {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = ser.serialize_struct("SubscriptionFilter", 5)?;
        s.serialize_field("filter_name", &self.filter_name)?;
        s.serialize_field("log_group_name", &self.log_group_name)?;
        s.serialize_field("filter_pattern", &self.filter_pattern)?;
        s.serialize_field("destination_arn", &self.destination_arn)?;
        s.serialize_field("creation_time", &self.creation_time)?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for SubscriptionFilter {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            filter_name: String,
            log_group_name: String,
            filter_pattern: String,
            destination_arn: String,
            creation_time: u64,
        }
        let w = Wire::deserialize(de)?;
        Ok(SubscriptionFilter {
            filter_name: w.filter_name,
            log_group_name: w.log_group_name,
            filter_pattern: w.filter_pattern,
            destination_arn: w.destination_arn,
            creation_time: w.creation_time,
        })
    }
}

impl Serialize for MetricFilter {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = ser.serialize_struct("MetricFilter", 5)?;
        s.serialize_field("filter_name", &self.filter_name)?;
        s.serialize_field("log_group_name", &self.log_group_name)?;
        s.serialize_field("filter_pattern", &self.filter_pattern)?;
        s.serialize_field("metric_transformations", &self.metric_transformations)?;
        s.serialize_field("creation_time", &self.creation_time)?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for MetricFilter {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            filter_name: String,
            log_group_name: String,
            filter_pattern: String,
            metric_transformations: Vec<Value>,
            creation_time: u64,
        }
        let w = Wire::deserialize(de)?;
        Ok(MetricFilter {
            filter_name: w.filter_name,
            log_group_name: w.log_group_name,
            filter_pattern: w.filter_pattern,
            metric_transformations: w.metric_transformations,
            creation_time: w.creation_time,
        })
    }
}

impl Serialize for QueryDefinition {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = ser.serialize_struct("QueryDefinition", 4)?;
        s.serialize_field("query_definition_id", &self.query_definition_id)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("query_string", &self.query_string)?;
        s.serialize_field("log_group_names", &self.log_group_names)?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for QueryDefinition {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            query_definition_id: String,
            name: String,
            query_string: String,
            log_group_names: Vec<String>,
        }
        let w = Wire::deserialize(de)?;
        Ok(QueryDefinition {
            query_definition_id: w.query_definition_id,
            name: w.name,
            query_string: w.query_string,
            log_group_names: w.log_group_names,
        })
    }
}

impl Serialize for InsightsQuery {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = ser.serialize_struct("InsightsQuery", 2)?;
        s.serialize_field("query_id", &self.query_id)?;
        s.serialize_field("status", &self.status)?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for InsightsQuery {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            query_id: String,
            status: String,
        }
        let w = Wire::deserialize(de)?;
        Ok(InsightsQuery {
            query_id: w.query_id,
            status: w.status,
        })
    }
}

impl Snapshottable for LogsState {
    type Snapshot = LogsRegionSnapshot;

    fn to_snapshot(&self, account_id: &str, region: &str) -> Self::Snapshot {
        let log_groups: Vec<LogGroupSnapshot> = self
            .log_groups
            .iter()
            .map(|entry| {
                let g = entry.value();
                let streams: Vec<LogStreamSnapshot> = g
                    .streams
                    .iter()
                    .map(|s_entry| {
                        let s = s_entry.value();
                        LogStreamSnapshot {
                            name: s.name.clone(),
                            arn: s.arn.clone(),
                            creation_time: s.creation_time,
                            first_event_timestamp: s.first_event_timestamp,
                            last_event_timestamp: s.last_event_timestamp,
                            last_ingestion_time: s.last_ingestion_time,
                            upload_sequence_token: s.upload_sequence_token.load(Ordering::SeqCst),
                        }
                    })
                    .collect();
                LogGroupSnapshot {
                    name: g.name.clone(),
                    arn: g.arn.clone(),
                    creation_time: g.creation_time,
                    retention_in_days: g.retention_in_days,
                    stored_bytes: g.stored_bytes,
                    tags: g.tags.clone(),
                    streams,
                    log_group_class: g.log_group_class.clone(),
                    deletion_protection: g.deletion_protection.clone(),
                    kms_key_id: g.kms_key_id.clone(),
                }
            })
            .collect();

        let subscription_filters: Vec<SubscriptionFilter> = self
            .subscription_filters
            .iter()
            .map(|e| e.value().clone())
            .collect();
        let metric_filters: Vec<MetricFilter> = self
            .metric_filters
            .iter()
            .map(|e| e.value().clone())
            .collect();
        let query_definitions: Vec<QueryDefinition> = self
            .query_definitions
            .iter()
            .map(|e| e.value().clone())
            .collect();
        let insights_queries: Vec<InsightsQuery> = self
            .insights_queries
            .iter()
            .map(|e| e.value().clone())
            .collect();

        LogsRegionSnapshot {
            account_id: account_id.to_string(),
            region: region.to_string(),
            log_groups,
            subscription_filters,
            metric_filters,
            query_definitions,
            insights_queries,
        }
    }

    fn from_snapshot(snapshot: Self::Snapshot) -> (String, String, Self) {
        let state = LogsState::default();
        for gs in snapshot.log_groups {
            let group = LogGroup {
                name: gs.name.clone(),
                arn: gs.arn,
                creation_time: gs.creation_time,
                retention_in_days: gs.retention_in_days,
                stored_bytes: gs.stored_bytes,
                tags: gs.tags,
                streams: DashMap::new(),
                log_group_class: gs.log_group_class,
                deletion_protection: gs.deletion_protection,
                kms_key_id: gs.kms_key_id,
            };
            for ss in gs.streams {
                let stream = LogStream {
                    name: ss.name.clone(),
                    arn: ss.arn,
                    creation_time: ss.creation_time,
                    first_event_timestamp: ss.first_event_timestamp,
                    last_event_timestamp: ss.last_event_timestamp,
                    last_ingestion_time: ss.last_ingestion_time,
                    upload_sequence_token: Arc::new(AtomicU64::new(ss.upload_sequence_token)),
                };
                group.streams.insert(ss.name, stream);
            }
            state.log_groups.insert(gs.name, group);
        }
        for f in snapshot.subscription_filters {
            state
                .subscription_filters
                .insert((f.log_group_name.clone(), f.filter_name.clone()), f);
        }
        for f in snapshot.metric_filters {
            state
                .metric_filters
                .insert((f.log_group_name.clone(), f.filter_name.clone()), f);
        }
        for q in snapshot.query_definitions {
            state
                .query_definitions
                .insert(q.query_definition_id.clone(), q);
        }
        for q in snapshot.insights_queries {
            state.insights_queries.insert(q.query_id.clone(), q);
        }
        (snapshot.account_id, snapshot.region, state)
    }
}
