use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{Queue, SqsState};

pub fn handle(state: &SqsState, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
    let queue_name = input["QueueName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueName is required"))?;

    // Validate queue name
    validate_queue_name(queue_name)?;

    let is_fifo = queue_name.ends_with(".fifo");

    // Collect user-supplied attributes
    let mut attributes: HashMap<String, String> = HashMap::new();
    if let Some(attrs) = input["Attributes"].as_object() {
        for (k, v) in attrs {
            if let Some(s) = v.as_str() {
                attributes.insert(k.clone(), s.to_string());
            }
        }
    }

    // If FifoQueue attribute supplied, validate consistency
    if let Some(fifo_attr) = attributes.get("FifoQueue") {
        let attr_says_fifo = fifo_attr == "true";
        if attr_says_fifo != is_fifo {
            return Err(AwsError::bad_request(
                "InvalidAttributeValue",
                "The FifoQueue attribute must be consistent with the .fifo suffix on the queue name",
            ));
        }
    }

    let url = format!(
        "http://sqs.{}.localhost:4566/{}/{}",
        ctx.region, ctx.account_id, queue_name
    );
    let arn = format!(
        "arn:aws:sqs:{}:{}:{}",
        ctx.region, ctx.account_id, queue_name
    );
    let created_at = chrono_now_epoch();

    // Check if the queue already exists.
    //
    // AWS makes CreateQueue idempotent only when the supplied attributes
    // match what is already stored: re-issuing the call with the same
    // queue name and identical attributes returns the existing URL,
    // while any conflicting attribute raises QueueAlreadyExists (the
    // wire-level Smithy code, not "QueueNameExists" — that's the
    // structure name).
    if let Some(existing) = state.queues.get(queue_name) {
        for (key, requested) in &attributes {
            // Compare only against the values the caller supplied;
            // attributes the caller omitted are not part of the contract.
            // Read-only counters (ApproximateNumberOfMessages…) shouldn't
            // appear in caller input, so a strict equality check is fine.
            let stored = existing.attributes.get(key).cloned().unwrap_or_default();
            if stored.as_str() != requested.as_str() {
                return Err(AwsError::bad_request(
                    "QueueAlreadyExists",
                    format!(
                        "A queue already exists with the same name and a different value for attribute {key}"
                    ),
                ));
            }
        }
        info!(queue = %queue_name, "Queue already exists with matching attributes, returning existing URL");
        let url = existing.url.clone();
        return Ok(json!({ "QueueUrl": url }));
    }

    let queue = Queue::new(
        queue_name.to_string(),
        url.clone(),
        arn,
        is_fifo,
        created_at,
        attributes,
    );

    // Collect tags
    let mut queue = queue;
    if let Some(tags) = input["tags"].as_object() {
        for (k, v) in tags {
            if let Some(s) = v.as_str() {
                queue.tags.insert(k.clone(), s.to_string());
            }
        }
    }

    info!(queue = %queue_name, is_fifo, "Created queue");
    state.queues.insert(queue_name.to_string(), queue);

    Ok(json!({ "QueueUrl": url }))
}

fn validate_queue_name(name: &str) -> Result<(), AwsError> {
    if name.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            "Queue name must not be empty",
        ));
    }
    if name.len() > 80 {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            "Queue name must be at most 80 characters",
        ));
    }
    let base = name.strip_suffix(".fifo").unwrap_or(name);
    if !base
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            "Queue name must contain only alphanumeric characters, hyphens, or underscores",
        ));
    }
    Ok(())
}

fn chrono_now_epoch() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("sqs", "us-east-1")
    }

    #[test]
    fn create_queue_idempotent_with_matching_attributes() {
        let state = SqsState::default();
        let r1 = handle(
            &state,
            &json!({
                "QueueName": "q1",
                "Attributes": { "DelaySeconds": "30" },
            }),
            &ctx(),
        )
        .unwrap();
        let r2 = handle(
            &state,
            &json!({
                "QueueName": "q1",
                "Attributes": { "DelaySeconds": "30" },
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(r1["QueueUrl"], r2["QueueUrl"]);
    }

    #[test]
    fn create_queue_omitted_attributes_remain_idempotent() {
        // Caller doesn't re-supply the attributes — must still succeed.
        let state = SqsState::default();
        handle(
            &state,
            &json!({
                "QueueName": "q1",
                "Attributes": { "DelaySeconds": "30" },
            }),
            &ctx(),
        )
        .unwrap();
        handle(&state, &json!({ "QueueName": "q1" }), &ctx()).unwrap();
    }

    #[test]
    fn create_queue_rejects_conflicting_attribute() {
        let state = SqsState::default();
        handle(
            &state,
            &json!({
                "QueueName": "q1",
                "Attributes": { "DelaySeconds": "30" },
            }),
            &ctx(),
        )
        .unwrap();
        let err = handle(
            &state,
            &json!({
                "QueueName": "q1",
                "Attributes": { "DelaySeconds": "60" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "QueueAlreadyExists");
        assert!(err.message.contains("DelaySeconds"));
    }
}
