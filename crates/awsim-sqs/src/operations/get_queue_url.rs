use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SqsState;

pub fn handle(state: &SqsState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let queue_name = input["QueueName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueName is required"))?;

    let queue = state.queues.get(queue_name).ok_or_else(|| {
        AwsError::bad_request(
            "AWS.SimpleQueueService.NonExistentQueue",
            format!("The specified queue does not exist: {queue_name}"),
        )
    })?;

    Ok(json!({ "QueueUrl": queue.url }))
}
