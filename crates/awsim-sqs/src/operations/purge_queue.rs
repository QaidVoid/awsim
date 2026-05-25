use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::state::SqsState;
use crate::util::queue_name_from_url;

pub fn handle(state: &SqsState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;

    let queue_name = queue_name_from_url(queue_url)?;

    let (msg_count, inflight_count) = {
        let mut queue = state.queues.get_mut(&queue_name).ok_or_else(|| {
            AwsError::bad_request(
                "AWS.SimpleQueueService.NonExistentQueue",
                format!("The specified queue does not exist: {queue_url}"),
            )
        })?;

        let msg_count = queue.messages.len();
        let inflight_count = queue.inflight.len();
        queue.messages.clear();
        queue.inflight.clear();
        (msg_count, inflight_count)
    };

    if let Some(bs) = state.body_store()
        && let Err(e) = bs.delete_bucket("sqs", &queue_name)
    {
        warn!(queue = %queue_name, error = %e, "Failed to purge persisted SQS message bodies");
    }

    info!(
        queue = %queue_name,
        messages_purged = msg_count + inflight_count,
        "Purged queue"
    );

    Ok(json!({}))
}
