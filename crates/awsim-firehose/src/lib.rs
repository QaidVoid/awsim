mod operations;
mod state;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::FirehoseState;

pub struct FirehoseService {
    store: AccountRegionStore<FirehoseState>,
}

impl FirehoseService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for FirehoseService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for FirehoseService {
    fn service_name(&self) -> &str {
        "firehose"
    }

    fn signing_name(&self) -> &str {
        "firehose"
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
        debug!(operation = %operation, "Firehose operation");
        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            "CreateDeliveryStream" => operations::streams::create_delivery_stream(&state, &input, ctx),
            "DeleteDeliveryStream" => operations::streams::delete_delivery_stream(&state, &input, ctx),
            "DescribeDeliveryStream" => operations::streams::describe_delivery_stream(&state, &input, ctx),
            "ListDeliveryStreams" => operations::streams::list_delivery_streams(&state, &input, ctx),
            "UpdateDestination" => operations::streams::update_destination(&state, &input, ctx),
            "PutRecord" => operations::records::put_record(&state, &input, ctx),
            "PutRecordBatch" => operations::records::put_record_batch(&state, &input, ctx),
            "TagDeliveryStream" => operations::tags::tag_delivery_stream(&state, &input, ctx),
            "UntagDeliveryStream" => operations::tags::untag_delivery_stream(&state, &input, ctx),
            "ListTagsForDeliveryStream" => operations::tags::list_tags_for_delivery_stream(&state, &input, ctx),
            "StartDeliveryStreamEncryption" => operations::encryption::start_encryption(&state, &input, ctx),
            "StopDeliveryStreamEncryption" => operations::encryption::stop_encryption(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
