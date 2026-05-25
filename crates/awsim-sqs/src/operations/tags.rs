use awsim_core::tags::{TagOpts, validate_aws_tag_keys, validate_aws_tags};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SqsState;
use crate::util::queue_name_from_url;

pub fn tag_queue(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;

    validate_aws_tags(&input["Tags"], &TagOpts::aws_default())?;

    let tags = input["Tags"]
        .as_object()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Tags is required"))?;

    let queue_name = queue_name_from_url(queue_url)?;

    let mut queue = state.queues.get_mut(&queue_name).ok_or_else(|| {
        AwsError::bad_request(
            "AWS.SimpleQueueService.NonExistentQueue",
            format!("The specified queue does not exist: {queue_url}"),
        )
    })?;

    for (k, v) in tags {
        if let Some(s) = v.as_str() {
            queue.tags.insert(k.clone(), s.to_string());
        }
    }

    Ok(json!({}))
}

pub fn untag_queue(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;

    validate_aws_tag_keys(&input["TagKeys"])?;

    let tag_keys = input["TagKeys"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "TagKeys is required"))?;

    let queue_name = queue_name_from_url(queue_url)?;

    let mut queue = state.queues.get_mut(&queue_name).ok_or_else(|| {
        AwsError::bad_request(
            "AWS.SimpleQueueService.NonExistentQueue",
            format!("The specified queue does not exist: {queue_url}"),
        )
    })?;

    for key in tag_keys {
        if let Some(k) = key.as_str() {
            queue.tags.remove(k);
        }
    }

    Ok(json!({}))
}

pub fn list_queue_tags(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;

    let queue_name = queue_name_from_url(queue_url)?;

    let queue = state.queues.get(&queue_name).ok_or_else(|| {
        AwsError::bad_request(
            "AWS.SimpleQueueService.NonExistentQueue",
            format!("The specified queue does not exist: {queue_url}"),
        )
    })?;

    let tags: serde_json::Map<String, Value> = queue
        .tags
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();

    Ok(json!({ "Tags": tags }))
}
