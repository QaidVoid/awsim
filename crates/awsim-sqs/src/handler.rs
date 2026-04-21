use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{
    attributes, change_visibility, create_queue, delete_message, delete_queue, get_queue_url,
    list_queues, purge_queue, receive_message, send_message, tags,
};
use crate::state::SqsState;

/// The SQS service handler.
pub struct SqsService {
    store: AccountRegionStore<SqsState>,
}

impl SqsService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for SqsService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for SqsService {
    fn service_name(&self) -> &str {
        "sqs"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_0
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation = %operation, "SQS operation");

        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            "CreateQueue" => create_queue::handle(&state, &input, ctx),
            "DeleteQueue" => delete_queue::handle(&state, &input, ctx),
            "ListQueues" => list_queues::handle(&state, &input, ctx),
            "GetQueueUrl" => get_queue_url::handle(&state, &input, ctx),
            "GetQueueAttributes" => attributes::get_queue_attributes(&state, &input, ctx),
            "SetQueueAttributes" => attributes::set_queue_attributes(&state, &input, ctx),
            "SendMessage" => send_message::handle(&state, &input, ctx),
            "SendMessageBatch" => send_message::handle_batch(&state, &input, ctx),
            "ReceiveMessage" => receive_message::handle(&state, &input, ctx),
            "DeleteMessage" => delete_message::handle(&state, &input, ctx),
            "DeleteMessageBatch" => delete_message::handle_batch(&state, &input, ctx),
            "ChangeMessageVisibility" => change_visibility::handle(&state, &input, ctx),
            "PurgeQueue" => purge_queue::handle(&state, &input, ctx),
            "TagQueue" => tags::tag_queue(&state, &input, ctx),
            "UntagQueue" => tags::untag_queue(&state, &input, ctx),
            "ListQueueTags" => tags::list_queue_tags(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
