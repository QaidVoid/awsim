use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::SqsState;
use crate::util::queue_name_from_url;

pub fn handle(state: &SqsState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;

    let queue_name = queue_name_from_url(queue_url)?;

    if state.queues.remove(&queue_name).is_none() {
        return Err(AwsError::not_found(
            "AWS.SimpleQueueService.NonExistentQueue",
            format!("The specified queue does not exist: {queue_url}"),
        ));
    }

    info!(queue = %queue_name, "Deleted queue");
    Ok(json!({}))
}
