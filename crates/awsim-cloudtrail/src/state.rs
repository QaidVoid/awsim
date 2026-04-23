use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default)]
pub struct CloudTrailState {
    pub trails: DashMap<String, Trail>,
    pub trail_status: DashMap<String, TrailStatus>,
    pub event_selectors: DashMap<String, Vec<EventSelector>>,
    pub insight_selectors: DashMap<String, Vec<InsightSelector>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trail {
    pub name: String,
    pub arn: String,
    pub s3_bucket_name: String,
    pub s3_key_prefix: Option<String>,
    pub sns_topic_name: Option<String>,
    pub sns_topic_arn: Option<String>,
    pub include_global_service_events: bool,
    pub is_multi_region_trail: bool,
    pub home_region: String,
    pub log_file_validation_enabled: bool,
    pub cloud_watch_logs_log_group_arn: Option<String>,
    pub cloud_watch_logs_role_arn: Option<String>,
    pub kms_key_id: Option<String>,
    pub has_custom_event_selectors: bool,
    pub has_insight_selectors: bool,
    pub is_organization_trail: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrailStatus {
    pub is_logging: bool,
    pub latest_delivery_error: Option<String>,
    pub latest_notification_error: Option<String>,
    pub latest_delivery_time: Option<u64>,
    pub latest_notification_time: Option<u64>,
    pub start_logging_time: Option<u64>,
    pub stop_logging_time: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSelector {
    pub read_write_type: String,
    pub include_management_events: bool,
    pub data_resources: Vec<serde_json::Value>,
    pub exclude_management_event_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightSelector {
    pub insight_type: String,
}

pub fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
