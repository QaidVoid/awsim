use std::collections::HashMap;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default)]
pub struct FirehoseState {
    pub streams: DashMap<String, DeliveryStream>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryStream {
    pub name: String,
    pub arn: String,
    pub status: String,
    pub stream_type: String,
    pub version_id: String,
    pub create_timestamp: u64,
    pub last_update_timestamp: u64,
    pub destinations: Vec<serde_json::Value>,
    pub has_more_destinations: bool,
    pub tags: HashMap<String, String>,
    pub encryption_enabled: bool,
    pub encryption_key_type: Option<String>,
    pub encryption_key_arn: Option<String>,
}

pub fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
