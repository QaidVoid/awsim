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

    // Check if the queue already exists
    if let Some(existing) = state.queues.get(queue_name) {
        // AWS SQS returns the URL for an existing queue if attributes match,
        // or an error if they conflict. For simplicity we return URL if name matches.
        info!(queue = %queue_name, "Queue already exists, returning existing URL");
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
    let base = if name.ends_with(".fifo") {
        &name[..name.len() - 5]
    } else {
        name
    };
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
