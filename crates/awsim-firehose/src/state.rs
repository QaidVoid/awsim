use std::collections::HashMap;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default)]
pub struct FirehoseState {
    pub streams: DashMap<String, DeliveryStream>,
    /// Per-stream delivery metrics + the most recently written S3 object
    /// keys. Kept in a parallel map (not on `DeliveryStream`) so it stays
    /// ephemeral and doesn't bloat the snapshot; real Firehose surfaces
    /// these to CloudWatch AWS/Firehose, which is out of scope.
    pub delivery_stats: DashMap<String, DeliveryStats>,
}

#[derive(Debug, Default, Clone)]
pub struct DeliveryStats {
    pub last_s3_keys: Vec<String>,
    pub succeeded_records: u64,
    pub processing_failed: u64,
    pub processing_dropped: u64,
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
    /// `true` only once encryption has fully reached ENABLED; drives the
    /// per-record `Encrypted` flag.
    pub encryption_enabled: bool,
    /// Wire status DISABLED -> ENABLING -> ENABLED -> DISABLING ->
    /// DISABLED, advanced by the tick driver.
    #[serde(default = "default_encryption_status")]
    pub encryption_status: String,
    pub encryption_key_type: Option<String>,
    pub encryption_key_arn: Option<String>,
    /// Raw source configuration captured at create time. Populated for
    /// `KinesisStreamAsSource`, `MSKAsSource`, or `DatabaseAsSource`
    /// streams; absent for `DirectPut`. Echoed back to clients under
    /// `Source.<Kind>SourceDescription`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_config: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FirehoseSnapshot {
    pub streams: Vec<DeliveryStream>,
}

impl FirehoseState {
    pub fn to_snapshot(&self) -> FirehoseSnapshot {
        FirehoseSnapshot {
            streams: self.streams.iter().map(|e| e.value().clone()).collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snap: FirehoseSnapshot) {
        self.streams.clear();
        for s in snap.streams {
            self.streams.insert(s.name.clone(), s);
        }
    }
}

pub fn default_encryption_status() -> String {
    "DISABLED".to_string()
}

impl DeliveryStream {
    /// Advance the encryption state one hop: ENABLING -> ENABLED and
    /// DISABLING -> DISABLED. Idempotent; called from the tick driver.
    /// Reaching ENABLED arms the `Encrypted` flag; reaching DISABLED
    /// clears it and the key material.
    pub fn advance_encryption(&mut self) {
        match self.encryption_status.as_str() {
            "ENABLING" => {
                self.encryption_status = "ENABLED".to_string();
                self.encryption_enabled = true;
            }
            "DISABLING" => {
                self.encryption_status = "DISABLED".to_string();
                self.encryption_enabled = false;
                self.encryption_key_type = None;
                self.encryption_key_arn = None;
            }
            _ => {}
        }
    }
}

pub fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
