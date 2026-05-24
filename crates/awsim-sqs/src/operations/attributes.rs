use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SqsState;
use crate::util::queue_name_from_url;

// Attributes recognized by GetQueueAttributes / SetQueueAttributes. Server-
// managed counters (ApproximateNumberOf* and timestamps) are surfaced on Get
// but not settable; we accept them in this list so SetQueueAttributes does
// not raise InvalidAttributeName for callers echoing a previously fetched
// attribute set.
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
    // Server-side encryption: KMS-managed and SQS-managed.
    "KmsDataKeyReusePeriodSeconds",
    "KmsMasterKeyId",
    "LastModifiedTimestamp",
    "MaximumMessageSize",
    "MessageRetentionPeriod",
    "Policy",
    "QueueArn",
    "ReceiveMessageWaitTimeSeconds",
    "RedriveAllowPolicy",
    "RedrivePolicy",
    "SqsManagedSseEnabled",
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
    queue.attributes.insert(
        "ApproximateNumberOfMessages".to_string(),
        visible.to_string(),
    );
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
        // FifoQueue is fixed at queue creation and cannot be flipped
        // afterwards. Real SQS rejects with InvalidAttributeName even
        // though the name itself is recognised - matches the AWS
        // contract that swapping queue type after the fact is
        // unsupported.
        if k == "FifoQueue" {
            return Err(AwsError::bad_request(
                "InvalidAttributeName",
                "FifoQueue cannot be modified after queue creation.",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Queue;
    use std::collections::HashMap;

    fn ctx() -> RequestContext {
        RequestContext::new("sqs", "us-east-1")
    }

    fn standard_queue() -> SqsState {
        let state = SqsState::default();
        let q = Queue::new(
            "q".to_string(),
            "http://localhost/queue/q".to_string(),
            "arn:aws:sqs:us-east-1:000000000000:q".to_string(),
            false,
            "now".to_string(),
            HashMap::new(),
        );
        state.queues.insert("q".to_string(), q);
        state
    }

    #[test]
    fn set_queue_attributes_accepts_kms_attributes() {
        let state = standard_queue();
        set_queue_attributes(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/q",
                "Attributes": {
                    "KmsMasterKeyId": "alias/aws/sqs",
                    "KmsDataKeyReusePeriodSeconds": "300",
                    "SqsManagedSseEnabled": "true",
                },
            }),
            &ctx(),
        )
        .unwrap();
        let resp = get_queue_attributes(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/q",
                "AttributeNames": ["All"],
            }),
            &ctx(),
        )
        .unwrap();
        let attrs = resp["Attributes"].as_object().unwrap();
        assert_eq!(attrs["KmsMasterKeyId"], json!("alias/aws/sqs"));
        assert_eq!(attrs["KmsDataKeyReusePeriodSeconds"], json!("300"));
        assert_eq!(attrs["SqsManagedSseEnabled"], json!("true"));
    }

    #[test]
    fn set_queue_attributes_rejects_fifo_queue_mutation() {
        let state = standard_queue();
        let err = set_queue_attributes(
            &state,
            &serde_json::json!({
                "QueueUrl": "http://localhost/queue/q",
                "Attributes": {"FifoQueue": "true"}
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidAttributeName");
    }

    #[test]
    fn set_queue_attributes_rejects_unknown_attribute() {
        let state = standard_queue();
        let err = set_queue_attributes(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/q",
                "Attributes": { "MadeUpAttribute": "x" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidAttributeName");
    }
}
