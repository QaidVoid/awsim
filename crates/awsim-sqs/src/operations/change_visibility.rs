use std::time::{Duration, Instant};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SqsState;
use crate::util::queue_name_from_url;

pub fn handle(state: &SqsState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;

    let receipt_handle = input["ReceiptHandle"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "ReceiptHandle is required"))?;

    let visibility_timeout = input["VisibilityTimeout"]
        .as_u64()
        .ok_or_else(|| {
            AwsError::bad_request("MissingParameter", "VisibilityTimeout is required")
        })?;

    let queue_name = queue_name_from_url(queue_url)?;

    let mut queue = state.queues.get_mut(&queue_name).ok_or_else(|| {
        AwsError::not_found(
            "AWS.SimpleQueueService.NonExistentQueue",
            format!("The specified queue does not exist: {queue_url}"),
        )
    })?;

    let im = queue.inflight.get_mut(receipt_handle).ok_or_else(|| {
        AwsError::bad_request(
            "ReceiptHandleIsInvalid",
            format!("The receipt handle '{}' is not valid.", receipt_handle),
        )
    })?;

    use std::time::{SystemTime, UNIX_EPOCH};
    let now = Instant::now();
    let now_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    im.visible_at = Some(now + Duration::from_secs(visibility_timeout));
    im.visible_at_secs = now_epoch + visibility_timeout;

    Ok(json!({}))
}
