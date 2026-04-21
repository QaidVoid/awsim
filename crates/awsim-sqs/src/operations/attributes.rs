use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SqsState;
use crate::util::queue_name_from_url;

// Attributes that are always present in GetQueueAttributes
const KNOWN_ATTRIBUTES: &[&str] = &[
    "All",
    "ApproximateNumberOfMessages",
    "ApproximateNumberOfMessagesDelayed",
    "ApproximateNumberOfMessagesNotVisible",
    "ContentBasedDeduplication",
    "CreatedTimestamp",
    "DeduplicationScope",
    "DelaySeconds",
    "FifoQueue",
    "FifoThroughputLimit",
    "LastModifiedTimestamp",
    "MaximumMessageSize",
    "MessageRetentionPeriod",
    "Policy",
    "QueueArn",
    "ReceiveMessageWaitTimeSeconds",
    "RedriveAllowPolicy",
    "RedrivePolicy",
    "VisibilityTimeout",
];

pub fn get_queue_attributes(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;

    let queue_name = queue_name_from_url(queue_url)?;

    let mut queue = state.queues.get_mut(&queue_name).ok_or_else(|| {
        AwsError::not_found(
            "AWS.SimpleQueueService.NonExistentQueue",
            format!("The specified queue does not exist: {queue_url}"),
        )
    })?;

    // Refresh live counters
    queue.tick();
    let visible = queue.approximate_number_of_messages();
    let delayed = queue.approximate_number_of_messages_delayed();
    let not_visible = queue.approximate_number_of_messages_not_visible();

    let created_at = queue.created_at.clone();
    queue
        .attributes
        .insert("ApproximateNumberOfMessages".to_string(), visible.to_string());
    queue.attributes.insert(
        "ApproximateNumberOfMessagesDelayed".to_string(),
        delayed.to_string(),
    );
    queue.attributes.insert(
        "ApproximateNumberOfMessagesNotVisible".to_string(),
        not_visible.to_string(),
    );
    queue
        .attributes
        .insert("CreatedTimestamp".to_string(), created_at);

    // Determine which attributes to return
    let want_all;
    let attribute_names: Vec<&str> = if let Some(names) = input["AttributeNames"].as_array() {
        let v: Vec<&str> = names.iter().filter_map(|n| n.as_str()).collect();
        want_all = v.contains(&"All");
        v
    } else {
        want_all = true;
        vec!["All"]
    };

    let mut result = serde_json::Map::new();
    for (k, v) in &queue.attributes {
        if want_all || attribute_names.contains(&k.as_str()) {
            result.insert(k.clone(), Value::String(v.clone()));
        }
    }

    Ok(json!({ "Attributes": result }))
}

pub fn set_queue_attributes(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;

    let attrs = input["Attributes"]
        .as_object()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Attributes is required"))?;

    let queue_name = queue_name_from_url(queue_url)?;

    let mut queue = state.queues.get_mut(&queue_name).ok_or_else(|| {
        AwsError::not_found(
            "AWS.SimpleQueueService.NonExistentQueue",
            format!("The specified queue does not exist: {queue_url}"),
        )
    })?;

    for (k, v) in attrs {
        // Reject unknown attribute names
        if !KNOWN_ATTRIBUTES.contains(&k.as_str()) {
            return Err(AwsError::bad_request(
                "InvalidAttributeName",
                format!("Unknown attribute name: {k}"),
            ));
        }
        if let Some(s) = v.as_str() {
            queue.attributes.insert(k.clone(), s.to_string());
        }
    }

    // Refresh the cached redrive_policy in case RedrivePolicy attribute changed
    queue.refresh_redrive_policy();

    Ok(json!({}))
}
