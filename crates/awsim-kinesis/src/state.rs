use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

use dashmap::DashMap;

/// Per-account/region Kinesis state. Records live in the shared
/// `SqliteStore` (off `state.sqlite`); everything below is metadata.
#[derive(Debug, Default)]
pub struct KinesisState {
    /// Stream name → KinesisStream
    pub streams: DashMap<String, KinesisStream>,
    /// Shard iterator token → ShardIteratorInfo
    pub iterators: DashMap<String, ShardIteratorInfo>,
    /// ConsumerArn → StreamConsumer
    pub consumers: DashMap<String, StreamConsumer>,
    /// Resource ARN -> policy JSON
    pub resource_policies: DashMap<String, String>,
    /// Resource ARN -> tags
    pub resource_tags: DashMap<String, HashMap<String, String>>,
    pub account_settings: std::sync::RwLock<AccountSettings>,
    /// Shared SQLite store for records. Set on first `get_state`.
    pub sqlite: OnceLock<Arc<crate::SqliteStore>>,
}

impl KinesisState {
    pub fn sqlite(&self) -> Option<&Arc<crate::SqliteStore>> {
        self.sqlite.get()
    }

    pub fn set_sqlite(&self, store: Arc<crate::SqliteStore>) {
        let _ = self.sqlite.set(store);
    }
}

#[derive(Debug, Clone)]
pub struct AccountSettings {
    pub max_record_size: u64,
    pub default_shard_limit: u64,
}

impl Default for AccountSettings {
    fn default() -> Self {
        Self {
            max_record_size: 1_048_576,
            default_shard_limit: 500,
        }
    }
}

/// A stream consumer (enhanced fan-out).
#[derive(Debug, Clone)]
pub struct StreamConsumer {
    pub consumer_arn: String,
    pub consumer_name: String,
    pub consumer_status: String,
    pub stream_arn: String,
    pub consumer_creation_timestamp: u64,
    /// Unix seconds of the last `SubscribeToShard` (or registration).
    /// The tick sweep deregisters consumers idle past
    /// [`CONSUMER_IDLE_SECS`].
    pub last_active_secs: u64,
}

/// Enhanced-fan-out consumers idle for longer than this are swept by
/// the tick loop, mirroring AWS reclaiming abandoned subscriptions.
pub const CONSUMER_IDLE_SECS: u64 = 300;

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
    /// PROVISIONED or ON_DEMAND
    pub stream_mode: String,
    pub warm_throughput_mibps: u64,
    pub warm_throughput_records: u64,
    /// Staged `UpdateShardCount` transition: the deadline after which
    /// the stream promotes back to ACTIVE plus the replacement shard
    /// set. `None` when the stream is settled. Every read path routes
    /// through [`KinesisStream::promote`] so a stream never appears
    /// stuck in `UPDATING`.
    pub pending_update: Option<(SystemTime, Vec<Shard>)>,
}

impl KinesisStream {
    /// Promote a staged `UpdateShardCount` once its deadline has
    /// elapsed: swap in the new shards and flip status back to
    /// `ACTIVE`. Idempotent and absolute-time gated, so the tick loop
    /// and every `Describe*` read path can call it freely.
    pub fn promote(&mut self, now: SystemTime) {
        let due = matches!(&self.pending_update, Some((at, _)) if now >= *at);
        if due && let Some((_, shards)) = self.pending_update.take() {
            self.shards = shards;
            self.status = "ACTIVE".to_string();
        }
    }

    /// Stage an `UpdateShardCount` transition to `new_shards`, flipping
    /// status to `UPDATING` until `deadline`.
    pub fn begin_update(&mut self, deadline: SystemTime, new_shards: Vec<Shard>) {
        self.status = "UPDATING".to_string();
        self.pending_update = Some((deadline, new_shards));
    }
}

/// A single shard within a stream. Records themselves live in the
/// SQLite store; the shard struct only tracks routing metadata + the
/// next sequence number to allocate.
#[derive(Debug, Clone)]
pub struct Shard {
    pub shard_id: String,
    /// (starting_hash_key, ending_hash_key)
    pub hash_key_range: (String, String),
    /// (starting_sequence_number, ending_sequence_number)
    pub sequence_number_range: (String, Option<String>),
    pub next_sequence: u64,
}

impl Shard {
    pub fn new_range(index: usize, start_hash: u128, end_hash: u128) -> Self {
        Shard {
            shard_id: shard_id_for(index),
            hash_key_range: (start_hash.to_string(), end_hash.to_string()),
            sequence_number_range: (format!("{:020}", 0), None),
            next_sequence: 1,
        }
    }

    /// Allocate the next sequence number for this shard, returning
    /// both the i64 numeric form (for SQLite indexing) and the
    /// 20-digit zero-padded string (the AWS wire format).
    pub fn alloc_sequence(&mut self) -> (i64, String) {
        let seq = self.next_sequence;
        self.next_sequence += 1;
        (seq as i64, format!("{seq:020}"))
    }
}

/// Information about an active shard iterator. `position` is now a
/// sequence-number cursor (exclusive lower bound) — `GetRecords`
/// returns rows with `seq > position`.
#[derive(Debug, Clone)]
pub struct ShardIteratorInfo {
    pub stream_name: String,
    pub shard_index: usize,
    /// Sequence-number cursor (exclusive lower bound). Records with
    /// `seq > position` are returned by `GetRecords`.
    pub position: u64,
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
