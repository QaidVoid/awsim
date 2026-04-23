use std::time::{Duration, Instant};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SqsState;
use crate::util::queue_name_from_url;

// ---------------------------------------------------------------------------
// ChangeMessageVisibility
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// ChangeMessageVisibilityBatch
// ---------------------------------------------------------------------------

pub fn handle_batch(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;

    let entries = input["Entries"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Entries is required"))?;

    if entries.is_empty() {
        return Err(AwsError::bad_request(
            "EmptyBatchRequest",
            "The batch request list is empty",
        ));
    }
    if entries.len() > 10 {
        return Err(AwsError::bad_request(
            "TooManyEntriesInBatchRequest",
            "Maximum 10 entries per batch",
        ));
    }

    let queue_name = queue_name_from_url(queue_url)?;

    let mut successful: Vec<Value> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();

    for entry in entries {
        let id = entry["Id"].as_str().unwrap_or("").to_string();
        let receipt_handle = entry["ReceiptHandle"].as_str();
        let visibility_timeout = entry["VisibilityTimeout"].as_u64();

        match (receipt_handle, visibility_timeout) {
            (None, _) => {
                failed.push(json!({
                    "Id": id,
                    "SenderFault": true,
                    "Code": "MissingParameter",
                    "Message": "ReceiptHandle is required",
                }));
            }
            (_, None) => {
                failed.push(json!({
                    "Id": id,
                    "SenderFault": true,
                    "Code": "MissingParameter",
                    "Message": "VisibilityTimeout is required",
                }));
            }
            (Some(rh), Some(vt)) => {
                let mut queue = match state.queues.get_mut(&queue_name) {
                    Some(q) => q,
                    None => {
                        failed.push(json!({
                            "Id": id,
                            "SenderFault": false,
                            "Code": "AWS.SimpleQueueService.NonExistentQueue",
                            "Message": format!("The specified queue does not exist: {queue_url}"),
                        }));
                        continue;
                    }
                };

                match queue.inflight.get_mut(rh) {
                    None => {
                        failed.push(json!({
                            "Id": id,
                            "SenderFault": true,
                            "Code": "ReceiptHandleIsInvalid",
                            "Message": format!("The receipt handle '{}' is not valid.", rh),
                        }));
                    }
                    Some(im) => {
                        use std::time::{SystemTime, UNIX_EPOCH};
                        let now = Instant::now();
                        let now_epoch = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        im.visible_at = Some(now + Duration::from_secs(vt));
                        im.visible_at_secs = now_epoch + vt;
                        successful.push(json!({ "Id": id }));
                    }
                }
            }
        }
    }

    Ok(json!({
        "Successful": successful,
        "Failed": failed,
    }))
}
