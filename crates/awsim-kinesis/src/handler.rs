use std::path::Path;
use std::sync::Arc;

use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::SqliteStore;
use crate::operations::{
    consumers, create_stream, delete_stream, describe_stream, describe_stream_summary, encryption,
    extras, get_records, get_shard_iterator, list_shards, list_streams, merge_split, monitoring,
    put_record, put_records, retention, tags, update_shard_count,
};
use crate::state::KinesisState;

/// The Kinesis Data Streams service handler. Records live in
/// `sqlite_store`; stream / shard / iterator metadata stays in
/// per-(account, region) DashMaps on `KinesisState`.
pub struct KinesisService {
    store: AccountRegionStore<KinesisState>,
    sqlite_store: Arc<SqliteStore>,
    _tempdir: Option<tempfile::TempDir>,
}

impl KinesisService {
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("awsim-kinesis-")
            .tempdir()
            .expect("creating ephemeral Kinesis tempdir should not fail");
        let path = dir.path().join("kinesis.db");
        let sqlite_store = Arc::new(
            SqliteStore::open(&path)
                .expect("opening ephemeral Kinesis sqlite store should not fail"),
        );
        Self {
            store: AccountRegionStore::new(),
            sqlite_store,
            _tempdir: Some(dir),
        }
    }

    pub fn with_data_dir(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir)
            .unwrap_or_else(|e| panic!("creating Kinesis data dir {} failed: {e}", dir.display()));
        let path = dir.join("kinesis.db");
        let sqlite_store = Arc::new(SqliteStore::open(&path).unwrap_or_else(|e| {
            panic!(
                "opening persistent Kinesis sqlite store at {} failed: {e}",
                path.display()
            )
        }));
        Self {
            store: AccountRegionStore::new(),
            sqlite_store,
            _tempdir: None,
        }
    }

    /// Path to the tempdir (when this instance owns one) so the
    /// awsim binary can clean it up before `process::exit`.
    pub fn tempdir_path(&self) -> Option<&Path> {
        self._tempdir.as_ref().map(|d| d.path())
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<KinesisState> {
        let state = self.store.get(&ctx.account_id, &ctx.region);
        state.set_sqlite(Arc::clone(&self.sqlite_store));
        state
    }
}

impl Default for KinesisService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for KinesisService {
    fn service_name(&self) -> &str {
        "kinesis"
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
        debug!(operation = %operation, "Kinesis operation");

        let state = self.get_state(ctx);

        match operation {
            "CreateStream" => create_stream::handle(&state, &input, ctx),
            "DeleteStream" => delete_stream::handle(&state, &input, ctx),
            "DescribeStream" => describe_stream::handle(&state, &input, ctx),
            "DescribeStreamSummary" => describe_stream_summary::handle(&state, &input, ctx),
            "ListStreams" => list_streams::handle(&state, &input, ctx),
            "ListShards" => list_shards::handle(&state, &input, ctx),
            "PutRecord" => put_record::handle(&state, &input, ctx),
            "PutRecords" => put_records::handle(&state, &input, ctx),
            "GetShardIterator" => get_shard_iterator::handle(&state, &input, ctx),
            "GetRecords" => get_records::handle(&state, &input, ctx),
            "MergeShards" => merge_split::handle_merge(&state, &input, ctx),
            "SplitShard" => merge_split::handle_split(&state, &input, ctx),
            "AddTagsToStream" => tags::add_tags(&state, &input, ctx),
            "RemoveTagsFromStream" => tags::remove_tags(&state, &input, ctx),
            "ListTagsForStream" => tags::list_tags(&state, &input, ctx),
            "IncreaseStreamRetentionPeriod" => retention::increase(&state, &input, ctx),
            "DecreaseStreamRetentionPeriod" => retention::decrease(&state, &input, ctx),
            // Consumers (enhanced fan-out)
            "RegisterStreamConsumer" => consumers::register_stream_consumer(&state, &input, ctx),
            "DeregisterStreamConsumer" => {
                consumers::deregister_stream_consumer(&state, &input, ctx)
            }
            "DescribeStreamConsumer" => consumers::describe_stream_consumer(&state, &input, ctx),
            "ListStreamConsumers" => consumers::list_stream_consumers(&state, &input, ctx),
            "SubscribeToShard" => consumers::subscribe_to_shard(&state, &input, ctx),
            // Monitoring
            "EnableEnhancedMonitoring" => {
                monitoring::enable_enhanced_monitoring(&state, &input, ctx)
            }
            "DisableEnhancedMonitoring" => {
                monitoring::disable_enhanced_monitoring(&state, &input, ctx)
            }
            // Encryption
            "StartStreamEncryption" => encryption::start_stream_encryption(&state, &input, ctx),
            "StopStreamEncryption" => encryption::stop_stream_encryption(&state, &input, ctx),
            // Update shard count
            "UpdateShardCount" => update_shard_count::handle(&state, &input, ctx),
            // Resource policies
            "PutResourcePolicy" => extras::put_resource_policy(&state, &input, ctx),
            "GetResourcePolicy" => extras::get_resource_policy(&state, &input, ctx),
            "DeleteResourcePolicy" => extras::delete_resource_policy(&state, &input, ctx),
            // Tags (resource ARN form)
            "TagResource" => extras::tag_resource(&state, &input, ctx),
            "UntagResource" => extras::untag_resource(&state, &input, ctx),
            "ListTagsForResource" => extras::list_tags_for_resource(&state, &input, ctx),
            // Account & limits
            "DescribeAccountSettings" => extras::describe_account_settings(&state, &input, ctx),
            "DescribeLimits" => extras::describe_limits(&state, &input, ctx),
            "UpdateAccountSettings" => extras::update_account_settings(&state, &input, ctx),
            "UpdateMaxRecordSize" => extras::update_max_record_size(&state, &input, ctx),
            // Stream mode / warm throughput
            "UpdateStreamMode" => extras::update_stream_mode(&state, &input, ctx),
            "UpdateStreamWarmThroughput" => {
                extras::update_stream_warm_throughput(&state, &input, ctx)
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
