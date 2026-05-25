use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SqsState;
use crate::util::queue_name_from_url;

/// ListDeadLetterSourceQueues — find all queues whose RedrivePolicy targets
/// the given DLQ URL (matched by ARN).
pub fn list_dead_letter_source_queues(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;

    let dlq_name = queue_name_from_url(queue_url)?;

    // Verify the DLQ itself exists and obtain its ARN.
    let dlq_arn = {
        let dlq = state.queues.get(&dlq_name).ok_or_else(|| {
            AwsError::bad_request(
                "AWS.SimpleQueueService.NonExistentQueue",
                format!("The specified queue does not exist: {queue_url}"),
            )
        })?;
        dlq.arn.clone()
    };

    // Collect URLs of queues that target this DLQ.
    let queue_urls: Vec<Value> = state
        .queues
        .iter()
        .filter(|entry| {
            entry
                .value()
                .redrive_policy
                .as_ref()
                .is_some_and(|rp| rp.dead_letter_target_arn == dlq_arn)
        })
        .map(|entry| Value::String(entry.value().url.clone()))
        .collect();

    Ok(json!({ "queueUrls": queue_urls }))
}
