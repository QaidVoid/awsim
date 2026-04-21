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

    let queue_name = queue_name_from_url(queue_url)?;

    let mut queue = state.queues.get_mut(&queue_name).ok_or_else(|| {
        AwsError::not_found(
            "AWS.SimpleQueueService.NonExistentQueue",
            format!("The specified queue does not exist: {queue_url}"),
        )
    })?;

    // SQS silently ignores invalid/expired receipt handles per AWS spec
    queue.inflight.remove(receipt_handle);

    Ok(json!({}))
}

pub fn handle_batch(
    state: &SqsState,
    input: &Value,
    ctx: &RequestContext,
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
            "There must be at least one entry in the batch",
        ));
    }
    if entries.len() > 10 {
        return Err(AwsError::bad_request(
            "TooManyEntriesInBatchRequest",
            "Maximum number of entries per request is 10",
        ));
    }

    let mut successful = vec![];
    let mut failed = vec![];

    for entry in entries {
        let id = entry["Id"].as_str().unwrap_or("").to_string();
        let receipt_handle = entry["ReceiptHandle"].as_str().unwrap_or("").to_string();

        let entry_input = json!({
            "QueueUrl": queue_url,
            "ReceiptHandle": receipt_handle,
        });

        match handle(state, &entry_input, ctx) {
            Ok(_) => {
                successful.push(json!({ "Id": id }));
            }
            Err(e) => {
                failed.push(json!({
                    "Id": id,
                    "SenderFault": true,
                    "Code": e.code,
                    "Message": e.message,
                }));
            }
        }
    }

    Ok(json!({
        "Successful": successful,
        "Failed": failed,
    }))
}
