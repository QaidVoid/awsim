use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{
    create_stream, delete_stream, describe_stream, describe_stream_summary, get_records,
    get_shard_iterator, list_shards, list_streams, merge_split, put_record, put_records,
    retention, tags,
};
use crate::state::KinesisState;

/// The Kinesis Data Streams service handler.
pub struct KinesisService {
    store: AccountRegionStore<KinesisState>,
}

impl KinesisService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
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

        let state = self.store.get(&ctx.account_id, &ctx.region);

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
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
