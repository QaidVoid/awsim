use std::collections::HashMap;

use dashmap::DashMap;

/// Per-account/region Kinesis state.
#[derive(Debug, Default)]
pub struct KinesisState {
    /// Stream name → KinesisStream
    pub streams: DashMap<String, KinesisStream>,
    /// Shard iterator token → ShardIteratorInfo
    pub iterators: DashMap<String, ShardIteratorInfo>,
    /// ConsumerArn → StreamConsumer
    pub consumers: DashMap<String, StreamConsumer>,
}

/// A stream consumer (enhanced fan-out).
#[derive(Debug, Clone)]
pub struct StreamConsumer {
    pub consumer_arn: String,
    pub consumer_name: String,
    pub consumer_status: String,
    pub stream_arn: String,
    pub consumer_creation_timestamp: u64,
}

/// A Kinesis Data Stream.
#[derive(Debug, Clone)]
pub struct KinesisStream {
    pub name: String,
    pub arn: String,
    /// CREATING, ACTIVE, DELETING, etc.
    pub status: String,
    pub shards: Vec<Shard>,
    pub retention_hours: u32,
    pub tags: HashMap<String, String>,
    pub created_at: u64,
    /// Enhanced monitoring shard-level metrics.
    pub enhanced_monitoring: Vec<String>,
    /// Encryption type: NONE or KMS.
    pub encryption_type: String,
    /// KMS key ID (if encrypted).
    pub key_id: Option<String>,
}

/// A single shard within a stream.
#[derive(Debug, Clone)]
pub struct Shard {
    pub shard_id: String,
    /// (starting_hash_key, ending_hash_key)
    pub hash_key_range: (String, String),
    /// (starting_sequence_number, ending_sequence_number)
    pub sequence_number_range: (String, Option<String>),
    pub records: Vec<KinesisRecord>,
    pub next_sequence: u64,
}

impl Shard {
    pub fn new_range(index: usize, start_hash: u128, end_hash: u128) -> Self {
        Shard {
            shard_id: shard_id_for(index),
            hash_key_range: (start_hash.to_string(), end_hash.to_string()),
            sequence_number_range: (format!("{:020}", 0), None),
            records: Vec::new(),
            next_sequence: 1,
        }
    }

    /// Allocate the next sequence number for this shard.
    pub fn alloc_sequence(&mut self) -> String {
        let seq = self.next_sequence;
        self.next_sequence += 1;
        format!("{:020}", seq)
    }
}

/// A record stored in a shard.
#[derive(Debug, Clone)]
pub struct KinesisRecord {
    pub sequence_number: String,
    /// base64-encoded data
    pub data: String,
    pub partition_key: String,
    pub timestamp_millis: u64,
}

/// Information about an active shard iterator.
#[derive(Debug, Clone)]
pub struct ShardIteratorInfo {
    pub stream_name: String,
    pub shard_index: usize,
    /// Next record index to return (position within shard.records).
    pub position: usize,
}

/// Build a standard shard ID string from an index.
pub fn shard_id_for(index: usize) -> String {
    format!("shardId-{:012}", index)
}

/// Current Unix timestamp in milliseconds.
pub fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Current Unix timestamp in seconds.
pub fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
